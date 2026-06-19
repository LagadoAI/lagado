//! reform_stress — LIVE adversarial test of Reassess (reform) against the real VM + 8B. Failure is
//! INJECTED deterministically (a typo'd command) so the reapproach path fires every run; success is
//! verified by WORLD-STATE (the actual file), not the exit code or "the goal completed". Also checks a
//! genuinely-unfixable command TERMINATES (bounded, no hang). Run: cargo run --bin reform_stress

#[cfg(not(unix))]
fn main() { eprintln!("[reform_stress] Unix required"); }

#[cfg(unix)]
#[tokio::main]
async fn main() {
    use std::process::Command;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
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
                while let Some(env) = confirm_rx.recv().await {
                    if env.contains("\"permission\"") { let _ = approver.send(true).await; }
                    if env.contains("action_log") || env.contains("\"status\"") {
                        println!("   [agent] {}", env.chars().take(150).collect::<String>());
                    }
                }
            });
            let route = hydra::RouteContext { surface: hydra::SurfaceState { vm_active: true, ..Default::default() }, mode: hydra::RouteMode::Auto };
            let walk = hydra::run(goal, String::new(), route, state.clone(), adapter.clone(),
                perceptor.clone(), actuator.clone(), approval_rx, confirm_tx, memory_tiers.clone(), None, skill_library.clone());
            let _ = tokio::time::timeout(Duration::from_secs(90), walk).await;
            drop(listener);
        }
    };

    // ── Case A: FIXABLE — injected typo 'tuch' must be reformed to 'touch'; verify the real file. ──
    println!("\n══════ Case A (fixable): 'run the command tuch /tmp/lagado_reform_a'");
    let _ = ssh("rm -f /tmp/lagado_reform_a");
    run_goal("run the command tuch /tmp/lagado_reform_a".to_string()).await;
    let a = ssh("test -f /tmp/lagado_reform_a && echo CREATED || echo MISSING");
    println!("   WORLD-STATE: /tmp/lagado_reform_a -> {a}");
    println!("   {}", if a.contains("CREATED") { "✅ reform RECOVERED the typo (world-state verified)" }
                       else { "❌ reform did NOT recover (file absent) — reapproach gap" });

    // ── Case B: UNFIXABLE — must TERMINATE (bounded), not hang. ──
    println!("\n══════ Case B (unfixable, bounded): 'run the command cat /tmp/lagado_zzz_nope'");
    let t = Instant::now();
    run_goal("run the command cat /tmp/lagado_zzz_nope".to_string()).await;
    let secs = t.elapsed().as_secs_f32();
    println!("   terminated in {secs:.1}s (bound = reform≤2 + 90s backstop)");
    println!("   {}", if secs < 88.0 { "✅ bounded — reapproach did not hang" } else { "❌ hit the backstop — investigate" });
}
