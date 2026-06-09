//! kv_slots.rs — KV cache slot management via llama-server /slots API.
//!
//! Warm-starts inference when the same screen/context is seen again.
//! Phase 1: interface only, full implementation in Phase 2.

pub struct KvSlotManager {
    base_url: String,
}

impl KvSlotManager {
    pub fn new(base_url: &str) -> Self {
        Self { base_url: base_url.to_string() }
    }

    /// Save current KV state under a fingerprint key.
    pub fn save(&self, key: &str) -> Result<(), String> {
        // Phase 2: POST to {base_url}/slots with save action
        tracing::debug!("kv_slot save: {key} (stub)");
        Ok(())
    }

    /// Restore KV state for a fingerprint key. Returns true if hit.
    pub fn restore(&self, key: &str) -> Result<bool, String> {
        // Phase 2: POST to {base_url}/slots with restore action
        tracing::debug!("kv_slot restore: {key} (stub, always miss)");
        Ok(false)
    }

    /// Check if a slot exists without restoring.
    pub fn has(&self, key: &str) -> bool {
        tracing::debug!("kv_slot has: {key} (stub, always false)");
        false
    }

    /// Generate a fingerprint for the current context.
    pub fn fingerprint(model_id: &str, state_hash: &str) -> String {
        format!("{model_id}:{state_hash}")
    }
}
