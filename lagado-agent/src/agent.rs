use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use crate::types::{PipelineError, Step, ToolCall};
use crate::forge::Forge;
use crate::operator::StepEnforcer;
use crate::memory::Memory;
use crate::inference::InferenceAdapter;
use crate::perception::{Perceptor, Actuator};
use crate::recovery::FailureType;
use crate::{chronos, config, envelope, gate, tools};
use crate::recovery::{RecoveryManager, RecoveryOutcome};
use crate::action_graph::ActionGraph;
use crate::skill_library::{Skill, SkillLibrary};
use tokio::sync::Mutex as TokioMutex;
use blake3;


// ── State shared between WebSocket and agent ──────────────────────
pub struct AgentState {
    pub goal: String,
    pub running: bool,
    pub approval_tx: Option<mpsc::Sender<bool>>,
    pub pending_id: Option<String>,
}

// ── Tool execution ────────────────────────────────────────────────
async fn execute_tool(
    call: &ToolCall,
    actuator: &Arc<dyn Actuator>,
    perceptor: &dyn Perceptor,
    memory_tiers: &Arc<tokio::sync::Mutex<crate::memory_tiers::MemoryTiers>>,
) -> String {
    // Actuation is a blocking SSH round-trip per action → run it on the blocking pool so the click/
    // type/key wait never freezes the async runtime (Theme 1). The Arc moves into the closure.
    match call {
        ToolCall::Click { selector } => {
            let (a, sel) = (actuator.clone(), selector.clone());
            tokio::task::spawn_blocking(move || a.click(&sel)).await.unwrap_or_default()
        }
        ToolCall::Type { selector, text } => {
            let (a, sel, t) = (actuator.clone(), selector.clone(), text.clone());
            tokio::task::spawn_blocking(move || a.type_text(&sel, &t)).await.unwrap_or_default()
        }
        ToolCall::Key { key } => {
            let (a, k) = (actuator.clone(), key.clone());
            tokio::task::spawn_blocking(move || a.key(&k)).await.unwrap_or_default()
        }
        ToolCall::Wait { ms } => {
            tokio::time::sleep(tokio::time::Duration::from_millis(*ms as u64)).await;
            format!("Waited {}ms", ms)
        }
        ToolCall::Task { description } => format!("Task completed: {}", description),
        ToolCall::Done { reason } => format!("Done: {}", reason),
        ToolCall::Chat { text } => text.clone(),
        ToolCall::Invoke { name, args } => {
            dispatch_invoke(name, args, actuator, perceptor, memory_tiers).await
        }
    }
}

/// Full Invoke dispatcher — routes to native executor or subsystem tools.
async fn dispatch_invoke(
    name: &str,
    args: &serde_json::Map<String, serde_json::Value>,
    actuator: &Arc<dyn Actuator>,
    perceptor: &dyn Perceptor,
    memory_tiers: &Arc<tokio::sync::Mutex<crate::memory_tiers::MemoryTiers>>,
) -> String {
    // Try self-contained native tools first
    if let Some(result) = crate::tools::executor::dispatch(name, args).await {
        return result;
    }

    // VM tools — route through actuator/perceptor
    let s = |key: &str| args.get(key).and_then(|v| v.as_str()).unwrap_or("").to_string();
    match name {
        "screenshot" => {
            // Capture via QMP screendump (same path as the live feed)
            match std::fs::read(crate::config::FRAME_PATH) {
                Ok(bytes) => {
                    use base64::Engine;
                    base64::engine::general_purpose::STANDARD.encode(&bytes)
                }
                Err(e) => format!("error: no frame available: {e}"),
            }
        }
        "vm_command" => { let (a, c) = (actuator.clone(), s("command")); tokio::task::spawn_blocking(move || a.run_command(&c)).await.unwrap_or_default() }
        "vm_type"    => { let (a, t) = (actuator.clone(), s("text"));    tokio::task::spawn_blocking(move || a.type_text("focused", &t)).await.unwrap_or_default() }
        "vm_click"   => { let (a, sel) = (actuator.clone(), s("selector")); tokio::task::spawn_blocking(move || a.click(&sel)).await.unwrap_or_default() }

        // Memory tools — delegate to MemoryTiers
        "memory_store" => {
            let key = s("key"); let value = s("value");
            let mut tiers = memory_tiers.lock().await;
            tiers.push_episode_id(format!("{key}: {value}"))
                .map(|_| format!("stored {key}"))
                .unwrap_or_else(|e| format!("error: {e}"))
        }
        "memory_get" => {
            let key = s("key");
            let tiers = memory_tiers.lock().await;
            let ctx = tiers.assemble_context(512);
            if ctx.is_empty() { format!("no memory entry for '{key}'") }
            else {
                // Filter assembled context to lines containing the key
                let matching: Vec<&str> = ctx.lines()
                    .filter(|l| l.contains(&key))
                    .collect();
                if matching.is_empty() { format!("no memory entry for '{key}'") }
                else { matching.join("\n") }
            }
        }
        "memory_list" => {
            let tiers = memory_tiers.lock().await;
            let ctx = tiers.assemble_context(4096);
            if ctx.is_empty() { "memory is empty".to_string() } else { ctx }
        }
        "memory_delete" => {
            // MemoryTiers doesn't yet have delete-by-key; decay handles cleanup
            format!("memory_delete: use tool_config.json to disable tools or let decay handle cleanup")
        }

        _ => format!("unknown tool: {name}"),
    }
}

// ── Permission request + await human approval ─────────────────────
async fn request_and_await_approval(
    confirm_type: &str, // "tap" | "typed"
    tool_call: &ToolCall,
    state: &Arc<Mutex<AgentState>>,
    actuator: &Arc<dyn Actuator>,
    perceptor: &dyn Perceptor,
    memory_tiers: &Arc<tokio::sync::Mutex<crate::memory_tiers::MemoryTiers>>,
    approval_rx: &mut mpsc::Receiver<bool>,
    confirm_tx: &mpsc::Sender<String>,
) -> String {
    let desc = gate::describe(tool_call);
    let desc_safe = gate::describe_redacted(tool_call);
    let tool_name: String = match tool_call {
        ToolCall::Click { .. }  => "click".to_string(),
        ToolCall::Type { .. }   => "type".to_string(),
        ToolCall::Key { .. }    => "key".to_string(),
        ToolCall::Wait { .. }   => "wait".to_string(),
        ToolCall::Done { .. }   => "done".to_string(),
        ToolCall::Task { .. }   => "task".to_string(),
        ToolCall::Chat { .. }   => "chat".to_string(),
        ToolCall::Invoke { name, .. } => name.clone(),
    };
    let id = uuid::Uuid::new_v4().to_string();
    chronos::log(&format!("confirm_requested: {confirm_type}: {desc_safe}"));
    let _ = confirm_tx
        .send(envelope::make(
            "permission",
            envelope::PermissionPayload {
                id: id.clone(),
                type_: confirm_type.to_string(),
                tool: tool_name,
                action: desc.clone(),
                reason: "Write action requires confirmation".to_string(),
                origin_surface: "immersive".to_string(),
                origin_agent: "main".to_string(),
            },
        ))
        .await;
    // Set pending_id (lock, set, drop guard) BEFORE awaiting approval
    {
        state.lock().await.pending_id = Some(id);
    }
    let approved = approval_rx.recv().await.unwrap_or(false);
    if approved {
        let out = execute_tool(tool_call, actuator, perceptor, memory_tiers).await;
        chronos::log(&format!("action: {desc_safe} -> {out}"));
        let _ = confirm_tx
            .send(envelope::make(
                "action_log",
                envelope::ActionLogPayload {
                    text: format!("{desc_safe} -> {out}"),
                },
            ))
            .await;
        out
    } else {
        chronos::log(&format!("denied: {desc_safe}"));
        let _ = confirm_tx
            .send(envelope::make(
                "status",
                envelope::StatusPayload {
                    state: "denied".to_string(),
                    detail: desc_safe.clone(),
                },
            ))
            .await;
        format!("Denied by user: {:?}", tool_call)
    }
}

/// PLAN-LEVEL APPROVAL (Option 2). A plan needs the single up-front approval if it contains any step
/// that would otherwise confirm — i.e. anything that is NOT a read-only command (a write/destructive
/// command, or any Type/Key/Click). An all-read-only plan auto-runs with no preview.
fn plan_requires_approval(sub_goals: &[SubGoal]) -> bool {
    sub_goals.iter().any(|sg| match &sg.action {
        SubAction::Command(cmd) => !crate::gate::is_read_only_command(cmd),
        SubAction::Type(_) | SubAction::Key(_) | SubAction::Click => true,
    })
}

/// Render the whole decomposed plan for the one-time preview. Destructive command steps are flagged —
/// they STILL hard-stop individually even after the plan is approved.
fn render_plan_preview(sub_goals: &[SubGoal]) -> String {
    let mut lines = vec!["Here's my plan — approve to run it:".to_string()];
    for (i, sg) in sub_goals.iter().enumerate() {
        let danger = matches!(&sg.action, SubAction::Command(c) if crate::gate::is_destructive_text(c));
        let mark = if danger { "   ⚠ destructive — I'll still ask before this step" } else { "" };
        lines.push(format!("{}. {}{}", i + 1, sg.text, mark));
    }
    lines.join("\n")
}

/// Request ONE approval for the whole plan. Mirrors `request_and_await_approval` but the frozen
/// permission envelope carries the plan (as `action`) instead of a single tool call.
async fn request_plan_approval(
    preview: &str,
    state: &Arc<Mutex<AgentState>>,
    approval_rx: &mut mpsc::Receiver<bool>,
    confirm_tx: &mpsc::Sender<String>,
) -> bool {
    let id = uuid::Uuid::new_v4().to_string();
    chronos::log("plan_approval_requested");
    let _ = confirm_tx
        .send(envelope::make(
            "permission",
            envelope::PermissionPayload {
                id: id.clone(),
                type_: "tap".to_string(),
                tool: "plan".to_string(),
                action: preview.to_string(),
                reason: "Approve this plan before I run it".to_string(),
                origin_surface: "immersive".to_string(),
                origin_agent: "main".to_string(),
            },
        ))
        .await;
    {
        state.lock().await.pending_id = Some(id);
    }
    approval_rx.recv().await.unwrap_or(false)
}

/// Declare the goal done — but ONLY after verifying its GOAL-DERIVED world-state holds. If an intended
/// artifact is absent (plan incompleteness, or a step that achieved its own effect but not the goal),
/// hand back instead of falsely claiming success. Emits the terminal status either way.
async fn complete_goal(goal: &str, actuator: &dyn Actuator, confirm_tx: &mpsc::Sender<String>) {
    for check in goal_postconditions(goal) {
        if parse_exit_code(&actuator.run_command(&check)) != Some(0) {
            chronos::log(&format!("goal_postcondition_failed: {check}"));
            let _ = confirm_tx.send(envelope::make("status", envelope::StatusPayload {
                state: "goal_done".to_string(),
                detail: format!("I ran the steps, but the goal isn't fully done — `{check}` doesn't hold. Handing back to you."),
            })).await;
            return;
        }
    }
    chronos::log("goal_verified: goal postconditions hold");
    // ONE message: the status carries the human detail (the UI renders it + flips to idle). Sending it
    // ALSO as an action_log duplicated "Goal accomplished" in the chat.
    let _ = confirm_tx.send(envelope::make("status", envelope::StatusPayload {
        state: "goal_done".to_string(), detail: "Goal accomplished — all steps completed.".to_string() })).await;
}

/// The VERIFICATION CONTRACT (the golden line, at the goal level): does the goal's expected end-state
/// already hold in the world? Runs the goal-derived postcondition checks deterministically. No
/// derivable check → false (can't verify ⇒ don't claim — fail-closed). All checks pass → true.
async fn goal_satisfied(goal: &str, actuator: &dyn Actuator) -> bool {
    let checks = goal_postconditions(goal);
    if checks.is_empty() { return false; }
    for check in &checks {
        if parse_exit_code(&actuator.run_command(check)) != Some(0) { return false; }
    }
    true
}

/// At a handback decision, FIRST verify the goal isn't ALREADY satisfied. The benchmark exposed
/// "under-claims": the agent DID the work (world-state ✅) but handed back because the sequencer/
/// supervisor didn't recognize completion. So check the world before giving up — if the goal holds,
/// claim success (expectation==observation); otherwise hand back as before.
async fn verify_or_handback(goal: &str, actuator: &dyn Actuator, confirm_tx: &mpsc::Sender<String>, handback_detail: &str) {
    if goal_satisfied(goal, actuator).await {
        chronos::log("verified_on_handback: goal already satisfied → claim success, not handback");
        complete_goal(goal, actuator, confirm_tx).await;
        return;
    }
    let _ = confirm_tx.send(envelope::make("status", envelope::StatusPayload {
        state: "goal_done".to_string(), detail: handback_detail.to_string(),
    })).await;
}

// ── Observation helpers ───────────────────────────────────────────

/// Returns Some(observation text) when an executed action's screen effect should be
/// reported to the model, or None when no injection is appropriate (first turn,
/// no prior action executed, or action was denied/blocked).
///
/// Injection rule:
/// - `None`  when `prev_executed` is false (no action ran last turn).
/// - `Some("did NOT change…")` when hashes are equal.
/// - `Some("changed — N appeared, M disappeared")` otherwise, with up to 5 sample ref lines.
pub fn observation_for(prev_executed: bool, prev: &str, curr: &str) -> Option<String> {
    if !prev_executed {
        return None;
    }
    if blake3::hash(prev.as_bytes()) == blake3::hash(curr.as_bytes()) {
        return Some(
            "OBSERVATION: the screen did NOT change after your last action — \
             it had no visible effect. Do not repeat the same action; reconsider.".to_string(),
        );
    }
    let prev_refs: std::collections::HashSet<&str> =
        prev.lines().filter(|l| l.contains("ref_")).collect();
    let curr_refs: std::collections::HashSet<&str> =
        curr.lines().filter(|l| l.contains("ref_")).collect();
    let n_new  = curr_refs.difference(&prev_refs).count();
    let n_gone = prev_refs.difference(&curr_refs).count();
    let sample: Vec<&str> = curr_refs.difference(&prev_refs).take(5).map(|s| s.trim()).collect();
    let appeared = if sample.is_empty() { String::new() } else { format!(" {}", sample.join(", ")) };
    Some(format!(
        "OBSERVATION: your last action changed the screen. \
         {n_new} new elements appeared:{appeared}. {n_gone} elements disappeared."
    ))
}

/// True when the agent should abort rather than execute a 3rd+ identical consecutive action.
///
/// `count`          — number of times `last` was already EXECUTED (2 = this would be the 3rd).
/// `screen_unchanged` — whether the screen is identical to what it was before the 2nd attempt.
///
/// Scroll-type repeats where the screen changed survive (`screen_unchanged = false`).
pub fn should_cutoff(current: &str, last: &str, count: usize, screen_unchanged: bool) -> bool {
    current == last && count >= 2 && screen_unchanged
}

/// Decompose a goal into ordered sub-goals on EXPLICIT sequential markers only. Deterministic
/// (the model cannot decompose — it emits a spurious `complete` even handed the plan, §2.14 — so
/// the harness does ordering, never the model). CONSERVATIVE by design: it splits only on clear
/// imperative-sequence connectives, never semantically. A goal with no marker is a single sub-goal.
/// A compound-but-unmarked goal ("find the cheapest flight and book it") stays ONE sub-goal and
/// relies on the executor + the supervisor's deviation/impasse handback — never a mangled plan
/// (§2.15: un-parseable → clean handback, not garbage steps). Pure — unit-testable.
pub fn decompose_goal(goal: &str) -> Vec<String> {
    // Markers ordered so multi-word forms are tried before their substrings.
    const MARKERS: &[&str] = &[", and then ", " and then ", ", then ", " then ", "; ", " after that "];
    // `;` is BOTH an agent separator ("open A ; open B") AND shell syntax. Inside a "run the command …"
    // payload it is SHELL (one compound command) — splitting it stranded the tail ("rm x") as a bogus
    // Click step. So exclude `;` from the markers FOR a command-lead part only; "then" still separates
    // distinct directives ("run the command A then run the command B").
    const SHELL_OP: &str = "; ";
    let mut parts: Vec<String> = vec![goal.trim().to_string()];
    loop {
        let mut next = Vec::new();
        let mut split_any = false;
        for p in &parts {
            let lower = p.to_lowercase();
            let is_command = COMMAND_LEADS.iter().any(|&l| lower.starts_with(l));
            // earliest marker position across all markers (`;` skipped inside a command payload)
            let cut = MARKERS.iter()
                .filter(|m| !(is_command && **m == SHELL_OP))
                .filter_map(|m| lower.find(m).map(|i| (i, m.len()))).min_by_key(|(i, _)| *i);
            match cut {
                Some((i, len)) => {
                    next.push(p[..i].trim().to_string());
                    next.push(p[i + len..].trim().to_string());
                    split_any = true;
                }
                None => next.push(p.clone()),
            }
        }
        parts = next;
        if !split_any { break; }
    }
    parts.into_iter().filter(|s| !s.is_empty()).collect()
}

/// A planned step's action class (Wall 2). The executor selects+clicks for `Click` (model in the
/// loop), but `Type`/`Key` are DETERMINISTIC one-shot harness actions — there is no element to
/// "click", and the model cannot be asked to pick a keystroke. They bypass perception/selection/
/// fail-closed/grammar entirely and execute through the safety gate, then fire-and-advance.
#[derive(Debug, Clone, PartialEq)]
pub enum SubAction {
    Click,
    Type(String),
    Key(String),
    /// A shell-command step routed through the gated command channel (run + read
    /// stdout/stderr/exit), NOT GUI typing. Advances on exit 0 (deterministic verification).
    Command(String),
}

/// One planned sub-goal: the original text (drives Click selection + logging) and its action class.
#[derive(Debug, Clone)]
pub struct SubGoal {
    pub text:   String,
    pub action: SubAction,
}

/// Classify a planned sub-goal string into its action class. Patterns are FEW and EXPLICIT
/// (NL→action parsing is the fragile part): a leading "type …" → Type with the literal payload
/// (ORIGINAL case preserved — commands are case-sensitive); a leading "press …"/"hit …" → Key;
/// everything else → Click (handled by the selection loop).
/// Explicit shell-command directives that route a step (and the goal's intent) to the command
/// channel rather than GUI typing. SINGLE SOURCE OF TRUTH — also consumed by hydra's intent
/// fast-path (`opens_with_command_phrase`) so classification and execution agree (a step the
/// sequencer would run as a command must not be misrouted to CHAT upstream).
pub const COMMAND_LEADS: &[&str] = &[
    "run the command", "run command", "execute the command", "execute command",
    "run shell command", "run shell", "run:", "$ ",
];

pub fn classify_subgoal(s: &str) -> SubGoal {
    let t = s.trim();
    let lower = t.to_lowercase();
    // Command channel: an explicit shell-command step routes to the deterministic CLI channel
    // (run + read output + exit-code verify), NOT GUI typing. Triggers are NARROW/EXPLICIT so
    // "Launch Firefox" (Click) and "type the command: …" (Type, into a focused field) are unaffected.
    for &lead in COMMAND_LEADS {
        if lower.starts_with(lead) {
            let payload = t[lead.len()..].trim_start_matches([':', ' ']).trim();
            if !payload.is_empty() {
                return SubGoal { text: t.to_string(), action: SubAction::Command(payload.to_string()) };
            }
        }
    }
    // WRITE-FILE PRIMITIVE: "write to <path>: <content>" authors file content ROBUSTLY (base64
    // round-trip — the content is never shell-escaped), instead of the brittle multi-line `echo` the
    // small model mangles (it emitted literal `\n` + invented commands). `\n`/`\t` in the content are
    // interpreted. Routes through the command channel as a base64-decode write → gated, verified,
    // postconditioned (a redirect `> path` → `test -e path`) like any command.
    if let Some((path, content)) = parse_write_file(t) {
        // ACTION-TYPE GUARD (planner mis-decomposition): the weak planner emits `write to <path>: <cmd>`
        // for what is really a RUN/MAKE step — e.g. `write to /tmp/osw_proj: mkdir /tmp/osw_proj` writes
        // the literal text "mkdir …" into a FILE named /tmp/osw_proj, corrupting the target. Catch it
        // deterministically: an EXTENSIONLESS target (a dir/repo name, not file.ext) whose content is a
        // single bare shell command → run the command instead of writing it. The extension check keeps a
        // genuine script write safe (`write to run.sh: rm -rf x` stays a write — .sh has an extension).
        let basename = path.rsplit('/').next().unwrap_or(&path);
        let extensionless = !basename.contains('.');
        if extensionless && !content.contains('\n') && is_bare_shell_command(content.trim()) {
            return SubGoal { text: t.to_string(), action: SubAction::Command(content.trim().to_string()) };
        }
        use base64::Engine;
        let bytes = content.replace("\\n", "\n").replace("\\t", "\t");
        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes.as_bytes());
        // mkdir -p the PARENT first: writing a file to a nested path inherently needs its directory
        // (the bug a user hit — "create folder /tmp/myapp with main.py inside" failed because the
        // planner never made /tmp/myapp; writing a file is deterministically allowed to create its dir).
        let cmd = match path.rsplit_once('/') {
            Some((dir, _)) if !dir.is_empty() => format!("mkdir -p {dir} && echo {b64} | base64 -d > {path}"),
            _ => format!("echo {b64} | base64 -d > {path}"),
        };
        return SubGoal { text: t.to_string(), action: SubAction::Command(cmd) };
    }
    if lower.starts_with("press ") {
        return SubGoal { text: t.to_string(), action: SubAction::Key(normalize_key(&t[6..])) };
    }
    if lower.starts_with("hit ") {
        return SubGoal { text: t.to_string(), action: SubAction::Key(normalize_key(&t[4..])) };
    }
    if lower.starts_with("type ") {
        let payload = strip_type_lead(&t[5..]);
        if !payload.is_empty() {
            return SubGoal { text: t.to_string(), action: SubAction::Type(payload) };
        }
    }
    // A BARE shell command that lost its "run the command" lead — a fragment of a split compound step,
    // or a planner step that dropped the prefix. Recognized only with a concrete shell argument so NL
    // ("find my documents", "open the menu") stays a Click.
    if is_bare_shell_command(t) {
        return SubGoal { text: t.to_string(), action: SubAction::Command(t.to_string()) };
    }
    SubGoal { text: t.to_string(), action: SubAction::Click }
}

/// Parse a write-file directive `write to <path>: <content>` (also "write the file <path> …: …") into
/// (path, content). Requires an explicit `:` separating the path clause from the content, and a path
/// before it. Deliberately does NOT match "write the text X into Y" (no colon → the planner emits a
/// plain `echo` for simple single-line text). PURE.
pub fn parse_write_file(s: &str) -> Option<(String, String)> {
    let lo = s.trim().to_lowercase();
    if !(lo.starts_with("write to ") || lo.starts_with("write the file ") || lo.starts_with("write file ")) {
        return None;
    }
    let colon = s.find(':')?;
    let content = s[colon + 1..].trim();
    if content.is_empty() {
        return None;
    }
    let path = s[..colon].split_whitespace()
        .find(|t| t.contains('/'))?
        .trim_matches(|c: char| matches!(c, '"' | '\'' | '`'));
    Some((path.to_string(), content.to_string()))
}

/// True if the fragment IS a bare shell command: a known non-GUI tool AND a concrete shell argument
/// (a path, a flag, or a redirect). CONSERVATIVE by design — a shell-tool name alone ("find", "echo")
/// without a path/flag is NOT enough, so natural language ("find my documents", "echo your thoughts")
/// and GUI phrasing ("open the menu") stay a Click. GUI verbs (open/launch/click) are not in the set.
fn is_bare_shell_command(s: &str) -> bool {
    let s = s.trim();
    let first = s.split_whitespace().next().unwrap_or("");
    const SHELL_BINS: &[&str] = &[
        "rm", "rmdir", "ls", "cat", "touch", "mkdir", "cp", "mv", "ln", "chmod", "chown", "grep",
        "egrep", "fgrep", "find", "head", "tail", "wc", "sort", "uniq", "cut", "tr", "sed", "awk",
        "tar", "gzip", "gunzip", "zip", "unzip", "curl", "wget", "git", "make", "stat", "file", "df",
        "du", "ps", "kill", "pkill", "md5sum", "sha256sum", "mount", "umount", "ping", "echo",
        "python3", "node", "npm", "pip3",
    ];
    if !SHELL_BINS.contains(&first) {
        return false;
    }
    let rest = s[first.len()..].trim();
    rest.contains('/') || rest.contains('>') || rest.split_whitespace().any(|w| w.starts_with('-'))
}

