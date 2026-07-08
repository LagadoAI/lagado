//! perception/mod.rs — Screen reading (Perceptor) and input actuation (Actuator).
//!
//! Platform-specific implementations (e.g. Windows UIAutomation) drop in behind
//! these traits. `MockPerceptor`/`MockActuator` let the agent core build and run
//! on any OS and in CI with no desktop dependency.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use regex::Regex;

/// Reads the focused window's interactive elements as a text dump.
pub trait Perceptor: Send + Sync {
    fn read_screen(&self) -> String;

    /// Capture the current screen frame to `config::FRAME_PATH`, synced with the perception instant,
    /// so the CV sense reads a fresh in-sync image rather than a stale UI-polled one. Call it on a
    /// SETTLED screen (after `read_settled_screen`), so the frame matches the a11y read the loop acts
    /// on. Default no-op (mock/host perceptors with no frame source); `SshPerceptor` does a QMP
    /// screendump. Best-effort: a capture failure leaves CV to fail-open to a11y-only.
    fn capture_frame(&self) {}

    /// Control whether perception PRUNES descent into in-document subtrees (spreadsheet/document cell
    /// grids). Default ON (fast, bounded) — correct when the API plane owns in-document content. The
    /// agent flips it OFF when it falls through to the GUI plane for an in-app-semantic task, so the
    /// selection loop can SEE in-document targets (still bounded by the perception deadline). Default
    /// no-op (perceptors without a document-heavy a11y tree). See `lagado-perception-latency-bug`.
    fn set_document_pruning(&self, _on: bool) {}
}

/// Mouse button for pointer actions. Maps to xdotool button numbers (1/2/3) and
/// pyautogui button names ("left"/"middle"/"right") in the backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton { Left, Middle, Right }

impl MouseButton {
    pub fn xdotool(self) -> u8 {
        match self { MouseButton::Left => 1, MouseButton::Middle => 2, MouseButton::Right => 3 }
    }
    pub fn pyautogui(self) -> &'static str {
        match self { MouseButton::Left => "left", MouseButton::Middle => "middle", MouseButton::Right => "right" }
    }
}

/// A raw pointer-level motor action — the full mouse vocabulary beneath the selector
/// `click()`. Coordinate variants are the substrate a trained motor tier drives directly;
/// selector variants resolve through the same perception cache `click()` uses, so both
/// address modes share one contract. `count` = clicks (1 single, 2 double).
///
/// Direction convention: `Scroll { dx, dy }` is in WHEEL CLICKS; +dy scrolls DOWN
/// (content moves up), +dx scrolls RIGHT — the wheel's own frame, identical across
/// backends (xdotool buttons 4/5/6/7, pyautogui scroll/hscroll signs are mapped).
#[derive(Debug, Clone, PartialEq)]
pub enum PointerAction {
    ClickAt { x: i32, y: i32, button: MouseButton, count: u8 },
    ClickOn { selector: String, button: MouseButton, count: u8 },
    MoveTo  { x: i32, y: i32 },
    Hover   { selector: String },
    Scroll  { dx: i32, dy: i32 },
    Drag    { x1: i32, y1: i32, x2: i32, y2: i32, button: MouseButton },
    DragTo  { from: String, to: String, button: MouseButton },
}

impl PointerAction {
    /// Human-readable confirmation for silent-on-success backends (xdotool prints
    /// nothing) — the same feedback-signal discipline as `Actuator::click`.
    pub fn describe(&self) -> String {
        match self {
            PointerAction::ClickAt { x, y, button, count } =>
                format!("Clicked ({x},{y}) {:?} x{count}", button),
            PointerAction::ClickOn { selector, button, count } =>
                format!("Clicked {selector} {:?} x{count}", button),
            PointerAction::MoveTo { x, y } => format!("Moved pointer to ({x},{y})"),
            PointerAction::Hover { selector } => format!("Hovering {selector}"),
            PointerAction::Scroll { dx, dy } => format!("Scrolled dx={dx} dy={dy}"),
            PointerAction::Drag { x1, y1, x2, y2, button } =>
                format!("Dragged ({x1},{y1})→({x2},{y2}) {:?}", button),
            PointerAction::DragTo { from, to, button } =>
                format!("Dragged {from}→{to} {:?}", button),
        }
    }
}

