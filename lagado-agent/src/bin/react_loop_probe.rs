//! react_loop_probe — PROVE the ReAct reflex loop before core-loop surgery. For each failing
//! user_stress goal: per-task isolated setup, then run observe→reason(ONE next action)→act→observe
//! against the REAL VM (:2222) so observation is real `ls` after every move — NOT a single upfront
//! plan. The reflex step is single-turn-fresh: it sees only {goal, current files, steps done} and emits
//! ONE next action or DONE. Tests the user's architecture: does per-step reasoning complete the
//! multi-step file goals plan-once couldn't? World-state verified. Needs VM :2222 + brain :8080.
//!   cargo run --bin react_loop_probe

#[cfg(not(unix))]
fn main() { eprintln!("[react_loop_probe] Unix required"); }

#[cfg(unix)]
#[tokio::main]
async fn main() {
    use std::process::Command;
    use std::sync::Arc;
    use lagado_agent::inference::llama_cpp::LlamaCppAdapter;
    use lagado_agent::{config, inference::InferenceAdapter};

    let port = 2222u16;
    let ssh = |cmd: &str| -> String {
        Command::new("ssh").args(["-o","StrictHostKeyChecking=no","-o","UserKnownHostsFile=/dev/null",
            "-o","BatchMode=yes","-o","ConnectTimeout=5","-p",&port.to_string(),"laputa@127.0.0.1",cmd])
            .output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
    };
    if !ssh("whoami").contains("laputa") { eprintln!("[FAIL] VM not on :2222"); std::process::exit(1); }

    let adapter: Arc<dyn InferenceAdapter> =
        Arc::new(LlamaCppAdapter::with_url(&config::llama_base_url(), "LFM2-8B-A1B", config::CONTEXT_SIZE));

    // The OBSERVE step: a compact listing of the user's folders = the "current environment" the reflex
    // step reasons over. Deterministic, read-only.
    let observe = |ssh: &dyn Fn(&str)->String| -> String {
        ssh("for d in ~ ~/Documents ~/Documents/Records ~/Documents/Scans ~/Documents/Smith ~/Downloads; do \
             [ -d \"$d\" ] && printf '%s: ' \"$d\" && (ls -1 \"$d\" 2>/dev/null | grep -v '^Documents$\\|^Downloads$' | tr '\\n' ' ') && echo; done")
    };

    // The REASON step: single-turn-fresh. The EXPECTED TARGET (a human statement of the file(s) that must
    // exist — deterministically DERIVED, NOT model-authored) is fed back each step as the forcing
    // function: it tells the model exactly what is still missing, so it corrects the _copy-suffix /
    // wrong-glob mistakes the bare loop missed. In production this `expected` is derived from discovery +
    // the goal; here it is provided per-case to isolate the LOOP MECHANISM from the check-derivation.
    let make_prompt = |goal: &str, expected: &str, env: &str, hist: &str| format!(
"You are doing ONE step of a file task on Linux (home is /home/laputa). Output ONLY the single next
command that moves toward the EXPECTED RESULT. No narration.
- use mv to move/rename, cp to copy (KEEP THE SAME FILENAME), mkdir -p to make a folder, rm to delete
- ALWAYS write the FULL absolute path (starting with /home/laputa/) for BOTH the source and the
  destination — copy the exact source path from 'Current files', never a bare filename.
- Never invent files/contents. One command only.

Goal: {goal}
EXPECTED RESULT (not yet satisfied — make it true): {expected}
Current files:
{env}
Steps already done:
{hist}
Next single command:");

    // (goal, isolated setup, world-state verify [the deterministic in-loop check = the judge],
    //  expected-result HINT fed back to the model each step). RST wipes both dirs first.
    const RST: &str = "rm -rf ~/Documents ~/Downloads; mkdir -p ~/Documents ~/Downloads; ";
    let cases: &[(&str,&str,&str,&str)] = &[
        ("move all the PDF files from my Downloads folder into my Documents folder",
         "printf r > ~/Downloads/report_jan.pdf; printf r > ~/Downloads/report_feb.pdf; printf n > ~/Downloads/notes.txt",
         "test -f ~/Documents/report_jan.pdf && test -f ~/Documents/report_feb.pdf && test ! -e ~/Downloads/report_jan.pdf && test -f ~/Downloads/notes.txt && echo OK",
         "report_jan.pdf and report_feb.pdf exist in /home/laputa/Documents; notes.txt stays in /home/laputa/Downloads"),
        ("make a folder called Scans in my Documents and move my scan images into it",
         "printf i > ~/Downloads/scan_001.jpg; printf i > ~/Downloads/scan_002.jpg",
         "test -d ~/Documents/Scans && test -f ~/Documents/Scans/scan_001.jpg && test -f ~/Documents/Scans/scan_002.jpg && echo OK",
         "/home/laputa/Documents/Scans/scan_001.jpg and /home/laputa/Documents/Scans/scan_002.jpg exist"),
        ("rename the notes file in my Downloads to meeting_notes.txt",
         "printf n > ~/Downloads/notes.txt",
         "test -f ~/Downloads/meeting_notes.txt && test ! -e ~/Downloads/notes.txt && echo OK",
         "/home/laputa/Downloads/meeting_notes.txt exists and /home/laputa/Downloads/notes.txt no longer exists"),
        ("put a copy of Smith's intake form into my Documents Records folder",
         "mkdir -p ~/Documents/Records; printf i > ~/Documents/smith_intake.txt",
         "test -f ~/Documents/Records/smith_intake.txt && test -f ~/Documents/smith_intake.txt && echo OK",
         "/home/laputa/Documents/Records/smith_intake.txt exists (a copy with the SAME name) and the original /home/laputa/Documents/smith_intake.txt stays"),
        ("make a folder called Smith in my Documents and move all of Smith's files into it",
         "printf i > ~/Documents/smith_intake.txt; printf n > ~/Documents/smith_notes.txt",
         "test -d ~/Documents/Smith && test -f ~/Documents/Smith/smith_intake.txt && test -f ~/Documents/Smith/smith_notes.txt && echo OK",
         "/home/laputa/Documents/Smith/smith_intake.txt and /home/laputa/Documents/Smith/smith_notes.txt exist"),
        ("open the monthly report in my Documents and save just the total figure to a file called total.txt in my Documents",
         "printf 'Monthly patient visits report\\nTotal: 4200\\n' > ~/Documents/monthly_report.txt",
         "grep -q 4200 ~/Documents/total.txt && echo OK",
         "/home/laputa/Documents/total.txt exists and contains the number 4200 from monthly_report.txt"),
    ];

    const MAX_STEPS: usize = 6;
    // The VERIFY SUB-TASK: a simple deterministic check (the goal's expected world-state) run against the
    // REAL filesystem. NOT the model. exit 0 ⇒ "OK". This is the judge AND the loop's stop condition.
    let verify_passes = |ssh: &dyn Fn(&str)->String, check: &str| -> bool { ssh(check).contains("OK") };
    let mut passed = 0;
    for (goal, setup, verify, expected) in cases {
        println!("\n══════ {goal}");
        let _ = ssh(&format!("{RST}{setup}; true"));
        let mut hist = String::from("(none yet)");
        let mut stopped = "max-steps";
        for step in 1..=MAX_STEPS {
            // VERIFY-FIRST (deterministic, real world): satisfied ⇒ stop. Never the model's say-so.
            if verify_passes(&ssh, verify) { stopped = "verified"; break; }
            let env = observe(&ssh);
            let raw = adapter.generate(&make_prompt(goal, expected, &env, &hist), 96, 0.1).unwrap_or_default();
            let action = raw.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_string();
            println!("   step {step}: {action}");
            if action.is_empty() { stopped = "empty action"; break; }
            let cmd = action.strip_prefix("run the command ").unwrap_or(&action).trim();
            let before = observe(&ssh);
            // Capture the command's OWN error (the real-world verify signal): a failed source path prints
            // "cp: cannot stat …: No such file", which tells the model exactly what to fix next step.
            let out = ssh(&format!("{{ {cmd} ; }} 2>&1"));
            let after = observe(&ssh);
            let changed = if before == after { "no change" } else { "files changed" };
            let note = if out.trim().is_empty() { changed.to_string() } else { format!("{changed}; ERROR: {}", out.trim()) };
            hist = if hist == "(none yet)" { format!("- {action}  ({note})") }
                   else { format!("{hist}\n- {action}  ({note})") };
        }
        let ok = verify_passes(&ssh, verify);
        if ok { passed += 1; }
        println!("   stop: {stopped} | world-state: {}", if ok { "✅ PASS" } else { "❌ FAIL" });
    }
    println!("\n══════ ReAct + deterministic-verify loop: {passed}/{} (bare loop 4/6, model-check 3/6)", cases.len());
}
