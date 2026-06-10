use crate::types::{PipelineError, ToolCall};
use regex::Regex;

/// Parse the first  name(key="val", key2=int)  call found in raw model output.
///
/// Uses a character-level scanner rather than regex for the outer `name(...)` boundary
/// so that arguments containing parentheses (e.g. `command="echo (hello)"` or
/// `content="def f(): pass"`) are captured correctly.
pub fn parse_bracket_tool_call(raw: &str) -> Result<ToolCall, PipelineError> {
    if let Some((name, args)) = find_bracket_call(raw) {
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
            other => {
                // Any unrecognised name → generic Invoke. Gate classifies by registry lookup.
                ToolCall::Invoke {
                    name: other.to_string(),
                    args: parse_args_to_map(args),
                }
            }
        };
        return Ok(tc);
    }

    rescue(raw).ok_or_else(|| PipelineError::ParseFailed(raw.to_string()))
}

/// Scan raw output for the first `word(...)` call. Returns `(name, args_str)`.
///
/// Correctly handles parentheses and backslash-escapes inside double-quoted strings,
/// so `run_command(command="echo (hi)")` and `write_file(content="fn f() {}")` parse
/// without truncation.
fn find_bracket_call(raw: &str) -> Option<(&str, &str)> {
    let bytes = raw.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        // Scan a potential identifier (word chars + underscore)
        let name_start = i;
        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
            i += 1;
        }
        let name_end = i;

        if name_end > name_start && i < bytes.len() && bytes[i] == b'(' {
            let name = &raw[name_start..name_end];
            let args_start = i + 1;
            i += 1; // skip opening (

            // Walk to the matching ) respecting quoted strings
            let mut depth: usize = 1;
            let mut in_quote = false;
            while i < bytes.len() && depth > 0 {
                match bytes[i] {
                    b'\\' if in_quote => { i += 1; } // skip escaped char
                    b'"' => { in_quote = !in_quote; }
                    b'(' if !in_quote => { depth += 1; }
                    b')' if !in_quote => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                i += 1;
            }

            if depth == 0 {
                return Some((name, &raw[args_start..i]));
            }
        } else if name_end == name_start {
            // Not an identifier start — skip one character
            i += 1;
        }
        // If we consumed an identifier but no `(` followed, continue from name_end
    }
    None
}

/// Parse `key="val"` and `key=integer` pairs into a JSON object map.
/// String values take priority: if a key appears as both string and int, string wins.
fn parse_args_to_map(args: &str) -> serde_json::Map<String, serde_json::Value> {
    let mut map = serde_json::Map::new();

    // String args: key="value" (value may be empty)
    let str_re = Regex::new(r#"(\w+)="([^"]*)""#).expect("valid regex");
    for cap in str_re.captures_iter(args) {
        map.insert(cap[1].to_string(), serde_json::Value::String(cap[2].to_string()));
    }

    // Integer args: key=digits (only insert if not already set by string parse)
    let int_re = Regex::new(r"(\w+)=(\d+)").expect("valid regex");
    for cap in int_re.captures_iter(args) {
        if !map.contains_key(&cap[1]) {
            if let Ok(n) = cap[2].parse::<i64>() {
                map.insert(cap[1].to_string(), serde_json::Value::Number(n.into()));
            }
        }
    }

    map
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ToolCall;

    #[test]
    fn parses_known_tools() {
        assert!(matches!(
            parse_bracket_tool_call(r#"click(selector="ref_3")"#).unwrap(),
            ToolCall::Click { .. }
        ));
        assert!(matches!(
            parse_bracket_tool_call(r#"wait(ms=500)"#).unwrap(),
            ToolCall::Wait { ms: 500 }
        ));
    }

    #[test]
    fn unknown_name_produces_invoke() {
        let tc = parse_bracket_tool_call(r#"web_search(query="rust async", num_results=5)"#).unwrap();
        if let ToolCall::Invoke { name, args } = tc {
            assert_eq!(name, "web_search");
            assert_eq!(args["query"], "rust async");
            assert_eq!(args["num_results"], 5);
        } else {
            panic!("expected Invoke");
        }
    }

    #[test]
    fn handles_paren_inside_quoted_arg() {
        // The outer scanner must not truncate at the ) inside the string
        let tc = parse_bracket_tool_call(r#"run_command(command="echo (hello)")"#).unwrap();
        if let ToolCall::Invoke { name, args } = tc {
            assert_eq!(name, "run_command");
            assert_eq!(args["command"].as_str().unwrap(), "echo (hello)");
        } else {
            panic!("expected Invoke");
        }
    }

    #[test]
    fn handles_paren_in_content_arg() {
        let tc = parse_bracket_tool_call(r#"write_file(path="/tmp/a.py", content="def f(): pass")"#).unwrap();
        if let ToolCall::Invoke { name, args } = tc {
            assert_eq!(name, "write_file");
            assert_eq!(args["content"].as_str().unwrap(), "def f(): pass");
        } else {
            panic!("expected Invoke");
        }
    }

    #[test]
    fn rescue_emits_chat_for_unknown() {
        let tc = parse_bracket_tool_call("I have no idea what to do here").unwrap();
        assert!(matches!(tc, ToolCall::Chat { .. }));
    }
}
