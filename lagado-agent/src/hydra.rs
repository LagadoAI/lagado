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

        match self.classifier.generate(&prompt, 10, 0.0) {
            Ok(response) => parse_intent_label(&response),
            Err(_) => Intent::Chat, // safe default on error
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
    is_paused: bool,        // if true, always CHAT
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

    // Classify intent (respecting pause state)
    let intent = if is_paused {
        Intent::Chat
    } else {
        hydra.classify_intent(&message).await
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
    use super::{parse_intent_label, Intent};

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
}
