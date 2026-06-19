//! hydra.rs — Dual-model orchestrator (Phase 1: single-model fallback)
//!
//! Routes user intent to the correct execution path:
//!   CHAT        → conversational inference, no tool loop
//!   INTERACTIVE → agent_loop (bracket tool calls + HITL gate)
//!   REASONING   → agent_loop with enhanced reasoning context
//!
//! CLEAN-CONTEXT DISCIPLINE: classify_intent() receives ONLY the current
//! user message — zero conversation history, zero screen data. This is
//! non-negotiable; history poisoning causes 78%→8% accuracy collapse.
//! Phase 2 will route classify_intent() to a dedicated 350M model.

use std::sync::Arc;
use tokio::sync::mpsc;
use crate::action_graph::ActionGraph;
use crate::inference::{InferenceAdapter, llama_cpp::LlamaCppAdapter};
use crate::perception::{Perceptor, Actuator};
use crate::retrieval::Retriever;
use crate::skill_library::SkillLibrary;
use crate::{agent, config, envelope, governor, tools};
use blake3;

/// Intent classification result
#[derive(Debug, Clone, PartialEq)]
pub enum Intent {
    Chat,
    Interactive,
    Reasoning,
}

/// Capability tier — determines context budget for assembled memory
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Capability {
    Low,   // ≤8GB RAM, no GPU — small context, simple actions only
    Mid,   // 8–16GB RAM or low GPU — medium context
    High,  // >16GB RAM or good GPU — full context, all features
}

/// Hydra configuration derived from system capabilities
pub struct HydraConfig {
    /// Capability tier from governor — determines context budget
    pub capability: Capability,
}

/// Dual-model orchestrator
pub struct Hydra {
    pub adapter: Arc<dyn InferenceAdapter>,
    /// 350M classifier (or falls back to adapter if classifier server isn't running)
    pub classifier: Arc<dyn InferenceAdapter + Send + Sync>,
    pub config: HydraConfig,
}

impl Hydra {
    /// Initialize Hydra from system governor detection.
    /// Checks port 8081 for a live classifier server — uses it if up, else falls back to adapter.
    pub fn from_governor(adapter: Arc<dyn InferenceAdapter + Send + Sync>) -> Self {
        let model_bytes = std::fs::metadata(config::model_path()).map(|m| m.len()).unwrap_or(0);
        let server_config = governor::detect_and_plan(config::CONTEXT_SIZE, model_bytes);

        let capability = if server_config.n_gpu_layers > 0 && server_config.ctx >= 16384 {
            Capability::High
        } else if server_config.n_gpu_layers > 0 || server_config.ctx >= 8192 {
            Capability::Mid
        } else {
            Capability::Low
        };

        // Try dedicated 350M classifier on port 8081 — ECONNREFUSED is immediate
        let classifier_url = config::classifier_base_url();
        let classifier: Arc<dyn InferenceAdapter + Send + Sync> =
            if ureq::get(&format!("{}/health", classifier_url))
                .timeout(std::time::Duration::from_millis(500))
                .call()
                .is_ok()
            {
                tracing::info!("Using 350M classifier on {classifier_url}");
                Arc::new(LlamaCppAdapter::with_url(
                    &classifier_url,
                    config::CLASSIFIER_MODEL_FILE,
                    config::CLASSIFIER_CONTEXT_SIZE,
                ))
            } else {
                tracing::debug!("Classifier server not available — main model handles classification");
                adapter.clone()
            };

        Hydra {
            adapter,
            classifier,
            config: HydraConfig { capability },
        }
    }

