//! first_walk — the agent's first real goal in the live VM, headless.
//!
//! Drives the FULL production pipeline (hydra::run → classifier → agent_loop)
//! with the real 8B brain, real VM perception/actuation, and the real HITL gate.
//! The human approval channel is scripted: every permission request is printed
//! and auto-approved, so the gate logic runs exactly as in production while the
//! walk stays unattended. The point is not success — it is observing where the
//! cortex stumbles now that it has eyes and hands.
//!
//! Usage: first_walk [goal text]
//! Default goal: "Click the Applications menu in the top panel"

#[cfg(not(unix))]
fn main() {
    eprintln!("[first_walk] Unix required");
}

#[cfg(unix)]
#[tokio::main]
async fn main() {
    use std::process::Command;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use lagado_agent::inference::llama_cpp::LlamaCppAdapter;
    use lagado_agent::perception::{Actuator, PerceptionCache, Perceptor};
    use lagado_agent::skill_library::SkillLibrary;
    use lagado_agent::perception::frame::FrameProcessor;
    use lagado_agent::vm::{QemuDesktopBackend, QmpClient, SshActuator, SshPerceptor, VmBackend, VmConfig};
    use lagado_agent::{agent, config, hydra, memory_tiers::MemoryTiers};
    use tokio::sync::mpsc;

    let goal = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    let goal = if goal.is_empty() {
        "Click the Applications menu in the top panel".to_string()
    } else {
        goal
    };

    fn ssh_try(port: u16, cmd: &str) -> Option<String> {
        let out = Command::new("ssh")
            .args([
                "-o", "StrictHostKeyChecking=no",
                "-o", "ConnectTimeout=5",
                "-o", "BatchMode=yes",
                "-p", &port.to_string(),
                "laputa@127.0.0.1",
                cmd,
            ])
            .output()
            .ok()?;
        if out.status.success() {
            Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
        } else {
            None
        }
    }

    println!("══ FIRST WALK ══════════════════════════════════════════");
    println!("goal: {goal}\n");

    // ── Sanity: the brains must be up ──
    for (port, name) in [(8080u16, "8B brain"), (8081, "classifier")] {
        let ok = ureq::get(&format!("http://127.0.0.1:{port}/health"))
            .timeout(Duration::from_secs(3))
            .call()
            .is_ok();
        println!("[pre] {name} on :{port} — {}", if ok { "up" } else { "DOWN" });
        if !ok && port == 8080 {
            eprintln!("[FAIL] 8B server is required");
            std::process::exit(1);
        }
    }

    // ── Boot VM through the real backend (kill-stale pre-flight included) ──
    let t0 = Instant::now();
    let backend = QemuDesktopBackend::default();
    let cfg = VmConfig::default();
    let port = cfg.ssh_port;
    println!("[vm] booting…");
    let handle = match backend.boot(&cfg) {
        Ok(h) => h,
        Err(e) => { eprintln!("[FAIL] boot: {e}"); std::process::exit(1); }
    };

    let deadline = Instant::now() + Duration::from_secs(240);
    let mut up = false;
    while Instant::now() < deadline {
        if let Some(w) = ssh_try(port, "whoami") {
            if w.contains("laputa") { up = true; break; }
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
    if !up {
        eprintln!("[FAIL] guest ssh never came up");
        let _ = backend.shutdown(handle);
        std::process::exit(1);
    }
    println!("[vm] ssh up after {:?}", t0.elapsed());

    let x_deadline = Instant::now() + Duration::from_secs(90);
    while Instant::now() < x_deadline {
        if ssh_try(port, "DISPLAY=:0 xdotool getdisplaygeometry 2>/dev/null")
            .map(|g| !g.is_empty())
            .unwrap_or(false)
        { break; }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
    println!("[vm] X up");

    // Fresh perceive.py (source of truth is repo root)
    let _ = Command::new("scp")
        .args([
            "-o", "StrictHostKeyChecking=no", "-o", "BatchMode=yes",
            "-P", &port.to_string(),
            "perceive.py", "laputa@127.0.0.1:/home/laputa/perceive.py",
        ])
        .status();

    // ── Wire the production stack ──
    let cache = Arc::new(Mutex::new(PerceptionCache::new()));
    let perceptor: Arc<dyn Perceptor> =
        Arc::new(SshPerceptor::with_cache("127.0.0.1", port, "laputa", cache.clone()));
    let actuator: Arc<dyn Actuator> =
        Arc::new(SshActuator::with_cache("127.0.0.1", port, "laputa", cache.clone()));

    let adapter: Arc<dyn lagado_agent::inference::InferenceAdapter> = Arc::new(
        LlamaCppAdapter::with_url(&config::llama_base_url(), "LFM2-8B-A1B", config::CONTEXT_SIZE),
    );

    let memory_db = config::data_dir().join("memory.db");
    let memory = MemoryTiers::open(&memory_db)
        .unwrap_or_else(|e| { eprintln!("[FAIL] memory open: {e}"); std::process::exit(1); });
    let memory_tiers = Arc::new(tokio::sync::Mutex::new(memory));
    let skill_library = Arc::new(SkillLibrary::open(&config::data_dir()));

    // HITL channels — the walk scripts the human: log every envelope, approve every request.
    let (approval_tx, approval_rx) = mpsc::channel::<bool>(8);
    let (confirm_tx, mut confirm_rx) = mpsc::channel::<String>(64);

    let state = Arc::new(tokio::sync::Mutex::new(agent::AgentState {
        goal: String::new(),
        running: false,
        approval_tx: Some(approval_tx.clone()),
        pending_id: None,
    }));

    // Envelope listener: prints the agent's narration; auto-approves permission requests.
    let approver = approval_tx.clone();
    let listener = tokio::spawn(async move {
        let mut n_approved = 0u32;
        while let Some(env) = confirm_rx.recv().await {
            println!("[agent] {env}");
            if env.contains("\"permission\"") || env.contains("\"type\":\"permission\"") {
                n_approved += 1;
                println!("[walk]  ↳ HITL gate fired — auto-approving (#{n_approved})");
                let _ = approver.send(true).await;
            }
        }
        n_approved
    });

    // Visual evidence: frame before the agent acts.
    let frame_before = "/dev/shm/first_walk_before.png";
    let frame_after = "/dev/shm/first_walk_after.png";
    if let Ok(mut qmp) = QmpClient::connect(&cfg.qmp_socket) {
        let _ = qmp.screendump(frame_before);
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
    // Frame freshness during the walk is now the perceptor's job: the agent loop calls
    // perceptor.capture_frame() (QMP screendump synced with each settled perception) before the CV
    // read — the real production frame-sync. No background feed (it would contend with capture_frame
    // for the single-client QMP socket).

    // ── Hand the goal to the full pipeline ──
    println!("\n[walk] handing goal to hydra::run — watching…\n");
    let walk = hydra::run(
        goal.clone(),
        String::new(),
        hydra::RouteContext { surface: hydra::SurfaceState { vm_active: true, ..Default::default() }, mode: hydra::RouteMode::Auto },
        state.clone(),
        adapter,
        perceptor.clone(),
        actuator,
        approval_rx,
        confirm_tx, // dropped inside run when done → listener ends
        memory_tiers,
        None, // visual encoder: skipped for the walk (episode embedding not the subject)
        skill_library,
    );

    let outcome = tokio::time::timeout(Duration::from_secs(300), walk).await;
    match outcome {
        Ok(()) => println!("\n[walk] pipeline returned cleanly in {:?}", t0.elapsed()),
        Err(_) => println!("\n[walk] TIMEOUT — pipeline still running after 300s (stumble: loop or hang)"),
    }

    let approvals = listener.await.unwrap_or(0);
    println!("[walk] HITL gate fired {approvals} time(s)");

    // Stop the frame feed and release QMP BEFORE the after-dump reconnects.

    // Visual evidence: frame after, plus cell-level delta.
    if let Ok(mut qmp) = QmpClient::connect(&cfg.qmp_socket) {
        let _ = qmp.screendump(frame_after);
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
    if let (Ok(b), Ok(a)) = (std::fs::read(frame_before), std::fs::read(frame_after)) {
        let mut fp = FrameProcessor::new();
        if fp.process_frame(&b).is_ok() {
            match fp.process_frame(&a) {
                Ok(changed) => println!(
                    "[walk] screen delta across the whole walk: {} of 48 cells changed \
                     (frames kept at {frame_before} / {frame_after})",
                    changed.len()
                ),
                Err(e) => println!("[walk] delta failed: {e}"),
            }
        }
    }

    // Final screen evidence
    let final_screen = perceptor.read_screen();
    println!("\n── final screen (first 12 lines) ──");
    for line in final_screen.lines().take(12) {
        println!("    | {line}");
    }

    let _ = backend.shutdown(handle);
    println!("\n[first_walk] done — total {:?}", t0.elapsed());
}
