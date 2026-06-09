#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tauri::{State, Emitter};
use lagado_agent::{
    agent::{AgentState, agent_loop},
    bootstrap::ensure_llama_server,
    config,
    inference::{InferenceAdapter, llama_cpp::LlamaCppAdapter},
    perception::{MockPerceptor, MockActuator, Perceptor, Actuator},
};

struct AppState {
    agent: Arc<Mutex<AgentState>>,
    adapter: Arc<dyn InferenceAdapter + Send + Sync>,
    perceptor: Arc<dyn Perceptor + Send + Sync>,
    actuator: Arc<dyn Actuator + Send + Sync>,
    _llama_child: Arc<Mutex<Option<std::process::Child>>>,
}

#[tauri::command]
async fn send_goal(
    goal: String,
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let (approval_tx, approval_rx) = mpsc::channel::<bool>(1);
    let (confirm_tx, mut confirm_rx) = mpsc::channel::<String>(32);

    // Bridge: forward serialised envelope JSON → Tauri events
    let app_h = app.clone();
    tokio::spawn(async move {
        while let Some(msg) = confirm_rx.recv().await {
            if let Ok(env) = serde_json::from_str::<serde_json::Value>(&msg) {
                let kind = env["kind"].as_str().unwrap_or("unknown").to_string();
                let _ = app_h.emit(&kind, env["payload"].clone());
            }
        }
    });

    // Update agent state — drop guard before spawn
    let agent_arc = state.agent.clone();
    {
        let mut s = agent_arc.lock().await;
        s.approval_tx = Some(approval_tx);
        s.pending_id = None;
        s.goal = goal;
        s.running = true;
    } // guard dropped here

    let agent_clone = agent_arc.clone();
    let adapter = state.adapter.clone();
    let perceptor = state.perceptor.clone();
    let actuator = state.actuator.clone();

    tokio::spawn(async move {
        agent_loop(agent_clone, adapter, perceptor, actuator, approval_rx, confirm_tx).await;
    });

    Ok(())
}

#[tauri::command]
async fn send_command(
    cmd: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    match cmd.as_str() {
        "pause" | "stop" => state.agent.lock().await.running = false,
        "resume"         => state.agent.lock().await.running = true,
        other            => tracing::warn!("unknown command: {other}"),
    }
    Ok(())
}

#[tauri::command]
async fn send_approval(
    id: String,
    approved: bool,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let (matched, tx) = {
        let mut s = state.agent.lock().await;
        if s.pending_id.as_deref() == Some(id.as_str()) {
            let tx = s.approval_tx.clone();
            s.pending_id = None;
            (true, tx)
        } else {
            tracing::warn!("stale approval id ignored: {id}");
            (false, None)
        }
    }; // guard dropped before await
    if matched {
        if let Some(tx) = tx {
            let _ = tx.send(approved).await;
        }
    }
    Ok(())
}

fn main() {
    tracing_subscriber::fmt::init();

    let model_path = config::model_path();
    let adapter: Arc<dyn InferenceAdapter + Send + Sync> =
        match LlamaCppAdapter::new(&model_path.to_string_lossy(), config::CONTEXT_SIZE) {
            Ok(a)  => Arc::new(a),
            Err(e) => { eprintln!("inference adapter init failed: {e}"); std::process::exit(1); }
        };

    let llama_child: Arc<Mutex<Option<std::process::Child>>> = Arc::new(Mutex::new(None));
    let llama_for_setup = llama_child.clone();

    let state = Arc::new(AppState {
        agent: Arc::new(Mutex::new(AgentState {
            goal:        String::new(),
            running:     false,
            approval_tx: None,
            pending_id:  None,
        })),
        adapter,
        perceptor: Arc::new(MockPerceptor),
        actuator:  Arc::new(MockActuator),
        _llama_child: llama_child,
    });

    tauri::Builder::default()
        .setup(move |_app| {
            tauri::async_runtime::spawn(async move {
                let child = ensure_llama_server().await;
                *llama_for_setup.lock().await = child;
            });
            Ok(())
        })
        .manage(state)
        .invoke_handler(tauri::generate_handler![send_goal, send_command, send_approval])
        .run(tauri::generate_context!())
        .expect("Lagado failed to start");
}
