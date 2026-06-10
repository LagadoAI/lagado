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

    // Pull cross-session episodic context from MemoryTiers (lock → extract → drop)
    let episodic_context = {
        let tiers = memory_tiers.lock().await;
        tiers.assemble_context(2048)
    };

    // Visual similarity context: encode current frame → find top-3 most visually
    // Visual similarity context: encode current frame → find top-3 past episodes with
    // similar visual context. Runs once per invocation. No-op when encoder absent.
    let visual_context: String = {
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
        let episodic_section = if episodic_context.is_empty() {
            String::new()
        } else {
            format!("Past sessions:\n{episodic_context}\n\n")
        };
        let visual_section = if visual_context.is_empty() {
            String::new()
        } else {
            format!("Visually similar past sessions:\n- {visual_context}\n\n")
        };
        // Top-10 tools most relevant to the current goal — never flat-dump all tools
        let tool_section = {
            let entries = registry.enabled_entries();
            let formatted = crate::retrieval::Retriever::format_tools_for_prompt(&entries, &goal, 10);
            if formatted.is_empty() { String::new() } else { format!("{formatted}\n\n") }
        };

        let prompt = format!(
            "{system_prompt}\n\n{episodic_section}{visual_section}{tool_section}{context}\n\nScreen:\n{screen}\n\nGoal: {goal}\n\nWhat is your next action?"
        );

        let adapter_clone = adapter.clone();
        let forge = Forge {
            model_fn: Box::new(move |p: String| {
                let adapter = adapter_clone.clone();
                Box::pin(async move {
                    tokio::task::spawn_blocking(move || {
                        adapter.generate_with_confidence(&p, 2048, 0.2)
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
                        let episode_id = {
                            let mut tiers = memory_tiers.lock().await;
                            tiers.push_episode_id(format!("Goal '{goal}': {reason}")).ok()
                        };
                        encode_and_store_async(episode_id, &visual_encoder, memory_tiers.clone());
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
