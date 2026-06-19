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
    // Bounded timeout: this runs at goal start and in the sleep-gate backfill. A
    // spawned-but-wedged embedder must surface as an error (→ recency floor / retry next
    // cycle), never hang the caller. Connection-refused already returns immediately.
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(15))
        .build();
    let resp = agent
        .post(&format!("{}/v1/embeddings", base_url))
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

/// Embed MANY texts in ONE request (the `/v1/embeddings` API accepts an `input` array). The sleep-gate
/// backfill previously did N sequential round-trips; this collapses them to one. Returns vectors in
/// input order (results are reordered by the response `index` field to be safe). Empty input → empty.
pub fn embed_batch(base_url: &str, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(30))
        .build();
    let resp = agent
        .post(&format!("{}/v1/embeddings", base_url))
        .set("Content-Type", "application/json")
        .send_json(json!({ "input": texts }))
        .map_err(|e| format!("embed_batch request failed: {}", e))?;

    let v: serde_json::Value = resp
        .into_json()
        .map_err(|e| format!("embed_batch parse failed: {}", e))?;

    let data = v["data"].as_array()
        .ok_or_else(|| format!("missing data[] in response: {}", v))?;

    // Place each embedding at its response `index` so order matches `texts` regardless of server order.
    let mut out: Vec<Vec<f32>> = vec![Vec::new(); texts.len()];
    for d in data {
        let idx = d["index"].as_u64().unwrap_or(0) as usize;
        let arr = d["embedding"].as_array()
            .ok_or_else(|| "missing embedding in data element".to_string())?;
        let vec: Vec<f32> = arr.iter()
            .map(|x| x.as_f64().map(|f| f as f32).ok_or_else(|| "non-numeric embedding element".to_string()))
            .collect::<Result<_, _>>()?;
        if idx < out.len() {
            out[idx] = vec;
        }
    }
    Ok(out)
}
