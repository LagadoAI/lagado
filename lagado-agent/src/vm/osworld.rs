//! vm/osworld.rs — drive an OSWorld benchmark guest through its HTTP API.
//!
//! The OSWorld guest runs a Flask server (`desktop_env/server/main.py`) exposing:
//!   - `GET  /screenshot`     → PNG bytes (with cursor)
//!   - `GET  /accessibility`  → `{"AT": "<AT-SPI XML>"}` (whole-desktop tree)
//!   - `POST /execute`        → runs a command; `{command, shell}` →
//!                              `{status, output, error, returncode}`
//!
//! This module implements the agent's `Perceptor` + `Actuator` traits over that
//! API so the EXISTING harness drives an OSWorld guest unchanged. It mirrors the
//! `SshPerceptor`/`SshActuator` pair: a shared `PerceptionCache`, the
//! `[exit N]\n<stdout>\n[stderr] <stderr>` command contract, and the
//! `ref_N role "label" (x,y,w,h)` screen-read format the perception parsers expect.
//!
//! TRANSPORT: synchronous `ureq` (already the crate's HTTP client for llama-server,
//! the embedder, and health checks). `ureq` is blocking by construction, so it is
//! safe to call from the SYNC trait methods regardless of the (tokio) caller — no
//! nested-runtime hazard, and no new crate dependency / feature toggle.
//!
//! run_command uses `{command: <cmd>, shell: true}` (the server runs it through the
//! shell directly and returns stdout/stderr/returncode) rather than wrapping the
//! command inside an embedded python `subprocess.run` literal — strictly simpler,
//! no double-escaping hazard, and it produces the identical `[exit N]…` contract.
//!
//! GUI actuation (`click`/`type_text`/`key`) resolves the selector → center coords
//! from the shared cache, then `POST /execute` a pyautogui python one-liner.
//!
//! read_screen runs an INLINE pyatspi script on the guest that walks the FOCUSED
//! application's interactive elements and prints the ref-format above (screen-
//! absolute bboxes via `getExtents(pyatspi.XY_SCREEN)`, mirroring main.py's
//! `_create_atspi_node`). The same `parse_ref_coords`/`parse_ref_bboxes` parsers
//! the Ssh pair uses then populate the shared cache, so the cache matches what the
//! rest of the harness parses byte-for-byte.

use crate::perception::{Actuator, Perceptor, PerceptionCache, parse_ref_bboxes, parse_ref_coords};
use std::collections::HashMap;
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// `/execute` is capped server-side at 120s (main.py). The HTTP read timeout must
/// exceed that so a long-but-legitimate command isn't killed by the client before
/// the server replies. Not model/hardware-derived — a property of the guest API.
const EXECUTE_TIMEOUT: Duration = Duration::from_secs(150);
/// Screenshot/accessibility are quick; bound them so a wedged guest can't hang the loop.
const READ_TIMEOUT: Duration = Duration::from_secs(20);

/// Inline pyatspi screen reader, run on the guest via `POST /execute`.
///
/// Walks the focused application's interactive elements and prints one line per
/// element in the format the perception parsers expect:
///     ref_N  role  "label"  (x,y,w,h)
/// with SCREEN-ABSOLUTE coordinates (pyatspi.XY_SCREEN), mirroring main.py's
/// `_create_atspi_node`. Filters to interactive roles and drops garbage/zero-size
/// bboxes (same discipline as perceive.py). On ANY failure it prints a sentinel
/// line so the harness degrades to a stale frame rather than panicking.
const PERCEIVE_PY: &str = r#"
import sys
try:
    import pyatspi
except Exception:
    print('[perception unavailable: pyatspi import failed]'); sys.exit(0)

INTERACTIVE = {
    'push button','toggle button','button','check box','radio button',
    'check menu item','radio menu item','entry','text','password text',
    'spin button','combo box','list','list box','list item','tree',
    'tree item','tree table','link','menu','menu item','menu bar','page tab',
    'tab','tab list','slider','scroll bar','icon','tool bar','document web',
}

