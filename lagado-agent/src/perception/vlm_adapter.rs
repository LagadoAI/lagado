//! vlm_adapter.rs — LFM2.5-VL vision-language bridge.
//!
//! Phase 1: stub. Phase 2: sends changed-region screenshots to llama-server
//! with LFM2.5-VL loaded, receives visual understanding tokens.
//! Uses Liquid's shipped SigLIP2 + 2-layer MLP projector (no fine-tuning needed).

pub struct VlmAdapter {
    base_url: String,
    available: bool,
}

impl VlmAdapter {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
            available: false, // Phase 2: probe llama-server for VL model
        }
    }

    /// Check if the VL model is loaded and available.
    pub fn is_available(&self) -> bool {
        self.available
    }

    /// Send an image region to the VL model and get a text description.
    /// Phase 1: returns placeholder. Phase 2: multipart POST to llama-server.
    pub fn describe_region(&self, _image_bytes: &[u8], prompt: &str) -> Result<String, String> {
        if !self.available {
            tracing::debug!("VlmAdapter: VL model not loaded (Phase 2)");
            return Ok(format!("[vision unavailable] {prompt}"));
        }
        // Phase 2: POST image + prompt to {base_url}/v1/chat/completions with vision payload
        Err("VL model not loaded".to_string())
    }

    /// Process changed cells from DeltaDetector output.
    /// Returns a combined description of all changed regions.
    pub fn process_changed_regions(
        &self,
        frame_bytes: &[u8],
        changed_cells: &[String],
    ) -> String {
        if changed_cells.is_empty() || !self.available {
            return String::new();
        }
        // Phase 2: crop changed cells from frame, send each to describe_region
        format!("[{} screen regions changed]", changed_cells.len())
    }
}
