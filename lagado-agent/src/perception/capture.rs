//! capture.rs — Screen capture to /dev/shm.
//!
//! Phase 1: subprocess-based (grim for Wayland, scrot for X11/XWayland).
//! Phase 2: PipeWire/xdg-desktop-portal native capture at 20Hz.
//! Output: PNG written to /dev/shm/lagado_frame.png for zero-copy access.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const SHM_FRAME_PATH: &str = "/dev/shm/lagado_frame.png";
const CAPTURE_INTERVAL_MS: u64 = 50; // 20Hz

pub struct ScreenCapture {
    frame_path: PathBuf,
    last_capture: Option<Instant>,
}

impl ScreenCapture {
    pub fn new() -> Self {
        Self {
            frame_path: PathBuf::from(SHM_FRAME_PATH),
            last_capture: None,
        }
    }

    /// Capture the current screen to /dev/shm/lagado_frame.png.
    /// Rate-limited to 20Hz. Returns path to frame on success.
    pub fn capture(&mut self) -> Result<&Path, String> {
        // Rate limiting
        if let Some(last) = self.last_capture {
            if last.elapsed() < Duration::from_millis(CAPTURE_INTERVAL_MS) {
                return Ok(&self.frame_path);
            }
        }

        // Try grim (Wayland) first, then scrot (X11)
        let result = self.try_grim().or_else(|_| self.try_scrot());
        match result {
            Ok(()) => {
                self.last_capture = Some(Instant::now());
                Ok(&self.frame_path)
            }
            Err(e) => Err(e),
        }
    }

    fn try_grim(&self) -> Result<(), String> {
        let status = std::process::Command::new("grim")
            .arg(self.frame_path.to_str().unwrap_or(SHM_FRAME_PATH))
            .status()
            .map_err(|e| format!("grim not available: {e}"))?;
        if status.success() { Ok(()) } else { Err("grim failed".to_string()) }
    }

    fn try_scrot(&self) -> Result<(), String> {
        let status = std::process::Command::new("scrot")
            .arg("--silent")
            .arg(self.frame_path.to_str().unwrap_or(SHM_FRAME_PATH))
            .status()
            .map_err(|e| format!("scrot not available: {e}"))?;
        if status.success() { Ok(()) } else { Err("scrot failed".to_string()) }
    }

    /// Read the last captured frame as raw bytes.
    pub fn read_frame(&self) -> Option<Vec<u8>> {
        std::fs::read(&self.frame_path).ok()
    }

    /// True if a frame exists in /dev/shm.
    pub fn has_frame(&self) -> bool {
        self.frame_path.exists()
    }
}
