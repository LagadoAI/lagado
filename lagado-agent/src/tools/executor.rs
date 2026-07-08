use std::collections::HashMap;
use std::time::Duration;
use serde_json::{Map, Value};

// ── Entry point ───────────────────────────────────────────────────────────────

/// Dispatch a native Rust tool call. Returns `Some(result)` for all self-contained
/// tools (filesystem, git, system, text, web). Returns `None` for tools that need
/// subsystem access (vm_*, memory_*, screenshot) — caller handles those separately.
pub async fn dispatch(name: &str, args: &Map<String, Value>) -> Option<String> {
    let s = |key: &str| -> String {
        args.get(key).and_then(|v| v.as_str()).unwrap_or("").to_string()
    };
    let u = |key: &str, default: u64| -> u64 {
        args.get(key).and_then(|v| v.as_u64()).unwrap_or(default)
    };

    let result = match name {
        // ── Filesystem ────────────────────────────────────────────────────────
        "read_file"     => read_file(&s("path")),
        "write_file"    => write_file(&s("path"), &s("content")),
        "list_dir"      => list_dir(&s("path")),
        "search_files"  => search_files(&s("path"), &s("pattern")),
        "move_file"     => move_file(&s("src"), &s("dst")),
        "copy_file"     => copy_file(&s("src"), &s("dst")),
        "delete_file"   => delete_file(&s("path")),
        "make_dir"      => make_dir(&s("path")),
        "file_info"     => file_info(&s("path")),
        "read_multiple" => read_multiple(args),

        // ── Git ───────────────────────────────────────────────────────────────
        "git_status"   => git_run(&["status", "--short", "--branch"]),
        "git_diff"     => git_diff(&s("path")),
        "git_log"      => git_log(u("n", 10) as usize),
        "git_add"      => git_run(&["add", &s("path")]),
        "git_commit"   => git_commit(&s("message")),
        "git_branch"   => git_branch(&s("name")),
        "git_checkout" => git_run(&["checkout", &s("branch")]),
        "git_push"     => git_run(&["push"]),
        "git_pull"     => git_run(&["pull"]),

        // ── System ────────────────────────────────────────────────────────────
        "run_command"   => run_command(&s("command")).await,
        "list_processes"=> list_processes(),
        "kill_process"  => kill_process(u("pid", 0) as u32),
        "get_env"       => get_env(&s("key")),
        "set_env"       => set_env(&s("key"), &s("value")),

        // ── Text and utility ──────────────────────────────────────────────────
        "regex_search"  => regex_search(&s("pattern"), &s("text")),
        "json_query"    => json_query(&s("pointer"), &s("data")),
        "hash_file"     => hash_file(&s("path")),
        "base64_encode" => base64_encode(&s("data")),
        "base64_decode" => base64_decode(&s("data")),
        "find_replace"  => find_replace(&s("path"), &s("pattern"), &s("replacement")),

        // ── Web ───────────────────────────────────────────────────────────────
        "web_search"    => web_search(&s("query"), u("num_results", 5) as usize).await,
        "fetch_url"     => fetch_url(&s("url")).await,
        "read_webpage"  => read_webpage(&s("url")).await,
        "download_file" => download_file(&s("url"), &s("path")).await,

        // ── Clipboard (platform-native commands) ──────────────────────────────
        "read_clipboard"  => clipboard_read(),
        "write_clipboard" => clipboard_write(&s("text")),

        // ── Timeline (chronos calendar — pull-based episodic/audit recall) ─────
        "recall" => crate::chronos::recall(&s("day"), &s("from"), &s("to"), &s("query"), u("limit", 20) as usize),

        // VM, memory, and screenshot are handled by caller (need subsystem refs)
        _ => return None,
    };

    Some(result)
}

// ── Filesystem ────────────────────────────────────────────────────────────────

fn read_file(path: &str) -> String {
    if path.is_empty() { return "error: path is required".to_string(); }
    std::fs::read_to_string(path).unwrap_or_else(|e| format!("error: {e}"))
}

