//! osworld_stress — a battery of hard, OSWorld-flavoured tasks run through the FULL autonomous chain
//! (intent router → capability planner → command channel → exit+postcondition verify → reapproach →
//! goal-postcondition). Tasks are NATURAL INTENT (the agent decides HOW) and every pass is checked by
//! WORLD-STATE, not the agent's own say-so. Honest: some are expected to be hard. Run against a live
//! VM (:2222) + brain (:8080). Run: cargo run --bin osworld_stress

#[cfg(not(unix))]
fn main() { eprintln!("[osworld_stress] Unix required"); }

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

    // (name, natural-intent goal, world-state verify -> must print OK)
    let tasks: &[(&str, &str, &str)] = &[
        ("nested-structure",
         "create a directory /tmp/osw_proj and an empty file README.md inside it",
         "test -d /tmp/osw_proj && test -f /tmp/osw_proj/README.md && echo OK"),
        ("bulk-files",
         "create three empty files: /tmp/osw_a, /tmp/osw_b and /tmp/osw_c",
         "test -e /tmp/osw_a && test -e /tmp/osw_b && test -e /tmp/osw_c && echo OK"),
        ("write-content",
         "write the text hello-osworld into the file /tmp/osw_greeting.txt",
         "grep -q hello-osworld /tmp/osw_greeting.txt && echo OK"),
        ("make-executable",
         "create the file /tmp/osw_run.sh and make it executable",
         "test -x /tmp/osw_run.sh && echo OK"),
        ("cleanup",   // depends on bulk-files having made /tmp/osw_a
         "delete the file /tmp/osw_a",
         "test ! -e /tmp/osw_a && echo OK"),
        ("git-init",  // hard: each command runs in a fresh shell (no persistent cwd)
         "create a git repository in /tmp/osw_repo",
         "test -d /tmp/osw_repo/.git && echo OK"),
        ("compress",  // depends on write-content having made the greeting
         "compress the file /tmp/osw_greeting.txt with gzip",
         "test -e /tmp/osw_greeting.txt.gz && echo OK"),
    ];

    // Clean slate.
    let _ = ssh("rm -rf /tmp/osw_proj /tmp/osw_a /tmp/osw_b /tmp/osw_c /tmp/osw_greeting.txt \
                 /tmp/osw_greeting.txt.gz /tmp/osw_run.sh /tmp/osw_repo");

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
    for (name, goal, verify) in tasks {
        println!("\n══════ [{name}]  \"{goal}\"");
        let final_status = run_goal(goal.to_string()).await;
        let ok = ssh(verify).contains("OK");
        let claimed = final_status.contains("accomplished");
        if ok { passed += 1; }
        println!("   world-state: {}   | agent claimed: {}",
                 if ok { "✅ PASS" } else { "❌ FAIL" },
                 if claimed { "accomplished" } else { "handed back" });
        // The integrity check the whole session is about: claim must MATCH reality.
        if claimed && !ok { println!("   ⚠ FALSE SUCCESS — agent claimed done but world-state says no"); }
        if !claimed && ok { println!("   (under-claimed: world-state OK but agent handed back)"); }
    }
    println!("\n══════ OSWorld battery: {passed}/{} world-state-verified", tasks.len());
}
