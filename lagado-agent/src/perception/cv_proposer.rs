//! cv_proposer.rs — Classical-CV bounding-box proposer for GUI elements.
//!
//! Proposes candidate bounding boxes from a decoded RGB cell using Canny edge
//! detection + 8-connected component labelling. No model, no GPU, no Python —
//! runs entirely on CPU in the same process.
//!
//! The proposer is intentionally NOISY. Its job is to surface candidate elements
//! that the AT-SPI2 accessibility tree cannot see (canvas content, custom-drawn
//! controls, unlabeled widgets). False positives are expected and are filtered
//! downstream by the IoU-dedup arbiter (TASK 6). The acceptance criterion here
//! is not "few boxes" — it is "the box count after area/aspect-ratio filtering
//! is small enough that downstream dedup is not overwhelmed."
//!
//! Threshold values are hardcoded named constants with comments explaining their
//! rationale. Promote to config.rs only after measurement on real screenshots
//! reveals which knobs matter.

use image::{ImageBuffer, Luma};
use imageproc::region_labelling::Connectivity;
use std::collections::HashMap;

// ── Hardcoded thresholds ────────────────────────────────────────────────────
//
// Do NOT tune these to improve reported numbers. Measure first, tune after.

/// Canny low threshold (hysteresis low). Gradient below this is suppressed.
/// Low value intentionally catches faint hairline borders on GUI widgets.
const CANNY_LOW: f32 = 15.0;

/// Canny high threshold (hysteresis high). Gradient above this is always an edge.
/// Keeps strong contrast boundaries (button outlines, window chrome).
const CANNY_HIGH: f32 = 45.0;

/// Minimum bounding-box area in pixels to keep a proposed box.
/// Filters sub-pixel rounding artifacts and single-glyph text fragments
/// that are not actionable widget regions.
const MIN_BOX_AREA_PX: u32 = 64; // 8×8 pixels

/// Maximum bounding-box area as a fraction of the cell's total pixel area.
/// Boxes spanning nearly the entire cell are background noise, not widgets.
const MAX_BOX_AREA_FRAC: f32 = 0.85;

/// Minimum aspect ratio (width / height).
/// Allows very tall narrow elements such as scrollbar tracks and dividers.
const MIN_ASPECT: f32 = 0.05;

/// Maximum aspect ratio (width / height).
/// Allows very wide short elements such as menu bars and toolbar separators.
const MAX_ASPECT: f32 = 20.0;

// ── Public types ────────────────────────────────────────────────────────────

/// A candidate bounding box in screen pixel coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct ScreenBox {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Result of one `propose_boxes` call, split for measurement reporting.
#[derive(Debug)]
pub struct ProposalResult {
    /// Boxes that survived the area/aspect-ratio filter.
    pub boxes: Vec<ScreenBox>,
    /// Total connected-component boxes before any filter was applied.
    pub raw_count: usize,
}

// ── Public API ──────────────────────────────────────────────────────────────

