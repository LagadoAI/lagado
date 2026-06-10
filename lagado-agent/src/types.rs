use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case")]
pub enum ToolCall {
    Click   { selector: String },
    Type    { selector: String, text: String },
    Key     { key: String },
    Wait    { ms: u64 },
    Task    { description: String },
    Done    { reason: String },
    Chat    { text: String },
    /// Generic invocation for all native + MCP tools. Name is looked up in ToolRegistry
    /// (Step 2) for risk classification and dispatch. Args are the parsed call arguments.
    Invoke  { name: String, args: serde_json::Map<String, serde_json::Value> },
}

#[derive(Debug, Clone)]
pub struct Step {
    pub index:  usize,
    pub prompt: String,
    pub output: String,
    pub action: Option<ToolCall>,
}

#[derive(Debug)]
pub enum PipelineError {
    ParseFailed(String),
    MaxRetriesExceeded,
    MaxStepsExceeded,
    ModelError(String),
}

/// Router decision from hydra — recorded in chronos
#[derive(Debug, Clone)]
pub struct RouterDecision {
    pub intent:    String,   // "chat" | "interactive" | "reasoning"
    pub message:   String,
    pub timestamp: i64,
}
