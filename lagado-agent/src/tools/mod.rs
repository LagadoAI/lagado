use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use crate::gate::RiskTier;

// ── Trust and backend types ───────────────────────────────────────────────────

/// Per-tool authorization level. User-configurable; stored in tool_config.json.
///
/// The default for each built-in tool is derived from its risk tier:
///   Read → Auto, Write → Tap, Destructive → Typed.
/// User-added MCP tools always default to Tap (never Auto) — explicit opt-in required.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustLevel {
    Auto,     // gate allows without confirmation — must be explicitly set
    Tap,      // single tap/click confirm
    Typed,    // must type a confirmation phrase
    Disabled, // tool is completely blocked
}

/// Where an MCP server subprocess runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerLocation {
    Host,   // runs on the host OS — user accepted supply-chain risk
    Guest,  // runs inside QEMU guest VM — code-execution is sandboxed
}

/// How a tool's execution is backed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolBackend {
    NativeRust,
    Mcp { cmd: Vec<String>, location: ServerLocation },
}

/// One catalog entry.
#[derive(Debug, Clone)]
pub struct ToolEntry {
    pub name:        String,
    pub description: String,
    pub risk:        RiskTier,
    pub backend:     ToolBackend,
    pub enabled:     bool,
    pub trust:       TrustLevel,
}

// ── Tool registry ─────────────────────────────────────────────────────────────

/// Central tool catalog. Seeded from bundled definitions; merged with user
/// overrides from `~/.laputa-secure/config/tool_config.json` at load time.
pub struct ToolRegistry {
    entries: HashMap<String, ToolEntry>,
}

impl ToolRegistry {
    /// Build from bundled defaults, then merge user overrides.
    pub fn load() -> Self {
        let mut entries: HashMap<String, ToolEntry> = builtin_entries()
            .into_iter()
            .map(|e| (e.name.clone(), e))
            .collect();
        if let Some(cfg) = load_config_overrides() {
            apply_overrides(&mut entries, cfg);
        }
        Self { entries }
    }

    /// Risk tier for a named tool. Unknown tools default to Write (conservative).
    pub fn risk_for(&self, name: &str) -> RiskTier {
        self.entries.get(name).map(|e| e.risk.clone()).unwrap_or(RiskTier::Write)
    }

    /// Trust level for a named tool. Unknown tools default to Tap.
    pub fn trust_for(&self, name: &str) -> TrustLevel {
        self.entries.get(name).map(|e| e.trust).unwrap_or(TrustLevel::Tap)
    }

    /// Whether the tool is enabled. Unknown tools are permitted (gate still classifies).
    pub fn is_enabled(&self, name: &str) -> bool {
        self.entries.get(name).map(|e| e.enabled).unwrap_or(true)
    }

    /// All enabled entries for retrieval scoring / prompt injection.
    pub fn enabled_entries(&self) -> Vec<&ToolEntry> {
        self.entries.values().filter(|e| e.enabled).collect()
    }

    /// Catalog size (for diagnostics).
    pub fn len(&self) -> usize { self.entries.len() }
}

// ── tool_config.json schema ───────────────────────────────────────────────────

/// Persisted user configuration for tools.
#[derive(Debug, Deserialize, Serialize)]
pub struct ToolConfig {
    #[serde(default)]
    pub trust_overrides: HashMap<String, TrustLevel>,
    /// Tool names the user has explicitly disabled.
    #[serde(default)]
    pub disabled: Vec<String>,
    /// User-added MCP servers (Phase 3.6 marketplace).
    #[serde(default)]
    pub mcp_servers: Vec<McpServerConfig>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct McpServerConfig {
    pub name:     String,
    pub cmd:      Vec<String>,
    pub location: ServerLocation,
}

fn load_config_overrides() -> Option<ToolConfig> {
    let path = crate::config::tool_config_path();
    let text = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&text).ok()
}

