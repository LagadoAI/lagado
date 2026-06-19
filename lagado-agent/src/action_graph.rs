//! action_graph.rs — Persistent action graph for Laputa warm-start memory.
//!
//! Stores a SQLite graph of (screen_state_hash → action) edges with success/failure
//! counts. Before each LLM call the agent checks the graph; if a high-confidence
//! action exists it is returned directly, bypassing inference entirely.
//!
//! # Usage
//! ```rust,ignore
//! let graph = ActionGraph::open("/home/user/laputa/vault/action_graph.db")?;
//!
//! // Before LLM call:
//! if let Some(action) = graph.get_best_action(&state_hash, 0.85)? {
//!     execute(action); // skip LLM
//! }
//!
//! // After execution outcome:
//! graph.record_outcome(&state_hash, &action_json, success)?;
//!
//! // During SleepGate pruning cycle:
//! let pruned = graph.prune_low_probability(0.2)?;
//! ```
//!
//! Add to Cargo.toml:
//!   rusqlite = { version = "0.31", features = ["bundled"] }

use rusqlite::{params, Connection, OpenFlags};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

// ── Cache ─────────────────────────────────────────────────────────────────────

/// In-memory cache entry: (action, success_count, failure_count, last_used)
type CacheEntry = (String, i64, i64, f64);

/// Cache key = state_hash
type Cache = HashMap<String, Vec<CacheEntry>>;

// ── Retry config ──────────────────────────────────────────────────────────────

const DB_RETRIES: usize = 3;
const DB_RETRY_MS: u64 = 10;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn now_unix() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn probability(success: i64, failure: i64) -> f64 {
    let total = success + failure;
    if total == 0 {
        0.0
    } else {
        success as f64 / total as f64
    }
}

/// Retry wrapper: retries `f` up to `DB_RETRIES` times on SQLite busy/locked.
fn with_retry<T, F>(mut f: F) -> Result<T, String>
where
    F: FnMut() -> Result<T, rusqlite::Error>,
{
    let mut last_err = String::new();
    for attempt in 0..=DB_RETRIES {
        match f() {
            Ok(val) => return Ok(val),
            Err(e) => {
                last_err = e.to_string();
                // Only retry on database locked / busy
                let s = last_err.to_lowercase();
                if s.contains("locked") || s.contains("busy") {
                    if attempt < DB_RETRIES {
                        std::thread::sleep(std::time::Duration::from_millis(
                            DB_RETRY_MS * (attempt as u64 + 1),
                        ));
                        continue;
                    }
                }
                break;
            }
        }
    }
    Err(format!("SQLite error after {DB_RETRIES} retries: {last_err}"))
}

// ── ActionGraph ───────────────────────────────────────────────────────────────

pub struct ActionGraph {
    conn:  Arc<Mutex<Connection>>,
    cache: Arc<Mutex<Cache>>,
}

impl ActionGraph {
    // ── Construction ──────────────────────────────────────────────────────────

    /// Open (or create) the action graph database at `path`.
    /// Use `":memory:"` for an ephemeral in-memory database (useful for tests).
    pub fn open(path: &str) -> Result<Self, String> {
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX;

        let conn = Connection::open_with_flags(path, flags)
            .map_err(|e| format!("Cannot open database at '{}': {}", path, e))?;

        // WAL mode: better concurrency, allows readers while writing
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .map_err(|e| format!("PRAGMA setup failed: {e}"))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS edges (
                state_hash    TEXT NOT NULL,
                action        TEXT NOT NULL,
                success_count INTEGER NOT NULL DEFAULT 0,
                failure_count INTEGER NOT NULL DEFAULT 0,
                last_used     REAL    NOT NULL DEFAULT (unixepoch()),
                PRIMARY KEY (state_hash, action)
            );
            CREATE INDEX IF NOT EXISTS idx_edges_state
                ON edges (state_hash);",
        )
        .map_err(|e| format!("Schema creation failed: {e}"))?;

