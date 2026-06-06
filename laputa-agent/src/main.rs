mod action_graph;
mod types;
mod parser;
mod bracket_parser;
mod forge;
mod gate;
mod governor;
mod operator;
mod memory;
mod chronos;
mod envelope;
mod inference;
mod vm;

use std::process::Command;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::{accept_async, tungstenite::Message};
use futures_util::{SinkExt, StreamExt};

use types::{PipelineError, Step, ToolCall};
use forge::Forge;
use operator::StepEnforcer;
use memory::Memory;
use inference::llama_cpp::LlamaCppAdapter;
use inference::InferenceAdapter;

const SYSTEM_PROMPT: &str = r#"You are an assistant that operates the user's Linux desktop on their behalf. You read the screen and act by emitting one tool call at a time.
Your perception tool outputs the active window's interactive elements in this format:
[focused: Terminal - laputa@archlinux:~]
[window: x=0 y=51 w=1280 h=749]
  ref_1  toggle button   "Applications"     state=has-tooltip
  ref_3  toggle button   "Xfce Terminal"    state=has-tooltip
  ref_5  entry           "Search"           state=editable
TOOLS:
  click(selector="<ref_id>")
  type(selector="<ref_id>", text="<string>")
  key(key="<key>")
  wait(ms=<int>)
  done(reason="<short reason>")
EXAMPLES:
  click(selector="ref_3")
  type(selector="ref_5", text="hello")
Respond with ONLY one bracket tool call. No markdown, no explanation."#;

const MODEL_PATH: &str = "/home/d/.laputa-secure/models/LFM2.5-8B-A1B-Q4_K_M.gguf";
const CONTEXT_SIZE: usize = 32768;
const LLAMA_SERVER_BIN: &str =
    "/home/d/laputa/laputa-agent/vendored/llama.cpp-2/build/bin/llama-server";

// ── State shared between WebSocket and agent ──────────────────────
struct AgentState {
    goal: String,
    running: bool,
    approval_tx: Option<mpsc::Sender<bool>>,
    pending_id: Option<String>,
}

// ── Screen reading ────────────────────────────────────────────────
async fn get_screen_state() -> String {
    let output = Command::new("python3")
        .args(["/home/d/laputa/perceive.py", "--focused"])
        .output()
        .ok();
    match output {
        Some(out) => String::from_utf8_lossy(&out.stdout).to_string(),
        None => "Screen state unavailable".to_string(),
    }
}

// ── Tool execution ────────────────────────────────────────────────
async fn execute_tool(call: &ToolCall) -> String {
    match call {
        ToolCall::Click { selector } => {
            let _ = Command::new("python3")
                .args(["-m", "tine.cli", "click", selector])
                .current_dir("/home/d/laputa/tine")
                .output();
            format!("Clicked {}", selector)
        }
        ToolCall::Type { selector, text } => {
            if selector != "body" {
                let _ = Command::new("python3")
                    .args(["-m", "tine.cli", "focus", selector])
                    .current_dir("/home/d/laputa/tine")
                    .output();
            }
            let _ = Command::new("python3")
                .args(["-m", "tine.cli", "type", text])
                .current_dir("/home/d/laputa/tine")
                .output();
            format!("Typed '{}' in {}", text, selector)
        }
        ToolCall::Key { key } => {
            let _ = Command::new("python3")
                .args(["-m", "tine.cli", "key", key])
                .current_dir("/home/d/laputa/tine")
                .output();
            format!("Pressed {}", key)
        }
        ToolCall::Wait { ms } => {
            tokio::time::sleep(tokio::time::Duration::from_millis(*ms as u64)).await;
            format!("Waited {}ms", ms)
        }
        ToolCall::Task { description } => {
            format!("Task completed: {}", description)
        }
        ToolCall::Done { reason } => {
            format!("Done: {}", reason)
        }
    }
}

