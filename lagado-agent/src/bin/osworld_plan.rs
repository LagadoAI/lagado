//! osworld_plan — the BRIDGE between OSWorld's Python agent and our Rust harness. Takes an OSWorld task
//! instruction (argv) and emits our planner's DECOMPOSITION as JSON to stdout, so the Python adapter
//! (mm_agents/lagado_agent.py) can drive OSWorld's guest with it. Tests OUR harness's decomposition (the
//! proven moat) on real OSWorld tasks. Terminal-first MVP: each step is classified (command/click/type/
//! key) so the Python side runs command steps in a guest terminal and we measure exactly which OSWorld
//! domains the terminal plane carries vs. which need the a11y/CV/pixel plane (the home/away map).
//! Needs the brain on :8080 (Qwen). Run: cargo run --bin osworld_plan -- "<instruction>"

use std::sync::Arc;
use lagado_agent::inference::llama_cpp::LlamaCppAdapter;
use lagado_agent::inference::InferenceAdapter;
use lagado_agent::{agent, config};
use lagado_agent::agent::SubAction;

fn main() {
    let instruction: String = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    if instruction.trim().is_empty() {
        eprintln!("usage: osworld_plan \"<task instruction>\"");
        std::process::exit(2);
    }
    let adapter: Arc<dyn InferenceAdapter> =
        Arc::new(LlamaCppAdapter::with_url(&config::llama_base_url(), "brain", config::CONTEXT_SIZE));

    // OUR planner decomposes the goal (single-turn-fresh). No skills (keep the bridge deterministic/clean).
    let steps = agent::plan_goal(&instruction, &[], &adapter);

    // Classify each step into a plane: a Command step runs in a guest terminal (our proven plane); a
    // Click/Type/Key step needs the GUI plane (a11y/CV/pixel) — flagged so the Python side + the per-domain
    // score reveal exactly where the terminal carries and where plane-transition is required.
    let items: Vec<serde_json::Value> = steps.iter().map(|s| {
        let sg = agent::classify_subgoal(s);
        let (kind, payload) = match &sg.action {
            SubAction::Command(c) => ("command", c.clone()),
            SubAction::Type(t)    => ("type", t.clone()),
            SubAction::Key(k)     => ("key", k.clone()),
            SubAction::Click      => ("click", sg.text.clone()),
        };
        serde_json::json!({ "text": sg.text, "kind": kind, "payload": payload })
    }).collect();

    let out = serde_json::json!({
        "instruction": instruction,
        "n": items.len(),
        "all_command": items.iter().all(|i| i["kind"] == "command"),
        "steps": items,
    });
    println!("{}", serde_json::to_string(&out).unwrap_or_else(|_| "{\"steps\":[]}".into()));
}