fn write_file(path: &str, content: &str) -> String {
    if path.is_empty() { return "error: path is required".to_string(); }
    if let Some(parent) = std::path::Path::new(path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(path, content)
        .map(|_| format!("wrote {} bytes to {path}", content.len()))
        .unwrap_or_else(|e| format!("error: {e}"))
}

fn list_dir(path: &str) -> String {
    let p = if path.is_empty() { "." } else { path };
    match std::fs::read_dir(p) {
        Ok(entries) => {
            let mut lines: Vec<String> = entries
                .filter_map(|e| e.ok())
                .map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    let meta = e.metadata().ok();
                    let kind = meta.as_ref().map(|m| if m.is_dir() { "dir" } else { "file" }).unwrap_or("?");
                    let size = meta.and_then(|m| if m.is_file() { Some(m.len()) } else { None });
                    match size {
                        Some(sz) => format!("{kind}  {name}  ({sz} bytes)"),
                        None     => format!("{kind}  {name}"),
                    }
                })
                .collect();
            lines.sort();
            lines.join("\n")
        }
        Err(e) => format!("error: {e}"),
    }
}

fn search_files(base: &str, pattern: &str) -> String {
    if pattern.is_empty() { return "error: pattern is required".to_string(); }
    let base = if base.is_empty() { "." } else { base };
    let mut matches = Vec::new();
    walk_dir(std::path::Path::new(base), pattern, &mut matches, 0);
    if matches.is_empty() {
        format!("no files matching '{pattern}' under {base}")
    } else {
        matches.join("\n")
    }
}

fn walk_dir(dir: &std::path::Path, pattern: &str, out: &mut Vec<String>, depth: usize) {
    if depth > 10 { return; } // guard against deeply nested trees
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') { continue; } // skip hidden
        if name.contains(pattern) {
            out.push(path.to_string_lossy().to_string());
            if out.len() >= 200 { return; }
        }
        if path.is_dir() {
            walk_dir(&path, pattern, out, depth + 1);
            if out.len() >= 200 { return; }
        }
    }
}

fn move_file(src: &str, dst: &str) -> String {
    if src.is_empty() || dst.is_empty() { return "error: src and dst are required".to_string(); }
    std::fs::rename(src, dst)
        .map(|_| format!("moved {src} → {dst}"))
        .unwrap_or_else(|e| format!("error: {e}"))
}

fn copy_file(src: &str, dst: &str) -> String {
    if src.is_empty() || dst.is_empty() { return "error: src and dst are required".to_string(); }
    std::fs::copy(src, dst)
        .map(|n| format!("copied {n} bytes: {src} → {dst}"))
        .unwrap_or_else(|e| format!("error: {e}"))
}

fn delete_file(path: &str) -> String {
    if path.is_empty() { return "error: path is required".to_string(); }
    std::fs::remove_file(path)
        .map(|_| format!("deleted {path}"))
        .unwrap_or_else(|e| format!("error: {e}"))
}

fn make_dir(path: &str) -> String {
    if path.is_empty() { return "error: path is required".to_string(); }
    std::fs::create_dir_all(path)
        .map(|_| format!("created {path}"))
        .unwrap_or_else(|e| format!("error: {e}"))
}

fn file_info(path: &str) -> String {
    if path.is_empty() { return "error: path is required".to_string(); }
    match std::fs::metadata(path) {
        Ok(m) => {
            let kind = if m.is_dir() { "directory" } else if m.is_symlink() { "symlink" } else { "file" };
            let modified = m.modified().ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs().to_string())
                .unwrap_or_else(|| "unknown".to_string());
            format!("type: {kind}\nsize: {} bytes\nmodified: {modified}", m.len())
        }
        Err(e) => format!("error: {e}"),
    }
}

