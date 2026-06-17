use std::process::{Child, Command, Stdio};
use crate::{chronos, config, governor};

/// Kills and reaps the managed server child on drop — prevents orphans on app exit.
/// Held in AppState and ServerGuard; dropped when both the state and the guard task
/// are torn down, which happens before the Tauri runtime returns from `.run()`.
pub struct KillOnDrop(pub std::process::Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Single-attempt synchronous health check. `timeout_secs` prevents hanging when
/// the server process is alive but not yet accepting requests.
/// Appends `/health` to `base_url` (e.g. `"http://127.0.0.1:8080"`).
/// Must be called from a blocking context (spawn_blocking or std thread).
pub fn check_health_sync(base_url: &str, timeout_secs: u64) -> bool {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build();
    agent.get(&format!("{base_url}/health")).call().is_ok()
}

/// Start a lightweight llama-server on port 8081 for intent classification.
/// Uses the 1.2B model, CPU-only (preserves GPU for the 8B), ctx=`CLASSIFIER_CONTEXT_SIZE`.
/// Returns None if the model file doesn't exist or if a server is already running.
pub async fn ensure_classifier_server() -> Option<Child> {
    let model_path = config::classifier_model_path();
    if !model_path.exists() {
        tracing::info!("No classifier model at {:?} — skipping classifier server", model_path);
        return None;
    }

    let already_up = tokio::task::spawn_blocking(|| {
        check_health_sync(&config::classifier_base_url(), 2)
    })
    .await
    .unwrap_or(false);

    if already_up {
        tracing::info!("Classifier server already running — reusing.");
        return None;
    }

    let mut cmd = Command::new(config::llama_server_bin());
    cmd.args([
        "-m", &model_path.to_string_lossy(),
        "-c", &config::CLASSIFIER_CONTEXT_SIZE.to_string(),
        "-ngl", "0",
        "-t", "2",
        "--parallel", "1",
        "--host", &config::llama_host(),
        "--port", &config::classifier_port().to_string(),
    ]);
    cmd.stdout(Stdio::null()).stderr(Stdio::null());

    match cmd.spawn() {
        Ok(child) => {
            if let Err(e) = crate::security::sandbox::apply_limits(
                child.id(), "classifier",
                config::classifier_memory_max_bytes(),
                256,
            ) {
                tracing::warn!("sandbox: classifier: {e}");
            }
            let ready = tokio::task::spawn_blocking(|| {
                let url = config::classifier_base_url();
                for _ in 0..30 {
                    if check_health_sync(&url, 2) { return true; }
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
                false
            })
            .await
            .unwrap_or(false);

            if ready {
                tracing::info!("Classifier server ready on port {}", config::classifier_port());
                Some(child)
            } else {
                tracing::warn!("Classifier server did not become ready within 30s — falling back to main model");
                None
            }
        }
        Err(e) => {
            tracing::warn!("Failed to spawn classifier server: {e} — classification uses main model");
            None
        }
    }
}

/// Start llama-server for the VLM (vision-language model) on port 8082.
/// Retired in Phase 3.3 (vision is now in-process FFI). Kept as dead code;
/// VLM subprocess approach is not used in the agent pipeline.
#[allow(dead_code)]
pub async fn ensure_vlm_server() -> Option<Child> {
    let model_path = config::vlm_model_path();
    let mmproj_path = config::vlm_mmproj_path();

    if !model_path.exists() {
        tracing::info!("VLM model not found at {:?} — skipping VLM server", model_path);
        return None;
    }
    if !mmproj_path.exists() {
        tracing::info!("VLM mmproj not found at {:?} — skipping VLM server", mmproj_path);
        return None;
    }

    let already_up = tokio::task::spawn_blocking(|| {
        check_health_sync(&config::vlm_base_url(), 2)
    })
    .await
    .unwrap_or(false);

    if already_up {
        tracing::info!("VLM server already running — reusing.");
        return None;
    }

    let mut cmd = Command::new(config::llama_server_bin());
    cmd.args([
        "-m", &model_path.to_string_lossy(),
        "--mmproj", &mmproj_path.to_string_lossy(),
        "-c", &config::VLM_CONTEXT_SIZE.to_string(),
        "-ngl", "32",
        "-t", "4",
        "--parallel", "1",
        "--host", &config::llama_host(),
        "--port", &config::vlm_port().to_string(),
    ]);
    cmd.stdout(Stdio::null()).stderr(Stdio::null());

    match cmd.spawn() {
        Ok(child) => {
            let ready = tokio::task::spawn_blocking(|| {
                let url = config::vlm_base_url();
                for _ in 0..30 {
                    if check_health_sync(&url, 2) { return true; }
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
                false
            })
            .await
            .unwrap_or(false);

            if ready {
                tracing::info!("VLM server ready on port {}", config::vlm_port());
                Some(child)
            } else {
                tracing::warn!("VLM server did not become ready within 30s — vision unavailable");
                None
            }
        }
        Err(e) => {
            tracing::warn!("Failed to spawn VLM server: {e} — vision unavailable");
            None
        }
    }
}

pub async fn ensure_llama_server() -> Option<Child> {
    let model_bytes = std::fs::metadata(config::model_path())
        .map(|m| m.len())
        .unwrap_or(0);
    let cfg = governor::detect_and_plan(config::CONTEXT_SIZE, model_bytes);
    chronos::log(&format!(
        "server_config: gpu={} vram_fit={:.0}% ctx={} ngl={} threads={} parallel={} moe_cpu={}",
        cfg.n_gpu_layers > 0,
        cfg.vram_fit_fraction(model_bytes) * 100.0,
        cfg.ctx, cfg.n_gpu_layers, cfg.threads, cfg.n_parallel,
        cfg.moe_experts_on_cpu,
    ));

    let already_up = tokio::task::spawn_blocking(|| {
        check_health_sync(&config::llama_base_url(), 2)
    })
    .await
    .unwrap_or(false);

    if already_up {
        chronos::log("server_config: reusing existing server");
        tracing::info!("llama-server already running — reusing.");
        return None;
    }

    let model_path = config::model_path();
    let mut args = vec![
        "-m".to_string(), model_path.to_string_lossy().to_string(),
        "-c".to_string(), cfg.ctx.to_string(),
        "-ngl".to_string(), cfg.n_gpu_layers.to_string(),
        "-t".to_string(), cfg.threads.to_string(),
        "--parallel".to_string(), cfg.n_parallel.to_string(),
        "--host".to_string(), config::llama_host(),
        "--port".to_string(), config::llama_port().to_string(),
    ];
    if cfg.flash_attn {
        args.push("-fa".to_string());
        args.push("on".to_string());
    }
    if cfg.moe_experts_on_cpu {
        // MoE model: keep expert tensors on CPU, attention/embedding on GPU.
        // Set by governor when Phase 3.x GGUF parser detects expert_count > 1.
        args.push("--cpu-moe".to_string());
    }
    tracing::info!("Spawning: {} {}", config::llama_server_bin().display(), args.join(" "));

    let mut cmd = Command::new(config::llama_server_bin());
    cmd.args(&args);
    match cmd.spawn() {
        Ok(child) => {
            if let Err(e) = crate::security::sandbox::apply_limits(
                child.id(), "llama",
                config::llama_memory_max_bytes(),
                256,
            ) {
                tracing::warn!("sandbox: llama: {e}");
            }
            let ready = tokio::task::spawn_blocking(|| {
                let url = config::llama_base_url();
                for _ in 0..60 {
                    if check_health_sync(&url, 2) { return true; }
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
                false
            })
            .await
            .unwrap_or(false);

            if !ready {
                tracing::error!("llama-server did not become ready within 60s — inference unavailable.");
                return None;
            }
            tracing::info!("llama-server ready.");
            Some(child)
        }
        Err(e) => {
            tracing::error!("Failed to spawn llama-server: {e} — inference unavailable.");
            None
        }
    }
}
