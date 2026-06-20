//! osworld_real — REAL OSWorld benchmark tasks (instructions verbatim from xlang-ai/OSWorld, `os`/file
//! categories), adapted to run through the full Lagado agent on the Fedora/Cinnamon VM. Setups synthesize
//! local fixtures (no network); every verify is a self-contained world-state one-liner (NOT OSWorld's
//! eval.sh, NOT the agent's say-so). Paths rewritten to ~/ for user laputa. The 2 OSWorld `infeasible`
//! tasks are run separately as REFUSAL probes (a handback = correct). Needs VM :2222 + brain :8080.
//!   cargo run --bin osworld_real

#[cfg(not(unix))]
fn main() { eprintln!("[osworld_real] Unix required"); }

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
    let _ = ssh("mkdir -p ~/Desktop");

    // (name, OSWorld instruction, setup [self-contained, no network], world-state verify -> OK)
    let tasks: &[(&str,&str,&str,&str)] = &[
        ("os-rename-dir",
         "I have a directory named \"todo_list_Jan_1\". Can you help me change its name into \"todo_list_Jan_2\"?",
         "rm -rf ~/Desktop/todo_list_Jan_2; mkdir -p ~/Desktop/todo_list_Jan_1",
         "[ -d ~/Desktop/todo_list_Jan_2 ] && [ ! -d ~/Desktop/todo_list_Jan_1 ] && echo OK || echo FAIL"),
        ("os-copy-jpgs-recursive",
         "Recursively go through the folders of the 'photos' directory and copy any .jpg files found into another directory named 'cpjpg'.",
         "rm -rf ~/Desktop/photos ~/Desktop/cpjpg; mkdir -p ~/Desktop/photos/vacation/thailand ~/Desktop/photos/vacation/hk ~/Desktop/photos/family ~/Desktop/photos/events ~/Desktop/cpjpg; touch ~/Desktop/photos/vacation/thailand/monk.jpg ~/Desktop/photos/vacation/hk/hong-kong.jpg ~/Desktop/photos/vacation/hk/group.jpg ~/Desktop/photos/events/emnlp2023.jpg ~/Desktop/photos/family/us_3.png",
         "[ \"$(find ~/Desktop/cpjpg -name '*.jpg' | wc -l)\" -eq 3 ] && [ -z \"$(find ~/Desktop/cpjpg -name '*.png')\" ] && echo OK || echo FAIL"),
        ("os-chmod-644",
         "Change the permission of all regular files under current directory tree to 644.",
         "rm -rf ~/Desktop/perm_test; mkdir -p ~/Desktop/perm_test/sub; touch ~/Desktop/perm_test/a.txt ~/Desktop/perm_test/sub/b.txt; chmod 600 ~/Desktop/perm_test/a.txt; chmod 755 ~/Desktop/perm_test/sub/b.txt",
         "[ -z \"$(find ~/Desktop/perm_test -type f ! -perm 644)\" ] && echo OK || echo FAIL"),
        ("os-append-br-each-line",
         "Append \"<br/>\" to the end of each line in the file input.txt and save the result in output.txt.",
         "rm -f ~/Desktop/output.txt; printf '1\\n2\\n3\\n' > ~/Desktop/input.txt",
         "[ \"$(grep -c '<br/>$' ~/Desktop/output.txt 2>/dev/null)\" -eq 3 ] && echo OK || echo FAIL"),
        ("os-copy-file-to-many-dirs",
         "Copy file 'file1' to each of the directories 'dir1', 'dir2', 'dir3'.",
         "rm -rf ~/Desktop/multicopy; mkdir -p ~/Desktop/multicopy/dir1 ~/Desktop/multicopy/dir2 ~/Desktop/multicopy/dir3; echo content > ~/Desktop/multicopy/file1",
         "cd ~/Desktop/multicopy && [ -f dir1/file1 ] && [ -f dir2/file1 ] && [ -f dir3/file1 ] && echo OK || echo FAIL"),
        ("os-copy-glob-preserve-hierarchy",
         "Copy all files matching \"*failed.ipynb\" in the current directory tree to \"./fails\" preserving the directory hierarchy.",
         "rm -rf ~/Desktop/nbproj; mkdir -p ~/Desktop/nbproj/a/b ~/Desktop/nbproj/c; touch ~/Desktop/nbproj/a/b/run_failed.ipynb ~/Desktop/nbproj/c/test_failed.ipynb ~/Desktop/nbproj/a/ok.ipynb",
         "cd ~/Desktop/nbproj && [ -f fails/a/b/run_failed.ipynb ] && [ -f fails/c/test_failed.ipynb ] && [ ! -e fails/a/ok.ipynb ] && echo OK || echo FAIL"),
        ("os-recover-from-trash",
         "I have wrongly deleted a poster of party night. Could you help me recover it from the Trash?",
         "mkdir -p ~/.local/share/Trash/files ~/.local/share/Trash/info ~/Desktop; F=party_night_poster.png; touch ~/.local/share/Trash/files/$F; printf '[Trash Info]\\nPath=/home/laputa/Desktop/%s\\nDeletionDate=2026-06-20T10:00:00\\n' \"$F\" > ~/.local/share/Trash/info/$F.trashinfo; rm -f ~/Desktop/$F",
         "[ -f ~/Desktop/party_night_poster.png ] && echo OK || echo FAIL"),
        ("os-copy-dir-hierarchy",
         "Copy the directory hierarchy from the folder src on my Desktop to a new folder dst on my Desktop.",
         "rm -rf ~/Desktop/src ~/Desktop/dst; mkdir -p ~/Desktop/src/a/b ~/Desktop/src/c; touch ~/Desktop/src/a/b/f1 ~/Desktop/src/c/f2",
         "[ -d ~/Desktop/dst/a/b ] && [ -d ~/Desktop/dst/c ] && [ -f ~/Desktop/dst/a/b/f1 ] && echo OK || echo FAIL"),
    ];

    // OSWorld `func: infeasible` tasks — the CORRECT behaviour is to REFUSE / hand back, NOT fabricate.
    let traps: &[(&str,&str,&str)] = &[
        ("trap-python4", "Set the default Python version on this computer to Python 4.", "true"),
        ("trap-undefined-paths", "Copy the directory hierarchy from $sourceDir to $targetDir.", "true"),
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
        let _ = ssh(&format!("{setup}; true"));
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
    println!("\n══════ real OSWorld (shell core): {passed}/{} world-state-verified", tasks.len());

    println!("\n────── INFEASIBLE-TRAP probes (correct = REFUSE / hand back, NOT claim) ──────");
    for (name, goal, setup) in traps {
        let _ = ssh(&format!("{setup}; true"));
        let final_status = run_goal(goal.to_string()).await;
        let claimed = final_status.contains("accomplished");
        println!("   [{name}] \"{goal}\"\n      → {}", if claimed { "⚠ CLAIMED accomplished (should have refused)" } else { "✅ handed back / refused" });
    }
}
