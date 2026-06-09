//! config.rs — Portable, cross-platform path & endpoint resolution.
//!
//! Every location resolves in priority order: env-var override → OS-appropriate
//! default (via `directories::ProjectDirs`). No absolute paths live in code.

use std::path::PathBuf;
use directories::ProjectDirs;

const DEFAULT_MODEL_FILE: &str = "LFM2.5-8B-A1B-Q4_K_M.gguf";

const LLAMA_HOST: &str = "127.0.0.1";
const LLAMA_PORT: u16 = 8080;
const WS_HOST: &str = "127.0.0.1";
const WS_PORT: u16 = 9090;

/// Model context window (tokens).
pub const CONTEXT_SIZE: usize = 32768;

fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("ai", "Lagado", "Lagado")
}

/// Base data directory (models, binaries, logs). Override: LAGADO_DATA_DIR.
pub fn data_dir() -> PathBuf {
    if let Ok(p) = std::env::var("LAGADO_DATA_DIR") {
        return PathBuf::from(p);
    }
    project_dirs()
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// GGUF model path. Override: LAGADO_MODEL_PATH.
pub fn model_path() -> PathBuf {
    if let Ok(p) = std::env::var("LAGADO_MODEL_PATH") {
        return PathBuf::from(p);
    }
    data_dir().join("models").join(DEFAULT_MODEL_FILE)
}

/// llama-server binary path. Override: LAGADO_LLAMA_SERVER.
pub fn llama_server_bin() -> PathBuf {
    if let Ok(p) = std::env::var("LAGADO_LLAMA_SERVER") {
        return PathBuf::from(p);
    }
    let name = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };
    data_dir().join("bin").join(name)
}

/// Append-only audit log path. Override: LAGADO_CHRONOS_LOG.
pub fn chronos_log() -> PathBuf {
    if let Ok(p) = std::env::var("LAGADO_CHRONOS_LOG") {
        return PathBuf::from(p);
    }
    data_dir().join("chronos.log")
}

/// Agent system prompt. Resolves: LAGADO_SYSTEM_PROMPT (file path) ->
/// `<data>/system_prompt.txt` -> the embedded default that ships with the binary.
pub fn system_prompt() -> String {
    if let Ok(p) = std::env::var("LAGADO_SYSTEM_PROMPT") {
        if let Ok(s) = std::fs::read_to_string(&p) {
            return s;
        }
    }
    let data_file = data_dir().join("system_prompt.txt");
    if let Ok(s) = std::fs::read_to_string(&data_file) {
        return s;
    }
    include_str!("../prompts/system_prompt.txt").to_string()
}

pub fn llama_host() -> String {
    std::env::var("LAGADO_LLAMA_HOST").unwrap_or_else(|_| LLAMA_HOST.to_string())
}
pub fn llama_port() -> u16 {
    std::env::var("LAGADO_LLAMA_PORT").ok().and_then(|s| s.parse().ok()).unwrap_or(LLAMA_PORT)
}
pub fn llama_base_url() -> String {
    format!("http://{}:{}", llama_host(), llama_port())
}

pub fn ws_host() -> String {
    std::env::var("LAGADO_WS_HOST").unwrap_or_else(|_| WS_HOST.to_string())
}
pub fn ws_port() -> u16 {
    std::env::var("LAGADO_WS_PORT").ok().and_then(|s| s.parse().ok()).unwrap_or(WS_PORT)
}
pub fn ws_addr() -> String {
    format!("{}:{}", ws_host(), ws_port())
}
