//! embedding.rs — text embedding client (LFM2-ColBERT-350M via llama-server, mean-pooled).
//!
//! The Board's relevance signal. Pooled-vector cosine (NOT late-interaction MaxSim);
//! see G3_RESULTS.md — mean pooling cleared the Jaccard floor (F1 0.43→0.52, recall
//! 0.75→0.92). The model is swappable behind this seam, like the inference adapter.

use serde_json::json;

/// Embed a single text into a pooled f32 vector via the configured embedding server.
pub fn embed(text: &str) -> Result<Vec<f32>, String> {
    embed_at(&crate::config::embed_base_url(), text)
}

/// Embed against an explicit base URL (test harnesses / non-default ports).
pub fn embed_at(base_url: &str, text: &str) -> Result<Vec<f32>, String> {
    let resp = ureq::post(&format!("{}/v1/embeddings", base_url))
        .set("Content-Type", "application/json")
        .send_json(json!({ "input": text }))
        .map_err(|e| format!("embed request failed: {}", e))?;

    let v: serde_json::Value = resp
        .into_json()
        .map_err(|e| format!("embed parse failed: {}", e))?;

    let arr = v["data"][0]["embedding"]
        .as_array()
        .ok_or_else(|| format!("missing data[0].embedding in response: {}", v))?;

    arr.iter()
        .map(|x| x.as_f64().map(|f| f as f32).ok_or_else(|| "non-numeric embedding element".to_string()))
        .collect()
}