        // Migration: add verified_success if not present (ignore error if column exists)
        let _ = conn.execute(
            "ALTER TABLE edges ADD COLUMN verified_success INTEGER NOT NULL DEFAULT 0",
            [],
        );

        Ok(Self {
            conn:  Arc::new(Mutex::new(conn)),
            cache: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    // ── Public API ────────────────────────────────────────────────────────────

    /// Return the best action for `state_hash` if its probability ≥ `min_confidence`.
    /// Also updates `last_used` in the database for the returned action.
    /// Returns `None` if no qualifying action exists.
    pub fn get_best_action(
        &self,
        state_hash: &str,
        min_confidence: f64,
    ) -> Result<Option<String>, String> {
        // 1. Try cache first
        {
            let cache = self.cache.lock().unwrap();
            if let Some(entries) = cache.get(state_hash) {
                return Ok(Self::best_from_entries(entries, min_confidence));
            }
        }

        // 2. Miss — load from DB
        let entries = self.load_entries(state_hash)?;
        let best = Self::best_from_entries(&entries, min_confidence);

        // 3. Store in cache
        {
            let mut cache = self.cache.lock().unwrap();
            cache.insert(state_hash.to_string(), entries);
        }

        // 4. Update last_used in DB if we're returning something
        if let Some(ref action) = best {
            let ts = now_unix();
            let hash = state_hash.to_string();
            let act  = action.clone();
            let conn = Arc::clone(&self.conn);
            with_retry(|| {
                conn.lock().unwrap().execute(
                    "UPDATE edges SET last_used = ?1 WHERE state_hash = ?2 AND action = ?3",
                    params![ts, hash, act],
                )
            })?;
        }

        Ok(best)
    }

    /// Record the outcome of executing `action` from `state_hash`.
    /// Inserts the edge if it doesn't exist; increments success or failure count.
    pub fn record_outcome(
        &self,
        state_hash: &str,
        action: &str,
        success: bool,
    ) -> Result<(), String> {
        let ts = now_unix();
        let conn = Arc::clone(&self.conn);

        with_retry(|| {
            conn.lock().unwrap().execute(
                "INSERT INTO edges (state_hash, action, success_count, failure_count, last_used)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(state_hash, action) DO UPDATE SET
                     success_count = success_count + excluded.success_count,
                     failure_count = failure_count + excluded.failure_count,
                     last_used     = excluded.last_used",
                params![
                    state_hash,
                    action,
                    if success { 1i64 } else { 0i64 },
                    if success { 0i64 } else { 1i64 },
                    ts,
                ],
            )
        })?;

        // Invalidate cache for this state
        let mut cache = self.cache.lock().unwrap();
        cache.remove(state_hash);

        Ok(())
    }

    /// Delete all edges whose probability < `threshold`.
    /// Returns the number of deleted edges.
    /// Call during the SleepGate / dream cycle.
    pub fn prune_low_probability(&self, threshold: f64) -> Result<usize, String> {
        // We can't do floating-point probability in SQLite WHERE directly,
        // so we compute it as success / (success + failure) using integer arithmetic.
        // Equivalent: success_count < threshold * (success_count + failure_count)
        // Rewritten:  success_count * 1.0 / (success_count + failure_count) < threshold
        // SQLite supports CAST so we use: CAST(success_count AS REAL) / ...
        let conn = Arc::clone(&self.conn);
        let deleted = with_retry(|| {
            conn.lock().unwrap().execute(
                "DELETE FROM edges
                 WHERE (success_count + failure_count) > 0
                   AND CAST(success_count AS REAL) / (success_count + failure_count) < ?1",
                params![threshold],
            )
        })?;

        // Pruning invalidates everything — clear whole cache
        let mut cache = self.cache.lock().unwrap();
        cache.clear();

        Ok(deleted)
    }

    /// Return (total_edges, average_probability).
    /// Used for monitoring and logging.
    pub fn get_statistics(&self) -> Result<(usize, f64), String> {
        let conn = self.conn.lock().unwrap();
        let (count, avg_prob): (i64, f64) = conn
            .query_row(
                "SELECT
                    COUNT(*),
                    COALESCE(
                        AVG(
                            CASE WHEN (success_count + failure_count) > 0
                                 THEN CAST(success_count AS REAL) / (success_count + failure_count)
                                 ELSE 0.0
                            END
                        ),
                        0.0
                    )
                 FROM edges",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| format!("Statistics query failed: {e}"))?;

        Ok((count as usize, avg_prob))
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    fn load_entries(&self, state_hash: &str) -> Result<Vec<CacheEntry>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT action, success_count, failure_count, last_used
                 FROM edges
                 WHERE state_hash = ?1
                 ORDER BY last_used DESC",
            )
            .map_err(|e| format!("Prepare failed: {e}"))?;

        let rows = stmt
            .query_map(params![state_hash], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, f64>(3)?,
                ))
            })
            .map_err(|e| format!("Query failed: {e}"))?;

