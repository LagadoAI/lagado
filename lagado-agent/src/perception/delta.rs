//! delta.rs — Pixel-space frame change detection.
//!
//! Divides decoded RGB frames into a fixed pixel-space grid. Hashes each cell's
//! actual pixel bytes with blake3. On subsequent frames, only cells whose pixel
//! content changed are returned — unchanged cells are skipped entirely.
//!
//! **Why decoded pixels, not compressed bytes:** PNG deflate interleaves pixel data
//! across the compressed byte stream; hashing byte-offset chunks of a PNG produces
//! non-deterministic cell boundaries (the same pixel change can flag different cells
//! across runs). Decoded RGB pixels have a fixed, stable layout — the same pixels
//! always produce the same hash, unconditionally.
//!
//! **Remainder policy:** when frame dimensions do not divide evenly into the grid,
//! remainder pixels are assigned to the last column and last row. Every pixel in
//! `[0, width) × [0, height)` belongs to exactly one cell — no blind seam, no
//! dropped pixels.

use std::collections::HashMap;

/// Grid dimensions for change detection (columns × rows).
pub const GRID_COLS: usize = 8;
pub const GRID_ROWS: usize = 6;

pub struct DeltaDetector {
    /// Hash per cell from last frame: cell_id → blake3 hex.
    cell_hashes: HashMap<String, String>,
}

impl DeltaDetector {
    pub fn new() -> Self {
        Self { cell_hashes: HashMap::new() }
    }

    /// Detect changed cells between this decoded RGB frame and the previous one.
    ///
    /// `rgb_pixels` — flat RGB byte array (R,G,B per pixel, row-major).
    /// Expected length: `width * height * 3`.
    ///
    /// Returns cell IDs (`"c{row}_{col}"`) for cells whose pixel content changed.
    /// On the first call (empty hash table), all cells are returned as changed.
    /// Unchanged cells are guaranteed to never appear in the output.
    pub fn detect_changes(&mut self, rgb_pixels: &[u8], width: u32, height: u32) -> Vec<String> {
        if rgb_pixels.is_empty() || width == 0 || height == 0 {
            return vec![];
        }

        let mut changed = Vec::new();

        for row in 0..GRID_ROWS {
            for col in 0..GRID_COLS {
                let cell_id = format!("c{row}_{col}");
                let (cx, cy, cw, ch) = Self::cell_pixel_bounds(col, row, width, height);
                let hash = hash_cell(rgb_pixels, width, cx, cy, cw, ch);

                if self.cell_hashes.get(&cell_id).map(String::as_str) != Some(&hash) {
                    changed.push(cell_id.clone());
                    self.cell_hashes.insert(cell_id, hash);
                }
            }
        }

        changed
    }

    /// Pixel bounds for cell `(col, row)` in a frame of the given dimensions.
    ///
    /// Returns `(x, y, w, h)` — top-left corner and dimensions in screen pixels.
    ///
    /// Remainder pixels (when `width % GRID_COLS != 0` or `height % GRID_ROWS != 0`)
    /// are absorbed by the last column or last row. No pixel is left uncovered.
    pub fn cell_pixel_bounds(col: usize, row: usize, width: u32, height: u32) -> (u32, u32, u32, u32) {
        let base_w = width / GRID_COLS as u32;
        let base_h = height / GRID_ROWS as u32;

        let x = col as u32 * base_w;
        let y = row as u32 * base_h;

        // Last column and last row absorb the remainder pixels.
        let w = if col == GRID_COLS - 1 { width - x } else { base_w };
        let h = if row == GRID_ROWS - 1 { height - y } else { base_h };

        (x, y, w, h)
    }

    /// Clear all stored cell hashes. Forces all cells to report as changed on the
    /// next `detect_changes` call. Call between separate perception sessions to
    /// prevent cross-session hash leakage.
    pub fn reset(&mut self) {
        self.cell_hashes.clear();
    }
}

