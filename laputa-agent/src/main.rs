mod action_graph;
mod types;
mod parser;
mod forge;
mod operator;
mod memory;
mod inference;

use std::process::Command;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio_tungstenite::accept_async;
use futures_util::StreamExt;

use types::{PipelineError, Step, ToolCall};
use forge::Forge;
use operator::StepEnforcer;
use memory::Memory;
use inference::llama_cpp::LlamaCppAdapter;
use inference::InferenceAdapter;

const SYSTEM_PROMPT: &str = r#"You are an autonomous GUI agent on a Linux desktop.
Your perception tool outputs the active window's interactive elements in this format:
[focused: Terminal - laputa@archlinux:~]
[window: x=0 y=51 w=1280 h=749]
  ref_1  toggle button   "Applications"     state=has-tooltip
  ref_3  toggle button   "Xfce Terminal"    state=has-tooltip
  ref_5  entry           "Search"           state=editable
TOOLS:
- click: {"tool":"click","selector":"<ref_id>"}
- type:  {"tool":"type","selector":"<ref_id>","text":"<string>"}
- key:   {"tool":"key","key":"<key>"}
- wait:  {"tool":"wait","ms":<milliseconds>}
- task:  {"tool":"task","description":"<done or abort>"}
RULES:
1. Always use the exact ref_id from the perception (e.g., "ref_3").
2. To open an app, click its toggle button.
3. To type, click the entry field FIRST, then use the type tool.
4. When done, output {"tool":"task","description":"done"}.
EXAMPLES:
Goal: Open Terminal
Screen:   ref_3  toggle button  "Xfce Terminal"  state=has-tooltip
Action: {"tool":"click","selector":"ref_3"}
Goal: Type 'hello' in the search box
Screen:   ref_5  entry  "Search"  state=editable
Action: {"tool":"type","selector":"ref_5","text":"hello"}
Respond ONLY with a valid JSON tool call. No markdown, no reasoning."#;

const GRAMMAR: &str = include_str!("../grammar.gbnf");

const MODEL_PATH: &str = "/home/d/.laputa-secure/models/LFM2-8B-A1B-Q4_K_M.gguf";
const CONTEXT_SIZE: usize = 4096;

// ── State shared between WebSocket and agent ──────────────────────
struct AgentState {
    goal: String,
    running: bool,
}

// ── Screen reading ────────────────────────────────────────────────
async fn get_screen_state() -> String {
    let output = Command::new("python3")
        .args(["/home/d/laputa/perceive.py", "--focused"])
        .output()
        .ok();
    match output {
        Some(out) => String::from_utf8_lossy(&out.stdout).to_string(),
        None => "Screen state unavailable".to_string(),
    }
}

// ── Tool execution ────────────────────────────────────────────────
async fn execute_tool(call: &ToolCall) -> String {
    match call {
        ToolCall::Click { selector } => {
            let _ = Command::new("python3")
                .args(["-m", "tine.cli", "click", selector])
                .current_dir("/home/d/laputa/tine")
                .output();
            format!("Clicked {}", selector)
        }
        ToolCall::Type { selector, text } => {
            if selector != "body" {
                let _ = Command::new("python3")
                    .args(["-m", "tine.cli", "focus", selector])
                    .current_dir("/home/d/laputa/tine")
                    .output();
            }
            let _ = Command::new("python3")
                .args(["-m", "tine.cli", "type", text])
                .current_dir("/home/d/laputa/tine")
                .output();
            format!("Typed '{}' in {}", text, selector)
        }
        ToolCall::Key { key } => {
            let _ = Command::new("python3")
                .args(["-m", "tine.cli", "key", key])
                .current_dir("/home/d/laputa/tine")
                .output();
            format!("Pressed {}", key)
        }
        ToolCall::Wait { ms } => {
            tokio::time::sleep(tokio::time::Duration::from_millis(*ms as u64)).await;
            format!("Waited {}ms", ms)
        }
        ToolCall::Task { description } => {
            format!("Task completed: {}", description)
        }
    }
}

