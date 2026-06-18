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

/// How many Board priors to surface per goal. A retrieval-tuning constant (cf. RAG K=15),
/// not a model/hardware value — invariant #9 does not apply.
const BOARD_TOP_K: usize = 8;

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
    actuator: &dyn Actuator,
    perceptor: &dyn Perceptor,
    memory_tiers: &Arc<tokio::sync::Mutex<crate::memory_tiers::MemoryTiers>>,
) -> String {
    match call {
        ToolCall::Click { selector } => actuator.click(selector),
        ToolCall::Type { selector, text } => actuator.type_text(selector, text),
        ToolCall::Key { key } => actuator.key(key),
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
    actuator: &dyn Actuator,
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
        "vm_command" => actuator.click(&format!("cmd:{}", s("command"))),
        "vm_type"    => actuator.type_text("focused", &s("text")),
        "vm_click"   => actuator.click(&s("selector")),

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
    actuator: &dyn Actuator,
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
    let mut parts: Vec<String> = vec![goal.trim().to_string()];
    loop {
        let mut next = Vec::new();
        let mut split_any = false;
        for p in &parts {
            let lower = p.to_lowercase();
            // earliest marker position across all markers
            let cut = MARKERS.iter().filter_map(|m| lower.find(m).map(|i| (i, m.len()))).min_by_key(|(i, _)| *i);
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
pub fn classify_subgoal(s: &str) -> SubGoal {
    let t = s.trim();
    let lower = t.to_lowercase();
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
    SubGoal { text: t.to_string(), action: SubAction::Click }
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
fn plan_goal(goal: &str, skills: &[Skill], adapter: &Arc<dyn InferenceAdapter>) -> Vec<String> {
    let syntactic = decompose_goal(goal);
    if syntactic.len() > 1 {
        return syntactic; // explicit multi-step → trust the deterministic split
    }
    let skill_block = SkillLibrary::format_for_prompt(skills);
    let prompt = format!(
"Break the goal into the FEWEST on-screen click steps, one per line.
Rules:
- Each step is exactly: Click <the element to click>. Nothing else.
- To launch or open an application: TWO steps — Click the Applications menu, then Click <the app>.
- One click per step. Do NOT add steps like locate, find, wait, open the folder, verify, or double-click.
- If the target is already a visible item, output ONE step.

Example:
Goal: Launch the Web Browser
Steps:
Click the Applications menu
Click Web Browser
{skill_block}
Goal: {goal}
Steps:"
    );
    // Lead verbs that are not a discrete click target — the planner sometimes emits narration
    // ("Locate X", "Wait for Y") that has no on-screen element and would only fail-close.
    const NON_ACTION_LEAD: &[&str] =
        &["wait", "locate", "find", "ensure", "verify", "confirm", "observe", "check", "look", "see"];
    match adapter.generate(&prompt, 128, 0.1) {
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
            // Adopt only a genuine expansion; a 0/1-line reply adds nothing and risks drift.
            if steps.len() > 1 {
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

/// Read the a11y screen, polling until it SETTLES — two consecutive reads with the same structural
/// signature (label-set + focus) — or a bounded poll ceiling. An action's effect may still be
/// painting (a window opening, a menu animating), and deciding on a mid-transition world is what
/// re-fired `term-type` and re-perceived the menu mid-open. This establishes a SETTLED before/after
/// state so the advance comparison is reliable and direction is read from the structure, not a
/// hardcoded open-vs-close rule. Returns the latest read regardless (ceiling = proceed, don't hang).
async fn read_settled_screen(perceptor: &dyn Perceptor) -> String {
    const STABILITY_INTERVAL_MS: u64 = 200;
    const MAX_POLLS: usize = 15; // ~3s ceiling; returns immediately once two reads agree
    let mut prev = perceptor.read_screen();
    for _ in 0..MAX_POLLS {
        tokio::time::sleep(tokio::time::Duration::from_millis(STABILITY_INTERVAL_MS)).await;
        let curr = perceptor.read_screen();
        if !structural_change(&prev, &curr) {
            return curr; // two consecutive reads agree → settled world
        }
        prev = curr;
    }
    prev // still churning at the ceiling — proceed with the latest rather than hang
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
    let state_hash = {
        let s = perceptor.read_screen();
        format!("{}", blake3::hash(s.as_bytes()))
    };

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

    // Consecutive-identical-action cutoff state.
    // Invariant: last_exec_action / consecutive_exec_count are updated only when
    // action_executed == true; reset when a different action executes.
    let mut last_exec_action = String::new();
    let mut consecutive_exec_count: usize = 0;

    let goal = state.lock().await.goal.clone();
    let system_prompt = config::system_prompt();

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
    let sub_goals: Vec<SubGoal> = {
        let g = goal.clone();
        let ad = adapter.clone();
        let sl = skill_library.clone();
        let strings = tokio::task::spawn_blocking(move || {
            let skills = sl.retrieve(&g, 3);
            plan_goal(&g, &skills, &ad)
        })
        .await
        .unwrap_or_else(|_| decompose_goal(&goal));
        // Classify each planned step into Click / Type / Key (Wall 2). Click steps run the selection
        // loop; Type/Key are deterministic.
        strings.iter().map(|s| classify_subgoal(s)).collect()
    };
    let mut current_sub: usize = 0;
    // DEVIATION DETECTION (§2.15): consecutive re-perceptions where NOTHING on screen matches the
    // current sub-goal. The deterministic plan is BLIND and cannot re-plan, so when the world goes
    // off-plan (error dialog, permission prompt, an already-done/ambiguous state) the safe move is
    // a clean handback, not looping or marching a dead plan. Reset on any progress; escalate at the
    // threshold. This also SAFELY subsumes "already-satisfied" — rather than risk a wrong auto-skip,
    // we hand back and let the human say "that's done, skip it."
    let mut subgoal_stuck: usize = 0;
    const SUBGOAL_STUCK_LIMIT: usize = 4;
    if sub_goals.len() > 1 {
        chronos::log(&format!("sequencer: {} sub-goals: {:?}", sub_goals.len(), sub_goals));
    }

    // Priors slice — the Board. Park-scored top-k (relevance × recency × importance) from
    // the ColBERT embedder when it's up AND the board has embedded rows; deterministic
    // recency floor (`assemble_context`) otherwise. The spine: a model-upgrade layer over a
    // floor that always works. embed() is blocking HTTP → run off the lock via spawn_blocking
    // (mutex-guard discipline: no await is held under the guard).
    // Reserved for the v2 upstream planner (the executor is memory-isolated, §2.5). Computed but
    // not injected here. TODO(v1-cleanup): skip these computations until the planner consumes them.
    let _episodic_context = {
        let goal_for_embed = goal.clone();
        let qvec = tokio::task::spawn_blocking(move || crate::embedding::embed(&goal_for_embed).ok())
            .await
            .ok()
            .flatten();
        let tiers = memory_tiers.lock().await;
        match qvec {
            Some(q) if !q.is_empty() => {
                let slice = tiers.assemble_slice(&q, BOARD_TOP_K, &crate::board::ParkWeights::default());
                if slice.is_empty() {
                    chronos::log("board: empty slice — recency floor");
                    tiers.assemble_context(2048)
                } else {
                    chronos::log(&format!("board: {} priors via Park slice", slice.len()));
                    slice.iter().map(|e| format!("- {}", e.text)).collect::<Vec<_>>().join("\n")
                }
            }
            _ => {
                chronos::log("board: embedder down — recency floor");
                tiers.assemble_context(2048)
            }
        }
    };

    // Visual similarity context: encode current frame → find top-3 most visually
    // Visual similarity context: encode current frame → find top-3 past episodes with
    // similar visual context. Runs once per invocation. No-op when encoder absent.
    let _visual_context: String = {
        match (&visual_encoder, std::fs::read(crate::config::FRAME_PATH)) {
            (Some(enc), Ok(png)) => {
                let enc2 = enc.clone();
                let embd = tokio::task::spawn_blocking(move || enc2.encode_png(&png))
                    .await
                    .unwrap_or(None);
                if let Some(embd) = embd {
                    let tiers = memory_tiers.lock().await;
                    let similar = tiers.find_similar_by_embedding(&embd, 3);
                    drop(tiers);
                    similar.join("\n- ")
                } else {
                    String::new()
                }
            }
            _ => String::new(),
        }
    };

    // Retrieve relevant skills as advisory depth context — top-3 by Jaccard on goal text.
    // These are injected into the prompt as guidance, never executed verbatim.
    let _skill_context: String = {
        let skills = skill_library.retrieve(&goal, 3);
        SkillLibrary::format_for_prompt(&skills)
    };

    chronos::log(&format!("goal_received: {goal}"));
    let _ = confirm_tx.send(envelope::make("status", envelope::StatusPayload {
        state: "goal_received".to_string(),
        detail: goal.clone(),
    })).await;

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
            read_settled_screen(perceptor.as_ref()).await
        } else {
            perceptor.read_screen()
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
        if had_prior_step {
            let screen_changed = !prev_screen.is_empty()
                && blake3::hash(screen.as_bytes()) != blake3::hash(prev_screen.as_bytes());
            let outcome = classify_step_outcome(prev_action_executed, screen_changed);
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
                    let _ = confirm_tx.send(envelope::make("action_log", envelope::ActionLogPayload {
                        text: msg.to_string(),
                    })).await;
                    let _ = confirm_tx.send(envelope::make("status", envelope::StatusPayload {
                        state: "goal_done".to_string(),
                        detail: msg.to_string(),
                    })).await;
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
            // v1 advance signal = structural screen change; the per-action-class effect SIGNATURE +
            // precondition (already-satisfied → advance without acting) are the §2.15 refinement.
            // Advance on a STRUCTURAL effect (element-set / focus change), not a raw screen hash —
            // a tooltip or ambient pixel change must not advance the plan (§2.15 failures 1+3).
            if prev_action_executed && structural_change(&prev_screen, &screen) {
                current_sub += 1;
                subgoal_stuck = 0; // fresh sub-goal — reset the deviation counter
                if current_sub >= sub_goals.len() {
                    chronos::log("sequencer_complete: all sub-goals done");
                    let _ = confirm_tx.send(envelope::make("action_log", envelope::ActionLogPayload {
                        text: "Goal accomplished — all steps completed.".to_string(),
                    })).await;
                    let _ = confirm_tx.send(envelope::make("status", envelope::StatusPayload {
                        state: "goal_done".to_string(),
                        detail: "Goal accomplished — all steps completed.".to_string(),
                    })).await;
                    break;
                }
                chronos::log(&format!("sequencer_advance: → sub-goal {}/{}: {}",
                    current_sub + 1, sub_goals.len(), sub_goals[current_sub].text));
            }
        }
        had_prior_step = true;

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
            };
            // confidence = 1.0: the harness chose this deterministically, not the model.
            let output = match gate::confidence_escalate(gate::evaluate_action(&tool_call, &registry), 1.0) {
                gate::Verdict::Allow => {
                    let desc = gate::describe_redacted(&tool_call);
                    let out = execute_tool(&tool_call, actuator.as_ref(), perceptor.as_ref(), &memory_tiers).await;
                    chronos::log(&format!("action(deterministic): {desc} -> {out}"));
                    let _ = confirm_tx.send(envelope::make("action_log", envelope::ActionLogPayload {
                        text: format!("{desc} -> {out}"),
                    })).await;
                    out
                }
                gate::Verdict::ConfirmTap => request_and_await_approval("tap", &tool_call, &state, actuator.as_ref(), perceptor.as_ref(), &memory_tiers, &mut approval_rx, &confirm_tx).await,
                gate::Verdict::ConfirmTyped => request_and_await_approval("typed", &tool_call, &state, actuator.as_ref(), perceptor.as_ref(), &memory_tiers, &mut approval_rx, &confirm_tx).await,
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
            prev_action_executed = false; // deterministic advance below — don't double-count at loop top
            prev_screen = read_settled_screen(perceptor.as_ref()).await;
            if current_sub >= sub_goals.len() {
                chronos::log("sequencer_complete: all sub-goals done (deterministic final step)");
                let _ = confirm_tx.send(envelope::make("action_log", envelope::ActionLogPayload {
                    text: "Goal accomplished — all steps completed.".to_string(),
                })).await;
                let _ = confirm_tx.send(envelope::make("status", envelope::StatusPayload {
                    state: "goal_done".to_string(), detail: "Goal accomplished — all steps completed.".to_string(),
                })).await;
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
        // Gated by LAGADO_CV_DISABLE (kill-switch + the Phase 1c pick-rate measurement knob).
        let cv_boxes: Vec<crate::perception::cv_proposer::ScreenBox> =
            if !crate::config::cv_enabled() {
                vec![]
            } else {
                match std::fs::read(crate::config::FRAME_PATH) {
                    Ok(png) => match image::load_from_memory(&png) {
                        Ok(img) => {
                            let rgb = img.to_rgb8();
                            let (w, h) = (rgb.width(), rgb.height());
                            crate::perception::cv_proposer::propose_frame(rgb.as_raw(), w, h)
                        }
                        Err(e) => { chronos::log(&format!("cv: frame decode failed ({e}) — a11y-only")); vec![] }
                    },
                    Err(e) => { chronos::log(&format!("cv: no frame ({e}) — a11y-only")); vec![] }
                }
            };
        // Labels flow THROUGH the arbiter (provenance: a11y > caption > OCR > None); CV
        // boxes carry no text, so they enter unlabeled but selectable. Vision stays []
        // until Phase 2.
        let fused = crate::perception::arbiter::fuse(&bboxes, &labels, &cv_boxes, &[]);
        let candidates = crate::perception::selection::build_candidates(&fused);
        chronos::log(&format!("perceive: {} a11y + {} cv → {} fused", bboxes.len(), cv_boxes.len(), fused.len()));
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
                let msg = format!(
                    "The screen doesn't match what this step needs (\"{active_goal}\") — handing back to you. \
                     It may already be done, or the screen went somewhere I didn't plan for."
                );
                chronos::log(&format!("deviation_escalate: stuck on sub-goal after {subgoal_stuck} re-perceptions"));
                let _ = confirm_tx.send(envelope::make("action_log", envelope::ActionLogPayload {
                    text: msg.clone(),
                })).await;
                let _ = confirm_tx.send(envelope::make("status", envelope::StatusPayload {
                    state: "goal_done".to_string(),
                    detail: msg,
                })).await;
                break;
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
                                let _ = confirm_tx.send(envelope::make("action_log", envelope::ActionLogPayload { text: msg.clone() })).await;
                                let _ = confirm_tx.send(envelope::make("status", envelope::StatusPayload {
                                    state: "goal_done".to_string(), detail: msg,
                                })).await;
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
                let output = match gate::confidence_escalate(base_verdict, confidence) {
                    gate::Verdict::Allow => {
                        let desc = gate::describe_redacted(&tool_call);
                        let out = execute_tool(&tool_call, actuator.as_ref(), perceptor.as_ref(), &memory_tiers).await;
                        chronos::log(&format!("action: {desc} -> {out}"));
                        let _ = confirm_tx.send(envelope::make("action_log", envelope::ActionLogPayload {
                            text: format!("{desc} -> {out}"),
                        })).await;
                        out
                    }
                    gate::Verdict::ConfirmTap => {
                        request_and_await_approval("tap", &tool_call, &state, actuator.as_ref(), perceptor.as_ref(), &memory_tiers, &mut approval_rx, &confirm_tx).await
                    }
                    gate::Verdict::ConfirmTyped => {
                        request_and_await_approval("typed", &tool_call, &state, actuator.as_ref(), perceptor.as_ref(), &memory_tiers, &mut approval_rx, &confirm_tx).await
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
                        let s = perceptor.read_screen();
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
                        let s = perceptor.read_screen();
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
}
