//! linux.rs — Real Linux perception via AT-SPI2 screen reader and xdotool actuator.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use regex::Regex;

/// Shared state: last screen dump + ref → screen center coordinates.
pub struct PerceptionCache {
    pub screen_text: String,
    pub coords: HashMap<String, (i32, i32)>, // ref_id → (cx, cy)
}

impl PerceptionCache {
    pub fn new() -> Self {
        Self {
            screen_text: String::new(),
            coords: HashMap::new(),
        }
    }
}

impl Default for PerceptionCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Linux-based perceptor: reads screen via perceive.py AT-SPI2 script.
pub struct LinuxPerceptor {
    cache: Arc<Mutex<PerceptionCache>>,
    perceive_path: std::path::PathBuf,
}

impl LinuxPerceptor {
    pub fn new(cache: Arc<Mutex<PerceptionCache>>) -> Self {
        // Locate perceive.py: LAGADO_PERCEIVE_SCRIPT env (debug only) → repo root relative path
        #[cfg(debug_assertions)]
        let path = std::env::var("LAGADO_PERCEIVE_SCRIPT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| locate_perceive_script());
        #[cfg(not(debug_assertions))]
        let path = locate_perceive_script();

        Self {
            cache,
            perceive_path: path,
        }
    }
}

fn locate_perceive_script() -> std::path::PathBuf {
    // Try: current directory, then data_dir
    let candidates = [
        std::path::PathBuf::from("perceive.py"),
        crate::config::data_dir().join("perceive.py"),
    ];
    for c in &candidates {
        if c.exists() {
            return c.clone();
        }
    }
    std::path::PathBuf::from("perceive.py") // fallback
}

impl crate::perception::Perceptor for LinuxPerceptor {
    fn read_screen(&self) -> String {
        let output = std::process::Command::new("python3")
            .arg(&self.perceive_path)
            .arg("--focused")
            .output();

        let text = match output {
            Ok(o) if o.status.success() => {
                String::from_utf8_lossy(&o.stdout).to_string()
            }
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr);
                tracing::warn!("perceive.py failed: {err}");
                "[focused: (perception unavailable)]\n".to_string()
            }
            Err(e) => {
                tracing::warn!("perceive.py spawn failed: {e}");
                "[focused: (perception unavailable)]\n".to_string()
            }
        };

        // Parse ref → bbox from the text and populate cache
        let coords = parse_ref_coords(&text);
        if let Ok(mut cache) = self.cache.lock() {
            cache.screen_text = text.clone();
            cache.coords = coords;
        }
        text
    }
}

/// Parse lines like `  ref_2  entry  "Search"  (200,300,200,30)  ...`
/// to extract center coordinates for each ref_id.
fn parse_ref_coords(screen: &str) -> HashMap<String, (i32, i32)> {
    let mut map = HashMap::new();
    // Match: ref_N anywhere on a line, followed by a (x,y,w,h) bbox
    let re = match Regex::new(r"(ref_\w+).*?\((\d+),(\d+),(\d+),(\d+)\)") {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("regex compilation failed: {e}");
            return map;
        }
    };

    for line in screen.lines() {
        if let Some(caps) = re.captures(line) {
            let ref_id = caps[1].to_string();
            let x: i32 = caps[2].parse().unwrap_or(0);
            let y: i32 = caps[3].parse().unwrap_or(0);
            let w: i32 = caps[4].parse().unwrap_or(0);
            let h: i32 = caps[5].parse().unwrap_or(0);
            let cx = x + w / 2;
            let cy = y + h / 2;
            map.insert(ref_id, (cx, cy));
        }
    }
    map
}

/// Linux-based actuator: uses xdotool to interact with the desktop.
pub struct LinuxActuator {
    cache: Arc<Mutex<PerceptionCache>>,
}

impl LinuxActuator {
    pub fn new(cache: Arc<Mutex<PerceptionCache>>) -> Self {
        Self { cache }
    }
}

impl crate::perception::Actuator for LinuxActuator {
    fn click(&self, selector: &str) -> String {
        let coords = self
            .cache
            .lock()
            .ok()
            .and_then(|c| c.coords.get(selector).copied());

        match coords {
            Some((cx, cy)) => {
                let status = std::process::Command::new("xdotool")
                    .args([
                        "mousemove",
                        "--sync",
                        &cx.to_string(),
                        &cy.to_string(),
                        "click",
                        "1",
                    ])
                    .status();

                match status {
                    Ok(s) if s.success() => format!("Clicked {selector} at ({cx},{cy})"),
                    Ok(_) => {
                        tracing::warn!("xdotool click failed — may be on native Wayland (try ydotool)");
                        format!("click failed for {selector}: xdotool error")
                    }
                    Err(e) => format!("click failed for {selector}: {e}"),
                }
            }
            None => {
                tracing::warn!("click: ref {selector} not found in last screen dump");
                format!("click failed: {selector} not in screen")
            }
        }
    }

    fn type_text(&self, selector: &str, text: &str) -> String {
        // Focus the element by clicking it first
        self.click(selector);

        let status = std::process::Command::new("xdotool")
            .args(["type", "--clearmodifiers", "--", text])
            .status();

        match status {
            Ok(s) if s.success() => {
                format!("Typed {} chars into {selector}", text.chars().count())
            }
            Ok(_) => format!("type failed for {selector}: xdotool error"),
            Err(e) => format!("type failed for {selector}: {e}"),
        }
    }

    fn key(&self, key: &str) -> String {
        let status = std::process::Command::new("xdotool")
            .args(["key", "--clearmodifiers", key])
            .status();

        match status {
            Ok(s) if s.success() => format!("Pressed {key}"),
            Ok(_) => format!("key failed for {key}: xdotool error"),
            Err(e) => format!("key failed for {key}: {e}"),
        }
    }
}

/// Create a matched (LinuxPerceptor, LinuxActuator) pair sharing the same cache.
pub fn linux_pair() -> (LinuxPerceptor, LinuxActuator) {
    let cache = Arc::new(Mutex::new(PerceptionCache::new()));
    (
        LinuxPerceptor::new(cache.clone()),
        LinuxActuator::new(cache),
    )
}
