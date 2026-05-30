mod action_graph;
use reqwest::Client;
use std::process::Command;
use tokio::net::TcpListener;
use tokio_tungstenite::accept_async;
use futures_util::StreamExt;
use std::sync::Arc;
use tokio::sync::Mutex;

mod types;
mod parser;
mod forge;
mod operator;
mod memory;

use types::{PipelineError, Step, ToolCall};
use forge::Forge;
use operator::StepEnforcer;

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

use memory::Memory;

// ── State shared between WebSocket and agent ──────────────────────
struct AgentState {
    goal: String,
    running: bool,
}

const QWEN_URL: &str = "http://127.0.0.1:8080/v1/chat/completions";
const GRAMMAR: &str = include_str!("../grammar.gbnf");

// ── Model call wrapper ────────────────────────────────────────────
// Takes an owned String to avoid lifetime issues inside the Forge closure.
async fn call_qwen(prompt: String) -> Result<String, PipelineError> {
    let client = Client::new();
    let payload = serde_json::json!({
        "model": "Qwen3-8B-ShiningValiant3.IQ4_XS.gguf",
        "messages": [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": prompt}
        ],
        "max_tokens": 256,
        "temperature": 0.2,
        "grammar": GRAMMAR
        // NOTE: "stop": ["}"] was removed — it prematurely terminates valid JSON objects.
    });

    let resp = client.post(QWEN_URL)
        .json(&payload)
        .send()
        .await
        .map_err(|e| PipelineError::ModelError(e.to_string()))?;

    let body: serde_json::Value = resp.json().await
        .map_err(|e| PipelineError::ModelError(e.to_string()))?;
    let content = body["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| PipelineError::ModelError("No content in response".into()))?;
    Ok(content.to_string())
}

// ── Screen reading ────────────────────────────────────────────────
async fn get_screen_state() -> String {
    let output = Command::new("python3")
        .args(["/home/d/laputa/scripts/perceive.py", "--focused"])
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
            let secs = *ms as f64 / 1000.0;
            tokio::time::sleep(tokio::time::Duration::from_secs_f64(secs)).await;
            format!("Waited {}ms", ms)
        }
        ToolCall::Task { description } => {
            format!("Task completed: {}", description)
        }
    }
}

// ── Agent loop (uses Forge, StepEnforcer, Memory) ─────────────────
async fn agent_loop(state: Arc<Mutex<AgentState>>) {
    let mut enforcer = StepEnforcer::new();
    let mut memory = Memory::new(|steps| {
        let actions: Vec<_> = steps.iter()
            .filter_map(|s| s.action.as_ref())
            .map(|a| format!("{:?}", a))
            .collect();
        actions.join(", ")
    });

    let goal = {
        state.lock().await.goal.clone()
    };

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
        let prompt = format!("{context}\n\nScreen:\n{screen}\n\nGoal: {goal}\n\nWhat is your next action?");

        // FIXED: closure now takes String and passes it by value, avoiding the
        // `call_qwen(&p)` borrow-of-local-variable lifetime error.
        let forge = Forge {
            model_fn: Box::new(|p: String| {
                Box::pin(call_qwen(p))
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

    let state = Arc::new(Mutex::new(AgentState {
        goal: String::new(),
        running: false,
    }));

    let state_ws = state.clone();
    tokio::spawn(async move {
        let listener = TcpListener::bind("127.0.0.1:9090").await.unwrap();
        println!("WebSocket server on ws://127.0.0.1:9090");
        while let Ok((stream, _)) = listener.accept().await {
            let ws = accept_async(stream).await.unwrap();
            let (_, mut receiver) = ws.split();
            let state = state_ws.clone();
            tokio::spawn(async move {
                while let Some(msg) = receiver.next().await {
                    if let Ok(msg) = msg {
                        if let Ok(text) = msg.to_text() {
                            let mut s = state.lock().await;
                            if text.starts_with("goal:") {
                                s.goal = text[5..].trim().to_string();
                                s.running = true;
                                let state_clone = state.clone();
                                tokio::spawn(async move {
                                    agent_loop(state_clone).await;
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