fn read_multiple(args: &Map<String, Value>) -> String {
    let paths = match args.get("paths").and_then(|v| v.as_array()) {
        Some(arr) => arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect::<Vec<_>>(),
        None => return "error: paths (array of strings) is required".to_string(),
    };
    paths.iter()
        .map(|p| {
            let content = std::fs::read_to_string(p).unwrap_or_else(|e| format!("error: {e}"));
            format!("=== {p} ===\n{content}")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

// ── Git ───────────────────────────────────────────────────────────────────────

fn git_run(args: &[&str]) -> String {
    match std::process::Command::new("git").args(args).output() {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            if out.status.success() {
                if stdout.is_empty() { "ok".to_string() } else { stdout.trim().to_string() }
            } else {
                format!("error: {}", if stderr.is_empty() { stdout.to_string() } else { stderr.to_string() })
            }
        }
        Err(e) => format!("error: git not found or failed: {e}"),
    }
}

fn git_diff(path: &str) -> String {
    if path.is_empty() {
        git_run(&["diff", "--stat", "HEAD"])
    } else {
        git_run(&["diff", "HEAD", "--", path])
    }
}

fn git_log(n: usize) -> String {
    let n_str = n.to_string();
    git_run(&["log", "--oneline", &format!("-{n_str}")])
}

fn git_commit(message: &str) -> String {
    if message.is_empty() { return "error: message is required".to_string(); }
    git_run(&["commit", "-m", message])
}

fn git_branch(name: &str) -> String {
    if name.is_empty() { git_run(&["branch", "--list"]) }
    else { git_run(&["checkout", "-b", name]) }
}

// ── System ────────────────────────────────────────────────────────────────────

async fn run_command(command: &str) -> String {
    if command.is_empty() { return "error: command is required".to_string(); }
    let command = command.to_string();
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        tokio::task::spawn_blocking(move || {
            let (shell, flag) = if cfg!(windows) { ("cmd", "/C") } else { ("sh", "-c") };
            std::process::Command::new(shell).args([flag, &command]).output()
        }),
    )
    .await;
    match result {
        Err(_)            => "error: command timed out after 30s".to_string(),
        Ok(Err(e))        => format!("error: spawn failed: {e}"),
        Ok(Ok(Err(e)))    => format!("error: {e}"),
        Ok(Ok(Ok(out)))   => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            let status = out.status.code().unwrap_or(-1);
            let mut parts = vec![format!("exit: {status}")];
            if !stdout.is_empty() { parts.push(format!("stdout:\n{}", stdout.trim())); }
            if !stderr.is_empty() { parts.push(format!("stderr:\n{}", stderr.trim())); }
            parts.join("\n")
        }
    }
}

fn list_processes() -> String {
    let out = if cfg!(target_os = "linux") {
        std::process::Command::new("ps").args(["aux", "--no-header"]).output()
    } else {
        std::process::Command::new("ps").args(["aux"]).output()
    };
    match out {
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stdout);
            let lines: Vec<&str> = text.lines().take(50).collect();
            lines.join("\n")
        }
        Err(e) => format!("error: {e}"),
    }
}

fn kill_process(pid: u32) -> String {
    if pid == 0 { return "error: pid is required".to_string(); }
    #[cfg(unix)]
    {
        use nix::unistd::Pid;
        use nix::sys::signal::{kill, Signal};
        match kill(Pid::from_raw(pid as i32), Signal::SIGTERM) {
            Ok(_) => format!("sent SIGTERM to pid {pid}"),
            Err(e) => format!("error: {e}"),
        }
    }
    #[cfg(not(unix))]
    format!("kill_process not implemented on this platform")
}

fn get_env(key: &str) -> String {
    if key.is_empty() { return "error: key is required".to_string(); }
    std::env::var(key).unwrap_or_else(|_| format!("{key} is not set"))
}

fn set_env(key: &str, value: &str) -> String {
    if key.is_empty() { return "error: key is required".to_string(); }
    // Safety: single-threaded context from the agent loop; no concurrent env access
    unsafe { std::env::set_var(key, value); }
    format!("set {key}={value}")
}

// ── Text and utility ──────────────────────────────────────────────────────────

fn regex_search(pattern: &str, text: &str) -> String {
    if pattern.is_empty() { return "error: pattern is required".to_string(); }
    match regex::Regex::new(pattern) {
        Err(e) => format!("error: bad regex: {e}"),
        Ok(re) => {
            let matches: Vec<&str> = re.find_iter(text).map(|m| m.as_str()).collect();
            if matches.is_empty() {
                "no matches".to_string()
            } else {
                format!("{} match(es):\n{}", matches.len(), matches.join("\n"))
            }
        }
    }
}

fn json_query(pointer: &str, data: &str) -> String {
    if data.is_empty() { return "error: data is required".to_string(); }
    let v: Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(e) => return format!("error: invalid JSON: {e}"),
    };
    if pointer.is_empty() {
        return serde_json::to_string_pretty(&v).unwrap_or_else(|e| e.to_string());
    }
    let ptr = if pointer.starts_with('/') { pointer.to_string() } else { format!("/{pointer}") };
    match v.pointer(&ptr) {
        Some(r) => serde_json::to_string_pretty(r).unwrap_or_else(|e| e.to_string()),
        None    => format!("no value at pointer '{ptr}'"),
    }
}

fn hash_file(path: &str) -> String {
    if path.is_empty() { return "error: path is required".to_string(); }
    match std::fs::read(path) {
        Ok(bytes) => {
            let hash = blake3::hash(&bytes);
            format!("blake3:{hash}")
        }
        Err(e) => format!("error: {e}"),
    }
}

fn base64_encode(data: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data.as_bytes())
}

