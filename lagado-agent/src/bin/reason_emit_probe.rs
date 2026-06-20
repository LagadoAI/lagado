//! reason_emit_probe — the two-stage REASONER + EMITTER architecture. Stage 1: gen2.5 (thinking, port
//! 8080, NO grammar) reads {goal, observe} and outputs a concrete DECISION. Stage 2: a no-CoT EMITTER
//! (1.2B-Tool, port from LAGADO_EMIT_PORT, grammar-constrained, paths bound to observe) transcribes the
//! decision into one capability call. The harness (lib capability_to_command) runs it; stop = the
//! deterministic battery verify. Tests whether reason→emit beats gen2-alone (7/16). Needs VM :2222,
//! gen2.5 on :8080, emitter on :8083. Run: LAGADO_EMIT_PORT=8083 cargo run --bin reason_emit_probe

#[cfg(not(unix))]
fn main() { eprintln!("[reason_emit_probe] Unix required"); }

#[cfg(unix)]
#[tokio::main]
async fn main() {
    use std::process::Command;
    use std::sync::Arc;
    use lagado_agent::inference::llama_cpp::LlamaCppAdapter;
    use lagado_agent::{agent, config, grammar, inference::InferenceAdapter};

    let port = 2222u16;
    let ssh = |cmd: &str| -> String {
        Command::new("ssh").args(["-o","StrictHostKeyChecking=no","-o","UserKnownHostsFile=/dev/null",
            "-o","BatchMode=yes","-o","ConnectTimeout=5","-o","ControlMaster=auto",
            "-o","ControlPath=/tmp/lagado-re-%r@%h:%p","-o","ControlPersist=180",
            "-p",&port.to_string(),"laputa@127.0.0.1",cmd])
            .output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
    };
    if !ssh("whoami").contains("laputa") { eprintln!("[FAIL] VM not on :2222"); std::process::exit(1); }

    let emit_port: u16 = std::env::var("LAGADO_EMIT_PORT").ok().and_then(|s| s.parse().ok()).unwrap_or(8083);
    let reasoner: Arc<dyn InferenceAdapter> = Arc::new(LlamaCppAdapter::with_url(
        &format!("http://127.0.0.1:{}", 8080), "gen2.5-reasoner", config::CONTEXT_SIZE));
    let emitter: Arc<dyn InferenceAdapter> = Arc::new(LlamaCppAdapter::with_url(
        &format!("http://127.0.0.1:{emit_port}"), "emitter", config::CONTEXT_SIZE));

    let observe = |ssh: &dyn Fn(&str)->String| -> String {
        for _ in 0..3 {
            let out = ssh("for r in ~/Desktop ~/Documents ~/Downloads; do [ -e \"$r\" ] && \
                 find \"$r\" -maxdepth 4 -not -path '*/.*' 2>/dev/null; done | sort -u | head -80");
            if !out.trim().is_empty() { return out; }
        }
        String::new()
    };

    // STAGE 1 — gen2.5 reasons (thinking on, no grammar) → a concrete one-line decision.
    let reason_prompt = |goal: &str, env: &str| format!(
"Decide the SINGLE next file operation to accomplish the goal. Think it through, then end with ONE line:
DECISION: <operation> | <exact source path(s) or glob> | <exact destination path>
Operations: move, copy, rename, make_folder, delete, count_to_file, extract_value. Use ONLY paths from
'Current files'. To affect all matching files use a glob (e.g. *.pdf). home is /home/laputa.

Goal: {goal}
Current files:
{env}");
    // STAGE 2 — the emitter transcribes the decision into ONE grammar-constrained call (paths bound).
    let emit_prompt = |decision: &str, env: &str| format!(
"Convert the DECISION into exactly ONE Pythonic capability call. Use ONLY absolute paths from 'Current files'.
Verbs: make_folder(path=), write_file(path=,content=), move(source_dir=,selector=,dest=[,new_name=]),
copy(source_dir=,selector=,dest=[,recursive=true]), rename(path=,new_name=), delete(source_dir=,selector=[,filter=]),
extract_to_file(mode=value|count|list, ...). selector is a glob (*.pdf) or a filename.

DECISION: {decision}
Current files:
{env}
Call:");

    const RST: &str = "rm -rf ~/Documents ~/Downloads ~/Desktop; mkdir -p ~/Documents ~/Downloads ~/Desktop; ";
    let tasks: &[(&str,&str,&str,&str)] = &[
        ("us:organize-pdfs","move all the PDF files from my Downloads folder into my Documents folder",
         "printf r>~/Downloads/report_jan.pdf;printf r>~/Downloads/report_feb.pdf;printf n>~/Downloads/notes.txt",
         "test -f ~/Documents/report_jan.pdf && test -f ~/Documents/report_feb.pdf && test ! -e ~/Downloads/report_jan.pdf && test -f ~/Downloads/notes.txt && echo OK"),
        ("us:new-folder-collect","make a folder called Scans in my Documents and move my scan images into it",
         "printf i>~/Downloads/scan_001.jpg;printf i>~/Downloads/scan_002.jpg",
         "test -d ~/Documents/Scans && test -f ~/Documents/Scans/scan_001.jpg && test -f ~/Documents/Scans/scan_002.jpg && echo OK"),
        ("us:rename","rename the notes file in my Downloads to meeting_notes.txt",
         "printf n>~/Downloads/notes.txt",
         "test -f ~/Downloads/meeting_notes.txt && test ! -e ~/Downloads/notes.txt && echo OK"),
        ("us:copy-to-records","put a copy of Smith's intake form into my Documents Records folder",
         "mkdir -p ~/Documents/Records;printf i>~/Documents/smith_intake.txt",
         "test -f ~/Documents/Records/smith_intake.txt && test -f ~/Documents/smith_intake.txt && echo OK"),
        ("us:gather","make a folder called Smith in my Documents and move all of Smith's files into it",
         "printf i>~/Documents/smith_intake.txt;printf n>~/Documents/smith_notes.txt",
         "test -d ~/Documents/Smith && test -f ~/Documents/Smith/smith_intake.txt && test -f ~/Documents/Smith/smith_notes.txt && echo OK"),
        ("us:extract-figure","open the monthly report in my Documents and save just the total figure to a file called total.txt in my Documents",
         "printf 'Monthly report\\nTotal: 4200\\n'>~/Documents/monthly_report.txt",
         "grep -q 4200 ~/Documents/total.txt && echo OK"),
        ("us:count-docs","count how many .txt files are in my Documents folder and write the number to count.txt in my Documents",
         "printf a>~/Documents/a.txt;printf b>~/Documents/b.txt;printf c>~/Documents/c.txt",
         "grep -q 3 ~/Documents/count.txt && echo OK"),
        ("us:tidy","delete any leftover image files in my Downloads folder",
         "printf i>~/Downloads/scan_001.jpg;printf i>~/Downloads/scan_002.jpg;printf k>~/Downloads/keep.txt",
         "test -z \"$(ls ~/Downloads/*.jpg 2>/dev/null)\" && test -f ~/Downloads/keep.txt && echo OK"),
        ("osw:rename-dir","I have a directory named todo_list_Jan_1. Change its name into todo_list_Jan_2.",
         "rm -rf ~/Desktop/todo_list_Jan_2;mkdir -p ~/Desktop/todo_list_Jan_1",
         "[ -d ~/Desktop/todo_list_Jan_2 ] && [ ! -d ~/Desktop/todo_list_Jan_1 ] && echo OK"),
        ("hd:selective","move only the text files from my Downloads into my Documents, leave everything else",
         "printf a>~/Downloads/a.txt;printf b>~/Downloads/b.txt;printf c>~/Downloads/c.jpg",
         "test -f ~/Documents/a.txt && test -f ~/Documents/b.txt && test -f ~/Downloads/c.jpg && test ! -e ~/Downloads/a.txt && echo OK"),
        ("hd:move-rename","move report.txt from my Downloads to my Documents and rename it to final_report.txt",
         "printf r>~/Downloads/report.txt",
         "test -f ~/Documents/final_report.txt && test ! -e ~/Downloads/report.txt && echo OK"),
        ("hd:count-logs","count how many .log files are in my Downloads and write just the number to logcount.txt in my Documents",
         "touch ~/Downloads/a.log ~/Downloads/b.log ~/Downloads/c.log ~/Downloads/x.txt",
         "grep -q 3 ~/Documents/logcount.txt && echo OK"),
    ];

    const MAX_STEPS: usize = 5;
    let passes = |ssh: &dyn Fn(&str)->String, v: &str| ssh(v).contains("OK");
    let strip_think = |s: &str| -> String {
        // drop everything up to and including a closing </think>, else use the whole text
        match s.rfind("</think>") { Some(i) => s[i+8..].to_string(), None => s.to_string() }
    };
    let mut passed = 0;
    for (name, goal, setup, verify) in tasks {
        println!("\n══════ [{name}]  {goal}");
        let _ = ssh(&format!("{RST}{setup}; true"));
        for step in 1..=MAX_STEPS {
            if passes(&ssh, verify) { break; }
            let env = observe(&ssh);
            // STAGE 1: reason (gen2.5, generous tokens so it can think then conclude)
            let raw = reasoner.generate(&reason_prompt(goal, &env), 400, 0.2).unwrap_or_default();
            let body = strip_think(&raw);
            let decision = body.lines().rev().find(|l| l.to_uppercase().contains("DECISION"))
                .map(|l| l.trim().to_string())
                .unwrap_or_else(|| body.lines().map(str::trim).filter(|l|!l.is_empty()).last().unwrap_or("").to_string());
            // STAGE 2: emit (grammar-constrained, paths bound to observe)
            let paths: Vec<String> = env.lines().map(|l| l.trim().to_string()).filter(|p| p.starts_with('/')).collect();
            let g = grammar::capability_grammar(&paths);
            let (call, _) = emitter.generate_constrained(&emit_prompt(&decision, &env), 120, 0.1, &g).unwrap_or_default();
            let line = call.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_string();
            let cmd = agent::parse_capability_call(&line).and_then(|(v,p)| agent::capability_to_command(&v,&p));
            println!("   step {step}: decision[{}] → call[{}] → cmd[{}]",
                decision.chars().take(60).collect::<String>(), line.chars().take(60).collect::<String>(),
                cmd.as_deref().unwrap_or("<none>").chars().take(60).collect::<String>());
            let Some(cmd) = cmd else { break; };
            let _ = ssh(&cmd);
        }
        let ok = passes(&ssh, verify);
        if ok { passed += 1; }
        println!("   world-state: {}", if ok {"✅ PASS"} else {"❌ FAIL"});
    }
    println!("\n══════ REASON→EMIT (gen2.5 + emitter:{emit_port}): {passed}/{} (gen2-alone was 7/16-equiv)", tasks.len());
}
