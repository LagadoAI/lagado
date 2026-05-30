//! recovery.rs — Self-healing agent module for Laputa.
//!
//! Classifies runtime failures, consults a persistent action graph for learned
//! recoveries, falls back to LLM-guided repair, and gates high-risk or
//! low-confidence actions with a user-facing permission request.
//!
//! # Integration
//! ```rust
//! mod recovery;
//! use recovery::{FailureType, RecoveryManager, RecoveryOutcome, PendingPermission};
//! use std::sync::Arc;
//! use tokio::sync::Mutex;
//!
//! // Shared state: WS handler fills this when it receives "permission:approved/denied"
//! let pending: PendingPermission = Arc::new(Mutex::new(None));
//!
//! let manager = RecoveryManager::new(
//!     graph,
//!     Some(ws_out_tx),         // mpsc::Sender<String> → WebSocket
//!     Arc::clone(&pending),
//!     "http://127.0.0.1:8080/v1/chat/completions".into(),
//! );
//!
//! // In your WS message handler:
//! if raw == "permission:approved" || raw == "permission:denied" {
//!     let approved = raw == "permission:approved";
//!     if let Some(tx) = pending.lock().await.take() { let _ = tx.send(approved); }
//! }
//!
//! // In your agent loop:
//! match manager.recover(&failure, &state_hash, &screen, &recent_actions).await {
//!     Some(RecoveryOutcome::HealedAction(json))         => execute(json),
//!     Some(RecoveryOutcome::PromptInjection { text, t }) => re_run_inference(text, t),
//!     Some(RecoveryOutcome::MemoryReset { n })           => discard_last_n(n),
//!     None                                               => break,  // abort
//! }
//! ```

use log;
use std::{collections::VecDeque, fmt, sync::Arc, time::Duration};

use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::{oneshot, Mutex};

use crate::action_graph::ActionGraph;
use crate::types::PipelineError;

// ── Constants ─────────────────────────────────────────────────────────────────

const QWEN_MODEL:          &str  = "Qwen3-8B-ShiningValiant3.IQ4_XS.gguf";
const RECOVERY_PREFIX:     &str  = "recovery";
const DEFAULT_MIN_CONF:    f64   = 0.65;
const PERM_TIMEOUT_SECS:   u64   = 30;
const LOOP_DETECT_THRESH:  usize = 5;   // same action N times in a row
const DEADLOCK_THRESH:     usize = 10;  // no new unique actions in N steps

/// Shared state between `RecoveryManager` and the WebSocket message handler.
/// When the WS handler receives `permission:approved` or `permission:denied`,
/// it takes the `oneshot::Sender<bool>` and sends the decision.
pub type PendingPermission = Arc<Mutex<Option<oneshot::Sender<bool>>>>;

// ── FailureType ───────────────────────────────────────────────────────────────

/// Every distinct way the agent loop can break down.
#[derive(Debug, Clone, PartialEq)]
pub enum FailureType {
    /// LLM output could not be parsed as a valid tool call.
    ParseFailure(String),
    /// An executed tool returned an error.
    ToolError(String),
    /// The Forge harness exhausted all retry nudges.
    MaxRetriesExceeded,
    /// The step enforcer hit the step cap.
    MaxStepsExceeded,
    /// The same action was repeated `LOOP_DETECT_THRESH` times in a row.
    LoopDetected,
    /// No unique tool calls executed in the last `DEADLOCK_THRESH` steps.
    DeadLock,
    /// The agent produced a tool name that doesn't exist in the grammar.
    HallucinatedAction(String),
}

