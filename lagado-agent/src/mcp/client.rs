use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use serde_json::Value;

/// Tool definition as reported by an MCP server's `tools/list` response.
#[derive(Debug, Clone)]
pub struct McpToolDef {
    pub name:         String,
    pub description:  String,
    /// JSON Schema for input validation and call construction.
    pub input_schema: Value,
}

/// Spawn an MCP server, run the MCP handshake, return its tool list, then kill it.
///
/// This is a one-shot discovery call — the subprocess is killed when done.
/// Must be called from a blocking context (use `tokio::task::spawn_blocking`).
pub fn discover_tools(cmd: &[String]) -> Result<Vec<McpToolDef>, String> {
    if cmd.is_empty() {
        return Err("MCP server command is empty".to_string());
    }

    let mut child = Command::new(&cmd[0])
        .args(&cmd[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("failed to spawn '{}': {e}", cmd[0]))?;

    let mut stdin = child.stdin.take()
        .ok_or_else(|| "no stdin pipe on MCP server".to_string())?;
    let stdout = child.stdout.take()
        .ok_or_else(|| "no stdout pipe on MCP server".to_string())?;
    let mut reader = BufReader::new(stdout);

    let result = (|| -> Result<Vec<McpToolDef>, String> {
        // 1. initialize
        let init = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "lagado", "version": "0.1"}
            }
        });
        writeln!(stdin, "{init}").map_err(|e| e.to_string())?;

        // 2. read initialize response (discard — just need it flushed)
        let mut line = String::new();
        reader.read_line(&mut line).map_err(|e| e.to_string())?;

        // 3. notifications/initialized (one-way, no response)
        let notif = serde_json::json!({
            "jsonrpc": "2.0", "method": "notifications/initialized"
        });
        writeln!(stdin, "{notif}").map_err(|e| e.to_string())?;

        // 4. tools/list
        let list_req = serde_json::json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}
        });
        writeln!(stdin, "{list_req}").map_err(|e| e.to_string())?;

        // 5. read response
        line.clear();
        reader.read_line(&mut line).map_err(|e| e.to_string())?;

        let response: Value = serde_json::from_str(line.trim())
            .map_err(|e| format!("failed to parse tools/list response: {e}"))?;

        parse_tools_list(&response)
    })();

    let _ = child.kill();
    let _ = child.wait();
    result
}

/// Parse a `tools/list` JSON-RPC response into `McpToolDef` entries.
///
/// Pure function — no I/O. Suitable for unit testing with captured payloads.
pub fn parse_tools_list(response: &Value) -> Result<Vec<McpToolDef>, String> {
    if let Some(err) = response.get("error") {
        return Err(format!("MCP server returned error: {err}"));
    }

    let tools = response["result"]["tools"]
        .as_array()
        .ok_or_else(|| "missing result.tools array in tools/list response".to_string())?;

    Ok(tools.iter().filter_map(|t| {
        let name = t["name"].as_str()?.to_string();
        let description = t.get("description")
            .and_then(|d| d.as_str())
            .unwrap_or("")
            .to_string();
        let input_schema = t.get("inputSchema").cloned().unwrap_or(Value::Null);
        Some(McpToolDef { name, description, input_schema })
    }).collect())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn filesystem_payload() -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "result": {
                "tools": [
                    {
                        "name": "read_file",
                        "description": "Read the complete contents of a file from the file system.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "path": {"type": "string", "description": "Path of the file to read"}
                            },
                            "required": ["path"]
                        }
                    },
                    {
                        "name": "write_file",
                        "description": "Create a new file or completely overwrite an existing file.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "path": {"type": "string"},
                                "content": {"type": "string"}
                            },
                            "required": ["path", "content"]
                        }
                    },
                    {
                        "name": "list_directory",
                        "description": "Get a detailed listing of all files and directories in a path.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "path": {"type": "string"}
                            },
                            "required": ["path"]
                        }
                    }
                ]
            }
        })
    }

    #[test]
    fn parses_filesystem_payload() {
        let tools = parse_tools_list(&filesystem_payload()).unwrap();
        assert_eq!(tools.len(), 3);
        assert_eq!(tools[0].name, "read_file");
        assert!(!tools[0].description.is_empty());
        assert_eq!(tools[1].name, "write_file");
        assert_eq!(tools[2].name, "list_directory");
    }

    #[test]
    fn input_schema_preserved() {
        let tools = parse_tools_list(&filesystem_payload()).unwrap();
        let schema = &tools[0].input_schema;
        assert_eq!(schema["type"].as_str().unwrap(), "object");
        assert!(schema["properties"]["path"].is_object());
    }

    #[test]
    fn empty_tools_array_is_ok() {
        let resp = json!({"jsonrpc": "2.0", "id": 2, "result": {"tools": []}});
        let tools = parse_tools_list(&resp).unwrap();
        assert!(tools.is_empty());
    }

    #[test]
    fn error_response_propagates() {
        let resp = json!({"jsonrpc": "2.0", "id": 2, "error": {"code": -32601, "message": "Method not found"}});
        assert!(parse_tools_list(&resp).is_err());
    }

    #[test]
    fn tool_without_description_uses_empty_string() {
        let resp = json!({
            "jsonrpc": "2.0", "id": 2,
            "result": {"tools": [{"name": "minimal_tool"}]}
        });
        let tools = parse_tools_list(&resp).unwrap();
        assert_eq!(tools[0].description, "");
    }
}
