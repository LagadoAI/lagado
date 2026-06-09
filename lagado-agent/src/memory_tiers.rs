//! memory_tiers.rs — Three-tier thermodynamic memory hierarchy.
//!
//! Tier 1 HOT  — current turn, in-RAM Vec, zero-copy
//! Tier 2 WARM — recent sessions, SQLite, summarized
//! Tier 3 COLD — vault, SQLite, exact text, lazy-loaded
//!
//! Phase 1: no encryption (placeholder for security/crypto.rs), no FAISS.
//! Temperature decays exponentially with time; reinforces on access.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[derive(Debug, Clone)]
pub struct MemoryEntry {
    pub id: String,           // uuid
    pub text: String,
    pub tier: Tier,
    pub temperature: f32,     // 0.0–1.0
    pub created_at: i64,      // unix seconds
    pub accessed_at: i64,
    pub access_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Hot,
    Warm,
    Cold,
}

impl Tier {
    fn as_str(&self) -> &str {
        match self {
            Tier::Hot => "hot",
            Tier::Warm => "warm",
            Tier::Cold => "cold",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "hot" => Some(Tier::Hot),
            "warm" => Some(Tier::Warm),
            "cold" => Some(Tier::Cold),
            _ => None,
        }
    }
}

pub struct MemoryTiers {
    hot: Vec<MemoryEntry>,
    db: rusqlite::Connection,
}

impl MemoryTiers {
    /// Opens/creates SQLite db and initializes the memory schema.
    pub fn open(db_path: &Path) -> Result<Self, String> {
        let db = rusqlite::Connection::open(db_path)
            .map_err(|e| format!("Failed to open SQLite database: {}", e))?;

        // Create table if not exists
        db.execute(
            "CREATE TABLE IF NOT EXISTS memory_entries (
                id TEXT PRIMARY KEY,
                text TEXT NOT NULL,
                tier TEXT NOT NULL,
                temperature REAL NOT NULL,
                created_at INTEGER NOT NULL,
                accessed_at INTEGER NOT NULL,
                access_count INTEGER NOT NULL
            )",
            [],
        )
        .map_err(|e| format!("Failed to create memory_entries table: {}", e))?;

