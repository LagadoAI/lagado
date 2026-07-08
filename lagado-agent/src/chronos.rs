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

// ── Calendar access (the `recall` tool) ──────────────────────────────────────────
//
// The timeline is PULL, not push: nothing about dates/times is injected into the
// agent's context. When the agent needs to know what happened (episodic memory /
// audit) or what time it is now, it invokes `recall` and reads the answer (user
// doctrine 2026-07-08). Calendar math goes through SQLite's 'localtime' handling —
// exact timezone/DST behavior with no new dependency.

/// One parsed audit-log event.
#[derive(Debug, Clone, PartialEq)]
pub struct LogEvent {
    pub ts: i64,
    pub text: String,
}

fn mem_conn() -> Option<Connection> {
    Connection::open_in_memory().ok()
}

/// Current LOCAL date+time, with weekday — what the agent asks when it needs "now".
pub fn now_local() -> String {
    mem_conn()
        .and_then(|c| {
            c.query_row(
                "SELECT datetime('now','localtime'), CAST(strftime('%w','now','localtime') AS INTEGER)",
                [], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
            ).ok()
        })
        .map(|(dt, w)| {
            let day = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"]
                [(w.rem_euclid(7)) as usize];
            format!("{dt} ({day})")
        })
        .unwrap_or_else(|| "unknown".to_string())
}

/// Today's LOCAL date as "YYYY-MM-DD".
pub fn today_local() -> String {
    mem_conn()
        .and_then(|c| c.query_row("SELECT date('now','localtime')", [], |r| r.get::<_, String>(0)).ok())
        .unwrap_or_default()
}

/// Unix bounds [start, end) of the LOCAL calendar day "YYYY-MM-DD". End is the NEXT
/// day's local midnight (not start+86400 — DST days aren't 24h). None on a malformed day.
pub fn day_bounds(day: &str) -> Option<(i64, i64)> {
    let c = mem_conn()?;
    let q = |expr: &str, arg: &str| -> Option<i64> {
        c.query_row(expr, [arg], |r| r.get::<_, Option<i64>>(0)).ok().flatten()
    };
    let start = q("SELECT CAST(strftime('%s', ?1 || ' 00:00:00', 'utc') AS INTEGER)", day)?;
    let end = q("SELECT CAST(strftime('%s', date(?1, '+1 day') || ' 00:00:00', 'utc') AS INTEGER)", day)?;
    if end <= start { return None; }
    Some((start, end))
}

/// Parse audit-log text into events within [from_ts, to_ts), optionally filtered by a
/// case-insensitive substring. Keeps the LAST `limit` matches (most recent are the ones
/// that matter when the cap bites). PURE — testable with no filesystem.
pub fn parse_log_events(text: &str, from_ts: i64, to_ts: i64, filter: Option<&str>, limit: usize) -> Vec<LogEvent> {
    let needle = filter.map(|f| f.to_lowercase()).filter(|f| !f.is_empty());
    let mut out: Vec<LogEvent> = Vec::new();
    for line in text.lines() {
        let Some((ts_s, ev)) = line.split_once('\t') else { continue };
        let Ok(ts) = ts_s.parse::<i64>() else { continue };
        if ts < from_ts || ts >= to_ts { continue; }
        if let Some(n) = &needle {
            if !ev.to_lowercase().contains(n.as_str()) { continue; }
        }
        out.push(LogEvent { ts, text: ev.to_string() });
    }
    if out.len() > limit {
        out.drain(..out.len() - limit);
    }
    out
}

/// Read the audit log and return events in the window (see `parse_log_events`).
pub fn log_events_between(from_ts: i64, to_ts: i64, filter: Option<&str>, limit: usize) -> Vec<LogEvent> {
    let text = std::fs::read_to_string(path()).unwrap_or_default();
    parse_log_events(&text, from_ts, to_ts, filter, limit)
}

