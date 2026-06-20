//! capability_probe — validates the STRUCTURED-ACTION (Capability) layer vs free-form authoring. The
//! model SELECTS a typed verb + fills slots (`move source_dir=… selector=*.pdf dest=…`); the HARNESS does
//! resolve→exec→verify (the model never writes shell or a check). Same deterministic stop (the battery's
//! world-state verify) as react_loop_probe v3, so it's a clean A/B: does typed SELECTION beat free-form's
//! 2/8? Runs the same user_stress 8 + OSWorld + hard tasks. Needs VM :2222 + brain :8080.
//!   cargo run --bin capability_probe

#[cfg(not(unix))]
fn main() { eprintln!("[capability_probe] Unix required"); }

#[cfg(unix)]
#[tokio::main]
async fn main() {
    use std::collections::HashMap;
    use std::process::Command;
    use std::sync::Arc;
    use lagado_agent::inference::llama_cpp::LlamaCppAdapter;
    use lagado_agent::{config, inference::InferenceAdapter};

    let port = 2222u16;
    // SSH connection MULTIPLEXING: reuse ONE master connection for every op (the probe fires hundreds of
    // ssh calls; fresh-connection-per-call exhausts the guest sshd → empty observe → grammar drops its
    // source-binding → the model runs unconstrained). ControlMaster fixes the root cause + is much faster.
    let ssh = |cmd: &str| -> String {
        Command::new("ssh").args(["-o","StrictHostKeyChecking=no","-o","UserKnownHostsFile=/dev/null",
            "-o","BatchMode=yes","-o","ConnectTimeout=5",
            "-o","ControlMaster=auto","-o","ControlPath=/tmp/lagado-cap-%r@%h:%p","-o","ControlPersist=180",
            "-p",&port.to_string(),"laputa@127.0.0.1",cmd])
            .output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
    };
    if !ssh("whoami").contains("laputa") { eprintln!("[FAIL] VM not on :2222"); std::process::exit(1); }
    let adapter: Arc<dyn InferenceAdapter> =
        Arc::new(LlamaCppAdapter::with_url(&config::llama_base_url(), "LFM2-8B-A1B", config::CONTEXT_SIZE));

    // OBSERVE: focused recursive listing of the work dirs (the capability `path` slots draw from this).
    let observe = |ssh: &dyn Fn(&str)->String| -> String {
        // Retry: an empty observe (transient SSH hiccup) would drop the grammar's source-binding.
        for _ in 0..3 {
            let out = ssh("for r in ~/Desktop ~/Documents ~/Downloads; do [ -e \"$r\" ] && \
                 find \"$r\" -maxdepth 4 -not -path '*/.*' 2>/dev/null; done | sort -u | head -80");
            if !out.trim().is_empty() { return out; }
        }
        String::new()
    };

    let menu = "ACTIONS — emit EXACTLY ONE Pythonic call. Use ONLY absolute paths from 'Current files'.
- make_folder(path=\"/abs/folder\")
- write_file(path=\"/abs/file.txt\", content=\"TEXT\")            (omit content for an empty file)
- move(source_dir=\"/abs/folder\", selector=\"*.pdf\", dest=\"/abs/folder\", new_name=\"name.ext\")   (new_name optional)
- copy(source_dir=\"/abs/folder\", selector=\"*.jpg\", dest=\"/abs/folder\", recursive=true)            (recursive optional)
- rename(path=\"/abs/file\", new_name=\"newname.ext\")
- delete(source_dir=\"/abs/folder\", selector=\"*.jpg\", filter=\"empty\")                              (filter optional)
- extract_to_file(mode=\"value\", source=\"/abs/file\", pattern=\"REGEX\", dest_file=\"/abs/file\")
- extract_to_file(mode=\"count\", source_dir=\"/abs/folder\", selector=\"*.log\", dest_file=\"/abs/file\")
- extract_to_file(mode=\"list\", source_dir=\"/abs/folder\", selector=\"*.jpg\", dest_file=\"/abs/file\")
To move/copy ONE named file, set selector to its exact filename. home is /home/laputa.";

    let make_prompt = |goal: &str, env: &str, hist: &str| format!(
"You operate a computer by choosing ONE typed action that moves toward the goal.
{menu}

Goal: {goal}
Current files:
{env}
Actions taken so far:
{hist}
Next single action:");

    // PYTHONIC parser: `[move(source_dir="/x", selector="*.pdf", dest="/y")]` → verb + kwargs.
    fn split_commas(s: &str) -> Vec<String> {  // top-level commas (not inside quotes)
        let (mut out, mut cur, mut q) = (Vec::new(), String::new(), false);
        for c in s.chars() { match c {
            '"' => { q=!q; cur.push(c); }
            ',' if !q => out.push(std::mem::take(&mut cur)),
            c => cur.push(c) } }
        if !cur.trim().is_empty() { out.push(cur); }
        out
    }
    fn parse_call(line: &str) -> Option<(String, HashMap<String,String>)> {
        let s = line.trim().trim_start_matches('[').trim_end_matches(']').trim();
        let open = s.find('(')?; let close = s.rfind(')')?; if close < open { return None; }
        let verb = s[..open].trim().trim_end_matches(':').to_lowercase();
        let mut m = HashMap::new();
        for part in split_commas(&s[open+1..close]) {
            if let Some((k,v)) = part.split_once('=') {
                let v = v.trim().trim_matches('"').trim_matches(|c| c=='<'||c=='>');
                m.insert(k.trim().to_lowercase(), v.to_string());
            }
        }
        Some((verb, m))
    }

    // Dispatch ONE capability: resolve → exec → verify. Returns a feedback string (success or why-not).
    let dispatch = |ssh: &dyn Fn(&str)->String, verb: &str, p: &HashMap<String,String>| -> String {
        let g = |k: &str| p.get(k).cloned().unwrap_or_default();
        let q = |s: &str| format!("\"{}\"", s.replace('"', ""));   // crude shell-quote
        match verb {
            "make_folder" => { let path=g("path"); if path.is_empty(){return "missing path".into();}
                ssh(&format!("mkdir -p {}", q(&path)));
                if ssh(&format!("test -d {} && echo Y", q(&path))).contains('Y') {"folder created".into()} else {"FAILED to create folder".into()} }
            "write_file" => { let path=g("path"); if path.is_empty(){return "missing path".into();}
                ssh(&format!("mkdir -p \"$(dirname {p})\"; printf '%s' {c} > {p}", p=q(&path), c=q(&g("content"))));
                if ssh(&format!("test -e {} && echo Y", q(&path))).contains('Y') {"file written".into()} else {"FAILED to write file".into()} }
            "move" | "copy" => {
                let (sd0, sel, dest)=(g("source_dir"), g("selector"), g("dest"));
                if sd0.is_empty()||dest.is_empty() {return "move/copy needs source_dir and dest".into();}
                // If source_dir is actually a FILE (common model slip), treat it as the single source.
                let is_file = ssh(&format!("test -f {} && echo Y", q(&sd0))).contains('Y');
                let (sd, files): (String, Vec<String>) = if is_file {
                    let base = sd0.rsplit('/').next().unwrap_or(&sd0).to_string();
                    let dir = sd0.rsplit_once('/').map(|(d,_)|d.to_string()).unwrap_or_else(||".".into());
                    (dir, vec![base])
                } else {
                    if sel.is_empty() { return "move/copy of a folder needs a selector".into(); }
                    let depth = if g("recursive")=="true" {""} else {"-maxdepth 1"};
                    let names = ssh(&format!("find {} {} -name {} -type f -printf '%f\\n' 2>/dev/null", q(&sd0), depth, q(&sel)));
                    (sd0.clone(), names.lines().filter(|l|!l.is_empty()).map(str::to_string).collect())
                };
                if files.is_empty() { return format!("no files in {sd0} match selector {sel}"); }
                let nn = g("new_name");
                ssh(&format!("mkdir -p {}", q(&dest)));
                let cpflag = if g("recursive")=="true" {"-r"} else {""};
                let op = if verb=="move" {"mv"} else {"cp"};
                let tgt = |f: &str| if files.len()==1 && !nn.is_empty() { format!("{dest}/{nn}") } else { format!("{dest}/{f}") };
                for f in &files { ssh(&format!("{op} {} {} {}", cpflag, q(&format!("{sd}/{f}")), q(&tgt(f)))); }
                let mut bad=0; for f in &files {
                    let dest_ok = ssh(&format!("test -e {} && echo Y", q(&tgt(f)))).contains('Y');
                    let src_present = ssh(&format!("test -e {} && echo Y", q(&format!("{sd}/{f}")))).contains('Y');
                    let src_state = if verb=="move" { !src_present } else { src_present };
                    if !(dest_ok && src_state) { bad+=1; }
                }
                if bad==0 { format!("{}d {} file(s) to {dest}", op, files.len()) } else { format!("{op} incomplete: {bad}/{} not verified", files.len()) }
            }
            "rename" => { let (path,nn)=(g("path"), g("new_name"));
                if path.is_empty()||nn.is_empty() {return "rename needs path, new_name".into();}
                ssh(&format!("mv {} \"$(dirname {p})\"/{n}", q(&path), p=q(&path), n=q(&nn)));
                let dir_new = format!("$(dirname {})/{}", q(&path), nn);
                let ok = ssh(&format!("test -e \"{}\" && echo Y", dir_new.replace('"',""))).contains('Y') ;
                let gone = !ssh(&format!("test -e {} && echo Y", q(&path))).contains('Y');
                if ok && gone {"renamed".into()} else {"FAILED to rename (check the source path exists)".into()} }
            "delete" => { let (sd,sel)=(g("source_dir"), g("selector"));
                if sd.is_empty()||sel.is_empty() {return "delete needs source_dir, selector".into();}
                let filt = match g("filter").as_str() { "empty"=>"-empty", "larger_than_1k"=>"-size +1k", _=>"" };
                let names = ssh(&format!("find {} -maxdepth 1 -name {} -type f {} -printf '%f\\n' 2>/dev/null", q(&sd), q(&sel), filt));
                let files: Vec<&str> = names.lines().filter(|l|!l.is_empty()).collect();
                if files.is_empty() { return format!("no files in {sd} match {sel} (filter={})", g("filter")); }
                for f in &files { ssh(&format!("rm -f {}/{}", q(&sd), f)); }
                format!("deleted {} file(s)", files.len()) }
            "extract_to_file" => { let mode=g("mode");
                let dest = if g("dest_file").is_empty() { g("dest") } else { g("dest_file") };  // accept dest alias
                if dest.is_empty() {return "extract_to_file needs dest_file".into();}
                let value = match mode.as_str() {
                    "value" => { let (src,pat)=(g("source"), g("pattern"));
                        if src.is_empty()||pat.is_empty() {return "mode=value needs source, pattern".into();}
                        ssh(&format!("grep -oE {} {} 2>/dev/null | head -1", q(&pat), q(&src))) }
                    "count" => { let (sd,sel)=(g("source_dir"), g("selector"));
                        ssh(&format!("find {} -maxdepth 1 -name {} -type f 2>/dev/null | wc -l", q(&sd), q(&sel))) }
                    "list"  => { let (sd,sel)=(g("source_dir"), g("selector"));
                        ssh(&format!("find {} -maxdepth 1 -name {} -type f -printf '%f\\n' 2>/dev/null", q(&sd), q(&sel))) }
                    _ => return "extract_to_file needs mode=value|count|list".into(),
                };
                if value.trim().is_empty() && mode!="count" { return "extract found nothing — check source/pattern/selector".into(); }
                // HARNESS writes the RESOLVED value (the model never supplies it).
                ssh(&format!("mkdir -p \"$(dirname {d})\"; printf '%s\\n' {v} > {d}", d=q(&dest), v=q(value.trim())));
                if ssh(&format!("test -s {} && echo Y", q(&dest))).contains('Y') { format!("wrote '{}' to {dest}", value.trim().chars().take(40).collect::<String>()) } else {"FAILED to write".into()} }
            _ => format!("unknown verb '{verb}' — choose one from the menu"),
        }
    };

    const RST: &str = "rm -rf ~/Documents ~/Downloads ~/Desktop; mkdir -p ~/Documents ~/Downloads ~/Desktop; ";
    // (name, goal, setup, battery world-state verify)
    let tasks: &[(&str,&str,&str,&str)] = &[
        // ── user_stress 8 (direct A/B vs free-form 2/8) ──
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
        // ── real OSWorld ──
        ("osw:rename-dir","I have a directory named todo_list_Jan_1. Change its name into todo_list_Jan_2.",
         "rm -rf ~/Desktop/todo_list_Jan_2;mkdir -p ~/Desktop/todo_list_Jan_1",
         "[ -d ~/Desktop/todo_list_Jan_2 ] && [ ! -d ~/Desktop/todo_list_Jan_1 ] && echo OK"),
        ("osw:copy-jpgs","Recursively go through the photos directory and copy any .jpg files into a directory named cpjpg.",
         "rm -rf ~/Desktop/photos ~/Desktop/cpjpg;mkdir -p ~/Desktop/photos/a/b ~/Desktop/cpjpg;touch ~/Desktop/photos/a/x.jpg ~/Desktop/photos/a/b/y.jpg ~/Desktop/photos/z.png",
         "[ \"$(find ~/Desktop/cpjpg -name '*.jpg'|wc -l)\" -eq 2 ] && [ -z \"$(find ~/Desktop/cpjpg -name '*.png')\" ] && echo OK"),
        ("osw:copy-many","Copy file file1 to each of the directories dir1, dir2, dir3.",
         "rm -rf ~/Desktop/mc;mkdir -p ~/Desktop/mc/dir1 ~/Desktop/mc/dir2 ~/Desktop/mc/dir3;echo c>~/Desktop/mc/file1",
         "cd ~/Desktop/mc && [ -f dir1/file1 ] && [ -f dir2/file1 ] && [ -f dir3/file1 ] && echo OK"),
        // ── hard ──
        ("hd:selective","move only the text files from my Downloads into my Documents, leave everything else",
         "printf a>~/Downloads/a.txt;printf b>~/Downloads/b.txt;printf c>~/Downloads/c.jpg",
         "test -f ~/Documents/a.txt && test -f ~/Documents/b.txt && test -f ~/Downloads/c.jpg && test ! -e ~/Downloads/a.txt && echo OK"),
        ("hd:nested-project","create a folder called project in my Documents with subfolders src and tests and an empty README.md inside project",
         "true",
         "test -d ~/Documents/project/src && test -d ~/Documents/project/tests && test -f ~/Documents/project/README.md && echo OK"),
        ("hd:count-logs","count how many .log files are in my Downloads and write just the number to logcount.txt in my Documents",
         "touch ~/Downloads/a.log ~/Downloads/b.log ~/Downloads/c.log ~/Downloads/x.txt",
         "grep -q 3 ~/Documents/logcount.txt && echo OK"),
        ("hd:delete-empty","delete the empty files in my Downloads, keep the ones that have content",
         ": >~/Downloads/e1.txt;: >~/Downloads/e2.txt;printf data>~/Downloads/full.txt",
         "test ! -e ~/Downloads/e1.txt && test ! -e ~/Downloads/e2.txt && test -f ~/Downloads/full.txt && echo OK"),
        ("hd:move-rename","move report.txt from my Downloads to my Documents and rename it to final_report.txt",
         "printf r>~/Downloads/report.txt",
         "test -f ~/Documents/final_report.txt && test ! -e ~/Downloads/report.txt && echo OK"),
    ];

    const MAX_STEPS: usize = 6;
    let passed_check = |ssh: &dyn Fn(&str)->String, v: &str| ssh(v).contains("OK");
    let mut passed = 0;
    for (name, goal, setup, verify) in tasks {
        println!("\n══════ [{name}]  {goal}");
        let _ = ssh(&format!("{RST}{setup}; true"));
        let mut hist = String::from("(none)");
        for step in 1..=MAX_STEPS {
            if passed_check(&ssh, verify) { break; }
            let env = observe(&ssh);
            // GBNF: Pythonic call format, source paths BOUND to the observe listing (off-screen unemittable).
            let paths: Vec<String> = env.lines().map(|l| l.trim().to_string()).filter(|p| p.starts_with('/')).collect();
            let grammar = lagado_agent::grammar::capability_grammar(&paths);
            let (raw, _) = adapter.generate_constrained(&make_prompt(goal, &env, &hist), 120, 0.1, &grammar).unwrap_or_default();
            let line = raw.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_string();
            let Some((verb, params)) = parse_call(&line) else { println!("   step {step}: <unparseable: {line}>"); break; };
            let fb = dispatch(&ssh, &verb, &params);
            println!("   step {step}: {verb} {:?} → {fb}", params);
            hist = if hist=="(none)" { format!("- {verb}: {fb}") } else { format!("{hist}\n- {verb}: {fb}") };
        }
        let ok = passed_check(&ssh, verify);
        if ok { passed += 1; }
        println!("   world-state: {}", if ok {"✅ PASS"} else {"❌ FAIL"});
    }
    println!("\n══════ CAPABILITY layer: {passed}/{} world-state-verified (free-form was ~2/8 user + 1/8 osworld)", tasks.len());
}
