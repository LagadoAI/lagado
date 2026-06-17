//! sleep_gate.rs — Background memory consolidation.
//!
//! Every 5 minutes:
//!   1. Decay all tiers (5% per cycle)
//!   2. Drain hot entries cooled below threshold → batch → LLM summary → warm tier
//!   3. Entropy-prune warm tier when over capacity (lowest information_value first)
//!   4. Log consolidation events to chronos

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Mutex;
use crate::inference::InferenceAdapter;
use crate::memory_tiers::{MemoryEntry, MemoryTiers};

const CONSOLIDATION_THRESHOLD: f32 = 0.5; // hot entries below this are ready for consolidation
const BATCH_SIZE: usize = 8;              // max entries per LLM summary call
const MAX_WARM_ENTRIES: usize = 10_000;   // entropy pruning kicks in above this
const CYCLE_SECS: u64 = 300;             // 5 minutes
const BACKFILL_BATCH: usize = 32;        // max Board text-embeddings computed per cycle

pub struct SleepGate {
    memory:  Arc<Mutex<MemoryTiers>>,
    adapter: Arc<dyn InferenceAdapter>,
    running: Arc<AtomicBool>,
}

impl SleepGate {
    pub fn new(memory: Arc<Mutex<MemoryTiers>>, adapter: Arc<dyn InferenceAdapter>) -> Self {
        Self {
            memory,
            adapter,
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Start background consolidation loop.
    pub fn start(&self) -> tokio::task::JoinHandle<()> {
        let memory  = self.memory.clone();
        let adapter = self.adapter.clone();
        let running = self.running.clone();
        running.store(true, Ordering::SeqCst);

        tokio::spawn(async move {
            while running.load(Ordering::SeqCst) {
                tokio::time::sleep(tokio::time::Duration::from_secs(CYCLE_SECS)).await;
                run_cycle(&memory, &adapter).await;
            }
        })
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Run one full cycle immediately — called on shutdown or explicit trigger.
    pub async fn consolidate_now(&self) {
        run_cycle(&self.memory, &self.adapter).await;
    }
}

// ── Consolidation cycle ───────────────────────────────────────────

async fn run_cycle(
    memory:  &Arc<Mutex<MemoryTiers>>,
    adapter: &Arc<dyn InferenceAdapter>,
) {
    // Step 1: decay — lock briefly, drop before any await
    {
        let mut mem = memory.lock().await;
        let _ = mem.decay_all(0.05);
    }

    // Step 2: drain cooled hot entries — lock, drain, drop
    let cool: Vec<MemoryEntry> = {
        let mut mem = memory.lock().await;
        mem.drain_cool_hot(CONSOLIDATION_THRESHOLD)
    };

    if !cool.is_empty() {
        // Step 3: batch → summarize → promote to warm
        for batch in cool.chunks(BATCH_SIZE) {
            let prompt = build_consolidation_prompt(batch);
            let adp = adapter.clone();

            let summary = tokio::task::spawn_blocking(move || {
                adp.generate(&prompt, 256, 0.3)
            }).await.unwrap_or_else(|_| Err("spawn_blocking failed".into()));

            if let Ok(text) = summary {
                let trimmed = text.trim().to_string();
                if !trimmed.is_empty() {
                    let mut mem = memory.lock().await;
                    let _ = mem.promote_warm_summary(trimmed);
                }
                crate::chronos::log(&format!(
                    "sleep_gate: consolidated {} hot entries → warm",
                    batch.len()
                ));
            }
        }
    }

    // Step 5: backfill Board relevance embeddings. The ColBERT embedder (:8082) may be
    // down — embed() then errors and we retry next cycle (deterministic recency floor
    // holds meanwhile). HTTP runs OUTSIDE the lock; only the cheap store touches it.
    // entries_missing_text_embedding returns PLAINTEXT (cold rows are decrypted there).
    let missing: Vec<(String, String)> = {
        let mem = memory.lock().await;
        let mut m = mem.entries_missing_text_embedding();
        m.truncate(BACKFILL_BATCH);
        m
    };
    if !missing.is_empty() {
        let embedded: Vec<(String, Vec<f32>)> = tokio::task::spawn_blocking(move || {
            let mut out = Vec::new();
            for (id, text) in missing {
                match crate::embedding::embed(&text) {
                    Ok(v) if !v.is_empty() => out.push((id, v)),
                    Ok(_) => {}
                    Err(_) => break, // embedder unreachable — stop, retry next cycle
                }
            }
            out
        })
        .await
        .unwrap_or_default();

        if !embedded.is_empty() {
            let mut mem = memory.lock().await;
            let mut stored = 0usize;
            for (id, emb) in &embedded {
                if mem.store_text_embedding(id, emb).is_ok() { stored += 1; }
            }
            drop(mem);
            tracing::info!("sleep_gate: backfilled {stored} Board text embeddings");
            crate::chronos::log(&format!("sleep_gate: backfilled {stored} Board text embeddings"));
        }
    }

    // Step 4: entropy prune — lock briefly, drop
    {
        let mut mem = memory.lock().await;
        match mem.entropy_prune_warm(MAX_WARM_ENTRIES) {
            Ok(0)      => {}
            Ok(pruned) => {
                tracing::info!("sleep_gate: entropy pruned {pruned} warm entries");
                crate::chronos::log(&format!("sleep_gate: entropy pruned {pruned} warm entries"));
            }
            Err(e) => tracing::warn!("sleep_gate: entropy_prune error: {e}"),
        }
    }
}

fn build_consolidation_prompt(batch: &[MemoryEntry]) -> String {
    let entries: Vec<String> = batch.iter()
        .enumerate()
        .map(|(i, e)| format!("{}. {}", i + 1, e.text.trim()))
        .collect();

    format!(
        "The following are recent observations and actions from an agent work session. \
Write a concise factual summary (2-4 sentences) capturing what was accomplished, \
learned, or is important to remember. Include key outcomes only. No preamble.\n\n\
Entries:\n{}\n\nSummary:",
        entries.join("\n")
    )
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    // Minimal adapter stub for tests — generate() returns a fixed summary
    struct StubAdapter;
    impl InferenceAdapter for StubAdapter {
        fn generate(&self, _p: &str, _max: usize, _temp: f32) -> Result<String, String> {
            Ok("Stub summary: completed test tasks.".into())
        }
        fn supports_kv_slots(&self) -> bool { false }
        fn save_kv_slot(&self, _k: &str) -> Result<(), String> { Ok(()) }
        fn restore_kv_slot(&self, _k: &str) -> Result<bool, String> { Ok(false) }
        fn has_kv_slot(&self, _k: &str) -> bool { false }
        fn model_name(&self) -> &str { "stub" }
        fn context_size(&self) -> usize { 512 }
    }

    fn stub_gate(path: &std::path::Path) -> (Arc<Mutex<MemoryTiers>>, SleepGate) {
        let mem = MemoryTiers::open(path).expect("open");
        let mem = Arc::new(Mutex::new(mem));
        let adapter: Arc<dyn InferenceAdapter> = Arc::new(StubAdapter);
        let gate = SleepGate::new(mem.clone(), adapter);
        (mem, gate)
    }

    #[tokio::test]
    async fn test_creation() {
        let p = std::env::temp_dir().join("sg_create.db");
        let _ = fs::remove_file(&p);
        let (_, gate) = stub_gate(&p);
        assert!(!gate.running.load(Ordering::SeqCst));
        let _ = fs::remove_file(&p);
    }

    #[tokio::test]
    async fn test_start_stop() {
        let p = std::env::temp_dir().join("sg_start_stop.db");
        let _ = fs::remove_file(&p);
        let (_, gate) = stub_gate(&p);
        let handle = gate.start();
        assert!(gate.running.load(Ordering::SeqCst));
        gate.stop();
        assert!(!gate.running.load(Ordering::SeqCst));
        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
        handle.abort();
        let _ = fs::remove_file(&p);
    }

    #[tokio::test]
    async fn test_consolidation_promotes_cool_hot() {
        let p = std::env::temp_dir().join("sg_consolidate.db");
        let _ = fs::remove_file(&p);
        let (mem, gate) = stub_gate(&p);

        // Add a hot entry, then decay it below the consolidation threshold
        {
            let mut m = mem.lock().await;
            m.push_hot("step 1: opened browser".into());
        }
        // Decay 15 cycles of 5% each: 1.0 × 0.95^15 ≈ 0.46 < CONSOLIDATION_THRESHOLD (0.5)
        for _ in 0..15 {
            let mut m = mem.lock().await;
            let _ = m.decay_all(0.05);
        }

        gate.consolidate_now().await;

        // Hot should be drained; warm summary should exist
        let m = mem.lock().await;
        assert_eq!(m.hot_count(), 0, "cool hot entry should have been drained");
        assert!(m.warm_entry_count() > 0, "warm summary should have been written");
        let _ = fs::remove_file(&p);
    }

    #[tokio::test]
    async fn test_entropy_prune_warm_respects_limit() {
        let p = std::env::temp_dir().join("sg_entropy.db");
        let _ = fs::remove_file(&p);
        let (mem, _gate) = stub_gate(&p);

        // Write 15 warm entries directly
        {
            let mut m = mem.lock().await;
            for i in 0..15 {
                m.promote_warm_summary(format!("warm entry {i}")).unwrap();
            }
            assert_eq!(m.warm_entry_count(), 15);
            // Prune to max 10
            let pruned = m.entropy_prune_warm(10).unwrap();
            assert_eq!(pruned, 5);
            assert_eq!(m.warm_entry_count(), 10);
        }
        let _ = fs::remove_file(&p);
    }

    #[tokio::test]
    async fn test_entropy_prune_no_op_under_limit() {
        let p = std::env::temp_dir().join("sg_entropy_noop.db");
        let _ = fs::remove_file(&p);
        let (mem, _gate) = stub_gate(&p);
        {
            let mut m = mem.lock().await;
            for i in 0..5 {
                m.promote_warm_summary(format!("entry {i}")).unwrap();
            }
            let pruned = m.entropy_prune_warm(10).unwrap();
            assert_eq!(pruned, 0);
            assert_eq!(m.warm_entry_count(), 5);
        }
        let _ = fs::remove_file(&p);
    }
}