/// Hash the pixel bytes of one cell using blake3.
///
/// Iterates pixel rows within the cell bounds in row-major order and feeds each
/// row's bytes into the hasher. The hash is a pure function of pixel content:
/// same pixels → same hash, always.
fn hash_cell(
    rgb_pixels: &[u8],
    frame_width: u32,
    cell_x: u32,
    cell_y: u32,
    cell_w: u32,
    cell_h: u32,
) -> String {
    let mut hasher = blake3::Hasher::new();
    for row_idx in cell_y..(cell_y + cell_h) {
        let row_start = (row_idx * frame_width + cell_x) as usize * 3;
        let row_end = row_start + cell_w as usize * 3;
        hasher.update(&rgb_pixels[row_start..row_end]);
    }
    hasher.finalize().to_hex().to_string()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Construct a flat RGB frame of `width × height` pixels, all set to `rgb`.
    fn solid_frame(width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
        let mut px = Vec::with_capacity((width * height * 3) as usize);
        for _ in 0..(width * height) {
            px.extend_from_slice(&rgb);
        }
        px
    }

    // ── Determinism ───────────────────────────────────────────────────────────

    #[test]
    fn same_frame_hashed_twice_zero_changes() {
        // After the first call establishes the baseline, feeding the identical
        // bytes again must produce zero changed cells — every cell hash must
        // match exactly.
        let mut d = DeltaDetector::new();
        let frame = solid_frame(160, 120, [100, 150, 200]);

        let first = d.detect_changes(&frame, 160, 120);
        assert_eq!(first.len(), GRID_COLS * GRID_ROWS, "first call must flag all cells");

        let second = d.detect_changes(&frame, 160, 120);
        assert!(
            second.is_empty(),
            "identical frame must produce zero changes (determinism); got: {:?}",
            second
        );
    }

    // ── Static regions never register ─────────────────────────────────────────

    #[test]
    fn unchanged_region_registers_zero_changes() {
        // Two consecutive identical frames — no cell may ever appear as changed.
        let mut d = DeltaDetector::new();
        let frame = solid_frame(160, 120, [0, 0, 0]);

        d.detect_changes(&frame, 160, 120); // establish baseline

        let changes = d.detect_changes(&frame, 160, 120);
        assert!(
            changes.is_empty(),
            "static frame must produce zero changes; got: {:?}",
            changes
        );
    }

    #[test]
    fn static_regions_never_flagged_when_one_cell_changes() {
        // 160×120 with 8×6 grid → base_w=20, base_h=20.
        // Cell IDs are "c{row}_{col}".
        // Pixel at (px=45, py=25): col=2 (x=[40,60)), row=1 (y=[20,40)) → cell "c1_2".
        // Modifying one pixel inside c1_2 must flag ONLY c1_2.
        // Every other cell is static and must never appear in the output.
        let mut d = DeltaDetector::new();
        let width = 160u32;
        let height = 120u32;

        let frame1 = solid_frame(width, height, [0, 0, 0]);
        d.detect_changes(&frame1, width, height); // establish baseline

        let mut frame2 = frame1.clone();
        let px = 45u32; // col 2 ([40,60))
        let py = 25u32; // row 1 ([20,40))
        frame2[(py * width + px) as usize * 3] = 255;

        let changes = d.detect_changes(&frame2, width, height);

        assert!(
            changes.contains(&"c1_2".to_string()),
            "modified cell c1_2 must be flagged"
        );

        let static_flagged: Vec<&String> =
            changes.iter().filter(|id| *id != "c1_2").collect();
        assert!(
            static_flagged.is_empty(),
            "static cells must never register as changed: {:?}",
            static_flagged
        );
    }

    // ── Non-divisible dimensions ──────────────────────────────────────────────

    #[test]
    fn non_divisible_width_area_sum_equals_frame_area() {
        // 1366×768: 1366/8=170 remainder 6 → last col gets 176px
        //           768/6=128 remainder 0 → exact
        let width = 1366u32;
        let height = 768u32;

        let total: u64 = (0..GRID_ROWS)
            .flat_map(|row| (0..GRID_COLS).map(move |col| (row, col)))
            .map(|(row, col)| {
                let (_, _, w, h) = DeltaDetector::cell_pixel_bounds(col, row, width, height);
                w as u64 * h as u64
            })
            .sum();

        assert_eq!(
            total,
            width as u64 * height as u64,
            "sum of all cell areas must equal total frame area"
        );
    }

    #[test]
    fn non_divisible_both_axes_every_pixel_in_exactly_one_cell() {
        // 101×97: both axes leave a remainder.
        //   width  101/8=12 r5: cells 0–6 → w=12, cell 7 → w=17 (12*7+17=101 ✓)
        //   height  97/6=16 r1: cells 0–4 → h=16, cell 5 → h=17 (16*5+17=97 ✓)
        let width = 101u32;
        let height = 97u32;

        let mut coverage = vec![0u8; (width * height) as usize];

        for row in 0..GRID_ROWS {
            for col in 0..GRID_COLS {
                let (x, y, w, h) = DeltaDetector::cell_pixel_bounds(col, row, width, height);
                for py in y..(y + h) {
                    for px in x..(x + w) {
                        coverage[(py * width + px) as usize] += 1;
                    }
                }
            }
        }

        let dropped = coverage.iter().filter(|&&c| c == 0).count();
        let double_counted = coverage.iter().filter(|&&c| c > 1).count();
        assert_eq!(dropped, 0, "{dropped} pixels not covered by any cell (blind seam)");
        assert_eq!(double_counted, 0, "{double_counted} pixels belong to more than one cell");
    }

    // ── Cell-bounds arithmetic ────────────────────────────────────────────────

    #[test]
    fn last_col_absorbs_remainder() {
        // 1366 / 8 = 170 r6 → last col: x=7*170=1190, w=1366-1190=176
        let (x, _, w, _) = DeltaDetector::cell_pixel_bounds(GRID_COLS - 1, 0, 1366, 128);
        assert_eq!(x, 7 * 170, "last col x-origin");
        assert_eq!(w, 1366 - 7 * 170, "last col width absorbs remainder");
    }

    #[test]
    fn last_row_absorbs_remainder() {
        // 97 / 6 = 16 r1 → last row: y=5*16=80, h=97-80=17
        let (_, y, _, h) = DeltaDetector::cell_pixel_bounds(0, GRID_ROWS - 1, 128, 97);
        assert_eq!(y, 5 * 16, "last row y-origin");
        assert_eq!(h, 97 - 5 * 16, "last row height absorbs remainder");
    }

    #[test]
    fn divisible_dimensions_uniform_cells() {
        // 160×120 divides exactly: all cells should be 20×20
        for row in 0..GRID_ROWS {
            for col in 0..GRID_COLS {
                let (_, _, w, h) = DeltaDetector::cell_pixel_bounds(col, row, 160, 120);
                assert_eq!(w, 20, "c{row}_{col} width");
                assert_eq!(h, 20, "c{row}_{col} height");
            }
        }
    }

    // ── Reset ─────────────────────────────────────────────────────────────────

    #[test]
    fn reset_forces_all_cells_changed_next_call() {
        let mut d = DeltaDetector::new();
        let frame = solid_frame(160, 120, [42, 42, 42]);

        d.detect_changes(&frame, 160, 120); // baseline
        let no_changes = d.detect_changes(&frame, 160, 120);
        assert!(no_changes.is_empty());

        d.reset();
        let after_reset = d.detect_changes(&frame, 160, 120);
        assert_eq!(
            after_reset.len(),
            GRID_COLS * GRID_ROWS,
            "reset must cause all cells to re-trigger"
        );
    }
}
