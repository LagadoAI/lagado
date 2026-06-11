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
}

/// Performs user-input actions on the desktop.
pub trait Actuator: Send + Sync {
    fn click(&self, selector: &str) -> String;
    fn type_text(&self, selector: &str, text: &str) -> String;
    fn key(&self, key: &str) -> String;
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
pub fn parse_ref_coords(screen: &str) -> HashMap<String, (i32, i32)> {
    let mut map = HashMap::new();
    let re = match Regex::new(r"(ref_\w+).*?\((\d+),(\d+),(\d+),(\d+)\)") {
        Ok(r) => r,
        Err(_) => return map,
    };
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
    let re = match Regex::new(r"(ref_\w+).*?\((\d+),(\d+),(\d+),(\d+)\)") {
        Ok(r) => r,
        Err(_) => return map,
    };
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

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "linux")]
pub use linux::{LinuxPerceptor, LinuxActuator, linux_pair};

pub mod capture;
pub mod delta;
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

    #[test]
    fn perception_cache_has_bboxes_field() {
        let mut cache = PerceptionCache::new();
        cache.bboxes.insert("ref_1".to_string(), (10, 20, 80, 30));
        assert_eq!(cache.bboxes.get("ref_1"), Some(&(10, 20, 80, 30)));
        assert!(cache.coords.is_empty(), "coords must not be polluted");
    }
}
