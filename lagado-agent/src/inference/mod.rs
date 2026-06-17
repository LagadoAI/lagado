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

    /// Like `generate_with_confidence`, but constrains decoding to a GBNF `grammar`.
    /// An empty grammar means "no constraint". The default impl ignores the grammar
    /// (so adapters that cannot constrain still compile and behave as before); real
    /// server-backed adapters override it to pass the grammar through.
    fn generate_constrained(
        &self,
        prompt: &str,
        max_tokens: usize,
        temperature: f32,
        grammar: &str,
    ) -> Result<(String, f32), String> {
        let _ = grammar;
        self.generate_with_confidence(prompt, max_tokens, temperature)
    }

    fn supports_kv_slots(&self) -> bool;
    fn save_kv_slot(&self, key: &str) -> Result<(), String>;
    fn restore_kv_slot(&self, key: &str) -> Result<bool, String>;
    fn has_kv_slot(&self, key: &str) -> bool;

    fn model_name(&self) -> &str;
    fn context_size(&self) -> usize;
}
