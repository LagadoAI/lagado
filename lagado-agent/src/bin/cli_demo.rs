//! cli_demo — drive ONE CLI-chain goal through the real agent loop against an ALREADY-RUNNING
//! guest VM (port 2222) and the live brain (:8080), to demonstrate the command channel + the
//! deterministic exit-code-verified sequencer (the fix for the chain-depth probe that broke when
//! the same chain was attempted by GUI typing).
//!
//! Prereqs: VM already booted on :2222 (this does NOT boot one), llama-server up on :8080.
//! Run:  cargo run --bin cli_demo

#[cfg(not(unix))]
fn main() { eprintln!("[cli_demo] Unix required"); }

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
        Command::new("ssh")
            .args(["-o","StrictHostKeyChecking=no","-o","UserKnownHostsFile=/dev/null",
                   "-o","BatchMode=yes","-o","ConnectTimeout=5","-p",&port.to_string(),
                   "laputa@127.0.0.1", cmd])
            .output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
    };

    // Sanity: VM + brain reachable.
    if !ssh("whoami").contains("laputa") { eprintln!("[FAIL] VM not reachable on :2222 — boot it first"); std::process::exit(1); }
    println!("[ok] VM up: {}", ssh("echo $(whoami)@$(hostname)"));

    // Clean slate for the demo artifacts.
    let _ = ssh("rm -f /tmp/lagado_cli_a /tmp/lagado_cli_b");
    println!("[pre] /tmp/lagado_cli_*: {:?}", ssh("ls /tmp/lagado_cli_a /tmp/lagado_cli_b 2>&1"));

    let cache = Arc::new(Mutex::new(PerceptionCache::new()));
    let perceptor: Arc<dyn Perceptor> = Arc::new(SshPerceptor::with_cache("127.0.0.1", port, "laputa", cache.clone()));
    let actuator: Arc<dyn Actuator> = Arc::new(SshActuator::with_cache("127.0.0.1", port, "laputa", cache.clone()));
    let adapter: Arc<dyn lagado_agent::inference::InferenceAdapter> =
        Arc::new(LlamaCppAdapter::with_url(&config::llama_base_url(), "LFM2-8B-A1B", config::CONTEXT_SIZE));
    let memory_tiers = Arc::new(tokio::sync::Mutex::new(
        MemoryTiers::open(&config::data_dir().join("memory.db")).expect("memory")));
    let skill_library = Arc::new(SkillLibrary::open(&config::data_dir()));
    let state = Arc::new(tokio::sync::Mutex::new(agent::AgentState {
        goal: String::new(), running: false, approval_tx: None, pending_id: None }));

    let (approval_tx, approval_rx) = mpsc::channel::<bool>(8);
    let (confirm_tx, mut confirm_rx) = mpsc::channel::<String>(64);
    { let mut s = state.lock().await; s.approval_tx = Some(approval_tx.clone()); }

    // Auto-approve any confirmation (writes like `touch` are Tap-gated) and stream the log.
    let approver = approval_tx.clone();
    let listener = tokio::spawn(async move {
        while let Some(env) = confirm_rx.recv().await {
            if env.contains("\"permission\"") { let _ = approver.send(true).await; }
            if env.contains("\"action_log\"") || env.contains("\"status\"") {
                println!("[agent] {}", env.chars().take(180).collect::<String>());
            }
        }
    });

    // The goal: an IMPLICIT intent — the user does NOT spell out the steps. The capability-aware
    // planner must decompose it into commands itself and the channel executes + verifies them.
    let goal = "create two empty files: /tmp/lagado_cli_a and /tmp/lagado_cli_b";
    println!("\n[goal] {goal}\n");

    let t = Instant::now();
    // Surface is active (VM up) → the state-aware router treats an action-shaped goal as Interactive.
    let route = hydra::RouteContext {
        surface: hydra::SurfaceState { vm_active: true, ..Default::default() },
        mode: hydra::RouteMode::Auto,
    };
    let walk = hydra::run(goal.to_string(), String::new(), route, state.clone(),
        adapter.clone(), perceptor.clone(), actuator.clone(), approval_rx, confirm_tx,
        memory_tiers.clone(), None, skill_library.clone());
    let _ = tokio::time::timeout(Duration::from_secs(90), walk).await;
    drop(listener);

    // VERIFY against the world: both files must exist.
    println!("\n[verify] elapsed {:.1}s", t.elapsed().as_secs_f32());
    let a = ssh("test -f /tmp/lagado_cli_a && echo A_OK || echo A_MISSING");
    let b = ssh("test -f /tmp/lagado_cli_b && echo B_OK || echo B_MISSING");
    println!("  file A: {a}");
    println!("  file B: {b}");
    if a.contains("A_OK") && b.contains("B_OK") {
        println!("\n✅ PASS — agent used the command channel; both files created (chain held).");
    } else {
        println!("\n❌ FAIL — chain did not complete via the channel.");
    }
}
