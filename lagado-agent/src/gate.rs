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
        // Scan all string-valued args for destructive patterns before trusting the name.
        // Default to Write (tap-confirm) until ToolRegistry classifies by name in Step 2.
        ToolCall::Invoke { args, .. } => {
            let has_destructive_arg = args.values()
                .filter_map(|v| v.as_str())
                .any(is_destructive_text);
            if has_destructive_arg { RiskTier::Destructive } else { RiskTier::Write }
        }
    }
}

pub fn is_destructive_text(text: &str) -> bool {
    let t = text.to_lowercase();
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
        ToolCall::Invoke { name, args } => {
            let pairs: Vec<String> = args.iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect();
            format!("{}({})", name, pairs.join(", "))
        }
    }
}

/// Like `describe`, but with potentially sensitive arg values redacted for logs.
/// Redacts values for known sensitive keys; shows key names so the log stays useful.
/// Step 2 will replace this with ToolRegistry sensitivity annotations.
pub fn describe_redacted(call: &ToolCall) -> String {
    match call {
        ToolCall::Type { selector, text } => format!(
            "type(selector=\"{}\", text=\"<redacted {} chars>\")",
            selector,
            text.chars().count()
        ),
        ToolCall::Invoke { name, args } => {
            let pairs: Vec<String> = args.iter()
                .map(|(k, v)| {
                    if is_sensitive_key(k) {
                        format!("{}=<redacted>", k)
                    } else {
                        format!("{}={}", k, v)
                    }
                })
                .collect();
            format!("{}({})", name, pairs.join(", "))
        }
        other => describe(other),
    }
}

fn is_sensitive_key(key: &str) -> bool {
    matches!(key, "content" | "text" | "data" | "body" | "password"
                | "token" | "key" | "secret" | "api_key" | "value")
}