    /// Classify user intent on a CLEAN PROMPT (no history, current message only).
    ///
    /// CRITICAL: This function receives ONLY the current user message.
    /// Zero conversation history, zero screen data. This discipline is
    /// load-bearing and prevents history poisoning.
    ///
    /// Routes to the 350M classifier on port 8081 if available; falls back to the
    /// main 8B adapter. The parser searches the full response (not just the first
    /// word) to handle small-model preamble like "The answer is INTERACTIVE".
    pub async fn classify_intent(&self, message: &str) -> Intent {
        // DETERMINISTIC FAST-PATH: a message opening with an unambiguous UI action verb is
        // Interactive — don't ask the weak 1.2B classifier (~80%), which misroutes LONG action
        // chains (e.g. "Open the menu, then click X, then type Y, then press Enter") to CHAT. CHAT
        // is the DANGEROUS misroute: it routes to a one-shot chat_response that silently does
        // nothing while reporting success. Determinism on the rails over a vibes guess.
        if opens_with_action_verb(message) || opens_with_command_phrase(message) {
            return Intent::Interactive;
        }
        // Few-shot prompt — empirically validated on 1.2B Instruct (~80% accuracy)
        let prompt = format!(
            "Classify each message as CHAT, INTERACTIVE, or REASONING. One word only.\n\
             \n\
             Examples:\n\
             open Firefox → INTERACTIVE\n\
             hello friend → CHAT\n\
             write sorting code → REASONING\n\
             click submit button → INTERACTIVE\n\
             explain how TCP works → REASONING\n\
             what time is it → CHAT\n\
             navigate to settings → INTERACTIVE\n\
             type my password → INTERACTIVE\n\
             search for files → INTERACTIVE\n\
             close this window → INTERACTIVE\n\
             how are you today → CHAT\n\
             \n\
             Now classify:\n\
             {message} →"
        );

        // Grammar-constrained: the model can ONLY emit a valid label, eliminating the
        // silent "unparseable output → CHAT default" failure (observed: "press Escape"
        // produced "Escape" → parsed to CHAT → agent does nothing). Falls back to the
        // 8B main adapter if the classifier server is down, still grammar-constrained.
        let grammar = crate::grammar::intent_grammar();
        let result = self
            .classifier
            .generate_constrained(&prompt, 10, 0.0, &grammar)
            .or_else(|_| self.adapter.generate_constrained(&prompt, 10, 0.0, &grammar));

        match result {
            Ok((response, confidence)) => {
                // Logged so the routing eval can later calibrate a C5 "when unsure, treat
                // as planning" confidence floor — deliberately NOT a vibes threshold now.
                tracing::debug!(confidence, "intent classified (grammar-constrained)");
                parse_intent_label(&response)
            }
            Err(_) => Intent::Chat, // inference fully unavailable — graceful degrade
        }
    }

    /// Conversational inference (no tool loop)
    pub async fn chat_response(&self, message: &str, context: &str) -> String {
        let system_prompt = config::system_prompt();
        let prompt = format!(
            "{system_prompt}\n\n{context}\n\nUser: {message}\nAssistant:"
        );

        match self.adapter.generate(&prompt, 2048, 0.7) {
            Ok(response) => response,
            Err(_) => "I'm having trouble responding right now.".to_string(),
        }
    }

    /// Context budget in tokens based on capability tier
    pub fn context_budget(&self) -> usize {
        match self.config.capability {
            Capability::Low => 512,
            Capability::Mid => 2048,
            Capability::High => 8192,
        }
    }
}

/// Parse the classifier model's raw text output into an Intent.
///
/// Checks the first word first (fast path for well-behaved models), then scans
/// the whole response for the label (handles small-model preamble like
/// "The classification is INTERACTIVE"). INTERACTIVE > REASONING > CHAT priority.
/// True if the message opens with an unambiguous UI action verb → route Interactive deterministically.
/// High-PRECISION list only (verbs that are essentially always computer-use commands): "open the
/// file" is interactive; borderline verbs ("find", "search", "run") are left to the classifier so a
/// genuine question isn't force-routed. Matches a whole leading word, so "opening hours" is excluded.
pub fn opens_with_action_verb(message: &str) -> bool {
    const VERBS: &[&str] = &[
        "open", "click", "double-click", "right-click", "type", "press", "launch", "close",
        "navigate", "select", "scroll", "drag", "switch", "minimize", "maximize", "focus", "toggle",
    ];
    let m = message.trim().to_lowercase();
    VERBS.iter().any(|v| {
        m.strip_prefix(v).is_some_and(|rest| rest.is_empty() || rest.starts_with(' '))
    })
}

/// True if the message opens with an explicit shell-command directive (a command-channel phrasing).
/// Unlike the bare verb "run" (excluded above as borderline — "run a marathon" is a question),
/// "run the command …" / "$ …" is unambiguously a CLI step → route Interactive deterministically.
/// Shares its lead list with the sequencer (`agent::COMMAND_LEADS`) so routing and execution agree.
pub fn opens_with_command_phrase(message: &str) -> bool {
    let m = message.trim().to_lowercase();
    crate::agent::COMMAND_LEADS.iter().any(|&lead| m.starts_with(lead))
}

