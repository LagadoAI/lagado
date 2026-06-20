use crate::perception::{Actuator, PerceptionCache};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Sentinel framing one command's exit code on its own line.
const SENTINEL_PREFIX: &str = "__LAGADO_EXIT_";
/// Read timeout on SILENCE (not runtime): a command producing NO output for this long is treated as
/// wedged → kill+respawn. Generous so a long-but-progressing build isn't murdered (command_would_hang
/// already blocks interactive programs up front; this is only the backstop).
const SILENCE_TIMEOUT: Duration = Duration::from_secs(180);

/// A PERSISTENT shell session over SSH — one long-lived `bash` the agent feeds commands to, so cwd, env,
/// and prior effects survive ACROSS the agent's steps (the cross-step state the benchmark needs:
/// `git init`→`add`→`commit` in the SAME repo). The first "kernel-level tool": a real persistent process
/// the agent operates directly, confined to the sovereign VM sandbox. A reader thread drains stdout into
/// a channel so `run` can apply a silence-timeout without blocking forever.
pub(crate) struct ShellSession {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<String>,
}

impl ShellSession {
    fn spawn(host: &str, port: u16, user: &str) -> std::io::Result<Self> {
        let mut child = Command::new("ssh")
            .args([
                "-o", "StrictHostKeyChecking=no",
                "-o", "ConnectTimeout=5",
                "-o", "BatchMode=yes",
                "-p", &port.to_string(),
                &format!("{user}@{host}"),
                // merge stderr→stdout, then a NON-interactive bash reading our commands from the pipe
                // (no -tt/PTY → no echo or prompt noise).
                "exec 2>&1; exec bash",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(l) => if tx.send(l).is_err() { break; },
                    Err(_) => break,
                }
            }
        });
        Ok(Self { child, stdin, rx })
    }

    /// Run one command in the persistent session. Returns the `[exit N]\n<output>` contract string, or
    /// Err if the pipe broke or the command went silent past SILENCE_TIMEOUT (caller respawns/falls back).
    fn run(&mut self, cmd: &str) -> Result<String, ()> {
        let framed = format!("{cmd}\nprintf '\\n{SENTINEL_PREFIX}%d__\\n' \"$?\"\n");
        self.stdin.write_all(framed.as_bytes()).map_err(|_| ())?;
        self.stdin.flush().map_err(|_| ())?;
        let mut out: Vec<String> = Vec::new();
        loop {
            match self.rx.recv_timeout(SILENCE_TIMEOUT) {
                Ok(line) => {
                    if let Some(code) = line.trim()
                        .strip_prefix(SENTINEL_PREFIX)
                        .and_then(|r| r.strip_suffix("__"))
                        .and_then(|n| n.parse::<i32>().ok())
                    {
                        let mut s = format!("[exit {code}]");
                        let body = out.join("\n");
                        let body = body.trim_end();
                        if !body.is_empty() { s.push('\n'); s.push_str(body); }
                        return Ok(s);
                    }
                    out.push(line);
                }
                Err(_) => return Err(()), // silence past the timeout → wedged
            }
        }
    }
}

impl Drop for ShellSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct SshActuator {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub cache: Arc<Mutex<PerceptionCache>>,
    /// The persistent command shell (cross-step state). Lazily spawned; reset per goal; shared across
    /// the ephemeral SshActuators DynamicActuator builds per call.
    session: Arc<Mutex<Option<ShellSession>>>,
}

impl SshActuator {
    pub fn new(host: &str, port: u16, user: &str) -> Self {
        Self::with_cache(host, port, user, Arc::new(Mutex::new(PerceptionCache::new())))
    }

    pub fn with_cache(host: &str, port: u16, user: &str, cache: Arc<Mutex<PerceptionCache>>) -> Self {
        Self::with_session(host, port, user, cache, Arc::new(Mutex::new(None)))
    }

    /// Construct with a SHARED persistent shell session — used by `DynamicActuator`, which builds an
    /// ephemeral `SshActuator` per call; the shared session is what makes cross-step state survive that.
    pub fn with_session(
        host: &str, port: u16, user: &str,
        cache: Arc<Mutex<PerceptionCache>>,
        session: Arc<Mutex<Option<ShellSession>>>,
    ) -> Self {
        Self { host: host.to_string(), port, user: user.to_string(), cache, session }
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

    /// One-shot fallback: a fresh `ssh … <cmd>` (NO session state). Used when the persistent session is
    /// unavailable or wedged — fail-open so the command channel never hangs or dies. Same `[exit N]\n…`
    /// format as the session path so all the verification consumers are unaffected.
    fn run_command_oneshot(&self, cmd: &str) -> String {
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
                if !o.is_empty() { s.push('\n'); s.push_str(o); }
                let e = stderr.trim_end();
                if !e.is_empty() { s.push_str("\n[stderr] "); s.push_str(e); }
                s
            }
            Err(e) => format!("[command channel error: {e}]"),
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
        // PERSISTENT session first (cross-step state: cwd/env survive between steps). Fail-open to a
        // one-shot ssh on any error (broken pipe / silence-wedge) so the channel never hangs or dies.
        if let Ok(mut guard) = self.session.lock() {
            if guard.is_none() {
                *guard = ShellSession::spawn(&self.host, self.port, &self.user).ok();
            }
            if let Some(sess) = guard.as_mut() {
                match sess.run(cmd) {
                    Ok(s) => return s,
                    Err(()) => { *guard = None; } // drop the wedged session (respawn next call), fall through
                }
            }
        }
        self.run_command_oneshot(cmd)
    }

    /// Reset the persistent shell between goals: drop the session → kill the child; the next
    /// `run_command` spawns a fresh shell at `$HOME`, so one goal's cwd/env never leak into the next.
    fn reset_command_session(&self) {
        if let Ok(mut guard) = self.session.lock() {
            *guard = None;
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
    }

    // The cross-step-state proof: cwd and env PERSIST across run_command calls, and reset clears them.
    #[test]
    #[ignore]
    fn persistent_session_keeps_cwd_and_env_until_reset() {
        let act = SshActuator::new("127.0.0.1", 2222, "laputa");
        let _ = act.run_command("cd /tmp");
        let pwd = act.run_command("pwd");
        println!("PWD AFTER cd /tmp:\n{pwd}");
        assert!(pwd.contains("[exit 0]") && pwd.contains("/tmp"), "cwd must persist, got: {pwd}");

        let _ = act.run_command("export LAGADO_X=42");
        let x = act.run_command("echo $LAGADO_X");
        assert!(x.contains("42"), "env must persist, got: {x}");

        // A real multi-step chain across SEPARATE calls (the benchmark shape).
        let _ = act.run_command("rm -rf /tmp/lagado_sess && mkdir /tmp/lagado_sess && cd /tmp/lagado_sess");
        let _ = act.run_command("git init -q");
        let gitdir = act.run_command("test -d .git && echo HAS_GIT");
        assert!(gitdir.contains("HAS_GIT"), ".git must exist in the persisted cwd, got: {gitdir}");

        act.reset_command_session();
        let pwd2 = act.run_command("pwd");
        println!("PWD AFTER reset:\n{pwd2}");
        assert!(!pwd2.contains("/tmp/lagado_sess"), "reset must clear cwd, got: {pwd2}");
    }
}
