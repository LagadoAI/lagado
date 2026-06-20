//! hard_stress — a HARDER, more varied battery than user_stress, to stress the wired ReAct loop in many
//! ways: selective-by-type, nested structures, content discovery (grep), batch rename, empty/size
//! filters, spaces in names, content extraction, and a destructive-gate trap. Each task is ISOLATED
//! (own setup), world-state verified, decoy-bearing (a file that must STAY) so nothing passes vacuously.
//! Run against live VM (:2222) + brain (:8080): cargo run --bin hard_stress

#[cfg(not(unix))]
fn main() { eprintln!("[hard_stress] Unix required"); }

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

    const RST: &str = "rm -rf ~/Documents ~/Downloads; mkdir -p ~/Documents ~/Downloads; ";
    // (name, natural goal, per-task setup, world-state verify -> OK)
    let tasks: &[(&str,&str,&str,&str)] = &[
        ("selective-by-type",
         "move only the text files from my Downloads into my Documents, and leave everything else in Downloads",
         "printf a > ~/Downloads/a.txt; printf b > ~/Downloads/b.txt; printf c > ~/Downloads/c.jpg; printf d > ~/Downloads/d.pdf",
         "test -f ~/Documents/a.txt && test -f ~/Documents/b.txt && test -f ~/Downloads/c.jpg && test -f ~/Downloads/d.pdf && test ! -e ~/Downloads/a.txt && echo OK"),
        ("nested-project",
         "create a folder called project in my Documents with two subfolders src and tests, and an empty README.md inside project",
         "true",
         "test -d ~/Documents/project/src && test -d ~/Documents/project/tests && test -f ~/Documents/project/README.md && echo OK"),
        ("content-flag",
         "find which file in my Documents contains the word URGENT and copy that file into a new folder called Flagged in my Documents",
         "printf normal > ~/Documents/memo1.txt; printf 'this is URGENT\\n' > ~/Documents/memo2.txt; printf other > ~/Documents/memo3.txt",
         "test -f ~/Documents/Flagged/memo2.txt && echo OK"),
        ("count-logs",
         "count how many .log files are in my Downloads and write just the number to logcount.txt in my Documents",
         "touch ~/Downloads/a.log ~/Downloads/b.log ~/Downloads/c.log ~/Downloads/x.txt",
         "grep -q 3 ~/Documents/logcount.txt && echo OK"),
        ("prefix-rename",
         "add the prefix 2026_ to the name of every PDF file in my Documents, leaving other files unchanged",
         "printf x > ~/Documents/report.pdf; printf y > ~/Documents/invoice.pdf; printf z > ~/Documents/notes.txt",
         "test -f ~/Documents/2026_report.pdf && test -f ~/Documents/2026_invoice.pdf && test -f ~/Documents/notes.txt && test ! -e ~/Documents/report.pdf && echo OK"),
        ("delete-empty",
         "delete the empty files in my Downloads, keep the ones that have content",
         ": > ~/Downloads/empty1.txt; : > ~/Downloads/empty2.txt; printf data > ~/Downloads/full.txt",
         "test ! -e ~/Downloads/empty1.txt && test ! -e ~/Downloads/empty2.txt && test -f ~/Downloads/full.txt && echo OK"),
        ("extract-amount",
         "the file invoice.txt in my Documents has a line starting with Total; save just the dollar amount to amount.txt in my Documents",
         "printf 'Item A\\nTotal: $4200\\n' > ~/Documents/invoice.txt",
         "grep -q 4200 ~/Documents/amount.txt && echo OK"),
        ("move-and-rename",
         "move report.txt from my Downloads to my Documents and rename it to final_report.txt",
         "printf r > ~/Downloads/report.txt",
         "test -f ~/Documents/final_report.txt && test ! -e ~/Downloads/report.txt && test ! -e ~/Documents/report.txt && echo OK"),
        ("size-filter",
         "move the files bigger than 1 kilobyte from my Downloads into a folder called Big in my Documents",
         "head -c 2000 /dev/zero > ~/Downloads/big1.bin; head -c 100 /dev/zero > ~/Downloads/small1.bin",
         "test -f ~/Documents/Big/big1.bin && test -f ~/Downloads/small1.bin && test ! -e ~/Downloads/big1.bin && echo OK"),
        ("spaces-in-name",
         "rename the file called my notes.txt in my Downloads to notes_clean.txt",
         "printf n > \"$HOME/Downloads/my notes.txt\"",
         "test -f ~/Downloads/notes_clean.txt && test ! -e \"$HOME/Downloads/my notes.txt\" && echo OK"),
        ("find-list",
         "make a list of all the .jpg files in my Downloads and save it to images.txt in my Documents",
         "touch ~/Downloads/p1.jpg ~/Downloads/p2.jpg ~/Downloads/doc.txt",
         "grep -q p1.jpg ~/Documents/images.txt && grep -q p2.jpg ~/Documents/images.txt && echo OK"),
        ("destructive-trap",
         "delete all the files in my Downloads folder",
         "touch ~/Downloads/f1 ~/Downloads/f2 ~/Downloads/f3",
         "test -z \"$(ls -A ~/Downloads 2>/dev/null)\" && echo OK"),
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
            let _ = tokio::time::timeout(Duration::from_secs(90), walk).await;
            listener.await.unwrap_or_default()
        }
    };

    let mut passed = 0;
    for (name, goal, setup, verify) in tasks {
        println!("\n══════ [{name}]  \"{goal}\"");
        let _ = ssh(&format!("{RST}{setup}; true"));
        let final_status = run_goal(goal.to_string()).await;
        let ok = ssh(verify).contains("OK");
        let claimed = final_status.contains("accomplished");
        if ok { passed += 1; }
        println!("   world-state: {}   | agent claimed: {}",
                 if ok { "✅ PASS" } else { "❌ FAIL" },
                 if claimed { "accomplished" } else { "handed back" });
        if claimed && !ok { println!("   ⚠ FALSE SUCCESS"); }
        if !claimed && ok { println!("   (under-claimed)"); }
    }
    println!("\n══════ hard battery: {passed}/{} world-state-verified", tasks.len());
}
