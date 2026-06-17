#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tauri::{State, Emitter};
use lagado_agent::{
    agent::AgentState,
    bootstrap::{ensure_llama_server, ensure_classifier_server, ensure_embedder_server, KillOnDrop},
    config,
    hydra,
    inference::{InferenceAdapter, llama_cpp::LlamaCppAdapter},
    memory_tiers::MemoryTiers,
    perception::{Perceptor, Actuator, PerceptionCache},
    server_guard::{ServerGuard, ServerEvent},
    skill_library::SkillLibrary,
    sleep_gate::SleepGate,
    vm::{QemuDesktopBackend, VmHandle, VmBackend, VmSshPort, DynamicActuator, DynamicPerceptor},
};
use lagado_agent::vision::VisualEncoder;

struct AppState {
    agent: Arc<Mutex<AgentState>>,
    adapter: Arc<dyn InferenceAdapter + Send + Sync>,
    perceptor: Arc<dyn Perceptor + Send + Sync>,
    actuator: Arc<dyn Actuator + Send + Sync>,
    _llama_child: Arc<Mutex<Option<KillOnDrop>>>,
    _classifier_child: Arc<Mutex<Option<KillOnDrop>>>,
    _embedder_child: Arc<Mutex<Option<KillOnDrop>>>,
    visual_encoder: Option<Arc<VisualEncoder>>,
    vm: Arc<Mutex<Option<VmHandle>>>,
    vm_ssh_port: VmSshPort,
    vm_backend: QemuDesktopBackend,
    session_dek: Arc<Mutex<Option<Vec<u8>>>>,
    ssh_cache: Arc<std::sync::Mutex<PerceptionCache>>,
    memory_tiers: Arc<Mutex<MemoryTiers>>,
    skill_library: Arc<SkillLibrary>,
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
    let visual_encoder = state.visual_encoder.clone();
    let skill_library = state.skill_library.clone();

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
            visual_encoder,
            skill_library,
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

/// Engine status: what the governor DISCOVERED about the model + probed hardware +
/// the derived plan. Every number is read/measured/computed — none assumed (invariant #9).
#[tauri::command]
fn get_engine_status() -> serde_json::Value {
    let model_path = lagado_agent::config::model_path();
    let gpu = lagado_agent::governor::detect_gpu();
    let cores = lagado_agent::governor::cpu_cores();

    match lagado_agent::gguf::read_metadata(&model_path) {
        Ok(m) => {
            let prefs = lagado_agent::governor::EnginePrefs::default();
            let cal: Vec<lagado_agent::governor::CalPoint> = vec![]; // cold start until runtime measures
            let plan = lagado_agent::governor::plan_engine(&m, gpu.as_ref(), &prefs, &cal);
            serde_json::json!({
                "ok": true,
                "model": {
                    "file": model_path.file_name().and_then(|s| s.to_str()).unwrap_or(""),
                    "arch": m.arch,
                    "context_length": m.context_length,
                    "block_count": m.block_count,
                    "embedding_length": m.embedding_length,
                    "expert_count": m.expert_count,
                    "is_moe": m.is_moe(),
                    "file_mb": m.file_bytes / (1024 * 1024),
                },
                "hardware": {
                    "gpu": gpu.as_ref().map(|g| format!("{:?}", g.vendor)),
                    "vram_total_mb": gpu.as_ref().map(|g| g.vram_total_mb),
                    "vram_free_mb": gpu.as_ref().map(|g| g.vram_free_mb),
                    "cpu_cores": cores,
                },
                "plan": {
                    "ctx": plan.ctx,
                    "n_gpu_layers": plan.n_gpu_layers,
                    "cpu_moe": plan.cpu_moe,
                    "predicted_vram_mb": plan.predicted_vram_mb,
                    "feasibility": plan.feasibility.map(|f| format!("{:?}", f)),
                    "rationale": plan.rationale,
                },
            })
        }
        Err(e) => serde_json::json!({ "ok": false, "error": e }),
    }
}

/// Real machine specs for onboarding — PROBED, never hardcoded (invariant #9).
#[tauri::command]
fn get_system_info() -> serde_json::Value {
    let s = lagado_agent::sysinfo::probe();
    let tier = match s.vram_total_mb {
        Some(v) if v >= 12 * 1024 => "full",
        Some(v) if v >= 6 * 1024 => "balanced",
        Some(_) => "light",
        None if s.ram_total_gb >= 16.0 => "balanced-cpu",
        None => "light-cpu",
    };
    serde_json::json!({
        "cpu_model": s.cpu_model,
        "physical_cores": s.physical_cores,
        "logical_threads": s.logical_threads,
        "ram_total_gb": s.ram_total_gb,
        "gpu_name": s.gpu_name,
        "vram_total_mb": s.vram_total_mb,
        "vram_free_mb": s.vram_free_mb,
        "storage_free_gb": s.storage_free_gb,
        "storage_total_gb": s.storage_total_gb,
        "os": s.os,
        "tier": tier,
    })
}

/// Real model catalog for onboarding — each model's actual GGUF metadata + a rough fit
/// against this machine's free VRAM. No hardcoded sizes/specs.
#[tauri::command]
fn get_models_detailed() -> serde_json::Value {
    let dir = lagado_agent::config::data_dir().join("models");
    let gpu = lagado_agent::governor::detect_gpu();
    let free_mb = gpu.as_ref().map(|g| g.vram_free_mb as f32);
    let models: Vec<serde_json::Value> = lagado_agent::config::available_models()
        .into_iter()
        .map(|f| match lagado_agent::gguf::read_metadata(&dir.join(&f)) {
            Ok(m) => {
                let size_mb = m.file_bytes / (1024 * 1024);
                let fit = match free_mb {
                    Some(fm) => {
                        let w = size_mb as f32;
                        if w * 1.15 <= fm { "fits" } else if w <= fm { "tight" } else { "partial/cpu" }
                    }
                    None => "cpu",
                };
                serde_json::json!({
                    "file": f, "arch": m.arch, "context_length": m.context_length,
                    "block_count": m.block_count, "expert_count": m.expert_count,
                    "is_moe": m.is_moe(), "size_mb": size_mb, "fit": fit,
                })
            }
            Err(e) => serde_json::json!({ "file": f, "error": e.to_string() }),
        })
        .collect();
    serde_json::json!({ "models": models })
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
    // Poll for SSH readiness in background — do NOT set vm_ssh_port until SSH
    // auth succeeds (whoami probe returns "laputa").  Bare TCP connect is NOT
    // sufficient: sshd may accept connections before key auth is configured.
    let port_ref = state.vm_ssh_port.clone();
    tokio::spawn(async move {
        for _ in 0..120u32 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let port_str = ssh_port.to_string();
            let auth_ok = tokio::task::spawn_blocking(move || {
                std::process::Command::new("ssh")
                    .args([
                        "-o", "StrictHostKeyChecking=no",
                        "-o", "ConnectTimeout=5",
                        "-o", "BatchMode=yes",
                        "-p", &port_str,
                        "laputa@127.0.0.1",
                        "whoami",
                    ])
                    .output()
                    .map(|out| {
                        out.status.success()
                            && String::from_utf8_lossy(&out.stdout).contains("laputa")
                    })
                    .unwrap_or(false)
            })
            .await
            .unwrap_or(false);
            if auth_ok {
                *port_ref.write().unwrap() = Some(ssh_port);
                tracing::info!("VM SSH auth confirmed on port {ssh_port}");
                return;
            }
        }
        tracing::warn!("VM SSH auth never succeeded (120s timeout)");
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

// ── Network settings ──────────────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default)]
struct NetworkSettings {
    proxy_enabled:  bool,
    proxy_type:     String,
    proxy_host:     String,
    proxy_port:     u16,
    bridge_address: String,
}

fn network_settings_path() -> std::path::PathBuf {
    lagado_agent::config::data_dir().join("config/network.json")
}

#[tauri::command]
fn get_network_settings() -> NetworkSettings {
    let path = network_settings_path();
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

#[tauri::command]
fn save_network_settings(settings: NetworkSettings) -> Result<(), String> {
    let path = network_settings_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())
}

#[tauri::command]
async fn test_network_connection(settings: NetworkSettings) -> String {
    if !settings.proxy_enabled {
        return "No proxy configured — direct connection active.".to_string();
    }
    let proxy_url = format!(
        "{}://{}:{}",
        settings.proxy_type, settings.proxy_host, settings.proxy_port
    );
    let client = match reqwest::Proxy::all(&proxy_url)
        .ok()
        .and_then(|p| reqwest::Client::builder().proxy(p).timeout(std::time::Duration::from_secs(10)).build().ok())
    {
        Some(c) => c,
        None => return "Failed to build proxy client — check proxy address.".to_string(),
    };
    match client.get("https://check.torproject.org/api/ip").send().await {
        Ok(r) if r.status().is_success() => {
            r.text().await
                .map(|t| format!("Connected — {}", t.chars().take(120).collect::<String>()))
                .unwrap_or_else(|_| "Connected.".to_string())
        }
        Ok(r) => format!("Proxy reachable but got HTTP {}.", r.status()),
        Err(e) => format!("Connection failed: {e}"),
    }
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

    let skill_library = Arc::new(SkillLibrary::open(&config::data_dir()));
    let memory_for_setup  = memory_tiers.clone();
    let adapter_for_sleep = adapter.clone();

    let llama_child: Arc<Mutex<Option<KillOnDrop>>> = Arc::new(Mutex::new(None));
    let llama_for_setup = llama_child.clone();
    let llama_for_guard = llama_child.clone();
    let classifier_child: Arc<Mutex<Option<KillOnDrop>>> = Arc::new(Mutex::new(None));
    let classifier_for_setup = classifier_child.clone();
    let classifier_for_guard = classifier_child.clone();
    let embedder_child: Arc<Mutex<Option<KillOnDrop>>> = Arc::new(Mutex::new(None));
    let embedder_for_setup = embedder_child.clone();
    let embedder_for_guard = embedder_child.clone();

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

    // Load in-process visual encoder. Returns None gracefully if model files are absent
    // or on non-Linux platforms (VisualEncoder::load returns Err there).
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
        _embedder_child: embedder_child,
        visual_encoder,
        vm: Arc::new(Mutex::new(None)),
        vm_ssh_port: vm_ssh_port.clone(),
        vm_backend: QemuDesktopBackend::default(),
        session_dek: Arc::new(Mutex::new(None)),
        ssh_cache,
        memory_tiers,
        skill_library,
    });

    // Clone for the SIGTERM/SIGINT handler — must happen before .manage() consumes state.
    #[cfg(unix)]
    let state_for_signal = state.clone();

    tauri::Builder::default()
        .setup(move |app| {
            // Clean up empty cgroup leaves from any previous Lagado run
            lagado_agent::security::sandbox::cleanup_stale();
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
            // Start ColBERT embedder server (CPU-only, port 8082) — Board relevance signal
            tauri::async_runtime::spawn(async move {
                let child = ensure_embedder_server().await;
                *embedder_for_setup.lock().await = child.map(KillOnDrop);
            });
            // Health monitor: polls /health every 10s, restarts crashed servers.
            // Holds Arc clones of both child slots — KillOnDrop still fires on exit
            // because Tauri tears down its async runtime before .run() returns,
            // releasing all Arc clones and letting Drop run normally.
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                ServerGuard::new(
                    llama_for_guard,
                    classifier_for_guard,
                    embedder_for_guard,
                    move |event| {
                        let (kind, server) = match event {
                            ServerEvent::Crashed { server }       => ("server_crashed", server),
                            ServerEvent::Restarted { server }     => ("server_restarted", server),
                            ServerEvent::RestartFailed { server } => ("server_restart_failed", server),
                        };
                        let _ = app_handle.emit(kind, serde_json::json!({ "server": server }));
                    },
                )
                .run()
                .await;
            });
            // Start background memory consolidation loop (5-min decay cycles)
            tauri::async_runtime::spawn(async move {
                let gate = SleepGate::new(memory_for_setup, adapter_for_sleep);
                let _handle = gate.start();
                std::future::pending::<()>().await;
            });
            // SIGTERM / SIGINT handler: shut down the VM cleanly before exiting.
            // Drop does not run on signals, so we must do this explicitly.
            // Guard is dropped before .await — mutex invariant satisfied.
            #[cfg(unix)]
            {
                tauri::async_runtime::spawn(async move {
                    use tokio::signal::unix::{signal, SignalKind};
                    let mut sigterm = match signal(SignalKind::terminate()) {
                        Ok(s) => s,
                        Err(e) => { tracing::warn!("SIGTERM handler setup failed: {e}"); return; }
                    };
                    let mut sigint = match signal(SignalKind::interrupt()) {
                        Ok(s) => s,
                        Err(e) => { tracing::warn!("SIGINT handler setup failed: {e}"); return; }
                    };
                    tokio::select! {
                        _ = sigterm.recv() => { tracing::info!("SIGTERM received — shutting down VM"); }
                        _ = sigint.recv()  => { tracing::info!("SIGINT received — shutting down VM"); }
                    }
                    let handle = {
                        let mut vm = state_for_signal.vm.lock().await;
                        vm.take()
                    }; // Mutex guard dropped here before any further .await
                    if let Some(h) = handle {
                        let _ = state_for_signal.vm_backend.shutdown(h);
                    }
                    std::process::exit(0);
                });
            }
            Ok(())
        })
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            send_goal, send_chat, send_command, send_approval,
            initialize_timeline, get_active_model, set_active_model, list_models,
            get_engine_status, get_system_info, get_models_detailed,
            get_chronos_recent, terminal_spawn, terminal_run, terminal_get_cwd,
            vault_list_files, get_server_status, capture_frame,
            vm_boot, vm_stop, vm_status,
            auth_check, auth_signup, auth_login, auth_recover,
            get_network_settings, save_network_settings, test_network_connection,
        ])
        .run(tauri::generate_context!())
        .expect("Lagado failed to start");
}
