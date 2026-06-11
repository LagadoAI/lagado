//! frame.rs — FrameProcessor: decodes PNG frames and detects pixel-space changes.
//!
//! `FrameProcessor` is stateful — it holds the previous frame's cell hashes inside
//! its `DeltaDetector` to diff against each new frame. One `FrameProcessor` per
//! perception session; call `reset()` between sessions to prevent hash leakage
//! across measurement runs.

use crate::perception::delta::{DeltaDetector, GRID_COLS, GRID_ROWS};

/// A cell that changed between the previous frame and the current one.
#[derive(Debug, Clone)]
pub struct ChangedCell {
    /// Grid identifier, format `"c{row}_{col}"`.
    pub cell_id: String,
    /// Top-left corner and dimensions in VM screen pixels: `(x, y, w, h)`.
    pub pixel_bounds: (u32, u32, u32, u32),
}

/// Stateful per-session frame processor.
///
/// Decodes raw PNG bytes to RGB pixels and delegates to `DeltaDetector` for
/// cell-level change detection. The detector's hash table is owned here; call
/// `reset()` before starting a new measurement session to flush stale state.
pub struct FrameProcessor {
    detector: DeltaDetector,
}

impl FrameProcessor {
    pub fn new() -> Self {
        Self { detector: DeltaDetector::new() }
    }

    /// Flush all stored cell hashes. All cells will report as changed on the next
    /// `process_frame` call. Call between separate perception sessions so that
    /// hash state from one run cannot contaminate the next.
    pub fn reset(&mut self) {
        self.detector.reset();
    }

    /// Decode `png_bytes` and return the cells that changed vs the previous frame.
    ///
    /// On the first call (no previous frame), all cells are returned as changed.
    /// On subsequent calls, only cells whose pixel content differs are returned;
    /// static cells are guaranteed absent from the output.
    pub fn process_frame(&mut self, png_bytes: &[u8]) -> Result<Vec<ChangedCell>, String> {
        let img = image::load_from_memory(png_bytes)
            .map_err(|e| format!("PNG decode failed: {e}"))?
            .to_rgb8();

        let width = img.width();
        let height = img.height();
        let rgb_pixels = img.as_raw();

        let changed_ids = self.detector.detect_changes(rgb_pixels, width, height);

        let cells = changed_ids
            .into_iter()
            .map(|id| {
                let (row, col) = parse_cell_id(&id).unwrap_or((0, 0));
                let pixel_bounds = DeltaDetector::cell_pixel_bounds(col, row, width, height);
                ChangedCell { cell_id: id, pixel_bounds }
            })
            .collect();

        Ok(cells)
    }
}

impl Default for FrameProcessor {
    fn default() -> Self { Self::new() }
}

/// Parse `"c{row}_{col}"` back to `(row, col)`.
fn parse_cell_id(id: &str) -> Option<(usize, usize)> {
    let stripped = id.strip_prefix('c')?;
    let mut parts = stripped.splitn(2, '_');
    let row: usize = parts.next()?.parse().ok()?;
    let col: usize = parts.next()?.parse().ok()?;
    Some((row, col))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageBuffer, Rgb};

    /// Encode a solid-color frame as PNG bytes for use in tests.
    fn encode_png(width: u32, height: u32, fill: [u8; 3]) -> Vec<u8> {
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
            ImageBuffer::from_fn(width, height, |_, _| Rgb(fill));
        let mut buf = Vec::new();
        DynamicImage::ImageRgb8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut buf),
                image::ImageFormat::Png,
            )
            .unwrap();
        buf
    }

    #[test]
    fn first_frame_reports_all_cells_changed() {
        let mut fp = FrameProcessor::new();
        let png = encode_png(160, 120, [128, 128, 128]);
        let changed = fp.process_frame(&png).unwrap();
        assert_eq!(
            changed.len(),
            GRID_COLS * GRID_ROWS,
            "all 48 cells must change on the first frame"
        );
    }

    #[test]
    fn identical_consecutive_frames_zero_changes() {
        let mut fp = FrameProcessor::new();
        let png = encode_png(160, 120, [0, 0, 0]);
        fp.process_frame(&png).unwrap(); // establish baseline
        let changed = fp.process_frame(&png).unwrap();
        assert!(
            changed.is_empty(),
            "identical consecutive frames must produce zero changes"
        );
    }

    #[test]
    fn reset_forces_full_retrigger() {
        let mut fp = FrameProcessor::new();
        let png = encode_png(160, 120, [50, 50, 50]);
        fp.process_frame(&png).unwrap(); // baseline

        fp.reset();

        // Same PNG after reset → all cells must re-trigger as if first frame.
        let changed = fp.process_frame(&png).unwrap();
        assert_eq!(
            changed.len(),
            GRID_COLS * GRID_ROWS,
            "reset must cause all cells to re-trigger"
        );
    }

    #[test]
    fn changed_cells_carry_correct_pixel_bounds() {
        // 160×120 with 8×6 grid → base_w=20, base_h=20; c0_0 = (0,0,20,20)
        let mut fp = FrameProcessor::new();
        let png = encode_png(160, 120, [0, 0, 0]);
        let changed = fp.process_frame(&png).unwrap();

        let c0_0 = changed
            .iter()
            .find(|c| c.cell_id == "c0_0")
            .expect("c0_0 must be in first-frame output");

        assert_eq!(c0_0.pixel_bounds, (0, 0, 20, 20));
    }

    #[test]
    fn parse_cell_id_round_trips() {
        assert_eq!(parse_cell_id("c0_0"), Some((0, 0)));
        assert_eq!(parse_cell_id("c3_5"), Some((3, 5)));
        assert_eq!(parse_cell_id("c5_7"), Some((5, 7)));
        assert_eq!(parse_cell_id("bad"),  None);
        assert_eq!(parse_cell_id("c3"),   None);
    }
}
