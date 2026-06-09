use crate::types::{PipelineError, ToolCall};
use regex::Regex;

/// Parse the first  name(key="val", key2=int)  call found in raw model output.
pub fn parse_bracket_tool_call(raw: &str) -> Result<ToolCall, PipelineError> {
    let re = Regex::new(r"(\w+)\(([^)]*)\)").expect("valid regex");
    if let Some(caps) = re.captures(raw) {
        let name = caps.get(1).unwrap().as_str();
        let args = caps.get(2).unwrap().as_str();
        let tc = match name {
            "click" => {
                let selector = str_arg(args, "selector").unwrap_or_else(|| "body".to_string());
                ToolCall::Click { selector }
            }
            "type" => {
                let selector = str_arg(args, "selector").unwrap_or_else(|| "body".to_string());
                let text = str_arg(args, "text").unwrap_or_default();
                ToolCall::Type { selector, text }
            }
            "key" => {
                let key = str_arg(args, "key").unwrap_or_else(|| "Return".to_string());
                ToolCall::Key { key }
            }
            "wait" => {
                let ms = int_arg(args, "ms").unwrap_or(1000);
                ToolCall::Wait { ms }
            }
            "done" => {
                let reason = str_arg(args, "reason").unwrap_or_else(|| "done".to_string());
                ToolCall::Done { reason }
            }
            _ => return rescue(raw).ok_or_else(|| PipelineError::ParseFailed(raw.to_string())),
        };
        return Ok(tc);
    }

    rescue(raw).ok_or_else(|| PipelineError::ParseFailed(raw.to_string()))
}

/// Best-effort rescue when no valid bracket call was found.
fn rescue(raw: &str) -> Option<ToolCall> {
    let low = raw.to_lowercase();
    if low.contains("click") {
        Some(ToolCall::Click { selector: any_quoted(raw).unwrap_or_else(|| "body".to_string()) })
    } else if low.contains("type") {
        let text = any_quoted(raw).unwrap_or_default();
        Some(ToolCall::Type { selector: "body".to_string(), text })
    } else if low.contains("key") {
        Some(ToolCall::Key { key: any_quoted(raw).unwrap_or_else(|| "Return".to_string()) })
    } else if low.contains("wait") {
        Some(ToolCall::Wait { ms: any_int(raw).unwrap_or(1000) })
    } else if low.contains("done") {
        Some(ToolCall::Done { reason: any_quoted(raw).unwrap_or_else(|| "done".to_string()) })
    } else {
        // No tool call detected — emit as conversational response
        Some(ToolCall::Chat { text: raw.to_string() })
    }
}

// ── arg extractors ────────────────────────────────────────────────────────────

fn str_arg(args: &str, key: &str) -> Option<String> {
    let pat = format!(r#"{}="([^"]*)""#, key);
    let re = Regex::new(&pat).ok()?;
    re.captures(args)?.get(1).map(|m| m.as_str().to_string())
}

fn int_arg(args: &str, key: &str) -> Option<u64> {
    let pat = format!(r"{}=(\d+)", key);
    let re = Regex::new(&pat).ok()?;
    re.captures(args)?.get(1)?.as_str().parse().ok()
}

fn any_quoted(s: &str) -> Option<String> {
    let re = Regex::new(r#""([^"]{1,})""#).ok()?;
    re.captures(s)?.get(1).map(|m| m.as_str().to_string())
}

fn any_int(s: &str) -> Option<u64> {
    let re = Regex::new(r"\d{3,5}").ok()?;
    re.find(s)?.as_str().parse().ok()
}