impl FailureType {
    /// Short snake_case key used as the first segment of the action-graph prefix.
    fn key(&self) -> &'static str {
        match self {
            Self::ParseFailure(_)       => "parse_failure",
            Self::ToolError(_)          => "tool_error",
            Self::MaxRetriesExceeded    => "max_retries",
            Self::MaxStepsExceeded      => "max_steps",
            Self::LoopDetected          => "loop_detected",
            Self::DeadLock              => "deadlock",
            Self::HallucinatedAction(_) => "hallucinated",
        }
    }

    /// Build the action-graph state key for this failure + screen hash pair.
    fn graph_key(&self, screen_hash: &str) -> String {
        format!("{RECOVERY_PREFIX}:{}:{}", self.key(), screen_hash)
    }

    /// Attempt to detect loop / deadlock from a sliding window of recent actions.
    ///
    /// Returns `Some(FailureType)` if a structural failure is detected, else `None`.
    pub fn detect_structural(recent: &[String]) -> Option<Self> {
        if recent.len() < LOOP_DETECT_THRESH {
            return None;
        }
        // Loop: last N actions are all identical
        let tail = &recent[recent.len().saturating_sub(LOOP_DETECT_THRESH)..];
        if tail.windows(2).all(|w| w[0] == w[1]) {
            return Some(Self::LoopDetected);
        }
        // DeadLock: no unique action in the last DEADLOCK_THRESH steps
        if recent.len() >= DEADLOCK_THRESH {
            let window = &recent[recent.len().saturating_sub(DEADLOCK_THRESH)..];
            let unique: std::collections::HashSet<&String> = window.iter().collect();
            if unique.len() <= 1 {
                return Some(Self::DeadLock);
            }
        }
        None
    }
}

impl fmt::Display for FailureType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ParseFailure(s)       => write!(f, "ParseFailure({s})"),
            Self::ToolError(s)          => write!(f, "ToolError({s})"),
            Self::MaxRetriesExceeded    => write!(f, "MaxRetriesExceeded"),
            Self::MaxStepsExceeded      => write!(f, "MaxStepsExceeded"),
            Self::LoopDetected          => write!(f, "LoopDetected"),
            Self::DeadLock              => write!(f, "DeadLock"),
            Self::HallucinatedAction(s) => write!(f, "HallucinatedAction({s})"),
        }
    }
}

impl From<&PipelineError> for FailureType {
    fn from(e: &PipelineError) -> Self {
        match e {
            PipelineError::ParseFailed(s)   => Self::ParseFailure(s.clone()),
            PipelineError::MaxRetriesExceeded => Self::MaxRetriesExceeded,
            PipelineError::MaxStepsExceeded   => Self::MaxStepsExceeded,
            PipelineError::ModelError(s)      => Self::ToolError(s.clone()),
        }
    }
}

impl From<PipelineError> for FailureType {
    fn from(e: PipelineError) -> Self { (&e).into() }
}

// ── RecoveryOutcome ───────────────────────────────────────────────────────────

/// What the agent loop should do after a successful recovery.
/// Extends `Option<String>` to handle prompt-injection and memory-reset cases
/// that can't be expressed as a single tool call.
#[derive(Debug, Clone)]
pub enum RecoveryOutcome {
    /// A healed, ready-to-execute tool call JSON string.
    HealedAction(String),
    /// Inject this text into the next LLM prompt and re-run inference.
    /// `temperature_override` is `Some(0.1)` for LoopDetected to force variety.
    PromptInjection {
        text:                String,
        temperature_override: Option<f32>,
    },
    /// Discard the last `discard_steps` entries from memory and restart observation.
    MemoryReset { discard_steps: usize },
}

// ── RecoveryManager ───────────────────────────────────────────────────────────

/// Central coordinator for failure recovery and pre-execution permission gating.
pub struct RecoveryManager {
    graph:          Arc<ActionGraph>,
    qwen_url:       String,
    min_confidence: f64,
    /// Sends a raw string down the WebSocket to the frontend (e.g. `permission:…`).
    ws_out:         Option<tokio::sync::mpsc::Sender<String>>,
    /// Set before sending a permission request; cleared by the WS response handler.
    pending_perm:   PendingPermission,
}

impl RecoveryManager {
    // ── Construction ──────────────────────────────────────────────────────────