// ── Agent loop ────────────────────────────────────────────────────
async fn agent_loop(
    state: Arc<Mutex<AgentState>>,
    adapter: Arc<dyn InferenceAdapter>,
    mut approval_rx: mpsc::Receiver<bool>,
    confirm_tx: mpsc::Sender<String>,
) {
    let mut enforcer = StepEnforcer::new();
    let mut memory = Memory::new(|steps| {
        let actions: Vec<_> = steps.iter()
            .filter_map(|s| s.action.as_ref())
            .map(|a| format!("{:?}", a))
            .collect();
        actions.join(", ")
    });

    let goal = state.lock().await.goal.clone();
    chronos::log(&format!("goal_received: {goal}"));
    let _ = confirm_tx.send(envelope::make("status", envelope::StatusPayload {
        state: "goal_received".to_string(),
        detail: goal.clone(),
    })).await;

    loop {
        {
            let s = state.lock().await;
            if !s.running { break; }
        } // guard dropped here — safe to await below

        if let Err(e) = enforcer.advance() {
            println!("Agent terminated: {:?}", e);
            break;
        }

        let screen = get_screen_state().await;
        let context = memory.context_string();
        let prompt = format!(
            "{}\n\n{context}\n\nScreen:\n{screen}\n\nGoal: {goal}\n\nWhat is your next action?",
            SYSTEM_PROMPT
        );

        let adapter_clone = adapter.clone();
        let forge = Forge {
            model_fn: Box::new(move |p: String| {
                let adapter = adapter_clone.clone();
                Box::pin(async move {
                    tokio::task::spawn_blocking(move || {
                        adapter.generate(&p, 2048, 0.2)
                            .map_err(|e| PipelineError::ModelError(e))
                    })
                    .await
                    .map_err(|e| PipelineError::ModelError(e.to_string()))?
                })
            }),
        };

        match forge.call_with_retry(&prompt, &enforcer).await {
            Ok(tool_call) => {
                println!("Step {}: {:?}", enforcer.step(), tool_call);

                // state mutex is NOT held from here through approval_rx.recv()
                let output = match gate::evaluate_action(&tool_call) {
                    gate::Verdict::Allow => {
                        let desc = gate::describe(&tool_call);
                        let out = execute_tool(&tool_call).await;
                        chronos::log(&format!("action: {desc} -> {out}"));
                        let _ = confirm_tx.send(envelope::make("action_log", envelope::ActionLogPayload {
                            text: format!("{desc} -> {out}"),
                        })).await;
                        out
                    }
                    gate::Verdict::ConfirmTap => {
                        let desc = gate::describe(&tool_call);
                        let tool_name = match &tool_call {
                            ToolCall::Click { .. } => "click",
                            ToolCall::Type  { .. } => "type",
                            ToolCall::Key   { .. } => "key",
                            _                      => "unknown",
                        };
                        let id = uuid::Uuid::new_v4().to_string();
                        chronos::log(&format!("confirm_requested: tap: {desc}"));
                        let _ = confirm_tx.send(envelope::make("permission", envelope::PermissionPayload {
                            id: id.clone(),
                            type_: "tap".to_string(),
                            tool: tool_name.to_string(),
                            action: desc.clone(),
                            reason: "Write action requires confirmation".to_string(),
                            origin_surface: "immersive".to_string(),
                            origin_agent: "main".to_string(),
                        })).await;
                        // Set pending_id (lock, set, drop guard) before awaiting approval
                        { state.lock().await.pending_id = Some(id); }
                        let approved = approval_rx.recv().await.unwrap_or(false);
                        if approved {
                            let out = execute_tool(&tool_call).await;
                            chronos::log(&format!("action: {desc} -> {out}"));
                            let _ = confirm_tx.send(envelope::make("action_log", envelope::ActionLogPayload {
                                text: format!("{desc} -> {out}"),
                            })).await;
                            out
                        } else {
                            chronos::log(&format!("denied: {desc}"));
                            let _ = confirm_tx.send(envelope::make("status", envelope::StatusPayload {
                                state: "denied".to_string(),
                                detail: desc.clone(),
                            })).await;
                            format!("Denied by user: {:?}", tool_call)
                        }
                    }
                    gate::Verdict::ConfirmTyped => {
                        let desc = gate::describe(&tool_call);
                        let tool_name = match &tool_call {
                            ToolCall::Click { .. } => "click",
                            ToolCall::Type  { .. } => "type",
                            ToolCall::Key   { .. } => "key",
                            _                      => "unknown",
                        };
                        let id = uuid::Uuid::new_v4().to_string();
                        chronos::log(&format!("confirm_requested: typed: {desc}"));
                        let _ = confirm_tx.send(envelope::make("permission", envelope::PermissionPayload {
                            id: id.clone(),
                            type_: "typed".to_string(),
                            tool: tool_name.to_string(),
                            action: desc.clone(),
                            reason: "Write action requires confirmation".to_string(),
                            origin_surface: "immersive".to_string(),
                            origin_agent: "main".to_string(),
                        })).await;
                        // Set pending_id (lock, set, drop guard) before awaiting approval
                        { state.lock().await.pending_id = Some(id); }
                        let approved = approval_rx.recv().await.unwrap_or(false);
                        if approved {
                            let out = execute_tool(&tool_call).await;
                            chronos::log(&format!("action: {desc} -> {out}"));
                            let _ = confirm_tx.send(envelope::make("action_log", envelope::ActionLogPayload {
                                text: format!("{desc} -> {out}"),
                            })).await;
                            out
                        } else {
                            chronos::log(&format!("denied: {desc}"));
                            let _ = confirm_tx.send(envelope::make("status", envelope::StatusPayload {
                                state: "denied".to_string(),
                                detail: desc.clone(),
                            })).await;
                            format!("Denied by user: {:?}", tool_call)
                        }
                    }
                    gate::Verdict::Block(reason) => {
                        chronos::log(&format!("blocked: {reason}"));
                        let _ = confirm_tx.send(envelope::make("status", envelope::StatusPayload {
                            state: "blocked".to_string(),
                            detail: reason.clone(),
                        })).await;
                        println!("Action blocked: {}", reason);
                        format!("Blocked: {}", reason)
                    }
                };

                memory.push(Step {
                    index: enforcer.step(),
                    prompt: prompt.clone(),
                    output,
                    action: Some(tool_call.clone()),
                });
                if matches!(tool_call, ToolCall::Task { .. } | ToolCall::Done { .. }) {
                    let reason = match &tool_call {
                        ToolCall::Done { reason } => reason.clone(),
                        ToolCall::Task { description } => description.clone(),
                        _ => "completed".to_string(),
                    };
                    chronos::log(&format!("goal_done: {reason}"));
                    let _ = confirm_tx.send(envelope::make("status", envelope::StatusPayload {
                        state: "goal_done".to_string(),
                        detail: reason,
                    })).await;
                    println!("Goal achieved.");
                    break;
                }
            }
            Err(e) => {
                println!("Pipeline error: {:?}", e);
                memory.push(Step {
                    index: enforcer.step(),
                    prompt: prompt.clone(),
                    output: format!("Error: {:?}", e),
                    action: None,
                });
                if matches!(e, PipelineError::MaxRetriesExceeded | PipelineError::MaxStepsExceeded) {
                    let detail = format!("{:?}", e);
                    chronos::log(&format!("goal_aborted: {detail}"));
                    let _ = confirm_tx.send(envelope::make("status", envelope::StatusPayload {
                        state: "goal_aborted".to_string(),
                        detail,
                    })).await;
                    break;
                }
            }
        }
    }
}