/// Strip a "type" sub-goal's framing prefix ("the command:", "command:", "the text:", …), leaving
/// the literal text to type. Case preserved; only the recognised lead is removed.
fn strip_type_lead(s: &str) -> String {
    let r = s.trim();
    let low = r.to_lowercase();
    for lead in ["the command:", "the text:", "the following:", "command:", "text:", "in:"] {
        if low.starts_with(lead) {
            return r[lead.len()..].trim_start_matches([':', ' ']).trim().to_string();
        }
    }
    r.to_string()
}

/// Map a natural key name to an xdotool keysym ("enter"→"Return", "esc"→"Escape", …). Strips a
/// leading "the " and a trailing " key"; unknown names are best-effort capitalised.
fn normalize_key(s: &str) -> String {
    let mut k = s.trim().to_lowercase();
    k = k.strip_prefix("the ").unwrap_or(&k).to_string();
    k = k.strip_suffix(" key").unwrap_or(&k).trim().to_string();
    match k.as_str() {
        "enter" | "return"        => "Return".to_string(),
        "tab"                     => "Tab".to_string(),
        "escape" | "esc"          => "Escape".to_string(),
        "space" | "spacebar" | "the space bar" => "space".to_string(),
        "backspace"               => "BackSpace".to_string(),
        "delete" | "del"          => "Delete".to_string(),
        "up"   => "Up".to_string(),   "down"  => "Down".to_string(),
        "left" => "Left".to_string(), "right" => "Right".to_string(),
        "home" => "Home".to_string(), "end"   => "End".to_string(),
        other => {
            let mut c = other.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None    => "Return".to_string(),
            }
        }
    }
}

/// Assemble the executor (action-selection) prompt. **MEMORY-ISOLATED BY CONSTRUCTION (invariant
/// #10):** this takes ONLY the pinned system prompt, the ranked+capped candidate block (or the raw
/// screen as fallback), and the discriminating goal phrase. It has NO parameter for episodic /
/// visual / skill memory — §4.3 proved that prepended memory silently flips the pick. New senses
/// (CV, OmniParser captions, DOM labels) feed `candidate_block` (their text is *element label* data,
/// which the executor is meant to see), never a memory slot. The SYS preamble LENGTH is load-bearing
/// (lands the candidate list in the late-attention band, §2.6) — do not reorder this template.
pub fn build_executor_prompt(
    system_prompt: &str,
    candidate_block: &str,
    screen: &str,
    prompt_goal: &str,
) -> String {
    let screen_section = if candidate_block.is_empty() {
        format!("Screen:\n{screen}\n\n")
    } else {
        format!("{candidate_block}\n")
    };
    format!("{system_prompt}\n\n{screen_section}Goal: {prompt_goal}\n\nWhat is your next action?")
}

/// Strip a leading list marker ("1.", "- ", "* ", "2) ", "• ") and surrounding whitespace from a
/// planner output line.
fn strip_list_marker(line: &str) -> String {
    line.trim()
        .trim_start_matches(|c: char| c.is_ascii_digit() || matches!(c, '.' | ')' | '-' | '*' | '•' | ' '))
        .trim()
        .to_string()
}

/// Board-informed planner (Wall 1 — precondition planning). Expands an IMPLICIT single-phrase goal
/// into an ordered list of on-screen sub-goals, making implicit PRECONDITIONS explicit (e.g. an app
/// that lives inside a menu needs that menu opened first — the gap that made "Launch the Terminal
/// Emulator" fail-closed on a bare desktop).
///
/// Informed by LEARNED skills (the Board / skill memory): this is **store-vs-INFLUENCE** in action —
/// memory shapes the PLAN here, and is still kept OUT of the executor's click prompt (invariant #10,
/// where prepended memory flips the pick). Planning needs world knowledge, so it is the one LLM step
/// on the *strategy* side; the executor stays deterministic on the rails.
///
/// Conservative by construction — it can only ADD precondition steps, never corrupt the deterministic
/// path:
/// - An EXPLICIT multi-step goal ("X then Y") keeps the deterministic connective-marker split,
///   untouched (zero regression on tasks that already work).
/// - The model plan is adopted ONLY if it actually expanded a single goal into >1 step; otherwise,
///   or on any model error, it falls back to the original goal.
pub fn plan_goal(goal: &str, env: &str, skills: &[Skill], adapter: &Arc<dyn InferenceAdapter>) -> Vec<String> {
    let syntactic = decompose_goal(goal);
    if syntactic.len() > 1 {
        return syntactic; // explicit multi-step → trust the deterministic split
    }
    let skill_block = SkillLibrary::format_for_prompt(skills);
    // CAPABILITY-AWARE prompt: the planner knows the agent's TWO action surfaces and CHOOSES per goal.
    // Verified on the live 8B (planner_probe): it decomposes implicit goals and picks CLI-vs-GUI well
    // for well-specified goals; the `no sudo / no interactive programs` rules + the post-filter below
    // catch the failure modes the probe exposed (hallucinated/dangerous/hang-prone commands).
    let prompt = format!(
"Break the goal into the FEWEST concrete steps, one per line. The agent can act two ways:
- run the command <shell command>   — runs a shell command and reads its output. STRONGLY PREFER this:
  file operations, system info, running a program, package work, AND changing SETTINGS / PREFERENCES /
  CONFIGURATION (use gsettings/dconf — desktop and app settings are changeable from the terminal without
  the GUI). Almost everything a desktop does can be done from the command line; try it FIRST.
- write to <path>: <content>        — writes the given content to a file (use \\n for a newline). Use
  this to author a SCRIPT or config file with multi-line content — never a multi-line echo.
- Click <element>                   — clicks an on-screen GUI element. LAST RESORT — use ONLY to launch a
  GUI app or for a task with NO command-line way. A settings/config task is NOT GUI-only: use gsettings/
  dconf. To open an app: Click the Applications menu, then Click <the app>.

Rules:
- One action per line. Pick the SIMPLEST surface for the goal.
- GROUND every path in 'Current files' below: when the goal names a file or directory, use its EXACT
  FULL absolute path from that listing (e.g. a goal saying 'photos' when the listing shows
  /home/user/Desktop/photos → use /home/user/Desktop/photos). NEVER a bare relative name (the command
  runs from a different directory) and NEVER an invented /path/to/... placeholder.
- Output ONLY the steps. No narration, no 'locate'/'wait'/'verify'/'check'/'open the folder'.
- Do NOT use sudo. Do NOT use interactive programs (nano, vim, less, top, man) — they hang.
- A 'write to' step authors ONLY the file's real content. Put chmod/run as SEPARATE later steps —
  never inside the file's content. To run a script you wrote: a separate 'run the command sh <path>'.
- If the goal is a single action, output ONE line.

Example:
Goal: create an empty file at /tmp/notes.txt
Steps:
run the command touch /tmp/notes.txt

Example:
Goal: delete the file /tmp/old.log
Steps:
run the command rm /tmp/old.log

Example:
Goal: create a directory /tmp/proj and an empty file notes.txt inside it
Steps:
run the command mkdir -p /tmp/proj
run the command touch /tmp/proj/notes.txt

Example:
Goal: create a git repository in /tmp/repo
Steps:
run the command git init /tmp/repo

Example:
Goal: write a script /tmp/run.sh that creates /tmp/out, make it executable, and run it
Steps:
write to /tmp/run.sh: #!/bin/sh\\ntouch /tmp/out
run the command chmod +x /tmp/run.sh
run the command sh /tmp/run.sh

Example:
Goal: create a git repository in /tmp/repo, add a file notes.txt to it, and make a commit
Steps:
run the command git init /tmp/repo
run the command touch /tmp/repo/notes.txt
run the command git -C /tmp/repo add notes.txt
run the command git -C /tmp/repo commit -m \"add notes.txt\"

Example:
Goal: open the web browser
Steps:
Click the Applications menu
Click Web Browser
{skill_block}
Current files (the command runs from the user's home directory; use these EXACT absolute paths):
{env}
Goal: {goal}
Steps:"
    );
    // Lead verbs that are not a discrete action — the planner sometimes emits narration
    // ("Locate X", "Wait for Y") that has no on-screen element / no command and would only fail-close.
    const NON_ACTION_LEAD: &[&str] =
        &["wait", "locate", "find", "ensure", "verify", "confirm", "observe", "check", "look", "see"];
    match adapter.generate(&prompt, 192, 0.1) {
        Ok(text) => {
            let steps: Vec<String> = text
                .lines()
                .map(strip_list_marker)
                .filter(|l| !l.is_empty())
                .filter(|l| {
                    let lead = l.split_whitespace().next().unwrap_or("").to_lowercase();
                    !NON_ACTION_LEAD.contains(&lead.as_str())
                })
                .collect();
            // GUARDRAIL (fail-closed, per the probe): a plan that needs sudo or an interactive program
            // would HANG the (TTY-less) command channel — reject the whole LLM plan and fall back to the
            // conservative deterministic path rather than execute a hanging step. Keeps plan_goal's
            // can-only-improve-never-corrupt property.
            if steps.iter().any(|s| command_would_hang(s)) {
                chronos::log(&format!("planner: rejected plan with hang-prone step for \"{goal}\" → deterministic"));
                return syntactic;
            }
            // Adopt when the model produced a real expansion: multiple steps, OR a single step that
            // CHOSE the command surface (a CLI plan the deterministic click-split could never produce).
            // A lone restated click adds nothing → fall back.
            let chose_command = steps.iter().any(|s| matches!(classify_subgoal(s).action, SubAction::Command(_)));
            if steps.len() > 1 || (steps.len() == 1 && chose_command) {
                chronos::log(&format!("planner: expanded \"{goal}\" → {steps:?}"));
                steps
            } else {
                syntactic
            }
        }
        Err(e) => {
            chronos::log(&format!("planner: model error ({e}) — deterministic decomposition"));
            syntactic
        }
    }
}

/// A planned step that would HANG the command channel: `sudo` (no TTY for the password prompt) or an
/// interactive program (editor/pager/monitor that never returns without a terminal). The command
/// channel runs over non-interactive SSH, so these block forever — reject the plan rather than run them.
fn command_would_hang(step: &str) -> bool {
    let cmd = match classify_subgoal(step).action {
        SubAction::Command(c) => c.to_lowercase(),
        _ => return false,
    };
    let first = cmd.split_whitespace().next().unwrap_or("");
    if first == "sudo" {
        return true;
    }
    const INTERACTIVE: &[&str] = &[
        "nano", "vim", "vi", "emacs", "pico", "less", "more", "man", "top", "htop",
        "vimdiff", "tmux", "screen", "ssh", "telnet", "ftp", "python", "python3", "node", "irb",
    ];
    INTERACTIVE.contains(&first)
}

// ── REASSESS: diagnose → reform → bounded retry → escalate (command class) ───────────────────────────
// The supervisor's reapproach muscle for commands ("forge retries; the supervisor reapproaches"). A
// failed command (exit != 0) is DIAGNOSED, REFORMED into a corrected command (bounded, no-repeat), and
// retried — not blind-retried, not marched-past. The exit code is the failure signal; the loop control
// (`decide_reapproach`) is PURE + exhaustively unit-tested; reform itself is the one LLM step (the
// §2.14-distrusted free generation — kept bounded + gated + no-repeat-guarded).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandFailure { CommandNotFound, NoSuchPath, PermissionDenied, Other }

/// Parse the `[exit N]` marker the command channel prepends. None ⇒ no marker (treat as failure).
pub fn parse_exit_code(output: &str) -> Option<i32> {
    output.lines().next()?.trim().strip_prefix("[exit ")?.strip_suffix(']')?.parse().ok()
}

/// Deterministic diagnosis from the command channel's output (exit code + stderr text).
pub fn diagnose_command(output: &str) -> CommandFailure {
    let lo = output.to_lowercase();
    let code = parse_exit_code(output);
    if code == Some(127) || lo.contains("command not found") {
        CommandFailure::CommandNotFound
    } else if code == Some(126) || lo.contains("permission denied") || lo.contains("operation not permitted") {
        CommandFailure::PermissionDenied
    } else if lo.contains("no such file or directory") {
        CommandFailure::NoSuchPath
    } else {
        CommandFailure::Other
    }
}

/// Reform this failure, or escalate immediately? PermissionDenied is unfixable without sudo (which the
/// agent refuses) → escalate fast, don't burn reform budget on it.
pub fn should_reform(failure: CommandFailure) -> bool {
    !matches!(failure, CommandFailure::PermissionDenied)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReapproachAction { Advance, Retry(String), Escalate(String) }

/// PURE reapproach decision — the testable core. Given the just-run `output`, an already-produced reform
/// `candidate`, the set of commands already `tried`, and the budget `limit` → decide. Exit 0 → Advance;
/// a fresh non-empty non-GIVE_UP candidate within budget → Retry; otherwise (perms / budget exhausted /
/// no candidate / GIVE_UP / oscillation back to an already-tried command) → Escalate.
///
/// ⚠ RESIDUAL (documented, tested): this TRUSTS exit 0 — it CANNOT detect "exit 0 but wrong effect" (a
/// reform that runs cleanly without achieving the sub-goal, e.g. deletes nothing / touches the wrong
/// path). Catching that needs a per-sub-goal WORLD-STATE postcondition (the §11.4 effect-signature for
/// commands), not the exit code. Until then, exit-as-verification is the known limit of the command class.
pub fn decide_reapproach(
    output: &str,
    candidate: Option<&str>,
    tried: &std::collections::HashSet<String>,
    limit: usize,
) -> ReapproachAction {
    if parse_exit_code(output) == Some(0) {
        return ReapproachAction::Advance;
    }
    let failure = diagnose_command(output);
    if !should_reform(failure) {
        return ReapproachAction::Escalate(format!("{failure:?} — can't fix without elevated rights"));
    }
    if tried.len() > limit {
        return ReapproachAction::Escalate("reform budget exhausted".to_string());
    }
    match candidate.map(str::trim) {
        Some(c) if !c.is_empty() && !c.eq_ignore_ascii_case("GIVE_UP") && !tried.contains(c) => {
            ReapproachAction::Retry(c.to_string())
        }
        _ => ReapproachAction::Escalate("no further fix found".to_string()),
    }
}

/// A short, directive hint per failure kind — gives the weak model a concrete repair instruction
/// instead of a vague "fix it" (measured: bare reform produced worse commands).
fn reform_hint(failure: CommandFailure) -> &'static str {
    match failure {
        CommandFailure::CommandNotFound =>
            "The program name was NOT FOUND — it is misspelled or the wrong program. Output the SAME command with ONLY the program name corrected to the right standard tool (e.g. a one-letter typo of a common command). Change nothing else.",
        CommandFailure::NoSuchPath =>
            "A path does not exist — create the parent directory first, or correct the path. Do not invent placeholder paths.",
        CommandFailure::PermissionDenied =>
            "Avoid anything needing elevated rights — the agent cannot sudo.",
        CommandFailure::Other => "Correct the command so it succeeds.",
    }
}

/// Derive a deterministic WORLD-STATE postcondition for a command — a shell `test` that exits 0 iff the
/// command's stated file-effect actually HOLDS in the world. This closes the "exit 0 ≠ effect" gap for
// ── ReAct command loop (validated 6/6, react_loop_probe) ─────────────────────────────────────────────
// The user's architecture: reflexive DOING (one move, single-turn-fresh, resets each step) + a SEPARATE
// planning/verify attention. We DON'T trust the 8B to author a full plan upfront (discover_probe: it
// hallucinates paths/ops). Instead, per step: observe the real filesystem → reason ONE next command →
// run it (capture the OS error) → VERIFY against a DERIVED expected end-state (deterministic `test`, the
// judge + stop) → feed back {expected + error}. Three forcing functions took it 4/6→6/6: deterministic
// verify, absolute-path rule, and feeding the command's own error back.

/// OBSERVE: a read-only, GOAL-RELEVANT, RECURSIVE listing = the "current environment" the reflex step
/// reasons over. Roots = the user's standard folders + /tmp + ANY absolute path named in the goal;
/// `find -maxdepth 4` so nested files (OSWorld trees, /tmp fixtures) are VISIBLE; sorted (stable for the
/// no-progress compare) and capped (bounded prompt). Deterministic; read-only.
/// Common instruction words that are NOT directory names — so `discover_environment` doesn't probe
/// `$HOME/<every-word>`. A real staged dir name (photos, IncomeStatement2, …) survives this filter.
const DISCOVER_STOPWORDS: &[&str] = &[
    "the", "and", "any", "all", "for", "from", "into", "with", "that", "this", "these", "those", "your",
    "you", "can", "please", "help", "each", "found", "files", "file", "folder", "folders", "directory",
    "directories", "dir", "copy", "move", "create", "make", "delete", "remove", "save", "open", "rename",
    "new", "text", "name", "named", "value", "values", "cell", "cells", "row", "rows", "column", "columns",
    "sheet", "sheets", "go", "through", "recursively", "inside", "above", "below", "blank", "empty",
    "finish", "work", "even", "they", "are", "but", "not", "their", "them", "set", "add", "fill", "out",
];

pub fn discover_environment(actuator: &dyn Actuator, goal: &str) -> String {
    // FOCUSED roots: the three work dirs (nested files visible) + any ABSOLUTE path named in the goal.
    // NOT $HOME-root or /tmp — recursing those is a firehose (tine/, system temp) that drowns the signal
    // (measured: it HALVED the pass rate vs a focused listing). Files+dirs, depth 4, sorted, capped.
    let mut roots = vec!["$HOME/Desktop".to_string(),
        "$HOME/Documents".to_string(), "$HOME/Downloads".to_string()];
    for tok in goal.split_whitespace() {
        let t = tok.trim_matches(|c: char| !c.is_alphanumeric() && !"/_-.".contains(c));
        if t.starts_with('/') && t.len() > 1 { roots.push(t.to_string()); }
    }
    // GOAL-NAMED relative dirs: a task may stage files in a dir named in the goal (e.g. "the 'photos'
    // directory") that lives directly under $HOME, which the three work-dir roots miss → the capability
    // loop can't ground it → handback. Add `$HOME/<name>` for plausible name tokens; the find's `[ -e ]`
    // gates existence, so non-existent candidates cost nothing (no $HOME-firehose). Class-general: grounds
    // a goal-named dir wherever the task put it, without recursing all of $HOME.
    for tok in goal.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-') {
        let t = tok.trim();
        if t.len() >= 3 && t.chars().next().is_some_and(|c| c.is_alphabetic()) && !DISCOVER_STOPWORDS.contains(&t.to_lowercase().as_str()) {
            roots.push(format!("$HOME/{t}"));
        }
    }
    roots.sort(); roots.dedup();
    roots.truncate(24); // bound the candidate set (perf + the head -80 cap)
    let script = format!(
        "for r in {}; do [ -e \"$r\" ] && find \"$r\" -maxdepth 4 -not -path '*/.*' 2>/dev/null; done \
         | sort -u | head -80", roots.join(" "));
    // RETRY: an empty observe (transient SSH/actuator hiccup) collapses the grammar's path-binding → the
    // model runs unconstrained → garbage. Retry before giving up; the caller fail-closes on a true empty.
    for _ in 0..3 {
        let out = actuator.run_command(&script).lines().filter(|l| !l.starts_with("[exit")).collect::<Vec<_>>().join("\n");
        if !out.trim().is_empty() { return out; }
    }
    String::new()
}

/// Derive the guest's home dir from the observed environment listing. The observe `find` roots are
/// `$HOME/{Desktop,Documents,Downloads}` (the guest shell expands `$HOME`), so the listing contains real
/// absolute guest paths — we read `/home/<user>` back off the first one. CLASS-GENERAL: works for ANY
/// guest user (laputa, OSWorld's `user`, a re-provisioned guest) instead of assuming one specific user.
/// Falls back to `$HOME` only when nothing is observed (callers fail-closed on empty env anyway).
pub fn guest_home(env: &str) -> String {
    for line in env.lines() {
        if let Some(rest) = line.trim().strip_prefix('/') {
            let mut segs = rest.split('/');
            if segs.next() == Some("home") {
                if let Some(user) = segs.next().filter(|u| !u.is_empty()) {
                    return format!("/home/{user}");
                }
            }
        }
    }
    "$HOME".to_string()
}

/// ROUTING gate (user-chosen posture 2026-06-20): declared file-ops verbs run AUTONOMOUSLY through the
/// typed-capability loop; ANYTHING undeclared (git/gzip/chmod/compile…) or any explicit NON-home target
/// falls through to the human-GATED raw-command sequencer (the pre-ffd9ce9 path). This is the safety story
/// for regulated buyers: a constrained autonomous surface + a gated escape, NOT a blanket shell hatch.
/// The capability layer was built+validated on HOME-dir file management; osworld's /tmp + dev tasks belong
/// on the raw path. Word-level match (not substring — "transcript" must not trip "script").
pub fn capability_expressible(goal: &str) -> bool {
    let lo = goal.to_lowercase();
    // RECURSIVE / GLOB multi-source op → NOT a single-src→dest typed verb. The typed `copy`/`move`
    // verbs model ONE source → ONE dest; a recursive walk or an extension/glob pattern ("any .jpg
    // files", "all *.txt") selects MANY sources, which they can't express → the capability loop would
    // re-author it as a single-file op and fail. Route these to the planned-command path (the planner
    // already emits `find … -exec cp`, which the main-loop command channel runs directly). Class signal
    // (recursion/multiplicity), NOT a per-task keyword: triggers on `recursiv*` or a `*` glob or a
    // `.<ext> file(s)` pattern — deliberately NOT on "all/any/each" alone (too common; they're also in
    // DISCOVER_STOPWORDS) so a single-file capability task ("copy the report to Documents") is untouched.
    let ext_pattern = regex::Regex::new(r"\.[a-z0-9]{1,4}\b").unwrap().is_match(&lo)
        && (lo.contains("file") || lo.contains('*'));
    if lo.contains("recursiv") || lo.contains('*') || ext_pattern { return false; }
    // verbs/objects the typed vocab does NOT declare → gated raw-command path
    const RAW: &[&str] = &["git","gzip","gunzip","tar","zip","unzip","chmod","chown","compress",
        "commit","repository","executable","compile","make","npm","pip","curl","wget","ssh","gpg","mount"];
    let toks: Vec<&str> = lo.split(|c: char| !c.is_alphanumeric()).filter(|s| !s.is_empty()).collect();
    if toks.iter().any(|t| RAW.contains(t)) { return false; }
    // an explicit absolute path OUTSIDE /home → gated raw-command path
    for tok in goal.split(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '`') {
        let t = tok.trim_matches(|c: char| !c.is_alphanumeric() && !"/_-.".contains(c));
        if t.starts_with('/') && t.len() > 1 && !t.starts_with("/home/") { return false; }
    }
    true
}

/// GROUNDING anchors named directly in the goal: every absolute path token + ALL its ancestor dirs.
/// The observe listing only surfaces paths that ALREADY EXIST under the home work-dirs — it can never
/// surface a NOT-YET-CREATED target (`/tmp/osw_proj`) or a path outside those dirs (`/etc`, `/mnt/...`).
/// Without these anchors the capability grammar can't represent the goal's own path, so the model is
/// forced to the nearest observed home path (measured: `/tmp/osw_proj` → `mkdir /home/laputa/osw_proj`).
/// Ancestors included so a NEW child (`write_file /tmp/osw_proj/README.md`) binds to its parent + a seg.
/// Path-AGNOSTIC by design (a file-ops agent must honour a path the user names, home or not) — NOT a
/// shell hatch; the typed-verb + grammar + validate discipline is unchanged.
pub fn goal_path_anchors(goal: &str) -> Vec<String> {
    let mut out = Vec::new();
    for tok in goal.split(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '`') {
        let t = tok.trim_matches(|c: char| !c.is_alphanumeric() && !"/_-.".contains(c));
        if !t.starts_with('/') || t.len() < 2 || t.contains("..") { continue; }
        // the path itself + every ancestor down to (not including) "/"
        let mut p = t.trim_end_matches('/').to_string();
        while p.len() > 1 {
            if !out.contains(&p) { out.push(p.clone()); }
            match p.rfind('/') { Some(0) | None => break, Some(i) => p.truncate(i) }
        }
    }
    out
}

