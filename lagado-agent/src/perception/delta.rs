//! delta.rs — Blake3 per-cell change detection.
//!
//! Divides the screen into a grid of cells. Hashes each cell.
//! On next frame, only cells whose hash changed need re-processing.
//! Target: 1000 iterations < 100ms (matches master plan spec).

use std::collections::HashMap;

/// Grid dimensions for change detection (columns × rows).
const GRID_COLS: usize = 8;
const GRID_ROWS: usize = 6;

pub struct DeltaDetector {
    /// Hash per cell from last frame: cell_id → blake3 hex
    cell_hashes: HashMap<String, String>,
}

impl DeltaDetector {
    pub fn new() -> Self {
        Self { cell_hashes: HashMap::new() }
    }

    /// Compare a new frame against the last. Returns list of changed cell IDs.
    /// Phase 1: works on raw bytes. Phase 2: accepts decoded image pixels.
    pub fn detect_changes(&mut self, frame_bytes: &[u8]) -> Vec<String> {
        if frame_bytes.is_empty() {
            return vec![];
        }

        // Divide frame into grid cells by byte offset approximation
        let cell_size = frame_bytes.len() / (GRID_COLS * GRID_ROWS).max(1);
        let mut changed = Vec::new();

        for row in 0..GRID_ROWS {
            for col in 0..GRID_COLS {
                let cell_id = format!("c{row}_{col}");
                let start = (row * GRID_COLS + col) * cell_size;
                let end = (start + cell_size).min(frame_bytes.len());
                let cell_data = &frame_bytes[start..end];

                // Blake3 hash of cell
                let hash = blake3::hash(cell_data).to_hex().to_string();
                let prev = self.cell_hashes.get(&cell_id).cloned();

                if prev.as_deref() != Some(&hash) {
                    changed.push(cell_id.clone());
                    self.cell_hashes.insert(cell_id, hash);
                }
            }
        }
        changed
    }

    /// True if any changes were detected since last frame.
    pub fn has_changes(&self) -> bool {
        !self.cell_hashes.is_empty()
    }

    /// Reset state (forces full re-vision on next frame).
    pub fn reset(&mut self) {
        self.cell_hashes.clear();
    }

    pub fn changed_cell_count(&self) -> usize {
        self.cell_hashes.len()
    }
}
