use crate::types::{PipelineError, ToolCall};
use regex::Regex;

/// Primary: extract first valid JSON object from raw output.
/// Falls back to rescue_parse on failure.
pub fn parse_tool_call(raw: &str) -> Result<ToolCall, PipelineError> {
    // 1. Try to find a JSON object anywhere in the output
    if let Some(json_str) = extract_json_object(raw) {
        if let Ok(tool) = serde_json::from_str::<ToolCall>(&json_str) {
            return Ok(tool);
        }
    }
    // 2. Fall back to rescue parsing
    rescue_parse(raw).ok_or_else(|| PipelineError::ParseFailed(raw.to_string()))
}

/// Scan raw text for a known tool name, build minimal valid JSON.
pub fn rescue_parse(raw: &str) -> Option<ToolCall> {
    let tools = ["click", "type", "key", "wait", "task"];
    let raw_lower = raw.to_lowercase();
    let matched = tools.iter().find(|&&t| raw_lower.contains(t))?;

    match *matched {
        "click" => {
            let selector = extract_quoted_value(raw, "selector")
                .or_else(|| extract_first_quoted(raw))
                .unwrap_or_else(|| "body".to_string());
            Some(ToolCall::Click { selector })
        }
        "type" => {
            let text = extract_quoted_value(raw, "text")
                .or_else(|| extract_first_quoted(raw))
                .unwrap_or_default();
            let selector = extract_quoted_value(raw, "selector")
                .unwrap_or_else(|| "body".to_string());
            Some(ToolCall::Type { selector, text })
        }
        "key" => {
            let key = extract_quoted_value(raw, "key")
                .or_else(|| extract_first_quoted(raw))
                .unwrap_or_else(|| "Return".to_string());
            Some(ToolCall::Key { key })
        }
        "wait" => {
            let ms = extract_number(raw).unwrap_or(1000);
            Some(ToolCall::Wait { ms })
        }
        "task" => {
            let description = extract_quoted_value(raw, "description")
                .or_else(|| extract_first_quoted(raw))
                .unwrap_or_else(|| "continue".to_string());
            Some(ToolCall::Task { description })
        }
        _ => None,
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn extract_json_object(s: &str) -> Option<String> {
    let start = s.find('{')?;
    let mut depth = 0usize;
    for (i, ch) in s[start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(s[start..start + i + 1].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

fn extract_quoted_value(s: &str, key: &str) -> Option<String> {
    let pattern = format!(r#""{}"\s*:\s*"([^"]+)""#, key);
    let re = Regex::new(&pattern).ok()?;
    re.captures(s)?.get(1).map(|m| m.as_str().to_string())
}

fn extract_first_quoted(s: &str) -> Option<String> {
    let re = Regex::new(r#""([^"]{2,})""#).ok()?;
    re.captures(s)?.get(1).map(|m| m.as_str().to_string())
}

fn extract_number(s: &str) -> Option<u64> {
    let re = Regex::new(r"\d{3,5}").ok()?; // 3‑5 digit number → plausible ms
    re.find(s)?.as_str().parse().ok()
}
