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
        ToolCall::Wait { .. } | ToolCall::Done { .. } | ToolCall::Task { .. } | ToolCall::Chat { .. } => RiskTier::Read,
        ToolCall::Click { .. } | ToolCall::Key { .. } => RiskTier::Write,
        ToolCall::Type { text, .. } => {
            if is_destructive_text(text) { RiskTier::Destructive } else { RiskTier::Write }
        }
    }
}

fn is_destructive_text(text: &str) -> bool {
    let t = text.to_lowercase();
    // Shell commands that destroy data or system state
    let patterns = [
        "rm -rf", "rm -r /", "mkfs", "dd if=", "format c:",
        ":(){:|:&};:", "chmod -r 000", "> /dev/sda",
        "drop table", "drop database", "truncate table",
        "del /f /s /q", "rd /s /q",
    ];
    patterns.iter().any(|p| t.contains(p))
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
        ToolCall::Chat { text } => format!("chat(\"{}\")", text),
    }
}

/// Like `describe`, but with typed text redacted — for logs and persisted history.
pub fn describe_redacted(call: &ToolCall) -> String {
    match call {
        ToolCall::Type { selector, text } => format!(
            "type(selector=\"{}\", text=\"<redacted {} chars>\")",
            selector,
            text.chars().count()
        ),
        other => describe(other),
    }
}