fn base64_decode(data: &str) -> String {
    use base64::Engine;
    match base64::engine::general_purpose::STANDARD.decode(data.trim()) {
        Ok(bytes) => String::from_utf8(bytes).unwrap_or_else(|_| "(binary data — not valid UTF-8)".to_string()),
        Err(e)    => format!("error: {e}"),
    }
}

fn find_replace(path: &str, pattern: &str, replacement: &str) -> String {
    if path.is_empty() || pattern.is_empty() { return "error: path and pattern are required".to_string(); }
    let content = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => return format!("error: {e}"),
    };
    let re = match regex::Regex::new(pattern) {
        Ok(r) => r,
        Err(e) => return format!("error: bad regex: {e}"),
    };
    let new_content = re.replace_all(&content, replacement);
    let count = re.find_iter(&content).count();
    std::fs::write(path, new_content.as_ref())
        .map(|_| format!("replaced {count} occurrence(s) in {path}"))
        .unwrap_or_else(|e| format!("error writing: {e}"))
}

// ── Web ───────────────────────────────────────────────────────────────────────

fn http_client() -> reqwest::Client {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("Lagado/0.1 (local-first AI agent)");

    // Env var takes priority (power-user override), then settings file.
    let proxy_url = std::env::var("LAGADO_HTTP_PROXY").ok().or_else(|| {
        let path = crate::config::data_dir().join("config/network.json");
        #[derive(serde::Deserialize)]
        struct Net { proxy_enabled: bool, proxy_type: String, proxy_host: String, proxy_port: u16 }
        std::fs::read_to_string(&path).ok()
            .and_then(|s| serde_json::from_str::<Net>(&s).ok())
            .filter(|n| n.proxy_enabled && !n.proxy_host.is_empty())
            .map(|n| format!("{}://{}:{}", n.proxy_type, n.proxy_host, n.proxy_port))
    });

    if let Some(url) = proxy_url {
        if let Ok(proxy) = reqwest::Proxy::all(&url) {
            builder = builder.proxy(proxy);
        }
    }
    builder.build().unwrap_or_default()
}

async fn web_search(query: &str, num_results: usize) -> String {
    if query.is_empty() { return "error: query is required".to_string(); }
    let n = num_results.min(10).max(1);
    let client = http_client();

    // SearXNG when configured — proper JSON search results
    if let Ok(instance) = std::env::var("LAGADO_SEARXNG_URL") {
        return searxng_search(&client, &instance, query, n).await;
    }

    // DuckDuckGo Instant Answer API — no key required
    ddg_search(&client, query, n).await
}

async fn searxng_search(client: &reqwest::Client, instance: &str, query: &str, n: usize) -> String {
    let url = format!("{instance}/search?q={}&format=json&categories=general", urlencoded(query));
    match client.get(&url).send().await {
        Err(e) => format!("error: SearXNG request failed: {e}"),
        Ok(resp) => {
            let json: Value = match resp.json().await {
                Ok(v) => v,
                Err(e) => return format!("error: parsing SearXNG response: {e}"),
            };
            let results = json["results"].as_array().map(|a| a.as_slice()).unwrap_or(&[]);
            if results.is_empty() { return format!("no results for '{query}'"); }
            results.iter().take(n)
                .filter_map(|r| {
                    let title   = r["title"].as_str()?;
                    let url     = r["url"].as_str()?;
                    let snippet = r.get("content").and_then(|c| c.as_str()).unwrap_or("").trim();
                    Some(format!("{title}\n{url}\n{snippet}"))
                })
                .collect::<Vec<_>>()
                .join("\n\n")
        }
    }
}

async fn ddg_search(client: &reqwest::Client, query: &str, n: usize) -> String {
    let url = format!(
        "https://api.duckduckgo.com/?q={}&format=json&no_html=1&no_redirect=1",
        urlencoded(query)
    );
    match client.get(&url).send().await {
        Err(e) => format!("error: DDG request failed: {e}"),
        Ok(resp) => {
            let json: Value = match resp.json().await {
                Ok(v) => v,
                Err(e) => return format!("error: parsing DDG response: {e}"),
            };
            let mut results: Vec<String> = Vec::new();

            // Definitive answer (Wikipedia-style)
            if let Some(text) = json["AbstractText"].as_str() {
                if !text.is_empty() {
                    let url = json["AbstractURL"].as_str().unwrap_or("");
                    results.push(format!("{text}\n{url}"));
                }
            }

            // Related topics as search result rows
            if let Some(topics) = json["RelatedTopics"].as_array() {
                for t in topics.iter().take(n.saturating_sub(results.len())) {
                    let text = t["Text"].as_str().unwrap_or("");
                    let url  = t["FirstURL"].as_str().unwrap_or("");
                    if !text.is_empty() && !url.is_empty() {
                        results.push(format!("{text}\n{url}"));
                    }
                }
            }

            if results.is_empty() {
                format!("no results for '{query}' (try a SearXNG instance for richer results: LAGADO_SEARXNG_URL)")
            } else {
                results.join("\n\n")
            }
        }
    }
}

