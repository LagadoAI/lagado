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
pub struct PerceptionCache {
    pub screen_text: String,
    pub coords: HashMap<String, (i32, i32)>,
}

impl PerceptionCache {
    pub fn new() -> Self {
        Self { screen_text: String::new(), coords: HashMap::new() }
    }
}

impl Default for PerceptionCache {
    fn default() -> Self { Self::new() }
}

/// Parse `ref_N (x,y,w,h)` lines from perceive.py output into center coords.
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

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "linux")]
pub use linux::{LinuxPerceptor, LinuxActuator, linux_pair};

pub mod capture;
pub mod delta;
pub mod vlm_adapter;

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
}