def garbage(x, y, w, h):
    return (x < -32768 or x > 32768 or y < -32768 or y > 32768 or w <= 0 or h <= 0)

def find_active(desktop):
    # The application whose window carries the ACTIVE state is the focused app.
    for app in desktop:
        try:
            for win in app:
                st = win.getState()
                if st.contains(pyatspi.STATE_ACTIVE):
                    return app
        except Exception:
            continue
    return None

def walk(node, out, cap=200):
    if len(out) >= cap:
        return
    try:
        role = node.getRoleName()
    except Exception:
        role = ''
    if role in INTERACTIVE:
        try:
            st = node.getState()
            if st.contains(pyatspi.STATE_SHOWING) and st.contains(pyatspi.STATE_VISIBLE):
                comp = node.queryComponent()
                ext = comp.getExtents(pyatspi.XY_SCREEN)
                x, y, w, h = int(ext[0]), int(ext[1]), int(ext[2]), int(ext[3])
                if not garbage(x, y, w, h):
                    name = (node.name or '').replace('"', "'").replace('\n', ' ')
                    out.append('ref_%d  %s  "%s"  (%d,%d,%d,%d)' % (len(out)+1, role, name, x, y, w, h))
        except Exception:
            pass
    try:
        for child in node:
            walk(child, out, cap)
            if len(out) >= cap:
                break
    except Exception:
        pass

try:
    desktop = pyatspi.Registry.getDesktop(0)
    app = find_active(desktop)
    out = []
    if app is not None:
        title = ''
        try:
            for win in app:
                if win.getState().contains(pyatspi.STATE_ACTIVE):
                    title = win.name or ''
                    break
        except Exception:
            pass
        print('[focused: %s]' % (title or app.name or '(unknown)'))
        walk(app, out)
    else:
        # No active window detected — surface every showing interactive element so the
        # agent still has targets (e.g. a bare desktop / panel-only state).
        print('[focused: (desktop)]')
        for a in desktop:
            walk(a, out)
            if len(out) >= 200:
                break
    if not out:
        print('[no interactive elements]')
    for line in out:
        print(line)
except Exception as e:
    print('[perception unavailable: %s]' % e)
"#;

/// Shared HTTP plumbing for the OSWorld guest. Held by both the Perceptor and the
/// Actuator; `base_url` is `http://<host>:<port>` (no hardcoding — constructor args).
#[derive(Clone)]
struct OsworldClient {
    base_url: String,
}

impl OsworldClient {
    fn new(host: &str, port: u16) -> Self {
        Self { base_url: format!("http://{host}:{port}") }
    }

    /// `POST /execute` with `{command, shell}`. Returns the parsed JSON value, or an
    /// Err string on transport failure. `shell=true` runs `cmd` directly through the
    /// guest shell; `shell=false` expects a `["python","-c",...]` command list.
    fn execute(&self, command: serde_json::Value, shell: bool) -> Result<serde_json::Value, String> {
        let body = serde_json::json!({ "command": command, "shell": shell });
        let resp = ureq::post(&format!("{}/execute", self.base_url))
            .timeout(EXECUTE_TIMEOUT)
            .set("Content-Type", "application/json")
            .send_json(body)
            .map_err(|e| format!("execute request failed: {e}"))?;
        resp.into_json::<serde_json::Value>()
            .map_err(|e| format!("execute response parse failed: {e}"))
    }

    /// Run a python source string on the guest (`["python","-c",src]`, shell=false)
    /// and return its captured stdout. Used by the perception and pyautogui paths.
    fn run_python(&self, src: &str) -> Result<String, String> {
        let cmd = serde_json::json!(["python", "-c", src]);
        let json = self.execute(cmd, false)?;
        Ok(json.get("output").and_then(|v| v.as_str()).unwrap_or("").to_string())
    }
}

