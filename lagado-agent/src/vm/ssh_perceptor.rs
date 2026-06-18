use crate::perception::{Perceptor, PerceptionCache, parse_ref_bboxes, parse_ref_coords};
use std::sync::{Arc, Mutex};

pub struct SshPerceptor {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub cache: Arc<Mutex<PerceptionCache>>,
}

impl SshPerceptor {
    pub fn new(host: &str, port: u16, user: &str) -> Self {
        Self::with_cache(host, port, user, Arc::new(Mutex::new(PerceptionCache::new())))
    }

    pub fn with_cache(host: &str, port: u16, user: &str, cache: Arc<Mutex<PerceptionCache>>) -> Self {
        Self { host: host.to_string(), port, user: user.to_string(), cache }
    }
}

impl Perceptor for SshPerceptor {
    fn read_screen(&self) -> String {
        let text = match std::process::Command::new("ssh")
            .args([
                "-o", "StrictHostKeyChecking=no",
                "-o", "ConnectTimeout=5",
                "-o", "BatchMode=yes",
                "-p", &self.port.to_string(),
                &format!("{}@{}", self.user, self.host),
                "DISPLAY=:0 python3 ~/perceive.py --focused 2>/dev/null || echo '[perception unavailable]'",
            ])
            .output()
        {
            Ok(out) => String::from_utf8_lossy(&out.stdout).trim().to_string(),
            Err(e) => format!("[ssh error: {e}]"),
        };

        let coords = parse_ref_coords(&text);
        let bboxes = parse_ref_bboxes(&text);
        if let Ok(mut c) = self.cache.lock() {
            c.screen_text = text.clone();
            c.coords = coords;
            c.bboxes = bboxes;
        }
        text
    }

    /// Co-capture the frame via a transient QMP screendump to FRAME_PATH (the production frame-sync:
    /// the CV sense reads THIS image, captured at the perception instant, not a stale UI-polled one).
    /// Best-effort — a QMP/connect failure just leaves the prior frame and CV fails open to a11y-only.
    fn capture_frame(&self) {
        if let Ok(mut qmp) = crate::vm::QmpClient::connect(&crate::config::qmp_socket()) {
            let _ = qmp.screendump(crate::config::FRAME_PATH);
        }
    }
}
