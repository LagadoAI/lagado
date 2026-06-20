//! osworld_heldout — the TRANSFER test. Same capability CLASSES as osworld_stress, but every goal is
//! phrased DIFFERENTLY from both the planner-prompt examples AND the osworld_stress wording (different
//! verbs: "make a folder"/"set up version control"/"mark it runnable"/"remove"/"put"; different paths +
//! filenames). If a task passes here it's a STRUCTURAL win; if it only passed osworld_stress it was an
//! answer-key match (teaching-to-the-test). This is the honest robustness number. Same production chain
//! (hydra::run), world-state verified. Run against a live VM (:2222) + brain (:8080). Run:
//! cargo run --bin osworld_heldout   (OSW_TRACE=1 to see each action).

#[cfg(not(unix))]
fn main() { eprintln!("[osworld_heldout] Unix required"); }

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

    // (name [↔ the osworld_stress class it mirrors], NATURAL-INTENT goal in DIFFERENT words, world-state verify)
    let tasks: &[(&str, &str, &str)] = &[
        ("make-folder+file ↔ nested-structure",
         "make a folder /tmp/hx_reports with an empty summary.csv inside it",
         "test -d /tmp/hx_reports && test -f /tmp/hx_reports/summary.csv && echo OK"),
        ("put-text ↔ write-content",
         "put the text greetings-earth into /tmp/hx_hello.txt",
         "grep -q greetings-earth /tmp/hx_hello.txt && echo OK"),
        ("remove ↔ cleanup",
         "remove the file /tmp/hx_junk.txt",
         "test ! -e /tmp/hx_junk.txt && echo OK"),
        ("runnable-mark ↔ make-executable",
         "create /tmp/hx_tool.sh and mark it runnable",
         "test -x /tmp/hx_tool.sh && echo OK"),
        ("version-control ↔ git-init",   // avoids the words "git repository"
         "set up version control in /tmp/hx_proj",
         "test -d /tmp/hx_proj/.git && echo OK"),
        ("count-lines ↔ compute→write",
         "count the number of lines in /etc/hostname and save the result to /tmp/hx_lines.txt",
         "test -s /tmp/hx_lines.txt && grep -qE '[0-9]' /tmp/hx_lines.txt && echo OK"),
        ("author+run ↔ author+execute",  // "writes done to"/"mark it runnable"/"execute" — different verbs
         "create /tmp/hx_build.sh that writes done to /tmp/hx_status, mark it runnable, and execute it",
         "test -x /tmp/hx_build.sh && grep -q done /tmp/hx_status && echo OK"),
        ("stage+commit ↔ git-workflow",  // different filename + message
         "in /tmp/hx_repo start a git repo, stage a file called data.log, and commit it with message saved-data",
         "git -C /tmp/hx_repo ls-files | grep -q data.log && git -C /tmp/hx_repo log --oneline 2>/dev/null | grep -q . && echo OK"),
    ];

    // Clean slate; pre-create the file the 'remove' task deletes; git identity so commits CAN succeed.
    let _ = ssh("rm -rf /tmp/hx_reports /tmp/hx_hello.txt /tmp/hx_junk.txt /tmp/hx_tool.sh /tmp/hx_proj \
                 /tmp/hx_lines.txt /tmp/hx_build.sh /tmp/hx_status /tmp/hx_repo; \
                 printf 'scratch\\n' > /tmp/hx_junk.txt; \
                 git config --global user.email lagado@test.local; git config --global user.name Lagado; true");

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
                    if std::env::var("OSW_TRACE").is_ok() && (env.contains("action_log") || env.contains("\"status\"")) {
                        eprintln!("   · {}", env.chars().take(300).collect::<String>());
                    }
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
    let mut confirmed = 0;
    for (name, goal, verify) in tasks {
        println!("\n══════ [{name}]  \"{goal}\"");
        let final_status = run_goal(goal.to_string()).await;
        let ok = ssh(verify).contains("OK");
        let claimed = final_status.contains("accomplished");
        if ok { passed += 1; }
        if ok && claimed { confirmed += 1; }
        println!("   world-state: {}   | agent claimed: {}",
                 if ok { "✅ PASS" } else { "❌ FAIL" },
                 if claimed { "accomplished" } else { "handed back" });
        if claimed && !ok { println!("   ⚠ FALSE SUCCESS — agent claimed done but world-state says no"); }
        if !claimed && ok { println!("   (under-claimed: world-state OK but agent handed back)"); }
    }
    println!("\n══════ HELD-OUT (transfer) battery: {passed}/{} world-state | {confirmed}/{} agent-confirmed",
             tasks.len(), tasks.len());
}
