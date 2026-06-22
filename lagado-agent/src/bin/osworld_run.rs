//! osworld_run — drive the FULL Lagado harness against one OSWorld task. The Python runner boots the
//! DesktopEnv and passes the guest's HTTP server URL + the task instruction; this runs the entire
//! agent_loop (router → planner → plane-governor → execute → verify) over the OSWorld guest via the
//! OsworldPerceptor/OsworldActuator, autonomously (auto-approves every gate — no human in the bench).
//! Prints the final status line (Python reads it; the real score is the Python side's env.evaluate()).
//!
//! Usage: osworld_run <http://guest_ip:port> <goal text>

#[cfg(not(unix))]
fn main() { eprintln!("[osworld_run] Unix required"); }

#[cfg(unix)]
#[tokio::main]
async fn main() {
    use std::sync::Arc;
    use std::time::Duration;
    use lagado_agent::inference::llama_cpp::LlamaCppAdapter;
    use lagado_agent::perception::{Actuator, PerceptionCache, Perceptor};
    use lagado_agent::skill_library::SkillLibrary;
    use lagado_agent::vm::{OsworldActuator, OsworldPerceptor};
    use lagado_agent::{agent, config, hydra, memory_tiers::MemoryTiers};
    use tokio::sync::{mpsc, Mutex};

    let args: Vec<String> = std::env::args().collect();
    let base_url = args.get(1).cloned().unwrap_or_default();
    let goal = args.get(2).cloned().unwrap_or_default();
    if base_url.is_empty() || goal.is_empty() {
        eprintln!("usage: osworld_run <http://guest_ip:port> <goal>");
        std::process::exit(2);
    }
    // parse host:port out of the guest URL
    let stripped = base_url.trim().trim_start_matches("https://").trim_start_matches("http://").trim_end_matches('/');
    let (host, port) = match stripped.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse::<u16>().unwrap_or(5000)),
        None => (stripped.to_string(), 5000),
    };

    // The whole harness, over the OSWorld guest (shared perception cache between perceptor + actuator).
    let cache = Arc::new(std::sync::Mutex::new(PerceptionCache::new()));
    let perceptor: Arc<dyn Perceptor> = Arc::new(OsworldPerceptor::with_cache(&host, port, cache.clone()));
    let actuator: Arc<dyn Actuator> = Arc::new(OsworldActuator::with_cache(&host, port, cache.clone()));
    let adapter: Arc<dyn lagado_agent::inference::InferenceAdapter> =
        Arc::new(LlamaCppAdapter::with_url(&config::llama_base_url(), "lagado-brain", config::CONTEXT_SIZE));
    let memory_tiers = Arc::new(Mutex::new(
        MemoryTiers::open(&config::data_dir().join("memory.db")).expect("memory")));
    let skill_library = Arc::new(SkillLibrary::open(&config::data_dir()));
    let state = Arc::new(Mutex::new(agent::AgentState {
        goal: String::new(), running: false, approval_tx: None, pending_id: None }));

    let (approval_tx, approval_rx) = mpsc::channel::<bool>(8);
    let (confirm_tx, mut confirm_rx) = mpsc::channel::<String>(64);
    { state.lock().await.approval_tx = Some(approval_tx.clone()); }
    let approver = approval_tx.clone();
    let trace = std::env::var("OSW_TRACE").is_ok();
    let listener = tokio::spawn(async move {
        let mut last = String::new();
        while let Some(env) = confirm_rx.recv().await {
            if env.contains("\"permission\"") { let _ = approver.send(true).await; } // autonomous: auto-approve
            if env.contains("\"status\"") { last = env.chars().take(200).collect(); }
            if trace && (env.contains("action_log") || env.contains("\"status\"")) {
                eprintln!("   · {}", env.chars().take(300).collect::<String>());
            }
        }
        last
    });

    let route = hydra::RouteContext {
        surface: hydra::SurfaceState { vm_active: true, ..Default::default() },
        mode: hydra::RouteMode::Auto,
    };
    let walk = hydra::run(goal, String::new(), route, state.clone(), adapter.clone(),
        perceptor.clone(), actuator.clone(), approval_rx, confirm_tx, memory_tiers.clone(), None, skill_library.clone());
    let _ = tokio::time::timeout(Duration::from_secs(200), walk).await;
    let final_status = listener.await.unwrap_or_default();
    println!("{final_status}");
}