/// PLAN (separate attention): derive the EXPECTED end-state as a list of absolute paths that must exist
/// when the goal is done — grounded in the current environment. A NARROW step (LIST paths, never author
/// shell — the weak model can't write a check, react_loop_probe v2). The harness wraps each in `test -e`
/// → deterministic, side-effect-free. Returns (checks, human-hint). Empty checks ⇒ caller fail-closed.
pub fn derive_expected(goal: &str, env: &str, adapter: &Arc<dyn InferenceAdapter>) -> (Vec<String>, String) {
    let home = guest_home(env);
    let home_slash = format!("{home}/");
    let prompt = format!(
"List the absolute path of EVERY file or folder that must EXIST when this goal is fully done. One path
per line, starting with {home}/. Use the exact filenames from 'Current files'. No narration, no commands.

Goal: {goal}
Current files:
{env}
Expected paths:");
    let text = adapter.generate(&prompt, 96, 0.1).unwrap_or_default();
    let paths: Vec<String> = text.lines().map(str::trim)
        .filter(|l| l.starts_with(&home_slash))
        .map(|l| l.split_whitespace().next().unwrap_or(l).trim_end_matches([',', '.', ';']).to_string())
        .filter(|p| p.len() > home_slash.len())
        .collect();
    let hint = if paths.is_empty() { goal.to_string() } else { paths.join(", ") };
    let checks = paths.into_iter().map(|p| format!("test -e {p}")).collect();
    (checks, hint)
}

/// REASON (reflex): single-turn-fresh — {goal, expected, current env, history+errors} → ONE next shell
/// command. Resets every step (no memory of prior reasoning). The expected target + the last command's
/// ERROR are the forcing functions. Rejects hang-prone commands (sudo/interactive). None ⇒ stop.
pub fn react_next_command(goal: &str, expected: &str, env: &str, hist: &str, adapter: &Arc<dyn InferenceAdapter>) -> Option<String> {
    let home = guest_home(env);
    let prompt = format!(
"You are doing ONE step of a file task on Linux (home is {home}). Output ONLY the single next
shell command that moves toward the EXPECTED RESULT. No narration.
- use mv to move/rename, cp to copy (KEEP THE SAME FILENAME), mkdir -p for a folder, rm to delete
- ALWAYS write the FULL absolute path ({home}/...) for BOTH source and destination — copy the
  exact source path from 'Current files', never a bare filename. Never invent files/contents. One command.

Goal: {goal}
EXPECTED RESULT (not yet satisfied — make it true): {expected}
Current files:
{env}
Steps already done:
{hist}
Next single command:");
    let text = adapter.generate(&prompt, 96, 0.1).ok()?;
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    let cmd = line.strip_prefix("run the command ").unwrap_or(line).trim().trim_start_matches('$').trim();
    if cmd.is_empty() || command_would_hang(&format!("run the command {cmd}")) { return None; }
    Some(cmd.to_string())
}

// ── CAPABILITY layer (App-Intents equivalent; validated ~2× free-form, capability_probe) ─────────────
// The model SELECTS a typed verb via the Pythonic GBNF (`grammar::capability_grammar`, source paths bound
// to observe) instead of authoring free-form shell. parse → `capability_to_command` builds ONE
// deterministic shell command (resolve happens inside it via `find`), which runs through the SAME
// gate+execute path as the free-form channel — so HITL/destructive gating is inherited unchanged.

/// The capability menu prompt (the model emits ONE Pythonic call; the grammar enforces well-formedness).
pub fn capability_prompt(goal: &str, env: &str, hist: &str) -> String {
    let home = guest_home(env);
    format!(
"You operate a computer by choosing ONE typed action (a Pythonic call) that moves toward the goal.
ACTIONS — emit EXACTLY ONE call. Use ONLY absolute paths under {home} (from 'Current files').
- make_folder(path=\"{home}/Documents/NewFolder\")
- write_file(path=\"{home}/Documents/file.txt\", content=\"TEXT\")
- move(source_dir=\"{home}/Downloads\", selector=\"*.pdf\", dest=\"{home}/Documents\")
- copy(source_dir=\"{home}/Downloads\", selector=\"*.jpg\", dest=\"{home}/Documents\", recursive=true)
- rename(path=\"{home}/Downloads/old.txt\", new_name=\"new.txt\")
- delete(source_dir=\"{home}/Downloads\", selector=\"*.jpg\", filter=\"empty\")
- extract_to_file(mode=\"value\", source=\"{home}/Documents/report.txt\", pattern=\"[0-9]+\", dest_file=\"{home}/Documents/out.txt\")
- extract_to_file(mode=\"count\", source_dir=\"{home}/Downloads\", selector=\"*.log\", dest_file=\"{home}/Documents/count.txt\")
RULES: source_dir and dest are FOLDERS — NEVER a file or a glob (the glob goes in selector). A NEW folder =
an EXISTING folder path + /Name. Anything ALREADY in 'Current files' is DONE — do the NEXT needed action,
NEVER repeat a completed one. Use a GLOB selector (e.g. *.pdf) to affect ALL matches. home is {home}.

Goal: {goal}
Current files:
{env}
Actions already done (do the NEXT step, do not repeat these):
{hist}
Next single action:")
}

/// Parse a Pythonic call `[move(source_dir="/x", selector="*.pdf", dest="/y")]` → (verb, kwargs).
pub fn parse_capability_call(line: &str) -> Option<(String, std::collections::HashMap<String, String>)> {
    let s = line.trim().trim_start_matches('[').trim_end_matches(']').trim();
    let open = s.find('(')?; let close = s.rfind(')')?; if close < open { return None; }
    let verb = s[..open].trim().trim_end_matches(':').to_lowercase();
    let (mut parts, mut cur, mut q) = (Vec::new(), String::new(), false);
    for c in s[open+1..close].chars() { match c {
        '"' => { q = !q; cur.push(c); }
        ',' if !q => parts.push(std::mem::take(&mut cur)),
        c => cur.push(c) } }
    if !cur.trim().is_empty() { parts.push(cur); }
    let mut m = std::collections::HashMap::new();
    for part in parts { if let Some((k, v)) = part.split_once('=') {
        let v = v.trim().trim_matches('"').trim_matches(|c| c == '<' || c == '>');
        m.insert(k.trim().to_lowercase(), v.to_string()); } }
    Some((verb, m))
}

/// Build ONE deterministic shell command for a capability call. Resolve (the `find`) happens inside the
/// command, so this is pure. Handles file-OR-folder sources and glob OR single-filename selectors. The
/// model NEVER writes shell or a check — it only filled typed slots. None ⇒ malformed/incomplete call.
pub fn capability_to_command(verb: &str, p: &std::collections::HashMap<String, String>) -> Option<String> {
    let g = |k: &str| p.get(k).cloned().unwrap_or_default();
    let q = |s: &str| format!("\"{}\"", s.replace('"', ""));
    // Tolerate the model putting a glob/file IN source_dir (a common slip the break-point map exposed):
    // split "/dir/*.ext" → (source_dir="/dir", selector="*.ext"). source_dir must be a FOLDER.
    let dirsel = |sd: String, sel: String| -> (String, String) {
        if sd.contains('*') || sd.contains('?') || sd.contains('[') {
            if let Some((d, pat)) = sd.rsplit_once('/') {
                return (d.to_string(), if sel.is_empty() { pat.to_string() } else { sel });
            }
        }
        (sd, sel)
    };
    match verb {
        "make_folder" => { let path = g("path"); if path.is_empty() { return None; } Some(format!("mkdir -p {}", q(&path))) }
        "write_file" => { let path = g("path"); if path.is_empty() { return None; }
            Some(format!("mkdir -p \"$(dirname {p})\"; printf '%s' {c} > {p}", p = q(&path), c = q(&g("content")))) }
        "move" | "copy" => {
            let (sd, sel) = dirsel(g("source_dir"), g("selector")); let dest = g("dest");
            if sd.is_empty() || dest.is_empty() { return None; }
            let op = if verb == "move" { "mv" } else { "cp" };
            let cf = if verb == "copy" && g("recursive") == "true" { "-r" } else { "" };
            let nn = g("new_name");
            if !nn.is_empty() && !sel.contains('*') {   // single-file rename-on-move/copy
                let src = if sel.is_empty() { sd.clone() } else { format!("{sd}/{sel}") };
                return Some(format!("mkdir -p {d} && {op} {cf} {} {}", q(&src), q(&format!("{dest}/{nn}")), d = q(&dest)));
            }
            let depth = if g("recursive") == "true" { "" } else { "-maxdepth 1" };
            if sel.is_empty() {
                Some(format!("mkdir -p {d} && {op} {cf} {s} {d}/", s = q(&sd), d = q(&dest)))
            } else {   // source_dir may be a FILE or a folder+selector
                Some(format!("mkdir -p {d} && if [ -f {s} ]; then {op} {cf} {s} {d}/; else find {s} {depth} -name {sel} -type f -exec {op} {cf} -t {d}/ {{}} +; fi",
                    s = q(&sd), d = q(&dest), sel = q(&sel)))
            }
        }
        "rename" => { let (path, nn) = (g("path"), g("new_name")); if path.is_empty() || nn.is_empty() { return None; }
            Some(format!("mv {p} \"$(dirname {p})\"/{n}", p = q(&path), n = q(&nn))) }
        "delete" => { let (sd, sel) = dirsel(g("source_dir"), g("selector")); if sd.is_empty() || sel.is_empty() { return None; }
            let filt = match g("filter").as_str() { "empty" => "-empty", "larger_than_1k" => "-size +1k", _ => "" };
            Some(format!("find {s} -maxdepth 1 -name {sel} -type f {filt} -exec rm -f {{}} +", s = q(&sd), sel = q(&sel))) }
        "extract_to_file" => {
            let mode = g("mode");
            let dest = if g("dest_file").is_empty() { g("dest") } else { g("dest_file") };
            if dest.is_empty() { return None; }
            let inner = match mode.as_str() {
                "value" => { let (src, pat) = (g("source"), g("pattern")); if src.is_empty() || pat.is_empty() { return None; } format!("grep -oE {} {} | head -1", q(&pat), q(&src)) }
                "count" => { let (sd, sel) = (g("source_dir"), g("selector")); if sd.is_empty() { return None; } format!("find {} -maxdepth 1 -name {} -type f | wc -l", q(&sd), q(&sel)) }
                "list"  => { let (sd, sel) = (g("source_dir"), g("selector")); if sd.is_empty() { return None; } format!("find {} -maxdepth 1 -name {} -type f -printf '%f\\n'", q(&sd), q(&sel)) }
                _ => return None,
            };
            Some(format!("mkdir -p \"$(dirname {d})\"; printf '%s\\n' \"$({inner})\" > {d}", d = q(&dest)))
        }
        _ => None,
    }
}

/// FAIL-SAFE validator (the keystone for multi-step reliability): a capability call must have a KNOWN
/// verb and every path slot must be GROUNDED (absolute, under a user home, no `..` traversal). This
/// REJECTS the garbage that intermittent grammar non-enforcement produces — `/abs/…` placeholders echoed
/// from the prompt, unknown verbs like `create_folder` — BEFORE it ever executes. The harness must NEVER
/// propagate a model/infra flake into an action; if a call can't be validated, re-emit or hand back, never
/// run it. Multi-step break-point map showed THIS (not cross-step coherence) is the dominant break.
pub fn validate_capability_call(verb: &str, params: &std::collections::HashMap<String, String>, grounded: &[String]) -> Result<(), String> {
    const VERBS: &[&str] = &["make_folder", "write_file", "move", "copy", "rename", "delete", "extract_to_file"];
    if !VERBS.contains(&verb) { return Err(format!("unknown verb '{verb}'")); }
    const PATH_KEYS: &[&str] = &["path", "source_dir", "source", "dest", "dest_file"];
    // GROUNDED (not home-hardcoded): absolute, no traversal, AND the value sits WITHIN or CONTAINS a path
    // we actually grounded (observe listing ∪ goal anchors). This rejects the hallucinated `/abs/...`
    // placeholder (intermittent grammar drop) while accepting any real target the user named — `/tmp`,
    // `/etc`, `/mnt/...` — not just `/home`. The grammar already constrains emission; this is the backstop.
    let in_grounded = |v: &str| grounded.iter().any(|g|
        v == g || v.starts_with(&format!("{g}/")) || g.starts_with(&format!("{v}/")));
    for k in PATH_KEYS {
        if let Some(v) = params.get(*k) {
            if v.is_empty() || !v.starts_with('/') || v.contains("..") || !in_grounded(v) {
                return Err(format!("ungrounded path {k}={v:?}"));
            }
        }
    }
    Ok(())
}

/// DETERMINISTIC completion check for the ReAct loop (NOT model-derived — the weak model can't author a
/// check, react_loop_probe v2 + the production false-success). Extracts the goal's NAMED target artifact:
/// a `x.ext` filename after a target preposition (to/into/called/named) → `test -e <dir>/<file>`; a
/// "folder called/named X" → that dir must be a NON-EMPTY dir (catches the empty-folder false success).
/// Resolves a folder word (Documents/Downloads/Desktop), default ~. Conservative: no derivable named
/// target ⇒ empty ⇒ caller hands back honestly (never a false claim). The judge; the model only acts.
pub fn goal_completion_checks(goal: &str) -> Vec<String> {
    let lo = goal.to_lowercase();
    let dir = if lo.contains("documents") { "~/Documents" }
              else if lo.contains("downloads") { "~/Downloads" }
              else if lo.contains("desktop") { "~/Desktop" } else { "~" };
    let mut checks = Vec::new();
    // ABSOLUTE targets named in the goal (`/tmp/osw_proj`, `/etc`, `/mnt/...`) — the home-dir resolver
    // below can't see them. A delete/remove goal wants the path GONE; everything else wants it to EXIST.
    // Leaf tokens only (ancestors like `/tmp` would always pass and mask a real miss). Without this the
    // agent acts on the right /tmp path but, finding no check, hands back even on success.
    let deleting = ["delete", "remove", "trash", "rm "].iter().any(|k| lo.contains(k));
    // CONSERVATIVE-OR-SILENT: a goal that demands the target be RUNNABLE needs `test -x`, NOT `test -e`.
    // `test -e` (mere existence) lets "create X and mark it runnable" claim success when the chmod step
    // was dropped — a FALSE SUCCESS (the held-out runnable-mark miss). A stronger check can only turn a
    // false claim into an honest handback, never the reverse. Scope to the SCRIPT path so an unrelated
    // named file isn't wrongly required to be executable.
    let wants_exec = ["runnable", "executable", "chmod +x", "make it executable", "mark it exec"]
        .iter().any(|k| lo.contains(k));
    let is_scriptish = |p: &str| [".sh", ".py", ".pl", ".rb", ".bash"].iter().any(|e| p.ends_with(e));
    for tok in goal.split(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '`') {
        let t = tok.trim_matches(|c: char| !c.is_alphanumeric() && !"/_-.".contains(c));
        if t.starts_with('/') && t.len() > 1 && !t.contains("..") {
            let op = if deleting { "! -e" }
                     else if wants_exec && is_scriptish(t) { "-x" } // executable ⇒ existence too
                     else { "-e" };
            checks.push(format!("test {op} {t}"));
        }
    }
    let words: Vec<&str> = goal.split_whitespace().collect();
    for (i, w) in words.iter().enumerate() {
        let prev = if i > 0 { words[i-1].to_lowercase() } else { String::new() };
        let tok = w.trim_matches(|c: char| !c.is_alphanumeric() && c != '.' && c != '_' && c != '-');
        let is_file = tok.contains('.') && tok.rsplit('.').next()
            .map_or(false, |e| (1..=5).contains(&e.len()) && e.chars().all(|c| c.is_ascii_alphanumeric()));
        if is_file && matches!(prev.as_str(), "to" | "into" | "called" | "named") {
            checks.push(format!("test -e {dir}/{tok}"));
        }
    }
    for kw in ["folder called ", "folder named ", "called ", "named "] {
        if let Some(p) = lo.find(kw) {
            let name = goal[p + kw.len()..].split_whitespace().next().unwrap_or("")
                .trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '-');
            if !name.is_empty() && name.chars().next().map_or(false, |c| c.is_uppercase()) {
                checks.push(format!("test -d {dir}/{name} && [ -n \"$(ls -A {dir}/{name} 2>/dev/null)\" ]"));
                break;
            }
        }
    }
    checks
}

/// file operations: a command (or reform) that exits cleanly without creating/removing the file is
/// caught. Returns None for commands whose effect isn't a checkable file-state (queries/compute — exit
/// code is the only signal we have there). DETERMINISTIC parse of the command — NOT the model asserting
/// its own completion (§11.4). Scope: existence-level for direct file ops + redirects; not content/semantics.
pub fn command_postcondition(cmd: &str) -> Option<String> {
    let toks: Vec<&str> = cmd.split_whitespace().collect();
    // A redirect `> file` / `>> file` must leave `file` existing.
    if let Some(pos) = toks.iter().position(|t| *t == ">" || *t == ">>") {
        if let Some(f) = toks.get(pos + 1) {
            return Some(format!("test -e {f}"));
        }
    }
    let bin = *toks.first()?;
    // Non-flag arguments — the operands (paths).
    let args: Vec<&str> = toks.iter().skip(1).copied()
        .take_while(|a| *a != "&&" && *a != "||" && *a != ";" && *a != "|")
        .filter(|a| !a.starts_with('-'))
        .collect();
    let last = args.last()?;
    match bin {
        "touch"          => Some(format!("test -e {last}")),
        "mkdir"          => Some(format!("test -d {last}")),
        "rmdir" | "rm"   => Some(format!("test ! -e {last}")),
        "cp" | "mv" | "install" | "ln" => Some(format!("test -e {last}")),
        _ => None,
    }
}

/// Path-like tokens in a string (contain `/`), stripped of surrounding quotes/parens and trailing
/// punctuation. PURE.
fn extract_paths(text: &str) -> Vec<String> {
    text.split(|c: char| c.is_whitespace() || c == ',' || c == ';')
        .map(|t| t.trim_matches(|c: char| matches!(c, '"' | '\'' | '(' | ')')).trim_end_matches([':', '.']))
        .filter(|t| t.contains('/') && t.len() > 1 && !t.starts_with('-'))
        .map(str::to_string)
        .collect()
}

/// GOAL-DERIVED world-state checks — parsed from the user's stated GOAL (effect verb + paths), verified
/// at plan completion. Catches what per-step exit codes can't: plan INCOMPLETENESS (a needed artifact
/// never got a step) or a step that achieved its OWN effect but not the goal's. DETERMINISTIC parse of
/// the user's words — NOT the model judging itself (§11.4). Conservative: only create/delete intents
/// with explicit paths; ambiguous source→target verbs (move/copy) are skipped to avoid false negatives.
pub fn goal_postconditions(goal: &str) -> Vec<String> {
    let lo = goal.to_lowercase();
    if ["move ", "rename", "copy ", " mv ", " cp ", " to /", "from /"].iter().any(|v| lo.contains(v)) {
        return Vec::new(); // a path moves between two locations — which one "counts" is ambiguous
    }
    // Git goals: a mere directory passes `test -e`, so the weak check FALSE-CLAIMS when `git init`
    // failed (it made the dir but not the repo). The REAL artifact is `.git`, and a committed repo's
    // is a reachable HEAD. Strong-check or it lies.
    if lo.contains("git repo") || lo.contains("git repository") {
        return extract_paths(goal).into_iter().map(|p| {
            let p = p.trim_end_matches('/').to_string();
            if lo.contains("commit") { format!("git -C {p} rev-parse HEAD") } // a commit must exist
            else { format!("test -d {p}/.git") }                              // a real repo, not a dir
        }).collect();
    }
    // GUI app-launch goals ("open the file manager") have no file artifact and no DESKTOP-AGNOSTIC shell
    // check (a role like "file manager" maps to a different binary on every desktop — thunar/nautilus/nemo
    // — so an English→binary table names ONE environment, not the class, and silently rots when the guest
    // changes). Launch completion is confirmed by the perception/effect layer instead (`effect_confirmed`
    // + `observe_until_quiet`: a new top-level window appeared), which is desktop-agnostic. So this PURE
    // goal-parse returns no check for pure-launch goals → fail-closed (never false-claims).
    let wants_absent = ["delete", "remove", "get rid of", "erase"].iter().any(|v| lo.contains(v));
    let wants_create = ["create", "make ", "write", "touch", "save", "generate", "new file",
                        "new folder", "new director"].iter().any(|v| lo.contains(v));
    if !wants_absent && !wants_create {
        return Vec::new(); // not a create/delete intent → no checkable goal artifact
    }
    let wants_dir = lo.contains("director") || lo.contains("folder");
    // EXECUTABLE intent → the artifact must be EXECUTABLE, not merely present. `test -e` FALSE-CLAIMS when
    // the file was created but never chmod'd (the held-out runnable-mark miss — "mark it runnable" is a
    // synonym the old list "executable"/"chmod" missed). Broadened synonyms; SCOPED to script files so an
    // unrelated output path in the same goal (e.g. /tmp/status) isn't wrongly required to be executable.
    let wants_exec = ["executable", "runnable", "chmod +x", "chmod 7", "make it run", "mark it run",
                      "execute it", "run it"].iter().any(|k| lo.contains(k));
    let is_scriptish = |p: &str| [".sh", ".py", ".pl", ".rb", ".bash"].iter().any(|e| p.ends_with(e));
    extract_paths(goal).into_iter().map(|p| {
        if wants_absent { format!("test ! -e {p}") }
        else if wants_dir { format!("test -d {p}") }
        else if wants_exec && is_scriptish(&p) { format!("test -x {p}") }
        else { format!("test -e {p}") }
    }).collect()
}

/// Build the reform prompt (pure — tested for content). Asks the model for ONE corrected command for the
/// SAME sub-goal, with a diagnosis-specific repair hint, or `GIVE_UP`.
pub fn reform_prompt(subgoal: &str, failed_cmd: &str, output: &str) -> String {
    let hint = reform_hint(diagnose_command(output));
    let err = output.lines().filter(|l| !l.starts_with("[exit")).collect::<Vec<_>>().join(" ");
    format!(
"A shell command failed. Output ONLY the single corrected command line.
Sub-goal: {subgoal}
Failed command: {failed_cmd}
Error: {err}
Fix: {hint}
If it genuinely cannot be fixed, output exactly: GIVE_UP
Corrected command:")
}

/// The one LLM step in reapproach: ask the model for a corrected command. Returns None on GIVE_UP /
/// empty / unchanged. Bounded + gated + no-repeat-guarded by `decide_reapproach` at the call site.
fn reform_command(subgoal: &str, failed_cmd: &str, output: &str, adapter: &Arc<dyn InferenceAdapter>) -> Option<String> {
    let text = adapter.generate(&reform_prompt(subgoal, failed_cmd, output), 96, 0.1).ok()?;
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    let cmd = line.trim_start_matches('$').trim();
    if cmd.is_empty() || cmd.eq_ignore_ascii_case("GIVE_UP") || cmd == failed_cmd || command_would_hang(&format!("run the command {cmd}")) {
        return None;
    }
    if !reform_is_conservative(failed_cmd, cmd) {
        chronos::log(&format!("reform_rejected (not a conservative correction): {cmd}"));
        return None;
    }
    Some(cmd.to_string())
}

/// A reform must be a CORRECTION of the failed command, not a re-plan: it may not introduce shell
/// chaining / redirection / substitution / a new program-launch that the original lacked. This bounds
/// the blast radius of a bad reform from the weak model (measured: the 8B "fixes" `cat x` into
/// `mkdir && cat x` and emits non-command meta-text) — the worst a guarded reform can do is swap the
/// program and fail again → clean escalate, never run a side-effecting chain. Pure.
pub fn reform_is_conservative(original: &str, candidate: &str) -> bool {
    const OPS: &[&str] = &["&&", "||", ";", "|", "exec ", ">", "<", "`", "$(", "\n"];
    !OPS.iter().any(|op| candidate.contains(op) && !original.contains(op))
}

// ── Deterministic-first reform for CommandNotFound (the spine floor under the weak LLM reform) ────────
// Curated EQUIVALENCE classes: interchangeable programs / known platform renames. If the requested
// member isn't installed, the first INSTALLED member of its class is the fix — reliable, unlike the
// 1B-active model that can't repair even a typo. This is deliberately NOT typo-correction (ambiguous:
// `tuch` is one edit from touch/much/such) — only intent-equivalent programs.
const EQUIVALENCE_CLASSES: &[&[&str]] = &[
    &["python", "python3", "python2"],
    &["pip", "pip3"],
    &["node", "nodejs"],
    &["fd", "fdfind"],
    &["bat", "batcat"],
    &["md5", "md5sum"],
    &["shasum", "sha256sum", "sha1sum"],
    &["open", "xdg-open"],
    &["gnome-terminal", "xfce4-terminal", "konsole", "xterm", "alacritty", "kitty"],
    &["firefox", "firefox-esr", "chromium", "chromium-browser", "google-chrome", "google-chrome-stable"],
    &["nvim", "vim", "vi"],
];

/// Equivalence-class alternatives for a program name (class order, excluding the name itself). PURE.
pub fn equivalence_alternatives(bin: &str) -> Vec<&'static str> {
    EQUIVALENCE_CLASSES.iter()
        .find(|c| c.contains(&bin))
        .map(|c| c.iter().copied().filter(|&m| m != bin).collect())
        .unwrap_or_default()
}

/// Deterministic-first reform for CommandNotFound: if the failed program is in an equivalence class,
/// substitute the first alternative ACTUALLY INSTALLED on the target (verified via `command -v`).
/// Returns None when there's no class / no installed alternative → the caller falls to the LLM reform.
/// The result is a pure program SWAP (same args) → inherently conservative (skips the LLM guard).
/// Reform a `cp [-flags] DIR/<glob> DEST` that matched NOTHING (NoSuchPath — the files are NESTED, so a
/// top-level glob can't see them) into the correct RECURSIVE form `find DIR -name '<glob>' -exec cp {} DEST/ \;`.
/// Class-general for "copy all files matching a pattern": the (unreliable) planner names the dir, pattern,
/// and dest correctly but often picks the wrong FORM (a flat glob) — this makes the form deterministic.
/// `-name '*.jpg'` also keeps the extension filter exact (excludes a `.png` decoy). Pure; unit-tested.
fn recursive_copy_reform(failed_cmd: &str) -> Option<String> {
    let toks: Vec<&str> = failed_cmd.split_whitespace().collect();
    if toks.first() != Some(&"cp") { return None; }
    let operands: Vec<&str> = toks[1..].iter().copied().filter(|t| !t.starts_with('-')).collect();
    if operands.len() != 2 { return None; }                 // exactly source + dest
    let (src, dest) = (operands[0], operands[1]);
    if !src.contains('*') { return None; }                  // only a glob source qualifies
    let slash = src.rfind('/')?;
    let (dir, pattern) = (&src[..slash], &src[slash + 1..]);
    if dir.is_empty() || !pattern.contains('*') { return None; }
    let dest = dest.trim_end_matches('/');
    Some(format!("find {dir} -name '{pattern}' -exec cp {{}} {dest}/ \\;"))
}

fn deterministic_reform(failed_cmd: &str, failure: CommandFailure, actuator: &dyn Actuator) -> Option<String> {
    // A glob `cp` that matched nothing → the files are nested → the recursive find-copy form (deterministic).
    if failure == CommandFailure::NoSuchPath {
        if let Some(r) = recursive_copy_reform(failed_cmd) {
            chronos::log(&format!("deterministic_reform: glob-cp matched nothing → recursive find ({r})"));
            return Some(r);
        }
    }
    if failure != CommandFailure::CommandNotFound {
        return None;
    }
    let mut it = failed_cmd.trim().splitn(2, char::is_whitespace);
    let bin = it.next()?;
    let rest = it.next().unwrap_or("").trim();
    for alt in equivalence_alternatives(bin) {
        // Is `alt` actually installed on the target? `command -v` exits 0 iff found.
        if parse_exit_code(&actuator.run_command(&format!("command -v {alt}"))) == Some(0) {
            chronos::log(&format!("deterministic_reform: {bin} → {alt} (installed)"));
            return Some(if rest.is_empty() { alt.to_string() } else { format!("{alt} {rest}") });
        }
    }
    None
}

/// The focused-window label from a perception dump line `[focused: X]`, or "" if absent.
fn screen_focus(screen: &str) -> String {
    for line in screen.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("[focused:") {
            return rest.trim_end_matches(']').trim().to_string();
        }
    }
    String::new()
}

/// Did the prior action produce a STRUCTURAL effect — a change in the set of element labels, or a
/// change of focused window? Deterministic and ELEMENT-level (not raw pixels), so it does NOT
/// advance the sequencer on ambient pixel noise or a transient tooltip that isn't an accessibility
/// element (§2.15 failure shapes 1+3: effect-without-accomplishment / wrong-change-coincidence).
/// The sequencer's advance signal. Empty prev → false (no prior step to judge). Pure.
pub fn structural_change(prev_screen: &str, screen: &str) -> bool {
    if prev_screen.is_empty() {
        return false;
    }
    use std::collections::BTreeSet;
    let labels = |s: &str| -> BTreeSet<String> {
        crate::perception::parse_ref_labels(s)
            .into_values()
            .filter(|l| !l.is_empty())
            .collect()
    };
    labels(prev_screen) != labels(screen) || screen_focus(prev_screen) != screen_focus(screen)
}

/// Is the world still CHANGING (an action still playing out) vs settled? The in-progress-vs-stuck
/// discrimination — the crux of observe-until-quiet. Active iff the a11y element-set/focus changed OR
/// pixels are painting above the ambient-noise floor (a cursor blink / clock tick is a cell or two).
/// A slow window paint or page load keeps `frame_changed_cells` high → we keep waiting (it's
/// progress, never a reason to quit). A genuinely hung screen is a11y-stable AND pixel-quiet → falls
/// through to settled, and the downstream no-effect path (effect_confirmed → should_cutoff) escalates.
/// `noise_cells` is the tunable floor. Pure — unit-tested.
fn settling_active(a11y_changed: bool, frame_changed_cells: usize, noise_cells: usize) -> bool {
    a11y_changed || frame_changed_cells > noise_cells
}

/// Decode the latest FRAME_PATH image and count grid cells whose pixels changed since the last call
/// (the in-progress signal when a11y is momentarily quiet — a window painting). 0 on any frame error
/// (then a11y carries the signal alone). Reuses the live DeltaDetector (same grid as the CV proposer).
fn frame_changed_cells(delta: &mut crate::perception::delta::DeltaDetector) -> usize {
    match std::fs::read(crate::config::FRAME_PATH)
        .ok()
        .and_then(|png| image::load_from_memory(&png).ok())
    {
        Some(img) => {
            let rgb = img.to_rgb8();
            let (w, h) = (rgb.width(), rgb.height());
            delta.detect_changes(rgb.as_raw(), w, h).len()
        }
        None => 0,
    }
}

/// OBSERVE-UNTIL-QUIET: read the world until the prior action's effect has MANIFESTED (changed from
/// the pre-action `baseline`) and the world has gone QUIET (N consecutive settled observations) —
/// terminating on an OBSERVED signal, never on a clock.
///
/// This replaces the fixed ~3s settle ceiling, which was the session's last "decide-without-observing"
/// bug: a terminal cold-start (or any slow action — a laggy app, a slow web call) that took longer
/// than the guess made the timer expire on a thing that was actually working → premature return →
/// spurious re-action / false stall. Here a slow action keeps the world ACTIVE (a11y churn or pixels
/// painting, via `settling_active`) and we keep waiting; we return only once it's stable. The
/// MANIFEST phase stops a premature return during pre-effect latency (the pre-action world is also
/// quiet). The fixed duration survives ONLY as a far-outer safety BACKSTOP (perpetual animation / a
/// genuine no-op) — not the primary control; a no-effect return is caught downstream by the
/// postcondition → should_cutoff escalation.
// ── Background-thread perception (Theme 1: never block the async runtime) ────────────────────────
// The Perceptor calls are SYNC and blocking (read_screen = ssh+python spawn; capture_frame = QMP;
// frame delta = fs read + PNG decode). Run them on the blocking pool via spawn_blocking so a
// perception round-trip never starves the tokio workers driving the UI / live feed / server_guard.
// The trait is Send+Sync, so the Arc moves into the closure cleanly.

/// Read the a11y screen on a blocking thread.
async fn read_screen_bg(perceptor: &Arc<dyn Perceptor>) -> String {
    let p = perceptor.clone();
    tokio::task::spawn_blocking(move || p.read_screen()).await.unwrap_or_default()
}

/// Capture a frame (QMP screendump) on a blocking thread. Used by the (currently-gated-off) Phase-2
/// CV sampled-collection hook; kept so flipping the CV toggle back on is a one-line, two-way door.
async fn capture_frame_bg(perceptor: &Arc<dyn Perceptor>) {
    let p = perceptor.clone();
    let _ = tokio::task::spawn_blocking(move || p.capture_frame()).await;
}

/// One settle-poll tick on a blocking thread: capture a fresh frame + count changed cells. The
/// stateful DeltaDetector is moved in and returned out (it accumulates cell hashes across ticks).
async fn poll_frame_bg(
    perceptor: &Arc<dyn Perceptor>,
    mut delta: crate::perception::delta::DeltaDetector,
) -> (crate::perception::delta::DeltaDetector, usize) {
    let p = perceptor.clone();
    tokio::task::spawn_blocking(move || {
        p.capture_frame();
        let cells = frame_changed_cells(&mut delta);
        (delta, cells)
    })
    .await
    .unwrap_or_else(|_| (crate::perception::delta::DeltaDetector::new(), 0))
}

async fn observe_until_quiet(perceptor: &Arc<dyn Perceptor>, baseline: &str) -> String {
    const INTERVAL_MS: u64 = 120;
    const QUIET_READS: usize = 3;       // consecutive settled a11y checks → the world is quiet
    const NOISE_CELLS: usize = 2;       // frame-delta floor: ambient cursor/clock, not activity (TUNABLE)
    const BACKSTOP_POLLS: usize = 300;  // far-outer safety ONLY — not the termination control
    let mut delta = crate::perception::delta::DeltaDetector::new();
    // Prime the frame baseline (first call flags all cells) — on the blocking pool.
    let (d, _) = poll_frame_bg(perceptor, delta).await;
    delta = d;
    // COST: a11y read is an ssh+python spawn; the frame-delta is the cheaper QMP path. So poll the
    // FRAME each interval and read a11y ONLY when the frame is quiet (the candidate settle points) —
    // while pixels are painting we already know the world is active and skip the expensive read.
    let mut last_a11y: Option<String> = None;
    let mut quiet = 0usize;
    let mut manifested = baseline.is_empty(); // no baseline → skip the manifest phase
    for _ in 0..BACKSTOP_POLLS {
        tokio::time::sleep(tokio::time::Duration::from_millis(INTERVAL_MS)).await;
        let (d, cells) = poll_frame_bg(perceptor, delta).await;
        delta = d;
        if settling_active(false, cells, NOISE_CELLS) {
            quiet = 0; // pixels painting → in progress → don't pay for an a11y read this tick
            continue;
        }
        // Frame quiet → read a11y to confirm the world settled, check manifest, and capture focus.
        let curr = read_screen_bg(perceptor).await;
        let a11y_changed = last_a11y.as_deref().is_some_and(|p| structural_change(p, &curr));
        if !manifested && (structural_change(baseline, &curr) || a11y_changed) {
            manifested = true; // the action's effect has appeared
        }
        if a11y_changed {
            quiet = 0; // a11y still settling (e.g. focus just moved to the new window) → keep waiting
        } else if manifested {
            quiet += 1;
            if quiet >= QUIET_READS {
                chronos::log(&format!("settled: quiet after effect (focus={})", screen_focus(&curr)));
                return curr;
            }
        }
        last_a11y = Some(curr);
    }
    chronos::log("settle_backstop: world never quieted — proceeding; downstream escalation decides");
    match last_a11y {
        Some(s) => s,
        None => read_screen_bg(perceptor).await,
    }
}

/// Structural effect class for a Click sub-goal's POSTCONDITION (§2.15). Derived DETERMINISTICALLY
/// from the sub-goal text (never the model asserting its own completion). Each class maps to ONE
/// structural signature that CONFIRMS the click's effect; the sequencer advances only on confirmation,
/// so a click whose effect didn't occur — or occurred in the WRONG DIRECTION — holds the pointer
/// instead of marching a dead plan. Direction-awareness falls OUT of the per-class signature (`Open`
/// confirms only when elements APPEAR, so toggling an already-open menu shut does not read as
/// "opened"); it is NOT a hardcoded open-vs-close rule bolted onto `structural_change` (the trap).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EffectClass {
    /// Reveal new on-screen elements (menu / dropdown / submenu / panel): the label set must GROW.
    Open,
    /// Any other control activation (button / tab / list item / app launch): any structural change.
    /// The weak catch-all — preserves the prior advance behavior for everything that isn't a reveal,
    /// so generic clicks don't over-escalate. Every Click sub-goal maps to one of these two (no
    /// unmapped class reaches the advance gate, so there is no guess-advance path).
    Activate,
}

/// Classify a Click sub-goal's effect class from its text. HIGH-PRECISION for `Open` (a verb that
/// reveals + a container target); everything else falls to `Activate`. By construction this can only
/// make a *reveal* stricter, never a generic click — keeping misclassification low-blast-radius.
fn effect_class(subgoal: &str) -> EffectClass {
    let s = subgoal.to_lowercase();
    let reveal_verb = s.contains("open") || s.contains("expand") || s.contains("show") || s.contains("reveal");
    let reveal_target = s.contains("menu") || s.contains("dropdown") || s.contains("drop-down")
        || s.contains("submenu") || s.contains("sub-menu") || s.contains("panel") || s.contains("context menu");
    if reveal_verb && reveal_target { EffectClass::Open } else { EffectClass::Activate }
}

/// Did the click's expected structural effect occur? The sequencer's advance gate, replacing the
/// direction-blind bare `structural_change`. Reuses the live a11y label-set / focus primitives.
fn effect_confirmed(class: EffectClass, prev: &str, curr: &str) -> bool {
    match class {
        // CONFIRMS only when NEW elements appeared net-positive (the revealed menu/panel items). A
        // net REMOVAL — the menu CLOSED because it was already open and the toggle shut it — does NOT
        // confirm "open". This is the fix for the already-open toggle advancing on a regression.
        EffectClass::Open => {
            use std::collections::BTreeSet;
            let labels = |s: &str| -> BTreeSet<String> {
                crate::perception::parse_ref_labels(s).into_values().filter(|l| !l.is_empty()).collect()
            };
            let (before, after) = (labels(prev), labels(curr));
            after.difference(&before).count() > before.difference(&after).count()
                && after.len() > before.len()
        }
        // Any structural change (label-set or focus) — the prior advance signal, kept for everything
        // that isn't a reveal (button / tab / item / app-launch all legitimately confirm on a change).
        EffectClass::Activate => structural_change(prev, curr),
    }
}

/// Map a step's observable result to a supervisor outcome. Pure — unit-testable.
/// Observed at the TOP of the next loop iteration, where the prior action's on-screen
/// effect is finally readable (the effect of an action at step N isn't visible until the
/// fresh `read_screen()` at step N+1). `action_executed == false` covers blocked/denied
/// actions and recovery-injection turns. Done/Task never reach this point (they break the
/// loop), so a terminal `Done` outcome is intentionally not produced here.
pub fn classify_step_outcome(action_executed: bool, screen_changed: bool) -> crate::supervisor::StepOutcome {
    use crate::supervisor::StepOutcome;
    if !action_executed { StepOutcome::Failed }
    else if screen_changed { StepOutcome::Progressed }
    else { StepOutcome::NoChange }
}

/// API plane: find the target office document on the guest + read its structure (headers + sample rows) so
/// the model authors native ops against REAL columns. Best-effort (openpyxl if present, else a soffice
/// csv-peek); returns (file_path, structure_text), or None if no document found.
fn api_read_target(actuator: &dyn Actuator, _goal: &str) -> Option<(String, String)> {
    let find = "find \"$HOME\" \"$HOME/Desktop\" \"$HOME/Documents\" -maxdepth 3 \
                \\( -name '*.xlsx' -o -name '*.ods' -o -name '*.csv' \\) 2>/dev/null | head -1";
    let path = actuator.run_command(find).lines()
        .find(|l| !l.starts_with("[exit") && l.trim().starts_with('/'))
        .map(|s| s.trim().to_string())?;
    if path.is_empty() { return None; }
    // headers + 2 sample rows; openpyxl if present, else a csv-convert peek (LibreOffice is on the guest)
    let dump = format!(
        "python3 - <<'PY' 2>/dev/null || true\n\
         try:\n import openpyxl; wb=openpyxl.load_workbook('{p}'); ws=wb.worksheets[0]\n\
         from openpyxl.utils import get_column_letter\n\
         print('sheets:', wb.sheetnames); print('active sheet:', ws.title)\n\
         h=next(ws.iter_rows(min_row=1,max_row=1,values_only=True),())\n\
         print('columns:', ', '.join('%s=%r'%(get_column_letter(i+1),v) for i,v in enumerate(h)))\n\
         print('rows 2..%d sample:'%ws.max_row, list(ws.iter_rows(min_row=2,max_row=3,values_only=True)))\n\
         except Exception: pass\nPY", p = path);
    let structure = actuator.run_command(&dump).lines()
        .filter(|l| !l.starts_with("[exit") && !l.starts_with("[stderr"))
        .collect::<Vec<_>>().join("\n");
    Some((path, structure))
}

/// The app's native-op MENU (the silver platter) — shared by the whole-plan prompt and the per-step
/// prompt so the vocabulary never drifts between them.
const API_OPS_MENU: &str =
"You operate a spreadsheet by issuing operations; the application computes formulas for you. Available ops:
  set_cell(sheet=\"S\", cell=\"A1\", formula=\"=...\")   set a cell to a formula
  set_cell(sheet=\"S\", cell=\"A1\", value=\"...\")       set a cell to a literal value
  fill(sheet=\"S\", range=\"B1:E30\", direction=\"down\")  fill blank cells in the range with the value before them along the axis
  fill_down / fill_up / fill_left / fill_right          shorthand for fill(direction=...) — same op
  set_formula_range(sheet=\"S\", range=\"I2:I30\", formula=\"=B2-C2\")  apply a formula down/across a whole range; the app adjusts relative refs per cell (=B2→=B3). Use this for a computed COLUMN — write the formula once for the first row.
  add_sheet(name=\"S\", index=0)                        add a sheet (index 0 = first)
  rename_sheet(old=\"S\", new=\"S2\")                     rename a sheet
Use formulas for any computation (e.g. =B2-C2-D2-SUM(F2:H2), cross-sheet =Sheet1!J2).
Prefer ONE fill/set_formula_range over many set_cell when filling a range — do NOT enumerate cells you can fill.
Use the actual sheet name from the Workbook structure.";

/// API plane prompt — the SILVER PLATTER: present the app's native ops (a decoding menu), DO NOT steer to an
/// answer. The model selects ops + authors the formula itself (its comprehension job).
fn api_plane_prompt(goal: &str, structure: &str) -> String {
    format!("{API_OPS_MENU}\n\nGoal: {goal}\nWorkbook:\n{structure}\n\nEmit the operations that accomplish the goal, as a list of calls:")
}

/// PER-STEP authoring prompt (sequencer-routed): ask for the NEXT SINGLE op given the goal, the workbook,
/// and the ops authored so far. Short output (one op or "done") → tiny generation → low temp-0 variance,
/// and the loop can't drop a step. The model sees its own running plan so it builds it up incrementally
/// instead of emitting the whole thing in one fragile 1400-token pass.
fn api_step_prompt(goal: &str, structure: &str, authored: &[String]) -> String {
    let so_far = if authored.is_empty() { "(none yet)".to_string() } else { authored.join("\n  ") };
    format!("{API_OPS_MENU}\n\nGoal: {goal}\nWorkbook:\n{structure}\n\nOps you have authored so far (all will be applied):\n  {so_far}\n\nAuthor the NEXT single operation toward the goal. If the ops above already fully accomplish the goal, reply exactly: done\nNext op:")
}

/// Scan a model's output for `verb(k=\"v\", k=N, ...)` op-calls, quote/paren-aware so a `)` inside a formula
/// (SUM(F2:H2)) doesn't end the call early. Returns (verb, kwargs) for each — typed by `api_plane::from_call`.
fn scan_op_calls(text: &str) -> Vec<(String, std::collections::HashMap<String, String>)> {
    let verbs = ["set_cell", "set_formula_range", "fill_down", "fill_up", "fill_left", "fill_right", "fill", "add_sheet", "rename_sheet"];
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    for verb in verbs {
        let mut from = 0;
        while let Some(rel) = text[from..].find(&format!("{verb}(")) {
            let open = from + rel + verb.len() + 1;
            // find the matching close paren, respecting quotes
            let (mut i, mut depth, mut in_q, mut esc) = (open, 1i32, false, false);
            while i < bytes.len() && depth > 0 {
                let c = bytes[i] as char;
                if esc { esc = false; }
                else if c == '\\' { esc = true; }
                else if c == '"' { in_q = !in_q; }
                else if !in_q && c == '(' { depth += 1; }
                else if !in_q && c == ')' { depth -= 1; }
                i += 1;
            }
            let body = &text[open..i.saturating_sub(1)];
            let mut kw = std::collections::HashMap::new();
            for cap in regex::Regex::new(r#"(\w+)\s*=\s*"((?:[^"\\]|\\.)*)"|(\w+)\s*=\s*(\d+)"#).unwrap().captures_iter(body) {
                if let (Some(k), Some(v)) = (cap.get(1), cap.get(2)) {
                    kw.insert(k.as_str().to_string(), v.as_str().replace("\\\"", "\""));
                } else if let (Some(k), Some(v)) = (cap.get(3), cap.get(4)) {
                    kw.insert(k.as_str().to_string(), v.as_str().to_string());
                }
            }
            out.push((verb.to_string(), kw));
            from = i;
        }
    }
    out
}

// ── Agent loop ────────────────────────────────────────────────────
pub async fn agent_loop(
    state: Arc<Mutex<AgentState>>,
    adapter: Arc<dyn InferenceAdapter>,
    perceptor: Arc<dyn Perceptor>,
    actuator: Arc<dyn Actuator>,
    mut approval_rx: mpsc::Receiver<bool>,
    confirm_tx: mpsc::Sender<String>,
    memory_tiers: Arc<tokio::sync::Mutex<crate::memory_tiers::MemoryTiers>>,
    visual_encoder: Option<Arc<crate::vision::VisualEncoder>>,
    registry: Arc<tools::ToolRegistry>,
    skill_library: Arc<SkillLibrary>,
) {
    let mut enforcer = StepEnforcer::new();
    let mut memory = Memory::new(|steps| {
        let actions: Vec<_> = steps.iter()
            .filter_map(|s| s.action.as_ref())
            .map(|a| format!("{:?}", a))
            .collect();
        actions.join(", ")
    });

    // Screen hash for action-graph state key (read once per goal, used for recovery lookup)
    let _t_osw = std::env::var("OSW_TRACE").is_ok();
    let _t0 = std::time::Instant::now();
    let state_hash = {
        let s = read_screen_bg(&perceptor).await;
        format!("{}", blake3::hash(s.as_bytes()))
    };
    if _t_osw { eprintln!("[timing] read_screen_bg(start-hash) {:.1}s", _t0.elapsed().as_secs_f32()); }

    // Recovery manager — graph-backed + LLM-assisted failure recovery
    let recovery_manager: Option<RecoveryManager> = {
        let graph_path = crate::config::data_dir().join("action_graph.db");
        ActionGraph::open(&graph_path.to_string_lossy()).ok().map(|g| {
            RecoveryManager::new(
                g,
                None,
                std::sync::Arc::new(TokioMutex::new(None)),
                "http://127.0.0.1:8080/v1/chat/completions".to_string(),
            )
        })
    };

    // Supervisor — the OUTER escalation bound. It observes every step's outcome (keeping
    // its stall/loop/retry state live) but the loop acts ONLY on its terminal directives:
    // Escalate→Human (clean HITL handoff) and Abort (ladder exhausted). Continue / Done /
    // ResetFromBoard / Escalate→Model defer to the existing inner machinery (recovery_manager,
    // should_cutoff, structural-failure detection). The ladder is governor-built from
    // hardware + mode + user settings — the supervisor stays model-agnostic.
    let mut supervisor = crate::supervisor::Supervisor::new(crate::governor::escalation_ladder());
    // Whether a step has been attempted yet — gates the top-of-loop observe (no prior step
    // to judge on the first iteration).
    let mut had_prior_step = false;

    // Sliding window of recent action descriptions for loop/deadlock detection
    let mut recent_actions: Vec<String> = Vec::new();

    // Screen-diff observation state — tracks previous screen text and whether
    // an action actually executed last turn (denied/blocked actions don't count).
    let mut prev_screen = String::new();
    let mut prev_action_executed = false;
    // A deterministic Command/Type/Key step that ADVANCED the sequencer is PROGRESS even though it makes
    // no SCREEN change (a command changes the filesystem, not the GUI). Without this the screen-change-
    // based supervisor counts a successful command chain as a stall and false-escalates mid-sequence.
    let mut prev_step_progressed = false;

    // Consecutive-identical-action cutoff state.
    // Invariant: last_exec_action / consecutive_exec_count are updated only when
    // action_executed == true; reset when a different action executes.
    let mut last_exec_action = String::new();
    let mut consecutive_exec_count: usize = 0;

    let goal = state.lock().await.goal.clone();
    let system_prompt = config::system_prompt();
    // Reset the persistent command shell at the start of each goal — cwd/env from a prior goal must
    // not leak into this one (the actuator is app-lifetime). Within THIS goal, the session persists,
    // so multi-step chains (git init → add → commit) share state. This is the cross-step-state fix.
    actuator.reset_command_session();

    // DETERMINISTIC SEQUENCER (§2.14–2.15): split the goal into ordered sub-goals up front (the
    // model can't decompose — it spuriously completes; harness owns ordering). The executor runs
    // ONE sub-goal at a time; `current_sub` is the deterministic progress pointer (trajectory
    // state, NOT retrieved memory — the safe category). The pointer advances when a sub-goal's
    // action takes effect (below); exhausting the plan is deterministic completion, not the
    // model's fallthrough `complete`. v1: one primary action per sub-goal (multi-action sub-goals
    // and semantically-compound goals fall to the executor + supervisor handback).
    // Board-informed planning (upstream of the memory-isolated executor). For an implicit goal this
    // asks the brain to make preconditions explicit, shaped by learned skills; an explicit "X then Y"
    // stays on the deterministic split. Runs on the blocking pool (sync HTTP generate).
    let _t_plan = std::time::Instant::now();
    let sub_goals: Vec<SubGoal> = {
        let g = goal.clone();
        let ad = adapter.clone();
        let sl = skill_library.clone();
        let act = actuator.clone();
        let strings = tokio::task::spawn_blocking(move || {
            let skills = sl.retrieve(&g, 3);
            // GROUND the planner in the real filesystem (the discovered tree) so it emits absolute paths
            // for goal-named dirs (e.g. 'photos' → /home/user/Desktop/photos) instead of bare relatives
            // that resolve against the wrong CWD. Sync (run_command); already on the blocking pool here.
            let env = discover_environment(act.as_ref(), &g);
            plan_goal(&g, &env, &skills, &ad)
        })
        .await
        .unwrap_or_else(|_| decompose_goal(&goal));
        // Classify each planned step into Click / Type / Key (Wall 2). Click steps run the selection
        // loop; Type/Key are deterministic.
        strings.iter().map(|s| classify_subgoal(s)).collect()
    };
    if _t_osw { eprintln!("[timing] plan_goal+discover_env {:.1}s → {} sub-goals", _t_plan.elapsed().as_secs_f32(), sub_goals.len()); }
    let mut current_sub: usize = 0;
    // DEVIATION DETECTION (§2.15): consecutive re-perceptions where NOTHING on screen matches the
    // current sub-goal. The deterministic plan is BLIND and cannot re-plan, so when the world goes
    // off-plan (error dialog, permission prompt, an already-done/ambiguous state) the safe move is
    // a clean handback, not looping or marching a dead plan. Reset on any progress; escalate at the
    // threshold. This also SAFELY subsumes "already-satisfied" — rather than risk a wrong auto-skip,
    // we hand back and let the human say "that's done, skip it."
    let mut subgoal_stuck: usize = 0;
    const SUBGOAL_STUCK_LIMIT: usize = 4;
    // Perception sense level, bumped by the supervisor on a Sense-tier escalation (a11y → richer
    // sense). 0 = a11y-only (the floor). ≥1 turns on the CV pass below — the seam where Phase-2
    // captioning plugs in. Stays 0 in production until the governor's ladder has a Sense rung.
    let mut sense_level: u8 = 0;
    // True for one iteration after a fail-closed re-perceive (a11y had no target → looked again, did
    // NOT act). Such an iteration is NOT a failed/stalled STEP, so it must be kept OUT of the
    // supervisor's Failed/oscillation counters — otherwise the stall path (loop_threshold=2) escalates
    // to Human at ~2 re-perceptions and pre-empts PerceptionBlind (which fires at SUBGOAL_STUCK_LIMIT=4),
    // making the a11y-blindness escalation unreachable. Lets subgoal_stuck climb to the limit.
    let mut blind_reperceive = false;
    if sub_goals.len() > 1 {
        chronos::log(&format!("sequencer: {} sub-goals: {:?}", sub_goals.len(), sub_goals));
    }

    // PLAN-LEVEL APPROVAL (Option 2 — the for-now floor; will evolve into the tiered earned-autonomy
    // model). If the plan has any step that would otherwise confirm, preview the WHOLE plan and get ONE
    // approval up front (vs a per-step tap-fest that defeats "faster than typing"). Approved → write
    // steps auto-run; destructive steps STILL hard-stop individually. Declined → run nothing.
    let plan_approved = if plan_requires_approval(&sub_goals) {
        let preview = render_plan_preview(&sub_goals);
        chronos::log(&format!("plan_preview:\n{preview}"));
        let _ = confirm_tx.send(envelope::make("action_log",
            envelope::ActionLogPayload { text: preview.clone() })).await;
        if !request_plan_approval(&preview, &state, &mut approval_rx, &confirm_tx).await {
            chronos::log("plan_denied");
            let _ = confirm_tx.send(envelope::make("status", envelope::StatusPayload {
                state: "goal_done".to_string(),
                detail: "Plan declined — nothing was run.".to_string(),
            })).await;
            return;
        }
        true
    } else {
        false
    };

    // Priors slice — the Board. Park-scored top-k (relevance × recency × importance) from
    // the ColBERT embedder when it's up AND the board has embedded rows; deterministic
    // recency floor (`assemble_context`) otherwise. The spine: a model-upgrade layer over a
    // floor that always works. embed() is blocking HTTP → run off the lock via spawn_blocking
    // NOTE: the upstream-planner priors (episodic Board slice, visual-similarity context, skill
    // context) were COMPUTED-AND-DISCARDED here every goal — a blocking embedder HTTP call, a VLM FFI
    // encode + frame read, and a skill DB retrieve, each taking a MemoryTiers lock, all on the
    // latency-to-first-action path, feeding nothing (the executor is memory-isolated, inv #10; nothing
    // consumed them). Removed (optimization audit v1, Theme 3). When the v2 upstream planner lands it
    // will compute these on its OWN path and consume them — re-add there, not here. The Board/skill
    // infra remains live (assemble_slice / find_similar_by_embedding / skill_library.retrieve).
    chronos::log(&format!("goal_received: {goal}"));
    let _ = confirm_tx.send(envelope::make("status", envelope::StatusPayload {
        state: "goal_received".to_string(),
        detail: goal.clone(),
    })).await;
    if _t_osw { eprintln!("[timing] reached goal_received (pre-loop done) at {:.1}s total", _t0.elapsed().as_secs_f32()); }

    // APP-AWARE PLANE CLASSIFICATION (computed ONCE, up front — perception is bounded/cheap now). A
    // spreadsheet-focused content goal is InAppSemantic and belongs to the API plane (UNO set_cell),
    // NOT the file-ops capability loop. plan_goal can hallucinate a bogus Command for such a goal (e.g.
    // `gsettings set …auto-fill`), which — being all-Command — would otherwise be CAPTURED by the
    // capability loop below before the API plane (further down) ever sees it. Classify by focused-app
    // IDENTITY here so the routing is correct regardless of what the planner authored.
    let _t_focus = std::time::Instant::now();
    let api_findings = {
        let focus = screen_focus(&read_screen_bg(&perceptor).await);
        crate::plane::Findings { focused_app: (!focus.is_empty()).then_some(focus), ..Default::default() }
    };
    let api_in_app = matches!(crate::plane::classify_task(&goal, &api_findings), crate::plane::TaskKind::InAppSemantic);
    if _t_osw { eprintln!("[timing] api focus read_screen_bg {:.1}s → in_app={} (total {:.1}s)", _t_focus.elapsed().as_secs_f32(), api_in_app, _t0.elapsed().as_secs_f32()); }

    // ── ReAct COMMAND LOOP ────────────────────────────────────────────────────────────────────────
    // When plan_goal chose an ALL-COMMAND surface (a CLI/file goal), DON'T walk the untrusted upfront
    // plan. Run the validated reflex+verify loop: observe → reason ONE command toward a DERIVED expected
    // → run (gated, capture the OS error) → VERIFY against the real world (deterministic, the judge+stop)
    // → feed back {expected + error}. Completion is the real-world check, never the model's say-so.
    if !sub_goals.is_empty() && sub_goals.iter().all(|sg| matches!(sg.action, SubAction::Command(_)))
        && capability_expressible(&goal) && !api_in_app {
        chronos::log("react_capability_loop: engaging (home file-ops goal)");
        // CAPABILITY layer: each step the model SELECTS a typed verb (Pythonic GBNF); the harness builds
        // the deterministic command. DETERMINISTIC goal completion check (the judge); empty ⇒ no derivable
        // named target ⇒ honest handback.
        let checks = goal_completion_checks(&goal);
        chronos::log(&format!("react_capability_loop checks={checks:?}"));
        const REACT_MAX_STEPS: usize = 8;
        let verify_now = |act: &dyn Actuator| -> bool {
            !checks.is_empty() && checks.iter().all(|c| parse_exit_code(&act.run_command(c)) == Some(0))
        };
        let mut hist = String::from("(none yet)");
        let mut completed = false;
        let mut prev_env = String::new();
        let mut stale = 0u8;
        for _step in 1..=REACT_MAX_STEPS {
            if verify_now(actuator.as_ref()) { completed = true; break; }
            let env = discover_environment(actuator.as_ref(), &goal);
            // No-progress guard: world unchanged across 2 consecutive steps ⇒ stop (honest handback).
            if env == prev_env { stale += 1; if stale >= 2 { break; } } else { stale = 0; prev_env = env.clone(); }
            // CAPABILITY SELECT (FAIL-SAFE): GBNF Pythonic, source/dest bound to observe → parse →
            // VALIDATE (known verb + grounded paths) → build cmd. REJECT the garbage that intermittent
            // grammar non-enforcement produces (`/abs/` placeholders, invalid verbs); re-emit up to 3×;
            // NEVER run an unvalidated call. Empty observe ⇒ no grounding ⇒ fail-closed (not unconstrained).
            let mut paths: Vec<String> = env.lines().map(|l| l.trim().to_string()).filter(|p| p.starts_with('/')).collect();
            // Anchor the goal's OWN named paths (+ ancestors) so the grammar can represent a not-yet-created
            // or non-home target — else `/tmp/osw_proj` is silently rewritten to the nearest observed path.
            for a in goal_path_anchors(&goal) { if !paths.contains(&a) { paths.push(a); } }
            if paths.is_empty() {
                chronos::log("capability: observe empty (no grounding) → handback");
                verify_or_handback(&goal, actuator.as_ref(), &confirm_tx,
                    "I couldn't read the workspace state — handing back.").await;
                return;
            }
            let mut cmd: Option<String> = None;
            for _try in 0..3 {
                let prompt = capability_prompt(&goal, &env, &hist);
                let grammar = crate::grammar::capability_grammar(&paths);
                let ad = adapter.clone();
                let parsed = tokio::task::spawn_blocking(move || ad.generate_constrained(&prompt, 128, 0.1, &grammar).ok().map(|(t, _)| t))
                    .await.ok().flatten()
                    .and_then(|raw| raw.lines().map(str::trim).find(|l| !l.is_empty()).map(|s| s.to_string()))
                    .and_then(|line| parse_capability_call(&line));
                if let Some((verb, params)) = parsed {
                    match validate_capability_call(&verb, &params, &paths) {
                        Ok(()) => { cmd = capability_to_command(&verb, &params); if cmd.is_some() { break; } }
                        Err(reason) => chronos::log(&format!("capability_rejected (re-emit): {reason}")),
                    }
                }
            }
            let Some(cmd) = cmd else {
                chronos::log("capability: no valid grounded call after 3 tries → handback");
                verify_or_handback(&goal, actuator.as_ref(), &confirm_tx,
                    "I couldn't form a valid grounded action — handing back.").await;
                return;
            };
            let mut args = serde_json::Map::new();
            args.insert("command".to_string(), serde_json::Value::String(cmd.clone()));
            let tool_call = ToolCall::Invoke { name: "vm_command".to_string(), args };
            let output = match gate::apply_plan_approval(gate::confidence_escalate(gate::evaluate_action(&tool_call, &registry), 1.0), plan_approved) {
                gate::Verdict::Allow => {
                    let out = execute_tool(&tool_call, &actuator, perceptor.as_ref(), &memory_tiers).await;
                    chronos::log(&format!("react_action: $ {cmd} -> {out}"));
                    let _ = confirm_tx.send(envelope::make("action_log", envelope::ActionLogPayload { text: format!("$ {cmd}\n{out}") })).await;
                    out
                }
                gate::Verdict::ConfirmTap => request_and_await_approval("tap", &tool_call, &state, &actuator, perceptor.as_ref(), &memory_tiers, &mut approval_rx, &confirm_tx).await,
                gate::Verdict::ConfirmTyped => request_and_await_approval("typed", &tool_call, &state, &actuator, perceptor.as_ref(), &memory_tiers, &mut approval_rx, &confirm_tx).await,
                gate::Verdict::Block(reason) => { chronos::log(&format!("react_blocked: {reason}")); format!("Blocked: {reason}") }
            };
            memory.push(Step { index: enforcer.step(), prompt: String::new(), output: output.clone(), action: None });
            // FEED BACK: the command's own ERROR (forcing function) on failure, OR — the OSCILLATION RAIL
            // (multistep map: 2/7→4/7) — a successful-but-WORLD-UNCHANGED action is a no-op/repeat, so tell
            // the model it had no effect and to ADVANCE (the static "don't repeat" prompt rule didn't take).
            let exit_ok = parse_exit_code(&output) == Some(0);
            let msg: String = output.lines().filter(|l| !l.starts_with("[exit")).collect::<Vec<_>>().join(" ");
            let no_effect = exit_ok && discover_environment(actuator.as_ref(), &goal) == env;
            let note = if !exit_ok {
                           if msg.trim().is_empty() { "  → (command failed)".to_string() }
                           else { format!("  → ERROR: {}", msg.trim().chars().take(160).collect::<String>()) }
                       } else if no_effect {
                           "  ← NO EFFECT (already done / matched nothing) — do a DIFFERENT next step".to_string()
                       } else { String::new() };
            hist = if hist == "(none yet)" { format!("- {cmd}{note}") } else { format!("{hist}\n- {cmd}{note}") };
        }
        if completed {
            chronos::log("react_command_loop: expected end-state verified → complete");
            complete_goal(&goal, actuator.as_ref(), &confirm_tx).await;
        } else {
            verify_or_handback(&goal, actuator.as_ref(), &confirm_tx,
                "I worked through the steps but couldn't verify the goal was achieved — handing back.").await;
        }
        return;
    }

    // ── API PLANE (in-app-semantic): the model authors the app's NATIVE ops (formulas etc., the silver
    // platter) and the harness applies them THROUGH the app, instead of GUI-fumbling. Routed by the
    // plane-governor's task classification; fail-closes FAST (no 200s GUI churn). The model still authors
    // the formula (its comprehension job); the harness owns the tool + the apply. ──────────────────────
    // APP-AWARE classification computed up front (`api_in_app`, above) by focused-app IDENTITY — a
    // spreadsheet routes here even when the planner authored a bogus Command (which would otherwise be
    // captured by the capability loop). `screen_focus` parsed the `[focused: X]` line once.
    if api_in_app {
        // The API plane is feasible ONLY when there's an API-addressable document. If there isn't, this is
        // an in-app-semantic goal on an app the API plane can't address yet (richest-first: API → … → GUI)
        // → FALL THROUGH to the GUI loop below, do NOT hand back (that would regress the GUI path).
        if let Some((file, structure)) = api_read_target(actuator.as_ref(), &goal) {
            chronos::log("api_plane: spreadsheet found → authoring native ops via the app's tools");
            if _t_osw { eprintln!("[timing] api_read_target done (total {:.1}s)", _t0.elapsed().as_secs_f32()); }
            let checks = goal_completion_checks(&goal);
            let verify_now = |act: &dyn Actuator| -> bool {
                !checks.is_empty() && checks.iter().all(|c| parse_exit_code(&act.run_command(c)) == Some(0))
            };
            // INCREMENTAL per-op authoring (sequencer-routed, the §2.14 primitive applied to the API
            // plane): author ONE op per SHORT generation against the goal + structure + ops-so-far, until
            // the model signals done / authors no new op / hits the budget. The list ACCUMULATES and is
            // applied ONCE below via the proven build_guest_apply (guards the 01b269ae single-op path).
            // Short gens = the temp-0 variance win; the loop = completeness (no dropped op). Termination is
            // no-progress + budget (model-"done" is only a weak hint — §2.14 disproved trusting it). Per-op
            // LIVE effect-verify (self-correction) is the deferred persistent-UNO-session ("app adaptation")
            // increment; here apply/reconcile stays batched-once.
            const API_MAX_OPS: usize = 12;
            let _t_auth = std::time::Instant::now();
            let mut ops: Vec<serde_json::Value> = Vec::new();
            let mut authored: Vec<String> = Vec::new();
            for _step in 0..API_MAX_OPS {
                let step_prompt = api_step_prompt(&goal, &structure, &authored);
                let ad = adapter.clone();
                let raw = tokio::task::spawn_blocking(move || ad.generate(&step_prompt, 96, 0.0).ok())
                    .await.ok().flatten().unwrap_or_default();
                // first valid op-call this step (one bite)
                let next = scan_op_calls(&raw).into_iter()
                    .find_map(|(verb, kw)| crate::api_plane::from_call(&verb, &kw)
                        .and_then(|op| crate::api_plane::op_to_json(&op)));
                match next {
                    // no new op authored → the model is done or stuck → stop (no-progress)
                    None => break,
                    Some(op) => {
                        let op_s = op.to_string();
                        // duplicate of the immediately-prior op → no-progress → stop
                        if authored.last() == Some(&op_s) { break; }
                        authored.push(op_s);
                        ops.push(op);
                    }
                }
            }
            if _t_osw { eprintln!("[timing] api incremental authoring {:.1}s → {} ops (total {:.1}s)", _t_auth.elapsed().as_secs_f32(), ops.len(), _t0.elapsed().as_secs_f32()); }
            if !ops.is_empty() {
                let ops_json = serde_json::Value::Array(ops).to_string();
                let cmd = crate::api_plane::build_guest_apply(&file, &ops_json);
                let tool_call = ToolCall::Invoke {
                    name: "vm_command".to_string(),
                    args: { let mut m = serde_json::Map::new();
                            m.insert("command".to_string(), serde_json::Value::String(cmd.clone())); m },
                };
                if let gate::Verdict::Allow = gate::apply_plan_approval(
                    gate::confidence_escalate(gate::evaluate_action(&tool_call, &registry), 1.0), plan_approved) {
                    let _t_apply = std::time::Instant::now();
                    let out = execute_tool(&tool_call, &actuator, perceptor.as_ref(), &memory_tiers).await;
                    chronos::log(&format!("api_plane: applied {} ops via the app → {out}", ops_json.len()));
                    if _t_osw { eprintln!("[timing] build_guest_apply(reconcile) {:.1}s (total {:.1}s)\n[apply-ops] {}\n[apply-out] {}", _t_apply.elapsed().as_secs_f32(), _t0.elapsed().as_secs_f32(), ops_json, out.replace('\n', " | ")); }
                    let _ = confirm_tx.send(envelope::make("action_log",
                        envelope::ActionLogPayload { text: "applied native app operations".to_string() })).await;
                }
            } else {
                chronos::log("api_plane: model authored no valid ops");
            }
            // honest completion check — never the model's say-so; fast handback (no GUI churn)
            if verify_now(actuator.as_ref()) {
                complete_goal(&goal, actuator.as_ref(), &confirm_tx).await;
            } else {
                verify_or_handback(&goal, actuator.as_ref(), &confirm_tx,
                    "I operated the app's tools but couldn't verify the goal — handing back.").await;
            }
            return;
        }
        chronos::log("api_plane: in-app-semantic but no API-addressable document → falling through to GUI plane");
        // The GUI plane now owns this in-document task → let perception SEE in-document targets (cells,
        // in-text controls) it pruned while the API plane was the candidate. Still bounded by the
        // perception deadline. (Plane-conditional pruning — `lagado-perception-latency-bug`.)
        perceptor.set_document_pruning(false);
    }

    loop {
        {
            let s = state.lock().await;
            if !s.running { break; }
        } // guard dropped here — safe to await below

        if let Err(e) = enforcer.advance() {
            tracing::warn!("Agent terminated: {:?}", e);
            break;
        }

        // Settle the world before reading it for the advance check + selection: only after an action
        // executed (a transition may still be painting). A no-action re-perceive needs no settle.
        // Reuses the live `structural_change` primitive as the stability test (not the dead
        // verifier.rs SHA-256 path). This is the fix for the term-type transition race.
        let screen = if prev_action_executed {
            // baseline = the screen the prior action was applied to → observe until its effect
            // manifests and the world goes quiet (no fixed ceiling).
            observe_until_quiet(&perceptor, &prev_screen).await
        } else {
            read_screen_bg(&perceptor).await
        };

        // Compute and log screen-effect observation from previous turn.
        let observation = observation_for(prev_action_executed, &prev_screen, &screen);
        if let Some(ref obs) = observation {
            chronos::log(&format!("observation_injected: {}", &obs[..obs.len().min(120)]));
        }
        let _observation_section = match &observation {
            Some(obs) => format!("{obs}\n\n"),
            None => String::new(),
        };

        // Supervisor observes the PRIOR step now that its on-screen effect is readable.
        // (An action's effect at step N isn't visible until this fresh `screen` at N+1.)
        // Acts only on terminal directives; everything else defers to the inner machinery.
        // A fail-closed re-perceive (a11y found no target → looked again, didn't act) is NOT a step
        // outcome — skip it so it can't feed the supervisor a phantom Failed/oscillation and pre-empt
        // the PerceptionBlind escalation (see `blind_reperceive`).
        if had_prior_step && !blind_reperceive {
            let screen_changed = !prev_screen.is_empty()
                && blake3::hash(screen.as_bytes()) != blake3::hash(prev_screen.as_bytes());
            // A deterministic command/Type/Key advance is PROGRESS (verified by its own exit code /
            // postcondition), even with no screen change — so it never reads as a supervisor stall.
            let outcome = if prev_step_progressed {
                crate::supervisor::StepOutcome::Progressed
            } else {
                classify_step_outcome(prev_action_executed, screen_changed)
            };
            prev_step_progressed = false;
            let step_hash = u64::from_le_bytes(
                blake3::hash(screen.as_bytes()).as_bytes()[..8].try_into().unwrap()
            );
            match supervisor.observe(outcome, step_hash) {
                crate::supervisor::Directive::Escalate(tier)
                    if tier.kind == crate::supervisor::TierKind::Human =>
                {
                    let msg = "I've stalled on this and can't make progress on my own — \
                               handing back to you for direction.";
                    chronos::log("supervisor_escalate_human");
                    verify_or_handback(&goal, actuator.as_ref(), &confirm_tx, msg).await;
                    break;
                }
                crate::supervisor::Directive::Abort(reason) => {
                    chronos::log(&format!("supervisor_abort: {reason}"));
                    let _ = confirm_tx.send(envelope::make("status", envelope::StatusPayload {
                        state: "goal_aborted".to_string(),
                        detail: reason,
                    })).await;
                    break;
                }
                crate::supervisor::Directive::Escalate(tier)
                    if tier.kind == crate::supervisor::TierKind::Sense =>
                {
                    // PLANE-GOVERNOR (in-app stepback): the within-plane stepback is SPENT (the supervisor
                    // accumulated stalls past its threshold — NOT one no-effect), so re-aim to the next
                    // feasible IN-APP plane down the visibility ladder (a11y → CV → pixel), REUSING the
                    // existing `sense` mechanism and the current locus (sub-goal). Bounded by feasibility:
                    // when the ladder is exhausted the bump is REFUSED so the supervisor's next escalation
                    // reaches Human — closing the old unbounded blind `sense_level += 1`.
                    let cur = match sense_level {
                        0 => crate::plane::PlaneId::A11y,
                        1 => crate::plane::PlaneId::Cv,
                        _ => crate::plane::PlaneId::Pixel,
                    };
                    let findings = crate::plane::Findings { gui_available: true, ..Default::default() };
                    match crate::plane::next_in_app(cur, &findings) {
                        Some(next) => {
                            sense_level = match next {
                                crate::plane::PlaneId::Cv => 1,
                                crate::plane::PlaneId::Pixel => 2,
                                _ => sense_level,
                            };
                            chronos::log(&format!(
                                "plane_governor: in-app stepback spent on {cur:?} → re-aim to {next:?} (sense {sense_level}) [{}]",
                                tier.label));
                        }
                        None => {
                            // AUTONOMY-FIRST: the in-task ladder is spent UNDER CURRENT FINDINGS — that is
                            // NOT "give up to the human". The world may have changed (a dialog closed, the
                            // app launched), so RE-DISCOVER + RE-PICK: reset to the top of the in-app ladder
                            // and re-attempt with fresh perception. Human is the absolute last resort, reached
                            // only via the supervisor's genuine-stall cap (no world progress) — never here.
                            sense_level = 0;
                            chronos::log(&format!(
                                "plane_governor: in-task ladder exhausted at {cur:?} under current findings → re-discover + re-pick (autonomy-first; human is last resort) [{}]",
                                tier.label));
                        }
                    }
                }
                // Continue / Done / ResetFromBoard / Escalate(Model): defer to the inner
                // tactical machinery (recovery_manager, should_cutoff, structural detection).
                _ => {}
            }

            // SEQUENCER ADVANCE (§2.15): the prior action targeted sub_goals[current_sub]. If it
            // took effect (the a11y screen changed — element-level, not raw pixels), that sub-goal's
            // action accomplished a change → advance the pointer. Exhausting the plan = DETERMINISTIC
            // completion (not the model's fallthrough `complete`). No-effect → pointer holds → the
            // sub-goal is re-selected and should_cutoff/impasse catches a true stall. This subsumes
            // the single-action completion case (one sub-goal → effect → advance past end → done).
            // §2.15 POSTCONDITION: advance only when the sub-goal's ACTION-CLASS effect was confirmed,
            // not on any structural delta. `Open` (reveal a menu) confirms only when elements APPEAR,
            // so clicking an already-open menu's toggle SHUT (a net removal) no longer false-advances
            // into a dead plan; `Activate` keeps the prior any-change signal for everything else. The
            // class→signature map is deterministic (never the model judging itself); the settled
            // before/after states (read_settled_screen) make the comparison direction-honest.
            if prev_action_executed
                && effect_confirmed(effect_class(&sub_goals[current_sub].text), &prev_screen, &screen)
            {
                // INSTRUMENT (Launch-dissolves? check): log the focus the advance fired on. For an app
                // launch ("click Terminal Emulator"), a correct advance fires once focus == the new
                // window; an advance that fires while focus is still the desktop/menu means the blank
                // gap won and a focus-to-new-window LAUNCH gate is still needed.
                chronos::log(&format!("advance_focus: \"{}\" → focus={}",
                    sub_goals[current_sub].text, screen_focus(&screen)));
                current_sub += 1;
                subgoal_stuck = 0; // fresh sub-goal — reset the deviation counter
                if current_sub >= sub_goals.len() {
                    chronos::log("sequencer_complete: all sub-goals done");
                    // GATE the claim on WORLD-STATE, not plan-exhaustion: finishing the planned steps is
                    // NOT the same as achieving the goal (an over-broad action can "complete" yet leave
                    // the world wrong). verify_or_handback claims only if goal_satisfied, else hands back
                    // honestly (conservative-or-silent — a plan-exhaustion claim is the false-success hole).
                    verify_or_handback(&goal, actuator.as_ref(), &confirm_tx,
                        "I finished the planned steps but couldn't verify the goal was achieved — handing back.").await;
                    break;
                }
                chronos::log(&format!("sequencer_advance: → sub-goal {}/{}: {}",
                    current_sub + 1, sub_goals.len(), sub_goals[current_sub].text));
            }
        }
        had_prior_step = true;
        blind_reperceive = false; // consumed for this iteration; re-armed only by a fail-closed re-perceive

        // ── Command channel: deterministic CLI sub-goal with EXIT-CODE verification ───────────
        // Routes a planned shell-command step through the gated command channel (run + read
        // stdout/stderr/exit) instead of GUI typing. Unlike Type/Key (fire-and-advance), the
        // sequencer advances ONLY on exit 0 — a non-zero exit HOLDS the pointer and escalates at
        // threshold, so a failed command in a chain is caught, not marched past (the chain-depth
        // fix). The gate applies the "1 and 3" tiering: read-only commands auto-run, writes confirm.
        if let SubAction::Command(cmd0) = sub_goals[current_sub].action.clone() {
            let subgoal_text = sub_goals[current_sub].text.clone();
            const REFORM_LIMIT: usize = 2; // ≤2 reforms ⇒ ≤3 command executions per sub-goal, all within
                                           // ONE outer step (does not burn subgoal_stuck or MAX_STEPS).
            let mut cmd = cmd0.clone();
            let mut tried: std::collections::HashSet<String> = std::collections::HashSet::new();
            tried.insert(cmd0.clone());
            // REASSESS loop: run → (exit 0 ? advance) → diagnose → reform (bounded, gated, no-repeat) → retry.
            let advanced = loop {
                let mut args = serde_json::Map::new();
                args.insert("command".to_string(), serde_json::Value::String(cmd.clone()));
                let tool_call = ToolCall::Invoke { name: "vm_command".to_string(), args };
                let output = match gate::apply_plan_approval(gate::confidence_escalate(gate::evaluate_action(&tool_call, &registry), 1.0), plan_approved) {
                    gate::Verdict::Allow => {
                        let out = execute_tool(&tool_call, &actuator, perceptor.as_ref(), &memory_tiers).await;
                        chronos::log(&format!("action(command): $ {cmd} -> {out}"));
                        let _ = confirm_tx.send(envelope::make("action_log", envelope::ActionLogPayload {
                            text: format!("$ {cmd}\n{out}"),
                        })).await;
                        out
                    }
                    gate::Verdict::ConfirmTap => request_and_await_approval("tap", &tool_call, &state, &actuator, perceptor.as_ref(), &memory_tiers, &mut approval_rx, &confirm_tx).await,
                    gate::Verdict::ConfirmTyped => request_and_await_approval("typed", &tool_call, &state, &actuator, perceptor.as_ref(), &memory_tiers, &mut approval_rx, &confirm_tx).await,
                    gate::Verdict::Block(reason) => {
                        chronos::log(&format!("blocked: {reason}"));
                        let _ = confirm_tx.send(envelope::make("status", envelope::StatusPayload {
                            state: "blocked".to_string(), detail: reason.clone(),
                        })).await;
                        format!("Blocked: {reason}")
                    }
                };
                memory.push(Step { index: enforcer.step(), prompt: String::new(), output: output.clone(), action: None });
                // WORLD-STATE POSTCONDITION (§11.4): exit 0 ≠ effect achieved. For a file command, confirm
                // its stated effect actually MATERIALIZED in the world (a deterministic `test`, not the
                // model judging itself). A clean exit with the effect ABSENT is exit-0-but-wrong → escalate
                // (reform can't fix a command that "succeeded" without doing the thing).
                if parse_exit_code(&output) == Some(0) {
                    if let Some(check) = command_postcondition(&cmd) {
                        if parse_exit_code(&actuator.run_command(&check)) != Some(0) {
                            chronos::log(&format!("postcondition_failed: '{cmd}' exited 0 but effect absent ({check}) → escalate"));
                            break false;
                        }
                        chronos::log(&format!("postcondition_ok: {check}"));
                    }
                }
                // Reform only when the failure is reformable AND budget remains. DETERMINISTIC-FIRST
                // (the spine): an equivalence-class program swap, command-v-verified — reliable; the weak
                // LLM reform is only the fallback for what determinism can't fix.
                let failure = diagnose_command(&output);
                let candidate = if parse_exit_code(&output) != Some(0)
                    && should_reform(failure)
                    && tried.len() <= REFORM_LIMIT
                {
                    deterministic_reform(&cmd, failure, actuator.as_ref())
                        .or_else(|| reform_command(&subgoal_text, &cmd, &output, &adapter))
                } else {
                    None
                };
                match decide_reapproach(&output, candidate.as_deref(), &tried, REFORM_LIMIT) {
                    ReapproachAction::Advance => break true,
                    ReapproachAction::Retry(next) => {
                        chronos::log(&format!("reapproach: '{cmd}' failed ({:?}) → reform → '{next}'", diagnose_command(&output)));
                        let _ = confirm_tx.send(envelope::make("action_log", envelope::ActionLogPayload {
                            text: format!("That didn't work — trying instead: {next}"),
                        })).await;
                        tried.insert(next.clone());
                        cmd = next;
                        continue;
                    }
                    ReapproachAction::Escalate(reason) => {
                        chronos::log(&format!("reapproach_escalate: {reason} (sub-goal '{subgoal_text}')"));
                        break false;
                    }
                }
            };
            prev_action_executed = false;
            prev_screen = read_screen_bg(&perceptor).await;
            if advanced {
                prev_step_progressed = true; // a verified command advance = progress (no screen change)
                subgoal_stuck = 0;
                current_sub += 1;
                if current_sub >= sub_goals.len() {
                    chronos::log("sequencer_complete: all sub-goals done (command verified)");
                    // EXIT-0 verifies the COMMAND ran, NOT that the GOAL holds (a successful `mv` can
                    // still move the wrong set — the bench's lone false success). Gate the claim on the
                    // goal-level world-state check; unverifiable → honest handback, never a false success.
                    verify_or_handback(&goal, actuator.as_ref(), &confirm_tx,
                        "I ran the planned commands but couldn't verify the goal was achieved — handing back.").await;
                    break;
                }
                chronos::log(&format!("sequencer_advance(command ok): → sub-goal {}/{}: {}",
                    current_sub + 1, sub_goals.len(), sub_goals[current_sub].text));
            } else {
                let msg = format!("A command step failed and I couldn't fix it (\"{subgoal_text}\") — handing back to you.");
                verify_or_handback(&goal, actuator.as_ref(), &confirm_tx, &msg).await;
                break;
            }
            continue;
        }

        // ── WALL 2: deterministic Type/Key sub-goal ──────────────────────────────────────────
        // No element to "click" and the model can't pick a keystroke, so a Type/Key step bypasses
        // perception/selection/fail-closed/grammar entirely: build the tool call, run it through the
        // SAME safety gate, then FIRE-AND-ADVANCE. We must not wait on structural_change to advance
        // (typing often leaves the a11y element-set unchanged → we'd retype); a deterministic step is
        // complete the moment it executes.
        if !matches!(sub_goals[current_sub].action, SubAction::Click) {
            let tool_call = match &sub_goals[current_sub].action {
                SubAction::Type(text) => ToolCall::Type { selector: "focused".to_string(), text: text.clone() },
                SubAction::Key(key)   => ToolCall::Key { key: key.clone() },
                SubAction::Click      => unreachable!(),
                SubAction::Command(_) => unreachable!(), // handled above
            };
            // confidence = 1.0: the harness chose this deterministically, not the model.
            let output = match gate::apply_plan_approval(gate::confidence_escalate(gate::evaluate_action(&tool_call, &registry), 1.0), plan_approved) {
                gate::Verdict::Allow => {
                    let desc = gate::describe_redacted(&tool_call);
                    let out = execute_tool(&tool_call, &actuator, perceptor.as_ref(), &memory_tiers).await;
                    chronos::log(&format!("action(deterministic): {desc} -> {out}"));
                    let _ = confirm_tx.send(envelope::make("action_log", envelope::ActionLogPayload {
                        text: format!("{desc} -> {out}"),
                    })).await;
                    out
                }
                gate::Verdict::ConfirmTap => request_and_await_approval("tap", &tool_call, &state, &actuator, perceptor.as_ref(), &memory_tiers, &mut approval_rx, &confirm_tx).await,
                gate::Verdict::ConfirmTyped => request_and_await_approval("typed", &tool_call, &state, &actuator, perceptor.as_ref(), &memory_tiers, &mut approval_rx, &confirm_tx).await,
                gate::Verdict::Block(reason) => {
                    chronos::log(&format!("blocked: {reason}"));
                    let _ = confirm_tx.send(envelope::make("status", envelope::StatusPayload {
                        state: "blocked".to_string(), detail: reason.clone(),
                    })).await;
                    format!("Blocked: {reason}")
                }
            };
            memory.push(Step { index: enforcer.step(), prompt: String::new(), output, action: Some(tool_call) });
            current_sub += 1;
            prev_step_progressed = true; // a deterministic Type/Key advance = progress (no screen change)
            prev_action_executed = false; // deterministic advance below — don't double-count at loop top
            // Plain read: Type/Key fire-and-advance (no effect-confirmation needed), and typing often
            // leaves the a11y tree unchanged, so a change-from-baseline settle would just burn the ceiling.
            prev_screen = read_screen_bg(&perceptor).await;
            if current_sub >= sub_goals.len() {
                chronos::log("sequencer_complete: all sub-goals done (deterministic final step)");
                // Same gate: a Type/Key plan that "finished" is not a verified goal. Claim only on a
                // passing world-state check; else honest handback (conservative-or-silent).
                verify_or_handback(&goal, actuator.as_ref(), &confirm_tx,
                    "I finished the planned steps but couldn't verify the goal was achieved — handing back.").await;
                break;
            }
            chronos::log(&format!("sequencer_advance(deterministic): → sub-goal {}/{}: {}",
                current_sub + 1, sub_goals.len(), sub_goals[current_sub].text));
            continue;
        }

        // Active sub-goal drives THIS step's selection (ranking / fail-closed / prompt).
        let active_goal: &str = &sub_goals[current_sub].text;

        // MEMORY-ISOLATED executor (verified 2026-06-17 §2.5): injecting retrieved priors (the
        // Board's episodic/visual/skill memory) lets semantically-related text OVERRIDE the
        // candidate labels and flip the pick (decoy-priming → 12/12 wrong). The selection call
        // therefore sees ONLY the pinned SYS framing + the late-band-ranked candidate list + the
        // goal — no priors, no tool dump, no trajectory. Retrieved memory belongs to an upstream
        // planning step (v2), never the click decision; loop/repeat control is the deterministic
        // supervisor's job. The board infra (assemble_slice/embedder/sleep_gate) stays for that
        // planner; it is simply not wired into THIS prompt.

        // Action space = the arbiter's fused, deterministically-indexed candidate set.
        // The model picks ONE synthetic-index token (`el_N`) or the escape ("none"); the
        // selection grammar makes any other target unrepresentable. This replaces the
        // position-biased raw screen dump AND the old single-element grounding hint:
        // determinism on the RAILS (which targets are valid), not on the STRATEGY (which
        // one to pick) — the model keeps full agency over tool + target + escape.
        //
        // Detection = union (a11y ∪ CV): a box enters the candidate set if ANY sense
        // sees it, so CV rescues canvas/custom widgets a11y is blind to. The index
        // space names every element regardless of `ref_id`, so a CV-only box is just
        // another `el_N`.
        let bboxes = crate::perception::parse_ref_bboxes(&screen);
        let labels = crate::perception::parse_ref_labels(&screen);
        // Phase 1b — live CV sense. Read the QMP screendump, decode, propose boxes over
        // the full frame. FAIL-OPEN to a11y-only on any frame error: a dead sense must
        // degrade to the remaining senses, never crash the loop (cross-cutting invariant).
        // CV PASS — gated OFF by default (TWO-WAY door: `LAGADO_CV_ENABLE=1` flips it back on for the
        // Phase-2 sampled-caption collector). Today CV has NO consumer — its output was DISCARDED — so
        // off = zero per-step cost (no Canny/CC, no extra capture, no PNG decode). Even when ENABLED,
        // raw CV boxes do NOT feed selection: captions (Phase 2), not label-less boxes, enter the
        // candidate set. This block is the sampled-collection hook; cv_proposer/arbiter stay as the
        // Phase-2 foundation. SEAM IS LIVE-BUT-IDLE BY DESIGN until captions are generated.
        // CV runs when configured OR when the supervisor has escalated the sense (sense_level ≥ 1)
        // because a11y came up blind — the GOVERNED, after-a11y-failed escalation the ax-blind probe
        // mandated (NOT a CV pre-scan). Today CV is still inert in selection (label-less); when Phase-2
        // captioning lands, these boxes get captioned here and become selectable.
        if crate::config::cv_enabled() || sense_level >= 1 {
            capture_frame_bg(&perceptor).await;
            if let Ok(png) = std::fs::read(crate::config::FRAME_PATH) {
                if let Ok(img) = image::load_from_memory(&png) {
                    let rgb = img.to_rgb8();
                    let _cv_boxes = crate::perception::cv_proposer::propose_frame(rgb.as_raw(), rgb.width(), rgb.height());
                    // TODO(Phase 2): caption `_cv_boxes` → persist for the caption pipeline → captioned
                    // (LABELED) boxes then enter selection via `fuse`'s caption argument.
                }
            }
        }
        // SELECTION IS a11y-ONLY. Raw CV boxes are label-less → `goal_matches_any`/`best_match_token`
        // can never match them. Confidence, stated honestly: removing them is PROVEN-equivalent on the
        // Phase-1 covered screens, expected-equivalent elsewhere BY MECHANISM (inert boxes can't win),
        // and UNVERIFIED on sparse / custom-rendered screens where a11y is thin and CV boxes are a larger
        // fraction — an accepted, known gap, pinned by `label_less_boxes_do_not_change_selection`.
        let fused = crate::perception::arbiter::fuse(&bboxes, &labels, &[], &[]);
        let candidates = crate::perception::selection::build_candidates(&fused);
        chronos::log(&format!("perceive: {} a11y → {} fused", bboxes.len(), fused.len()));
        // AUDIT: log the exact labels the agent perceives this step (not just the count) so we can
        // see what the selector/fail-closed actually had to match against.
        chronos::log(&format!("candidates[{}] for \"{active_goal}\": {}", candidates.len(),
            candidates.iter().map(|c| if c.label.is_empty() { "<unlabeled>".to_string() } else { format!("\"{}\"", c.label) }).collect::<Vec<_>>().join(" ")));
        // Deterministic FAIL-CLOSED (verified §2.7: the model emits the escape token `none` 0/12
        // on a no-match screen — it forces a wrong click instead of declining, so the harness must
        // decide). No candidate label shares a content token with the goal → re-perceive rather
        // than force a wrong action. Label-less CV/vision-only elements never match → Tier 2/3
        // escalate, by design. Bounded by the step cap (enforcer.advance) + supervisor stall→human.
        if !candidates.is_empty()
            && !crate::perception::selection::goal_matches_any(active_goal, &candidates)
        {
            subgoal_stuck += 1;
            // DEVIATION → ESCALATE: the screen has not matched this sub-goal for several
            // re-perceptions → the world is off-plan (or this step is already done / ambiguous).
            // The deterministic plan can't re-plan → clean handback, not an infinite re-perceive.
            if subgoal_stuck >= SUBGOAL_STUCK_LIMIT {
                // a11y is structurally blind to a target for this sub-goal. Route through the
                // supervisor as PerceptionBlind — the semantic/outcome escalation the ax-blind probe
                // mandated (retrying the same model on the same a11y read cannot help). The governed
                // ladder decides: a Sense rung (when captioning lands) → bump the sense level and
                // re-perceive richer; otherwise (today: [model, human]) → clean handback to the human.
                let blind_hash = u64::from_le_bytes(
                    blake3::hash(screen.as_bytes()).as_bytes()[..8].try_into().unwrap()
                );
                chronos::log(&format!("perception_blind: a11y has no target for sub-goal after {subgoal_stuck} re-perceptions"));
                match supervisor.observe(crate::supervisor::StepOutcome::PerceptionBlind, blind_hash) {
                    crate::supervisor::Directive::Escalate(tier)
                        if tier.kind == crate::supervisor::TierKind::Sense =>
                    {
                        sense_level = sense_level.saturating_add(1);
                        chronos::log(&format!("sense_escalate: a11y-blind → sense '{}' (level {sense_level}) → re-perceive", tier.label));
                        subgoal_stuck = 0;
                        prev_screen = screen.clone();
                        prev_action_executed = false;
                        continue;
                    }
                    _ => {
                        let msg = format!(
                            "The screen doesn't match what this step needs (\"{active_goal}\") — handing back to you. \
                             It may already be done, or the screen went somewhere I didn't plan for."
                        );
                        // "It may already be done" → CHECK before giving up (the under-claim fix).
                        verify_or_handback(&goal, actuator.as_ref(), &confirm_tx, &msg).await;
                        break;
                    }
                }
            }
            chronos::log(&format!("fail_closed: no candidate matches sub-goal → re-perceive ({subgoal_stuck}/{SUBGOAL_STUCK_LIMIT})"));
            let _ = confirm_tx.send(envelope::make("action_log", envelope::ActionLogPayload {
                text: "No on-screen element matches the current step; re-perceiving.".to_string(),
            })).await;
            memory.push(Step {
                index: enforcer.step(), prompt: String::new(),
                output: "fail_closed: re-perceive".to_string(), action: None,
            });
            prev_screen = screen.clone();
            prev_action_executed = false;
            blind_reperceive = true; // keep this no-act re-perceive out of the supervisor's counters
            continue;
        }
        subgoal_stuck = 0; // a candidate matches this sub-goal → not stuck
        // Deterministic LATE-BAND RANK (verified §2.2–2.6): place the most goal-relevant candidate
        // LAST, where the model's label-reading holds (it collapses for early rows). Determinism on
        // the RAILS (ordering); the model still picks. Tokens/coords stay stable under the reorder.
        let candidates = crate::perception::selection::rank_late_band(candidates, active_goal);
        // Register `el_N` → center so the chosen token resolves to a coord click (works for
        // label-less / `ref_id`-`None` elements — the point of the index space).
        actuator.set_targets(crate::perception::selection::candidate_coords(&candidates));
        let candidate_block = crate::perception::selection::render_candidates(&candidates);
        // GBNF over THIS frame's RENDERED candidates (+ escape; escape is a deterministic-only
        // path — the model never reaches it). Derived from the ranked+capped count, NOT `fused`,
        // so the grammar offers exactly the tokens the prompt shows. Empty → unconstrained decoding.
        let grammar = crate::grammar::selector_grammar(candidates.len());
        if !grammar.is_empty() {
            chronos::log(&format!("selector_grammar: {} candidates (late-band) + escape", candidates.len()));
        }

        // PINNED, MEMORY-ISOLATED executor prompt: SYS framing + late-band candidate list + goal.
        // ⚠ The SYS preamble LENGTH is LOAD-BEARING — it lands the candidate list in the model's
        // late-attention band, which is WHY label-reading works (verified §2.6: strip the preamble
        // and selection collapses to first-position, label-blind). Do NOT trim the system prompt or
        // reorder this template without re-running the position sweep (the regression guard:
        // docs/plans/experiments/lean_gate.py + h2h.py) — selection rots SILENTLY otherwise.
        // Goal-slot uses the DISCRIMINATING phrasing (§2.18): the verbose sub-goal leaks category
        // tokens ("…menu") that lexically pull a decoy ("Directory Menu"); the discriminating token
        // ("Applications") clicks correctly. Deterministic, at the handoff — not a ranker.
        let prompt_goal = crate::perception::selection::discriminating_phrase(active_goal);
        // MEMORY-ISOLATED BY CONSTRUCTION (inv #10): the builder takes ONLY sys + candidates + goal —
        // it has no parameter through which episodic/visual/skill memory could reach the executor.
        // Guarded by `executor_prompt_is_memory_isolated` so new senses can't silently leak (§4.3).
        let prompt = build_executor_prompt(&system_prompt, &candidate_block, &screen, &prompt_goal);

        let adapter_clone = adapter.clone();
        let grammar_for_call = grammar.clone();
        let forge = Forge {
            model_fn: Box::new(move |p: String| {
                let adapter = adapter_clone.clone();
                let grammar = grammar_for_call.clone();
                Box::pin(async move {
                    tokio::task::spawn_blocking(move || {
                        // Empty grammar == unconstrained (llama_cpp omits the field), so this
                        // is a safe no-op when there are no candidates.
                        adapter.generate_constrained(&p, 2048, 0.2, &grammar)
                            .map_err(|e| PipelineError::ModelError(e))
                    })
                    .await
                    .map_err(|e| PipelineError::ModelError(e.to_string()))?
                })
            }),
        };

        match forge.call_with_retry(&prompt, &enforcer).await {
            Ok((tool_call, confidence)) => {
                tracing::info!("Step {}: {:?} [conf={:.2}]", enforcer.step(), tool_call, confidence);
                if confidence < 0.6 && confidence != 1.0 {
                    chronos::log(&format!(
                        "low_confidence: step={} conf={:.2} action={:?}",
                        enforcer.step(), confidence, tool_call
                    ));
                }

                // Escape production: the model judged none of the candidates fit. Re-perceive
                // rather than force a wrong click — a fusion miss becomes a recoverable signal,
                // never a forced wrong action. Bounded by the step cap (enforcer.advance at the
                // loop top) and the supervisor's stall detection (repeated no-change → human).
                if let ToolCall::Click { ref selector } = tool_call {
                    if selector == crate::perception::selection::ESCAPE_TOKEN {
                        chronos::log("selector_escape: none-of-these → re-perceive");
                        let _ = confirm_tx.send(envelope::make("action_log", envelope::ActionLogPayload {
                            text: "No suitable on-screen element for the goal; re-perceiving.".to_string(),
                        })).await;
                        memory.push(Step {
                            index: enforcer.step(),
                            prompt: prompt.clone(),
                            output: "selector_escape: re-perceive".to_string(),
                            action: None,
                        });
                        prev_screen = screen.clone();
                        prev_action_executed = false;
                        continue;
                    }
                }

                // SELECTION-INTENT DIVERGENCE rail (§2.18+): if the deterministic matcher has a
                // UNIQUE best-matching candidate for this sub-goal and the model clicked a DIFFERENT
                // element, fail closed BEFORE acting — a divergent click is exactly how the step-1
                // decoy ("Directory Menu") and the step-2 wrong-app ("Run Program…" → Application
                // Finder, falsely "complete") slipped through. Validates the pick (determinism on the
                // RAILS), does NOT decide it (no unique match → model's pick stands). This also makes
                // the completion signal HONEST: an advance now means the intended target was clicked.
                if let ToolCall::Click { ref selector } = tool_call {
                    if let Some(intended) = crate::perception::selection::best_match_token(&candidates, active_goal) {
                        if *selector != intended {
                            subgoal_stuck += 1;
                            chronos::log(&format!(
                                "selection_divergence: model picked {selector}, intended {intended} ({subgoal_stuck}/{SUBGOAL_STUCK_LIMIT})"
                            ));
                            if subgoal_stuck >= SUBGOAL_STUCK_LIMIT {
                                let msg = format!(
                                    "I keep selecting the wrong on-screen element for this step (\"{active_goal}\") — handing back to you."
                                );
                                chronos::log("selection_divergence_escalate");
                                verify_or_handback(&goal, actuator.as_ref(), &confirm_tx, &msg).await;
                                break;
                            }
                            let _ = confirm_tx.send(envelope::make("action_log", envelope::ActionLogPayload {
                                text: "Selected the wrong element for the current step; re-perceiving.".to_string(),
                            })).await;
                            memory.push(Step { index: enforcer.step(), prompt: String::new(),
                                output: "selection_divergence: re-perceive".to_string(), action: None });
                            prev_screen = screen.clone();
                            prev_action_executed = false;
                            continue;
                        }
                    }
                }

                let current_action_desc = gate::describe(&tool_call);
                let screen_unchanged = !prev_screen.is_empty()
                    && blake3::hash(screen.as_bytes()) == blake3::hash(prev_screen.as_bytes());

                // (Q1 action-effect detection now lives at the loop TOP as the SEQUENCER ADVANCE:
                // an action that took effect advances the sub-goal pointer, and the single-sub-goal
                // case advances past the end → deterministic completion — so the re-click is
                // prevented by the sub-goal changing/completing before the next selection, not by a
                // halt here. should_cutoff below still catches the same-action-with-NO-effect stall.)

                // Pre-execution cutoff: 3rd+ identical action with no visible screen effect.
                // Uses prev_screen (set at end of prior turn) to detect whether the 2nd
                // identical execution actually changed anything.
                if should_cutoff(&current_action_desc, &last_exec_action, consecutive_exec_count, screen_unchanged) {
                    let impasse = format!(
                        "I attempted '{}' twice with no visible effect; stopping rather than repeating.",
                        last_exec_action
                    );
                    chronos::log(&format!("impasse: {impasse}"));
                    let _ = confirm_tx.send(envelope::make("action_log", envelope::ActionLogPayload {
                        text: impasse.clone(),
                    })).await;
                    let _ = confirm_tx.send(envelope::make("status", envelope::StatusPayload {
                        state: "goal_done".to_string(),
                        detail: impasse,
                    })).await;
                    prev_screen = screen.clone();
                    prev_action_executed = false;
                    break;
                }

                // Conversational response — emit as chat message and end this goal
                if let ToolCall::Chat { ref text } = tool_call {
                    chronos::log(&format!("chat_response: {}", &text[..text.len().min(80)]));
                    let _ = confirm_tx.send(envelope::make("action_log", envelope::ActionLogPayload {
                        text: text.clone(),
                    })).await;
                    memory.push(Step {
                        index: enforcer.step(),
                        prompt: prompt.clone(),
                        output: text.clone(),
                        action: Some(tool_call.clone()),
                    });
                    break;
                }

                // state mutex is NOT held from here through approval_rx.recv()
                let base_verdict = gate::evaluate_action(&tool_call, &registry);
                let output = match gate::apply_plan_approval(gate::confidence_escalate(base_verdict, confidence), plan_approved) {
                    gate::Verdict::Allow => {
                        let desc = gate::describe_redacted(&tool_call);
                        let out = execute_tool(&tool_call, &actuator, perceptor.as_ref(), &memory_tiers).await;
                        chronos::log(&format!("action: {desc} -> {out}"));
                        let _ = confirm_tx.send(envelope::make("action_log", envelope::ActionLogPayload {
                            text: format!("{desc} -> {out}"),
                        })).await;
                        out
                    }
                    gate::Verdict::ConfirmTap => {
                        request_and_await_approval("tap", &tool_call, &state, &actuator, perceptor.as_ref(), &memory_tiers, &mut approval_rx, &confirm_tx).await
                    }
                    gate::Verdict::ConfirmTyped => {
                        request_and_await_approval("typed", &tool_call, &state, &actuator, perceptor.as_ref(), &memory_tiers, &mut approval_rx, &confirm_tx).await
                    }
                    gate::Verdict::Block(reason) => {
                        chronos::log(&format!("blocked: {reason}"));
                        let _ = confirm_tx.send(envelope::make("status", envelope::StatusPayload {
                            state: "blocked".to_string(),
                            detail: reason.clone(),
                        })).await;
                        tracing::warn!("Action blocked: {}", reason);
                        format!("Blocked: {}", reason)
                    }
                };

                // An action "executed" if it wasn't denied by the user or blocked by the gate.
                let action_executed = !output.starts_with("Denied by user:")
                    && !output.starts_with("Blocked:");

                memory.push(Step {
                    index: enforcer.step(),
                    prompt: prompt.clone(),
                    output,
                    action: Some(tool_call.clone()),
                });

                recent_actions.push(gate::describe(&tool_call));
                if recent_actions.len() > 15 { recent_actions.remove(0); }

                // Consecutive-identical-action tracking.
                // At count == 2: fire urgency injection (recovery manager) so the model knows
                // it has repeated the same action and should reconsider.
                if action_executed {
                    if current_action_desc == last_exec_action {
                        consecutive_exec_count += 1;
                    } else {
                        last_exec_action = current_action_desc.clone();
                        consecutive_exec_count = 1;
                    }

                    if consecutive_exec_count == 2 {
                        if let Some(ref rm) = recovery_manager {
                            match rm.recover(&FailureType::LoopDetected, &state_hash, &screen, &recent_actions).await {
                                Some(RecoveryOutcome::PromptInjection { text, .. }) => {
                                    tracing::info!("Repeated-action urgency injected at count=2");
                                    memory.push(Step { index: enforcer.step(), prompt: text, output: "recovery_injection".to_string(), action: None });
                                    prev_screen = screen.clone();
                                    prev_action_executed = false;
                                    continue;
                                }
                                _ => {}
                            }
                        }
                    }
                }

                // Structural failure detection (loop / deadlock — still guards scroll-type repeats)
                if let Some(structural) = FailureType::detect_structural(&recent_actions) {
                    tracing::warn!("Structural failure detected: {structural}");
                    if let Some(ref rm) = recovery_manager {
                        let s = read_screen_bg(&perceptor).await;
                        match rm.recover(&structural, &state_hash, &s, &recent_actions).await {
                            Some(RecoveryOutcome::PromptInjection { text, .. }) => {
                                tracing::info!("Recovery injection: {}", &text[..text.len().min(80)]);
                                memory.push(Step { index: enforcer.step(), prompt: text, output: "recovery_injection".to_string(), action: None });
                                prev_screen = screen.clone();
                                prev_action_executed = false;
                                continue;
                            }
                            _ => {
                                prev_screen = screen.clone();
                                prev_action_executed = false;
                                break;
                            }
                        }
                    } else {
                        prev_screen = screen.clone();
                        prev_action_executed = false;
                        break;
                    }
                }

                // Update screen observation state for next turn.
                // Done here so all `continue` paths above that explicitly set these
                // are the only exceptions; normal fall-through always updates both.
                prev_screen = screen.clone();
                prev_action_executed = action_executed;

                match &tool_call {
                    ToolCall::Done { reason } => {
                        chronos::log(&format!("goal_done: {reason}"));
                        let _ = confirm_tx.send(envelope::make("status", envelope::StatusPayload {
                            state: "goal_done".to_string(),
                            detail: reason.clone(),
                        })).await;
                        let episode_id = {
                            let mut tiers = memory_tiers.lock().await;
                            tiers.push_episode_id(format!("Goal '{goal}': {reason}")).ok()
                        };
                        encode_and_store_async(episode_id, &visual_encoder, memory_tiers.clone());
                        distill_skill_async(goal.clone(), memory.context_string(), recent_actions.len(), adapter.clone(), skill_library.clone());
                        tracing::info!("Goal achieved.");
                        break;
                    }
                    ToolCall::Task { description } => {
                        chronos::log(&format!("goal_done: {description}"));
                        let _ = confirm_tx.send(envelope::make("status", envelope::StatusPayload {
                            state: "goal_done".to_string(),
                            detail: description.clone(),
                        })).await;
                        let episode_id = {
                            let mut tiers = memory_tiers.lock().await;
                            tiers.push_episode_id(format!("Task '{goal}': {description}")).ok()
                        };
                        encode_and_store_async(episode_id, &visual_encoder, memory_tiers.clone());
                        distill_skill_async(goal.clone(), memory.context_string(), recent_actions.len(), adapter.clone(), skill_library.clone());
                        tracing::info!("Goal achieved.");
                        break;
                    }
                    ToolCall::Click { .. } | ToolCall::Type { .. } | ToolCall::Key { .. }
                    | ToolCall::Wait { .. } | ToolCall::Chat { .. } | ToolCall::Invoke { .. } => {}
                }
            }
            Err(e) => {
                tracing::warn!("Pipeline error: {:?}", e);
                let failure_type = FailureType::from(&e);
                tracing::info!("Failure classified: {failure_type}");

                memory.push(Step {
                    index: enforcer.step(),
                    prompt: prompt.clone(),
                    output: format!("Error: {:?}", e),
                    action: None,
                });

                if matches!(e, PipelineError::MaxRetriesExceeded | PipelineError::MaxStepsExceeded) {
                    // Try recovery before aborting
                    if let Some(ref rm) = recovery_manager {
                        let s = read_screen_bg(&perceptor).await;
                        match rm.recover(&failure_type, &state_hash, &s, &recent_actions).await {
                            Some(RecoveryOutcome::PromptInjection { text, .. }) => {
                                tracing::info!("Recovery: prompt injection");
                                memory.push(Step { index: enforcer.step(), prompt: text, output: "recovery_injection".to_string(), action: None });
                                prev_screen = screen.clone();
                                prev_action_executed = false;
                                continue;
                            }
                            Some(RecoveryOutcome::MemoryReset { discard_steps }) => {
                                tracing::info!("Recovery: memory reset ({discard_steps} steps)");
                                prev_screen = screen.clone();
                                prev_action_executed = false;
                                // Phase 2: implement memory.discard_last(discard_steps)
                                continue;
                            }
                            Some(RecoveryOutcome::HealedAction(action)) => {
                                tracing::info!("Recovery: healed action from graph");
                                memory.push(Step { index: enforcer.step(), prompt: action, output: "healed".to_string(), action: None });
                                prev_screen = screen.clone();
                                prev_action_executed = false;
                                continue;
                            }
                            None => {}
                        }
                    }
                    let detail = format!("{:?}", e);
                    chronos::log(&format!("goal_aborted: {detail}"));
                    let episode_id = {
                        let mut tiers = memory_tiers.lock().await;
                        tiers.push_episode_id(format!(
                            "Aborted '{goal}' at step {}: {detail}", enforcer.step()
                        )).ok()
                    };
                    encode_and_store_async(episode_id, &visual_encoder, memory_tiers.clone());
                    let _ = confirm_tx.send(envelope::make("status", envelope::StatusPayload {
                        state: "goal_aborted".to_string(),
                        detail,
                    })).await;
                    break;
                }
            }
        }
    }
}

/// Spawn a background task that encodes the current frame and stores the embedding.
/// Lock is held only for the brief store call — encode runs outside the lock.
/// No-op when encoder is None (non-Linux or model files absent).
fn encode_and_store_async(
    episode_id: Option<String>,
    encoder: &Option<Arc<crate::vision::VisualEncoder>>,
    memory_tiers: Arc<tokio::sync::Mutex<crate::memory_tiers::MemoryTiers>>,
) {
    let id = match episode_id { Some(id) => id, None => return };
    let enc = match encoder { Some(e) => e.clone(), None => return };
    tokio::spawn(async move {
        let png = match std::fs::read(crate::config::FRAME_PATH) { Ok(b) => b, Err(_) => return };
        let embd = tokio::task::spawn_blocking(move || enc.encode_png(&png))
            .await
            .unwrap_or(None);
        if let Some(embd) = embd {
            let mut tiers = memory_tiers.lock().await;
            let _ = tiers.store_visual_embedding(&id, &embd);
        }
    });
}

/// Distill a reusable skill from a completed episode via a background LLM summarization call.
/// Fail-silent — never blocks or errors the completion path.
/// Skips episodes with fewer than 2 real tool steps (those belong to action_graph).
fn distill_skill_async(
    goal: String,
    trajectory: String,
    step_count: usize,
    adapter: Arc<dyn crate::inference::InferenceAdapter>,
    skill_library: Arc<SkillLibrary>,
) {
    if step_count < 2 { return; }
    tokio::spawn(async move {
        let prompt = build_distill_prompt(&goal, &trajectory);
        let result = tokio::task::spawn_blocking(move || {
            adapter.generate(&prompt, 256, 0.3)
        }).await;
        if let Ok(Ok(text)) = result {
            if let Some(skill) = parse_skill_json(&text, &goal) {
                let name = skill.name.clone();
                if skill_library.save(&skill).is_ok() {
                    tracing::debug!("skill distilled: {name}");
                }
            }
        }
    });
}

fn build_distill_prompt(goal: &str, trajectory: &str) -> String {
    format!(
        "Goal completed: \"{goal}\"\n\nTrajectory:\n{trajectory}\n\n\
Extract a reusable skill from this experience. \
Respond with ONLY a valid JSON object — no markdown fences, no explanation:\n\
{{\"name\": \"snake_case_name\", \"description\": \"phrase as a user would state the goal or trigger\", \"approach\": \"the key lesson or method\"}}\n\n\
Rules: name is lowercase_snake_case 2-4 words; description uses vocabulary a user would write when asking for this task; approach is 1-3 sentences."
    )
}

fn parse_skill_json(text: &str, fallback_goal: &str) -> Option<Skill> {
    let stripped = text.trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let start = stripped.find('{')?;
    let end   = stripped.rfind('}')?;
    let json  = &stripped[start..=end];

    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let name        = v["name"].as_str()?.trim().to_string();
    let description = v["description"].as_str()?.trim().to_string();
    let approach    = v["approach"].as_str()?.trim().to_string();

    if name.is_empty() || approach.is_empty() { return None; }
    let description = if description.len() >= 5 { description } else { fallback_goal.to_string() };

    Some(Skill::from_episode(name, description, approach, vec![]))
}

// ── Observation + cutoff tests ────────────────────────────────────

#[cfg(test)]
mod observation_tests {
    use super::*;

    // ── ACCEPTANCE GATES (pending hardware verification) ────────────────────────────────────────
    // The smoothness claim is NOT "met" until these pass on real hardware. They are #[ignore]'d so
    // they show as "ignored" in EVERY `cargo test` run (a permanent, un-forgettable pending marker)
    // and FAIL if run before someone verifies them on the VM — converting a recorded caveat into a
    // gate the system won't let silently re-inflate to "smooth". Run on hardware:
    //   cargo test --lib -- --ignored acceptance_gate
    // and replace the panic with the real scenario assertion once it passes.

    #[test]
    #[ignore = "ACCEPTANCE GATE — verify on hardware, then implement the assertion"]
    fn acceptance_gate_slow_action_30s() {
        // GATE: an injected ~30s slow action (e.g. a deliberately laggy app launch) must NOT trip a
        // premature settle/re-action — observe_until_quiet must wait it out, the step completes once,
        // and the runtime stays responsive throughout (spawn_blocking, not a frozen worker).
        panic!("UNVERIFIED on hardware: inject a 30s action, assert single completion + responsive runtime");
    }

    #[test]
    #[ignore = "ACCEPTANCE GATE — verify on hardware, then implement the assertion"]
    fn acceptance_gate_hung_app_escalation() {
        // GATE: a genuinely hung app (a11y-stable AND pixel-quiet, never completing) must escalate via
        // the chain settle→no-confirm→should_cutoff→human within bounds — NOT loop, NOT hang the loop.
        panic!("UNVERIFIED on hardware: launch a hung app, assert bounded escalation to human handback");
    }

    #[test]
    fn strip_list_marker_handles_common_bullets() {
        assert_eq!(strip_list_marker("1. Open the Applications menu"), "Open the Applications menu");
        assert_eq!(strip_list_marker("  - Click Terminal Emulator"), "Click Terminal Emulator");
        assert_eq!(strip_list_marker("2) press Enter"), "press Enter");
        assert_eq!(strip_list_marker("• type touch /tmp/x"), "type touch /tmp/x");
        assert_eq!(strip_list_marker("Open the menu"), "Open the menu"); // no marker → unchanged
    }

    #[test]
    fn executor_prompt_is_memory_isolated() {
        // A multi-sense candidate block (a11y label + label-less CV + an OmniParser-style caption),
        // exactly the wiring that risks leaking a new sense's text into a memory slot.
        let sys = "SYSTEM-PREAMBLE-PINNED";
        let candidate_block = "el_0 \"Applications\"\nel_1 <no label>\nel_2 \"Submit button\"";
        let goal = "Applications";
        let prompt = build_executor_prompt(sys, candidate_block, "RAW-SCREEN-TEXT", goal);

        // The prompt is EXACTLY sys + candidates + goal — any added section breaks this equality,
        // which is the whole point: the builder cannot carry memory it was never given.
        assert_eq!(
            prompt,
            format!("{sys}\n\n{candidate_block}\nGoal: {goal}\n\nWhat is your next action?")
        );
        // Raw screen is omitted whenever candidates exist.
        assert!(!prompt.contains("RAW-SCREEN-TEXT"));
        // None of the Board/memory context markers can appear (inv #10) — the builder has no path.
        for leak in ["Relevant procedures from experience", "episodic", "similar past episode", "skill", "used 1 time"] {
            assert!(!prompt.to_lowercase().contains(&leak.to_lowercase()), "memory leaked into executor prompt: {leak}");
        }
    }

    #[test]
    fn classify_subgoal_types_clicks_and_keys() {
        // Type: framing prefix stripped, literal command preserved (case-sensitive).
        assert_eq!(classify_subgoal("type the command: touch /tmp/x").action,
                   SubAction::Type("touch /tmp/x".into()));
        assert_eq!(classify_subgoal("type echo HeLLo > /tmp/y").action,
                   SubAction::Type("echo HeLLo > /tmp/y".into()));
        // Key: natural names → xdotool keysyms.
        assert_eq!(classify_subgoal("press Enter").action, SubAction::Key("Return".into()));
        assert_eq!(classify_subgoal("press the Tab key").action, SubAction::Key("Tab".into()));
        assert_eq!(classify_subgoal("hit Escape").action, SubAction::Key("Escape".into()));
        // Click: anything else (the default selection path).
        assert_eq!(classify_subgoal("Click Terminal Emulator").action, SubAction::Click);
        assert_eq!(classify_subgoal("Open the Applications menu").action, SubAction::Click);
        // A bare "type" with no payload must not become an empty Type — falls back to Click.
        assert_eq!(classify_subgoal("type").action, SubAction::Click);
    }

    #[test]
    fn planner_passes_explicit_multistep_through_deterministically() {
        // An explicit "X then Y" goal must NOT reach the model planner — it keeps the deterministic
        // connective split. decompose_goal is the proof point (plan_goal returns it verbatim when
        // len > 1, before any adapter call), so a null adapter would never be invoked.
        let parts = decompose_goal("Open the Applications menu then launch the Terminal Emulator");
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[1], "launch the Terminal Emulator");
    }

    #[test]
    fn no_observation_when_no_prior_action() {
        // First turn: prev_executed = false → always None regardless of content
        assert!(observation_for(false, "ref_1 button", "ref_1 button").is_none());
        assert!(observation_for(false, "",            "ref_1 button").is_none());
    }

    #[test]
    fn unchanged_screen_emits_no_effect_message() {
        let s = "[focused: Thunar]\n  ref_1  button  \"Back\"  (14,113,37,35)";
        let obs = observation_for(true, s, s).unwrap();
        assert!(obs.contains("did NOT change"), "got: {obs}");
    }

    #[test]
    fn appeared_elements_reported_with_count() {
        let prev = "[focused: Thunar]\n  ref_1  button  \"Back\"";
        let curr = "[focused: Thunar]\n  ref_1  button  \"Back\"\n  ref_2  menu  \"File\"";
        let obs = observation_for(true, prev, curr).unwrap();
        assert!(obs.contains("changed"), "got: {obs}");
        assert!(obs.contains("1 new"), "expected '1 new', got: {obs}");
    }

    #[test]
    fn disappeared_elements_reported_with_count() {
        let prev = "[focused: Thunar]\n  ref_1  button  \"Back\"\n  ref_2  menu  \"File\"";
        let curr = "[focused: Thunar]\n  ref_1  button  \"Back\"";
        let obs = observation_for(true, prev, curr).unwrap();
        assert!(obs.contains("changed"), "got: {obs}");
        assert!(obs.contains("1 elements disappeared"), "got: {obs}");
    }

    #[test]
    fn cutoff_fires_on_third_identical_with_no_change() {
        assert!(should_cutoff("click(ref_1)", "click(ref_1)", 2, true));
    }

    #[test]
    fn cutoff_suppressed_when_screen_changed_scroll_type() {
        // Scroll-type repeat: same action but screen DID change → must NOT cut off
        assert!(!should_cutoff("scroll(ref_1)", "scroll(ref_1)", 2, false));
    }

    #[test]
    fn cutoff_suppressed_before_count_two() {
        // First repeat (count=1 already executed, this is the 2nd) — not yet cutoff territory
        assert!(!should_cutoff("click(ref_1)", "click(ref_1)", 1, true));
    }

    #[test]
    fn cutoff_suppressed_on_different_action() {
        assert!(!should_cutoff("click(ref_2)", "click(ref_1)", 2, true));
    }

    #[test]
    fn step_outcome_unexecuted_is_failed() {
        use crate::supervisor::StepOutcome;
        // Blocked/denied action or recovery-injection turn → no real attempt landed.
        assert_eq!(classify_step_outcome(false, false), StepOutcome::Failed);
        assert_eq!(classify_step_outcome(false, true), StepOutcome::Failed);
    }

    #[test]
    fn step_outcome_executed_with_screen_change_is_progress() {
        use crate::supervisor::StepOutcome;
        assert_eq!(classify_step_outcome(true, true), StepOutcome::Progressed);
    }

    #[test]
    fn step_outcome_executed_without_screen_change_is_nochange() {
        use crate::supervisor::StepOutcome;
        assert_eq!(classify_step_outcome(true, false), StepOutcome::NoChange);
    }

    // ── decompose_goal ───────────────────────────────────────────────

    #[test]
    fn decompose_single_step_is_one_subgoal() {
        assert_eq!(decompose_goal("Click the Applications menu"), vec!["Click the Applications menu"]);
    }

    #[test]
    fn decompose_splits_on_then() {
        assert_eq!(
            decompose_goal("Open the Applications menu then launch the Terminal Emulator"),
            vec!["Open the Applications menu", "launch the Terminal Emulator"]
        );
    }

    #[test]
    fn decompose_splits_multiple_and_then() {
        assert_eq!(
            decompose_goal("open A, then open B, then open C"),
            vec!["open A", "open B", "open C"]
        );
    }

    #[test]
    fn decompose_does_not_split_bare_and() {
        // "and" without "then" is NOT a sequential marker — stays one sub-goal (never mangle a
        // semantically-compound goal into garbage steps; rely on executor + handback instead).
        assert_eq!(
            decompose_goal("find the cheapest flight and book it"),
            vec!["find the cheapest flight and book it"]
        );
    }

    #[test]
    fn decompose_trims_and_drops_empty() {
        assert_eq!(decompose_goal("  open A ;  open B  "), vec!["open A", "open B"]);
    }

    // ── structural_change (sequencer advance signal) ─────────────────

    const DESK: &str = "[focused: (desktop)]\n  ref_1 toggle button \"Applications\" (0,0,102,26)\n  ref_2 toggle button \"Show Desktop\" (488,752,48,48)";
    const MENU: &str = "[focused: (desktop)]\n  ref_1 toggle button \"Applications\" (0,0,102,26)\n  ref_2 toggle button \"Show Desktop\" (488,752,48,48)\n  ref_7 menu item \"Terminal Emulator\" (0,55,173,25)";

    #[test]
    fn structural_change_true_when_new_element_appears() {
        // menu opened → "Terminal Emulator" label is new → structural change.
        assert!(structural_change(DESK, MENU));
    }

    #[test]
    fn structural_change_false_when_label_set_identical() {
        assert!(!structural_change(DESK, DESK));
    }

    #[test]
    fn structural_change_true_on_focus_change() {
        let term = "[focused: Terminal - laputa@vm]\n  ref_1 toggle button \"Applications\" (0,0,102,26)\n  ref_2 toggle button \"Show Desktop\" (488,752,48,48)";
        assert!(structural_change(DESK, term));
    }

    #[test]
    fn structural_change_false_on_empty_prev() {
        assert!(!structural_change("", MENU));
    }

    #[test]
    fn structural_change_ignores_non_a11y_noise() {
        // Same a11y label set + same focus, even if other (non-label) text differs → no advance
        // (a tooltip / pixel animation that isn't an accessibility element must not advance).
        let with_trailing = format!("{DESK}\n  (cursor moved)");
        assert!(!structural_change(DESK, &with_trailing));
    }

    // ── §2.15 effect-signature postcondition ─────────────────────────

    #[test]
    fn effect_class_open_only_for_reveal_of_a_container() {
        assert_eq!(effect_class("Open the Applications menu"), EffectClass::Open);
        assert_eq!(effect_class("expand the context menu"), EffectClass::Open);
        // App launch / generic clicks are NOT Open (no container target) → Activate (any-change).
        assert_eq!(effect_class("click the Terminal Emulator"), EffectClass::Activate);
        assert_eq!(effect_class("Open the file manager"), EffectClass::Activate);
        assert_eq!(effect_class("click Submit"), EffectClass::Activate);
    }

    #[test]
    fn open_confirms_on_reveal_not_on_close() {
        // Opening the menu (DESK → MENU): "Terminal Emulator" appeared → confirmed.
        assert!(effect_confirmed(EffectClass::Open, DESK, MENU));
        // THE FIX: the menu was already open and the toggle SHUT it (MENU → DESK): elements vanished,
        // none appeared → NOT confirmed → the sequencer must not advance into a now-closed menu.
        assert!(!effect_confirmed(EffectClass::Open, MENU, DESK));
        // No change at all → not confirmed either.
        assert!(!effect_confirmed(EffectClass::Open, MENU, MENU));
    }

    #[test]
    fn activate_keeps_any_structural_change_signal() {
        // Activate is the catch-all = prior behavior: any structural change confirms.
        assert!(effect_confirmed(EffectClass::Activate, DESK, MENU));
        assert!(effect_confirmed(EffectClass::Activate, MENU, DESK)); // even a close confirms Activate
        assert!(!effect_confirmed(EffectClass::Activate, DESK, DESK));
    }

    // ── observe-until-quiet: in-progress vs stuck discrimination (the hard part) ──

    #[test]
    fn settling_active_discriminates_in_progress_from_settled() {
        const NOISE: usize = 2;
        // COLD-START LAUNCH / SLOW WEB CALL (slow but FINE): pixels painting → in-progress → keep waiting.
        assert!(settling_active(false, 40, NOISE), "a window painting / page loading is in-progress");
        // GENUINELY HUNG (stalled): a11y stable AND pixels quiet → NOT active → settle, then the
        // downstream no-effect path escalates (we don't wait forever on the inner control).
        assert!(!settling_active(false, 0, NOISE), "a frozen screen is settled, not in-progress");
        // A11Y CHANGING (focus/label churn): in-progress regardless of pixels.
        assert!(settling_active(true, 0, NOISE), "a11y churn is in-progress");
        // AMBIENT NOISE (cursor blink / clock tick = a cell or two): below the floor → NOT active,
        // so an idle screen with a blinking cursor still reads as settled (no infinite wait).
        assert!(!settling_active(false, 1, NOISE), "ambient 1-cell blink is not activity");
        assert!(settling_active(false, 3, NOISE), "above the noise floor is activity");
    }
}

// ── Distillation tests ────────────────────────────────────────────

#[cfg(test)]
mod distill_tests {
    use super::*;

    #[test]
    fn parse_clean_json() {
        let text = r#"{"name": "open_browser", "description": "user wants to open the web browser", "approach": "Click the browser icon in the taskbar."}"#;
        let skill = parse_skill_json(text, "open browser").unwrap();
        assert_eq!(skill.name, "open_browser");
        assert!(skill.approach.contains("taskbar"));
        assert_eq!(skill.description, "user wants to open the web browser");
    }

    #[test]
    fn parse_fenced_json() {
        let text = "```json\n{\"name\": \"copy_file\", \"description\": \"copy a file to another directory\", \"approach\": \"Use file manager copy-paste.\"}\n```";
        let skill = parse_skill_json(text, "copy file").unwrap();
        assert_eq!(skill.name, "copy_file");
    }

    #[test]
    fn parse_empty_description_falls_back_to_goal() {
        let text = r#"{"name": "run_test", "description": "", "approach": "Use cargo test from the project root."}"#;
        let skill = parse_skill_json(text, "run the tests").unwrap();
        assert_eq!(skill.description, "run the tests");
    }

    #[test]
    fn parse_rejects_empty_name() {
        let text = r#"{"name": "", "description": "some desc", "approach": "some approach"}"#;
        assert!(parse_skill_json(text, "goal").is_none());
    }

    #[test]
    fn parse_rejects_empty_approach() {
        let text = r#"{"name": "do_thing", "description": "some desc", "approach": ""}"#;
        assert!(parse_skill_json(text, "goal").is_none());
    }

    #[test]
    fn parse_rejects_invalid_json() {
        assert!(parse_skill_json("not json at all", "goal").is_none());
    }

    #[test]
    fn parse_json_with_surrounding_text() {
        let text = r#"Here is the skill: {"name": "resize_window", "description": "user needs to resize an application window", "approach": "Drag the window edge."} Done."#;
        let skill = parse_skill_json(text, "resize window").unwrap();
        assert_eq!(skill.name, "resize_window");
    }

    #[test]
    fn classify_subgoal_routes_command_steps_to_channel() {
        // Explicit command directives → the CLI channel (payload stripped of the lead).
        assert_eq!(classify_subgoal("run the command touch /tmp/x").action,
                   SubAction::Command("touch /tmp/x".to_string()));
        assert_eq!(classify_subgoal("execute the command: ls -la").action,
                   SubAction::Command("ls -la".to_string()));
        assert_eq!(classify_subgoal("$ whoami").action,
                   SubAction::Command("whoami".to_string()));
        // GUI typing and app-launch must NOT be hijacked by the command leads.
        assert_eq!(classify_subgoal("type the command: touch /tmp/x").action,
                   SubAction::Type("touch /tmp/x".to_string()));
        assert!(matches!(classify_subgoal("Launch the Terminal Emulator").action, SubAction::Click));
    }

    #[test]
    fn recursive_copy_reform_rewrites_failed_glob_cp_to_find_exec() {
        // a flat glob cp that matched nothing → the recursive find form, dir/pattern/dest preserved
        assert_eq!(
            recursive_copy_reform("cp -r /home/user/Desktop/photos/*.jpg /home/user/Desktop/cpjpg"),
            Some("find /home/user/Desktop/photos -name '*.jpg' -exec cp {} /home/user/Desktop/cpjpg/ \\;".to_string()));
        // trailing slash on dest is normalized (no double slash)
        assert_eq!(
            recursive_copy_reform("cp /a/b/*.txt /c/d/"),
            Some("find /a/b -name '*.txt' -exec cp {} /c/d/ \\;".to_string()));
        // not a glob cp → no reform (leave single-file/non-cp commands alone)
        assert_eq!(recursive_copy_reform("cp /a/b/x.jpg /c/d"), None);
        assert_eq!(recursive_copy_reform("mv /a/*.jpg /c"), None);
        assert_eq!(recursive_copy_reform("cp a b c d"), None); // too many operands
    }

    #[test]
    fn capability_expressible_routes_recursive_and_glob_to_planned_command() {
        // single-source file ops → the typed-capability loop (unchanged)
        assert!(capability_expressible("copy the report to the Documents folder"));
        assert!(capability_expressible("rename notes.txt to old.txt")); // an extension but no "file"/glob → still typed
        assert!(capability_expressible("move the budget into Archive"));
        // recursive / glob / extension-pattern multi-source → NOT a single typed verb → planned-command path
        assert!(!capability_expressible(
            "Recursively go through the folders of the 'photos' directory and copy any .jpg files into 'cpjpg'"));
        assert!(!capability_expressible("copy all *.txt files into backup"));
        assert!(!capability_expressible("delete every .log file under logs"));
        // the existing undeclared-op-class routing still holds
        assert!(!capability_expressible("git init /tmp/repo"));
    }

    #[test]
    fn classify_task_no_focus_never_flips_stress_goals_to_in_app() {
        // REGRESSION GUARD (no VM): with NO focused app, none of the proven file/CLI stress goals may
        // classify as InAppSemantic (which would divert them to the API plane). App-awareness must be
        // strictly ADDITIVE — it only activates on a real spreadsheet focus.
        use crate::plane::{classify_task, Findings, TaskKind};
        let none = Findings::default();
        for g in [
            "create a directory /tmp/proj and an empty file notes.txt inside it",
            "delete the file /tmp/old.log",
            "create a git repository in /tmp/repo",
            "Recursively go through the folders of the 'photos' directory and copy any .jpg files into 'cpjpg'",
            "turn up to the max volume",
            "set the terminal size permanently",
        ] {
            assert_ne!(classify_task(g, &none), TaskKind::InAppSemantic, "flipped: {g}");
        }
    }

    #[test]
    fn shell_command_is_not_split_and_bare_command_classifies() {
        // ROOT CAUSE: a "run the command X ; Y" must stay ONE compound shell command (`;` is shell
        // syntax), not be split into a stranded "rm x" Click step.
        assert_eq!(decompose_goal("run the command touch /tmp/a ; rm /tmp/a").len(), 1);
        // But "then" still separates two agent-level command directives.
        assert_eq!(decompose_goal("run the command touch a then run the command touch b").len(), 2);
        // A bare shell command (lost its lead) with a concrete path/flag → Command, not Click.
        assert_eq!(classify_subgoal("rm /tmp/x").action, SubAction::Command("rm /tmp/x".to_string()));
        assert_eq!(classify_subgoal("find /home -name foo").action, SubAction::Command("find /home -name foo".to_string()));
        // Conservative: NL phrasing and GUI actions stay a Click (no concrete shell arg / not a tool).
        assert!(matches!(classify_subgoal("find my documents").action, SubAction::Click));
        assert!(matches!(classify_subgoal("open the file manager").action, SubAction::Click));
        assert!(matches!(classify_subgoal("echo your thoughts").action, SubAction::Click));
    }

    // ── REASSESS (reapproach) adversarial tests — the pure core, deterministically injected ─────────
    use std::collections::HashSet;

    #[test]
    fn diagnose_and_parse_exit() {
        assert_eq!(parse_exit_code("[exit 0]"), Some(0));
        assert_eq!(parse_exit_code("[exit 127]\nbash: foo: command not found"), Some(127));
        assert_eq!(parse_exit_code("no marker here"), None);
        assert_eq!(diagnose_command("[exit 127]\n[stderr] bash: gnome-terminal: command not found"), CommandFailure::CommandNotFound);
        assert_eq!(diagnose_command("[exit 2]\n[stderr] ls: cannot access '/x': No such file or directory"), CommandFailure::NoSuchPath);
        assert_eq!(diagnose_command("[exit 126]\n[stderr] Permission denied"), CommandFailure::PermissionDenied);
        assert_eq!(diagnose_command("[exit 1]\n[stderr] some other error"), CommandFailure::Other);
    }

    #[test]
    fn decide_reapproach_table() {
        let empty: HashSet<String> = HashSet::new();
        // exit 0 → Advance (the success path)...
        assert_eq!(decide_reapproach("[exit 0]\nok", Some("anything"), &empty, 2), ReapproachAction::Advance);
        // ⚠ RESIDUAL: exit 0 ADVANCES even when the effect was wrong — the core cannot tell. Documented.
        assert_eq!(decide_reapproach("[exit 0]", None, &empty, 2), ReapproachAction::Advance);
        // PermissionDenied → escalate fast, never reform (agent can't sudo).
        assert!(matches!(decide_reapproach("[exit 126]\nPermission denied", Some("sudo x"), &empty, 2), ReapproachAction::Escalate(_)));
        // Fresh candidate within budget → Retry it.
        assert_eq!(decide_reapproach("[exit 127]\ncommand not found", Some("xfce4-terminal"), &empty, 2),
                   ReapproachAction::Retry("xfce4-terminal".to_string()));
        // GIVE_UP / empty / None → escalate.
        assert!(matches!(decide_reapproach("[exit 1]\nx", Some("GIVE_UP"), &empty, 2), ReapproachAction::Escalate(_)));
        assert!(matches!(decide_reapproach("[exit 1]\nx", None, &empty, 2), ReapproachAction::Escalate(_)));
        // Oscillation: candidate is a command already tried → escalate, do NOT loop back to it.
        let mut tried = HashSet::new(); tried.insert("badA".to_string());
        assert!(matches!(decide_reapproach("[exit 1]\nx", Some("badA"), &tried, 2), ReapproachAction::Escalate(_)));
        // Budget exhausted (tried beyond limit) → escalate even with a fresh candidate.
        let mut full = HashSet::new(); for c in ["c0","c1","c2"] { full.insert(c.to_string()); }
        assert!(matches!(decide_reapproach("[exit 1]\nx", Some("c3"), &full, 2), ReapproachAction::Escalate(_)));
    }

    // Drive the reapproach control flow EXACTLY as the Command branch does, with injected mock
    // reform/run/confirm closures — so failure, reform, AND world-state postcondition are deterministic.
    fn simulate(initial: &str, limit: usize,
                mut reform: impl FnMut(&str) -> Option<String>,
                mut run: impl FnMut(&str) -> String,
                mut confirm: impl FnMut(&str) -> bool) -> (usize, ReapproachAction) {
        let mut cmd = initial.to_string();
        let mut tried: HashSet<String> = HashSet::new();
        tried.insert(initial.to_string());
        let mut iters = 0usize;
        loop {
            iters += 1;
            assert!(iters <= 100, "INFINITE LOOP — reapproach failed to terminate");
            let output = run(&cmd);
            // World-state postcondition: a clean exit whose effect is ABSENT must NOT advance.
            if parse_exit_code(&output) == Some(0) && !confirm(&cmd) {
                return (iters, ReapproachAction::Escalate("postcondition failed".into()));
            }
            let candidate = if parse_exit_code(&output) != Some(0)
                && should_reform(diagnose_command(&output)) && tried.len() <= limit {
                reform(&cmd)
            } else { None };
            match decide_reapproach(&output, candidate.as_deref(), &tried, limit) {
                ReapproachAction::Advance => return (iters, ReapproachAction::Advance),
                ReapproachAction::Retry(next) => { tried.insert(next.clone()); cmd = next; }
                e @ ReapproachAction::Escalate(_) => return (iters, e),
            }
        }
    }

    #[test]
    fn command_postcondition_derivation() {
        assert_eq!(command_postcondition("touch /tmp/x"), Some("test -e /tmp/x".into()));
        assert_eq!(command_postcondition("mkdir -p /tmp/d"), Some("test -d /tmp/d".into()));
        assert_eq!(command_postcondition("rm -f /tmp/x"), Some("test ! -e /tmp/x".into()));
        assert_eq!(command_postcondition("cp a /tmp/b"), Some("test -e /tmp/b".into()));
        assert_eq!(command_postcondition("mv a /tmp/b"), Some("test -e /tmp/b".into()));
        assert_eq!(command_postcondition("echo hi > /tmp/x"), Some("test -e /tmp/x".into()));
        // queries / compute have no checkable file-state → exit code is the only available signal.
        assert_eq!(command_postcondition("lsof -i :8080"), None);
        assert_eq!(command_postcondition("df -h"), None);
        assert_eq!(command_postcondition("python3 --version"), None);
    }

    #[test]
    fn goal_postcondition_derivation() {
        // Create intents → the artifacts must EXIST (catches plan incompleteness: a path that never got a step).
        assert_eq!(goal_postconditions("create two empty files: /tmp/a and /tmp/b"),
                   vec!["test -e /tmp/a", "test -e /tmp/b"]);
        assert_eq!(goal_postconditions("make a directory /tmp/project"), vec!["test -d /tmp/project"]);
        // Delete intent → the artifact must be GONE.
        assert_eq!(goal_postconditions("delete the file /tmp/old.log"), vec!["test ! -e /tmp/old.log"]);
        // Ambiguous source→target (move/copy) → skip (which path "counts" is unclear).
        assert!(goal_postconditions("move /tmp/a to /tmp/b").is_empty());
        // Not a create/delete intent, or no path → nothing to check.
        assert!(goal_postconditions("show the contents of /etc/hosts").is_empty());
        assert!(goal_postconditions("what is using port 8080").is_empty());
        assert!(goal_postconditions("create a poem about the sea").is_empty()); // create, but no path
        // GUI app-launch → no DESKTOP-AGNOSTIC shell check exists (a role maps to a different binary on
        // every DE), so the PURE goal-parse returns empty; launch completion is confirmed by the
        // perception/effect layer (a new top-level window appeared), not a hardcoded binary table.
        assert!(goal_postconditions("open the web browser").is_empty());
        assert!(goal_postconditions("open the file manager and the terminal emulator").is_empty());
        assert!(goal_postconditions("open the pod bay doors").is_empty());
        // Git: STRONG check or it lies — a bare dir passes `test -e`, so verify `.git` (or a HEAD when
        // a commit is asked for). This is the false-success the benchmark caught.
        assert_eq!(goal_postconditions("create a git repository in /tmp/osw_repo"),
                   vec!["test -d /tmp/osw_repo/.git"]);
        assert_eq!(goal_postconditions("create a git repository in /tmp/r2, add notes.txt, and make a commit"),
                   vec!["git -C /tmp/r2 rev-parse HEAD"]);
    }

    #[test]
    fn postcondition_catches_exit_0_but_wrong() {
        // Command exits 0 EVERY time but its effect never materializes (confirm=false) → must NOT
        // advance — the exit-0-but-wrong case the advisor flagged. Escalates instead of false-success.
        let (iters, action) = simulate("touch /tmp/x", 2,
            |_| None, |_| "[exit 0]".into(), |_| false);
        assert!(matches!(action, ReapproachAction::Escalate(_)), "exit-0-but-wrong must NOT advance");
        assert_eq!(iters, 1);
        // Sanity: exit 0 WITH the effect present → advance.
        let (_, ok) = simulate("touch /tmp/x", 2, |_| None, |_| "[exit 0]".into(), |_| true);
        assert_eq!(ok, ReapproachAction::Advance);
    }

    #[test]
    fn reapproach_recovers_a_fixable_failure() {
        // initial fails (command not found); reform yields a good command that runs clean.
        let (iters, action) = simulate("gnome-terminal", 2,
            |_| Some("xfce4-terminal".to_string()),
            |c| if c == "xfce4-terminal" { "[exit 0]".into() } else { "[exit 127]\ncommand not found".into() },
            |_| true);
        assert_eq!(action, ReapproachAction::Advance);
        assert_eq!(iters, 2); // initial fail + one reformed success
    }

    #[test]
    fn reapproach_bounds_an_unfixable_failure() {
        // run ALWAYS fails; reform ALWAYS produces a fresh (still-bad) command → must escalate, bounded.
        let mut n = 0;
        let (iters, action) = simulate("bad0", 2,
            |_| { n += 1; Some(format!("bad{n}")) },
            |_| "[exit 1]\nstill broken".into(),
            |_| true);
        assert!(matches!(action, ReapproachAction::Escalate(_)));
        assert_eq!(iters, 3, "must escalate within REFORM_LIMIT+1 executions, not loop");
    }

    #[test]
    fn reform_must_be_conservative() {
        // A correction (binary swap, same shape) is allowed.
        assert!(reform_is_conservative("tuch /tmp/x", "touch /tmp/x"));
        assert!(reform_is_conservative("lst -la /tmp", "ls -la /tmp"));
        assert!(reform_is_conservative("grep x | wc", "grep y | wc")); // original already had the pipe
        // Introducing chaining / redirection / exec / substitution the original lacked is REJECTED —
        // these are the exact garbage shapes the live 8B produced and they have side effects.
        assert!(!reform_is_conservative("cat x", "mkdir -p x && cat x"));
        assert!(!reform_is_conservative("cat x", "cat x; exec bash -c 'cat x'"));
        assert!(!reform_is_conservative("echo hi", "echo hi > /tmp/f"));
        assert!(!reform_is_conservative("ls", "ls $(rm -rf /tmp/z)"));
    }

    #[test]
    fn equivalence_classes_resolve() {
        assert!(equivalence_alternatives("python").contains(&"python3"));
        assert!(equivalence_alternatives("gnome-terminal").contains(&"xfce4-terminal"));
        assert_eq!(equivalence_alternatives("md5"), vec!["md5sum"]);
        assert!(equivalence_alternatives("python3").contains(&"python")); // symmetric within the class
        assert!(equivalence_alternatives("definitely_not_a_program").is_empty()); // no class → LLM fallback
    }

    #[test]
    fn reapproach_stops_on_oscillation() {
        // reform flip-flops between two already-tried bad commands → no-repeat guard escalates fast.
        let (iters, action) = simulate("A", 5,
            |c| Some(if c == "A" { "B".to_string() } else { "A".to_string() }),
            |_| "[exit 1]\nbroken".into(),
            |_| true);
        assert!(matches!(action, ReapproachAction::Escalate(_)));
        assert!(iters <= 3, "oscillation must be caught fast, got {iters} iters");
    }

    #[test]
    fn write_file_primitive_authors_robustly() {
        assert_eq!(parse_write_file("write to /tmp/run.sh: #!/bin/sh\\necho hi"),
                   Some(("/tmp/run.sh".to_string(), "#!/bin/sh\\necho hi".to_string())));
        assert_eq!(parse_write_file("write the file /tmp/x: content"),
                   Some(("/tmp/x".to_string(), "content".to_string())));
        // "write the text X into Y" (no colon, not a write-to lead) is NOT the primitive → plain echo.
        assert_eq!(parse_write_file("write the text hello into the file /tmp/y"), None);
        // classify_subgoal builds a robust base64 write that mkdir -p's the parent; decoded content
        // interprets \n. (The /tmp/myapp/main.py failure: a nested path needs its dir created first.)
        match classify_subgoal("write to /tmp/proj/run.sh: #!/bin/sh\\ntouch /tmp/out").action {
            SubAction::Command(cmd) => {
                assert!(cmd.starts_with("mkdir -p /tmp/proj &&"), "must create parent dir: {cmd}");
                assert!(cmd.contains("base64 -d > /tmp/proj/run.sh"), "got: {cmd}");
                use base64::Engine;
                let b64 = cmd.split("echo ").nth(1).unwrap().split_whitespace().next().unwrap();
                let decoded = String::from_utf8(
                    base64::engine::general_purpose::STANDARD.decode(b64).unwrap()).unwrap();
                assert_eq!(decoded, "#!/bin/sh\ntouch /tmp/out"); // \n became a real newline
            }
            other => panic!("expected Command, got {other:?}"),
        }
    }

    #[test]
    fn command_would_hang_rejects_sudo_and_interactive() {
        // These hang the TTY-less command channel → the planner must reject a plan containing them.
        for s in ["run the command sudo apt-get clean", "run the command nano /tmp/x",
                  "run the command vim foo", "run the command top", "run the command less /var/log/syslog",
                  "run the command python"] {
            assert!(command_would_hang(s), "{s:?} should be flagged as hang-prone");
        }
        // Normal commands and non-command steps are fine.
        for s in ["run the command touch /tmp/x", "run the command ls -la", "run the command df -h",
                  "Click the Applications menu"] {
            assert!(!command_would_hang(s), "{s:?} should NOT be flagged");
        }
    }
}