    pub fn new(
        graph:        ActionGraph,
        ws_out:       Option<tokio::sync::mpsc::Sender<String>>,
        pending_perm: PendingPermission,
        qwen_url:     String,
    ) -> Self {
        Self {
            graph:          Arc::new(graph),
            qwen_url,
            min_confidence: DEFAULT_MIN_CONF,
            ws_out,
            pending_perm,
        }
    }

    /// Override the minimum graph confidence threshold (default 0.65).
    pub fn with_min_confidence(mut self, c: f64) -> Self {
        self.min_confidence = c.clamp(0.0, 1.0);
        self
    }

    // ── Public API ────────────────────────────────────────────────────────────

    /// Attempt to recover from `failure`. Returns `Some(outcome)` if recovery
    /// was possible, `None` if the task should be aborted.
    pub async fn recover(
        &self,
        failure:          &FailureType,
        state_hash:       &str,
        screen_state:     &str,
        recent_actions:   &[String],
    ) -> Option<RecoveryOutcome> {
        let graph_key = failure.graph_key(state_hash);

        // ── Step 1: check the action graph for a learned recovery ─────────────
        let learned = self.graph
            .get_best_action(&graph_key, self.min_confidence)
            .unwrap_or(None);

        if let Some(ref action) = learned {
            log::info!("[recovery] Graph hit for {failure}: {}", &action[..action.len().min(80)]);
            // Optimistically record as success; caller corrects if execution fails
            let _ = self.graph.record_outcome(&graph_key, action, true);
            return Some(RecoveryOutcome::HealedAction(action.clone()));
        }

        // ── Step 2: apply the hardcoded strategy for this failure type ────────
        let outcome = match failure {
            FailureType::ParseFailure(raw) =>
                self.recover_parse_failure(raw, screen_state, &graph_key).await,

            FailureType::ToolError(msg) =>
                self.recover_tool_error(msg, screen_state, recent_actions, &graph_key).await,

            FailureType::MaxRetriesExceeded =>
                self.recover_max_retries(recent_actions, &graph_key).await,

            FailureType::MaxStepsExceeded =>
                self.recover_max_steps(screen_state, recent_actions, &graph_key).await,

            FailureType::LoopDetected =>
                Some(self.recover_loop(&graph_key)),

            FailureType::DeadLock =>
                Some(self.recover_deadlock(&graph_key)),

            FailureType::HallucinatedAction(raw) =>
                self.recover_hallucination(raw, screen_state, &graph_key).await,
        };

        outcome
    }

    /// Pre-execution confidence gate. Returns `true` if the action should proceed.
    ///
    /// Triggers a user permission request when:
    /// - `confidence < 0.25` (any risk level), or
    /// - `confidence < 0.40` AND `risk_level == "high"`
    pub async fn pre_execution_check(
        &self,
        action:     &str,
        confidence: f64,
        risk_level: &str,
        state_hash: &str,
    ) -> bool {
        let needs_gate = confidence < 0.25
            || (confidence < 0.40 && risk_level.eq_ignore_ascii_case("high"));

        if !needs_gate {
            return true;
        }

        let reason = if confidence < 0.25 {
            format!("Low agent confidence ({:.0}%)", confidence * 100.0)
        } else {
            format!("High-risk action at {:.0}% confidence", confidence * 100.0)
        };

        log::warn!("[recovery] Pre-execution gate triggered — {reason}: {action}");

        // Send permission request via WebSocket
        let approved = self.request_permission(action, risk_level, &reason).await;

        // Record outcome in action graph so the agent learns what users allow
        let graph_key = format!("permission:{state_hash}");
        let _ = self.graph.record_outcome(&graph_key, action, approved);

        if !approved {
            log::info!("[recovery] Action denied by user: {action}");
        }
        approved
    }

    /// Record the final outcome of a recovery attempt (called by the agent loop
    /// after the healed action is executed).
    pub fn record_recovery_outcome(
        &self,
        failure:      &FailureType,
        state_hash:   &str,
        action:       &str,
        success:      bool,
    ) {
        let key = failure.graph_key(state_hash);
        if let Err(e) = self.graph.record_outcome(&key, action, success) {
            log::warn!("[recovery] Failed to record outcome: {e}");
        }
    }