        let mut entries = Vec::new();
        for row in rows {
            entries.push(row.map_err(|e| format!("Row error: {e}"))?);
        }
        Ok(entries)
    }

    fn best_from_entries(entries: &[CacheEntry], min_confidence: f64) -> Option<String> {
        entries
            .iter()
            .map(|(action, s, f, ts)| (action, probability(*s, *f), *ts))
            .filter(|(_, prob, _)| *prob >= min_confidence)
            // Primary: highest probability; secondary: most recent last_used
            .max_by(|(_, p1, ts1), (_, p2, ts2)| {
                p1.partial_cmp(p2)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(ts1.partial_cmp(ts2).unwrap_or(std::cmp::Ordering::Equal))
            })
            .map(|(action, _, _)| action.clone())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_graph() -> ActionGraph {
        ActionGraph::open(":memory:").expect("in-memory DB failed")
    }

    #[test]
    fn test_create_and_insert() {
        let g = mem_graph();
        g.record_outcome("hash1", r#"{"tool":"click","selector":"btn"}"#, true)
            .unwrap();
        let (count, _) = g.get_statistics().unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_get_best_action_above_threshold() {
        let g = mem_graph();
        let action = r#"{"tool":"click","selector":"submit"}"#;
        // 3 successes, 1 failure → 0.75
        for _ in 0..3 {
            g.record_outcome("s1", action, true).unwrap();
        }
        g.record_outcome("s1", action, false).unwrap();

        // Should return at 0.70 threshold
        let result = g.get_best_action("s1", 0.70).unwrap();
        assert_eq!(result, Some(action.to_string()));
    }

    #[test]
    fn cache_hit_does_not_write_last_used() {
        // PIN the "bypass inference" fast path: a cache HIT must be a pure READ — it returns before
        // the last_used UPDATE. (The audit wrongly claimed it wrote on every hit; it returns early.
        // This test makes that early-return permanent: if a refactor moves the write before the cache
        // return, the hot read path silently becomes a disk fsync and this fails.)
        let g = mem_graph();
        let action = r#"{"tool":"click","selector":"submit"}"#;
        g.record_outcome("s1", action, true).unwrap();
        // First call → ensures the state is cached.
        assert!(g.get_best_action("s1", 0.0).unwrap().is_some());
        // Stamp a sentinel last_used directly, then do a cache HIT.
        g.conn.lock().unwrap()
            .execute("UPDATE edges SET last_used = 1.0 WHERE state_hash = 's1'", [])
            .unwrap();
        assert!(g.get_best_action("s1", 0.0).unwrap().is_some()); // cache hit
        let lu: f64 = g.conn.lock().unwrap()
            .query_row("SELECT last_used FROM edges WHERE state_hash = 's1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(lu, 1.0, "cache-hit path wrote last_used — the read-only early-return regressed");
    }

    #[test]
    fn test_get_best_action_below_threshold() {
        let g = mem_graph();
        let action = r#"{"tool":"click","selector":"submit"}"#;
        // 1 success, 3 failures → 0.25
        g.record_outcome("s2", action, true).unwrap();
        for _ in 0..3 {
            g.record_outcome("s2", action, false).unwrap();
        }

        // Should return None at 0.80 threshold
        let result = g.get_best_action("s2", 0.80).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_unknown_state_returns_none() {
        let g = mem_graph();
        let result = g.get_best_action("nonexistent_hash", 0.5).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_record_success_and_failure_counts() {
        let g = mem_graph();
        let action = r#"{"tool":"type","text":"hello"}"#;
        g.record_outcome("s3", action, true).unwrap();
        g.record_outcome("s3", action, true).unwrap();
        g.record_outcome("s3", action, false).unwrap();

        // 2/3 ≈ 0.667
        let result = g.get_best_action("s3", 0.60).unwrap();
        assert!(result.is_some());
        let result = g.get_best_action("s3", 0.70).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_prune_low_probability() {
        let g = mem_graph();
        // Good edge: 4 success, 1 fail → 0.80
        g.record_outcome("s4", r#"{"tool":"click","selector":"a"}"#, true).unwrap();
        g.record_outcome("s4", r#"{"tool":"click","selector":"a"}"#, true).unwrap();
        g.record_outcome("s4", r#"{"tool":"click","selector":"a"}"#, true).unwrap();
        g.record_outcome("s4", r#"{"tool":"click","selector":"a"}"#, true).unwrap();
        g.record_outcome("s4", r#"{"tool":"click","selector":"a"}"#, false).unwrap();
        // Bad edge: 1 success, 4 fail → 0.20
        g.record_outcome("s5", r#"{"tool":"click","selector":"b"}"#, true).unwrap();
        g.record_outcome("s5", r#"{"tool":"click","selector":"b"}"#, false).unwrap();
        g.record_outcome("s5", r#"{"tool":"click","selector":"b"}"#, false).unwrap();
        g.record_outcome("s5", r#"{"tool":"click","selector":"b"}"#, false).unwrap();
        g.record_outcome("s5", r#"{"tool":"click","selector":"b"}"#, false).unwrap();

        let pruned = g.prune_low_probability(0.5).unwrap();
        assert_eq!(pruned, 1);

        let (count, _) = g.get_statistics().unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_cache_invalidated_after_write() {
        let g = mem_graph();
        let action = r#"{"tool":"wait","ms":500}"#;

        // Prime the cache with 1 success
        g.record_outcome("s6", action, true).unwrap();
        let _ = g.get_best_action("s6", 0.5).unwrap(); // loads into cache

        // Add 4 failures — should invalidate cache
        for _ in 0..4 {
            g.record_outcome("s6", action, false).unwrap();
        }

        // Now probability is 1/5 = 0.20 — should not meet 0.50 threshold
        // If cache was NOT invalidated, the old 1/1=1.0 entry would be returned (wrong)
        let result = g.get_best_action("s6", 0.50).unwrap();
        assert!(result.is_none(), "Cache should have been invalidated after write");
    }

    #[test]
    fn test_prefers_higher_probability_when_tie_broken_by_recency() {
        let g = mem_graph();
        let a1 = r#"{"tool":"click","selector":"x"}"#;
        let a2 = r#"{"tool":"click","selector":"y"}"#;

        // a1: 2/2 = 1.0
        g.record_outcome("s7", a1, true).unwrap();
        g.record_outcome("s7", a1, true).unwrap();
        // a2: 1/2 = 0.5
        g.record_outcome("s7", a2, true).unwrap();
        g.record_outcome("s7", a2, false).unwrap();

        let result = g.get_best_action("s7", 0.5).unwrap();
        assert_eq!(result, Some(a1.to_string()));
    }

    #[test]
    fn test_statistics_empty_graph() {
        let g = mem_graph();
        let (count, avg) = g.get_statistics().unwrap();
        assert_eq!(count, 0);
        assert_eq!(avg, 0.0);
    }
}
