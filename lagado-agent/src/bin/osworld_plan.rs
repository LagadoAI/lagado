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
    let args: Vec<String> = std::env::args().skip(1).collect();
    let adapter: Arc<dyn InferenceAdapter> =
        Arc::new(LlamaCppAdapter::with_url(&config::llama_base_url(), "brain", config::CONTEXT_SIZE));

    // REGROUND mode (discover-then-operate): a command failed because the model assumed system facts that
    // are false on THIS machine (e.g. a hallucinated gsettings schema name). Given the goal + the failed
    // command + its error + the DISCOVERED actual facts, emit ONE corrected command grounded in those
    // facts. Prefer `dconf write <path> <value>` for app config (schema-agnostic — no name to hallucinate).
    if args.first().map(|s| s.as_str()) == Some("--reground") {
        let goal = args.get(1).cloned().unwrap_or_default();
        let failed = args.get(2).cloned().unwrap_or_default();
        let error = args.get(3).cloned().unwrap_or_default();
        let discovery = args.get(4).cloned().unwrap_or_default();
        let prompt = format!(
"A shell command FAILED because it assumed facts that are WRONG on this machine. Use ONLY the DISCOVERED
facts below to write ONE corrected command. Output ONLY the command line, nothing else.
- For application/desktop settings prefer `dconf write <full/dconf/path> <value>` (no schema name needed).
- Use the EXACT names/paths/UUIDs from DISCOVERED — never invent schema names or keys.

Goal: {goal}
Failed command: {failed}
Error: {error}
DISCOVERED (actual system facts):
{discovery}

Corrected command:");
        let cmd = adapter.generate(&prompt, 160, 0.1).unwrap_or_default();
        let cmd = cmd.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with('#')).unwrap_or("").to_string();
        println!("{}", serde_json::json!({ "command": cmd }));
        return;
    }

    // SELECT mode (GUI plane — our perception-fusion selection discipline): given the goal/sub-goal + a
    // candidate element list (from OSWorld's a11y tree), pick the ONE element that matches via a
    // GRAMMAR-CONSTRAINED `el_N | none` choice (fail-closed escape — `none` when nothing matches, never a
    // hallucinated coord). Mirrors selection.rs (el_N index + ESCAPE_TOKEN). Caller resolves el_N → coords.
    if args.first().map(|s| s.as_str()) == Some("--select") {
        let goal = args.get(1).cloned().unwrap_or_default();
        let cands: Vec<String> = args.get(2..).unwrap_or(&[]).to_vec();
        if cands.is_empty() { println!("{}", serde_json::json!({"index": -1})); return; }
        let list = cands.iter().enumerate().map(|(i, c)| format!("el_{i}: {c}")).collect::<Vec<_>>().join("\n");
        let prompt = format!(
"Pick the ONE on-screen element that best matches the goal. Output ONLY its token (el_0..el_{}) — or `none`
if NONE of them matches.
Goal: {goal}
Elements:
{list}
Answer:", cands.len() - 1);
        // terminal-leading alternation of literals (GBNF-safe) — el_0 | el_1 | … | none
        let alts = (0..cands.len()).map(|i| format!("\"el_{i}\"")).collect::<Vec<_>>().join(" | ");
        let grammar = format!("root ::= {alts} | \"none\"");
        let out = adapter.generate_constrained(&prompt, 8, 0.1, &grammar).map(|(t, _)| t).unwrap_or_default();
        let idx = out.trim().strip_prefix("el_").and_then(|n| n.parse::<i64>().ok()).unwrap_or(-1);
        println!("{}", serde_json::json!({"index": idx, "token": out.trim()}));
        return;
    }

    // NEXT mode (REACTIVE GUI loop, R7b): given the GOAL + the elements ON SCREEN RIGHT NOW, pick the ONE
    // element to click NEXT to make progress — or `done` (goal looks achieved) or `none` (nothing useful
    // visible → settle/re-observe). One step from the live screen, NOT a fixed upfront click-list (the
    // planner's upfront GUI plans don't survive contact). Grammar-constrained el_N | done | none.
    if args.first().map(|s| s.as_str()) == Some("--next") {
        let goal = args.get(1).cloned().unwrap_or_default();
        let cands: Vec<String> = args.get(2..).unwrap_or(&[]).to_vec();
        if cands.is_empty() { println!("{}", serde_json::json!({"token": "none", "index": -1})); return; }
        let list = cands.iter().enumerate().map(|(i, c)| format!("el_{i}: {c}")).collect::<Vec<_>>().join("\n");
        let prompt = format!(
"You are operating a GUI one step at a time. The target application is ALREADY OPEN and focused. Pick the
ONE element to click NEXT to make progress toward the goal. Output ONLY: an element token (el_0..el_{}), or
`done` if the goal already appears achieved, or `none` if no useful element is visible (so the screen should
be re-observed).
RULES: Do NOT click application-launcher / dock / taskbar icons (the app is already open — clicking them
disrupts it). Work INSIDE the window: the menu bar (File/Edit/Image/…), toolbars, dialogs, canvas. Think
about the menu PATH the goal needs (e.g. Image→Mode→Indexed, Layer→Transparency→Add Alpha Channel).
Goal: {goal}
Elements on screen now:
{list}
Next:", cands.len() - 1);
        let alts = (0..cands.len()).map(|i| format!("\"el_{i}\"")).collect::<Vec<_>>().join(" | ");
        let grammar = format!("root ::= {alts} | \"done\" | \"none\"");
        let out = adapter.generate_constrained(&prompt, 8, 0.2, &grammar).map(|(t, _)| t).unwrap_or_default();
        let tok = out.trim().to_string();
        let idx = tok.strip_prefix("el_").and_then(|n| n.parse::<i64>().ok()).unwrap_or(-1);
        println!("{}", serde_json::json!({"token": tok, "index": idx}));
        return;
    }

    // VERIFY mode (R1a — GOAL-LEVEL effect-verify, the spine's plane-switch trigger): after a plane runs,
    // does the GOAL ARTIFACT actually hold? Not rc==0, not a key-readback — derive a READ-ONLY check command
    // + the exact substring that proves success, from the GOAL itself. The adapter runs it; absent ⇒ the CLI
    // plane did not achieve the goal ⇒ reload (R1b) / SWITCH to GUI. Empty check ⇒ unverifiable (stay safe).
    if args.first().map(|s| s.as_str()) == Some("--verify") {
        let goal = args.get(1).cloned().unwrap_or_default();
        // EXIT-CODE semantics (robust): a check command that EXITS 0 iff the goal holds. Handles `grep -q`,
        // `test`, comparisons — no fragile 'substring in output' (a silent `grep -q` printed nothing and a
        // backtick-wrapped check broke the old version). If unprovable from the shell → NONE (don't guess).
        let prompt = format!(
"Write ONE READ-ONLY shell command that EXITS 0 if and only if this goal is ALREADY satisfied on this Linux
machine, and exits non-zero otherwise. Use test, [ ], grep -q, comparisons of `gsettings get`/`dconf read`/
`pactl`/`stat` output. READ-ONLY — never modify anything. Do NOT wrap it in backticks. If the goal cannot be
checked from the shell, output exactly: NONE
Output ONLY the command on one line (or NONE).
Goal: {goal}
Command:");
        let out = adapter.generate(&prompt, 100, 0.1).unwrap_or_default();
        // strip markdown code fences (the model wraps in ```bash … ```) then take the first real command line
        let cleaned = out.replace("```bash", "").replace("```sh", "").replace("```", "");
        let line = cleaned.lines().map(str::trim)
            .map(|l| l.strip_prefix("Command:").unwrap_or(l).trim())
            .map(|l| l.strip_prefix("$").unwrap_or(l).trim())
            .find(|l| !l.is_empty() && !l.starts_with('#'))
            .unwrap_or("")
            .trim_matches('`').trim().to_string();
        let check = if line.eq_ignore_ascii_case("none") { String::new() } else { line };
        println!("{}", serde_json::json!({ "check": check }));
        return;
    }

    // LEAFPICK mode (A — ground the final operation in the ACTUAL on-screen submenu items): the planned leaf
    // can be wrong/hallucinated (e.g. 'CMYK Color' for 'Palette-Based' when the real options are RGB /
    // Grayscale / Indexed). Given the goal + the items REALLY visible, pick the one that achieves it by
    // KNOWLEDGE (Indexed = palette-based) — grammar-constrained el_N | none, no lexical decoy at this level.
    if args.first().map(|s| s.as_str()) == Some("--leafpick") {
        let goal = args.get(1).cloned().unwrap_or_default();
        let items: Vec<String> = args.get(2..).unwrap_or(&[]).to_vec();
        if items.is_empty() { println!("{}", serde_json::json!({"index": -1})); return; }
        let list = items.iter().enumerate().map(|(i, c)| format!("el_{i}: {c}")).collect::<Vec<_>>().join("\n");
        let prompt = format!(
"These are the menu items VISIBLE on screen right now. Pick the ONE that accomplishes the goal — use your
KNOWLEDGE of what each item does (e.g. 'Indexed' makes an image palette-based). Output ONLY its token
(el_0..el_{}), or `none` if none of them does.
Goal: {goal}
Items:
{list}
Answer:", items.len() - 1);
        let alts = (0..items.len()).map(|i| format!("\"el_{i}\"")).collect::<Vec<_>>().join(" | ");
        let grammar = format!("root ::= {alts} | \"none\"");
        let out = adapter.generate_constrained(&prompt, 8, 0.1, &grammar).map(|(t, _)| t).unwrap_or_default();
        let idx = out.trim().strip_prefix("el_").and_then(|n| n.parse::<i64>().ok()).unwrap_or(-1);
        println!("{}", serde_json::json!({"index": idx, "token": out.trim()}));
        return;
    }

    // MENUPATH mode (F13 — the knowledge frame, not the selection frame): per-step menu SELECTION fails
    // (the model lexically picks the menu whose NAME matches a goal word — "image" → Image — 9/9 wrong for
    // a goal whose function lives under Layer). But asked as a KNOWLEDGE question ("what is the menu path?")
    // the SAME model answers correctly (Layer > Transparency > Add Alpha Channel, 6/6). So plan the path
    // here, then the adapter FOLLOWS it deterministically (match each token on screen, fail-closed). Grounded
    // in the ACTUAL menu-bar menus so the first token is real (and the app is implied without naming it).
    if args.first().map(|s| s.as_str()) == Some("--menupath") {
        let goal = args.get(1).cloned().unwrap_or_default();
        let app = args.get(2).cloned().unwrap_or_default();
        let app = if app.trim().is_empty() { "this application".to_string() } else { app };
        // RE-PLAN (args 3+): paths already tried that produced NO change → ask for a DIFFERENT next operation.
        let tried: Vec<String> = args.get(3..).unwrap_or(&[]).to_vec();
        let tried_block = if tried.is_empty() { String::new() } else {
            format!("\nThese were ALREADY tried and changed nothing — give a DIFFERENT path for the NEXT step \
toward the goal, do NOT repeat them:\n- {}", tried.join("\n- "))
        };
        // KNOWLEDGE frame, naming the app — proven 5/5 correct (Layer > Transparency …). Do NOT list the
        // menu bar here: priming the model with the menu names re-triggers the lexical mis-pick (Image …).
        let prompt = format!(
"In {app}, give the EXACT menu-bar path that accomplishes the goal, as names separated by ' > '
(menu > submenu > item). Use your KNOWLEDGE of where the function lives — do NOT pick a menu just because its
name matches a word in the goal. Output ONLY the path, e.g. `Layer > Transparency > Add Alpha Channel`.
Goal: {goal}{tried_block}
Path:");
        let out = adapter.generate(&prompt, 48, 0.1).unwrap_or_default();
        let line = out.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
        let path: Vec<String> = line.split('>')
            .map(|s| s.trim().trim_matches('`').trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        println!("{}", serde_json::json!({ "path": path }));
        return;
    }

    let instruction: String = args.join(" ");
    if instruction.trim().is_empty() {
        eprintln!("usage: osworld_plan \"<task instruction>\"  |  osworld_plan --reground GOAL FAILED ERR DISCOVERY");
        std::process::exit(2);
    }

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