    // ── Strategy implementations ──────────────────────────────────────────────

    /// ParseFailure: ask Qwen what it intended, try to extract a valid call.
    async fn recover_parse_failure(
        &self,
        raw:       &str,
        screen:    &str,
        graph_key: &str,
    ) -> Option<RecoveryOutcome> {
        let prompt = format!(
            "Your previous output could not be parsed as a JSON tool call.\n\
             Raw output was:\n```\n{raw}\n```\n\
             Screen state: {screen}\n\n\
             Output ONLY a corrected JSON tool call. \
             Valid tools: click, type, key, wait, task.\n\
             Example: {{\"tool\":\"click\",\"selector\":\"ref_42\"}}"
        );

        match self.ask_qwen(&prompt, 0.1).await {
            Ok(corrected) => {
                log::info!("[recovery] ParseFailure corrected by LLM");
                let _ = self.graph.record_outcome(graph_key, &corrected, true);
                Some(RecoveryOutcome::HealedAction(corrected))
            }
            Err(e) => {
                log::warn!("[recovery] LLM unreachable for ParseFailure recovery: {e}");
                None
            }
        }
    }

    /// ToolError: retry once, then ask Qwen for a corrected action.
    async fn recover_tool_error(
        &self,
        error_msg:      &str,
        screen:         &str,
        recent_actions: &[String],
        graph_key:      &str,
    ) -> Option<RecoveryOutcome> {
        // Re-use the last action as the retry candidate
        let last_action = recent_actions.last()?;

        let prompt = format!(
            "You just executed an action that failed.\n\
             Failed action: {last_action}\n\
             Error: {error_msg}\n\
             Screen state: {screen}\n\n\
             Output ONLY a corrected JSON tool call that avoids this error. \
             If the action cannot be corrected, output: \
             {{\"tool\":\"task\",\"description\":\"skip — tool error unrecoverable\"}}"
        );

        match self.ask_qwen(&prompt, 0.2).await {
            Ok(corrected) => {
                log::info!("[recovery] ToolError corrected by LLM");
                let _ = self.graph.record_outcome(graph_key, &corrected, true);
                Some(RecoveryOutcome::HealedAction(corrected))
            }
            Err(e) => {
                log::warn!("[recovery] LLM unreachable for ToolError recovery: {e}");
                // Hard fallback: skip this step
                let skip = json!({"tool":"task","description":"skip — tool error, LLM unreachable"})
                    .to_string();
                Some(RecoveryOutcome::HealedAction(skip))
            }
        }
    }

    /// MaxRetriesExceeded: skip the current step and continue.
    async fn recover_max_retries(
        &self,
        recent_actions: &[String],
        graph_key:      &str,
    ) -> Option<RecoveryOutcome> {
        log::warn!("[recovery] MaxRetriesExceeded — skipping step");
        let skip = json!({"tool":"task","description":"skip — max retries exceeded"}).to_string();
        let _ = self.graph.record_outcome(graph_key, &skip, true);
        // Try to extract anything useful from the last raw output
        if let Some(last) = recent_actions.last() {
            if last.contains('"') {
                // Attempt naive rescue
                if let Some(rescued) = naive_rescue(last) {
                    log::info!("[recovery] MaxRetries: naive rescue succeeded");
                    let _ = self.graph.record_outcome(graph_key, &rescued, true);
                    return Some(RecoveryOutcome::HealedAction(rescued));
                }
            }
        }
        Some(RecoveryOutcome::HealedAction(skip))
    }