// ── Agent loop ────────────────────────────────────────────────────
async fn agent_loop(state: Arc<Mutex<AgentState>>, adapter: Arc<dyn InferenceAdapter>) {
    let mut enforcer = StepEnforcer::new();
    let mut memory = Memory::new(|steps| {
        let actions: Vec<_> = steps.iter()
            .filter_map(|s| s.action.as_ref())
            .map(|a| format!("{:?}", a))
            .collect();
        actions.join(", ")
    });

    let goal = state.lock().await.goal.clone();

    loop {
        {
            let s = state.lock().await;
            if !s.running { break; }
        }

        if let Err(e) = enforcer.advance() {
            println!("Agent terminated: {:?}", e);
            break;
        }

        let screen = get_screen_state().await;
        let context = memory.context_string();
        let prompt = format!(
            "{}\n\n{context}\n\nScreen:\n{screen}\n\nGoal: {goal}\n\nWhat is your next action?",
            SYSTEM_PROMPT
        );

        let adapter_clone = adapter.clone();
        let forge = Forge {
            model_fn: Box::new(move |p: String| {
                let adapter = adapter_clone.clone();
                Box::pin(async move {
                    tokio::task::spawn_blocking(move || {
                        adapter.generate(&p, 256, 0.2)
                            .map_err(|e| PipelineError::ModelError(e))
                    })
                    .await
                    .map_err(|e| PipelineError::ModelError(e.to_string()))?
                })
            }),
        };

        match forge.call_with_retry(&prompt, &enforcer).await {
            Ok(tool_call) => {
                println!("Step {}: {:?}", enforcer.step(), tool_call);
                let output = execute_tool(&tool_call).await;
                memory.push(Step {
                    index: enforcer.step(),
                    prompt: prompt.clone(),
                    output,
                    action: Some(tool_call.clone()),
                });
                if let ToolCall::Task { .. } = tool_call {
                    println!("Goal achieved.");
                    break;
                }
            }
            Err(e) => {
                println!("Pipeline error: {:?}", e);
                memory.push(Step {
                    index: enforcer.step(),
                    prompt: prompt.clone(),
                    output: format!("Error: {:?}", e),
                    action: None,
                });
                if matches!(e, PipelineError::MaxRetriesExceeded | PipelineError::MaxStepsExceeded) {
                    break;
                }
            }
        }
    }
}

// ── WebSocket server ──────────────────────────────────────────────
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let adapter: Arc<dyn InferenceAdapter> = Arc::new(
        LlamaCppAdapter::new(MODEL_PATH, CONTEXT_SIZE)
            .expect("Failed to load LFM2 model — check MODEL_PATH")
    );

    let state = Arc::new(Mutex::new(AgentState {
        goal: String::new(),
        running: false,
    }));

    let state_ws = state.clone();
    let adapter_ws = adapter.clone();

    tokio::spawn(async move {
        let listener = TcpListener::bind("127.0.0.1:9090").await.unwrap();
        println!("WebSocket server on ws://127.0.0.1:9090");
        while let Ok((stream, _)) = listener.accept().await {
            let ws = accept_async(stream).await.unwrap();
            let (_, mut receiver) = ws.split();
            let state = state_ws.clone();
            let adapter = adapter_ws.clone();
            tokio::spawn(async move {
                while let Some(msg) = receiver.next().await {
                    if let Ok(msg) = msg {
                        if let Ok(text) = msg.to_text() {
                            let mut s = state.lock().await;
                            if text.starts_with("goal:") {
                                s.goal = text[5..].trim().to_string();
                                s.running = true;
                                let state_clone = state.clone();
                                let adapter_clone = adapter.clone();
                                tokio::spawn(async move {
                                    agent_loop(state_clone, adapter_clone).await;
                                });
                            } else if text == "pause" {
                                s.running = false;
                            } else if text == "resume" {
                                s.running = true;
                            } else if text == "stop" {
                                s.running = false;
                            }
                        }
                    }
                }
            });
        }
    });

    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
}