async fn fetch_url(url: &str) -> String {
    if url.is_empty() { return "error: url is required".to_string(); }
    let client = http_client();
    match client.get(url).send().await {
        Err(e)   => format!("error: {e}"),
        Ok(resp) => {
            let status = resp.status().as_u16();
            match resp.text().await {
                Ok(body) => format!("HTTP {status}\n{body}"),
                Err(e)   => format!("HTTP {status} — error reading body: {e}"),
            }
        }
    }
}

async fn read_webpage(url: &str) -> String {
    if url.is_empty() { return "error: url is required".to_string(); }
    let raw = fetch_url(url).await;
    if raw.starts_with("error:") { return raw; }
    // Strip HTML tags with a simple regex and collapse whitespace
    let re_tag = regex::Regex::new(r"<[^>]+>").expect("valid regex");
    let re_ws  = regex::Regex::new(r"\s{2,}").expect("valid regex");
    let text = re_tag.replace_all(&raw, " ");
    let text = re_ws.replace_all(&text, " ");
    let text = text.trim();
    // Truncate to 6000 chars — fits 8B context budget
    if text.len() > 6000 {
        format!("{}\n\n[truncated — {} chars total]", &text[..6000], text.len())
    } else {
        text.to_string()
    }
}

async fn download_file(url: &str, path: &str) -> String {
    if url.is_empty() || path.is_empty() { return "error: url and path are required".to_string(); }
    let client = http_client();
    match client.get(url).send().await {
        Err(e)   => format!("error: {e}"),
        Ok(resp) => {
            let status = resp.status().as_u16();
            if status >= 400 { return format!("error: HTTP {status}"); }
            match resp.bytes().await {
                Err(e) => format!("error reading body: {e}"),
                Ok(bytes) => {
                    if let Some(parent) = std::path::Path::new(path).parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    std::fs::write(path, &bytes)
                        .map(|_| format!("downloaded {} bytes to {path}", bytes.len()))
                        .unwrap_or_else(|e| format!("error writing: {e}"))
                }
            }
        }
    }
}

// ── Clipboard ─────────────────────────────────────────────────────────────────

fn clipboard_read() -> String {
    let (cmd, args): (&str, &[&str]) = if cfg!(target_os = "macos") {
        ("pbpaste", &[])
    } else if cfg!(windows) {
        ("powershell", &["-Command", "Get-Clipboard"])
    } else {
        ("xclip", &["-selection", "clipboard", "-o"])
    };
    match std::process::Command::new(cmd).args(args).output() {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        Ok(_)  => try_xsel_read(),
        Err(_) => try_xsel_read(),
    }
}

fn try_xsel_read() -> String {
    match std::process::Command::new("xsel").args(["--clipboard", "--output"]).output() {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        _ => "error: clipboard read failed (install xclip or xsel on Linux)".to_string(),
    }
}

fn clipboard_write(text: &str) -> String {
    let (cmd, args): (&str, Vec<String>) = if cfg!(target_os = "macos") {
        ("pbcopy", vec![])
    } else if cfg!(windows) {
        ("powershell", vec!["-Command".to_string(), format!("Set-Clipboard -Value '{text}'")])
    } else {
        ("xclip", vec!["-selection".to_string(), "clipboard".to_string()])
    };
    let mut child = match std::process::Command::new(cmd)
        .args(&args)
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return clipboard_write_xsel(text),
    };
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(text.as_bytes());
    }
    match child.wait() {
        Ok(s) if s.success() => "clipboard updated".to_string(),
        _ => clipboard_write_xsel(text),
    }
}

fn clipboard_write_xsel(text: &str) -> String {
    let mut child = match std::process::Command::new("xsel")
        .args(["--clipboard", "--input"])
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return "error: clipboard write failed (install xclip or xsel on Linux)".to_string(),
    };
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(text.as_bytes());
    }
    child.wait()
        .map(|_| "clipboard updated".to_string())
        .unwrap_or_else(|e| format!("error: {e}"))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn urlencoded(s: &str) -> String {
    s.chars().map(|c| match c {
        'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
        ' ' => "+".to_string(),
        c => format!("%{:02X}", c as u32),
    }).collect()
}