/// Performs user-input actions on the desktop.
pub trait Actuator: Send + Sync {
    fn click(&self, selector: &str) -> String;
    fn type_text(&self, selector: &str, text: &str) -> String;
    fn key(&self, key: &str) -> String;

    /// The completed motor surface: right/double/middle click, hover, wheel scroll,
    /// drag — by coordinate OR selector. Default: unavailable (mock/pre-platform
    /// backends), same fail-open contract as `run_command`. VM-backed actuators
    /// implement it over their native input channel (xdotool / pyautogui).
    fn pointer(&self, _action: &PointerAction) -> String {
        "[pointer channel unavailable]".to_string()
    }

    /// Run a shell command on the target and return its output (stdout/stderr plus an
    /// `[exit N]` marker). Default: unavailable — only a VM-backed actuator implements a
    /// real command channel. This is the agent's CLI sense: it RUNS commands and reads
    /// their output directly, because terminal text is not exposed through a11y (verified
    /// on the live guest — the AT-SPI Text interface returns empty for the VTE terminal).
    fn run_command(&self, _cmd: &str) -> String {
        "[command channel unavailable]".to_string()
    }

    /// Reset the persistent command-shell session between goals so cwd/env from one goal don't leak
    /// into the next. Default no-op; the VM-backed actuator drops + respawns its persistent `bash`.
    fn reset_command_session(&self) {}

    /// Register synthetic-index targets (`el_N` → center coords) for the current
    /// frame so the selection grammar's tokens resolve to coordinate clicks. Works
    /// for label-less / `ref_id`-`None` elements too. Default no-op; coord-cache
    /// actuators (e.g. `SshActuator`) override it to merge into their shared cache.
    fn set_targets(&self, _targets: HashMap<String, (i32, i32)>) {}
}

/// Canned perceptor for tests / headless CI / pre-platform-impl development.
pub struct MockPerceptor;

impl Perceptor for MockPerceptor {
    fn read_screen(&self) -> String {
        "[focused: Mock Window]\n\
         [window: x=0 y=0 w=1280 h=720]\n  \
         ref_1  button   \"OK\"      state=enabled\n  \
         ref_2  entry    \"Search\"  state=editable"
            .to_string()
    }
}

/// Echoing actuator for tests / headless CI / pre-platform-impl development.
pub struct MockActuator;

impl Actuator for MockActuator {
    fn click(&self, selector: &str) -> String {
        format!("Clicked {selector}")
    }
    fn type_text(&self, selector: &str, text: &str) -> String {
        format!("Typed {} chars into {}", text.chars().count(), selector)
    }
    fn key(&self, key: &str) -> String {
        format!("Pressed {key}")
    }
    fn pointer(&self, action: &PointerAction) -> String {
        action.describe()
    }
}

/// Coordinate cache shared between a matched Perceptor/Actuator pair.
///
/// `coords` — ref_id → (center_x, center_y) in VM screen pixels. Used by actuators for targeting.
/// `bboxes` — ref_id → (x, y, w, h) in VM screen pixels. Used by the fusion harness for IoU.
///
/// Both are populated after every `read_screen()` call. Actuators read only `coords` and are
/// unaffected by `bboxes`.
pub struct PerceptionCache {
    pub screen_text: String,
    pub coords: HashMap<String, (i32, i32)>,
    pub bboxes: HashMap<String, (i32, i32, i32, i32)>,
}

impl PerceptionCache {
    pub fn new() -> Self {
        Self {
            screen_text: String::new(),
            coords: HashMap::new(),
            bboxes: HashMap::new(),
        }
    }
}

impl Default for PerceptionCache {
    fn default() -> Self { Self::new() }
}

/// Parse `ref_N (x,y,w,h)` lines from perceive.py output into center coords.
/// Signature and behavior are unchanged — production actuation depends on this.
// Compiled ONCE, not on every screen read (perception runs every step). The bbox regex is shared by
// coords + bboxes; labels use its own. Patterns are static and valid, so the LazyLock never panics.
static RE_BBOX: std::sync::LazyLock<Regex> =
    std::sync::LazyLock::new(|| Regex::new(r"(ref_\w+).*?\((\d+),(\d+),(\d+),(\d+)\)").unwrap());
