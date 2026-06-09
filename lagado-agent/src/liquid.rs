//! liquid.rs — Liquid AI model roster and selection.
//!
//! Manages the available LFM2.5 model variants and selects the right one
//! for each task based on capability tier and latency budget.
//! Phase 1: stub. Phase 2: multi-model support with dynamic loading.

use crate::governor::ServerConfig;

#[derive(Debug, Clone, PartialEq)]
pub enum ModelId {
    Lfm25_350M,
    Lfm25_1B,
    Lfm25_8B,
    Lfm25_Vl,   // vision-language
}

impl ModelId {
    pub fn param_count_b(&self) -> f32 {
        match self {
            ModelId::Lfm25_350M => 0.35,
            ModelId::Lfm25_1B   => 1.2,
            ModelId::Lfm25_8B   => 8.0,
            ModelId::Lfm25_Vl   => 0.45,
        }
    }

    pub fn filename(&self) -> &'static str {
        match self {
            ModelId::Lfm25_350M => "LFM2.5-350M-Q4_K_M.gguf",
            ModelId::Lfm25_1B   => "LFM2.5-1.2B-Instruct-Q4_K_M.gguf",
            ModelId::Lfm25_8B   => "LFM2.5-8B-A1B-Q4_K_M.gguf",
            ModelId::Lfm25_Vl   => "LFM2.5-VL-450M-F16.gguf",
        }
    }
}

/// Select the best model for an intent given hardware capabilities.
pub fn select_model(intent: &str, cfg: &ServerConfig) -> ModelId {
    // Phase 2: dynamic selection per intent + latency budget
    // Phase 1: always use the 8B (already loaded)
    let _ = (intent, cfg);
    ModelId::Lfm25_8B
}

/// Vision pipeline entry point (stub).
pub fn vision_available() -> bool {
    false // Phase 2: check if VL model is loaded
}