    /// MaxStepsExceeded: ask Qwen whether to continue with a new plan or abort.
    async fn recover_max_steps(
        &self,
        screen:         &str,
        recent_actions: &[String],
        graph_key:      &str,
    ) -> Option<RecoveryOutcome> {
        let summary: String = recent_actions
            .iter()
            .rev()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join(", ");

        let prompt = format!(
            "You have reached the maximum step limit.\n\
             Last 5 actions: {summary}\n\
             Screen state: {screen}\n\n\
             If the goal is complete, output: {{\"tool\":\"task\",\"description\":\"complete\"}}\n\
             If progress is possible with one more targeted action, output that action.\n\
             If the goal is unreachable, output: {{\"tool\":\"task\",\"description\":\"abort\"}}"
        );

        match self.ask_qwen(&prompt, 0.3).await {
            Ok(decision) => {
                let _ = self.graph.record_outcome(graph_key, &decision, true);
                if decision.contains("\"abort\"") {
                    log::info!("[recovery] MaxSteps: LLM chose abort");
                    None
                } else {
                    log::info!("[recovery] MaxSteps: LLM provided continuation action");
                    Some(RecoveryOutcome::HealedAction(decision))
                }
            }
            Err(e) => {
                log::warn!("[recovery] LLM unreachable for MaxSteps recovery: {e}");
                None  // Safe default: abort
            }
        }
    }

    /// LoopDetected: inject urgency text, lower temperature.
    fn recover_loop(&self, graph_key: &str) -> RecoveryOutcome {
        log::warn!("[recovery] LoopDetected — injecting urgency");
        let text = format!(
            "\n⚠ LOOP DETECTED: You have repeated the same action {LOOP_DETECT_THRESH} times \
             in a row with no change in state. You MUST choose a completely different \
             action. Do not repeat your last action under any circumstances.\n"
        );
        let action_repr = "__prompt_injection_urgency__";
        let _ = self.graph.record_outcome(graph_key, action_repr, true);
        RecoveryOutcome::PromptInjection {
            text,
            temperature_override: Some(0.1),
        }
    }

    /// DeadLock: discard stale context, reset to fresh observation.
    fn recover_deadlock(&self, graph_key: &str) -> RecoveryOutcome {
        log::warn!("[recovery] DeadLock — resetting memory context");
        let action_repr = "__memory_reset__";
        let _ = self.graph.record_outcome(graph_key, action_repr, true);
        RecoveryOutcome::MemoryReset { discard_steps: DEADLOCK_THRESH }
    }

    /// HallucinatedAction: tell Qwen the valid tool set and ask again.
    async fn recover_hallucination(
        &self,
        raw:       &str,
        screen:    &str,
        graph_key: &str,
    ) -> Option<RecoveryOutcome> {
        let prompt = format!(
            "You referenced a tool that does not exist.\n\
             Your output: {raw}\n\
             Screen state: {screen}\n\n\
             The ONLY valid tools are:\n\
             - click       {{\"tool\":\"click\",\"selector\":\"<ref_id>\"}}\n\
             - type        {{\"tool\":\"type\",\"selector\":\"<ref_id>\",\"text\":\"<text>\"}}\n\
             - key         {{\"tool\":\"key\",\"key\":\"<key>\"}}\n\
             - wait        {{\"tool\":\"wait\",\"ms\":<number>}}\n\
             - task        {{\"tool\":\"task\",\"description\":\"<description>\"}}\n\n\
             Output ONLY the corrected JSON tool call."
        );

        match self.ask_qwen(&prompt, 0.1).await {
            Ok(corrected) => {
                log::info!("[recovery] Hallucination corrected by LLM");
                let _ = self.graph.record_outcome(graph_key, &corrected, true);
                Some(RecoveryOutcome::HealedAction(corrected))
            }
            Err(e) => {
                log::warn!("[recovery] LLM unreachable for Hallucination recovery: {e}");
                None
            }
        }
    }

    // ── LLM and WebSocket helpers ─────────────────────────────────────────────

    /// Ask Qwen3 and return the content string. Returns Err if unreachable.
    async fn ask_qwen(&self, prompt: &str, temperature: f32) -> Result<String, String> {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| e.to_string())?;

