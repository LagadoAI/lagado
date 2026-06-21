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

    // The classifier task needs only a small context (short clean-context prompts), but never ASSUME the
    // model supports that size — clamp the task-need to the model's DISCOVERED trained context (inv #9:
    // discover, don't hardcode a model-capability assumption). A model with a smaller max would otherwise
    // fail to spawn.
    let ctx = {
        let p = model_path.clone();
        let model_max = tokio::task::spawn_blocking(move || crate::gguf::read_metadata(&p).ok())
            .await.ok().flatten().and_then(|m| m.context_length).map(|c| c as usize);
        model_max.map_or(config::CLASSIFIER_CONTEXT_SIZE, |max| config::CLASSIFIER_CONTEXT_SIZE.min(max))
    };

    let mut cmd = Command::new(config::llama_server_bin());
    cmd.args([
        "-m", &model_path.to_string_lossy(),
        "-c", &ctx.to_string(),
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

/// Start the Board's relevance embedder (LFM2-ColBERT-350M, mean-pooled) on port 8082.
/// CPU-only (-ngl 0) — it's tiny (~228 MB) and runs off the hot path (sleep-gate backfill
/// + once per goal at loop start), so it must not compete with the 8B for VRAM. Context is
/// DISCOVERED from the GGUF (invariant #9), falling back to a conservative floor.
/// Returns None if the model file is absent or a server is already up on the port.
pub async fn ensure_embedder_server() -> Option<Child> {
    let model_path = config::embed_model_path();
    if !model_path.exists() {
        tracing::info!("No embedder model at {:?} — Board relevance disabled (recency floor only)", model_path);
        return None;
    }

    let already_up = tokio::task::spawn_blocking(|| {
        check_health_sync(&config::embed_base_url(), 2)
    })
    .await
    .unwrap_or(false);

    if already_up {
        tracing::info!("Embedder server already running — reusing.");
        return None;
    }

    // DISCOVER the model's trained context from its GGUF metadata; fall back to the floor.
    let ctx = {
        let p = model_path.clone();
        tokio::task::spawn_blocking(move || crate::gguf::read_metadata(&p).ok())
            .await
            .ok()
            .flatten()
            .and_then(|m| m.context_length)
            .map(|c| c as usize)
            .unwrap_or(config::EMBED_CONTEXT_FALLBACK)
    };

    let mut cmd = Command::new(config::llama_server_bin());
    cmd.args([
        "-m", &model_path.to_string_lossy(),
        "-c", &ctx.to_string(),
        "-ngl", "0",
        "-t", "2",
        "--parallel", "1",
        "--embeddings",
        "--pooling", "mean",
        "--host", &config::llama_host(),
        "--port", &config::embed_port().to_string(),
    ]);
    cmd.stdout(Stdio::null()).stderr(Stdio::null());

    match cmd.spawn() {
        Ok(child) => {
            if let Err(e) = crate::security::sandbox::apply_limits(
                child.id(), "embedder",
                config::embed_memory_max_bytes(),
                256,
            ) {
                tracing::warn!("sandbox: embedder: {e}");
            }
            let ready = tokio::task::spawn_blocking(|| {
                let url = config::embed_base_url();
                for _ in 0..30 {
                    if check_health_sync(&url, 2) { return true; }
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
                false
            })
            .await
            .unwrap_or(false);

            if ready {
                tracing::info!("Embedder server ready on port {} (ctx {ctx})", config::embed_port());
                Some(child)
            } else {
                tracing::warn!("Embedder server did not become ready within 30s — Board on recency floor");
                None
            }
        }
        Err(e) => {
            tracing::warn!("Failed to spawn embedder server: {e} — Board on recency floor");
            None
        }
    }
}

pub async fn ensure_llama_server() -> Option<Child> {
    let model_path = config::model_path();
    let model_bytes = std::fs::metadata(&model_path).map(|m| m.len()).unwrap_or(0);
    // Base config (threads / parallelism / GPU detection) from the legacy planner.
    let cfg = governor::detect_and_plan(config::CONTEXT_SIZE, model_bytes);

    // GGUF-AWARE OFFLOAD (optimization audit v1, Theme 2): read the model's real layer/expert/ctx
    // metadata and run the partial-offload planner instead of the crude all-or-nothing 1.1×-headroom
    // rule. This is what lets a 4.7GB MoE model run mostly on a 6GB GPU (partial -ngl + --cpu-moe)
    // rather than dumping everything to CPU. Env overrides (LAGADO_NGL/LAGADO_CTX/LAGADO_CPU_MOE) feed
    // EnginePrefs (invariant #9 defer-to-user). Falls back to the legacy cfg if GGUF can't be read.
    let prefs = governor::EnginePrefs {
        ctx: std::env::var("LAGADO_CTX").ok().and_then(|v| v.parse().ok()),
        n_gpu_layers: std::env::var("LAGADO_NGL").ok().and_then(|v| v.parse().ok()),
        cpu_moe: if std::env::var("LAGADO_CPU_MOE").is_ok() { Some(true) } else { None },
    };
    let (ctx, n_gpu_layers, cpu_moe) = match crate::gguf::read_metadata(&model_path) {
        Ok(model) => {
            let plan = governor::plan_engine(&model, cfg.gpu.as_ref(), &prefs, &[]);
            chronos::log(&format!("engine_plan: {}", plan.rationale));
            (plan.ctx as usize, plan.n_gpu_layers, plan.cpu_moe)
        }
        Err(e) => {
            chronos::log(&format!("gguf read failed ({e}) — legacy offload plan"));
            (cfg.ctx, cfg.n_gpu_layers, cfg.moe_experts_on_cpu)
        }
    };
    let flash_attn = n_gpu_layers > 0; // follow the FINAL offload, not the legacy cfg's
    chronos::log(&format!(
        "server_config: gpu={} ctx={} ngl={} threads={} parallel={} moe_cpu={}",
        n_gpu_layers > 0, ctx, n_gpu_layers, cfg.threads, cfg.n_parallel, cpu_moe,
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

    let mut args = vec![
        "-m".to_string(), model_path.to_string_lossy().to_string(),
        "-c".to_string(), ctx.to_string(),
        "-ngl".to_string(), n_gpu_layers.to_string(),
        "-t".to_string(), cfg.threads.to_string(),
        "--parallel".to_string(), cfg.n_parallel.to_string(),
        "--host".to_string(), config::llama_host(),
        "--port".to_string(), config::llama_port().to_string(),
    ];
    if flash_attn {
        args.push("-fa".to_string());
        args.push("on".to_string());
    }
    if cpu_moe {
        // MoE model: keep expert tensors on CPU, attention/embedding on GPU. Set by plan_engine
        // when the GGUF metadata reports expert_count > 1 and VRAM is tight (or LAGADO_CPU_MOE).
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