// ── State-aware routing levers (deterministic-first; the LLM router is the residual) ────────────
// Routing = f(message-shape, system-state) — NOT f(message) alone. The system state is GROUND TRUTH;
// the LLM classify is a guess, so the deterministic levers decide first and the 1.2B fires only on the
// genuinely-ambiguous remainder (latency + reliability win). State is NOT conversation history, so this
// honors invariant #2 (clean-context routing): the message is still the only prose the classifier sees.

/// What the agent can act on RIGHT NOW. `any()` false ⇒ there is no surface to operate, so an action
/// request can only be CHAT (or an offer to start one). `host_control_active` is the forward slot for
/// Segment-7 live-host mode (stubbed false until built — we model the full shape without depending on it).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SurfaceState {
    pub vm_active: bool,
    pub immersive_active: bool,
    pub host_control_active: bool,
}
impl SurfaceState {
    pub fn any(&self) -> bool { self.vm_active || self.immersive_active || self.host_control_active }
}

/// Explicit user routing mode — the real, intentional version of the old (weak, never-wired) pause flag.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RouteMode {
    /// State + message-shape + (residual) LLM decide.
    #[default]
    Auto,
    /// "Just chat — don't touch anything." Hard override → always CHAT. (Replaces `is_paused`.)
    ChatLock,
    /// "You have control — act on what's actionable." Actionable shapes act; clear questions still chat.
    ActLock,
}

/// The deterministic routing context the caller assembles from real system state + the user's mode.
#[derive(Debug, Clone, Copy, Default)]
pub struct RouteContext {
    pub surface: SurfaceState,
    pub mode: RouteMode,
}

/// A hard-lever routing decision. `None` from `deterministic_route` means "fall to the LLM classifier."
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteOutcome {
    Chat,
    Interactive,
    /// Action requested but NO surface is active → offer to start one rather than silently chat.
    Offer,
}

/// Does the message have the SHAPE of a computer action (vs a question/conversation)? Deterministic —
/// the message-shape lever. TRUE on: an explicit command phrase; a GUI action verb; a STRONG task verb
/// (install/run/kill/git… — always a computer action); OR a SOFT task verb (create/make/delete/show…)
/// paired with a COMPUTER-OBJECT (a path, a filename extension, or a system noun). The object is the
/// discriminator that separates "create a FILE" (action) from "create a POEM" (chat). Conservative:
/// a soft verb with no object is NOT action-shaped.
pub fn is_action_shaped(message: &str) -> bool {
    if opens_with_action_verb(message) || opens_with_command_phrase(message) {
        return true;
    }
    let m = message.trim().to_lowercase();
    let first = m.split_whitespace().next().unwrap_or("");
    // Verbs that are inherently a computer action regardless of object.
    const STRONG: &[&str] = &[
        "install", "uninstall", "run", "execute", "kill", "mount", "unmount", "compile", "build",
        "deploy", "reboot", "shutdown", "ping", "curl", "wget", "clone", "commit", "push", "pull",
        "chmod", "chown", "untar", "unzip", "grep",
    ];
    if STRONG.contains(&first) {
        return true;
    }
    // Verbs that are an action ONLY with a computer-object present.
    const SOFT: &[&str] = &[
        "create", "make", "delete", "remove", "show", "list", "move", "copy", "rename", "find",
        "read", "write", "edit", "append", "count", "search", "download", "extract", "compress", "print",
    ];
    if !SOFT.contains(&first) {
        return false;
    }
    let has_path = m.contains('/') || m.contains('\\') || m.contains('~');
    let has_ext = [".txt", ".md", ".log", ".json", ".sh", ".py", ".rs", ".png", ".jpg", ".pdf",
                   ".csv", ".zip", ".tar", ".gz", ".conf", ".toml", ".yaml", ".yml"]
        .iter().any(|e| m.contains(e));
    const SYS_NOUNS: &[&str] = &[
        "file", "files", "folder", "directory", "directories", "dir", "app", "application", "window",
        "terminal", "browser", "process", "port", "package", "command", "script", "disk", "memory",
        "service", "container", "repo", "repository", "branch", "commit",
    ];
    let has_sys_noun = m.split(|c: char| !c.is_alphanumeric()).any(|w| SYS_NOUNS.contains(&w));
    has_path || has_ext || has_sys_noun
}

