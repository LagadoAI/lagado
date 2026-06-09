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
use crate::inference::InferenceAdapter;
use crate::perception::{Perceptor, Actuator};
use crate::retrieval::Retriever;
use crate::{agent, config, envelope, governor};

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
    pub config: HydraConfig,
}

impl Hydra {
    /// Initialize Hydra from system governor detection
    pub fn from_governor(adapter: Arc<dyn InferenceAdapter>) -> Self {
        let server_config = governor::detect_and_plan(config::CONTEXT_SIZE);

        // Derive capability tier from server config
        let capability = if server_config.n_gpu_layers > 0 && server_config.ctx >= 16384 {
            Capability::High
        } else if server_config.n_gpu_layers > 0 || server_config.ctx >= 8192 {
            Capability::Mid
        } else {
            Capability::Low
        };

        Hydra {
            adapter,
            config: HydraConfig { capability },
        }
    }

    /// Classify user intent on a CLEAN PROMPT (no history, current message only)
    ///
    /// CRITICAL: This function receives ONLY the current user message.
    /// Zero conversation history, zero screen data. This discipline is
    /// load-bearing and prevents history poisoning.
    pub async fn classify_intent(&self, message: &str) -> Intent {
        let prompt = format!(
            "You are a binary classifier. Classify the user's message.\n\
             Reply with exactly one word: CHAT, INTERACTIVE, or REASONING.\n\
             \n\
             CHAT = conversation, question, greeting, thanks, opinion\n\
             INTERACTIVE = requests to click, type, open, navigate, search, find, scroll, close anything on screen\n\
             REASONING = complex analysis, planning, code writing, math, multi-step problem solving\n\
             \n\
             Message: {message}\n\
             Classification:"
        );

        match self.adapter.generate(&prompt, 10, 0.0) {
            Ok(response) => {
                let trimmed = response.trim().to_uppercase();
                let first_word = trimmed.split_whitespace().next().unwrap_or("");

                if first_word.starts_with("INTERACTIVE") {
                    Intent::Interactive
                } else if first_word.starts_with("REASONING") {
                    Intent::Reasoning
                } else {
                    Intent::Chat // safe default
                }
            }
            Err(_) => Intent::Chat, // safe default on error
        }
    }

    /// Conversational inference (no tool loop)
    pub async fn chat_response(&self, message: &str, context: &str) -> String {
        let system_prompt = config::system_prompt();
        let prompt = format!(
            "{system_prompt}\n\n{context}\n\nUser: {message}\nAssistant:"
        );

        match self.adapter.generate(&prompt, 512, 0.7) {
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
) {
    let hydra = Hydra::from_governor(adapter.clone());

    // Assemble context via retriever (RAG K=15)
    let context = {
        let retriever = Retriever::new(&config::data_dir());
        let entries = retriever.retrieve_context(&message, 15);
        Retriever::format_context(&entries)
    };

    // Action graph shortcut: known high-confidence workflow → skip classification
    let graph_path = config::data_dir().join("action_graph.db");
    let state_hash = format!("{:x}", message.len()); // Phase 2: real screen hash
    if let Ok(graph) = ActionGraph::open(&graph_path.to_string_lossy()) {
        if let Ok(Some(known_action)) = graph.get_best_action(&state_hash, 0.65) {
            tracing::info!("action_graph shortcut: {known_action}");
            // Shortcut fires: set goal and jump straight to agent loop
            { let mut s = state.lock().await; s.goal = message.clone(); s.running = true; }
            agent::agent_loop(state, adapter, perceptor, actuator, approval_rx, confirm_tx).await;
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
            agent::agent_loop(state, adapter, perceptor, actuator, approval_rx, confirm_tx).await;
        }
    }
}
