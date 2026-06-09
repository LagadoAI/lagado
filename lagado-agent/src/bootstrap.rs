use std::process::{Child, Command, Stdio};
use crate::{chronos, config, governor};

/// Start a lightweight llama-server on port 8081 for intent classification.
/// Uses the 350M model, CPU-only (preserves GPU for the 8B), ctx=512.
/// Returns None if the model file doesn't exist or if a server is already running.
pub async fn ensure_classifier_server() -> Option<Child> {
    let model_path = config::classifier_model_path();
    if !model_path.exists() {
        tracing::info!("No classifier model at {:?} — skipping classifier server", model_path);
        return None;
    }

    let already_up = tokio::task::spawn_blocking(|| {
        ureq::get(&format!("{}/health", config::classifier_base_url())).call().is_ok()
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
        "-ngl", "0",        // CPU-only — leave GPU headroom for the 8B model
        "-t", "2",
        "--parallel", "1",
        "--host", &config::llama_host(),
        "--port", &config::classifier_port().to_string(),
    ]);
    cmd.stdout(Stdio::null()).stderr(Stdio::null());

    match cmd.spawn() {
        Ok(child) => {
            let ready = tokio::task::spawn_blocking(|| {
                let agent = ureq::AgentBuilder::new()
                    .timeout(std::time::Duration::from_secs(2))
                    .build();
                let url = format!("{}/health", config::classifier_base_url());
                for _ in 0..30 {
                    if agent.get(&url).call().is_ok() { return true; }
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

pub async fn ensure_llama_server() -> Option<Child> {
    // ── Governor: detect hardware, plan server config ─────────────
    let cfg = governor::detect_and_plan(config::CONTEXT_SIZE);
    chronos::log(&format!(
        "server_config: gpu={} ctx={} ngl={} threads={} parallel={}",
        cfg.n_gpu_layers > 0, cfg.ctx, cfg.n_gpu_layers, cfg.threads, cfg.n_parallel
    ));

    // Check if llama-server is already up before spawning
    let already_up = tokio::task::spawn_blocking(|| {
        let url = format!("{}/health", config::llama_base_url());
        ureq::get(&url).call().is_ok()
    })
    .await
    .unwrap_or(false);

    // Keep child alive for the duration of the program
    if already_up {
        chronos::log("server_config: reusing existing server");
        tracing::info!("llama-server already running — reusing.");
        None
    } else {
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
        tracing::info!("Spawning: {} {}", config::llama_server_bin().display(), args.join(" "));

        let mut cmd = Command::new(config::llama_server_bin());
        cmd.args(&args);
        match cmd.spawn() {
            Ok(child) => {
                let ready = tokio::task::spawn_blocking(|| {
                    let agent = ureq::AgentBuilder::new()
                        .timeout(std::time::Duration::from_secs(2))
                        .build();
                    let url = format!("{}/health", config::llama_base_url());
                    for _ in 0..60 {
                        if agent.get(&url).call().is_ok() {
                            return true;
                        }
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
}
