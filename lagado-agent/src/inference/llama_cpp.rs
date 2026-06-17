use super::InferenceAdapter;
use std::path::Path;

pub struct LlamaCppAdapter {
    pub base_url: String,
    pub model_name: String,
    pub context_size: usize,
    /// top_k nucleus sampling; 0 = disabled (use min_p instead)
    top_k: u32,
    /// min_p nucleus sampling; 0.0 = disabled (use top_k instead)
    min_p: f32,
}

impl LlamaCppAdapter {
    /// Main model adapter (LFM2 gen2): min_p nucleus, no top_k.
    pub fn new(model_path: &str, context_size: usize) -> Result<Self, String> {
        let model_name = Path::new(model_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(model_path)
            .to_string();

        Ok(LlamaCppAdapter {
            base_url: crate::config::llama_base_url(),
            model_name,
            context_size,
            top_k: 0,
            min_p: crate::config::GEN2_MIN_P,
        })
    }

    /// Generic URL adapter — defaults to gen2.5 sampling (top_k=50, no min_p).
    /// Use for the classifier (LFM2.5-1.2B-Instruct) and test harnesses.
    pub fn with_url(base_url: &str, model_name: &str, context_size: usize) -> Self {
        Self {
            base_url: base_url.to_string(),
            model_name: model_name.to_string(),
            context_size,
            top_k: crate::config::GEN25_TOP_K,
            min_p: 0.0,
        }
    }

    /// Build + POST a chat-completions request and return the parsed JSON body.
    /// `grammar` empty = unconstrained; `logprobs` toggles per-token logprobs.
    /// Single source of truth for request shape so the public methods can never
    /// drift in their sampling params (they did once — a stale hardcoded top_k=80).
    fn request(
        &self,
        prompt: &str,
        max_tokens: usize,
        temperature: f32,
        grammar: &str,
        logprobs: bool,
    ) -> Result<serde_json::Value, String> {
        let mut body = serde_json::json!({
            "messages": [{ "role": "user", "content": prompt }],
            "temperature": temperature,
            "repeat_penalty": crate::config::REPEAT_PENALTY,
            "max_tokens": max_tokens,
            "stream": false,
            "cache_prompt": true
        });
        if self.top_k > 0 { body["top_k"] = serde_json::json!(self.top_k); }
        if self.min_p > 0.0 { body["min_p"] = serde_json::json!(self.min_p); }
        if logprobs { body["logprobs"] = serde_json::json!(true); }
        if !grammar.is_empty() { body["grammar"] = serde_json::json!(grammar); }

        let response = ureq::post(&format!("{}/v1/chat/completions", self.base_url))
            .set("Content-Type", "application/json")
            .send_json(body)
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        response
            .into_json()
            .map_err(|e| format!("Failed to parse response JSON: {}", e))
    }
}

impl InferenceAdapter for LlamaCppAdapter {
    fn generate(&self, prompt: &str, max_tokens: usize, temperature: f32) -> Result<String, String> {
        let json = self.request(prompt, max_tokens, temperature, "", false)?;
        extract_content(&json)
    }

    fn generate_with_confidence(&self, prompt: &str, max_tokens: usize, temperature: f32) -> Result<(String, f32), String> {
        // Unconstrained is just a constrained call with an empty grammar.
        self.generate_constrained(prompt, max_tokens, temperature, "")
    }

    fn generate_constrained(&self, prompt: &str, max_tokens: usize, temperature: f32, grammar: &str) -> Result<(String, f32), String> {
        let json = self.request(prompt, max_tokens, temperature, grammar, true)?;
        let content = extract_content(&json)?;
        // Geometric mean of per-token probabilities; 1.0 when logprobs absent (no gating).
        let confidence = compute_confidence(&json["choices"][0]["logprobs"]);
        Ok((content, confidence))
    }

    fn supports_kv_slots(&self) -> bool { false }
    fn save_kv_slot(&self, _key: &str) -> Result<(), String> { Ok(()) }
    fn restore_kv_slot(&self, _key: &str) -> Result<bool, String> { Ok(false) }
    fn has_kv_slot(&self, _key: &str) -> bool { false }
    fn model_name(&self) -> &str { &self.model_name }
    fn context_size(&self) -> usize { self.context_size }
}

/// Pull the assistant message text out of a chat-completions response.
fn extract_content(json: &serde_json::Value) -> Result<String, String> {
    json["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| format!("Missing choices[0].message.content in response: {}", json))
}

/// Geometric mean of per-token probabilities from a logprobs block.
/// Returns 1.0 when logprobs are absent (no information → don't gate).
fn compute_confidence(logprobs: &serde_json::Value) -> f32 {
    let content = match logprobs.get("content").and_then(|c| c.as_array()) {
        Some(arr) if !arr.is_empty() => arr,
        _ => return 1.0,
    };
    let sum: f64 = content.iter()
        .filter_map(|entry| entry["logprob"].as_f64())
        .sum();
    let count = content.len() as f64;
    (sum / count).exp() as f32   // geometric mean: exp(mean log-prob)
}