/// Format an `/execute` JSON response into the `[exit N]\n<stdout>\n[stderr] <stderr>`
/// contract SshActuator uses, so all the harness's verification consumers are
/// unaffected. A `status:"error"` response (server-side exception, e.g. command not
/// found) maps to `[exit -1]` with the message on the stderr line — mirroring
/// `run_command_oneshot`'s `unwrap_or(-1)`. Pure function → unit-tested.
fn format_execute(json: &serde_json::Value) -> String {
    if json.get("status").and_then(|v| v.as_str()) == Some("error") {
        let msg = json.get("message").and_then(|v| v.as_str()).unwrap_or("execute error");
        return format!("[exit -1]\n[stderr] {}", msg.trim_end());
    }
    let code = json.get("returncode").and_then(|v| v.as_i64()).unwrap_or(-1);
    let stdout = json.get("output").and_then(|v| v.as_str()).unwrap_or("");
    let stderr = json.get("error").and_then(|v| v.as_str()).unwrap_or("");
    let mut s = format!("[exit {code}]");
    let o = stdout.trim_end();
    if !o.is_empty() { s.push('\n'); s.push_str(o); }
    let e = stderr.trim_end();
    if !e.is_empty() { s.push_str("\n[stderr] "); s.push_str(e); }
    s
}

// ── Perceptor ───────────────────────────────────────────────────────────────────

pub struct OsworldPerceptor {
    client: OsworldClient,
    pub cache: Arc<Mutex<PerceptionCache>>,
}

impl OsworldPerceptor {
    pub fn new(host: &str, port: u16) -> Self {
        Self::with_cache(host, port, Arc::new(Mutex::new(PerceptionCache::new())))
    }

    pub fn with_cache(host: &str, port: u16, cache: Arc<Mutex<PerceptionCache>>) -> Self {
        Self { client: OsworldClient::new(host, port), cache }
    }
}

impl Perceptor for OsworldPerceptor {
    fn read_screen(&self) -> String {
        let text = match self.client.run_python(PERCEIVE_PY) {
            Ok(out) => {
                let trimmed = out.trim().to_string();
                if trimmed.is_empty() { "[perception unavailable]".to_string() } else { trimmed }
            }
            Err(e) => format!("[perception unavailable: {e}]"),
        };

        // SAME parsers the Ssh pair uses → the cache matches what the harness parses.
        let coords = parse_ref_coords(&text);
        let bboxes = parse_ref_bboxes(&text);
        if let Ok(mut c) = self.cache.lock() {
            c.screen_text = text.clone();
            c.coords = coords;
            c.bboxes = bboxes;
        }
        text
    }

    /// `GET /screenshot` → write the PNG bytes to `config::FRAME_PATH` so the CV
    /// sense reads a fresh in-sync frame. Best-effort: a failure leaves the prior
    /// frame and CV fails open to a11y-only (same contract as SshPerceptor's QMP path).
    fn capture_frame(&self) {
        let resp = match ureq::get(&format!("{}/screenshot", self.client.base_url))
            .timeout(READ_TIMEOUT)
            .call()
        {
            Ok(r) => r,
            Err(_) => return,
        };
        let mut bytes = Vec::new();
        if resp.into_reader().read_to_end(&mut bytes).is_ok() && !bytes.is_empty() {
            let _ = std::fs::write(crate::config::FRAME_PATH, &bytes);
        }
    }
}

// ── Actuator ────────────────────────────────────────────────────────────────────

pub struct OsworldActuator {
    client: OsworldClient,
    pub cache: Arc<Mutex<PerceptionCache>>,
}

impl OsworldActuator {
    pub fn new(host: &str, port: u16) -> Self {
        Self::with_cache(host, port, Arc::new(Mutex::new(PerceptionCache::new())))
    }

    pub fn with_cache(host: &str, port: u16, cache: Arc<Mutex<PerceptionCache>>) -> Self {
        Self { client: OsworldClient::new(host, port), cache }
    }

    /// Run a pyautogui statement on the guest. FAILSAFE is disabled so a corner-bound
    /// move can't abort the action (matches OSWorld's own pyautogui prefix).
    fn pyautogui(&self, stmt: &str) -> Result<String, String> {
        let src = format!("import pyautogui; pyautogui.FAILSAFE = False; {stmt}");
        self.client.run_python(&src)
    }
}

