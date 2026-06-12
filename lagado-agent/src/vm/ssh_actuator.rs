use crate::perception::{Actuator, PerceptionCache, parse_ref_coords};
use std::sync::{Arc, Mutex};

pub struct SshActuator {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub cache: Arc<Mutex<PerceptionCache>>,
}

impl SshActuator {
    pub fn new(host: &str, port: u16, user: &str) -> Self {
        Self::with_cache(host, port, user, Arc::new(Mutex::new(PerceptionCache::new())))
    }

    pub fn with_cache(host: &str, port: u16, user: &str, cache: Arc<Mutex<PerceptionCache>>) -> Self {
        Self { host: host.to_string(), port, user: user.to_string(), cache }
    }

    fn ssh_run(&self, cmd: &str) -> String {
        match std::process::Command::new("ssh")
            .args([
                "-o", "StrictHostKeyChecking=no",
                "-o", "ConnectTimeout=5",
                "-o", "BatchMode=yes",
                "-p", &self.port.to_string(),
                &format!("{}@{}", self.user, self.host),
                cmd,
            ])
            .output()
        {
            Ok(out) => String::from_utf8_lossy(&out.stdout).trim().to_string(),
            Err(e) => format!("ssh error: {e}"),
        }
    }
}

impl Actuator for SshActuator {
    fn click(&self, selector: &str) -> String {
        let coords = self.cache.lock().ok().and_then(|c| c.coords.get(selector).copied());
        match coords {
            Some((cx, cy)) => {
                let out = self.ssh_run(&format!(
                    "DISPLAY=:0 xdotool mousemove --sync {cx} {cy} click 1"
                ));
                // xdotool is silent on success; an empty result tells the model
                // nothing. Return an explicit confirmation so the agent gets a
                // feedback signal it can reason about.
                if out.is_empty() {
                    format!("Clicked {selector} at ({cx},{cy})")
                } else {
                    out
                }
            }
            None => format!("click failed: {selector} not in screen cache — call read_screen first"),
        }
    }

    fn type_text(&self, selector: &str, text: &str) -> String {
        let _ = self.click(selector);
        let out = self.ssh_run(&format!("DISPLAY=:0 xdotool type --clearmodifiers -- {text:?}"));
        if out.is_empty() {
            format!("Typed {} chars into {selector}", text.chars().count())
        } else {
            out
        }
    }

    fn key(&self, key: &str) -> String {
        let out = self.ssh_run(&format!("DISPLAY=:0 xdotool key --clearmodifiers {key}"));
        if out.is_empty() {
            format!("Pressed {key}")
        } else {
            out
        }
    }
}
