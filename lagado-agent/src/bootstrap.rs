use std::process::{Child, Command};
use crate::{chronos, config, governor};

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
                    tracing::error!("llama-server did not become ready within 60s — exiting.");
                    std::process::exit(1);
                }
                tracing::info!("llama-server ready.");
                Some(child)
            }
            Err(e) => {
                tracing::error!("Failed to spawn llama-server: {e}");
                std::process::exit(1);
            }
        }
    }
}
