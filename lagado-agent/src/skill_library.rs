//! skill_library.rs — Experiential procedure store.
//!
//! Skills are distilled named procedures retrieved as *advisory context* for
//! the agent's reasoning loop — not executed verbatim.  The model reads
//! retrieved skills as guidance and re-grounds them against live perception.
//!
//! Two complementary layers:
//!   action_graph  — muscle memory: exact state-hash → action shortcut (bypasses inference)
//!   skill_library — depth: situation-class → procedure guidance (informs inference)
//!
//! Schema (skills table):
//!   id, name, description, situation, approach, steps_json,
//!   success_count, failure_count, last_success, source

use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

// ── Data model ────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SkillSource {
    Distilled,  // promoted from a completed episode by the sleep gate
    Recorded,   // extracted from a single successful session
    Manual,     // hand-authored
}

impl SkillSource {
    fn as_str(self) -> &'static str {
        match self {
            SkillSource::Distilled => "distilled",
            SkillSource::Recorded  => "recorded",
            SkillSource::Manual    => "manual",
        }
    }

    fn from_str(s: &str) -> Self {
        match s {
            "recorded" => SkillSource::Recorded,
            "manual"   => SkillSource::Manual,
            _          => SkillSource::Distilled,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub id:            String,
    pub name:          String,
    /// When does this apply? (situation/trigger description)
    pub description:   String,
    /// The key lesson or approach in plain language.
    pub approach:      String,
    /// Illustrative reference steps — advisory only, not executed verbatim.
    pub steps:         Vec<crate::types::ToolCall>,
    pub success_count: u32,
    pub failure_count: u32,
    pub last_success:  i64,
    pub source:        SkillSource,
}

impl Skill {
    /// Create a new skill from a completed episode.
    /// `name`        — short snake_case label, e.g. "navigate_file_dialog"
    /// `description` — when does this situation arise?
    /// `approach`    — the key lesson/method, plain NL
    /// `steps`       — reference steps from the episode (illustrative)
    pub fn from_episode(
        name: impl Into<String>,
        description: impl Into<String>,
        approach: impl Into<String>,
        steps: Vec<crate::types::ToolCall>,
    ) -> Self {
        Self {
            id:            uuid(),
            name:          name.into(),
            description:   description.into(),
            approach:      approach.into(),
            steps,
            success_count: 1,
            failure_count: 0,
            last_success:  now_unix(),
            source:        SkillSource::Distilled,
        }
    }
}

// ── SkillLibrary ──────────────────────────────────────────────────

pub struct SkillLibrary {
    db_path: std::path::PathBuf,
}

impl SkillLibrary {
    pub fn open(data_dir: &Path) -> Self {
        let db_path = data_dir.join("skill_library.db");
        if let Some(p) = db_path.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        // Schema + migrations run ONCE here (they live in the DB file, not the connection) — they
        // used to re-run CREATE TABLE + 3 failing ALTERs on EVERY conn(), i.e. every retrieve on the
        // planning path. WAL too, so reads don't block the distill writer.
        if let Ok(conn) = rusqlite::Connection::open(&db_path) {
            let _ = conn.pragma_update(None, "journal_mode", "WAL");
            let _ = conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS skills (
                    id            TEXT PRIMARY KEY,
                    name          TEXT NOT NULL,
                    description   TEXT NOT NULL,
                    approach      TEXT NOT NULL DEFAULT '',
                    steps_json    TEXT NOT NULL,
                    success_count INTEGER NOT NULL DEFAULT 0,
                    failure_count INTEGER NOT NULL DEFAULT 0,
                    last_success  INTEGER NOT NULL DEFAULT 0,
                    source        TEXT NOT NULL DEFAULT 'distilled'
                );",
            );
            let _ = conn.execute_batch(
                "ALTER TABLE skills ADD COLUMN approach      TEXT NOT NULL DEFAULT '';
                 ALTER TABLE skills ADD COLUMN failure_count INTEGER NOT NULL DEFAULT 0;
                 ALTER TABLE skills ADD COLUMN source        TEXT NOT NULL DEFAULT 'distilled';"
            );
        }
        Self { db_path }
    }

    /// Open a connection. Schema/migrations already ran in `open()` (the table lives in the file), so
    /// this is just the cheap file-open — no per-call DDL.
    fn conn(&self) -> Result<rusqlite::Connection, String> {
        rusqlite::Connection::open(&self.db_path).map_err(|e| e.to_string())
    }

    // ── Write ─────────────────────────────────────────────────────

    /// Upsert a skill. If the id already exists, bump success_count and update fields.
    pub fn save(&self, skill: &Skill) -> Result<(), String> {
        let conn = self.conn()?;
        let steps_json = serde_json::to_string(&skill.steps)
            .map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO skills
               (id, name, description, approach, steps_json,
                success_count, failure_count, last_success, source)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(id) DO UPDATE SET
               name          = excluded.name,
               description   = excluded.description,
               approach      = excluded.approach,
               steps_json    = excluded.steps_json,
               success_count = success_count + 1,
               last_success  = excluded.last_success,
               source        = excluded.source",
            params![
                skill.id, skill.name, skill.description, skill.approach,
                steps_json, skill.success_count, skill.failure_count,
                skill.last_success, skill.source.as_str()
            ],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Record a successful use of a skill — call after the agent completes a task
    /// that matched a retrieved skill, as feedback that the guidance was useful.
    pub fn record_success(&self, skill_id: &str) -> Result<(), String> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE skills SET success_count = success_count + 1, last_success = ?1 WHERE id = ?2",
            params![now_unix(), skill_id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Record that a retrieved skill's guidance didn't help (or was actively wrong).
    pub fn record_failure(&self, skill_id: &str) -> Result<(), String> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE skills SET failure_count = failure_count + 1 WHERE id = ?1",
            params![skill_id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Delete a skill by id (for settings UI and pruning).
    pub fn delete(&self, skill_id: &str) -> Result<(), String> {
        let conn = self.conn()?;
        conn.execute("DELETE FROM skills WHERE id = ?1", params![skill_id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    // ── Read ──────────────────────────────────────────────────────

    /// Retrieve the K most contextually relevant skills for a query.
    ///
    /// Scoring: Jaccard word-overlap on name+description+approach,
    /// boosted by success ratio (success / (success + failure + 1)).
    /// Skills with success_ratio < 0.3 are excluded as unreliable.
    pub fn retrieve(&self, query: &str, k: usize) -> Vec<Skill> {
        let conn = match self.conn() {
            Ok(c) => c,
            Err(_) => return vec![],
        };
        let mut stmt = match conn.prepare(
            "SELECT id, name, description, approach, steps_json,
                    success_count, failure_count, last_success, source
             FROM skills
             ORDER BY success_count DESC
             LIMIT 300",
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
                    row.get::<_, String>(4)?,
                    row.get::<_, u32>(5)?,
                    row.get::<_, u32>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, String>(8)?,
                ))
            })
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|r| r.ok())
            .filter_map(|(id, name, desc, approach, steps_json,
                          success_count, failure_count, last_success, source)| {
                // Filter skills that have been marked unreliable
                let total = success_count + failure_count;
                let success_ratio = if total > 0 {
                    success_count as f32 / total as f32
                } else {
                    1.0 // new skills get benefit of the doubt
                };
                if total >= 3 && success_ratio < 0.3 {
                    return None;
                }

                let steps: Vec<crate::types::ToolCall> =
                    serde_json::from_str(&steps_json).ok()?;

                let skill = Skill {
                    id,
                    name:          name.clone(),
                    description:   desc.clone(),
                    approach:      approach.clone(),
                    steps,
                    success_count,
                    failure_count,
                    last_success,
                    source:        SkillSource::from_str(&source),
                };

                // Jaccard over name + description + approach
                let search_text = format!("{name} {desc} {approach}");
                let c_words: HashSet<&str> = search_text.split_whitespace().collect();
                let inter = q_words.intersection(&c_words).count() as f32;
                let union = q_words.union(&c_words).count() as f32;
                let jaccard = if union > 0.0 { inter / union } else { 0.0 };

                // Boost by success reliability, capped at 0.2
                let reliability_boost = (success_ratio * 0.2).min(0.2);
                let score = jaccard * 0.8 + reliability_boost;

                // Require meaningful overlap — don't inject noise as "relevant"
                if score < 0.05 { return None; }

                Some((score, skill))
            })
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.into_iter().take(k).map(|(_, s)| s).collect()
    }

    /// List every skill — used by the settings UI.
    pub fn list_all(&self) -> Vec<Skill> {
        self.retrieve("", usize::MAX)
    }

    // ── Prompt formatting ─────────────────────────────────────────

    /// Format a slice of skills as advisory prompt context.
    ///
    /// Injected into the agent loop as "Relevant procedures from experience:"
    /// so the model can adapt them to the current screen rather than replay them.
    pub fn format_for_prompt(skills: &[Skill]) -> String {
        if skills.is_empty() {
            return String::new();
        }
        let mut out = String::from("Relevant procedures from experience:\n");
        for s in skills {
            // Compact reference step list — max 4 shown
            let step_hint = if s.steps.is_empty() {
                String::new()
            } else {
                let shown: Vec<String> = s.steps.iter().take(4)
                    .map(|t| step_hint(t))
                    .collect();
                let ellipsis = if s.steps.len() > 4 { ", …" } else { "" };
                format!(" [{}{}]", shown.join(" → "), ellipsis)
            };
            let uses = s.success_count;
            out.push_str(&format!(
                "• {} (used {} time{}): {}{}\n",
                s.name,
                uses,
                if uses == 1 { "" } else { "s" },
                s.approach,
                step_hint,
            ));
        }
        out
    }
}

// ── Helpers ───────────────────────────────────────────────────────

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn step_hint(call: &crate::types::ToolCall) -> String {
    match call {
        crate::types::ToolCall::Click  { selector }       => format!("click({})", selector),
        crate::types::ToolCall::Type   { selector, text } => format!("type({}, {:?})", selector, text),
        crate::types::ToolCall::Key    { key }            => format!("key({})", key),
        crate::types::ToolCall::Wait   { ms }             => format!("wait({}ms)", ms),
        crate::types::ToolCall::Invoke { name, .. }       => format!("{}(…)", name),
        crate::types::ToolCall::Done   { .. }             => "done".to_string(),
        crate::types::ToolCall::Task   { description }    => format!("task({})", description),
        crate::types::ToolCall::Chat   { .. }             => "chat".to_string(),
    }
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_N: AtomicU64 = AtomicU64::new(0);

    fn test_lib() -> SkillLibrary {
        let n = TEST_N.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("lagado_skill_test_{}_{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir); // start clean — temp dirs persist across cargo-test runs
        std::fs::create_dir_all(&dir).ok();
        SkillLibrary::open(&dir)
    }

    #[test]
    fn save_and_retrieve_by_description() {
        let lib = test_lib();
        let skill = Skill::from_episode(
            "navigate_file_dialog",
            "A file open or save dialog is visible",
            "Type the full path directly in the address bar rather than navigating the tree. Faster and more reliable.",
            vec![],
        );
        lib.save(&skill).unwrap();

        let results = lib.retrieve("file dialog open save", 5);
        assert!(!results.is_empty());
        assert_eq!(results[0].name, "navigate_file_dialog");
    }

    #[test]
    fn approach_included_in_search() {
        let lib = test_lib();
        let skill = Skill::from_episode(
            "recover_stalled_terminal",
            "Terminal command appears frozen with no output",
            "Send Ctrl+C to interrupt, then retry the command. If still stalled, check if it is waiting for stdin.",
            vec![],
        );
        lib.save(&skill).unwrap();

        // Search by words from the approach, not just description
        let results = lib.retrieve("interrupt stalled command ctrl", 5);
        assert!(!results.is_empty());
        assert_eq!(results[0].name, "recover_stalled_terminal");
    }

    #[test]
    fn unreliable_skills_filtered() {
        let lib = test_lib();
        let mut skill = Skill::from_episode(
            "bad_approach",
            "some situation",
            "this never works",
            vec![],
        );
        // 1 success, 9 failures → ratio = 0.1 < 0.3
        skill.success_count = 1;
        skill.failure_count = 9;
        lib.save(&skill).unwrap();

        let results = lib.retrieve("some situation", 5);
        assert!(results.is_empty(), "unreliable skill should be filtered");
    }

    #[test]
    fn record_success_and_failure() {
        let lib = test_lib();
        let skill = Skill::from_episode("test_skill", "test", "test approach", vec![]);
        let id = skill.id.clone();
        lib.save(&skill).unwrap();

        lib.record_success(&id).unwrap();
        lib.record_failure(&id).unwrap();

        let all = lib.list_all();
        let found = all.iter().find(|s| s.id == id).unwrap();
        assert!(found.success_count >= 2);
        assert_eq!(found.failure_count, 1);
    }

    #[test]
    fn format_for_prompt_nonempty() {
        let skill = Skill::from_episode(
            "open_browser",
            "Need to open the web browser",
            "Click the browser icon in the taskbar or use the applications menu.",
            vec![],
        );
        let formatted = SkillLibrary::format_for_prompt(&[skill]);
        assert!(formatted.contains("open_browser"));
        assert!(formatted.contains("taskbar"));
        assert!(formatted.contains("Relevant procedures"));
    }

    #[test]
    fn format_for_prompt_empty() {
        assert_eq!(SkillLibrary::format_for_prompt(&[]), String::new());
    }

    #[test]
    fn delete_skill() {
        let lib = test_lib();
        let skill = Skill::from_episode("to_delete", "desc", "approach", vec![]);
        let id = skill.id.clone();
        lib.save(&skill).unwrap();
        assert!(!lib.list_all().is_empty());

        lib.delete(&id).unwrap();
        assert!(lib.list_all().is_empty());
    }
}