static RE_LABEL: std::sync::LazyLock<Regex> =
    std::sync::LazyLock::new(|| Regex::new(r#"(ref_\w+).*?"([^"]*)""#).unwrap());

pub fn parse_ref_coords(screen: &str) -> HashMap<String, (i32, i32)> {
    let mut map = HashMap::new();
    let re = &*RE_BBOX;
    for line in screen.lines() {
        if let Some(caps) = re.captures(line) {
            let ref_id = caps[1].to_string();
            let x: i32 = caps[2].parse().unwrap_or(0);
            let y: i32 = caps[3].parse().unwrap_or(0);
            let w: i32 = caps[4].parse().unwrap_or(0);
            let h: i32 = caps[5].parse().unwrap_or(0);
            map.insert(ref_id, (x + w / 2, y + h / 2));
        }
    }
    map
}

/// Parse `ref_N (x,y,w,h)` lines from perceive.py output into full bounding boxes.
/// Returns (x, y, w, h) in VM screen pixels. Used by the fusion harness; actuators
/// use `parse_ref_coords` and are unaffected by this function.
pub fn parse_ref_bboxes(screen: &str) -> HashMap<String, (i32, i32, i32, i32)> {
    let mut map = HashMap::new();
    let re = &*RE_BBOX;
    for line in screen.lines() {
        if let Some(caps) = re.captures(line) {
            let ref_id = caps[1].to_string();
            let x: i32 = caps[2].parse().unwrap_or(0);
            let y: i32 = caps[3].parse().unwrap_or(0);
            let w: i32 = caps[4].parse().unwrap_or(0);
            let h: i32 = caps[5].parse().unwrap_or(0);
            map.insert(ref_id, (x, y, w, h));
        }
    }
    map
}

/// Parse `ref_N … "label" …` lines from perceive.py output into `ref_id → label`.
/// The label is the first double-quoted field on the row. Used to join human-readable
/// labels back onto `FusedElement`s for the candidate list (the fused set carries
/// `ref_id` + bbox but not the label text).
pub fn parse_ref_labels(screen: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let re = &*RE_LABEL;
    for line in screen.lines() {
        if let Some(caps) = re.captures(line) {
            map.insert(caps[1].to_string(), caps[2].to_string());
        }
    }
    map
}

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "linux")]
pub use linux::{LinuxPerceptor, LinuxActuator, linux_pair};

pub mod arbiter;
pub mod capture;
pub mod cv_proposer;
pub mod delta;
pub mod frame;
pub mod selection;
pub mod vlm_adapter;

/// Wraps any Perceptor with a VLM layer that appends visual context to AT-SPI2 output.
/// Reads the latest QMP frame from FRAME_PATH. Falls through to text-only when
/// the VLM server is unavailable or the frame file doesn't exist.
pub struct VlmPerceptor {
    pub inner: Arc<dyn Perceptor + Send + Sync>,
    pub vlm: vlm_adapter::VlmAdapter,
    pub frame_path: String,
}