// ── WebSocket server ──────────────────────────────────────────────
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    // ── Governor: detect hardware, plan server config ─────────────
    let cfg = governor::detect_and_plan(CONTEXT_SIZE);
    chronos::log(&format!(
        "server_config: gpu={} ctx={} ngl={} threads={} parallel={}",
        cfg.n_gpu_layers > 0, cfg.ctx, cfg.n_gpu_layers, cfg.threads, cfg.n_parallel
    ));

    // Check if llama-server is already up before spawning
    let already_up = tokio::task::spawn_blocking(|| {
        ureq::get("http://127.0.0.1:8080/health").call().is_ok()
    })
    .await
    .unwrap_or(false);

    // Keep child alive for the duration of the program
    let _server_child: Option<std::process::Child> = if already_up {
        chronos::log("server_config: reusing existing server");
        println!("llama-server already running on :8080 — reusing.");
        None
    } else {
        let mut args = vec![
            "-m".to_string(), MODEL_PATH.to_string(),
            "-c".to_string(), cfg.ctx.to_string(),
            "-ngl".to_string(), cfg.n_gpu_layers.to_string(),
            "-t".to_string(), cfg.threads.to_string(),
            "--parallel".to_string(), cfg.n_parallel.to_string(),
            "--host".to_string(), "127.0.0.1".to_string(),
            "--port".to_string(), "8080".to_string(),
        ];
        if cfg.flash_attn {
            args.push("-fa".to_string());
            args.push("on".to_string());
        }
        println!("Spawning: {} {}", LLAMA_SERVER_BIN, args.join(" "));

        let mut cmd = Command::new(LLAMA_SERVER_BIN);
        cmd.args(&args);
        match cmd.spawn() {
            Ok(child) => {
                let ready = tokio::task::spawn_blocking(|| {
                    let agent = ureq::AgentBuilder::new()
                        .timeout(std::time::Duration::from_secs(2))
                        .build();
                    for _ in 0..60 {
                        if agent.get("http://127.0.0.1:8080/health").call().is_ok() {
                            return true;
                        }
                        std::thread::sleep(std::time::Duration::from_secs(1));
                    }
                    false
                })
                .await
                .unwrap_or(false);

                if !ready {
                    eprintln!("llama-server did not become ready within 60s — exiting.");
                    std::process::exit(1);
                }
                println!("llama-server ready on :8080");
                Some(child)
            }
            Err(e) => {
                eprintln!("Failed to spawn llama-server: {e}");
                std::process::exit(1);
            }
        }
    };

    // ── Inference adapter ─────────────────────────────────────────
    let adapter: Arc<dyn InferenceAdapter> = Arc::new(
        LlamaCppAdapter::new(MODEL_PATH, CONTEXT_SIZE)
            .expect("Failed to construct LlamaCppAdapter")
    );

    let state = Arc::new(Mutex::new(AgentState {
        goal: String::new(),
        running: false,
        approval_tx: None,
        pending_id: None,
    }));

    let state_ws = state.clone();
    let adapter_ws = adapter.clone();

    tokio::spawn(async move {
        let listener = TcpListener::bind("127.0.0.1:9090").await.unwrap();
        println!("WebSocket server on ws://127.0.0.1:9090");
        while let Ok((stream, _)) = listener.accept().await {
            let ws = accept_async(stream).await.unwrap();
            let (mut ws_sender, mut receiver) = ws.split();
            let state = state_ws.clone();
            let adapter = adapter_ws.clone();

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
                                            tokio::spawn(async move {
                                                agent_loop(state_clone, adapter_clone, approval_rx, confirm_tx).await;
                                            });
                                        }
                                    }
                                    "command" => {
                                        if let Ok(p) = serde_json::from_value::<envelope::CommandPayload>(env.payload) {
                                            match p.cmd.as_str() {
                                                "pause"  => state.lock().await.running = false,
                                                "resume" => state.lock().await.running = true,
                                                "stop"   => state.lock().await.running = false,
                                                other    => println!("unknown command: {other}"),
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
                                                    println!("stale approval id ignored: {}", p.id);
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
                                    other => println!("unknown envelope kind: {other}"),
                                },
                                _ => println!("envelope parse failed or wrong version: {text}"),
                            }
                        }
                    }
                }
                // Connection closed — drop approval_tx so agent_loop unblocks
                state.lock().await.approval_tx = None;
            });
        }
    });

    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
}
