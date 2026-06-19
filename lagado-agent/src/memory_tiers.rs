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
                access_count INTEGER NOT NULL,
                embedding BLOB
            )",
            [],
        )
        .map_err(|e| format!("Failed to create memory_entries table: {}", e))?;

        // Migrate existing DBs that don't have the embedding column yet
        let _ = db.execute(
            "ALTER TABLE memory_entries ADD COLUMN embedding BLOB",
            [],
        );

        // Board relevance: a SEPARATE text-embedding column. The `embedding` column
        // holds VISUAL vectors (episode frames); the Board needs TEXT vectors (ColBERT)
        // — different spaces, so they cannot share a column. Migration no-ops if present.
        let _ = db.execute(
            "ALTER TABLE memory_entries ADD COLUMN text_embedding BLOB",
            [],
        );

        // WAL mode: concurrent reads don't block the sleep-gate's writes (and vice-versa), and a
        // crash/kill mid-write rolls back atomically — the shutdown path relies on this.
        let _ = db.pragma_update(None, "journal_mode", "WAL");
        // Index the tier column: every `WHERE tier=...` (warm count, entropy prune, similarity scan)
        // was a full table scan — O(N) growing with the store. Now indexed. Partial index on the
        // backfill predicate too (find rows still missing a text embedding without scanning all).
        let _ = db.execute("CREATE INDEX IF NOT EXISTS idx_tier ON memory_entries(tier)", []);
        let _ = db.execute(
            "CREATE INDEX IF NOT EXISTS idx_text_embed_null ON memory_entries(id) WHERE text_embedding IS NULL",
            [],
        );

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

    /// Push a cross-session episode and return its UUID for later embedding attachment.
    pub fn push_episode_id(&mut self, text: String) -> Result<String, String> {
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
        Ok(id)
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

    /// Store a visual embedding for an already-persisted episode.
    /// `embedding` is a mean-pooled float vector (n_embd dims) stored as raw bytes.
    pub fn store_visual_embedding(&mut self, episode_id: &str, embedding: &[f32]) -> Result<(), String> {
        let bytes: Vec<u8> = embedding.iter()
            .flat_map(|f| f.to_le_bytes())
            .collect();
        self.db.execute(
            "UPDATE memory_entries SET embedding = ?1 WHERE id = ?2",
            rusqlite::params![bytes, episode_id],
        ).map_err(|e| format!("store_visual_embedding: {e}"))?;
        Ok(())
    }

    /// Store a TEXT embedding (ColBERT, the Board relevance vector) for an entry, as
    /// raw f32 LE bytes in the `text_embedding` column. Distinct from
    /// `store_visual_embedding` (the `embedding` column = visual vectors).
    pub fn store_text_embedding(&mut self, id: &str, embedding: &[f32]) -> Result<(), String> {
        let bytes: Vec<u8> = embedding.iter().flat_map(|f| f.to_le_bytes()).collect();
        self.db.execute(
            "UPDATE memory_entries SET text_embedding = ?1 WHERE id = ?2",
            rusqlite::params![bytes, id],
        ).map_err(|e| format!("store_text_embedding: {e}"))?;
        Ok(())
    }

    /// (id, PLAINTEXT) of persisted entries lacking a text embedding — for Board backfill.
    /// Cold `text` is ciphertext (hex-encoded AES-GCM); it is DECRYPTED here so the caller
    /// always embeds plaintext. Cold rows whose decryption fails are skipped (a bad key or
    /// corrupt blob must never silently embed ciphertext and poison relevance). Warm rows
    /// are already plaintext and pass through unchanged.
    pub fn entries_missing_text_embedding(&self) -> Vec<(String, String)> {
        let mut stmt = match self.db.prepare(
            "SELECT id, text, tier FROM memory_entries WHERE text_embedding IS NULL",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows: Vec<(String, String, String)> = stmt
            .query_map([], |r| Ok((
                r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?,
            )))
            .map(|it| it.filter_map(|x| x.ok()).collect())
            .unwrap_or_default();

        let passphrase = crate::auth::active_key();
        rows.into_iter()
            .filter_map(|(id, raw_text, tier_s)| {
                if Tier::from_str(&tier_s)? == Tier::Cold {
                    // Cold is ciphertext: decrypt, or skip (never embed ciphertext).
                    let bytes = hex::decode(&raw_text).ok()?;
                    let plain = crate::security::crypto::decrypt(&bytes, &passphrase)
                        .ok()
                        .and_then(|b| String::from_utf8(b).ok())?;
                    Some((id, plain))
                } else {
                    Some((id, raw_text))
                }
            })
            .collect()
    }

    /// Score every persisted entry that has a text embedding by cosine vs `query`.
    /// Cold text is decrypted (raw fallback) like `find_similar_by_embedding`.
    /// Shared candidate builder for `rank_by_relevance` and `assemble_slice`.
    fn scored_candidates(&self, query: &[f32]) -> Vec<(MemoryEntry, f32)> {
        if query.is_empty() {
            return Vec::new();
        }
        let mut stmt = match self.db.prepare(
            "SELECT id, text, tier, temperature, created_at, accessed_at, access_count, text_embedding
             FROM memory_entries WHERE text_embedding IS NOT NULL",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows: Vec<(String, String, String, f32, i64, i64, u32, Vec<u8>)> = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?,
                    r.get::<_, f32>(3)?, r.get::<_, i64>(4)?, r.get::<_, i64>(5)?,
                    r.get::<_, u32>(6)?, r.get::<_, Vec<u8>>(7)?,
                ))
            })
            .map(|it| it.filter_map(|x| x.ok()).collect())
            .unwrap_or_default();

        rows.into_iter()
            .filter_map(|(id, raw_text, tier_s, temp, created, accessed, count, blob)| {
                if blob.is_empty() || blob.len() % 4 != 0 {
                    return None;
                }
                let stored: Vec<f32> = blob
                    .chunks_exact(4)
                    .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                    .collect();
                if stored.len() != query.len() {
                    return None;
                }
                let tier = Tier::from_str(&tier_s)?;
                let cos = crate::vision::cosine_similarity(query, &stored);
                // PLAINTEXT MINIMIZATION: do NOT decrypt cold text here. Scoring uses only the
                // embedding (cosine) — the text isn't needed to rank. Decrypting every cold candidate
                // before truncation materialized the whole vault in plaintext per call; decryption is
                // now deferred to `decrypt_entry`, applied ONLY to the top-k survivors in the callers.
                Some((
                    MemoryEntry {
                        id, text: raw_text, tier, temperature: temp,
                        created_at: created, accessed_at: accessed, access_count: count,
                    },
                    cos,
                ))
            })
            .collect()
    }

    /// Decrypt a cold entry's text IN PLACE (cold text is stored as encrypted hex; hot/warm are
    /// plaintext → no-op). Applied to the top-k survivors AFTER truncation, so a scoring pass never
    /// materializes the whole cold vault in plaintext. Decrypt failure leaves the (encrypted) text,
    /// matching the prior fallback.
    fn decrypt_entry(&self, mut e: MemoryEntry) -> MemoryEntry {
        if e.tier == Tier::Cold {
            if let Ok(bytes) = hex::decode(&e.text) {
                if let Some(plain) = crate::security::crypto::decrypt(&bytes, &crate::auth::active_key())
                    .ok().and_then(|b| String::from_utf8(b).ok())
                {
                    e.text = plain;
                }
            }
        }
        e
    }

    /// Rank persisted entries by PURE text-embedding cosine (the Board's β signal in
    /// isolation). Used for G3 parity against the Python eval and as the relevance
    /// input to `assemble_slice`. Top-k, descending.
    pub fn rank_by_relevance(&self, query: &[f32], top_k: usize) -> Vec<(MemoryEntry, f32)> {
        let mut scored = self.scored_candidates(query);
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);
        // Decrypt ONLY the survivors (plaintext minimization).
        scored.into_iter().map(|(e, c)| (self.decrypt_entry(e), c)).collect()
    }

    /// The Board: assemble the top-k slice by the full Park score
    /// (α·recency + β·relevance + γ·importance), recomputed stateless per call.
    /// Relevance is normalized across the candidate set inside `board::park_scores`.
    pub fn assemble_slice(
        &self,
        query: &[f32],
        top_k: usize,
        weights: &crate::board::ParkWeights,
    ) -> Vec<MemoryEntry> {
        // Board freshness half-life — "fresh for this step", faster than the 30-day
        // forgetting curve. Set by principle (G3 cannot tune α; recency is uniform there).
        const BOARD_RECENCY_HALF_LIFE_SECS: f32 = 86_400.0; // 1 day
        let cands = self.scored_candidates(query);
        if cands.is_empty() {
            return Vec::new();
        }
        let now = now_unix();
        let signals: Vec<crate::board::ParkSignals> = cands
            .iter()
            .map(|(e, cos)| crate::board::ParkSignals {
                recency: crate::board::recency_factor(now - e.accessed_at, BOARD_RECENCY_HALF_LIFE_SECS),
                relevance: *cos,
                importance: crate::board::importance_heuristic(e),
            })
            .collect();
        crate::board::top_k_indices(&signals, weights, top_k)
            .into_iter()
            .map(|i| self.decrypt_entry(cands[i].0.clone())) // decrypt only the top-k survivors
            .collect()
    }

    /// Return the text of the top-K cold episodes most visually similar to `query`.
    /// Loads all cold embeddings into RAM and runs cosine similarity in Rust.
    /// Decrypts episode text before returning.
    pub fn find_similar_by_embedding(&self, query: &[f32], top_k: usize) -> Vec<String> {
        if query.is_empty() || top_k == 0 {
            return Vec::new();
        }

        let mut stmt = match self.db.prepare(
            "SELECT text, embedding FROM memory_entries WHERE tier = 'cold' AND embedding IS NOT NULL"
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };

        let rows: Vec<(String, Vec<u8>)> = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
            })
            .map(|r| r.filter_map(|x| x.ok()).collect())
            .unwrap_or_default();

        let passphrase = crate::auth::active_key();

        let mut scored: Vec<(f32, String)> = rows
            .into_iter()
            .filter_map(|(raw_text, embd_bytes)| {
                // Decode embedding bytes → &[f32]
                if embd_bytes.len() % 4 != 0 {
                    return None;
                }
                let stored: Vec<f32> = embd_bytes
                    .chunks_exact(4)
                    .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                    .collect();

                if stored.len() != query.len() {
                    return None;
                }

                let sim = crate::vision::cosine_similarity(query, &stored);

                // Decrypt the text
                let text = if let Ok(bytes) = hex::decode(&raw_text) {
                    crate::security::crypto::decrypt(&bytes, &passphrase)
                        .ok()
                        .and_then(|b| String::from_utf8(b).ok())
                        .unwrap_or(raw_text)
                } else {
                    raw_text
                };

                Some((sim, text))
            })
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.into_iter().take(top_k).map(|(_, t)| t).collect()
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

        // Decay SQLite entries — EXCLUDING cold. This is the fix for the periodic freeze that grew
        // with use: the old UPDATE rewrote EVERY row including the cold vault (the unbounded tier),
        // so the per-cycle stall grew forever. Cold's temperature is meaningless — cold is never
        // pruned by this threshold (the DELETE below already excludes it; the vault uses a 365-day
        // half-life). So decaying it was pure waste. With `tier != 'cold'` the write is BOUNDED to
        // hot+warm (warm is capped at MAX_WARM_ENTRIES) regardless of total store size — the stall
        // no longer grows. (`tier` is indexed.)
        self.db
            .execute(
                "UPDATE memory_entries SET temperature = temperature * (1.0 - ?1) WHERE tier != 'cold'",
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

    /// Number of entries currently in the hot tier.
    pub fn hot_count(&self) -> usize {
        self.hot.len()
    }

    /// Drain hot entries cooled below `threshold` — removes them from hot Vec and returns them.
    /// Called by sleep_gate to collect entries ready for consolidation.
    pub fn drain_cool_hot(&mut self, threshold: f32) -> Vec<MemoryEntry> {
        let (cool, stay): (Vec<_>, Vec<_>) = self.hot.drain(..).partition(|e| e.temperature < threshold);
        self.hot = stay;
        cool
    }

    /// Write a consolidation summary directly to WARM tier.
    /// Called by sleep_gate after LLM summarizes a batch of cooled hot entries.
    pub fn promote_warm_summary(&mut self, summary: String) -> Result<(), String> {
        let now = now_unix();
        let id = Uuid::new_v4().to_string();
        self.db.execute(
            "INSERT INTO memory_entries (id, text, tier, temperature, created_at, accessed_at, access_count)
             VALUES (?1, ?2, 'warm', 0.8, ?3, ?4, 0)",
            rusqlite::params![&id, &summary, now, now],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Count warm entries currently in SQLite.
    pub fn warm_entry_count(&self) -> usize {
        self.db.query_row(
            "SELECT COUNT(*) FROM memory_entries WHERE tier = 'warm'",
            [],
            |row| row.get::<_, usize>(0),
        ).unwrap_or(0)
    }

    /// Entropy-based pruning of the WARM tier when it exceeds `max_warm` entries.
    ///
    /// Information value per entry:
    ///   V = temperature × e^(−λ × age_secs) × (1 + ln(access_count + 1))
    ///   λ = ln(2) / 30_days  — value halves every 30 days without access
    ///
    /// Entries with lowest V are pruned first. Cold tier is never touched.
    /// Returns the number of entries pruned.
    pub fn entropy_prune_warm(&mut self, max_warm: usize) -> Result<usize, String> {
        let now = now_unix();
        let count = self.warm_entry_count();
        if count <= max_warm {
            return Ok(0);
        }
        let to_prune = count - max_warm;

        let mut stmt = self.db.prepare(
            "SELECT id, temperature, accessed_at, access_count
             FROM memory_entries WHERE tier = 'warm'"
        ).map_err(|e| e.to_string())?;

        let mut scored: Vec<(String, f32)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, f32>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, u32>(3)?,
                ))
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .map(|(id, temp, accessed_at, access_count)| {
                let age_secs = (now - accessed_at).max(0);
                let v = information_value(temp, age_secs, access_count);
                (id, v)
            })
            .collect();

        // Lowest information value pruned first
        scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        let ids: Vec<String> = scored.into_iter().take(to_prune).map(|(id, _)| id).collect();
        let pruned = ids.len();
        for id in &ids {
            self.db.execute(
                "DELETE FROM memory_entries WHERE id = ?1",
                rusqlite::params![id],
            ).map_err(|e| e.to_string())?;
        }
        Ok(pruned)
    }
}

