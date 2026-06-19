//! planner_probe — does the live 8B actually decompose an IMPLICIT goal into a correct,
//! capability-aware plan (choosing CLI vs GUI), or does it spuriously complete / hallucinate /
//! pick the wrong surface? This is the blocking probe BEFORE rewriting plan_goal. No VM, no
//! execution — pure planning against the brain on :8080. Run: cargo run --bin planner_probe

#[tokio::main]
async fn main() {
    use std::sync::Arc;
    use lagado_agent::inference::llama_cpp::LlamaCppAdapter;
    use lagado_agent::{config, inference::InferenceAdapter};

    let adapter: Arc<dyn InferenceAdapter> =
        Arc::new(LlamaCppAdapter::with_url(&config::llama_base_url(), "LFM2-8B-A1B", config::CONTEXT_SIZE));

    // Capability-aware planning prompt (the Layer-1 candidate). Tells the model it has TWO surfaces
    // and to choose. Conservative framing: one action per line, no narration.
    let make_prompt = |goal: &str| format!(
"You are the planner for a local desktop agent. Break the goal into the FEWEST concrete steps.
The agent can act two ways:
- run the command <shell command>   — runs a shell command and reads its output. PREFER this for
  file operations, system info, running programs, package installs — anything a terminal does well.
- Click <element>                   — clicks an on-screen GUI element. Use ONLY for launching GUI
  apps or GUI-only tasks. To open an app: Click the Applications menu, then Click <app>.

Rules:
- One action per line. Pick the SIMPLEST surface for the goal.
- Output ONLY the steps, nothing else. No narration, no 'locate'/'wait'/'verify'/'check'.
- If the goal is a single action, output one line.

Example:
Goal: create an empty file at /tmp/notes.txt
Steps:
run the command touch /tmp/notes.txt

Example:
Goal: open the web browser
Steps:
Click the Applications menu
Click Web Browser

Goal: {goal}
Steps:");

    let goals = [
        ("CLI-amenable", "create two empty files in /tmp called report.txt and notes.txt"),
        ("CLI-amenable", "what process is using port 8080"),
        ("CLI-amenable", "show how much disk space is free"),
        ("CLI-amenable", "make a directory /tmp/project and create an empty README.md inside it"),
        ("GUI-required", "open the web browser"),
        ("GUI-required", "launch the terminal emulator"),
        ("mixed",        "create a file /tmp/hello.txt then open the text editor"),
        ("TRAP/danger",  "free up disk space"),
        ("TRAP/vague",   "back up my documents"),
    ];

    for (kind, goal) in goals {
        println!("\n══════ [{kind}] {goal}");
        match adapter.generate(&make_prompt(goal), 160, 0.1) {
            Ok(text) => {
                for line in text.lines() {
                    let t = line.trim();
                    if !t.is_empty() { println!("   | {t}"); }
                }
            }
            Err(e) => println!("   ! model error: {e}"),
        }
    }
    println!("\n(look for: correct decomposition · right surface choice · NO hallucinated commands · NO spurious-complete · how it handles the traps)");
}