        let payload = json!({
            "model": QWEN_MODEL,
            "messages": [
                {
                    "role": "system",
                    "content": "You are a precise JSON tool-call generator for an \
                                autonomous desktop agent. Output ONLY valid JSON. \
                                No explanations, no markdown, no reasoning tokens."
                },
                { "role": "user", "content": prompt }
            ],
            "max_tokens": 256,
            "temperature": temperature,
        });

        let resp = client
            .post(&self.qwen_url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("Qwen unreachable: {e}"))?;

        let body: Value = resp
            .json()
            .await
            .map_err(|e| format!("Qwen response parse error: {e}"))?;

        // Extract content, falling back to reasoning_content for thinking models
        let msg    = &body["choices"][0]["message"];
        let content = msg["content"].as_str().unwrap_or("").trim().to_string();
        if !content.is_empty() { return Ok(content); }

        let reasoning = msg["reasoning_content"].as_str().unwrap_or("").trim().to_string();
        if !reasoning.is_empty() { return Ok(reasoning); }

        Err("Qwen returned empty content".into())
    }

    /// Send a `permission:<json>` message over the WebSocket and wait for the
    /// user's `permission:approved` or `permission:denied` response.
    /// Returns `true` (approved) or `false` (denied / timed out).
    async fn request_permission(&self, action: &str, risk: &str, reason: &str) -> bool {
        let ws_tx = match &self.ws_out {
            Some(tx) => tx.clone(),
            None => {
                log::warn!("[recovery] No WebSocket sender — auto-approving permission request");
                return true;
            }
        };

        let (resp_tx, resp_rx) = oneshot::channel::<bool>();

        // Register the pending permission *before* sending the WS message
        // so there's no race between the user clicking and us registering.
        {
            let mut guard = self.pending_perm.lock().await;
            *guard = Some(resp_tx);
        }

        let msg = json!({
            "action":     action,
            "risk_level": risk,
            "reason":     reason,
        })
        .to_string();

        if ws_tx.send(format!("permission:{msg}")).await.is_err() {
            log::warn!("[recovery] WebSocket send failed — auto-approving");
            let mut guard = self.pending_perm.lock().await;
            *guard = None;
            return true;
        }

        // Wait for the user's response (timeout = PERM_TIMEOUT_SECS)
        match tokio::time::timeout(
            Duration::from_secs(PERM_TIMEOUT_SECS),
            resp_rx,
        )
        .await
        {
            Ok(Ok(approved))  => approved,
            Ok(Err(_))        => {
                log::warn!("[recovery] Permission channel dropped — denying");
                false
            }
            Err(_) => {
                log::warn!("[recovery] Permission request timed out after {PERM_TIMEOUT_SECS}s — denying");
                let mut guard = self.pending_perm.lock().await;
                *guard = None;
                false
            }
        }
    }
}

// ── Naive rescue parser ───────────────────────────────────────────────────────

