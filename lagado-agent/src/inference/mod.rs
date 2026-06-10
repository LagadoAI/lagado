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

    /// Like `generate`, but also returns a confidence score in [0.0, 1.0].
    ///
    /// Confidence is the geometric mean of per-token probabilities
    /// (exp of mean logprob). Returns 1.0 when logprobs are unavailable —
    /// callers must not gate on 1.0 (it means "no information", not "certain").
    ///
    /// Default implementation delegates to `generate` and returns 1.0.
    fn generate_with_confidence(
        &self,
        prompt: &str,
        max_tokens: usize,
        temperature: f32,
    ) -> Result<(String, f32), String> {
        self.generate(prompt, max_tokens, temperature).map(|s| (s, 1.0))
    }

    fn supports_kv_slots(&self) -> bool;
    fn save_kv_slot(&self, key: &str) -> Result<(), String>;
    fn restore_kv_slot(&self, key: &str) -> Result<bool, String>;
    fn has_kv_slot(&self, key: &str) -> bool;

    fn model_name(&self) -> &str;
    fn context_size(&self) -> usize;
}
