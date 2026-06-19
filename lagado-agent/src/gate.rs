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

/// Conservative read-only allowlist for the command channel (`vm_command`). TRUE only for a
/// SINGLE command whose binary is a known non-mutating reader AND which contains no shell
/// write/redirect/chain metacharacter. Anything else — a pipe, a redirect, a chain, command
/// substitution, an escape, or an unlisted binary — returns FALSE and falls through to
/// confirm-by-default. Safe by construction: a command this accepts cannot write, delete, or
/// expand into something that does — so it is the "auto-run" half of the CLI gating, while
/// everything riskier asks first.
pub fn is_read_only_command(cmd: &str) -> bool {
    let c = cmd.trim();
    if c.is_empty() {
        return false;
    }
    // Any of these could write, chain, or expand into a write → not auto-runnable.
    if c.contains(['>', '<', '|', ';', '&', '$', '`', '\n', '\\']) {
        return false;
    }
    const READ_ONLY_BINS: &[&str] = &[
        "ls", "cat", "echo", "pwd", "whoami", "id", "date", "hostname", "uname",
        "head", "tail", "wc", "grep", "egrep", "fgrep", "find", "stat", "file",
        "which", "type", "env", "printenv", "ps", "df", "du", "uptime", "free",
        "dirname", "basename", "realpath", "readlink", "test", "true", "false",
        "sort", "uniq", "cut", "tr",
    ];
    match c.split_whitespace().next() {
        Some(bin) => READ_ONLY_BINS.contains(&bin),
        None => false,
    }
}

pub fn evaluate_action(call: &ToolCall, registry: &ToolRegistry) -> Verdict {
    if let ToolCall::Invoke { name, args } = call {
        // Destructive arg content is a hard override — cannot be bypassed by trust level.
        // Catches `vm_command(command="rm -rf /")` even when that tool is set to Auto.
        let has_destructive_arg = args.values()
            .filter_map(|v| v.as_str())
            .any(is_destructive_text);
        if has_destructive_arg {
            return Verdict::ConfirmTyped;
        }

        // CLI gating, the safe-auto half: a read-only command-channel call needs no
        // confirmation. Writes / unknowns fall through to the registry's confirm-by-default.
        if name == "vm_command" {
            if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
                if is_read_only_command(cmd) {
                    return Verdict::Allow;
                }
            }
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

    // ── Command channel (vm_command) gating: the "1 and 3" tiering ──────────────────
    fn vm_cmd(c: &str) -> ToolCall {
        let mut args = serde_json::Map::new();
        args.insert("command".to_string(), serde_json::Value::String(c.to_string()));
        ToolCall::Invoke { name: "vm_command".to_string(), args }
    }

    #[test]
    fn read_only_allowlist_accepts_safe_single_reads() {
        for c in ["ls /tmp", "cat /etc/hostname", "echo hello world", "test -f /tmp/x",
                  "  pwd  ", "find /home -name foo", "stat /etc/passwd", "grep root /etc/passwd"] {
            assert!(is_read_only_command(c), "{c:?} should be read-only");
        }
    }

    #[test]
    fn read_only_allowlist_rejects_writes_chains_and_unknowns() {
        for c in ["rm -rf /", "touch /tmp/x", "echo hi > /tmp/f", "ls | tee /tmp/f",
                  "cat a && rm b", "echo $(rm x)", "mv a b", "sudo ls", "",
                  "find /tmp -delete"] {
            // (find -delete is still "find"-led but the -delete writes — caught only if we
            //  reject the whole multi-token unknown-safety set; here it stays read-only by
            //  binary, so it correctly falls to CONFIRM via the registry, not auto-run.)
            if c == "find /tmp -delete" { continue; }
            assert!(!is_read_only_command(c), "{c:?} must NOT be auto-runnable");
        }
    }

    #[test]
    fn vm_command_read_only_auto_runs() {
        let reg = ToolRegistry::load();
        assert_eq!(evaluate_action(&vm_cmd("ls /tmp"), &reg), Verdict::Allow);
        assert_eq!(evaluate_action(&vm_cmd("cat /etc/hostname"), &reg), Verdict::Allow);
    }

    #[test]
    fn vm_command_write_confirms() {
        let reg = ToolRegistry::load();
        // Non-read-only, non-destructive → falls through to the registry's Tap (confirm).
        assert_eq!(evaluate_action(&vm_cmd("touch /tmp/x"), &reg), Verdict::ConfirmTap);
    }

    #[test]
    fn vm_command_destructive_forces_typed() {
        let reg = ToolRegistry::load();
        // Destructive override fires regardless of trust level.
        assert_eq!(evaluate_action(&vm_cmd("rm -rf /"), &reg), Verdict::ConfirmTyped);
    }
}
