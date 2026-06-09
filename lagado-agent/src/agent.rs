use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use crate::types::{PipelineError, Step, ToolCall};
use crate::forge::Forge;
use crate::operator::StepEnforcer;
use crate::memory::Memory;
use crate::inference::InferenceAdapter;
use crate::perception::{Perceptor, Actuator};
use crate::recovery::FailureType;
use crate::{chronos, config, envelope, gate};
use crate::recovery::{RecoveryManager, RecoveryOutcome};
use crate::action_graph::ActionGraph;
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
async fn execute_tool(call: &ToolCall, actuator: &dyn Actuator) -> String {
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
    }
}

// ── Permission request + await human approval ─────────────────────
async fn request_and_await_approval(
    confirm_type: &str, // "tap" | "typed"
    tool_call: &ToolCall,
    state: &Arc<Mutex<AgentState>>,
    actuator: &dyn Actuator,
    approval_rx: &mut mpsc::Receiver<bool>,
    confirm_tx: &mpsc::Sender<String>,
) -> String {
    let desc = gate::describe(tool_call);
    let desc_safe = gate::describe_redacted(tool_call);
    let tool_name = match tool_call {
        ToolCall::Click { .. } => "click",
        ToolCall::Type { .. } => "type",
        ToolCall::Key { .. } => "key",
        ToolCall::Chat { .. } => "chat",
        _ => "unknown",
    };
    let id = uuid::Uuid::new_v4().to_string();
    chronos::log(&format!("confirm_requested: {confirm_type}: {desc_safe}"));
    let _ = confirm_tx
        .send(envelope::make(
            "permission",
            envelope::PermissionPayload {
                id: id.clone(),
                type_: confirm_type.to_string(),
                tool: tool_name.to_string(),
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
        let out = execute_tool(tool_call, actuator).await;
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

// ── Agent loop ────────────────────────────────────────────────────
pub async fn agent_loop(
    state: Arc<Mutex<AgentState>>,
    adapter: Arc<dyn InferenceAdapter>,
    perceptor: Arc<dyn Perceptor>,
    actuator: Arc<dyn Actuator>,
    mut approval_rx: mpsc::Receiver<bool>,
    confirm_tx: mpsc::Sender<String>,
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

    // Sliding window of recent action descriptions for loop/deadlock detection
    let mut recent_actions: Vec<String> = Vec::new();

    let goal = state.lock().await.goal.clone();
    let system_prompt = config::system_prompt();
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

        let screen = perceptor.read_screen();
        let context = memory.context_string();
        let prompt = format!(
            "{system_prompt}\n\n{context}\n\nScreen:\n{screen}\n\nGoal: {goal}\n\nWhat is your next action?"
        );

        let adapter_clone = adapter.clone();
        let forge = Forge {
            model_fn: Box::new(move |p: String| {
                let adapter = adapter_clone.clone();
                Box::pin(async move {
                    tokio::task::spawn_blocking(move || {
                        adapter.generate(&p, 2048, 0.2)
                            .map_err(|e| PipelineError::ModelError(e))
                    })
                    .await
                    .map_err(|e| PipelineError::ModelError(e.to_string()))?
                })
            }),
        };

        match forge.call_with_retry(&prompt, &enforcer).await {
            Ok(tool_call) => {
                tracing::info!("Step {}: {:?}", enforcer.step(), tool_call);

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
                let output = match gate::evaluate_action(&tool_call) {
                    gate::Verdict::Allow => {
                        let desc = gate::describe_redacted(&tool_call);
                        let out = execute_tool(&tool_call, actuator.as_ref()).await;
                        chronos::log(&format!("action: {desc} -> {out}"));
                        let _ = confirm_tx.send(envelope::make("action_log", envelope::ActionLogPayload {
                            text: format!("{desc} -> {out}"),
                        })).await;
                        out
                    }
                    gate::Verdict::ConfirmTap => {
                        request_and_await_approval("tap", &tool_call, &state, actuator.as_ref(), &mut approval_rx, &confirm_tx).await
                    }
                    gate::Verdict::ConfirmTyped => {
                        request_and_await_approval("typed", &tool_call, &state, actuator.as_ref(), &mut approval_rx, &confirm_tx).await
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

                memory.push(Step {
                    index: enforcer.step(),
                    prompt: prompt.clone(),
                    output,
                    action: Some(tool_call.clone()),
                });

                recent_actions.push(gate::describe(&tool_call));
                if recent_actions.len() > 15 { recent_actions.remove(0); }

                // Structural failure detection (loop / deadlock)
                if let Some(structural) = FailureType::detect_structural(&recent_actions) {
                    tracing::warn!("Structural failure detected: {structural}");
                    if let Some(ref rm) = recovery_manager {
                        let s = perceptor.read_screen();
                        match rm.recover(&structural, &state_hash, &s, &recent_actions).await {
                            Some(RecoveryOutcome::PromptInjection { text, .. }) => {
                                tracing::info!("Recovery injection: {}", &text[..text.len().min(80)]);
                                memory.push(Step { index: enforcer.step(), prompt: text, output: "recovery_injection".to_string(), action: None });
                                continue;
                            }
                            _ => break,
                        }
                    } else {
                        break;
                    }
                }

                match &tool_call {
                    ToolCall::Done { reason } => {
                        chronos::log(&format!("goal_done: {reason}"));
                        let _ = confirm_tx.send(envelope::make("status", envelope::StatusPayload {
                            state: "goal_done".to_string(),
                            detail: reason.clone(),
                        })).await;
                        tracing::info!("Goal achieved.");
                        break;
                    }
                    ToolCall::Task { description } => {
                        chronos::log(&format!("goal_done: {description}"));
                        let _ = confirm_tx.send(envelope::make("status", envelope::StatusPayload {
                            state: "goal_done".to_string(),
                            detail: description.clone(),
                        })).await;
                        tracing::info!("Goal achieved.");
                        break;
                    }
                    _ => {}
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
                                continue;
                            }
                            Some(RecoveryOutcome::MemoryReset { discard_steps }) => {
                                tracing::info!("Recovery: memory reset ({discard_steps} steps)");
                                // Phase 2: implement memory.discard_last(discard_steps)
                                continue;
                            }
                            Some(RecoveryOutcome::HealedAction(action)) => {
                                tracing::info!("Recovery: healed action from graph");
                                memory.push(Step { index: enforcer.step(), prompt: action, output: "healed".to_string(), action: None });
                                continue;
                            }
                            None => {}
                        }
                    }
                    let detail = format!("{:?}", e);
                    chronos::log(&format!("goal_aborted: {detail}"));
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
