//! stress_test — OSWorld-style task suite against the live VM, many runs, execution-verified.
//!
//! Each task has a deterministic SUCCESS PREDICATE checked on the final screen (execution-based
//! verification, like OSWorld — not "did it return", but "is the world in the goal state"). The
//! suite mixes single-step, explicit multi-step ("X then Y"), and IMPLICIT-discovery tasks (no
//! path given) to honestly map where the agent succeeds vs escalates. Desktop state is reset
//! between runs so each trial starts clean.
//!
//! Usage: stress_test [runs_per_task]   (default 5)
//! Output: /tmp/stress_results.csv  +  a printed summary table.

#[cfg(not(unix))]
fn main() { eprintln!("[stress] Unix required"); }

#[cfg(unix)]
#[tokio::main]
async fn main() {
    use std::process::Command;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    use std::io::Write as _;

    use lagado_agent::inference::llama_cpp::LlamaCppAdapter;
    use lagado_agent::perception::{Actuator, PerceptionCache, Perceptor};
    use lagado_agent::skill_library::SkillLibrary;
    use lagado_agent::vm::{QemuDesktopBackend, QmpClient, SshActuator, SshPerceptor, VmBackend, VmConfig};
    use std::sync::atomic::{AtomicBool, Ordering};
    use lagado_agent::{agent, config, hydra, memory_tiers::MemoryTiers};
    use tokio::sync::mpsc;

    // `verify_cmd`: when Some, success is checked by running this over SSH and matching `success`
    // against its stdout (execution-verified by the world, e.g. a file the agent created) — far
    // stronger than a window-title keyword. When None, `success` is matched against the a11y screen.
    struct Task { name: &'static str, kind: &'static str, goal: &'static str, success: &'static [&'static str], verify_cmd: Option<&'static str> }
    // success keywords are SPECIFIC to the accomplished state (e.g. "laputa@" only appears in a
    // real terminal window title, not in the "Terminal Emulator" menu item).
    let tasks = vec![
        // ── baseline ladder (single → 2-step → implicit discovery) ──
        Task { name: "open-apps-menu",        kind: "single",   goal: "Open the Applications menu", success: &["Run Program"], verify_cmd: None },
        Task { name: "menu-then-terminal",    kind: "2-step",   goal: "Open the Applications menu then launch the Terminal Emulator", success: &["laputa@"], verify_cmd: None },
        Task { name: "menu-then-filemanager", kind: "2-step",   goal: "Open the Applications menu then open the File Manager", success: &["Thunar"], verify_cmd: None },
        Task { name: "menu-then-browser",     kind: "2-step",   goal: "Open the Applications menu then open the Web Browser", success: &["Firefox", "Mozilla"], verify_cmd: None },
        Task { name: "implicit-terminal",     kind: "implicit", goal: "Launch the Terminal Emulator", success: &["laputa@"], verify_cmd: None },
        Task { name: "implicit-browser",      kind: "implicit", goal: "Open the web browser", success: &["Firefox", "Mozilla"], verify_cmd: None },
        Task { name: "implicit-filemanager",  kind: "implicit", goal: "Open the file manager", success: &["Thunar"], verify_cmd: None },
        // ── complexity ladder: action chains (click→type→Enter), world-verified by the filesystem ──
        // These run BEFORE the mail task: a Mail Reader launch pops a "Choose Preferred Application"
        // modal (no mail app configured) that leaks across resets and traps later tasks at 0 clicks.
        // Action chain (Wall 2): explicit click→click→TYPE→Enter, verified by the file it creates.
        // Explicit phrasing spells out the precondition (open menu, click app) so this isolates the
        // type/key execution path, not Wall-1 precondition planning.
        Task { name: "term-type-touch",       kind: "4-step",   goal: "Open the Applications menu, then click the Terminal Emulator, then type the command: touch /tmp/lagado_probe, then press Enter", success: &["/tmp/lagado_probe"], verify_cmd: Some("ls /tmp/lagado_probe 2>/dev/null") },
        // typing chain with content + redirect — verifies the agent typed the whole argument string.
        Task { name: "term-type-echo",        kind: "4-step",   goal: "Open the Applications menu, then click the Terminal Emulator, then type the command: echo lagadomark > /tmp/lagado_echo, then press Enter", success: &["lagadomark"], verify_cmd: Some("cat /tmp/lagado_echo 2>/dev/null") },
        // discrimination stressor LAST (its leaked modal can only poison nothing after it).
        Task { name: "menu-then-mail",        kind: "2-step",   goal: "Open the Applications menu then open the Mail Reader", success: &["Mail", "Thunderbird", "Evolution", "Geary"], verify_cmd: None },
    ];
    let runs: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(5);

    fn ssh_try(port: u16, cmd: &str) -> Option<String> {
        let out = Command::new("ssh").args([
            "-o","StrictHostKeyChecking=no","-o","ConnectTimeout=5","-o","BatchMode=yes",
            "-p",&port.to_string(),"laputa@127.0.0.1",cmd,
        ]).output().ok()?;
        if out.status.success() { Some(String::from_utf8_lossy(&out.stdout).trim().to_string()) } else { None }
    }

    println!("══ STRESS TEST ═════════════════════════════ {} tasks × {runs} runs", tasks.len());

    for (port, name) in [(8080u16, "8B"), (8081, "classifier")] {
        let ok = ureq::get(&format!("http://127.0.0.1:{port}/health")).timeout(Duration::from_secs(3)).call().is_ok();
        println!("[pre] {name} :{port} — {}", if ok { "up" } else { "DOWN" });
        if !ok && port == 8080 { eprintln!("[FAIL] 8B required"); std::process::exit(1); }
    }

    let backend = QemuDesktopBackend::default();
    let cfg = VmConfig::default();
    let port = cfg.ssh_port;
    println!("[vm] booting…");
    let handle = match backend.boot(&cfg) { Ok(h) => h, Err(e) => { eprintln!("[FAIL] boot: {e}"); std::process::exit(1); } };
    let deadline = Instant::now() + Duration::from_secs(240);
    let mut up = false;
    while Instant::now() < deadline {
        if let Some(w) = ssh_try(port, "whoami") { if w.contains("laputa") { up = true; break; } }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
    if !up { eprintln!("[FAIL] ssh never up"); let _ = backend.shutdown(handle); std::process::exit(1); }
    let xd = Instant::now() + Duration::from_secs(90);
    while Instant::now() < xd {
        if ssh_try(port, "DISPLAY=:0 xdotool getdisplaygeometry 2>/dev/null").map(|g| !g.is_empty()).unwrap_or(false) { break; }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
    let _ = Command::new("scp").args(["-o","StrictHostKeyChecking=no","-o","BatchMode=yes","-P",&port.to_string(),"perceive.py","laputa@127.0.0.1:/home/laputa/perceive.py"]).status();
    println!("[vm] up.\n");

    // ── Frame feed: keep FRAME_PATH fresh so the live CV sense runs (headless has no UI to drive
    // the Tauri capture_frame IPC). Owns QMP for the whole session; the agent uses SSH and the
    // success predicates use SSH, so nothing else contends for QMP. Skipped when CV is disabled.
    let feed_stop = Arc::new(AtomicBool::new(false));
    let feed_handle = if config::cv_enabled() {
        let stop = feed_stop.clone();
        let sock = cfg.qmp_socket.clone();
        Some(std::thread::spawn(move || -> u32 {
            let mut qmp = match QmpClient::connect(&sock) { Ok(q) => q, Err(_) => return 0 };
            let mut n = 0u32;
            while !stop.load(Ordering::Relaxed) {
                if qmp.screendump(config::FRAME_PATH).is_ok() { n += 1; }
                std::thread::sleep(Duration::from_millis(600));
            }
            n
        }))
    } else { None };
    println!("[vm] CV sense: {} (frame feed {})",
        if config::cv_enabled() { "ON" } else { "OFF (a11y-only)" },
        if feed_handle.is_some() { "running" } else { "off" });

    let cache = Arc::new(Mutex::new(PerceptionCache::new()));
    let perceptor: Arc<dyn Perceptor> = Arc::new(SshPerceptor::with_cache("127.0.0.1", port, "laputa", cache.clone()));
    let actuator: Arc<dyn Actuator> = Arc::new(SshActuator::with_cache("127.0.0.1", port, "laputa", cache.clone()));
    let adapter: Arc<dyn lagado_agent::inference::InferenceAdapter> =
        Arc::new(LlamaCppAdapter::with_url(&config::llama_base_url(), "LFM2-8B-A1B", config::CONTEXT_SIZE));
    let memory = MemoryTiers::open(&config::data_dir().join("memory.db"))
        .unwrap_or_else(|e| { eprintln!("[FAIL] memory: {e}"); std::process::exit(1); });
    let memory_tiers = Arc::new(tokio::sync::Mutex::new(memory));
    let skill_library = Arc::new(SkillLibrary::open(&config::data_dir()));
    let state = Arc::new(tokio::sync::Mutex::new(agent::AgentState { goal: String::new(), running: false, approval_tx: None, pending_id: None }));

    let csv_path = "/tmp/stress_results.csv";
    let mut csv = std::fs::File::create(csv_path).expect("csv");
    writeln!(csv, "task,kind,run,success,completed,clicks,seconds,final_focus").unwrap();

    // (task_idx) -> (successes, total, avg_secs)
    let mut summary: Vec<(usize, usize)> = vec![(0, 0); tasks.len()];

    for (ti, task) in tasks.iter().enumerate() {
        for run in 0..runs {
            // ── reset desktop to a clean state ──
            // THOROUGH reset: heavier tasks spawn modal dialogs ("Choose Preferred Application")
            // and extra windows that survive a naive pkill and leak into the next task (false
            // pass/fail + 0-click traps). Kill the app set + choosers, then POLITELY close the
            // active window with alt+F4 a few times (dialogs obey it; the xfce4-panel ignores it,
            // so the Applications menu survives — a blanket windowkill would nuke the panel).
            let reset = "DISPLAY=:0 sh -c '\
                for p in xfce4-terminal Thunar thunar firefox Navigator exo-open xfce4-appfinder \
                         thunderbird evolution geary xmessage mousepad ristretto; do pkill -9 -f \"$p\"; done; \
                rm -f /tmp/lagado_probe /tmp/lagado_echo; \
                for i in 1 2 3; do xdotool key --clearmodifiers Escape; xdotool key --clearmodifiers alt+F4; done; \
                xdotool key --clearmodifiers Escape; xdotool mousemove 640 400 click 1' 2>/dev/null; true";
            let _ = ssh_try(port, reset);
            tokio::time::sleep(Duration::from_millis(2200)).await;
            // confirm the desktop is actually clean; if a stray window still has focus, sweep once
            // more so the next task starts from a known state (and log the leak).
            if !perceptor.read_screen().to_lowercase().contains("desktop") {
                let _ = ssh_try(port, reset);
                tokio::time::sleep(Duration::from_millis(1500)).await;
            }

            let (approval_tx, approval_rx) = mpsc::channel::<bool>(8);
            let (confirm_tx, mut confirm_rx) = mpsc::channel::<String>(64);
            { let mut s = state.lock().await; s.goal = String::new(); s.running = false; s.approval_tx = Some(approval_tx.clone()); s.pending_id = None; }

            let approver = approval_tx.clone();
            let listener = tokio::spawn(async move {
                let mut clicks = 0u32; let mut completed = false; let mut detail = String::new();
                while let Some(env) = confirm_rx.recv().await {
                    if env.contains("\"permission\"") { clicks += 1; let _ = approver.send(true).await; }
                    if env.contains("\"status\"") && (env.contains("goal_done") || env.contains("goal_aborted")) {
                        completed = true;
                        if let Some(i) = env.find("\"detail\":\"") {
                            let rest = &env[i + 10..];
                            if let Some(j) = rest.find('"') { detail = rest[..j].to_string(); }
                        }
                    }
                }
                (clicks, completed, detail)
            });

            let t = Instant::now();
            let walk = hydra::run(task.goal.to_string(), String::new(), false, state.clone(),
                adapter.clone(), perceptor.clone(), actuator.clone(), approval_rx, confirm_tx,
                memory_tiers.clone(), None, skill_library.clone());
            let outcome = tokio::time::timeout(Duration::from_secs(120), walk).await;
            let (clicks, completed, _detail) = listener.await.unwrap_or((0, false, String::new()));
            let secs = t.elapsed().as_secs_f32();

            // settle (apps like Firefox take a moment to paint a title) then verify end state.
            tokio::time::sleep(Duration::from_secs(3)).await;
            let final_screen = perceptor.read_screen();
            // EXECUTION-VERIFIED when verify_cmd is set: run it over SSH and match against its
            // stdout (the world's actual state). Otherwise match keywords on the a11y screen.
            let verify_src = match task.verify_cmd {
                Some(cmd) => ssh_try(port, &format!("DISPLAY=:0 {cmd}")).unwrap_or_default(),
                None => final_screen.clone(),
            };
            let lc = verify_src.to_lowercase();
            let success = task.success.iter().any(|k| lc.contains(&k.to_lowercase()));
            let focus = final_screen.lines().find(|l| l.contains("focused:")).map(|l| l.trim().replace(',', " ")).unwrap_or_default();
            let timed_out = outcome.is_err();

            summary[ti].0 += success as usize;
            summary[ti].1 += 1;
            writeln!(csv, "{},{},{},{},{},{},{:.1},{}", task.name, task.kind, run,
                success, completed && !timed_out, clicks, secs, focus).unwrap();
            csv.flush().ok();
            println!("[{:>20} {}/{}] {} | {}clk {:.0}s | {}",
                task.name, run + 1, runs, if success { "✓ PASS" } else { "✗ fail" }, clicks, secs,
                focus.chars().take(48).collect::<String>());
        }
    }

    println!("\n══ SUMMARY ═════════════════════════════════════════");
    for (ti, task) in tasks.iter().enumerate() {
        let (s, n) = summary[ti];
        println!("  {:<22} {:<9} {}/{}  ({:.0}%)", task.name, task.kind, s, n,
            if n > 0 { 100.0 * s as f32 / n as f32 } else { 0.0 });
    }
    let total_s: usize = summary.iter().map(|(s, _)| s).sum();
    let total_n: usize = summary.iter().map(|(_, n)| n).sum();
    println!("  {:-<46}", "");
    println!("  {:<32} {}/{}  ({:.0}%)", "OVERALL", total_s, total_n,
        if total_n > 0 { 100.0 * total_s as f32 / total_n as f32 } else { 0.0 });
    println!("\n[stress] full results: {csv_path}");

    feed_stop.store(true, Ordering::Relaxed);
    if let Some(h) = feed_handle {
        println!("[vm] frame feed: {} screendumps total", h.join().unwrap_or(0));
    }
    let _ = backend.shutdown(handle);
}