impl Actuator for OsworldActuator {
    fn click(&self, selector: &str) -> String {
        let coords = self.cache.lock().ok().and_then(|c| c.coords.get(selector).copied());
        match coords {
            Some((cx, cy)) => match self.pyautogui(&format!("pyautogui.click({cx}, {cy})")) {
                Ok(_) => format!("Clicked {selector} at ({cx},{cy})"),
                Err(e) => format!("click failed: {e}"),
            },
            None => format!("click failed: {selector} not in screen cache — call read_screen first"),
        }
    }

    fn type_text(&self, selector: &str, text: &str) -> String {
        let _ = self.click(selector);
        // serde_json::to_string yields a valid python str literal for the common cases
        // (handles quotes, backslashes, newlines) — never `{:?}`.
        let lit = serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_string());
        match self.pyautogui(&format!("pyautogui.typewrite({lit})")) {
            Ok(_) => format!("Typed {} chars into {selector}", text.chars().count()),
            Err(e) => format!("type failed: {e}"),
        }
    }

    fn key(&self, key: &str) -> String {
        // CAVEAT: the harness emits xdotool key names (e.g. "Return", "Tab"); pyautogui
        // wants lowercase (e.g. "return"/"enter", "tab"). Lowercase here covers the
        // common single-key cases; an exotic xdotool-only name may not map 1:1.
        let lit = serde_json::to_string(&key.to_lowercase()).unwrap_or_else(|_| "\"\"".to_string());
        match self.pyautogui(&format!("pyautogui.press({lit})")) {
            Ok(_) => format!("Pressed {key}"),
            Err(e) => format!("key failed: {e}"),
        }
    }

    /// The command channel: `POST /execute` with `{command: cmd, shell: true}` — the
    /// guest runs `cmd` through the shell and returns stdout/stderr/returncode, which
    /// we format into the same `[exit N]\n…[stderr]…` contract SshActuator produces.
    ///
    /// FIDELITY CAVEAT — STATELESS, unlike SshActuator. OSWorld's `/execute` runs
    /// `subprocess.run(cmd, shell=True)`: a FRESH process per call. cwd/env do NOT
    /// persist across calls (`cd /tmp` then `pwd` returns $HOME, not /tmp). OSWorld
    /// exposes no persistent-shell endpoint, so cross-step state must be expressed
    /// IN the command — chain dependent steps with `&&` and use absolute paths rather
    /// than relying on a prior `cd`/`export` surviving. `reset_command_session` is
    /// therefore the default no-op: there is no session to reset and no cwd/env leak.
    fn run_command(&self, cmd: &str) -> String {
        match self.client.execute(serde_json::json!(cmd), true) {
            Ok(json) => format_execute(&json),
            Err(e) => format!("[command channel error: {e}]"),
        }
    }

    /// Merge `el_N → center` targets into the shared coord cache so the selection
    /// grammar's tokens resolve to clicks alongside the `ref_N` entries that
    /// `read_screen` populates. Independent of `ref_id`, so vision-only elements work.
    /// (Copied from SshActuator.)
    fn set_targets(&self, targets: HashMap<String, (i32, i32)>) {
        if let Ok(mut c) = self.cache.lock() {
            for (token, center) in targets {
                c.coords.insert(token, center);
            }
        }
    }
}

