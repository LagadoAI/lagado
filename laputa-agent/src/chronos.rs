use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn log(event: &str) {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let line = format!("{ts}\t{event}\n");
    let p = path();
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&p) {
        let _ = f.write_all(line.as_bytes());
    }
}

fn path() -> PathBuf {
    crate::config::chronos_log()
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