        Ok(MemoryTiers {
            hot: Vec::new(),
            db,
        })
    }

    /// Push an entry into HOT tier (current turn, in-RAM).
    pub fn push_hot(&mut self, text: String) {
        let now = now_unix();
        let entry = MemoryEntry {
            id: Uuid::new_v4().to_string(),
            text,
            tier: Tier::Hot,
            temperature: 1.0,
            created_at: now,
            accessed_at: now,
            access_count: 0,
        };
        self.hot.push(entry);
    }

    /// Promote an entry to WARM tier (SQLite).
    pub fn promote_to_warm(&mut self, mut entry: MemoryEntry) -> Result<(), String> {
        entry.tier = Tier::Warm;
        entry.accessed_at = now_unix();
        self.db
            .execute(
                "INSERT OR REPLACE INTO memory_entries (id, text, tier, temperature, created_at, accessed_at, access_count)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    &entry.id,
                    &entry.text,
                    entry.tier.as_str(),
                    entry.temperature,
                    entry.created_at,
                    entry.accessed_at,
                    entry.access_count,
                ],
            )
            .map_err(|e| format!("Failed to promote entry to warm: {}", e))?;
        Ok(())
    }

    /// Push an entry directly into COLD tier (vault).
    pub fn push_cold(&mut self, text: String) -> Result<(), String> {
        let now = now_unix();
        let entry = MemoryEntry {
            id: Uuid::new_v4().to_string(),
            text: text.clone(),
            tier: Tier::Cold,
            temperature: 0.5,
            created_at: now,
            accessed_at: now,
            access_count: 0,
        };

        // Encrypt cold tier entry before storage
        let passphrase = crate::auth::active_key();
        let encrypted = crate::security::crypto::encrypt(text.as_bytes(), &passphrase)
            .unwrap_or_else(|_| text.as_bytes().to_vec());
        let stored_text = hex::encode(&encrypted);

        self.db
            .execute(
                "INSERT INTO memory_entries (id, text, tier, temperature, created_at, accessed_at, access_count)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    &entry.id,
                    &stored_text,
                    entry.tier.as_str(),
                    entry.temperature,
                    entry.created_at,
                    entry.accessed_at,
                    entry.access_count,
                ],
            )
            .map_err(|e| format!("Failed to insert cold entry: {}", e))?;
        Ok(())
    }

    /// Push a cross-session episode into COLD tier at full temperature.
    /// Use for goal completions, aborts, and significant events that should
    /// survive session restarts. Encrypted via active_key().
    pub fn push_episode(&mut self, text: String) -> Result<(), String> {
        let now = now_unix();
        let id = Uuid::new_v4().to_string();

        let passphrase = crate::auth::active_key();
        let encrypted = crate::security::crypto::encrypt(text.as_bytes(), &passphrase)
            .unwrap_or_else(|_| text.as_bytes().to_vec());
        let stored_text = hex::encode(&encrypted);

        self.db
            .execute(
                "INSERT INTO memory_entries (id, text, tier, temperature, created_at, accessed_at, access_count)
                 VALUES (?1, ?2, 'cold', 1.0, ?3, ?4, 0)",
                rusqlite::params![&id, &stored_text, now, now],
            )
            .map_err(|e| format!("Failed to insert episode: {}", e))?;
        Ok(())
    }

    /// Reinforce an entry by ID (increase temperature, update access metadata).
    pub fn reinforce(&mut self, id: &str) -> Result<(), String> {
        // Try hot tier first
        if let Some(entry) = self.hot.iter_mut().find(|e| e.id == id) {
            entry.temperature = (entry.temperature + 0.2).min(1.0);
            entry.accessed_at = now_unix();
            entry.access_count += 1;
            return Ok(());
        }

        // Fall back to SQLite (warm or cold)
        let mut stmt = self
            .db
            .prepare("SELECT temperature, access_count FROM memory_entries WHERE id = ?1")
            .map_err(|e| format!("Failed to prepare select: {}", e))?;

        let (temp, count): (f32, u32) = stmt
            .query_row([id], |row| {
                Ok((row.get::<_, f32>(0)?, row.get::<_, u32>(1)?))
            })
            .map_err(|_| format!("Entry not found: {}", id))?;

        let new_temp = (temp + 0.2).min(1.0);
        let new_count = count + 1;
        let now = now_unix();

        self.db
            .execute(
                "UPDATE memory_entries SET temperature = ?1, accessed_at = ?2, access_count = ?3 WHERE id = ?4",
                rusqlite::params![new_temp, now, new_count, id],
            )
            .map_err(|e| format!("Failed to update entry: {}", e))?;

        Ok(())
    }

    /// Decay all entries across all tiers and remove cold entries below threshold.
    pub fn decay_all(&mut self, decay_factor: f32) -> Result<(), String> {
        // Decay hot entries
        for e in &mut self.hot {
            e.temperature *= 1.0 - decay_factor;
        }
        self.hot.retain(|e| e.temperature >= 0.05);

        // Decay SQLite entries
        self.db
            .execute(
                "UPDATE memory_entries SET temperature = temperature * (1.0 - ?1)",
                rusqlite::params![decay_factor],
            )
            .map_err(|e| format!("Failed to decay SQLite entries: {}", e))?;

        // Delete only hot/warm entries below threshold — cold is the vault and must not decay out
        self.db
            .execute(
                "DELETE FROM memory_entries WHERE temperature < 0.05 AND tier != 'cold'",
                [],
            )
            .map_err(|e| format!("Failed to delete warm entries: {}", e))?;

        Ok(())
    }

    /// Assemble context from memory within a token budget.
    pub fn assemble_context(&self, budget_tokens: usize) -> String {
        let budget_chars = budget_tokens * 4;
        let half_budget = budget_chars / 2;

        let mut result = String::new();

        // Hot entries: sort by temperature desc, take up to half budget
        let mut hot_sorted = self.hot.clone();
        hot_sorted.sort_by(|a, b| b.temperature.partial_cmp(&a.temperature).unwrap_or(std::cmp::Ordering::Equal));

        let mut hot_chars = 0;
        for entry in hot_sorted {
            if hot_chars >= half_budget {
                break;
            }
            let line = format!("- {}\n", entry.text);
            if hot_chars + line.len() <= half_budget {
                result.push_str(&line);
                hot_chars += line.len();
            }
        }

        // Warm/Cold entries from SQLite
        let mut stmt = match self
            .db
            .prepare("SELECT text, tier FROM memory_entries ORDER BY temperature DESC")
        {
            Ok(s) => s,
            Err(_) => return result,
        };

        let entries: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
            .unwrap_or_default();

        let passphrase = crate::auth::active_key();
        let mut db_chars = 0;
        for (raw_text, tier) in entries {
            if db_chars >= half_budget {
                break;
            }

            // Decrypt cold entries only
            let display_text = if tier == "cold" {
                if let Ok(bytes) = hex::decode(&raw_text) {
                    crate::security::crypto::decrypt(&bytes, &passphrase)
                        .ok()
                        .and_then(|b| String::from_utf8(b).ok())
                        .unwrap_or(raw_text) // fallback to raw if decrypt fails
                } else {
                    raw_text
                }
            } else {
                raw_text
            };

            let line = format!("- {}\n", display_text);
            if db_chars + line.len() <= half_budget {
                result.push_str(&line);
                db_chars += line.len();
            }
        }

        // Ensure we don't exceed total budget
        if result.len() > budget_chars {
            result.truncate(budget_chars);
        }

        result
    }

    /// Clear all hot entries.
    pub fn clear_hot(&mut self) {
        self.hot.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_episode_persists_across_reopen() {
        let db_file = "/tmp/test_episode_persist.db";
        let _ = fs::remove_file(db_file);

        // Session 1: push episode, close
        {
            let mut mem = MemoryTiers::open(db_file.as_ref()).expect("open failed");
            mem.push_episode("Goal 'open browser': opened Firefox (5 steps)".to_string())
                .expect("push_episode failed");
        }

        // Session 2: reopen from same path, assert episode survives
        {
            let mem = MemoryTiers::open(db_file.as_ref()).expect("reopen failed");
            let ctx = mem.assemble_context(4096);
            assert!(ctx.contains("open browser") || ctx.contains("opened Firefox"),
                "episode not found in context after reopen: {ctx:?}");
        }

        let _ = fs::remove_file(db_file);
    }

    #[test]
    fn test_decay_all_preserves_cold() {
        let db_file = "/tmp/test_decay_cold.db";
        let _ = fs::remove_file(db_file);

        let mut mem = MemoryTiers::open(db_file.as_ref()).expect("open failed");
        mem.push_episode("important vault entry".to_string()).expect("push failed");

        // Decay 50 cycles — should NOT delete cold entry
        for _ in 0..50 {
            mem.decay_all(0.05).expect("decay failed");
        }

        let ctx = mem.assemble_context(4096);
        assert!(ctx.contains("important vault entry"),
            "cold entry was wrongly deleted by decay: {ctx:?}");

        let _ = fs::remove_file(db_file);
    }

    #[test]
    fn test_memory_tiers_basic() {
        let db_file = "/tmp/test_memory_tiers.db";
        let _ = fs::remove_file(db_file);

        let mut mem = MemoryTiers::open(db_file.as_ref()).expect("Failed to open");

        // Test push_hot
        mem.push_hot("Hot memory entry".to_string());
        assert_eq!(mem.hot.len(), 1);
        assert_eq!(mem.hot[0].temperature, 1.0);

        // Test push_cold
        mem.push_cold("Cold memory entry".to_string()).expect("Failed to push cold");

        // Test reinforce
        let hot_id = mem.hot[0].id.clone();
        mem.reinforce(&hot_id).expect("Failed to reinforce");
        assert_eq!(mem.hot[0].access_count, 1);
        assert!(mem.hot[0].temperature > 1.0 || mem.hot[0].temperature == 1.0);

        // Test clear_hot
        mem.clear_hot();
        assert_eq!(mem.hot.len(), 0);

        let _ = fs::remove_file(db_file);
    }
}
