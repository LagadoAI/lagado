use crate::types::ToolCall;

#[derive(Debug, Clone, PartialEq)]
pub enum RiskTier {
    Read,
    Write,
    Destructive,
}

#[derive(Debug, Clone)]
pub enum Verdict {
    Allow,
    ConfirmTap,
    ConfirmTyped,
    Block(String),
}

pub fn classify(call: &ToolCall) -> RiskTier {
    match call {
        ToolCall::Wait { .. } | ToolCall::Done { .. } | ToolCall::Task { .. } => RiskTier::Read,
        ToolCall::Click { .. } | ToolCall::Key { .. } | ToolCall::Type { .. } => RiskTier::Write,
    }
}

pub fn evaluate_action(call: &ToolCall) -> Verdict {
    match classify(call) {
        RiskTier::Read => Verdict::Allow,
        RiskTier::Write => Verdict::ConfirmTap,
        RiskTier::Destructive => Verdict::ConfirmTyped,
    }
}

pub fn describe(call: &ToolCall) -> String {
    match call {
        ToolCall::Click { selector } => format!("click(selector=\"{}\")", selector),
        ToolCall::Type { selector, text } => format!("type(selector=\"{}\", text=\"{}\")", selector, text),
        ToolCall::Key { key } => format!("key(key=\"{}\")", key),
        ToolCall::Wait { ms } => format!("wait(ms={})", ms),
        ToolCall::Done { reason } => format!("done(reason=\"{}\")", reason),
        ToolCall::Task { description } => format!("task(description=\"{}\")", description),
    }
}
