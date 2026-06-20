//! user_stress — the EXTENSION-OF-THE-USER battery. Document/file-management tasks stated the way a
//! real (non-developer) user would say them, operating on the USER'S OWN folders (~/Documents,
//! ~/Downloads) — NOT dev scratch, NOT git. Every pass is checked by WORLD-STATE (the user's files are
//! now the way they wanted them), never the agent's say-so. This replaces the dev-flavoured osworld
//! battery: Lagado is a sovereign computer-use agent for regulated work, not a coding tool, so the
//! benchmark measures the product. Run against a live VM (:2222) + brain (:8080):
//!   cargo run --bin user_stress

#[cfg(not(unix))]
fn main() { eprintln!("[user_stress] Unix required"); }

#[cfg(unix)]
#[tokio::main]
async fn main() {
    use std::process::Command;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use lagado_agent::inference::llama_cpp::LlamaCppAdapter;
    use lagado_agent::perception::{Actuator, PerceptionCache, Perceptor};
    use lagado_agent::skill_library::SkillLibrary;
    use lagado_agent::vm::{SshActuator, SshPerceptor};
    use lagado_agent::{agent, config, hydra, memory_tiers::MemoryTiers};
    use tokio::sync::mpsc;

    let port: u16 = 2222;
    let ssh = |cmd: &str| -> String {
        Command::new("ssh").args(["-o","StrictHostKeyChecking=no","-o","UserKnownHostsFile=/dev/null",
            "-o","BatchMode=yes","-o","ConnectTimeout=5","-p",&port.to_string(),"laputa@127.0.0.1",cmd])
            .output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
    };
    if !ssh("whoami").contains("laputa") { eprintln!("[FAIL] VM not on :2222"); std::process::exit(1); }

    // The user's world, stated as the user would state it. Verify uses ABSOLUTE paths (the ssh shell
    // runs as laputa, so ~ == /home/laputa); the agent must resolve "my Documents" itself.
    // EACH TASK IS ISOLATED: its own `setup` rebuilds exactly the files it needs from scratch, so one
    // task's effect can NEVER cascade into another (the 1/8 run was a shared-state cascade, not a
    // terminal failure). `RST` wipes both dirs first. Verifies are STRICT and decoy-bearing: a file
    // that must STAY proves the agent didn't over-act (e.g. organize-pdfs must not move notes.txt;
    // tidy-downloads must not delete keep.txt) — so nothing passes vacuously.
    // (name, natural-intent goal, per-task setup, world-state verify -> must print OK)
    const RST: &str = "rm -rf ~/Documents ~/Downloads; mkdir -p ~/Documents ~/Downloads; ";
    let tasks: &[(&str, &str, &str, &str)] = &[
        ("organize-pdfs",
         "move all the PDF files from my Downloads folder into my Documents folder",
         "printf r > ~/Downloads/report_jan.pdf; printf r > ~/Downloads/report_feb.pdf; printf n > ~/Downloads/notes.txt",
         "test -f ~/Documents/report_jan.pdf && test -f ~/Documents/report_feb.pdf && \
          test ! -e ~/Downloads/report_jan.pdf && test -f ~/Downloads/notes.txt && echo OK"),
        ("new-folder-and-collect",
         "make a folder called Scans in my Documents and move my scan images into it",
         "printf i > ~/Downloads/scan_001.jpg; printf i > ~/Downloads/scan_002.jpg",
         "test -d ~/Documents/Scans && test -f ~/Documents/Scans/scan_001.jpg && \
          test -f ~/Documents/Scans/scan_002.jpg && echo OK"),
        ("rename-for-clarity",
         "rename the notes file in my Downloads to meeting_notes.txt",
         "printf n > ~/Downloads/notes.txt",
         "test -f ~/Downloads/meeting_notes.txt && test ! -e ~/Downloads/notes.txt && echo OK"),
        ("copy-to-records",
         "put a copy of Smith's intake form into my Documents Records folder",
         "mkdir -p ~/Documents/Records; printf i > ~/Documents/smith_intake.txt",
         "test -f ~/Documents/Records/smith_intake.txt && test -f ~/Documents/smith_intake.txt && echo OK"),
        ("gather-patient-files",
         "make a folder called Smith in my Documents and move all of Smith's files into it",
         "printf i > ~/Documents/smith_intake.txt; printf n > ~/Documents/smith_notes.txt",
         "test -d ~/Documents/Smith && test -f ~/Documents/Smith/smith_intake.txt && \
          test -f ~/Documents/Smith/smith_notes.txt && echo OK"),
        ("extract-figure",  // document data task: read a value out of a report, write it where asked
         "open the monthly report in my Documents and save just the total figure to a file called total.txt in my Documents",
         "printf 'Monthly patient visits report\\nTotal: 4200\\n' > ~/Documents/monthly_report.txt",
         "grep -q 4200 ~/Documents/total.txt && echo OK"),
        ("count-documents",
         "count how many .txt files are in my Documents folder and write the number to count.txt in my Documents",
         "printf a > ~/Documents/a.txt; printf b > ~/Documents/b.txt; printf c > ~/Documents/c.txt",
         "test -s ~/Documents/count.txt && grep -qE '[0-9]' ~/Documents/count.txt && echo OK"),
        ("tidy-downloads",
         "delete any leftover image files in my Downloads folder",
         "printf i > ~/Downloads/scan_001.jpg; printf i > ~/Downloads/scan_002.jpg; printf k > ~/Downloads/keep.txt",
         "test -z \"$(ls ~/Downloads/*.jpg 2>/dev/null)\" && test -f ~/Downloads/keep.txt && echo OK"),
    ];

    let run_goal = |goal: String| {
        let cache = Arc::new(Mutex::new(PerceptionCache::new()));
        let perceptor: Arc<dyn Perceptor> = Arc::new(SshPerceptor::with_cache("127.0.0.1", port, "laputa", cache.clone()));
        let actuator: Arc<dyn Actuator> = Arc::new(SshActuator::with_cache("127.0.0.1", port, "laputa", cache.clone()));
        let adapter: Arc<dyn lagado_agent::inference::InferenceAdapter> =
            Arc::new(LlamaCppAdapter::with_url(&config::llama_base_url(), "LFM2-8B-A1B", config::CONTEXT_SIZE));
        let memory_tiers = Arc::new(tokio::sync::Mutex::new(MemoryTiers::open(&config::data_dir().join("memory.db")).expect("memory")));
        let skill_library = Arc::new(SkillLibrary::open(&config::data_dir()));
        let state = Arc::new(tokio::sync::Mutex::new(agent::AgentState { goal: String::new(), running: false, approval_tx: None, pending_id: None }));
        async move {
            let (approval_tx, approval_rx) = mpsc::channel::<bool>(8);
            let (confirm_tx, mut confirm_rx) = mpsc::channel::<String>(64);
            { state.lock().await.approval_tx = Some(approval_tx.clone()); }
            let approver = approval_tx.clone();
            let listener = tokio::spawn(async move {
                let mut last = String::new();
                while let Some(env) = confirm_rx.recv().await {
                    if env.contains("\"permission\"") { let _ = approver.send(true).await; }
                    if env.contains("\"status\"") { last = env.chars().take(160).collect(); }
                }
                last
            });
            let route = hydra::RouteContext { surface: hydra::SurfaceState { vm_active: true, ..Default::default() }, mode: hydra::RouteMode::Auto };
            let walk = hydra::run(goal, String::new(), route, state.clone(), adapter.clone(),
                perceptor.clone(), actuator.clone(), approval_rx, confirm_tx, memory_tiers.clone(), None, skill_library.clone());
            let _ = tokio::time::timeout(Duration::from_secs(75), walk).await;
            listener.await.unwrap_or_default()
        }
    };

    let mut passed = 0;
    for (name, goal, setup, verify) in tasks {
        println!("\n══════ [{name}]  \"{goal}\"");
        let _ = ssh(&format!("{RST}{setup}; true"));  // isolated per-task starting state
        let final_status = run_goal(goal.to_string()).await;
        let ok = ssh(verify).contains("OK");
        let claimed = final_status.contains("accomplished");
        if ok { passed += 1; }
        println!("   world-state: {}   | agent claimed: {}",
                 if ok { "✅ PASS" } else { "❌ FAIL" },
                 if claimed { "accomplished" } else { "handed back" });
        if claimed && !ok { println!("   ⚠ FALSE SUCCESS — agent claimed done but the user's files say otherwise"); }
        if !claimed && ok { println!("   (under-claimed: world-state OK but agent handed back)"); }
    }
    println!("\n══════ user battery: {passed}/{} world-state-verified", tasks.len());
}
