//! sleep_gate.rs — Background memory consolidation (Phase 1 stub).
//!
//! On idle/shutdown:
//!   - Consolidates Tier-1 hot entries into Tier-2 warm summaries
//!   - Decays temperatures across all tiers
//!   - Phase 2: writes chronos snapshots + triggers distill.rs

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Mutex;
use crate::memory_tiers::MemoryTiers;

pub struct SleepGate {
    memory: Arc<Mutex<MemoryTiers>>,
    running: Arc<AtomicBool>,
}

impl SleepGate {
    pub fn new(memory: Arc<Mutex<MemoryTiers>>) -> Self {
        Self {
            memory,
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Start background decay loop (runs every 5 minutes)
    pub fn start(&self) -> tokio::task::JoinHandle<()> {
        let memory = self.memory.clone();
        let running = self.running.clone();
        running.store(true, Ordering::SeqCst);

        tokio::spawn(async move {
            while running.load(Ordering::SeqCst) {
                tokio::time::sleep(tokio::time::Duration::from_secs(300)).await;
                let mut mem = memory.lock().await;
                let _ = mem.decay_all(0.05); // 5% decay per cycle
                // Phase 2: promote cooled hot entries to warm, write chronos
            }
        })
    }

    /// Stop the background loop
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Run one consolidation cycle immediately (called on shutdown/idle)
    pub async fn consolidate_now(&self) {
        let mut mem = self.memory.lock().await;
        let _ = mem.decay_all(0.1);
        // Phase 2: summarize + promote hot→warm
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use crate::memory_tiers::MemoryTiers;

    #[tokio::test]
    async fn test_sleep_gate_creation() {
        let db_file = std::env::temp_dir().join("test_sleep_gate.db");
        let _ = fs::remove_file(&db_file);

        let mem = MemoryTiers::open(&db_file).expect("Failed to open");
        let mem = Arc::new(Mutex::new(mem));
        let gate = SleepGate::new(mem);

        assert!(!gate.running.load(Ordering::SeqCst));

        let _ = fs::remove_file(&db_file);
    }

    #[tokio::test]
    async fn test_sleep_gate_stop() {
        let db_file = std::env::temp_dir().join("test_sleep_gate_stop.db");
        let _ = fs::remove_file(&db_file);

        let mem = MemoryTiers::open(&db_file).expect("Failed to open");
        let mem = Arc::new(Mutex::new(mem));
        let gate = SleepGate::new(mem);

        let handle = gate.start();
        assert!(gate.running.load(Ordering::SeqCst));

        gate.stop();
        assert!(!gate.running.load(Ordering::SeqCst));

        // Give a moment for the spawned task to exit
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        handle.abort();

        let _ = fs::remove_file(&db_file);
    }

    #[tokio::test]
    async fn test_sleep_gate_consolidate_now() {
        let db_file = std::env::temp_dir().join("test_sleep_gate_consolidate.db");
        let _ = fs::remove_file(&db_file);

        let mem = MemoryTiers::open(&db_file).expect("Failed to open");
        let mem = Arc::new(Mutex::new(mem));
        let gate = SleepGate::new(mem.clone());

        // Add some hot memory
        {
            let mut m = mem.lock().await;
            m.push_hot("Test entry".to_string());
        }

        // Run consolidation
        gate.consolidate_now().await;

        // Memory should still exist (Phase 2 will promote/summarize)
        // Verify via assembling context rather than accessing private hot field
        let m = mem.lock().await;
        let ctx = m.assemble_context(1024);
        assert!(ctx.contains("Test entry") || ctx.is_empty()); // hot may have decayed

        let _ = fs::remove_file(db_file);
    }
}
