//! vlm_adapter.rs — LFM2.5-VL vision-language bridge.
//!
//! Sends the current VM frame to llama-server (port 8082) with the VL model +
//! mmproj loaded. Returns a concise visual description that augments AT-SPI2
//! element output in read_screen().
//!
//! Gracefully unavailable when the VLM server isn't running — callers check
//! is_available() and the perceptor falls back to text-only mode.

use base64::Engine;

pub struct VlmAdapter {
    base_url: String,
    available: bool,
}

impl VlmAdapter {
    /// Probe the VLM server health endpoint to confirm vision capability.
    pub fn probe(base_url: &str) -> Self {
        let available = Self::check_multimodal(base_url);
        if available {
            tracing::info!("VlmAdapter: vision available at {base_url}");
        } else {
            tracing::debug!("VlmAdapter: server not available or text-only — vision off");
        }
        Self { base_url: base_url.to_string(), available }
    }

    fn check_multimodal(base_url: &str) -> bool {
        // health check
        if ureq::get(&format!("{}/health", base_url))
            .timeout(std::time::Duration::from_millis(500))
            .call()
            .is_err()
        {
            return false;
        }
        // confirm "multimodal" capability — text-only VLM server (no mmproj) lacks it
        let models_url = format!("{}/v1/models", base_url);
        let resp = match ureq::get(&models_url)
            .timeout(std::time::Duration::from_millis(500))
            .call()
        {
            Ok(r) => r,
            Err(_) => return false,
        };
        let body: serde_json::Value = match resp.into_json() {
            Ok(v) => v,
            Err(_) => return false,
        };
        body["models"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|m| m["capabilities"].as_array())
            .map(|caps| caps.iter().any(|c| c.as_str() == Some("multimodal")))
            .unwrap_or(false)
    }

    pub fn is_available(&self) -> bool {
        self.available
    }

    /// Send a PNG frame to the VLM and get a concise screen description.
    /// Returns None if VLM is unavailable or inference fails.
    pub fn describe_screen(&self, frame_bytes: &[u8]) -> Option<String> {
        if !self.available || frame_bytes.is_empty() {
            return None;
        }

        let b64 = base64::engine::general_purpose::STANDARD.encode(frame_bytes);
        let payload = serde_json::json!({
            "messages": [{
                "role": "user",
                "content": [
                    {
                        "type": "image_url",
                        "image_url": { "url": format!("data:image/png;base64,{b64}") }
                    },
                    {
                        "type": "text",
                        "text": "Describe the UI elements and content visible on this screen. Be concise and factual."
                    }
                ]
            }],
            "max_tokens": 256,
            "temperature": 0.1
        });

        let resp = ureq::post(&format!("{}/v1/chat/completions", self.base_url))
            .set("Content-Type", "application/json")
            .send_json(payload)
            .ok()?;

        let json: serde_json::Value = resp.into_json().ok()?;
        json["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_when_server_down() {
        let adapter = VlmAdapter::probe("http://127.0.0.1:19999"); // no server here
        assert!(!adapter.is_available());
        assert_eq!(adapter.describe_screen(b"fake"), None);
    }

    #[test]
    fn describe_screen_returns_none_on_empty_bytes() {
        let adapter = VlmAdapter { base_url: "http://127.0.0.1:8082".to_string(), available: true };
        // Empty bytes should return None without hitting the server
        assert_eq!(adapter.describe_screen(b""), None);
    }
}