/// Is the message clearly a QUESTION (interrogative)? Used by ActLock to keep a genuine question as chat
/// even when the user handed over control. Deterministic.
pub fn is_clear_question(message: &str) -> bool {
    let m = message.trim().to_lowercase();
    if m.ends_with('?') {
        return true;
    }
    const Q_LEADS: &[&str] = &["what", "why", "how", "who", "when", "where", "which", "is", "are",
        "do", "does", "can", "could", "would", "should", "explain", "tell me", "describe"];
    let first = m.split_whitespace().next().unwrap_or("");
    Q_LEADS.contains(&first) || Q_LEADS.iter().any(|q| q.contains(' ') && m.starts_with(q))
}

/// The deterministic routing gate. Returns `Some(outcome)` when a HARD lever decides (no LLM call);
/// `None` means "ambiguous/question with a surface in Auto mode" → fall to the LLM classifier. Pure.
pub fn deterministic_route(message: &str, ctx: &RouteContext) -> Option<RouteOutcome> {
    match ctx.mode {
        RouteMode::ChatLock => return Some(RouteOutcome::Chat), // hard: just chat
        RouteMode::ActLock => {
            return Some(if is_clear_question(message) { RouteOutcome::Chat } else { RouteOutcome::Interactive });
        }
        RouteMode::Auto => {}
    }
    if !ctx.surface.any() {
        // Nothing to act on → an action request can only be an offer to start a surface; else chat.
        return Some(if is_action_shaped(message) { RouteOutcome::Offer } else { RouteOutcome::Chat });
    }
    // Surface active + Auto: a clear action acts deterministically.
    if is_action_shaped(message) {
        return Some(RouteOutcome::Interactive);
    }
    // A clear QUESTION is conversational even with a surface active — route Chat, NEVER the action
    // planner. (A user hit this: "do you know how to write in rust?" was classified REASONING and driven
    // through the file-task planner into a bogus write-to-file plan.) An action-shaped message already
    // returned above, so this can't swallow a real command.
    if is_clear_question(message) {
        return Some(RouteOutcome::Chat);
    }
    None // genuinely ambiguous → fall to the LLM classifier
}

pub fn parse_intent_label(response: &str) -> Intent {
    let upper = response.trim().to_uppercase();

    // Fast path: first word is the label (expected from instruction-tuned models)
    if let Some(first) = upper.split_whitespace().next() {
        if first.starts_with("INTERACTIVE") { return Intent::Interactive; }
        if first.starts_with("REASONING")   { return Intent::Reasoning; }
        if first.starts_with("CHAT")        { return Intent::Chat; }
    }

    // Fallback: label appears anywhere in response (handles preamble from small models)
    if upper.contains("INTERACTIVE") { return Intent::Interactive; }
    if upper.contains("REASONING")   { return Intent::Reasoning; }

    Intent::Chat // safe default
}

