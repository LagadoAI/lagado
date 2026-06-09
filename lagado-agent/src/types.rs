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
