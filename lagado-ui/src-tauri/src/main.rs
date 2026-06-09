#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tauri::{State, Emitter};
use lagado_agent::{
    agent::AgentState,
    bootstrap::ensure_llama_server,
    config,
    hydra,
    inference::{InferenceAdapter, llama_cpp::LlamaCppAdapter},
    perception::{Perceptor, Actuator},
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

    // Check if paused and set up state — drop guard before await
    let is_paused = {
        let mut s = state.agent.lock().await;
        s.approval_tx = Some(approval_tx);
        s.pending_id = None;
        s.running = true;
        false // paused state will be passed in from frontend in future; false for now
    }; // guard dropped

    let agent_arc = state.agent.clone();
    let adapter = state.adapter.clone();
    let perceptor = state.perceptor.clone();
    let actuator = state.actuator.clone();

    tokio::spawn(async move {
        hydra::run(
            goal,
            String::new(),
            is_paused,
            agent_arc,
            adapter,
            perceptor,
            actuator,
            approval_rx,
            confirm_tx,
        )
        .await;
    });

    Ok(())
}

#[tauri::command]
async fn send_chat(
    message: String,
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let adapter = state.adapter.clone();
    let app_h = app.clone();
    tokio::spawn(async move {
        let hydra = hydra::Hydra::from_governor(adapter);
        let response = hydra.chat_response(&message, "").await;
        let _ = app_h.emit("action_log", serde_json::json!({ "text": response }));
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

#[tauri::command]
async fn initialize_timeline() -> Result<(), String> {
    lagado_agent::chronos::initialize_timeline("default");
    Ok(())
}

#[tauri::command]
fn get_active_model() -> String {
    lagado_agent::config::active_model()
}

#[tauri::command]
fn set_active_model(filename: String) -> Result<(), String> {
    lagado_agent::config::set_active_model(&filename)
}

#[tauri::command]
fn list_models() -> Vec<String> {
    lagado_agent::config::available_models()
}

#[tauri::command]
fn get_chronos_recent(n: usize) -> Vec<serde_json::Value> {
    match lagado_agent::chronos::ChronosDb::open() {
        Ok(db) => db.recent(n).into_iter().map(|s| serde_json::json!({
            "timestamp": s.timestamp,
            "active_goal": s.active_goal,
            "last_action": s.last_action,
            "confidence": s.confidence,
        })).collect(),
        Err(_) => vec![],
    }
}

#[tauri::command]
async fn terminal_spawn(session_id: String, shell: String) -> Result<(), String> {
    let shell_path = if shell.is_empty() {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string())
    } else { shell };
    if std::path::Path::new(&shell_path).exists() {
        tracing::info!("Terminal session {session_id} spawned with {shell_path}");
        Ok(())
    } else {
        Err(format!("Shell not found: {shell_path}"))
    }
}

#[tauri::command]
async fn terminal_run(command: String) -> Result<String, String> {
    let output = std::process::Command::new("bash")
        .arg("-c")
        .arg(&command)
        .current_dir(std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".to_string())))
        .output()
        .map_err(|e| format!("failed to run command: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    Ok(if stderr.is_empty() { stdout } else { format!("{stdout}{stderr}") })
}

#[tauri::command]
fn terminal_get_cwd() -> String {
    std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "~".to_string())
}

#[tauri::command]
fn vault_list_files(subfolder: String) -> Vec<serde_json::Value> {
    let base = std::path::PathBuf::from(
        std::env::var("LAGADO_DATA_DIR")
            .unwrap_or_else(|_| format!("{}/.laputa-secure", std::env::var("HOME").unwrap_or_default()))
    );
    let dir = if subfolder.is_empty() { base.clone() } else { base.join(&subfolder) };

    std::fs::read_dir(&dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let meta = e.metadata().ok()?;
            let size = if meta.is_file() {
                let bytes = meta.len();
                if bytes < 1024 { format!("{bytes} B") }
                else if bytes < 1_048_576 { format!("{:.1} KB", bytes as f64 / 1024.0) }
                else { format!("{:.1} MB", bytes as f64 / 1_048_576.0) }
            } else { "—".to_string() };
            Some(serde_json::json!({
                "name": name,
                "is_dir": meta.is_dir(),
                "size": size,
            }))
        })
        .collect()
}

#[tauri::command]
async fn get_server_status() -> serde_json::Value {
    // Use reqwest (already a dep via lagado-agent) for the health check
    let healthy = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        async { reqwest::get("http://127.0.0.1:8080/health").await.is_ok() }
    ).await.unwrap_or(false);

    let model = lagado_agent::config::active_model();
    serde_json::json!({
        "running": healthy,
        "model": model,
        "host": "127.0.0.1",
        "port": 8080,
        "endpoint": "http://127.0.0.1:8080",
    })
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

    #[cfg(target_os = "linux")]
    let (perceptor_impl, actuator_impl) = lagado_agent::perception::linux_pair();
    #[cfg(target_os = "linux")]
    let perceptor: Arc<dyn lagado_agent::perception::Perceptor + Send + Sync> =
        Arc::new(perceptor_impl);
    #[cfg(target_os = "linux")]
    let actuator: Arc<dyn lagado_agent::perception::Actuator + Send + Sync> =
        Arc::new(actuator_impl);

    #[cfg(not(target_os = "linux"))]
    let perceptor: Arc<dyn lagado_agent::perception::Perceptor + Send + Sync> =
        Arc::new(lagado_agent::perception::MockPerceptor);
    #[cfg(not(target_os = "linux"))]
    let actuator: Arc<dyn lagado_agent::perception::Actuator + Send + Sync> =
        Arc::new(lagado_agent::perception::MockActuator);

    let state = Arc::new(AppState {
        agent: Arc::new(Mutex::new(AgentState {
            goal:        String::new(),
            running:     false,
            approval_tx: None,
            pending_id:  None,
        })),
        adapter,
        perceptor,
        actuator,
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
        .invoke_handler(tauri::generate_handler![send_goal, send_chat, send_command, send_approval, initialize_timeline, get_active_model, set_active_model, list_models, get_chronos_recent, terminal_spawn, terminal_run, terminal_get_cwd, vault_list_files, get_server_status])
        .run(tauri::generate_context!())
        .expect("Lagado failed to start");
}