/// Propose candidate bounding boxes from a single decoded RGB cell.
///
/// `cell_rgb` — flat RGB bytes of the cell (R,G,B per pixel, row-major).
///              Length must be exactly `cell_w * cell_h * 3`.
/// `cell_w`, `cell_h` — dimensions of this cell in pixels.
/// `cell_origin` — `(x, y)` of the cell's top-left corner in screen coordinates.
///
/// Returns `ProposalResult` containing both the filtered boxes (in screen coords)
/// and the raw pre-filter count, so callers can measure filter effectiveness.
pub fn propose_boxes(
    cell_rgb: &[u8],
    cell_w: u32,
    cell_h: u32,
    cell_origin: (u32, u32),
) -> ProposalResult {
    if cell_rgb.is_empty() || cell_w == 0 || cell_h == 0 {
        return ProposalResult { boxes: vec![], raw_count: 0 };
    }

    // Convert cell RGB bytes → grayscale ImageBuffer
    let gray = rgb_to_gray(cell_rgb, cell_w, cell_h);

    // Canny edge detection → binary edge map (non-zero = edge pixel)
    let edges = imageproc::edges::canny(&gray, CANNY_LOW, CANNY_HIGH);

    // 8-connected component labelling on edge pixels
    let labels = imageproc::region_labelling::connected_components(
        &edges,
        Connectivity::Eight,
        Luma([0u8]),
    );

    // Bounding box per component label (label 0 = background, skipped)
    let raw_boxes = bboxes_from_labels(&labels, cell_w, cell_h, cell_origin);
    let raw_count = raw_boxes.len();

    // Filter: area and aspect ratio
    let cell_area = (cell_w * cell_h) as f32;
    let boxes = raw_boxes
        .into_iter()
        .filter(|b| {
            let area = (b.w * b.h) as u32;
            if area < MIN_BOX_AREA_PX {
                return false;
            }
            if area as f32 > cell_area * MAX_BOX_AREA_FRAC {
                return false;
            }
            if b.h == 0 {
                return false;
            }
            let aspect = b.w as f32 / b.h as f32;
            aspect >= MIN_ASPECT && aspect <= MAX_ASPECT
        })
        .collect();

    ProposalResult { boxes, raw_count }
}

/// Extract the RGB bytes for a single cell from a full decoded frame.
///
/// `full_rgb` — flat RGB bytes of the full frame (R,G,B per pixel, row-major).
/// `frame_width` — width of the full frame in pixels.
///
/// Returns a contiguous `cell_w * cell_h * 3` byte vector suitable for passing
/// directly to `propose_boxes`.
pub fn extract_cell_rgb(
    full_rgb: &[u8],
    frame_width: u32,
    cell_x: u32,
    cell_y: u32,
    cell_w: u32,
    cell_h: u32,
) -> Vec<u8> {
    let mut cell = Vec::with_capacity((cell_w * cell_h * 3) as usize);
    for row in cell_y..(cell_y + cell_h) {
        let row_start = (row * frame_width + cell_x) as usize * 3;
        let row_end = row_start + cell_w as usize * 3;
        cell.extend_from_slice(&full_rgb[row_start..row_end]);
    }
    cell
}

// ── Internal helpers ────────────────────────────────────────────────────────

fn rgb_to_gray(rgb: &[u8], w: u32, h: u32) -> ImageBuffer<Luma<u8>, Vec<u8>> {
    let mut gray = ImageBuffer::new(w, h);
    for (i, chunk) in rgb.chunks_exact(3).enumerate() {
        let (r, g, b) = (chunk[0] as f32, chunk[1] as f32, chunk[2] as f32);
        // BT.601 luminance
        let luma = (0.299 * r + 0.587 * g + 0.114 * b).min(255.0) as u8;
        let px = (i as u32) % w;
        let py = (i as u32) / w;
        gray.put_pixel(px, py, Luma([luma]));
    }
    gray
}

