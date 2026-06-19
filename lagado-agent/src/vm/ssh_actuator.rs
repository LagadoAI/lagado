use crate::perception::{Actuator, PerceptionCache};
use std::collections::HashMap;
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

    /// The command channel: run `cmd` on the guest over SSH and return its FULL result —
    /// stdout, stderr, and the exit code. Unlike `ssh_run` (the GUI path, which trims to
    /// stdout) this preserves the exit status so the caller can verify success
    /// deterministically. The remote command's exit code surfaces as the SSH process exit
    /// code; an SSH-transport failure shows up as a non-zero code with an empty stdout.
    ///
    /// TIMEOUT SEMANTICS (deliberate): `ConnectTimeout=5` bounds only the SSH *handshake*
    /// (a dead/unreachable VM fails fast instead of hanging) — it does NOT cap how long the
    /// command may RUN. A long-but-progressing command (build, install) runs to completion.
    /// There is intentionally NO hardcoded run-time kill: a fixed guillotine would murder a
    /// working command on a guessed clock (the 3s-settle-ceiling mistake). Hung-command
    /// detection belongs to the observe-and-escalate path (governor-budgeted, escalate-to-
    /// human, never a silent kill), wired at the sequencer/recalibration layer — NOT here.
    fn run_command(&self, cmd: &str) -> String {
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
            Ok(out) => {
                let code = out.status.code().unwrap_or(-1);
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                let mut s = format!("[exit {code}]");
                let o = stdout.trim_end();
                if !o.is_empty() {
                    s.push('\n');
                    s.push_str(o);
                }
                let e = stderr.trim_end();
                if !e.is_empty() {
                    s.push_str("\n[stderr] ");
                    s.push_str(e);
                }
                s
            }
            Err(e) => format!("[command channel error: {e}]"),
        }
    }

    /// Merge `el_N → center` targets into the shared coord cache so the selection
    /// grammar's tokens resolve to clicks alongside the `ref_N` entries that
    /// `read_screen` populates. Independent of `ref_id`, so vision-only elements work.
    fn set_targets(&self, targets: HashMap<String, (i32, i32)>) {
        if let Ok(mut c) = self.cache.lock() {
            for (token, center) in targets {
                c.coords.insert(token, center);
            }
        }
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    use crate::perception::Actuator;

    // Live smoke test of the command channel against a running guest on :2222.
    // Boot the VM first, then: cargo test --lib ssh_actuator -- --ignored --nocapture
    #[test]
    #[ignore]
    fn run_command_live_captures_exit_and_output() {
        let act = SshActuator::new("127.0.0.1", 2222, "laputa");

        let ok = act.run_command("echo LAGADO_CHANNEL_OK");
        println!("OK RESULT:\n{ok}");
        assert!(ok.contains("[exit 0]"), "expected exit 0, got: {ok}");
        assert!(ok.contains("LAGADO_CHANNEL_OK"), "expected marker, got: {ok}");

        // Non-zero exit must surface (deterministic failure detection — the basis of
        // free verification for command tasks).
        let fail = act.run_command("ls /nonexistent_lagado_zzz");
        println!("FAIL RESULT:\n{fail}");
        assert!(!fail.contains("[exit 0]"), "expected non-zero exit, got: {fail}");
        assert!(fail.contains("[stderr]"), "expected stderr captured, got: {fail}");
    }
}
