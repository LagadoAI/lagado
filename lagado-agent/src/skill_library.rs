//! skill_library.rs — Voyager-style verified multi-step procedure store.
//!
//! Stores successful action sequences (skills) indexed by natural-language description.
//! Retrieved by embedding similarity (Phase 1: word overlap, Phase 2: vectors).
//! Skills are verified-only: only sequences with confirmed success are stored.
//!
//! Schema:
//!   skills (id, name, description, steps_json, success_count, last_success_ts)

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub id: String,                              // uuid
    pub name: String,                            // short label e.g. "open_browser"
    pub description: String,                     // NL description
    pub steps: Vec<crate::types::ToolCall>,      // ordered verified tool calls
    pub success_count: u32,
    pub last_success: i64,                       // unix timestamp
}

pub struct SkillLibrary {
    db_path: std::path::PathBuf,
}

impl SkillLibrary {
    pub fn open(data_dir: &Path) -> Self {
        Self {
            db_path: data_dir.join("skill_library.db"),
        }
    }

    fn conn(&self) -> Result<rusqlite::Connection, String> {
        if let Some(p) = self.db_path.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        let conn = rusqlite::Connection::open(&self.db_path).map_err(|e| e.to_string())?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS skills (
                id            TEXT PRIMARY KEY,
                name          TEXT NOT NULL,
                description   TEXT NOT NULL,
                steps_json    TEXT NOT NULL,
                success_count INTEGER NOT NULL DEFAULT 0,
                last_success  INTEGER NOT NULL DEFAULT 0
            );",
        )
        .map_err(|e| e.to_string())?;
        Ok(conn)
    }

    /// Save a verified skill. If a skill with the same id exists, increment success_count.
    pub fn save(&self, skill: &Skill) -> Result<(), String> {
        let conn = self.conn()?;
        let steps_json = serde_json::to_string(&skill.steps).map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO skills (id, name, description, steps_json, success_count, last_success)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
               success_count = success_count + 1,
               last_success  = excluded.last_success",
            rusqlite::params![
                skill.id,
                skill.name,
                skill.description,
                steps_json,
                skill.success_count,
                skill.last_success
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Retrieve the K most relevant skills for a query using word-overlap scoring.
    pub fn retrieve(&self, query: &str, k: usize) -> Vec<Skill> {
        let conn = match self.conn() {
            Ok(c) => c,
            Err(_) => return vec![],
        };
        let mut stmt = match conn.prepare(
            "SELECT id, name, description, steps_json, success_count, last_success
             FROM skills ORDER BY success_count DESC LIMIT 200",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };

        let q_words: HashSet<&str> = query.split_whitespace().collect();

        let mut scored: Vec<(f32, Skill)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, u32>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            })
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|r| r.ok())
            .filter_map(|(id, name, desc, steps_json, success_count, last_success)| {
                let steps: Vec<crate::types::ToolCall> =
                    serde_json::from_str(&steps_json).ok()?;
                let skill = Skill {
                    id,
                    name: name.clone(),
                    description: desc.clone(),
                    steps,
                    success_count,
                    last_success,
                };
                // Score: word overlap with name+description, weighted by success_count
                let text = format!("{name} {desc}");
                let c_words: HashSet<&str> = text.split_whitespace().collect();
                let inter = q_words.intersection(&c_words).count() as f32;
                let union = q_words.union(&c_words).count() as f32;
                let jaccard = if union > 0.0 { inter / union } else { 0.0 };
                let success_boost = (success_count as f32 / 10.0).min(0.3);
                let score = (jaccard * 0.7 + success_boost).min(1.0);
                Some((score, skill))
            })
            .collect();

        scored.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.into_iter().take(k).map(|(_, s)| s).collect()
    }

    /// Record a successful execution of a skill (increments success_count).
    pub fn record_success(&self, skill_id: &str) -> Result<(), String> {
        let conn = self.conn()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        conn.execute(
            "UPDATE skills SET success_count = success_count + 1, last_success = ?1 WHERE id = ?2",
            rusqlite::params![now, skill_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// List all skills (for settings UI).
    pub fn list_all(&self) -> Vec<Skill> {
        self.retrieve("", usize::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_and_retrieve() {
        let dir = std::env::temp_dir().join("lagado_skill_test");
        std::fs::create_dir_all(&dir).ok();
        let lib = SkillLibrary::open(&dir);
        let skill = Skill {
            id: "test-1".into(),
            name: "open_browser".into(),
            description: "Open the web browser".into(),
            steps: vec![],
            success_count: 1,
            last_success: 0,
        };
        lib.save(&skill).unwrap();
        let results = lib.retrieve("open browser", 5);
        assert!(!results.is_empty());
        assert_eq!(results[0].name, "open_browser");
    }
}
