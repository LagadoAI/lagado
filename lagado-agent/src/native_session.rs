//! Native Session Plane — host-side driver (Rust) for the resident guest UNO daemon.
//!
//! P2c: the agent's API plane drives a calc doc ONE op at a time WITH per-op observation by
//! talking to the resident `uno_daemon.py` (proven on the real OSWorld bench in P2a). This is
//! the RICHEST rung of the plane ladder; on ANY wedge the caller falls back to the proven
//! stateless one-shot (`api_plane::build_guest_apply`) — the floor, which is NOT modified.
//!
//! Transport: everything goes over the Actuator's `run_command` (the guest `/execute` channel).
//! The three daemon files are embedded with `include_str!` (the committed P1 sources) and pushed
//! via a quoted heredoc — no extra crate, no base64. Requests/responses are line-delimited JSON
//! parsed out of the command stdout.
//!
//! SAFETY mirror of P2a: the global `pkill soffice` (clobber-avoidance for the evaluator's
//! activate_window+ctrl+s) runs ONCE at the DRIVER, before the daemon opens the file. The daemon
//! itself never global-pkills. This is guest-only (the OSWorld sandbox), never a dev host.

use serde_json::Value;
use std::sync::Arc;

use crate::perception::Actuator;

const UNO_OPS_PY: &str = include_str!("../../docs/osworld/uno_ops.py");
const UNO_DAEMON_PY: &str = include_str!("../../docs/osworld/uno_daemon.py");
const UNO_CLIENT_PY: &str = include_str!("../../docs/osworld/uno_client.py");

const GUEST_DIR: &str = "/tmp";
const SOCK: &str = "/tmp/lagado_session.sock";
const HEREDOC: &str = "LAGADO_SESSION_DEPLOY_EOF";

/// A live session over the guest daemon. Holds an `Arc<dyn Actuator>` so it can be moved into a
/// `spawn_blocking` closure (the command channel is sync/blocking by construction).
pub struct NativeSession {
    act: Arc<dyn Actuator>,
    file: String,
    /// guest interpreter that can `import uno` (the daemon needs it); resolved at deploy.
    unopy: String,
}

