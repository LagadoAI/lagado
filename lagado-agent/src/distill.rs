//! distill.rs — Continual learning hooks (Phase 2: QLoRA training pipeline).
//!
//! v1 builds ONLY the data infrastructure:
//!   - ReplayEntry: a verified experience tagged for future training
//!   - ReplayManifest: append-only log of training candidates
//!   - tag_for_replay(): called by action_graph when verified_success=true
//!
//! Phase 2 adds: batch assembly → QLoRA train → eval gate → adapter merge.
//! The frozen constitution (safety constraints) is the floor — never trained away.

use serde::{Deserialize, Serialize};
use rusqlite::params;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayEntry {
    pub id: String,
    pub prompt: String,
    pub response: String,
    pub verified: bool,   // only true entries should be trained on
    pub source: String,   // "action_graph" | "self_model" | "chronos"
    pub created_at: i64,
}

pub struct ReplayManifest {
    db_path: PathBuf,
}

impl ReplayManifest {
    pub fn open(data_dir: &Path) -> Self {
        let db_path = data_dir.join("replay_manifest.db");
        if let Some(p) = db_path.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        // Schema once, in the file (was re-run on every conn()).
        if let Ok(conn) = rusqlite::Connection::open(&db_path) {
            let _ = conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS replay_entries (
                    id         TEXT PRIMARY KEY,
                    prompt     TEXT NOT NULL,
                    response   TEXT NOT NULL,
                    verified   INTEGER NOT NULL DEFAULT 0,
                    source     TEXT NOT NULL,
                    created_at INTEGER NOT NULL
                );"
            );
        }
        Self { db_path }
    }

    fn conn(&self) -> Result<rusqlite::Connection, String> {
        rusqlite::Connection::open(&self.db_path).map_err(|e| e.to_string())
    }

    /// Tag a prompt+response pair for potential training (v1 distill hook).
    pub fn tag_for_replay(
        &self, prompt: &str, response: &str, verified: bool, source: &str,
    ) -> Result<String, String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO replay_entries (id, prompt, response, verified, source, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, prompt, response, verified as i32, source, now],
        ).map_err(|e| e.to_string())?;
        Ok(id)
    }

    /// Retrieve verified entries for Phase 2 training batch assembly.
    pub fn verified_entries(&self, limit: usize) -> Vec<ReplayEntry> {
        let conn = match self.conn() {
            Ok(c) => c,
            Err(_) => return vec![],
        };
        let mut stmt = match conn.prepare(
            "SELECT id, prompt, response, verified, source, created_at
             FROM replay_entries WHERE verified = 1
             ORDER BY created_at DESC LIMIT ?1"
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map(params![limit as i64], |row| {
            Ok(ReplayEntry {
                id: row.get(0)?,
                prompt: row.get(1)?,
                response: row.get(2)?,
                verified: row.get::<_, i32>(3)? != 0,
                source: row.get(4)?,
                created_at: row.get(5)?,
            })
        })
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|r| r.ok())
        .collect()
    }

    pub fn entry_count(&self) -> usize {
        let conn = match self.conn() {
            Ok(c) => c,
            Err(_) => return 0,
        };
        conn.query_row("SELECT COUNT(*) FROM replay_entries", [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap_or(0) as usize
    }
}

/// Convenience: tag a successful agent action for replay (called from agent loop).
pub fn tag_successful_action(data_dir: &Path, prompt: &str, action: &str) {
    let manifest = ReplayManifest::open(data_dir);
    let _ = manifest.tag_for_replay(prompt, action, true, "action_graph");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn replay_entry_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let manifest = ReplayManifest::open(tmp.path());

        let id = manifest.tag_for_replay(
            "What is 2+2?",
            "2+2 = 4",
            true,
            "action_graph",
        ).unwrap();

        assert!(!id.is_empty());

        let entries = manifest.verified_entries(10);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].prompt, "What is 2+2?");
        assert_eq!(entries[0].response, "2+2 = 4");
        assert!(entries[0].verified);
        assert_eq!(entries[0].source, "action_graph");
    }

    #[test]
    fn entry_count_tracking() {
        let tmp = TempDir::new().unwrap();
        let manifest = ReplayManifest::open(tmp.path());

        assert_eq!(manifest.entry_count(), 0);

        let _ = manifest.tag_for_replay("q1", "r1", true, "test").unwrap();
        assert_eq!(manifest.entry_count(), 1);

        let _ = manifest.tag_for_replay("q2", "r2", false, "test").unwrap();
        assert_eq!(manifest.entry_count(), 2);

        // verified_entries should only return true entries
        let verified = manifest.verified_entries(10);
        assert_eq!(verified.len(), 1);
    }
}