impl Perceptor for VlmPerceptor {
    fn read_screen(&self) -> String {
        let text = self.inner.read_screen();

        if !self.vlm.is_available() {
            return text;
        }

        let visual = std::fs::read(&self.frame_path)
            .ok()
            .and_then(|bytes| self.vlm.describe_screen(&bytes));

        match visual {
            Some(v) if !v.is_empty() => format!("{text}\n\n[Visual]\n{v}"),
            _ => text,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_actuator_describes_actions() {
        let a = MockActuator;
        assert_eq!(a.click("ref_3"), "Clicked ref_3");
        assert_eq!(a.type_text("ref_5", "hi"), "Typed 2 chars into ref_5");
        assert_eq!(a.key("Return"), "Pressed Return");
    }

    #[test]
    fn mock_perceptor_returns_focused_dump() {
        let p = MockPerceptor;
        assert!(p.read_screen().contains("focused"));
    }

    // ── parse_ref_coords (regression — must not change) ──────────────

    #[test]
    fn parse_ref_coords_computes_center() {
        let screen = "  ref_1  button  \"OK\"  (10,20,80,30)  state=enabled";
        let coords = parse_ref_coords(screen);
        // center = (10 + 80/2, 20 + 30/2) = (50, 35)
        assert_eq!(coords.get("ref_1"), Some(&(50, 35)));
    }

    #[test]
    fn parse_ref_coords_multiple_elements() {
        let screen = "  ref_1  button  \"A\"  (0,0,100,50)\n  ref_2  entry  \"B\"  (0,60,200,25)";
        let coords = parse_ref_coords(screen);
        assert_eq!(coords.get("ref_1"), Some(&(50, 25)));
        assert_eq!(coords.get("ref_2"), Some(&(100, 72)));
    }

    #[test]
    fn parse_ref_coords_empty_when_no_bbox() {
        let screen = "  ref_1  button  \"OK\"  state=enabled";
        assert!(parse_ref_coords(screen).is_empty());
    }

    // ── parse_ref_bboxes ──────────────────────────────────────────────

    #[test]
    fn parse_ref_bboxes_retains_full_xywh() {
        let screen = "  ref_1  button  \"OK\"  (10,20,80,30)  state=enabled";
        let bboxes = parse_ref_bboxes(screen);
        assert_eq!(bboxes.get("ref_1"), Some(&(10, 20, 80, 30)));
    }

    #[test]
    fn parse_ref_bboxes_multiple_elements() {
        let screen = "  ref_1  button  \"Submit\"  (100,200,80,30)  [center]\n  ref_2  entry  \"Name\"  (10,60,200,25)";
        let bboxes = parse_ref_bboxes(screen);
        assert_eq!(bboxes.get("ref_1"), Some(&(100, 200, 80, 30)));
        assert_eq!(bboxes.get("ref_2"), Some(&(10, 60, 200, 25)));
    }

    #[test]
    fn parse_ref_bboxes_empty_when_no_bbox() {
        let screen = "  ref_1  button  \"OK\"  state=enabled";
        assert!(parse_ref_bboxes(screen).is_empty());
    }

    #[test]
    fn parse_ref_bboxes_does_not_affect_coords() {
        // Same screen — both parsers run on it independently, neither corrupts the other.
        let screen = "  ref_42  button  \"Go\"  (5,10,90,40)";
        let coords = parse_ref_coords(screen);
        let bboxes = parse_ref_bboxes(screen);
        assert_eq!(coords.get("ref_42"), Some(&(50, 30)));   // center: 5+45=50, 10+20=30
        assert_eq!(bboxes.get("ref_42"), Some(&(5, 10, 90, 40)));
    }

    // ── parse_ref_labels ──────────────────────────────────────────────

    #[test]
    fn parse_ref_labels_takes_first_quoted_field() {
        let screen = "      ref_1  toggle button   \"Applications\"  (0,0,102,26)";
        let labels = parse_ref_labels(screen);
        assert_eq!(labels.get("ref_1").map(|s| s.as_str()), Some("Applications"));
    }

    #[test]
    fn parse_ref_labels_handles_empty_label() {
        let screen = "      ref_2  toggle button   \"\"  (1101,0,26,26)";
        let labels = parse_ref_labels(screen);
        assert_eq!(labels.get("ref_2").map(|s| s.as_str()), Some(""));
    }

    #[test]
    fn parse_ref_labels_multiple_rows() {
        let screen = "  ref_1  toggle button  \"Applications\"  (0,0,102,26)\n  ref_4  toggle button  \"2026-06-17\"  (1161,0,68,26)";
        let labels = parse_ref_labels(screen);
        assert_eq!(labels.get("ref_1").map(|s| s.as_str()), Some("Applications"));
        assert_eq!(labels.get("ref_4").map(|s| s.as_str()), Some("2026-06-17"));
    }

    #[test]
    fn perception_cache_has_bboxes_field() {
        let mut cache = PerceptionCache::new();
        cache.bboxes.insert("ref_1".to_string(), (10, 20, 80, 30));
        assert_eq!(cache.bboxes.get("ref_1"), Some(&(10, 20, 80, 30)));
        assert!(cache.coords.is_empty(), "coords must not be polluted");
    }
}