/// Day-bucket events into a calendar view: (local "YYYY-MM-DD", event count), most
/// recent day first, capped to `days` buckets. Exact local-day bucketing via SQLite.
pub fn calendar_from_events(events: &[LogEvent], days: usize) -> Vec<(String, i64)> {
    let Some(c) = mem_conn() else { return vec![] };
    if c.execute_batch("CREATE TABLE e(ts INTEGER);").is_err() { return vec![]; }
    {
        let Ok(mut ins) = c.prepare("INSERT INTO e(ts) VALUES (?1)") else { return vec![] };
        for e in events {
            let _ = ins.execute(params![e.ts]);
        }
    }
    let Ok(mut stmt) = c.prepare(
        "SELECT date(ts,'unixepoch','localtime') d, COUNT(*) FROM e GROUP BY d ORDER BY d DESC LIMIT ?1"
    ) else { return vec![] };
    stmt.query_map(params![days as i64], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
        .ok()
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
}

/// Format a unix ts as local "YYYY-MM-DD HH:MM:SS" using an already-open connection.
fn fmt_local(c: &Connection, ts: i64) -> String {
    c.query_row("SELECT datetime(?1,'unixepoch','localtime')", params![ts], |r| r.get::<_, String>(0))
        .unwrap_or_else(|_| ts.to_string())
}

/// The `recall` tool body. Arguments (all optional, empty string = absent):
///   day   — "YYYY-MM-DD": events on that local day
///   from  — "YYYY-MM-DD": window start (with `to`, or open-ended to now)
///   to    — "YYYY-MM-DD": window end (inclusive day)
///   query — case-insensitive substring filter over event text
///   limit — max events shown (most recent kept)
/// With no window: current date/time + a calendar of recent activity (+ query search
/// over the last 30 days when `query` is given).
pub fn recall(day: &str, from: &str, to: &str, query: &str, limit: usize) -> String {
    let limit = limit.clamp(1, 200);
    let filter = if query.is_empty() { None } else { Some(query) };
    let mut out = format!("now: {}\n", now_local());

    let window: Option<(i64, i64, String)> = if !day.is_empty() {
        day_bounds(day).map(|(a, b)| (a, b, day.to_string()))
    } else if !from.is_empty() {
        let start = day_bounds(from).map(|(a, _)| a);
        let end = if to.is_empty() {
            Some(i64::MAX)
        } else {
            day_bounds(to).map(|(_, b)| b)
        };
        match (start, end) {
            (Some(a), Some(b)) if b > a => {
                let label = if to.is_empty() { format!("{from} → now") } else { format!("{from} → {to}") };
                Some((a, b, label))
            }
            _ => None,
        }
    } else if !query.is_empty() {
        // Query with no window → search the last 30 days.
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        Some((now - 30 * 86_400, i64::MAX, format!("\"{query}\" in the last 30 days")))
    } else {
        None
    };

    match window {
        None if day.is_empty() && from.is_empty() => {
            // Calendar view: recent activity by local day.
            let all = log_events_between(0, i64::MAX, None, usize::MAX);
            let cal = calendar_from_events(&all, 14);
            if cal.is_empty() {
                out.push_str("timeline: empty\n");
            } else {
                out.push_str("calendar (recent days with activity):\n");
                for (d, n) in &cal {
                    out.push_str(&format!("  {d}  {n} events\n"));
                }
                out.push_str("(recall with day=\"YYYY-MM-DD\" to open a day)\n");
            }
            out
        }
        None => {
            out.push_str(&format!("recall failed: malformed window (day={day:?} from={from:?} to={to:?}) — use YYYY-MM-DD\n"));
            out
        }
        Some((a, b, label)) => {
            let events = log_events_between(a, b, filter, limit);
            if events.is_empty() {
                out.push_str(&format!("{label}: no matching events\n"));
                return out;
            }
            out.push_str(&format!("{label} ({} shown, most recent last):\n", events.len()));
            if let Some(c) = mem_conn() {
                for e in &events {
                    out.push_str(&format!("  {}  {}\n", fmt_local(&c, e.ts), e.text));
                }
            } else {
                for e in &events {
                    out.push_str(&format!("  {}  {}\n", e.ts, e.text));
                }
            }
            out
        }
    }
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

    // ── calendar / recall ──────────────────────────────────────────────

    #[test]
    fn day_bounds_cover_a_local_day() {
        let (a, b) = day_bounds("2026-07-08").expect("valid day");
        // A local day is 23–25 hours (DST); anything else means the math broke.
        let len = b - a;
        assert!((23 * 3600..=25 * 3600).contains(&len), "day length {len}s");
        assert!(day_bounds("not-a-date").is_none());
        assert!(day_bounds("").is_none());
    }

    #[test]
    fn parse_log_events_filters_range_substring_and_caps_to_tail() {
        let text = "100\talpha one\n200\tbeta two\n300\tALPHA three\nbadline\n400\tgamma four\n";
        let all = parse_log_events(text, 0, i64::MAX, None, 10);
        assert_eq!(all.len(), 4);
        let ranged = parse_log_events(text, 150, 350, None, 10);
        assert_eq!(ranged.iter().map(|e| e.ts).collect::<Vec<_>>(), vec![200, 300]);
        // case-insensitive substring
        let alpha = parse_log_events(text, 0, i64::MAX, Some("alpha"), 10);
        assert_eq!(alpha.len(), 2);
        // cap keeps the LAST (most recent) matches
        let tail = parse_log_events(text, 0, i64::MAX, None, 2);
        assert_eq!(tail.iter().map(|e| e.ts).collect::<Vec<_>>(), vec![300, 400]);
    }

    #[test]
    fn calendar_buckets_by_local_day() {
        let (a, _) = day_bounds("2026-07-08").unwrap();
        let (c, _) = day_bounds("2026-07-06").unwrap();
        let events = vec![
            LogEvent { ts: a + 60, text: "x".into() },
            LogEvent { ts: a + 120, text: "y".into() },
            LogEvent { ts: c + 60, text: "z".into() },
        ];
        let cal = calendar_from_events(&events, 14);
        assert_eq!(cal, vec![("2026-07-08".to_string(), 2), ("2026-07-06".to_string(), 1)]);
    }

    #[test]
    fn recall_surfaces_now_and_a_logged_event_today() {
        log("recall_marker_e2e: unique probe event");
        let today = today_local();
        assert!(!today.is_empty());
        let out = recall(&today, "", "", "recall_marker_e2e", 20);
        assert!(out.starts_with("now: "), "must carry current datetime: {out}");
        assert!(out.contains("recall_marker_e2e"), "logged event must be recallable: {out}");
        // No-window call = calendar view, still carries now().
        let cal = recall("", "", "", "", 20);
        assert!(cal.starts_with("now: ") && (cal.contains("calendar") || cal.contains("timeline: empty")), "got: {cal}");
        // Malformed window fails closed with guidance, not garbage.
        let bad = recall("yesterday", "", "", "", 20);
        assert!(bad.contains("recall failed") && bad.contains("YYYY-MM-DD"), "got: {bad}");
    }
}