/// Main entry point: route user message based on intent classification
pub async fn run(
    message: String,
    _context_hint: String,  // caller-supplied hint (unused — we assemble from tiers)
    route: RouteContext,    // deterministic routing levers: surface state + explicit user mode
    state: Arc<tokio::sync::Mutex<agent::AgentState>>,
    adapter: Arc<dyn InferenceAdapter>,
    perceptor: Arc<dyn Perceptor>,
    actuator: Arc<dyn Actuator>,
    approval_rx: mpsc::Receiver<bool>,
    confirm_tx: mpsc::Sender<String>,
    memory_tiers: Arc<tokio::sync::Mutex<crate::memory_tiers::MemoryTiers>>,
    visual_encoder: Option<Arc<crate::vision::VisualEncoder>>,
    skill_library: Arc<SkillLibrary>,
) {
    let hydra = Hydra::from_governor(adapter.clone());
    let mut reg = tools::ToolRegistry::load();
    let mcp_entries = tools::discover_mcp_tools().await;
    if !mcp_entries.is_empty() {
        reg.merge_entries(mcp_entries);
    }
    let registry = Arc::new(reg);

    // Assemble context via retriever (RAG K=15)
    let context = {
        let retriever = Retriever::new(&config::data_dir());
        let entries = retriever.retrieve_context(&message, 15);
        Retriever::format_context(&entries)
    };

    // Action graph shortcut: known high-confidence workflow → skip classification
    let graph_path = config::data_dir().join("action_graph.db");
    let screen_snap = perceptor.read_screen();
    let state_hash = format!("{}", blake3::hash(screen_snap.as_bytes()));
    if let Ok(graph) = ActionGraph::open(&graph_path.to_string_lossy()) {
        if let Ok(Some(known_action)) = graph.get_best_action(&state_hash, 0.65) {
            tracing::info!("action_graph shortcut: {known_action}");
            // Shortcut fires: set goal and jump straight to agent loop
            { let mut s = state.lock().await; s.goal = message.clone(); s.running = true; }
            agent::agent_loop(
                state, adapter, perceptor, actuator, approval_rx, confirm_tx, memory_tiers,
                visual_encoder, registry, skill_library,
            ).await;
            return;
        }
    }

    // STATE-AWARE ROUTING: the deterministic levers (surface state + user mode + message shape) decide
    // first — the LLM classifier fires ONLY on the ambiguous/question residual (surface-active + Auto).
    let intent = match deterministic_route(&message, &route) {
        Some(RouteOutcome::Chat) => Intent::Chat,
        Some(RouteOutcome::Interactive) => Intent::Interactive,
        Some(RouteOutcome::Offer) => {
            // Action requested but no surface is active — offer to start one rather than silently chat.
            let _ = confirm_tx.send(envelope::make("action_log", envelope::ActionLogPayload {
                text: "I'd need an active workspace to do that — start the VM (or open Immersive) and I'll take it from there.".to_string(),
            })).await;
            return;
        }
        None => hydra.classify_intent(&message).await,
    };

    match intent {
        Intent::Chat => {
            let response = hydra.chat_response(&message, &context).await;
            let _ = confirm_tx
                .send(envelope::make(
                    "action_log",
                    envelope::ActionLogPayload {
                        text: response,
                    },
                ))
                .await;
        }
        Intent::Interactive | Intent::Reasoning => {
            // Set goal in state (lock, set, drop guard) BEFORE spawning agent loop
            {
                let mut s = state.lock().await;
                s.running = true;
            } // guard dropped

            // For Reasoning: prepend a planning header
            let effective_goal = match intent {
                Intent::Reasoning => format!("[Think step by step]\n{message}"),
                _ => message,
            };

            // Update goal (lock, set, drop guard)
            {
                let mut s = state.lock().await;
                s.goal = effective_goal;
            } // guard dropped

            // Run the agent loop
            agent::agent_loop(
                state, adapter, perceptor, actuator, approval_rx, confirm_tx, memory_tiers,
                visual_encoder, registry, skill_library,
            ).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{opens_with_action_verb, opens_with_command_phrase, parse_intent_label, Intent};

    #[test]
    fn action_verb_fast_path() {
        // The case that broke Wall-2 testing: a long explicit action chain → Interactive.
        assert!(opens_with_action_verb(
            "Open the Applications menu, then click the Terminal Emulator, then type touch /tmp/x, then press Enter"));
        assert!(opens_with_action_verb("Click submit"));
        assert!(opens_with_action_verb("press Enter"));
        assert!(opens_with_action_verb("Launch the Terminal Emulator"));
        // Not an action command → leave to the classifier.
        assert!(!opens_with_action_verb("how are you today"));
        assert!(!opens_with_action_verb("what time is it"));
        assert!(!opens_with_action_verb("opening hours for the museum")); // whole-word guard
    }

    #[test]
    fn parser_first_word_labels() {
        assert_eq!(parse_intent_label("INTERACTIVE"), Intent::Interactive);
        assert_eq!(parse_intent_label("REASONING"), Intent::Reasoning);
        assert_eq!(parse_intent_label("CHAT"), Intent::Chat);
    }

    #[test]
    fn parser_handles_preamble() {
        assert_eq!(parse_intent_label("The answer is INTERACTIVE"), Intent::Interactive);
        assert_eq!(parse_intent_label("Classification: REASONING\n"), Intent::Reasoning);
        assert_eq!(parse_intent_label("Sure, I'd say CHAT is appropriate."), Intent::Chat);
    }

    #[test]
    fn parser_safe_default() {
        assert_eq!(parse_intent_label(""), Intent::Chat);
        assert_eq!(parse_intent_label("I don't know"), Intent::Chat);
        assert_eq!(parse_intent_label("ERROR"), Intent::Chat);
    }

    #[test]
    fn parser_priority_interactive_over_reasoning() {
        // If a confused model emits both, INTERACTIVE wins (more impactful routing)
        assert_eq!(parse_intent_label("INTERACTIVE or REASONING"), Intent::Interactive);
    }

    #[test]
    fn command_phrases_route_interactive_deterministically() {
        // The fix for the CHAT-misroute: explicit command directives must NOT fall to the
        // weak classifier (which sent "run the command …" → CHAT → hallucinated no-op).
        for m in ["run the command touch /tmp/x", "run the command touch a, then run the command touch b",
                  "execute the command ls -la", "$ whoami", "RUN THE COMMAND echo hi"] {
            assert!(opens_with_command_phrase(m), "{m:?} should route Interactive");
        }
        // The bare verb "run" stays borderline (a genuine question must not be force-routed).
        for m in ["run a marathon with me", "running late today", "is it worth a run"] {
            assert!(!opens_with_command_phrase(m), "{m:?} must stay borderline");
        }
    }
}

#[cfg(test)]
mod routing_lever_tests {
    use super::*;

    fn vm() -> RouteContext {
        RouteContext { surface: SurfaceState { vm_active: true, ..Default::default() }, mode: RouteMode::Auto }
    }
    fn none() -> RouteContext {
        RouteContext { surface: SurfaceState::default(), mode: RouteMode::Auto }
    }

    #[test]
    fn action_shape_catches_tasks_rejects_chat() {
        // The cases routing_probe showed MISROUTING to CHAT/REASONING — now caught deterministically.
        for m in ["create two empty files: /tmp/a and /tmp/b", "make a directory called /tmp/project",
                  "delete the file /tmp/old.log", "show how much disk space is free",
                  "rename report.txt to final.txt", "install ripgrep", "open the web browser",
                  "run the command touch /tmp/x"] {
            assert!(is_action_shaped(m), "{m:?} should be action-shaped");
        }
        // Conversation / reasoning must NOT be grabbed (a soft verb with no computer-object is chat).
        for m in ["write a poem about the sea", "what is the capital of France", "hello there",
                  "explain how TCP works", "create"] {
            assert!(!is_action_shaped(m), "{m:?} must NOT be action-shaped");
        }
    }

    #[test]
    fn clear_question_detection() {
        for m in ["what is the capital of France", "how does TCP work", "is the VM running?",
                  "explain how memory works", "Can you do this?"] {
            assert!(is_clear_question(m), "{m:?} is a question");
        }
        for m in ["create two files in /tmp", "open firefox", "install ripgrep"] {
            assert!(!is_clear_question(m), "{m:?} is not a question");
        }
    }

    #[test]
    fn hard_lever_no_surface_is_chat_or_offer() {
        // No surface → an action becomes an OFFER (start a surface), a non-action is CHAT.
        assert_eq!(deterministic_route("create two files in /tmp", &none()), Some(RouteOutcome::Offer));
        assert_eq!(deterministic_route("what is the capital of France", &none()), Some(RouteOutcome::Chat));
    }

    #[test]
    fn surface_active_action_is_interactive_question_is_chat() {
        // Surface active + clear action → Interactive, no LLM. The exact demo goal that misrouted.
        assert_eq!(deterministic_route("create two empty files: /tmp/a and /tmp/b", &vm()),
                   Some(RouteOutcome::Interactive));
        // Surface active + a clear QUESTION → Chat, never the action planner (the user bug:
        // "do you know how to write in rust?" became a bogus write-to-file plan).
        assert_eq!(deterministic_route("what is the capital of France", &vm()), Some(RouteOutcome::Chat));
        assert_eq!(deterministic_route("do you know how to write in rust?", &vm()), Some(RouteOutcome::Chat));
        // Genuinely ambiguous (not action-shaped, not a clear question) → LLM residual.
        assert_eq!(deterministic_route("the files in my downloads folder", &vm()), None);
    }

    #[test]
    fn explicit_modes_hard_override() {
        let chat_lock = RouteContext { surface: SurfaceState { vm_active: true, ..Default::default() }, mode: RouteMode::ChatLock };
        // ChatLock → CHAT even for a clear action.
        assert_eq!(deterministic_route("delete the file /tmp/x", &chat_lock), Some(RouteOutcome::Chat));
        let act_lock = RouteContext { surface: SurfaceState { vm_active: true, ..Default::default() }, mode: RouteMode::ActLock };
        // ActLock acts on anything that isn't a clear question; a question still chats.
        assert_eq!(deterministic_route("organize my downloads", &act_lock), Some(RouteOutcome::Interactive));
        assert_eq!(deterministic_route("what is the capital of France", &act_lock), Some(RouteOutcome::Chat));
    }
}