/// Single-quote a string for the guest shell (`'` → `'\''`). JSON uses double quotes, so the
/// only thing that needs escaping is an apostrophe inside cell text/values.
fn squote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Pull the first line that parses as a JSON object out of `run_command` output (which is
/// prefixed with an `[exit N]` marker and may carry trailing stderr).
fn parse_json(out: &str) -> Result<Value, String> {
    for line in out.lines() {
        let t = line.trim();
        if t.starts_with('{') {
            if let Ok(v) = serde_json::from_str::<Value>(t) {
                return Ok(v);
            }
        }
    }
    Err(format!("no JSON in daemon output: {}", out.chars().take(300).collect::<String>()))
}

fn ok(v: &Value) -> bool {
    v.get("ok").and_then(Value::as_bool).unwrap_or(false)
}

fn err_of(v: &Value) -> String {
    v.get("error").and_then(Value::as_str).unwrap_or("unknown error").to_string()
}

impl NativeSession {
    /// Deploy the daemon files, pick a uno-capable interpreter, kill the reset()-opened GUI +
    /// lock (clobber-avoidance), launch the daemon detached, and `open` the file. On any failure
    /// returns Err so the caller falls back to the one-shot floor. `file` is the GUEST path.
    pub fn deploy_and_open(act: Arc<dyn Actuator>, file: &str) -> Result<NativeSession, String> {
        // 1. push the three files via quoted heredocs (single-quoted delimiter → shell-inert).
        for (name, body) in [("uno_ops.py", UNO_OPS_PY), ("uno_daemon.py", UNO_DAEMON_PY),
                             ("uno_client.py", UNO_CLIENT_PY)] {
            let cmd = format!("cat > {GUEST_DIR}/{name} <<'{HEREDOC}'\n{body}\n{HEREDOC}");
            act.run_command(&cmd);
        }
        // 2. find a guest python that can import uno.
        let unopy = ["python3", "/usr/lib/libreoffice/program/python", "/usr/bin/python3"]
            .into_iter()
            .find(|p| {
                let o = act.run_command(&format!("{p} -c 'import uno; print(\"ok\")' 2>&1"));
                o.contains("ok")
            })
            .ok_or_else(|| "no guest python can import uno".to_string())?
            .to_string();

        // 3. clobber-avoidance: kill the reset()-opened GUI + lock BEFORE the daemon opens.
        let dir = std::path::Path::new(file).parent().and_then(|p| p.to_str()).unwrap_or("/tmp");
        let base = std::path::Path::new(file).file_name().and_then(|p| p.to_str()).unwrap_or("");
        act.run_command("pkill -9 soffice; pkill -9 soffice.bin; true");
        act.run_command(&format!("rm -f '{dir}/.~lock.{base}#' 2>/dev/null; true"));

        // 4. launch the daemon detached (setsid → survives the /execute shell exit).
        act.run_command(&format!(
            "rm -f {SOCK}; setsid {unopy} {GUEST_DIR}/uno_daemon.py --sock={SOCK} \
             > /tmp/daemon.log 2>&1 < /dev/null &"));
        // 5. wait for readiness (the daemon prints DAEMON READY after binding the socket).
        let mut ready = false;
        for _ in 0..20 {
            let log = act.run_command("cat /tmp/daemon.log 2>/dev/null; true");
            if log.contains("DAEMON READY") { ready = true; break; }
            if log.contains("Traceback") {
                return Err(format!("daemon crashed on launch: {}", log.chars().take(400).collect::<String>()));
            }
            std::thread::sleep(std::time::Duration::from_millis(800));
        }
        if !ready {
            return Err("daemon did not signal READY".to_string());
        }

        let s = NativeSession { act, file: file.to_string(), unopy };
        let r = s.call("open", &serde_json::json!({ "file": file }))?;
        if !ok(&r) {
            return Err(format!("open failed: {}", err_of(&r)));
        }
        Ok(s)
    }

    /// Run one uno_client verb, return the parsed JSON response.
    fn call(&self, verb: &str, args: &Value) -> Result<Value, String> {
        let payload = serde_json::to_string(args).map_err(|e| e.to_string())?;
        let cmd = format!(
            "{} {GUEST_DIR}/uno_client.py {verb} {} --sock={SOCK}",
            self.unopy, squote(&payload));
        let out = self.act.run_command(&cmd);
        parse_json(&out)
    }

    /// Apply ONE op to the live doc. Err on a wedge (transport) or a rejected op (so the caller
    /// can drop it from the host-authoritative log and/or fall back).
    pub fn apply(&self, op: &Value) -> Result<(), String> {
        let r = self.call("apply", &serde_json::json!({ "op": op }))?;
        if ok(&r) { Ok(()) } else { Err(err_of(&r)) }
    }

    /// Read a range from the live doc → the `cells` grid (the effect-sensor). `sheet=None` = active.
    pub fn read(&self, sheet: Option<&str>, range: &str) -> Result<Value, String> {
        let r = self.call("read", &serde_json::json!({ "sheet": sheet, "range": range }))?;
        if ok(&r) { Ok(r.get("cells").cloned().unwrap_or(Value::Null)) } else { Err(err_of(&r)) }
    }

    /// Observe sheets/headers/extents of the live doc.
    pub fn structure(&self) -> Result<Value, String> {
        let r = self.call("structure", &serde_json::json!({}))?;
        if ok(&r) { Ok(r) } else { Err(err_of(&r)) }
    }

    /// True if the daemon + its soffice are alive (crash sensor for the drive loop).
    pub fn healthy(&self) -> bool {
        self.call("health", &serde_json::json!({}))
            .map(|r| ok(&r) && r.get("soffice_alive").and_then(Value::as_bool).unwrap_or(false))
            .unwrap_or(false)
    }

    /// Store the corrected xlsx and reload it into a GUI for the evaluator (activate_window+ctrl+s).
    pub fn reconcile(&self) -> Result<(), String> {
        let r = self.call("reconcile", &serde_json::json!({ "gui": true }))?;
        if ok(&r) { Ok(()) } else { Err(err_of(&r)) }
    }

    /// Clean teardown (kill own soffice, rm lock, rm profile, daemon exits). Best-effort.
    pub fn close(&self) {
        let _ = self.call("close", &serde_json::json!({}));
    }

    pub fn file(&self) -> &str { &self.file }
}