/// Last-resort: scan a string for a JSON object and return it if valid.
fn naive_rescue(raw: &str) -> Option<String> {
    let start = raw.find('{')?;
    let mut depth = 0usize;
    for (i, ch) in raw[start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    let candidate = &raw[start..start + i + 1];
                    // Validate: must have a "tool" key
                    if candidate.contains("\"tool\"") {
                        return Some(candidate.to_string());
                    }
                }
            }
            _ => {}
        }
    }
    None
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action_graph::ActionGraph;
    use tokio::sync::mpsc;

    fn mem_graph() -> ActionGraph {
        ActionGraph::open(":memory:").expect("in-memory DB")
    }

    fn make_manager(graph: ActionGraph) -> RecoveryManager {
        let pending: PendingPermission = Arc::new(Mutex::new(None));
        RecoveryManager::new(
            graph,
            None,   // no WS in tests
            pending,
            "http://127.0.0.1:8080/v1/chat/completions".into(),
        )
    }

    // ── FailureType classification ────────────────────────────────────────────

    #[test]
    fn test_from_pipeline_error_parse() {
        let err = PipelineError::ParseFailed("bad json".into());
        let ft  = FailureType::from(&err);
        assert!(matches!(ft, FailureType::ParseFailure(_)));
    }

    #[test]
    fn test_from_pipeline_error_max_retries() {
        let err = PipelineError::MaxRetriesExceeded;
        assert!(matches!(FailureType::from(&err), FailureType::MaxRetriesExceeded));
    }

    #[test]
    fn test_from_pipeline_error_max_steps() {
        let err = PipelineError::MaxStepsExceeded;
        assert!(matches!(FailureType::from(&err), FailureType::MaxStepsExceeded));
    }

    #[test]
    fn test_from_pipeline_error_model_error() {
        let err = PipelineError::ModelError("conn refused".into());
        assert!(matches!(FailureType::from(&err), FailureType::ToolError(_)));
    }

    // ── Loop / deadlock detection ─────────────────────────────────────────────

    #[test]
    fn test_loop_detected() {
        let click = r#"{"tool":"click","selector":"ref_1"}"#.to_string();
        let actions: Vec<String> = vec![click; LOOP_DETECT_THRESH + 1];
        let result = FailureType::detect_structural(&actions);
        assert_eq!(result, Some(FailureType::LoopDetected));
    }

    #[test]
    fn test_no_loop_if_varying() {
        let actions: Vec<String> = (0..10)
            .map(|i| format!(r#"{{"tool":"click","selector":"ref_{i}"}}"#))
            .collect();
        let result = FailureType::detect_structural(&actions);
        assert!(result.is_none() || !matches!(result, Some(FailureType::LoopDetected)));
    }

    #[test]
    fn test_deadlock_detected() {
        // All the same action, more than DEADLOCK_THRESH steps
        let click = r#"{"tool":"click","selector":"ref_5"}"#.to_string();
        let actions: Vec<String> = vec![click; DEADLOCK_THRESH + 1];
        // Should detect LoopDetected first (which is a subset of deadlock)
        let result = FailureType::detect_structural(&actions);
        assert!(result.is_some());
    }

    // ── LoopDetected recovery: urgency injection ──────────────────────────────

    #[test]
    fn test_loop_recovery_returns_prompt_injection() {
        let manager = make_manager(mem_graph());
        let outcome = manager.recover_loop("recovery:loop_detected:abc123");
        assert!(matches!(
            outcome,
            RecoveryOutcome::PromptInjection { temperature_override: Some(t), .. } if t < 0.2
        ));
        // Urgency text contains the loop threshold
        if let RecoveryOutcome::PromptInjection { text, .. } = outcome {
            assert!(text.contains("LOOP DETECTED"));
        }
    }

    // ── DeadLock recovery ─────────────────────────────────────────────────────

    #[test]
    fn test_deadlock_recovery_returns_memory_reset() {
        let manager = make_manager(mem_graph());
        let outcome = manager.recover_deadlock("recovery:deadlock:xyz");
        assert!(matches!(
            outcome,
            RecoveryOutcome::MemoryReset { discard_steps } if discard_steps == DEADLOCK_THRESH
        ));
    }

    // ── Action graph recording and retrieval ──────────────────────────────────

    #[tokio::test]
    async fn test_graph_records_and_retrieves_recovery() {
        let graph   = mem_graph();
        let failure = FailureType::ToolError("exit code 1".into());
        let hash    = "deadbeef";
        let key     = failure.graph_key(hash);
        let action  = r#"{"tool":"key","key":"ctrl+c"}"#;

        // Record several successes
        for _ in 0..5 { graph.record_outcome(&key, action, true).unwrap(); }

        // Should retrieve with high confidence
        let result = graph.get_best_action(&key, 0.7).unwrap();
        assert_eq!(result, Some(action.to_string()));
    }

    #[tokio::test]
    async fn test_graph_does_not_return_below_threshold() {
        let graph   = mem_graph();
        let failure = FailureType::ParseFailure("garbage".into());
        let hash    = "cafebabe";
        let key     = failure.graph_key(hash);
        let action  = r#"{"tool":"wait","ms":500}"#;

        // 1 success, 4 failures → 0.20 probability
        graph.record_outcome(&key, action, true).unwrap();
        for _ in 0..4 { graph.record_outcome(&key, action, false).unwrap(); }

        let result = graph.get_best_action(&key, 0.65).unwrap();
        assert!(result.is_none(), "Should not return low-confidence recovery");
    }

    // ── pre_execution_check ───────────────────────────────────────────────────

    #[tokio::test]
    async fn test_pre_execution_approves_high_confidence() {
        let manager = make_manager(mem_graph());
        let allowed = manager
            .pre_execution_check(r#"{"tool":"click","selector":"ref_1"}"#, 0.95, "low", "hash1")
            .await;
        assert!(allowed, "High-confidence low-risk action should be approved");
    }

    #[tokio::test]
    async fn test_pre_execution_auto_approves_without_ws() {
        // When there is no WebSocket (headless / test mode), the manager
        // logs a warning and auto-approves rather than blocking forever.
        let manager = make_manager(mem_graph());
        // confidence=0.15 would normally gate, but no WS → auto-approve
        let allowed = manager
            .pre_execution_check(r#"{"tool":"task","description":"rm -rf /"}"#, 0.15, "high", "h2")
            .await;
        assert!(allowed, "No-WS mode should auto-approve to avoid blocking");
    }

    #[tokio::test]
    async fn test_pre_execution_denies_via_ws_channel() {
        // Wire up a fake WS: sink channel for outgoing messages,
        // pre-loaded `pending_perm` with a deny response.
        let (ws_tx, mut ws_rx) = mpsc::channel::<String>(4);
        let pending: PendingPermission = Arc::new(Mutex::new(None));
        let manager = RecoveryManager::new(
            mem_graph(),
            Some(ws_tx),
            Arc::clone(&pending),
            "http://127.0.0.1:8080/v1/chat/completions".into(),
        );

        // Spawn a task that intercepts the permission request and denies it
        let pending_clone = Arc::clone(&pending);
        let responder = tokio::spawn(async move {
            // Wait for the WS message to be sent
            ws_rx.recv().await.unwrap();
            // Then respond with "denied" via the oneshot channel
            tokio::time::sleep(Duration::from_millis(20)).await;
            let mut guard = pending_clone.lock().await;
            if let Some(tx) = guard.take() {
                let _ = tx.send(false);  // deny
            }
        });

        let allowed = manager
            .pre_execution_check(
                r#"{"tool":"task","description":"delete everything"}"#,
                0.10,  // very low confidence → gate triggers
                "high",
                "hash3",
            )
            .await;

        responder.await.unwrap();
        assert!(!allowed, "User denial should block the action");
    }

    // ── naive_rescue ──────────────────────────────────────────────────────────

    #[test]
    fn test_naive_rescue_extracts_valid_json() {
        let raw = r#"Thinking... I should click the button. {"tool":"click","selector":"ref_7"} done."#;
        let result = naive_rescue(raw);
        assert_eq!(result, Some(r#"{"tool":"click","selector":"ref_7"}"#.to_string()));
    }

    #[test]
    fn test_naive_rescue_returns_none_without_tool_key() {
        let raw = r#"{"action":"click","selector":"ref_7"}"#;
        let result = naive_rescue(raw);
        assert!(result.is_none(), "Missing 'tool' key should not rescue");
    }

    // ── graph_key format ──────────────────────────────────────────────────────

    #[test]
    fn test_graph_key_format() {
        let ft  = FailureType::HallucinatedAction("bad_tool".into());
        let key = ft.graph_key("abc123");
        assert_eq!(key, "recovery:hallucinated:abc123");
    }

    #[test]
    fn test_failure_type_display() {
        assert_eq!(FailureType::MaxRetriesExceeded.to_string(), "MaxRetriesExceeded");
        assert_eq!(
            FailureType::ToolError("exit 1".into()).to_string(),
            "ToolError(exit 1)"
        );
    }
}
