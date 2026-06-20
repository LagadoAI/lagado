//! discover_probe — BEFORE wiring discover→operate into plan_goal, prove that injecting a real
//! filesystem listing into the planner prompt actually fixes the failing user_stress goals. For each
//! goal it generates the plan TWICE through the REAL plan_goal prompt shape: (A) ungrounded (today),
//! (B) grounded with a "Current files" block (the proposed fix). Read the two plans side by side: does
//! grounding turn a guessed/vague plan into concrete mv/mkdir/cp with the RIGHT paths? 8B on :8080, no VM.
//!   cargo run --bin discover_probe

#[tokio::main]
async fn main() {
    use std::sync::Arc;
    use lagado_agent::inference::llama_cpp::LlamaCppAdapter;
    use lagado_agent::{config, inference::InferenceAdapter};

    let adapter: Arc<dyn InferenceAdapter> =
        Arc::new(LlamaCppAdapter::with_url(&config::llama_base_url(), "LFM2-8B-A1B", config::CONTEXT_SIZE));

    // The REAL plan_goal prompt (agent.rs:606), with an OPTIONAL filesystem block injected before Goal.
    let make_prompt = |fs: &str, goal: &str| format!(
"Break the goal into the FEWEST concrete steps, one per line. The agent can act two ways:
- run the command <shell command>   — runs a shell command and reads its output. PREFER this for
  file operations, system info, running a program, package work — anything a terminal does well.
- write to <path>: <content>        — writes the given content to a file (use \\n for a newline).
- Click <element>                   — clicks an on-screen GUI element. Use ONLY to launch a GUI app.

Rules:
- One action per line. Pick the SIMPLEST surface for the goal.
- Output ONLY the steps. No narration, no 'locate'/'wait'/'verify'/'check'/'open the folder'.
- Do NOT use sudo. Do NOT use interactive programs (nano, vim, less, top, man) — they hang.
- Use the EXACT paths from 'Current files' below — do not guess where files live.

Example:
Goal: create an empty file at /tmp/notes.txt
Steps:
run the command touch /tmp/notes.txt
{fs}
Goal: {goal}
Steps:");

    // (goal, the real on-disk listing the setup produces). $HOME == /home/laputa.
    let cases = [
        ("move all the PDF files from my Downloads folder into my Documents folder",
         "Current files:\n/home/laputa/Downloads: report_jan.pdf report_feb.pdf notes.txt\n/home/laputa/Documents: (empty)"),
        ("make a folder called Scans in my Documents and move my scan images into it",
         "Current files:\n/home/laputa/Downloads: scan_001.jpg scan_002.jpg\n/home/laputa/Documents: (empty)"),
        ("rename the notes file in my Downloads to meeting_notes.txt",
         "Current files:\n/home/laputa/Downloads: notes.txt"),
        ("put a copy of Smith's intake form into my Documents Records folder",
         "Current files:\n/home/laputa/Documents: smith_intake.txt\n/home/laputa/Documents/Records: (empty)"),
        ("make a folder called Smith in my Documents and move all of Smith's files into it",
         "Current files:\n/home/laputa/Documents: smith_intake.txt smith_notes.txt"),
        ("open the monthly report in my Documents and save just the total figure to a file called total.txt in my Documents",
         "Current files:\n/home/laputa/Documents: monthly_report.txt"),
    ];

    let emit = |label: &str, text: &str| {
        println!("   ── {label} ──");
        for line in text.lines() { let t = line.trim(); if !t.is_empty() { println!("      | {t}"); } }
    };

    for (goal, fs) in cases {
        println!("\n══════ {goal}");
        match adapter.generate(&make_prompt("", goal), 192, 0.1) {
            Ok(t) => emit("A: ungrounded (today)", &t),
            Err(e) => println!("   ! model error: {e}"),
        }
        match adapter.generate(&make_prompt(fs, goal), 192, 0.1) {
            Ok(t) => emit("B: grounded (proposed)", &t),
            Err(e) => println!("   ! model error: {e}"),
        }
    }
    println!("\n(look for B: concrete mkdir/mv/cp/grep with the RIGHT source+dest paths vs A's guesses)");
}