fn apply_overrides(entries: &mut HashMap<String, ToolEntry>, cfg: ToolConfig) {
    for name in &cfg.disabled {
        if let Some(e) = entries.get_mut(name) { e.enabled = false; }
    }
    for (name, trust) in &cfg.trust_overrides {
        if let Some(e) = entries.get_mut(name) {
            e.trust = *trust;
        }
    }
    // MCP servers add new entries — backend provided by marketplace install.
    for server in &cfg.mcp_servers {
        // Tools from MCP servers are discovered via stdio tools/list at runtime (Step 3).
        // For now, register a placeholder entry so the gate can classify by name.
        let entry_name = server.name.clone();
        entries.entry(entry_name.clone()).or_insert_with(|| ToolEntry {
            name:        entry_name.clone(),
            description: format!("MCP server: {entry_name}"),
            risk:        RiskTier::Write,
            backend:     ToolBackend::Mcp {
                cmd:      server.cmd.clone(),
                location: match server.location { ServerLocation::Host => ServerLocation::Host, ServerLocation::Guest => ServerLocation::Guest },
            },
            enabled: true,
            trust:   TrustLevel::Tap, // user-added MCP: never Auto by default
        });
    }
}

// ── Built-in tool definitions ─────────────────────────────────────────────────

fn builtin_entries() -> Vec<ToolEntry> {
    use RiskTier::{Read, Write, Destructive};
    use TrustLevel::{Auto, Tap, Typed};
    use ToolBackend::NativeRust;

    // TrustLevel default derived from RiskTier: Read→Auto, Write→Tap, Destructive→Typed
    let tools: &[(&str, &str, RiskTier, TrustLevel)] = &[
        // ── Filesystem (10) ──────────────────────────────────────────────────────
        ("read_file",      "Read a file from disk and return its contents.",            Read,        Auto),
        ("write_file",     "Write text content to a file, creating it if needed.",      Write,       Tap),
        ("list_dir",       "List files and directories at a path.",                     Read,        Auto),
        ("search_files",   "Recursively find files matching a name pattern.",           Read,        Auto),
        ("move_file",      "Move or rename a file from src to dst.",                    Write,       Tap),
        ("copy_file",      "Copy a file from src to dst.",                              Write,       Tap),
        ("delete_file",    "Permanently delete a file.",                                Destructive, Typed),
        ("make_dir",       "Create a directory (and parents) at the given path.",       Write,       Tap),
        ("file_info",      "Return metadata for a file: size, modified time, type.",    Read,        Auto),
        ("read_multiple",  "Read several files at once and return a map of contents.",  Read,        Auto),

        // ── Git (9) ──────────────────────────────────────────────────────────────
        ("git_status",    "Show working-tree status of the current git repository.",         Read,        Auto),
        ("git_diff",      "Show unstaged or staged diff, optionally for a specific path.",   Read,        Auto),
        ("git_log",       "Show recent commit history (default last 10 commits).",           Read,        Auto),
        ("git_add",       "Stage files for commit.",                                         Write,       Tap),
        ("git_commit",    "Create a commit with the given message.",                         Write,       Tap),
        ("git_branch",    "List branches or create a new branch.",                           Write,       Tap),
        ("git_checkout",  "Switch to a branch or restore a file.",                           Write,       Tap),
        ("git_push",      "Push commits to the remote repository.",                          Destructive, Typed),
        ("git_pull",      "Pull and merge changes from the remote repository.",              Write,       Tap),

        // ── System (5) ──────────────────────────────────────────────────────────
        ("run_command",   "Execute a shell command on the host. Requires typed confirmation.", Destructive, Typed),
        ("list_processes","List running processes with their PIDs and names.",                 Read,        Auto),
        ("kill_process",  "Send SIGTERM to a process by PID.",                                Destructive, Typed),
        ("get_env",       "Read an environment variable.",                                    Read,        Auto),
        ("set_env",       "Set an environment variable for the current session.",             Write,       Tap),

        // ── Text and utility (6) ─────────────────────────────────────────────────
        ("regex_search",   "Search text for a regex pattern and return all matches.",         Read,  Auto),
        ("json_query",     "Run a jq-style query against a JSON string.",                     Read,  Auto),
        ("hash_file",      "Compute the Blake3 or SHA-256 hash of a file.",                   Read,  Auto),
        ("base64_encode",  "Encode bytes or a string to Base64.",                             Read,  Auto),
        ("base64_decode",  "Decode a Base64 string to bytes or UTF-8 text.",                  Read,  Auto),
        ("find_replace",   "Find and replace a pattern in a file (regex supported).",         Write, Tap),

        // ── Web (4) ──────────────────────────────────────────────────────────────
        ("web_search",    "Search the web via DuckDuckGo or SearXNG. Uses LAGADO_SEARCH_BACKEND and LAGADO_HTTP_PROXY.", Read, Auto),
        ("fetch_url",     "Fetch a URL and return the raw response body.",                    Read, Auto),
        ("read_webpage",  "Fetch a URL and return clean readable text (HTML stripped).",      Read, Auto),
        ("download_file", "Download a URL to a local file path.",                             Write, Tap),

        // ── Clipboard (2) ────────────────────────────────────────────────────────
        ("read_clipboard",  "Return the current clipboard contents.",                         Read,  Auto),
        ("write_clipboard", "Write text to the clipboard.",                                   Write, Tap),

        // ── VM tools (4) ─────────────────────────────────────────────────────────
        ("screenshot",    "Capture the current VM desktop as a PNG and return base64.",        Read,  Auto),
        ("vm_command",    "Run a shell command inside the QEMU guest VM (sandboxed).",         Write, Tap),
        ("vm_type",       "Type text at the current cursor position in the VM.",               Write, Tap),
        ("vm_click",      "Click a UI element by ref_id inside the VM.",                       Write, Tap),

        // ── Memory (4) ──────────────────────────────────────────────────────────
        ("memory_store",  "Persist a key-value pair in the agent's long-term memory.",         Write, Tap),
        ("memory_get",    "Retrieve a value from long-term memory by key.",                    Read,  Auto),
        ("memory_list",   "List all keys in long-term memory.",                                Read,  Auto),
        ("memory_delete", "Remove a key from long-term memory.",                               Write, Tap),
    ];

    tools.iter().map(|(name, desc, risk, trust)| ToolEntry {
        name:        name.to_string(),
        description: desc.to_string(),
        risk:        risk.clone(),
        backend:     NativeRust,
        enabled:     true,
        trust:       *trust,
    }).collect()
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gate::RiskTier;

    fn reg() -> ToolRegistry { ToolRegistry::load() }

    #[test]
    fn catalog_has_44_tools() {
        // Verify the full expected tool count ships
        assert_eq!(builtin_entries().len(), 44);
    }

    #[test]
    fn read_tools_are_auto_trusted() {
        let r = reg();
        for name in &["read_file", "web_search", "git_status", "list_processes"] {
            assert_eq!(r.trust_for(name), TrustLevel::Auto, "{name} should be Auto");
        }
    }

    #[test]
    fn destructive_tools_require_typed() {
        let r = reg();
        for name in &["delete_file", "run_command", "kill_process", "git_push"] {
            assert_eq!(r.trust_for(name), TrustLevel::Typed, "{name} should be Typed");
        }
    }

    #[test]
    fn unknown_tool_defaults_to_write_tap() {
        let r = reg();
        assert_eq!(r.risk_for("nonexistent_tool"), RiskTier::Write);
        assert_eq!(r.trust_for("nonexistent_tool"), TrustLevel::Tap);
    }

    #[test]
    fn json_roundtrip_trust_level() {
        assert_eq!(
            serde_json::from_str::<TrustLevel>(r#""auto""#).unwrap(),
            TrustLevel::Auto
        );
        assert_eq!(
            serde_json::to_string(&TrustLevel::Typed).unwrap(),
            r#""typed""#
        );
    }
}