fn bboxes_from_labels(
    labels: &ImageBuffer<Luma<u32>, Vec<u32>>,
    cell_w: u32,
    cell_h: u32,
    cell_origin: (u32, u32),
) -> Vec<ScreenBox> {
    // Track (min_x, min_y, max_x, max_y) per label.
    let mut bounds: HashMap<u32, (u32, u32, u32, u32)> = HashMap::new();

    for py in 0..cell_h {
        for px in 0..cell_w {
            let label = labels.get_pixel(px, py)[0];
            if label == 0 {
                continue; // background
            }
            bounds
                .entry(label)
                .and_modify(|(min_x, min_y, max_x, max_y)| {
                    *min_x = (*min_x).min(px);
                    *min_y = (*min_y).min(py);
                    *max_x = (*max_x).max(px);
                    *max_y = (*max_y).max(py);
                })
                .or_insert((px, py, px, py));
        }
    }

    let (ox, oy) = cell_origin;
    bounds
        .values()
        .map(|&(min_x, min_y, max_x, max_y)| ScreenBox {
            x: (ox + min_x) as i32,
            y: (oy + min_y) as i32,
            w: (max_x - min_x + 1) as i32,
            h: (max_y - min_y + 1) as i32,
        })
        .collect()
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn solid_cell(w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
        let mut v = Vec::with_capacity((w * h * 3) as usize);
        for _ in 0..(w * h) {
            v.extend_from_slice(&rgb);
        }
        v
    }

    /// Three isolated white rectangles on black — each yields one connected-
    /// component box that survives the area and aspect filters.
    fn isolated_rects_cell(w: u32, h: u32) -> Vec<u8> {
        let mut v = vec![0u8; (w * h * 3) as usize];
        // Rectangles are well-separated (>20px gap) so their edge rings never touch.
        let rects: &[(u32, u32, u32, u32)] = &[
            (20, 20, 50, 35),   // top-left region
            (120, 20, 50, 35),  // top-right region
            (60, 90, 50, 35),   // bottom-center region
        ];
        for &(rx, ry, rw, rh) in rects {
            for py in ry..(ry + rh) {
                for px in rx..(rx + rw) {
                    let idx = (py * w + px) as usize * 3;
                    v[idx]     = 240;
                    v[idx + 1] = 240;
                    v[idx + 2] = 240;
                }
            }
        }
        v
    }

    #[test]
    fn blank_cell_zero_boxes() {
        // A solid-colour cell has no edges → no connected components → zero boxes.
        let cell = solid_cell(200, 150, [128, 128, 128]);
        let result = propose_boxes(&cell, 200, 150, (0, 0));
        assert_eq!(result.raw_count, 0, "solid cell must produce zero raw boxes");
        assert!(result.boxes.is_empty());
    }

    #[test]
    fn isolated_rectangles_produce_filtered_boxes() {
        // Three isolated white rectangles (50×35 each) on a black background.
        // Each rectangle's edge pixels form one closed-loop component.
        // All three pass area filter (50×35=1750px >> MIN_BOX_AREA_PX=64) and
        // aspect filter (50/35≈1.43, within [0.05, 20.0]).
        let w = 200u32;
        let h = 150u32;
        let cell = isolated_rects_cell(w, h);
        let result = propose_boxes(&cell, w, h, (0, 0));
        assert!(
            result.raw_count >= 3,
            "three isolated rects must produce at least 3 raw components; got {}",
            result.raw_count
        );
        assert!(
            result.boxes.len() >= 3,
            "all three rects must survive filtering; got {} of {} raw",
            result.boxes.len(),
            result.raw_count
        );
    }

    #[test]
    fn boxes_are_in_screen_coordinates() {
        // Origin offset must be reflected in output box coordinates.
        let cell = isolated_rects_cell(200, 150);
        let origin = (500u32, 300u32);
        let result = propose_boxes(&cell, 200, 150, origin);
        for b in &result.boxes {
            assert!(b.x >= 500, "box x must be ≥ cell origin x");
            assert!(b.y >= 300, "box y must be ≥ cell origin y");
        }
    }

    #[test]
    fn tiny_cell_empty_rgb_graceful() {
        let result = propose_boxes(&[], 0, 0, (0, 0));
        assert_eq!(result.raw_count, 0);
        assert!(result.boxes.is_empty());
    }

    #[test]
    fn single_pixel_artifacts_filtered() {
        // A frame with isolated single-pixel edges — all components should have
        // area = 1×1 = 1 px, far below MIN_BOX_AREA_PX (64). All filtered out.
        let w = 100u32;
        let h = 80u32;
        let mut cell = solid_cell(w, h, [0, 0, 0]);
        // Place a handful of isolated bright pixels (each will be its own component)
        for &(px, py) in &[(10u32, 10u32), (50, 50), (90, 70)] {
            let idx = (py * w + px) as usize * 3;
            cell[idx] = 255;
            cell[idx + 1] = 255;
            cell[idx + 2] = 255;
        }
        let result = propose_boxes(&cell, w, h, (0, 0));
        // Raw components will exist for the bright pixels, but all area=1 → filtered
        assert!(
            result.boxes.is_empty(),
            "single-pixel components must be filtered by MIN_BOX_AREA_PX"
        );
    }
}
