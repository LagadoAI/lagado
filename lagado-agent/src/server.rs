use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::{accept_async, tungstenite::Message};
use futures_util::{SinkExt, StreamExt};
use crate::agent::{AgentState, agent_loop};
use crate::config;
use crate::envelope;
use crate::inference::InferenceAdapter;
use crate::perception::{Perceptor, Actuator};
use crate::skill_library::SkillLibrary;
use crate::tools::ToolRegistry;

pub(crate) async fn run_ws_server(
    state: Arc<Mutex<AgentState>>,
    adapter: Arc<dyn InferenceAdapter>,
    perceptor: Arc<dyn Perceptor>,
    actuator: Arc<dyn Actuator>,
    memory_tiers: Arc<tokio::sync::Mutex<crate::memory_tiers::MemoryTiers>>,
    visual_encoder: Option<Arc<crate::vision::VisualEncoder>>,
    registry: Arc<ToolRegistry>,
    skill_library: Arc<SkillLibrary>,
) {
    let addr = config::ws_addr();
    let listener = match TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("failed to bind WebSocket server to {addr}: {e}");
            return;
        }
    };
    tracing::info!("WebSocket server on ws://{addr}");
    loop {
        let (stream, _peer) = match listener.accept().await {
            Ok(s) => s,
            Err(e) => { tracing::warn!("accept error: {e}"); continue; }
        };
        let ws = match accept_async(stream).await {
            Ok(ws) => ws,
            Err(e) => { tracing::warn!("websocket handshake failed: {e}"); continue; }
        };
        let (mut ws_sender, mut receiver) = ws.split();
        let state = state.clone();
        let adapter = adapter.clone();
        let perceptor = perceptor.clone();
        let actuator = actuator.clone();

        // Per-connection outbound channel: agent_loop → ws_sender
        let (outbound_tx, mut outbound_rx) = mpsc::channel::<String>(4);
        tokio::spawn(async move {
            while let Some(msg) = outbound_rx.recv().await {
                if ws_sender.send(Message::Text(msg)).await.is_err() {
                    break;
                }
            }
        });

        tokio::spawn(async move {
            while let Some(msg) = receiver.next().await {
                if let Ok(msg) = msg {
                    if let Ok(text) = msg.to_text() {
                        match envelope::parse(text) {
                            Some(env) if env.v == 1 => match env.kind.as_str() {
                                "goal" => {
                                    if let Ok(p) = serde_json::from_value::<envelope::GoalPayload>(env.payload) {
                                        let (approval_tx, approval_rx) = mpsc::channel::<bool>(1);
                                        let confirm_tx = outbound_tx.clone();
                                        {
                                            let mut s = state.lock().await;
                                            s.approval_tx = Some(approval_tx);
                                            s.pending_id = None;
                                            s.goal = p.text.clone();
                                            s.running = true;
                                        } // guard dropped before spawn
                                        let state_clone = state.clone();
                                        let adapter_clone = adapter.clone();
                                        let perceptor_clone = perceptor.clone();
                                        let actuator_clone = actuator.clone();
                                        let mt_clone = memory_tiers.clone();
                                        let ve_clone = visual_encoder.clone();
                                        let reg_clone = registry.clone();
                                        let sl_clone = skill_library.clone();
                                        tokio::spawn(async move {
                                            agent_loop(state_clone, adapter_clone, perceptor_clone, actuator_clone, approval_rx, confirm_tx, mt_clone, ve_clone, reg_clone, sl_clone).await;
                                        });
                                    }
                                }
                                "command" => {
                                    if let Ok(p) = serde_json::from_value::<envelope::CommandPayload>(env.payload) {
                                        match p.cmd.as_str() {
                                            "pause"  => state.lock().await.running = false,
                                            "resume" => state.lock().await.running = true,
                                            "stop"   => state.lock().await.running = false,
                                            other    => tracing::warn!("unknown command: {other}"),
                                        }
                                    }
                                }
                                "approval" => {
                                    if let Ok(p) = serde_json::from_value::<envelope::ApprovalPayload>(env.payload) {
                                        let (matched, tx) = {
                                            let mut s = state.lock().await;
                                            if s.pending_id.as_deref() == Some(p.id.as_str()) {
                                                let tx = s.approval_tx.clone();
                                                s.pending_id = None;
                                                (true, tx)
                                            } else {
                                                tracing::warn!("stale approval id ignored: {}", p.id);
                                                (false, None)
                                            }
                                        }; // guard dropped before await
                                        if matched {
                                            if let Some(tx) = tx {
                                                let _ = tx.send(p.approved).await;
                                            }
                                        }
                                    }
                                }
                                other => tracing::warn!("unknown envelope kind: {other}"),
                            },
                            _ => tracing::warn!("envelope parse failed or wrong version: {text}"),
                        }
                    }
                }
            }
            // Connection closed — drop approval_tx so agent_loop unblocks
            state.lock().await.approval_tx = None;
        });
    }
}
