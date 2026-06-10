#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tauri::{State, Emitter};
use lagado_agent::{
    agent::AgentState,
    bootstrap::{ensure_llama_server, ensure_classifier_server},
    config,
    hydra,
    inference::{InferenceAdapter, llama_cpp::LlamaCppAdapter},
    memory_tiers::MemoryTiers,
    perception::{Perceptor, Actuator, PerceptionCache},
    sleep_gate::SleepGate,
    vm::{QemuDesktopBackend, VmHandle, VmBackend, VmSshPort, DynamicActuator, DynamicPerceptor},
};
#[cfg(target_os = "linux")]
use lagado_agent::vision::VisualEncoder;

/// Kills and reaps the child process on drop — prevents server orphans on app exit.
struct KillOnDrop(std::process::Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct AppState {
    agent: Arc<Mutex<AgentState>>,
    adapter: Arc<dyn InferenceAdapter + Send + Sync>,
    perceptor: Arc<dyn Perceptor + Send + Sync>,
    actuator: Arc<dyn Actuator + Send + Sync>,
    _llama_child: Arc<Mutex<Option<KillOnDrop>>>,
    _classifier_child: Arc<Mutex<Option<KillOnDrop>>>,
    #[cfg(target_os = "linux")]
    visual_encoder: Option<Arc<VisualEncoder>>,
    vm: Arc<Mutex<Option<VmHandle>>>,
    vm_ssh_port: VmSshPort,
    vm_backend: QemuDesktopBackend,
    session_dek: Arc<Mutex<Option<Vec<u8>>>>,
    ssh_cache: Arc<std::sync::Mutex<PerceptionCache>>,
    memory_tiers: Arc<Mutex<MemoryTiers>>,
}

#[tauri::command]
async fn send_goal(
    goal: String,
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let (approval_tx, approval_rx) = mpsc::channel::<bool>(1);
    let (confirm_tx, mut confirm_rx) = mpsc::channel::<String>(32);

    let app_h = app.clone();
    tokio::spawn(async move {
        while let Some(msg) = confirm_rx.recv().await {
            if let Ok(env) = serde_json::from_str::<serde_json::Value>(&msg) {
                let kind = env["kind"].as_str().unwrap_or("unknown").to_string();
                let _ = app_h.emit(&kind, env["payload"].clone());
            }
        }
    });

    let is_paused = {
        let mut s = state.agent.lock().await;
        s.approval_tx = Some(approval_tx);
        s.pending_id = None;
        s.running = true;
        false
    };

    let agent_arc = state.agent.clone();
    let adapter = state.adapter.clone();
    let perceptor = state.perceptor.clone();
    let actuator = state.actuator.clone();
    let memory_tiers = state.memory_tiers.clone();
    #[cfg(target_os = "linux")]
    let visual_encoder = state.visual_encoder.clone();

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
            memory_tiers,
            #[cfg(target_os = "linux")] visual_encoder,
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
    };
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
async fn terminal_run(command: String, state: State<'_, Arc<AppState>>) -> Result<String, String> {
    if state.session_dek.lock().await.is_none() {
        return Err("unauthenticated".to_string());
    }
    let output = std::process::Command::new("bash")
        .arg("-c")
        .arg(&command)
        .current_dir(std::path::PathBuf::from(
            std::env::var("HOME").unwrap_or_else(|_| "/".to_string())
        ))
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
            .unwrap_or_else(|_| format!("{}/.laputa-secure",
                std::env::var("HOME").unwrap_or_default()))
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

#[tauri::command]
async fn capture_frame(state: State<'_, Arc<AppState>>, source: Option<String>) -> Result<String, String> {
    use std::sync::Mutex as StdMutex;
    static LAST_HASH_VM: StdMutex<Option<[u8; 32]>> = StdMutex::new(None);
    static LAST_HASH_HOST: StdMutex<Option<[u8; 32]>> = StdMutex::new(None);

    const FRAME_PATH: &str = "/dev/shm/lagado_frame.png";
    let use_host = source.as_deref() == Some("host");

    let bytes = if use_host {
        tokio::task::spawn_blocking(|| {
            let mut cap = lagado_agent::perception::capture::ScreenCapture::new();
            cap.capture()?;
            cap.read_frame().ok_or_else(|| "no frame captured".to_string())
        }).await.map_err(|e| format!("spawn error: {e}"))??
    } else {
        let qmp_socket = {
            let vm = state.vm.lock().await;
            vm.as_ref().map(|h| h.qmp_socket.clone())
        };
        if let Some(socket) = qmp_socket {
            tokio::task::spawn_blocking(move || {
                if let Ok(mut qmp) = lagado_agent::vm::QmpClient::connect(&socket) {
                    let _ = qmp.screendump(FRAME_PATH);
                }
            }).await.ok();
        }
        std::fs::read(FRAME_PATH).map_err(|_| "no frame — VM not ready".to_string())?
    };

    let last_hash = if use_host { &LAST_HASH_HOST } else { &LAST_HASH_VM };
    let hash: [u8; 32] = *blake3::hash(&bytes).as_bytes();
    {
        let mut last = last_hash.lock().unwrap();
        if *last == Some(hash) {
            return Ok("unchanged".to_string());
        }
        *last = Some(hash);
    }

    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:image/png;base64,{b64}"))
}

#[tauri::command]
async fn vm_boot(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, String> {
    let mut vm = state.vm.lock().await;
    // If we have a handle, check if the process is still alive before refusing
    if let Some(ref mut existing) = *vm {
        match existing.child.try_wait() {
            Ok(None) => return Err("VM already running".to_string()),
            _ => { *vm = None; } // process died — clear and boot fresh
        }
    }
    let cfg = lagado_agent::vm::VmConfig::default();
    if !std::path::Path::new(&cfg.disk_image).exists() {
        return Err(format!("Disk image not found: {}", cfg.disk_image));
    }
    let cfg_clone = lagado_agent::vm::VmConfig {
        disk_image: cfg.disk_image.clone(),
        seed_iso: cfg.seed_iso.clone(),
        mem_mib: cfg.mem_mib,
        vcpus: cfg.vcpus,
        ssh_port: cfg.ssh_port,
        qmp_socket: cfg.qmp_socket.clone(),
    };
    let backend = &state.vm_backend;
    let ssh_port = cfg.ssh_port;
    let handle = backend.boot(&cfg_clone)?;
    *vm = Some(handle);
    // Poll for SSH readiness in background — do NOT set vm_ssh_port until guest sshd is up
    let port_ref = state.vm_ssh_port.clone();
    tokio::spawn(async move {
        for _ in 0..120u32 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            if tokio::net::TcpStream::connect(("127.0.0.1", ssh_port)).await.is_ok() {
                *port_ref.write().unwrap() = Some(ssh_port);
                tracing::info!("VM SSH ready on port {ssh_port}");
                return;
            }
        }
        tracing::warn!("VM SSH never became ready (120s timeout)");
    });
    Ok(serde_json::json!({ "status": "booting", "ssh_port": ssh_port }))
}

#[tauri::command]
async fn vm_stop(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let handle = {
        let mut vm = state.vm.lock().await;
        vm.take()
    };
    if let Some(h) = handle {
        let backend = &state.vm_backend;
        backend.shutdown(h)?;
    }
    *state.vm_ssh_port.write().unwrap() = None;
    Ok(())
}

#[tauri::command]
async fn vm_status(state: State<'_, Arc<AppState>>) -> Result<serde_json::Value, String> {
    let mut vm = state.vm.lock().await;
    if let Some(ref mut handle) = *vm {
        let running = matches!(handle.child.try_wait(), Ok(None));
        if !running {
            *vm = None;
            *state.vm_ssh_port.write().unwrap() = None;
            return Ok(serde_json::json!({ "running": false }));
        }
        Ok(serde_json::json!({ "running": true, "ssh_port": handle.ssh_port }))
    } else {
        Ok(serde_json::json!({ "running": false }))
    }
}

#[tauri::command]
fn auth_check() -> serde_json::Value {
    let needs_setup = !lagado_agent::auth::keychain_exists();
    let locked_secs = lagado_agent::auth::lockout_check();
    serde_json::json!({
        "needs_setup": needs_setup,
        "locked": locked_secs > 0,
        "locked_until_secs": if locked_secs > 0 { locked_secs } else { 0 },
        "failures": lagado_agent::auth::lockout_failures(),
    })
}

#[tauri::command]
async fn auth_signup(
    password: String,
    recovery_phrase: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    if password.len() < 8 {
        return Err("Password must be at least 8 characters".to_string());
    }
    if recovery_phrase.len() < 12 {
        return Err("Recovery phrase must be at least 12 characters".to_string());
    }
    let dek = lagado_agent::auth::keychain_create(&password, &recovery_phrase)?;
    lagado_agent::auth::set_session_dek(dek.clone());
    *state.session_dek.lock().await = Some(dek);
    Ok(())
}

#[tauri::command]
async fn auth_login(
    password: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let dek = lagado_agent::auth::keychain_unlock(&password)?;
    lagado_agent::auth::set_session_dek(dek.clone());
    *state.session_dek.lock().await = Some(dek);
    Ok(())
}

#[tauri::command]
async fn auth_recover(
    recovery_phrase: String,
    new_password: String,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    if new_password.len() < 8 {
        return Err("Password must be at least 8 characters".to_string());
    }
    let dek = lagado_agent::auth::keychain_recover(&recovery_phrase, &new_password)?;
    lagado_agent::auth::set_session_dek(dek.clone());
    *state.session_dek.lock().await = Some(dek);
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

    let memory_db_path = config::data_dir().join("memory.db");
    let memory_tiers: Arc<Mutex<MemoryTiers>> = Arc::new(Mutex::new(
        MemoryTiers::open(&memory_db_path).unwrap_or_else(|e| {
            eprintln!("memory_tiers init failed: {e}");
            std::process::exit(1);
        })
    ));
    let memory_for_setup = memory_tiers.clone();

    let llama_child: Arc<Mutex<Option<KillOnDrop>>> = Arc::new(Mutex::new(None));
    let llama_for_setup = llama_child.clone();
    let classifier_child: Arc<Mutex<Option<KillOnDrop>>> = Arc::new(Mutex::new(None));
    let classifier_for_setup = classifier_child.clone();

    let vm_ssh_port: VmSshPort = std::sync::Arc::new(std::sync::RwLock::new(None));

    #[cfg(target_os = "linux")]
    let (perceptor_impl, actuator_impl) = lagado_agent::perception::linux_pair();
    #[cfg(target_os = "linux")]
    let host_perceptor: Arc<dyn lagado_agent::perception::Perceptor + Send + Sync> =
        Arc::new(perceptor_impl);
    #[cfg(target_os = "linux")]
    let host_actuator: Arc<dyn lagado_agent::perception::Actuator + Send + Sync> =
        Arc::new(actuator_impl);

    #[cfg(not(target_os = "linux"))]
    let host_perceptor: Arc<dyn lagado_agent::perception::Perceptor + Send + Sync> =
        Arc::new(lagado_agent::perception::MockPerceptor);
    #[cfg(not(target_os = "linux"))]
    let host_actuator: Arc<dyn lagado_agent::perception::Actuator + Send + Sync> =
        Arc::new(lagado_agent::perception::MockActuator);

    let ssh_cache = Arc::new(std::sync::Mutex::new(PerceptionCache::new()));

    let perceptor: Arc<dyn lagado_agent::perception::Perceptor + Send + Sync> =
        Arc::new(DynamicPerceptor { vm_port: vm_ssh_port.clone(), ssh_cache: ssh_cache.clone(), host: host_perceptor });

    let actuator: Arc<dyn lagado_agent::perception::Actuator + Send + Sync> =
        Arc::new(DynamicActuator { vm_port: vm_ssh_port.clone(), ssh_cache: ssh_cache.clone(), host: host_actuator });

    // Load in-process visual encoder (Linux only). Gracefully absent if model files missing.
    #[cfg(target_os = "linux")]
    let visual_encoder: Option<Arc<VisualEncoder>> = {
        let model_p  = config::vlm_model_path();
        let mmproj_p = config::vlm_mmproj_path();
        if model_p.exists() && mmproj_p.exists() {
            match VisualEncoder::load(
                &model_p.to_string_lossy(),
                &mmproj_p.to_string_lossy(),
                true,
            ) {
                Ok(enc) => { tracing::info!("VisualEncoder ready"); Some(Arc::new(enc)) }
                Err(e)  => { tracing::warn!("VisualEncoder failed to load: {e}"); None }
            }
        } else {
            tracing::info!("VLM model files absent — visual embedding disabled");
            None
        }
    };

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
        _classifier_child: classifier_child,
        #[cfg(target_os = "linux")]
        visual_encoder,
        vm: Arc::new(Mutex::new(None)),
        vm_ssh_port: vm_ssh_port.clone(),
        vm_backend: QemuDesktopBackend::default(),
        session_dek: Arc::new(Mutex::new(None)),
        ssh_cache,
        memory_tiers,
    });

    tauri::Builder::default()
        .setup(move |_app| {
            // Start main 8B llama-server
            tauri::async_runtime::spawn(async move {
                let child = ensure_llama_server().await;
                *llama_for_setup.lock().await = child.map(KillOnDrop);
            });
            // Start 1.2B classifier server (CPU-only, port 8081)
            tauri::async_runtime::spawn(async move {
                let child = ensure_classifier_server().await;
                *classifier_for_setup.lock().await = child.map(KillOnDrop);
            });
            // Start background memory consolidation loop (5-min decay cycles)
            tauri::async_runtime::spawn(async move {
                let gate = SleepGate::new(memory_for_setup);
                let _handle = gate.start();
                std::future::pending::<()>().await;
            });
            Ok(())
        })
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            send_goal, send_chat, send_command, send_approval,
            initialize_timeline, get_active_model, set_active_model, list_models,
            get_chronos_recent, terminal_spawn, terminal_run, terminal_get_cwd,
            vault_list_files, get_server_status, capture_frame,
            vm_boot, vm_stop, vm_status,
            auth_check, auth_signup, auth_login, auth_recover,
        ])
        .run(tauri::generate_context!())
        .expect("Lagado failed to start");
}
