//! config.rs — Portable, cross-platform path & endpoint resolution.
//!
//! Every location resolves in priority order: env-var override → OS-appropriate
//! default (via `directories::ProjectDirs`). No absolute paths live in code.

use std::path::PathBuf;
use directories::ProjectDirs;

const DEFAULT_MODEL_FILE: &str = "LFM2.5-8B-A1B-Q4_K_M.gguf";
pub const CLASSIFIER_MODEL_FILE: &str = "LFM2.5-1.2B-Instruct-Q4_K_M.gguf";
pub const CLASSIFIER_CONTEXT_SIZE: usize = 512;
const CLASSIFIER_PORT: u16 = 8081;

/// Reads an env override ONLY in debug builds. Release builds ignore env-based
/// path/binary/prompt overrides to remove a code-loading / tamper surface.
#[cfg(debug_assertions)]
fn dev_override(key: &str) -> Option<String> {
    std::env::var(key).ok()
}
#[cfg(not(debug_assertions))]
fn dev_override(_key: &str) -> Option<String> {
    None
}

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
    if let Some(p) = dev_override("LAGADO_DATA_DIR") {
        return PathBuf::from(p);
    }
    project_dirs()
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// GGUF model path. Override: LAGADO_MODEL_PATH.
pub fn model_path() -> PathBuf {
    if let Some(p) = dev_override("LAGADO_MODEL_PATH") {
        return PathBuf::from(p);
    }
    data_dir().join("models").join(DEFAULT_MODEL_FILE)
}

/// Path to the 350M classifier model. Override: LAGADO_CLASSIFIER_MODEL_PATH.
pub fn classifier_model_path() -> PathBuf {
    if let Some(p) = dev_override("LAGADO_CLASSIFIER_MODEL_PATH") {
        return PathBuf::from(p);
    }
    data_dir().join("models").join(CLASSIFIER_MODEL_FILE)
}

pub fn classifier_port() -> u16 {
    std::env::var("LAGADO_CLASSIFIER_PORT").ok().and_then(|s| s.parse().ok()).unwrap_or(CLASSIFIER_PORT)
}

pub fn classifier_base_url() -> String {
    format!("http://{}:{}", llama_host(), classifier_port())
}

/// llama-server binary path. Override: LAGADO_LLAMA_SERVER.
pub fn llama_server_bin() -> PathBuf {
    if let Some(p) = dev_override("LAGADO_LLAMA_SERVER") {
        return PathBuf::from(p);
    }
    let name = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };
    data_dir().join("bin").join(name)
}

/// Append-only audit log path. Override: LAGADO_CHRONOS_LOG.
pub fn chronos_log() -> PathBuf {
    if let Some(p) = dev_override("LAGADO_CHRONOS_LOG") {
        return PathBuf::from(p);
    }
    data_dir().join("chronos.log")
}

/// Agent system prompt. Resolves: LAGADO_SYSTEM_PROMPT (file path) ->
/// `<data>/system_prompt.txt` -> the embedded default that ships with the binary.
pub fn system_prompt() -> String {
    if let Some(p) = dev_override("LAGADO_SYSTEM_PROMPT") {
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

/// Read the active model filename from data_dir/config/model.txt.
/// Falls back to DEFAULT_MODEL_FILE if the file doesn't exist.
pub fn active_model() -> String {
    if let Some(p) = dev_override("LAGADO_MODEL_PATH") {
        return p;
    }
    let config_file = data_dir().join("config").join("model.txt");
    if let Ok(s) = std::fs::read_to_string(&config_file) {
        let trimmed = s.trim().to_string();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }
    DEFAULT_MODEL_FILE.to_string()
}

/// Write the active model filename to data_dir/config/model.txt.
pub fn set_active_model(filename: &str) -> Result<(), String> {
    let config_dir = data_dir().join("config");
    std::fs::create_dir_all(&config_dir).map_err(|e| e.to_string())?;
    std::fs::write(config_dir.join("model.txt"), filename).map_err(|e| e.to_string())
}

/// List all .gguf files in the models directory.
pub fn available_models() -> Vec<String> {
    let models_dir = data_dir().join("models");
    std::fs::read_dir(&models_dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name.ends_with(".gguf") { Some(name) } else { None }
        })
        .collect()
}
