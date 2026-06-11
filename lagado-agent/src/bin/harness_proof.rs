//! harness_proof — end-to-end VM control test through the real agent modules.
//!
//! Boots a fresh QEMU desktop VM via QemuDesktopBackend, waits for the guest's
//! sshd, then drives perception (SshPerceptor → AT-SPI2 → PerceptionCache),
//! frame capture (QMP screendump), and actuation (SshActuator → xdotool) using
//! the actual library code — not hand-rolled SSH. Closes the loop by diffing the
//! pre/post screendumps with the perception-fusion DeltaDetector (FrameProcessor).
//!
//! Throwaway integration harness. Cleans up the VM on exit.

#[cfg(not(unix))]
fn main() {
    eprintln!("[harness_proof] Unix required");
}

#[cfg(unix)]
fn main() {
    use std::process::Command;
    use std::sync::{Arc, Mutex};
    use std::thread::sleep;
    use std::time::{Duration, Instant};

    use lagado_agent::perception::frame::FrameProcessor;
    use lagado_agent::perception::{Actuator, Perceptor, PerceptionCache};
    use lagado_agent::vm::{
        QemuDesktopBackend, QmpClient, SshActuator, SshPerceptor, VmBackend, VmConfig,
    };

    fn stage(n: u32, msg: &str) {
        println!("\n── STAGE {n} ──────────────────────────────────────────\n{msg}");
    }

    // Probe SSH the exact way the agent does (BatchMode key auth). Returns the
    // stdout of `cmd`, or None on any failure/timeout.
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

    let t0 = Instant::now();
    let backend = QemuDesktopBackend::default();
    let cfg = VmConfig::default();
    let port = cfg.ssh_port;
    let qmp_socket = cfg.qmp_socket.clone();

    stage(1, &format!("Booting fresh VM (disk={}, ssh_port={port})", cfg.disk_image));
    let handle = match backend.boot(&cfg) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[FAIL] boot: {e}");
            std::process::exit(1);
        }
    };
    println!("[ok] qemu spawned in {:?}, pid {}", t0.elapsed(), handle.child.id());

    // ── STAGE 2: wait for guest sshd to actually answer (not just port open) ──
    stage(2, "Waiting for guest sshd (real banner, not just TCP)…");
    let boot_deadline = Instant::now() + Duration::from_secs(330);
    let mut ssh_up = false;
    while Instant::now() < boot_deadline {
        if let Some(who) = ssh_try(port, "whoami") {
            if who.contains("laputa") {
                println!("[ok] sshd answered after {:?}: whoami={who}", t0.elapsed());
                ssh_up = true;
                break;
            }
        }
        print!(".");
        use std::io::Write;
        let _ = std::io::stdout().flush();
        sleep(Duration::from_secs(3));
    }
    if !ssh_up {
        eprintln!("\n[FAIL] guest sshd never answered within 240s");
        let _ = backend.shutdown(handle);
        std::process::exit(1);
    }

    // Wait for the X session so DISPLAY=:0 tools work.
    stage(3, "Waiting for X session (DISPLAY=:0)…");
    let mut x_up = false;
    let x_deadline = Instant::now() + Duration::from_secs(90);
    while Instant::now() < x_deadline {
        if let Some(geo) = ssh_try(port, "DISPLAY=:0 xdotool getdisplaygeometry 2>/dev/null") {
            if !geo.is_empty() {
                println!("[ok] X up, display geometry: {geo}");
                x_up = true;
                break;
            }
        }
        print!(".");
        use std::io::Write;
        let _ = std::io::stdout().flush();
        sleep(Duration::from_secs(3));
    }
    if !x_up {
        println!("[warn] X session not confirmed; perception/actuation may be degraded");
    }

    // Deploy the repo's perceive.py to the guest so --focused emits screen coords.
    // Run from the repo root (CWD when invoked as ./target/debug/harness_proof).
    let perceive_src = std::env::current_dir()
        .map(|d| d.join("perceive.py"))
        .unwrap_or_else(|_| std::path::PathBuf::from("perceive.py"));
    if perceive_src.exists() {
        let ok = Command::new("scp")
            .args([
                "-o", "StrictHostKeyChecking=no",
                "-o", "BatchMode=yes",
                "-P", &port.to_string(),        // scp uses -P (capital) for port
                perceive_src.to_str().unwrap_or("perceive.py"),
                "laputa@127.0.0.1:/home/laputa/perceive.py",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            println!("[ok] perceive.py deployed to guest");
        } else {
            println!("[warn] perceive.py scp failed — guest may have a stale copy");
        }
    } else {
        println!("[warn] perceive.py not found at {} — using guest's existing copy",
                 perceive_src.display());
    }

    // Open Thunar (file manager) in the guest so --focused has a window with
    // real interactive AT-SPI2 elements (buttons, tree items, toolbar).
    // Thunar is the verified test app (background facts: "Thunar at X=635,Y=315").
    // Redirect all FDs so SSH exits immediately without waiting for the app.
    let _ = Command::new("ssh")
        .args([
            "-o", "StrictHostKeyChecking=no",
            "-o", "ConnectTimeout=5",
            "-o", "BatchMode=yes",
            "-p", &port.to_string(),
            "laputa@127.0.0.1",
            "DISPLAY=:0 setsid thunar </dev/null >/dev/null 2>&1 &",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    sleep(Duration::from_secs(5));  // Thunar needs ~5s to fully render its a11y tree

    // Shared cache: perceptor populates coords/bboxes, actuator resolves clicks from it.
    let cache = Arc::new(Mutex::new(PerceptionCache::new()));
    let perceptor = SshPerceptor::with_cache("127.0.0.1", port, "laputa", cache.clone());
    let actuator = SshActuator::with_cache("127.0.0.1", port, "laputa", cache.clone());

    // ── STAGE 4: perception through the agent ──
    stage(4, "Perception: SshPerceptor::read_screen() → AT-SPI2 → cache");
    let screen = perceptor.read_screen();
    let (n_coords, n_bboxes) = {
        let c = cache.lock().unwrap();
        (c.coords.len(), c.bboxes.len())
    };
    println!("[perceive] {} bytes of screen text", screen.len());
    println!("[perceive] cache: {n_coords} coords, {n_bboxes} bboxes");
    for line in screen.lines().take(8) {
        println!("    | {line}");
    }
    if n_coords == 0 {
        eprintln!("[FAIL] STAGE 4: coords cache is empty after read_screen() — \
                   --focused emitted no parseable (x,y,w,h) tuples");
        let _ = backend.shutdown(handle);
        std::process::exit(1);
    }

    // ── STAGE 5: frame capture via QMP ──
    stage(5, "Frame capture: QMP screendump → PNG");
    let frame_before = "/dev/shm/lagado_proof_before.png";
    let frame_after = "/dev/shm/lagado_proof_after.png";
    let mut qmp = match QmpClient::connect(&qmp_socket) {
        Ok(q) => q,
        Err(e) => {
            eprintln!("[FAIL] QMP connect: {e}");
            let _ = backend.shutdown(handle);
            std::process::exit(1);
        }
    };
    match qmp.screendump(frame_before) {
        Ok(()) => {
            sleep(Duration::from_millis(400));
            let sz = std::fs::metadata(frame_before).map(|m| m.len()).unwrap_or(0);
            println!("[ok] screendump before: {sz} bytes at {frame_before}");
        }
        Err(e) => println!("[warn] screendump before failed: {e}"),
    }

    // ── STAGE 6: actuation through the agent ──
    stage(6, "Actuation: SshActuator (click cached element + induce visible change)");
    // 6a: test the cache-resolution click path (historically the broken bit).
    let first_ref = {
        let c = cache.lock().unwrap();
        c.coords.keys().next().cloned()
    };
    match &first_ref {
        Some(r) => {
            let res = actuator.click(r);
            // Print verbatim — the raw string from SshActuator is the ground truth
            println!("[click] click('{r}') → {res}");
            if res.contains("not in screen cache") || res.starts_with("ssh error") {
                println!("[warn] click path returned an error string");
            } else {
                println!("[ok] click dispatched through coord cache");
            }
        }
        None => println!("[warn] no cached coords to click; skipping click-by-selector test"),
    }
    // 6b: induce a deterministic visible change so the delta is meaningful.
    let _ = actuator.key("super");
    sleep(Duration::from_millis(600));

    match qmp.screendump(frame_after) {
        Ok(()) => {
            sleep(Duration::from_millis(400));
            let sz = std::fs::metadata(frame_after).map(|m| m.len()).unwrap_or(0);
            println!("[ok] screendump after: {sz} bytes at {frame_after}");
        }
        Err(e) => println!("[warn] screendump after failed: {e}"),
    }

    // ── STAGE 7: close the loop — diff frames with the fusion DeltaDetector ──
    stage(7, "Loop closure: FrameProcessor delta(before, after)");
    if let (Ok(b), Ok(a)) = (std::fs::read(frame_before), std::fs::read(frame_after)) {
        let mut fp = FrameProcessor::new();
        match fp.process_frame(&b) {
            Ok(first) => {
                println!("[delta] baseline frame: {} cells", first.len());
                match fp.process_frame(&a) {
                    Ok(changed) => {
                        println!("[delta] {} cells changed between before/after", changed.len());
                        if changed.is_empty() {
                            // WARN not FAIL: some clicks (e.g. on already-focused
                            // elements) produce no visible pixel change.
                            println!("[warn] 0 changed cells — click may not have altered the screen");
                        } else {
                            println!("[ok] actuation produced a visible screen change — loop closed");
                        }
                    }
                    Err(e) => println!("[warn] delta after: {e}"),
                }
            }
            Err(e) => println!("[warn] delta before: {e}"),
        }
    } else {
        println!("[warn] could not read both frames for delta");
    }

    // ── Cleanup ──
    stage(8, "Clean shutdown via backend.shutdown()");
    match backend.shutdown(handle) {
        Ok(()) => println!("[ok] VM shut down cleanly"),
        Err(e) => println!("[warn] shutdown: {e}"),
    }
    println!("\n[harness_proof] total elapsed {:?}", t0.elapsed());
}
