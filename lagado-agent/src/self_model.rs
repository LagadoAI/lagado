//! self_model.rs — Agent's accepted beliefs about the user and environment.
//!
//! Beliefs are statements the agent has formed and the user has accepted.
//! Phase 2: accepted statements feed distill.rs training set.
//! v1 hook: `accepted` flag tags entries for future distillation.

use serde::{Deserialize, Serialize};
use rusqlite::params;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Belief {
    pub id: String,           // uuid
    pub statement: String,    // e.g. "User prefers dark mode"
    pub confidence: f32,      // 0.0–1.0
    pub accepted: bool,       // true = user confirmed, feeds distill
    pub created_at: i64,
    pub source: String,       // "observation" | "inference" | "user_stated"
}

pub struct SelfModel {
    db_path: PathBuf,
}

impl SelfModel {
    pub fn open(data_dir: &Path) -> Self {
        let db_path = data_dir.join("self_model.db");
        if let Some(p) = db_path.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        // Schema once, in the file (was re-run on every conn()).
        if let Ok(conn) = rusqlite::Connection::open(&db_path) {
            let _ = conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS beliefs (
                    id         TEXT PRIMARY KEY,
                    statement  TEXT NOT NULL,
                    confidence REAL NOT NULL,
                    accepted   INTEGER NOT NULL DEFAULT 0,
                    created_at INTEGER NOT NULL,
                    source     TEXT NOT NULL
                );"
            );
        }
        Self { db_path }
    }

    fn conn(&self) -> Result<rusqlite::Connection, String> {
        rusqlite::Connection::open(&self.db_path).map_err(|e| e.to_string())
    }

    pub fn add(&self, statement: &str, confidence: f32, source: &str) -> Result<String, String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO beliefs (id, statement, confidence, accepted, created_at, source)
             VALUES (?1, ?2, ?3, 0, ?4, ?5)",
            params![id, statement, confidence, now, source],
        ).map_err(|e| e.to_string())?;
        Ok(id)
    }

    /// Mark a belief as accepted by the user (v1 distill hook).
    pub fn accept(&self, id: &str) -> Result<(), String> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE beliefs SET accepted = 1 WHERE id = ?1",
            params![id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Retrieve accepted beliefs for distillation or context injection.
    pub fn accepted_beliefs(&self) -> Vec<Belief> {
        let conn = match self.conn() {
            Ok(c) => c,
            Err(_) => return vec![],
        };
        let mut stmt = match conn.prepare(
            "SELECT id, statement, confidence, accepted, created_at, source
             FROM beliefs WHERE accepted = 1 ORDER BY confidence DESC LIMIT 50"
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map([], |row| {
            Ok(Belief {
                id: row.get(0)?,
                statement: row.get(1)?,
                confidence: row.get(2)?,
                accepted: row.get::<_, i32>(3)? != 0,
                created_at: row.get(4)?,
                source: row.get(5)?,
            })
        })
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|r| r.ok())
        .collect()
    }

    /// Format accepted beliefs as context for prompt injection.
    pub fn context_string(&self) -> String {
        let beliefs = self.accepted_beliefs();
        if beliefs.is_empty() {
            return String::new();
        }
        let lines: Vec<String> = beliefs
            .iter()
            .map(|b| format!("- {} (confidence: {:.1})", b.statement, b.confidence))
            .collect();
        format!("Known about user:\n{}", lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn belief_add_and_accept() {
        let tmp = TempDir::new().unwrap();
        let model = SelfModel::open(tmp.path());

        let id = model.add("User prefers dark mode", 0.9, "observation").unwrap();
        assert!(!id.is_empty());

        // Belief should not be in accepted yet
        let accepted = model.accepted_beliefs();
        assert_eq!(accepted.len(), 0);

        // Accept the belief
        model.accept(&id).unwrap();

        // Now it should appear
        let accepted = model.accepted_beliefs();
        assert_eq!(accepted.len(), 1);
        assert_eq!(accepted[0].statement, "User prefers dark mode");
        assert_eq!(accepted[0].confidence, 0.9);
        assert!(accepted[0].accepted);
        assert_eq!(accepted[0].source, "observation");
    }

    #[test]
    fn context_string_formatting() {
        let tmp = TempDir::new().unwrap();
        let model = SelfModel::open(tmp.path());

        // Empty context
        assert_eq!(model.context_string(), "");

        // Add and accept a belief
        let id1 = model.add("User is in UTC-5", 0.8, "inference").unwrap();
        model.accept(&id1).unwrap();

        let context = model.context_string();
        assert!(context.contains("Known about user:"));
        assert!(context.contains("User is in UTC-5"));
        assert!(context.contains("0.8"));
    }

    #[test]
    fn multiple_beliefs_ordered_by_confidence() {
        let tmp = TempDir::new().unwrap();
        let model = SelfModel::open(tmp.path());

        let id1 = model.add("High confidence belief", 0.95, "user_stated").unwrap();
        let id2 = model.add("Low confidence belief", 0.3, "observation").unwrap();
        let id3 = model.add("Medium confidence belief", 0.6, "inference").unwrap();

        model.accept(&id1).unwrap();
        model.accept(&id2).unwrap();
        model.accept(&id3).unwrap();

        let accepted = model.accepted_beliefs();
        assert_eq!(accepted.len(), 3);
        // Should be ordered by confidence descending
        assert_eq!(accepted[0].confidence, 0.95);
        assert_eq!(accepted[1].confidence, 0.6);
        assert_eq!(accepted[2].confidence, 0.3);
    }
}
