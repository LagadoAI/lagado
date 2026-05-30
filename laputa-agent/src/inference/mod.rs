pub mod llama_cpp;

/// Generic inference boundary — Liquid-specific tuning lives in hydra.rs, not here.
/// Any GGUF model loads through the same adapter.
pub trait InferenceAdapter: Send + Sync {
    fn generate(
        &self,
        prompt: &str,
        max_tokens: usize,
        temperature: f32,
    ) -> Result<String, String>;

    fn supports_kv_slots(&self) -> bool;
    fn save_kv_slot(&self, key: &str) -> Result<(), String>;
    fn restore_kv_slot(&self, key: &str) -> Result<bool, String>;
    fn has_kv_slot(&self, key: &str) -> bool;

    fn model_name(&self) -> &str;
    fn context_size(&self) -> usize;
}
