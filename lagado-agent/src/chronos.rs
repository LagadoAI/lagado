use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use rusqlite::{params, Connection};

/// One persistent append handle, opened once. The previous `log()` opened+created_dir+wrote+closed
/// the file on EVERY event — a syscall storm on a hot path (the agent loop + sleep-gate log often).
static LOG_FILE: OnceLock<Option<Mutex<File>>> = OnceLock::new();

fn log_handle() -> &'static Option<Mutex<File>> {
    LOG_FILE.get_or_init(|| {
        let p = path();
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        OpenOptions::new().create(true).append(true).open(&p).ok().map(Mutex::new)
    })
}

pub fn log(event: &str) {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // ONE timestamped line per event — sanitize embedded newlines (multi-line events, e.g. a plan
    // preview, would otherwise emit continuation lines with no timestamp and break the format).
    let event = event.replace(['\n', '\r'], " | ");
    let line = format!("{ts}\t{event}\n");
    if let Some(lock) = log_handle() {
        if let Ok(mut f) = lock.lock() {
            let _ = f.write_all(line.as_bytes());
        }
    }
}

fn path() -> PathBuf {
    crate::config::chronos_log()
}

/// Phase 1 snapshot — written once per agent turn.
pub struct ChronosSnapshot {
    pub timestamp:    i64,
    pub active_goal:  String,
    pub last_action:  String,
    pub confidence:   f32,    // 0.0–1.0, placeholder for now
    pub delta:        String, // "unchanged" in Phase 1, full in Phase 4
}

pub struct ChronosDb {
    conn: Connection,
}

impl ChronosDb {
    pub fn open() -> Result<Self, String> {
        let p = crate::config::data_dir().join("chronos.db");
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(&p).map_err(|e| e.to_string())?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS snapshots (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp   INTEGER NOT NULL,
                active_goal TEXT NOT NULL,
                last_action TEXT NOT NULL,
                confidence  REAL NOT NULL,
                delta       TEXT NOT NULL
            );"
        ).map_err(|e| e.to_string())?;
        Ok(Self { conn })
    }

    pub fn write_snapshot(&self, snap: &ChronosSnapshot) -> Result<(), String> {
        self.conn.execute(
            "INSERT INTO snapshots (timestamp, active_goal, last_action, confidence, delta)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![snap.timestamp, snap.active_goal, snap.last_action, snap.confidence, snap.delta],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn recent(&self, n: usize) -> Vec<ChronosSnapshot> {
        let mut stmt = match self.conn.prepare(
            "SELECT timestamp, active_goal, last_action, confidence, delta
             FROM snapshots ORDER BY timestamp DESC LIMIT ?1"
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map(params![n as i64], |row| {
            Ok(ChronosSnapshot {
                timestamp:   row.get(0)?,
                active_goal: row.get(1)?,
                last_action: row.get(2)?,
                confidence:  row.get(3)?,
                delta:        row.get(4)?,
            })
        })
        .ok()
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
    }
}

/// Convenience: log a snapshot from the agent loop.
pub fn snapshot(goal: &str, last_action: &str) {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    if let Ok(db) = ChronosDb::open() {
        let _ = db.write_snapshot(&ChronosSnapshot {
            timestamp:   ts,
            active_goal: goal.to_string(),
            last_action: last_action.to_string(),
            confidence:  1.0,
            delta:       "unchanged".to_string(),
        });
    }
}

/// Initialize the timeline at T=0 (first launch).
pub fn initialize_timeline(user_id: &str) {
    log(&format!("timeline_init: user={user_id}"));
    snapshot("init", "first_launch");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_timestamped_lines() {
        log("goal_received: test goal");
        log("confirm_requested: tap: click(selector=\"ref_1\")");
        log("action: click(selector=\"ref_1\") -> Clicked ref_1");
        log("goal_done: test complete");

        let contents = std::fs::read_to_string(&path()).expect("chronos.log must exist");
        assert!(contents.contains("goal_received: test goal"));
        assert!(contents.contains("confirm_requested: tap:"));
        assert!(contents.contains("action: click"));
        assert!(contents.contains("goal_done: test complete"));
        // every line must have a unix timestamp prefix (10 digits)
        for line in contents.lines() {
            let ts: &str = line.split('\t').next().unwrap_or("");
            assert!(ts.parse::<u64>().is_ok(), "bad timestamp: {ts}");
        }
    }
}
