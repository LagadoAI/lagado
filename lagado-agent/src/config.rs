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

pub const VLM_MODEL_FILE: &str = "LFM2.5-VL-450M-F16.gguf";
pub const VLM_MMPROJ_FILE: &str = "mmproj-LFM2.5-VL-450m-F16.gguf";
pub const VLM_CONTEXT_SIZE: usize = 2048;
const VLM_PORT: u16 = 8082;

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

pub fn vlm_model_path() -> PathBuf {
    if let Some(p) = dev_override("LAGADO_VLM_MODEL_PATH") {
        return PathBuf::from(p);
    }
    data_dir().join("models").join(VLM_MODEL_FILE)
}

pub fn vlm_mmproj_path() -> PathBuf {
    if let Some(p) = dev_override("LAGADO_VLM_MMPROJ_PATH") {
        return PathBuf::from(p);
    }
    data_dir().join("models").join(VLM_MMPROJ_FILE)
}

pub fn vlm_port() -> u16 {
    std::env::var("LAGADO_VLM_PORT").ok().and_then(|s| s.parse().ok()).unwrap_or(VLM_PORT)
}

pub fn vlm_base_url() -> String {
    format!("http://{}:{}", llama_host(), vlm_port())
}

/// Shared frame path for QMP screendump output (used by capture_frame and VlmPerceptor).
pub const FRAME_PATH: &str = "/dev/shm/lagado_frame.png";

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

/// Maximum memory (bytes) the llama-server process may use.
///
/// Derived from the active model file size: a GGUF file is roughly equal to the
/// model's in-RAM weight footprint, so `file_size × 1.5` gives the weights plus
/// headroom for KV cache and IO buffers. This is model-agnostic — swap in a 3B
/// or a 70B model and the cap adjusts automatically.
///
/// Override: LAGADO_LLAMA_MEMORY_MAX_GIB (integer GiB). Fallback: 8 GiB when
/// the model file is absent (e.g. CI or first launch before download).
pub fn llama_memory_max_bytes() -> u64 {
    if let Some(gib) = std::env::var("LAGADO_LLAMA_MEMORY_MAX_GIB")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
    {
        return gib * 1024 * 1024 * 1024;
    }
    // GGUF file size ≈ weight footprint; add 50% for KV cache + overhead
    if let Ok(meta) = std::fs::metadata(model_path()) {
        let file_bytes = meta.len();
        if file_bytes > 0 {
            return file_bytes + file_bytes / 2;
        }
    }
    8 * 1024 * 1024 * 1024 // fallback: 8 GiB
}

/// Maximum memory (bytes) the classifier server process may use.
/// Override: LAGADO_CLASSIFIER_MEMORY_MAX_GIB (integer GiB).
/// Otherwise derived from the classifier model file size × 1.5, same logic as above.
pub fn classifier_memory_max_bytes() -> u64 {
    if let Some(gib) = std::env::var("LAGADO_CLASSIFIER_MEMORY_MAX_GIB")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
    {
        return gib * 1024 * 1024 * 1024;
    }
    if let Ok(meta) = std::fs::metadata(classifier_model_path()) {
        let file_bytes = meta.len();
        if file_bytes > 0 {
            return file_bytes + file_bytes / 2;
        }
    }
    2 * 1024 * 1024 * 1024 // fallback: 2 GiB
}

/// User tool configuration — trust overrides and marketplace MCP servers.
pub fn tool_config_path() -> PathBuf {
    data_dir().join("config").join("tool_config.json")
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