/// Information value of a memory entry.
///
/// V = T × e^(−λ × age_secs) × (1 + ln(n + 1))
///
/// Based on the Ebbinghaus forgetting curve with logarithmic access reinforcement:
/// - T: current temperature (integrated history of decay + reinforcement)
/// - e^(−λt): recency factor — value halves every 30 days without access
/// - (1 + ln(n+1)): each access compounds but with diminishing returns
pub fn information_value(temperature: f32, age_secs: i64, access_count: u32) -> f32 {
    const HALF_LIFE_SECS: f64 = 30.0 * 86400.0;
    let lambda = std::f64::consts::LN_2 / HALF_LIFE_SECS;
    let recency = (-(lambda * age_secs.max(0) as f64)).exp() as f32;
    let reinforcement = 1.0_f32 + (access_count as f32 + 1.0).ln();
    temperature * recency * reinforcement
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_episode_persists_across_reopen() {
        let db_file = std::env::temp_dir().join("test_episode_persist.db");
        let _ = fs::remove_file(&db_file);

        // Session 1: push episode, close
        {
            let mut mem = MemoryTiers::open(&db_file).expect("open failed");
            mem.push_episode("Goal 'open browser': opened Firefox (5 steps)".to_string())
                .expect("push_episode failed");
        }

        // Session 2: reopen from same path, assert episode survives
        {
            let mem = MemoryTiers::open(&db_file).expect("reopen failed");
            let ctx = mem.assemble_context(4096);
            assert!(ctx.contains("open browser") || ctx.contains("opened Firefox"),
                "episode not found in context after reopen: {ctx:?}");
        }

        let _ = fs::remove_file(&db_file);
    }

    #[test]
    fn test_decay_all_preserves_cold() {
        let db_file = std::env::temp_dir().join("test_decay_cold.db");
        let _ = fs::remove_file(&db_file);

        let mut mem = MemoryTiers::open(&db_file).expect("open failed");
        mem.push_episode("important vault entry".to_string()).expect("push failed");

        // Decay 50 cycles — should NOT delete cold entry
        for _ in 0..50 {
            mem.decay_all(0.05).expect("decay failed");
        }

        let ctx = mem.assemble_context(4096);
        assert!(ctx.contains("important vault entry"),
            "cold entry was wrongly deleted by decay: {ctx:?}");

        let _ = fs::remove_file(&db_file);
    }

    #[test]
    fn test_memory_tiers_basic() {
        let db_file = std::env::temp_dir().join("test_memory_tiers.db");
        let _ = fs::remove_file(&db_file);

        let mut mem = MemoryTiers::open(&db_file).expect("Failed to open");

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

    /// Parity: the Rust ColBERT cosine path (f32 BLOB round-trip) must reproduce the
    /// Python G3 ranking (eval_g3_embed.py, F1=0.52). Needs the embedding server on
    /// :8082 and the seeded ~/.laputa-secure/g3_eval.db.
    /// Run: `cargo test -p lagado-agent -- --ignored g3_relevance_parity --nocapture`
    #[test]
    #[ignore]
    fn g3_relevance_parity_with_python() {
        let home = std::env::var("HOME").expect("HOME");
        let db = std::path::PathBuf::from(home).join(".laputa-secure/g3_eval.db");
        assert!(db.exists(), "seed the eval DB first: {db:?}");

        let mut mem = MemoryTiers::open(&db).expect("open eval db");

        // Backfill text embeddings from plaintext (idempotent — eval DB only).
        let missing = mem.entries_missing_text_embedding();
        eprintln!("backfilling {} text embeddings...", missing.len());
        for (id, text) in &missing {
            let emb = crate::embedding::embed(text).expect("embed (is :8082 up?)");
            mem.store_text_embedding(id, &emb).expect("store");
        }

        for q in [
            "open Firefox and navigate to google",
            "what happened in the browser earlier",
            "run a shell command",
            "what was the last terminal command",
            "move a file to another folder",
            "find a file in the project",
        ] {
            let qv = crate::embedding::embed(q).expect("embed query");
            let top = mem.rank_by_relevance(&qv, 15);
            eprintln!("\nQ: '{q}'");
            for (e, cos) in top.iter().take(5) {
                let snip: String = e.text.chars().take(58).collect();
                eprintln!("   cos={cos:.3}  [{}]  {snip}", e.tier.as_str());
            }
            // Regression guard: a broken BLOB round-trip would scramble the ranking.
            if q == "open Firefox and navigate to google" {
                assert!(top[0].0.text.to_lowercase().contains("firefox"),
                    "top-1 must be a Firefox entry, got: {}", top[0].0.text);
            }
            if q == "move a file to another folder" {
                let t = top[0].0.text.to_lowercase();
                assert!(["file", "folder", "download", "document"].iter().any(|w| t.contains(w)),
                    "top-1 must be a files entry, got: {}", top[0].0.text);
            }
        }
    }
}
