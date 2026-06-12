use super::InferenceAdapter;
use std::path::Path;

pub struct LlamaCppAdapter {
    pub base_url: String,
    pub model_name: String,
    pub context_size: usize,
}

impl LlamaCppAdapter {
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
        })
    }

    /// Construct pointing at a custom base URL (e.g., classifier on port 8081).
    pub fn with_url(base_url: &str, model_name: &str, context_size: usize) -> Self {
        Self {
            base_url: base_url.to_string(),
            model_name: model_name.to_string(),
            context_size,
        }
    }
}

impl InferenceAdapter for LlamaCppAdapter {
    fn generate(&self, prompt: &str, max_tokens: usize, temperature: f32) -> Result<String, String> {
        let body = serde_json::json!({
            "messages": [{ "role": "user", "content": prompt }],
            "temperature": temperature,
            "top_k": 80,
            "repeat_penalty": 1.05,
            "max_tokens": max_tokens,
            "stream": false,
            "cache_prompt": true
        });

        let response = ureq::post(&format!("{}/v1/chat/completions", self.base_url))
            .set("Content-Type", "application/json")
            .send_json(body)
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        let json: serde_json::Value = response
            .into_json()
            .map_err(|e| format!("Failed to parse response JSON: {}", e))?;

        json["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| format!("Missing choices[0].message.content in response: {}", json))
    }

    fn generate_with_confidence(&self, prompt: &str, max_tokens: usize, temperature: f32) -> Result<(String, f32), String> {
        let body = serde_json::json!({
            "messages": [{ "role": "user", "content": prompt }],
            "temperature": temperature,
            "top_k": 80,
            "repeat_penalty": 1.05,
            "max_tokens": max_tokens,
            "stream": false,
            "logprobs": true,
            "cache_prompt": true
        });

        let response = ureq::post(&format!("{}/v1/chat/completions", self.base_url))
            .set("Content-Type", "application/json")
            .send_json(body)
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        let json: serde_json::Value = response
            .into_json()
            .map_err(|e| format!("Failed to parse response JSON: {}", e))?;

        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| format!("Missing choices[0].message.content in response: {}", json))?;

        // Compute geometric mean of per-token probabilities from logprobs.
        // Falls back to 1.0 (no gating) if the server doesn't return logprobs.
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
