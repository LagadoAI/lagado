//! multistep_probe — the BREAK-POINT MAP for multi-step reliability (the hard problem: everyone fails
//! cross-step, field SOTA ~26%). Each task is genuinely multi-step: step N+1 DEPENDS on step N's effect.
//! Ordered CHECKPOINTS let us see exactly WHICH cross-step transition breaks. Runs the real capability
//! loop (observe→reason→emit[GBNF]→act, lib functions) on gen2 :8080; after EACH step records which
//! checkpoints are now satisfied → the map of how far each task got and where it plateaued.
//! Needs VM :2222 + model :8080. Run: cargo run --bin multistep_probe

#[cfg(not(unix))]
fn main() { eprintln!("[multistep_probe] Unix required"); }

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
            "-o","ControlPath=/tmp/lagado-ms-%r@%h:%p","-o","ControlPersist=180",
            "-p",&port.to_string(),"laputa@127.0.0.1",cmd])
            .output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
    };
    if !ssh("whoami").contains("laputa") { eprintln!("[FAIL] VM not on :2222"); std::process::exit(1); }
    let adapter: Arc<dyn InferenceAdapter> = Arc::new(LlamaCppAdapter::with_url(
        &config::llama_base_url(), "model", config::CONTEXT_SIZE));

    let observe = |ssh: &dyn Fn(&str)->String| -> String {
        for _ in 0..3 {
            let out = ssh("for r in ~/Desktop ~/Documents ~/Downloads; do [ -e \"$r\" ] && \
                 find \"$r\" -maxdepth 4 -not -path '*/.*' 2>/dev/null; done | sort -u | head -80");
            if !out.trim().is_empty() { return out; }
        }
        String::new()
    };

    // (name, goal, setup, ORDERED checkpoints — each a cmd printing OK; last = full completion).
    // Every task: step N+1 DEPENDS on step N (a real cross-step dependency).
    const RST: &str = "rm -rf ~/Documents ~/Downloads; mkdir -p ~/Documents ~/Downloads; ";
    let tasks: &[(&str,&str,&str,&[&str])] = &[
        ("folder-then-move",
         "make a folder called Scans in my Documents and move my scan images into it",
         "printf i>~/Downloads/scan_001.jpg;printf i>~/Downloads/scan_002.jpg",
         &["test -d ~/Documents/Scans && echo OK",
           "test -f ~/Documents/Scans/scan_001.jpg && test -f ~/Documents/Scans/scan_002.jpg && echo OK"]),
        ("extract-then-write",
         "read the total from the monthly report in my Documents and save just that number to total.txt in my Documents",
         "printf 'Monthly report\\nTotal: 4200\\n'>~/Documents/monthly_report.txt",
         &["test -f ~/Documents/total.txt && echo OK",
           "grep -q 4200 ~/Documents/total.txt && echo OK"]),
        ("collect-patient (3-dep)",
         "make a folder called Smith in my Documents and move all of Smith's files into it",
         "printf i>~/Documents/smith_intake.txt;printf n>~/Documents/smith_notes.txt",
         &["test -d ~/Documents/Smith && echo OK",
           "test -f ~/Documents/Smith/smith_intake.txt && echo OK",
           "test -f ~/Documents/Smith/smith_notes.txt && echo OK"]),
        ("nested-structure (3-dep)",
         "create a folder called project in my Documents with subfolders src and tests, and an empty README.md inside project",
         "true",
         &["test -d ~/Documents/project && echo OK",
           "test -d ~/Documents/project/src && test -d ~/Documents/project/tests && echo OK",
           "test -f ~/Documents/project/README.md && echo OK"]),
        ("make-count-into (3-dep)",
         "make a folder called Logs in my Documents, then count how many .txt files are in my Downloads and write that number to count.txt inside Logs",
         "printf a>~/Downloads/a.txt;printf b>~/Downloads/b.txt;printf c>~/Downloads/c.txt",
         &["test -d ~/Documents/Logs && echo OK",
           "test -f ~/Documents/Logs/count.txt && echo OK",
           "grep -q 3 ~/Documents/Logs/count.txt && echo OK"]),
        ("move-then-rename",
         "move report.txt from my Downloads to my Documents and rename it to final_report.txt",
         "printf r>~/Downloads/report.txt",
         &["test ! -e ~/Downloads/report.txt && echo OK",
           "test -f ~/Documents/final_report.txt && echo OK"]),
        ("organize-by-type (2-branch)",
         "in my Downloads, move the PDF files into a folder called Pdfs in my Documents, and the JPG images into a folder called Images in my Documents",
         "printf p>~/Downloads/report.pdf;printf q>~/Downloads/doc.pdf;printf i>~/Downloads/pic.jpg",
         &["test -f ~/Documents/Pdfs/report.pdf && test -f ~/Documents/Pdfs/doc.pdf && echo OK",
           "test -f ~/Documents/Images/pic.jpg && echo OK"]),
    ];

    const MAX_STEPS: usize = 7;
    let cp_ok = |ssh: &dyn Fn(&str)->String, c: &str| ssh(c).contains("OK");
    // aggregate: for each "from→to" checkpoint transition, count reached vs attempted
    let mut transitions_passed = vec![0usize; 4];
    let mut transitions_total = vec![0usize; 4];
    let mut fully = 0;

    for (name, goal, setup, checks) in tasks {
        println!("\n══════ [{name}]  {goal}");
        let _ = ssh(&format!("{RST}{setup}; true"));
        let mut hist = String::from("(none)");
        let n = checks.len();
        // reached[i] = highest checkpoint satisfied so far
        let mut reached = 0usize;
        let mut plateau_step = 0usize;
        for step in 1..=MAX_STEPS {
            // advance the reached pointer over any now-satisfied checkpoints (cumulative, in order)
            while reached < n && cp_ok(&ssh, checks[reached]) { reached += 1; plateau_step = step; }
            if reached == n { break; }
            let env = observe(&ssh);
            let paths: Vec<String> = env.lines().map(|l| l.trim().to_string()).filter(|p| p.starts_with('/')).collect();
            // FAIL-SAFE: validate (known verb + grounded paths); reject garbage (/abs, invalid verbs); re-emit up to 3×.
            let mut cmd: Option<String> = None; let mut line = String::new();
            for _try in 0..3 {
                let g = grammar::capability_grammar(&paths);
                let (raw, _) = adapter.generate_constrained(&agent::capability_prompt(goal, &env, &hist), 128, 0.1, &g).unwrap_or_default();
                line = raw.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_string();
                if let Some((v,p)) = agent::parse_capability_call(&line) {
                    if agent::validate_capability_call(&v,&p).is_ok() { cmd = agent::capability_to_command(&v,&p); if cmd.is_some() { break; } }
                }
            }
            let out = match &cmd { Some(c) => ssh(&format!("{{ {c} ; }} 2>&1")), None => String::new() };
            let err: String = out.lines().filter(|l|!l.is_empty()).collect::<Vec<_>>().join(" ");
            // OSCILLATION RAIL: if the action changed NOTHING in the world, it was a no-op/repeat → tell
            // the model explicitly so it ADVANCES instead of repeating (the static prompt rule didn't take).
            let no_effect = cmd.is_some() && err.is_empty() && observe(&ssh) == env;
            println!("   step {step} [reached {reached}/{n}]: {} {}{}",
                line.chars().take(60).collect::<String>(),
                if err.is_empty() {String::new()} else {format!("→ {}", err.chars().take(40).collect::<String>())},
                if no_effect {"  [NO EFFECT]"} else {""});
            let mark = if no_effect { "  ← NO EFFECT (already done / matched nothing) — do a DIFFERENT next step" }
                       else if !err.is_empty() { "  ← FAILED" } else { "" };
            hist = if hist=="(none)" { format!("- {line}{mark}") } else { format!("{hist}\n- {line}{mark}") };
        }
        while reached < n && cp_ok(&ssh, checks[reached]) { reached += 1; }
        // record the transition that broke (reached → reached, i.e. the reached-th transition failed)
        for t in 0..n { if t < 4 { transitions_total[t] += 1; if t < reached { transitions_passed[t] += 1; } } }
        if reached == n { fully += 1; }
        let bar: String = (0..n).map(|i| if i < reached {'■'} else {'□'}).collect();
        println!("   ⟹ MAP: {bar}  reached {reached}/{n}{}", if reached==n {" ✅ COMPLETE".to_string()}
                 else {format!("  ✗ BROKE at transition {}→{} (plateau step {plateau_step})", reached, reached+1)});
    }
    println!("\n══════ MULTI-STEP BREAK-POINT MAP: {fully}/{} fully complete", tasks.len());
    println!("transition pass-rate (how often the Nth dependent step lands):");
    for t in 0..4 { if transitions_total[t] > 0 {
        println!("   step {}→{}: {}/{}", t+1, t+2, transitions_passed[t], transitions_total[t]); } }
}
