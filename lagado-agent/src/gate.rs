use crate::tools::{TrustLevel, ToolRegistry};
use crate::types::ToolCall;

#[derive(Debug, Clone, PartialEq)]
pub enum RiskTier {
    Read,
    Write,
    Destructive,
}

#[derive(Debug, Clone, PartialEq)]
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

pub fn evaluate_action(call: &ToolCall, registry: &ToolRegistry) -> Verdict {
    if let ToolCall::Invoke { name, args } = call {
        // Destructive arg content is a hard override — cannot be bypassed by trust level.
        // Catches `run_command(command="rm -rf /")` even when that tool is set to Auto.
        let has_destructive_arg = args.values()
            .filter_map(|v| v.as_str())
            .any(is_destructive_text);
        if has_destructive_arg {
            return Verdict::ConfirmTyped;
        }

        return match registry.trust_for(name) {
            TrustLevel::Disabled => Verdict::Block(format!("Tool '{name}' is disabled")),
            TrustLevel::Auto     => Verdict::Allow,
            TrustLevel::Tap      => Verdict::ConfirmTap,
            TrustLevel::Typed    => Verdict::ConfirmTyped,
        };
    }

    // Non-Invoke tools: use static risk classification
    match classify(call) {
        RiskTier::Read        => Verdict::Allow,
        RiskTier::Write       => Verdict::ConfirmTap,
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

const CONFIDENCE_LOW: f32  = 0.30; // below this: always ConfirmTyped
const CONFIDENCE_MID: f32  = 0.60; // below this: escalate one tier

/// Apply a confidence-based escalation on top of the base verdict.
///
/// The model's geometric-mean token probability (from logprobs) is used as a
/// proxy for certainty. Low-confidence actions surface to the human regardless
/// of the tool's normal risk tier:
///   < 0.30  → ConfirmTyped (very uncertain — human must actively confirm)
///   < 0.60  → escalate one tier (Allow→Tap, Tap→Typed)
///   ≥ 0.60  → pass through unchanged
///   = 1.0   → no logprob data (adapter doesn't support it) — pass through
///
/// The 1.0 sentinel means "no information" and is explicitly not gated, so
/// adapters without logprob support are never blocked by this function.
pub fn confidence_escalate(verdict: Verdict, confidence: f32) -> Verdict {
    // Block is a hard stop — confidence cannot lift it
    if matches!(verdict, Verdict::Block(_)) { return verdict; }
    if confidence == 1.0 { return verdict; } // no logprob data — don't interfere
    if confidence < CONFIDENCE_LOW { return Verdict::ConfirmTyped; }
    if confidence < CONFIDENCE_MID {
        return match verdict {
            Verdict::Allow       => Verdict::ConfirmTap,
            Verdict::ConfirmTap  => Verdict::ConfirmTyped,
            other                => other,
        };
    }
    verdict
}

fn is_sensitive_key(key: &str) -> bool {
    matches!(key, "content" | "text" | "data" | "body" | "password"
                | "token" | "key" | "secret" | "api_key" | "value")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn high_confidence_passes_through() {
        assert_eq!(confidence_escalate(Verdict::Allow,      0.9), Verdict::Allow);
        assert_eq!(confidence_escalate(Verdict::ConfirmTap, 0.7), Verdict::ConfirmTap);
    }

    #[test]
    fn mid_confidence_escalates_one_tier() {
        // Allow → Tap, Tap → Typed
        assert!(matches!(confidence_escalate(Verdict::Allow,      0.5), Verdict::ConfirmTap));
        assert!(matches!(confidence_escalate(Verdict::ConfirmTap, 0.4), Verdict::ConfirmTyped));
    }

    #[test]
    fn low_confidence_forces_typed() {
        assert!(matches!(confidence_escalate(Verdict::Allow,      0.1), Verdict::ConfirmTyped));
        assert!(matches!(confidence_escalate(Verdict::ConfirmTap, 0.2), Verdict::ConfirmTyped));
    }

    #[test]
    fn sentinel_1_0_is_never_gated() {
        // 1.0 means "no logprob data" — must not gate anything
        assert_eq!(confidence_escalate(Verdict::Allow, 1.0), Verdict::Allow);
    }

    #[test]
    fn block_verdict_never_escalated() {
        let blocked = Verdict::Block("reason".to_string());
        // Block stays Block regardless of confidence
        match confidence_escalate(blocked, 0.1) {
            Verdict::Block(_) => {}
            other => panic!("expected Block, got {:?}", other),
        }
    }
}