/// Construct a matched Perceptor/Actuator pair sharing ONE `PerceptionCache`, so a
/// `read_screen()` populates the coords the `click()` resolves — mirroring the Ssh
/// pair's shared-cache contract and `vm::mod`'s `Dynamic*`/pairing pattern.
/// `base_url` is given as host + port (no hardcoded endpoint).
pub fn osworld_pair(host: &str, port: u16) -> (OsworldPerceptor, OsworldActuator) {
    let cache = Arc::new(Mutex::new(PerceptionCache::new()));
    (
        OsworldPerceptor::with_cache(host, port, cache.clone()),
        OsworldActuator::with_cache(host, port, cache),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── format_execute: the [exit N] contract (pure) ──────────────────────────

    #[test]
    fn format_execute_success_stdout_only() {
        let json = serde_json::json!({
            "status": "success", "output": "hello\n", "error": "", "returncode": 0
        });
        assert_eq!(format_execute(&json), "[exit 0]\nhello");
    }

    #[test]
    fn format_execute_includes_stderr() {
        let json = serde_json::json!({
            "status": "success", "output": "out", "error": "boom\n", "returncode": 2
        });
        assert_eq!(format_execute(&json), "[exit 2]\nout\n[stderr] boom");
    }

    #[test]
    fn format_execute_nonzero_no_output() {
        let json = serde_json::json!({
            "status": "success", "output": "", "error": "", "returncode": 1
        });
        assert_eq!(format_execute(&json), "[exit 1]");
    }

    #[test]
    fn format_execute_server_error_maps_to_minus_one() {
        let json = serde_json::json!({ "status": "error", "message": "No such file" });
        let out = format_execute(&json);
        assert!(out.starts_with("[exit -1]"), "got: {out}");
        assert!(out.contains("[stderr] No such file"), "got: {out}");
    }

    #[test]
    fn format_execute_missing_returncode_defaults_minus_one() {
        let json = serde_json::json!({ "status": "success", "output": "x", "error": "" });
        assert_eq!(format_execute(&json), "[exit -1]\nx");
    }

    // ── ref-line format → cache parse round-trip ──────────────────────────────
    // Proves the lines the inline pyatspi script emits are exactly what the
    // harness's parsers consume (center coords + full bbox + label).

    #[test]
    fn ref_line_format_parses_into_cache() {
        // Exactly the shape the PERCEIVE_PY '%s' format string emits.
        let screen = "[focused: Files]\n\
                      ref_1  push button  \"New Folder\"  (10,20,80,30)\n\
                      ref_2  entry  \"Search\"  (100,60,200,25)";
        let coords = parse_ref_coords(screen);
        let bboxes = parse_ref_bboxes(screen);
        // center = (x + w/2, y + h/2)
        assert_eq!(coords.get("ref_1"), Some(&(50, 35)));
        assert_eq!(coords.get("ref_2"), Some(&(200, 72)));
        assert_eq!(bboxes.get("ref_1"), Some(&(10, 20, 80, 30)));
        assert_eq!(bboxes.get("ref_2"), Some(&(100, 60, 200, 25)));
    }

    #[test]
    fn ref_line_empty_label_still_parses_coords() {
        let screen = "ref_3  icon  \"\"  (5,5,40,40)";
        assert_eq!(parse_ref_coords(&screen).get("ref_3"), Some(&(25, 25)));
        assert_eq!(parse_ref_bboxes(&screen).get("ref_3"), Some(&(5, 5, 40, 40)));
    }

    #[test]
    fn perceive_script_emits_parseable_format_string() {
        // Guard against drift: the format literal must keep the ref_N…"label"…(x,y,w,h) shape.
        assert!(PERCEIVE_PY.contains(r#"'ref_%d  %s  "%s"  (%d,%d,%d,%d)'"#));
    }

    #[test]
    fn osworld_pair_shares_one_cache() {
        let (p, a) = osworld_pair("127.0.0.1", 5000);
        p.cache.lock().unwrap().coords.insert("ref_9".to_string(), (11, 22));
        assert_eq!(a.cache.lock().unwrap().coords.get("ref_9"), Some(&(11, 22)));
    }

    #[test]
    fn set_targets_merges_into_shared_cache() {
        let (p, a) = osworld_pair("127.0.0.1", 5000);
        let mut t = HashMap::new();
        t.insert("el_0".to_string(), (5, 6));
        a.set_targets(t);
        assert_eq!(p.cache.lock().unwrap().coords.get("el_0"), Some(&(5, 6)));
    }

    #[test]
    fn type_text_text_is_escaped_not_debug_formatted() {
        // serde_json produces a valid python str literal — quotes/backslashes survive.
        let lit = serde_json::to_string("a\"b\\c").unwrap();
        assert_eq!(lit, "\"a\\\"b\\\\c\"");
    }
}
