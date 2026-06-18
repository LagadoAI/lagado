//! perception/arbiter.rs — IoU-dedup fusion arbiter.
//!
//! Fuses three perception sources into one deduplicated element set:
//! - AT-SPI2 accessibility boxes (a11y) from PerceptionCache.bboxes
//! - Classical-CV proposer boxes from cv_proposer::ScreenBox
//! - Per-patch visual embeddings from vision::TilePatches
//!
//! Overview tiles (TilePatches::is_overview == true) are never used for
//! spatial attachment — their patch coords are zeroed and carry no spatial meaning.

use std::collections::HashMap;
use crate::perception::cv_proposer::ScreenBox;
use crate::vision::TilePatches;

/// IoU threshold for matching elements across perception sources.
///
/// MUST stay below 0.5. Rationale: a spatial vision patch in original image
/// coordinates spans ~25–27 px while AT-SPI2 boxes are native-pixel-precise.
/// The same GUI element therefore overlaps only partially across sources; a
/// threshold ≥ 0.5 would wrongly split matching elements into two separate
/// entries. IoU is a scale-invariant ratio — this constant does NOT depend on
/// frame resolution and must not be raised to "clean up" test numbers.
const MATCH_THRESHOLD: f32 = 0.30;

/// Which perception source(s) detected this element.
pub enum Sense {
    /// Only the AT-SPI2 accessibility tree reported this element.
    A11yOnly,
    /// Only the classical-CV proposer (pixel-level) reported this element.
    VisionOnly,
    /// Both the accessibility tree and at least one vision source agree.
    Both,
}

/// Which sense supplied an element's *label* (text), independent of which sense
/// supplied its *box*. Detection and labelling are separate merges: a box can be
/// `Both` (a11y + CV saw it) while its label comes solely from a11y. The merge
/// priority is `A11y > Caption > Ocr > None` — a real accessibility label always
/// wins; an OmniParser caption rescues a box a11y left unlabeled; OCR is the last
/// resort; `None` means no sense produced text (still selectable by index token).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelSource {
    /// Text came from the AT-SPI2 accessibility tree.
    A11y,
    /// Text came from an OmniParser-style captioner (Phase 2).
    Caption,
    /// Text came from OCR (Phase 2).
    Ocr,
    /// No sense produced a non-empty label for this element.
    None,
}

/// A fused, deduplicated GUI element from all active perception sources.
pub struct FusedElement {
    /// Some(ref_id) iff backed by an AT-SPI2 box; None for CV-only elements.
    pub ref_id: Option<String>,
    /// Representative bounding box (x, y, w, h) in screen pixels.
    pub bbox: (i32, i32, i32, i32),
    /// Which perception source(s) detected this element.
    pub sense: Sense,
    /// Embedding of the highest-IoU SPATIAL vision patch, if any.
    /// Never populated from an overview tile.
    pub patch_embd: Option<Vec<f32>>,
    /// Resolved label text by provenance priority (a11y > caption > OCR), or
    /// `None` when no sense produced text. A `None`-labelled element is still a
    /// valid selection target via its index token.
    pub label: Option<String>,
    /// Which sense supplied `label`. `LabelSource::None` iff `label` is `None`.
    pub label_source: LabelSource,
}

/// Resolve an element's label by provenance priority: real a11y label > OmniParser
/// caption > OCR text > None. Pure.
///
/// A source counts as *present* only if it is `Some` and non-empty after trimming —
/// an empty or whitespace-only a11y label must NOT mask a real caption below it.
/// This is the exact label-less case that breaks selection: a11y leaves the box
/// unlabeled, and the next sense down is supposed to rescue it.
pub fn resolve_label(
    a11y:    Option<&str>,
    caption: Option<&str>,
    ocr:     Option<&str>,
) -> (Option<String>, LabelSource) {
    for (text, src) in [
        (a11y,    LabelSource::A11y),
        (caption, LabelSource::Caption),
        (ocr,     LabelSource::Ocr),
    ] {
        if let Some(t) = text {
            let t = t.trim();
            if !t.is_empty() {
                return (Some(t.to_string()), src);
            }
        }
    }
    (None, LabelSource::None)
}

/// IoU of two `(x, y, w, h)` boxes. Returns 0.0 on zero-area input or no overlap. Pure.
pub fn iou(a: (i32, i32, i32, i32), b: (i32, i32, i32, i32)) -> f32 {
    let (ax, ay, aw, ah) = a;
    let (bx, by, bw, bh) = b;

    let a_area = aw * ah;
    let b_area = bw * bh;
    if a_area <= 0 || b_area <= 0 {
        return 0.0;
    }

    let ix1 = ax.max(bx);
    let iy1 = ay.max(by);
    let ix2 = (ax + aw).min(bx + bw);
    let iy2 = (ay + ah).min(by + bh);

    let inter = (ix2 - ix1).max(0) * (iy2 - iy1).max(0);
    if inter == 0 {
        return 0.0;
    }

    let union = a_area + b_area - inter;
    if union <= 0 {
        return 0.0;
    }

    inter as f32 / union as f32
}

/// Expand a `(x, y, w, h)` bbox outward by `dx` pixels per horizontal side
/// and `dy` pixels per vertical side. Used for ±1 patch edge fuzz compensation.
fn inflate(bbox: (i32, i32, i32, i32), dx: i32, dy: i32) -> (i32, i32, i32, i32) {
    (bbox.0 - dx, bbox.1 - dy, bbox.2 + 2 * dx, bbox.3 + 2 * dy)
}

/// Fuse three perception inputs into one deduplicated element set.
///
/// Step 1: seed from a11y — one FusedElement per box, sense = A11yOnly, label
///         resolved from `a11y_labels` (the only label source until Phase 2).
/// Step 2: for each CV box — dedup against VisionOnly elements, upgrade a matching
///         a11y element to Both, or add a new VisionOnly element. A CV box carries
///         no text, so an upgrade to `Both` keeps the a11y label untouched and a
///         new VisionOnly element is `None`-labelled.
/// Step 3: attach the highest-IoU SPATIAL patch embedding to each element.
///         Overview tiles (is_overview == true) are skipped entirely (C3).
pub fn fuse(
    a11y:        &HashMap<String, (i32, i32, i32, i32)>,
    a11y_labels: &HashMap<String, String>,
    cv_boxes:    &[ScreenBox],
    patches:     &[TilePatches],
) -> Vec<FusedElement> {
    // ── Step 1: seed from a11y ─────────────────────────────────────────────
    let mut elements: Vec<FusedElement> = a11y
        .iter()
        .map(|(id, &bbox)| {
            let (label, label_source) =
                resolve_label(a11y_labels.get(id).map(String::as_str), None, None);
            FusedElement {
                ref_id:     Some(id.clone()),
                bbox,
                sense:      Sense::A11yOnly,
                patch_embd: None,
                label,
                label_source,
            }
        })
        .collect();

    // ── Step 2: merge CV boxes ─────────────────────────────────────────────
    for sb in cv_boxes {
        let cv_bbox = (sb.x, sb.y, sb.w, sb.h);

        // 2a: dedup against already-accepted VisionOnly elements
        let duplicate = elements.iter().any(|e| {
            matches!(e.sense, Sense::VisionOnly) && iou(e.bbox, cv_bbox) >= MATCH_THRESHOLD
        });
        if duplicate {
            continue;
        }

        // 2b: find best-matching a11y element (A11yOnly or already-Both)
        let best_a11y_idx = elements
            .iter()
            .enumerate()
            .filter(|(_, e)| matches!(e.sense, Sense::A11yOnly | Sense::Both))
            .map(|(i, e)| (i, iou(e.bbox, cv_bbox)))
            .filter(|(_, s)| *s >= MATCH_THRESHOLD)
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i);

        match best_a11y_idx {
            Some(idx) => {
                // Upgrade the a11y element in place; keep its ref_id and bbox
                elements[idx].sense = Sense::Both;
            }
            None => {
                elements.push(FusedElement {
                    ref_id:       None,
                    bbox:         cv_bbox,
                    sense:        Sense::VisionOnly,
                    patch_embd:   None,
                    label:        None,            // CV carries no text; Phase 2 captions fill this
                    label_source: LabelSource::None,
                });
            }
        }
    }

    // ── Step 3: attach spatial patch embeddings (mean-pool all overlapping) ──
    // C3: filter out overview tiles before collecting patches.
    let spatial: Vec<&crate::vision::PatchEmbedding> = patches
        .iter()
        .filter(|t| !t.is_overview)
        .flat_map(|t| t.patches.iter())
        .collect();

    if !spatial.is_empty() {
        for elem in &mut elements {
            // Collect ALL spatial patches that overlap this element.
            // C2: inflate the element bbox by one patch dimension per side before
            // the IoU test to absorb ±1 patch edge fuzz.
            let overlapping: Vec<&crate::vision::PatchEmbedding> = spatial
                .iter()
                .copied()
                .filter(|patch| {
                    let patch_bbox = (
                        patch.patch_x as i32,
                        patch.patch_y as i32,
                        patch.patch_w as i32,
                        patch.patch_h as i32,
                    );
                    let inflated = inflate(
                        elem.bbox,
                        patch.patch_w as i32,
                        patch.patch_h as i32,
                    );
                    iou(inflated, patch_bbox) > 0.0
                })
                .collect();

            if overlapping.is_empty() {
                continue; // patch_embd stays None
            }

            // Plain arithmetic mean over all overlapping patches, dimension by dimension.
            // No IoU weighting — matches lagado_encode_image()'s mean-pool convention.
            // C4: derive length from first patch, never hardcode n_embd.
            let n_embd = overlapping[0].embd.len();
            let mut sum = vec![0.0f32; n_embd];
            let mut count = 0usize;
            for patch in &overlapping {
                if patch.embd.len() != n_embd {
                    continue; // skip mismatched-length patches rather than panic
                }
                for (d, &v) in patch.embd.iter().enumerate() {
                    sum[d] += v;
                }
                count += 1;
            }
            if count > 0 {
                elem.patch_embd = Some(sum.iter().map(|&s| s / count as f32).collect());
            }
        }
    }

    // Sort is the final step — MUST come after step 2's index-based merging is complete.
    elements.sort_by_key(|e| (e.bbox.1, e.bbox.0, e.bbox.2, e.bbox.3));
    elements
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::perception::cv_proposer::ScreenBox;
    use crate::vision::{PatchEmbedding, TilePatches};

    /// Empty a11y-label map for detection/embedding tests that don't exercise labels.
    fn nolabels() -> HashMap<String, String> {
        HashMap::new()
    }

    fn spatial_tile(patch_x: u32, patch_y: u32, patch_w: u32, patch_h: u32, embd: Vec<f32>) -> TilePatches {
        TilePatches {
            is_overview:   false,
            tile_origin_x: 0,
            tile_origin_y: 0,
            patches: vec![PatchEmbedding { patch_x, patch_y, patch_w, patch_h, embd }],
        }
    }

    fn overview_tile() -> TilePatches {
        TilePatches {
            is_overview:   true,
            tile_origin_x: 0,
            tile_origin_y: 0,
            patches: vec![PatchEmbedding { patch_x: 0, patch_y: 0, patch_w: 0, patch_h: 0, embd: vec![7.0] }],
        }
    }

    // ── IoU ──────────────────────────────────────────────────────────────────

    #[test]
    fn iou_identical_boxes() {
        assert!((iou((0, 0, 100, 100), (0, 0, 100, 100)) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn iou_disjoint_boxes() {
        assert_eq!(iou((0, 0, 50, 50), (100, 100, 50, 50)), 0.0);
    }

    #[test]
    fn iou_partial_overlap_in_range() {
        // a=(0,0,100,100), b=(50,0,100,100): inter=50×100=5000, union=15000, iou≈0.333
        let v = iou((0, 0, 100, 100), (50, 0, 100, 100));
        assert!(v > 0.3 && v < 0.5, "expected iou in (0.3, 0.5), got {v}");
    }

    #[test]
    fn iou_zero_area_returns_zero() {
        assert_eq!(iou((0, 0, 0, 100), (0, 0, 100, 100)), 0.0);
    }

    // ── A11y preservation ────────────────────────────────────────────────────

    #[test]
    fn a11y_only_preserved_with_no_cv_match() {
        let mut a11y = HashMap::new();
        a11y.insert("ref_1".to_string(), (0, 0, 100, 100));
        // CV box is far away — no overlap
        let cv = vec![ScreenBox { x: 500, y: 500, w: 50, h: 50 }];

        let result = fuse(&a11y, &nolabels(), &cv, &[]);

        let elem = result.iter().find(|e| e.ref_id.as_deref() == Some("ref_1")).unwrap();
        assert!(matches!(elem.sense, Sense::A11yOnly));
    }

    // ── Both ─────────────────────────────────────────────────────────────────

    #[test]
    fn both_when_cv_overlaps_a11y_at_loose_threshold() {
        // a11y=(0,0,100,100), cv=(50,0,100,100) → iou≈0.333 ≥ 0.30 → Both
        // Proves loose threshold merges: would FAIL at threshold 0.5 (0.333 < 0.5)
        let mut a11y = HashMap::new();
        a11y.insert("ref_1".to_string(), (0, 0, 100, 100));
        let cv = vec![ScreenBox { x: 50, y: 0, w: 100, h: 100 }];

        let result = fuse(&a11y, &nolabels(), &cv, &[]);

        // One merged element, not two
        assert_eq!(result.len(), 1, "overlapping a11y+CV must merge, not split");
        assert!(matches!(result[0].sense, Sense::Both));
        assert_eq!(result[0].ref_id.as_deref(), Some("ref_1"));
    }

    // ── VisionOnly ───────────────────────────────────────────────────────────

    #[test]
    fn vision_only_cv_with_no_a11y_overlap() {
        let a11y = HashMap::new();
        let cv = vec![ScreenBox { x: 200, y: 200, w: 60, h: 40 }];

        let result = fuse(&a11y, &nolabels(), &cv, &[]);

        assert_eq!(result.len(), 1);
        assert!(matches!(result[0].sense, Sense::VisionOnly));
        assert!(result[0].ref_id.is_none());
    }

    // ── CV self-dedup ─────────────────────────────────────────────────────────

    #[test]
    fn cv_self_dedup_heavy_overlap() {
        // (0,0,100,100) and (10,10,80,80): second is nearly inside first
        // iou = 6400 / (10000+6400-6400) = 6400/10000 = 0.64 ≥ 0.30 → deduplicated
        let a11y = HashMap::new();
        let cv = vec![
            ScreenBox { x: 0,  y: 0,  w: 100, h: 100 },
            ScreenBox { x: 10, y: 10, w: 80,  h: 80  },
        ];

        let result = fuse(&a11y, &nolabels(), &cv, &[]);

        assert_eq!(result.len(), 1, "heavily-overlapping CV boxes must collapse to one VisionOnly");
        assert!(matches!(result[0].sense, Sense::VisionOnly));
    }

    // ── Embedding attach ──────────────────────────────────────────────────────

    #[test]
    fn embedding_attached_for_overlapping_spatial_patch() {
        let mut a11y = HashMap::new();
        a11y.insert("ref_1".to_string(), (0, 0, 100, 100));
        let tile = spatial_tile(0, 0, 27, 25, vec![1.0, 2.0, 3.0]);

        let result = fuse(&a11y, &nolabels(), &[], &[tile]);

        assert!(result[0].patch_embd.is_some(), "overlapping spatial patch must attach");
        assert_eq!(result[0].patch_embd.as_ref().unwrap(), &[1.0_f32, 2.0, 3.0]);
    }

    // ── ±1 fuzz ───────────────────────────────────────────────────────────────

    #[test]
    fn edge_fuzz_attaches_just_outside_raw_bbox() {
        // Element at (0,0,50,50). Patch at x=55 — 5px outside the raw element bbox.
        // Raw iou = 0 (no overlap). With inflate by patch_w=27 → inflated element
        // reaches x=77, overlapping the patch at x=55..82 → iou > 0 → attaches.
        let mut a11y = HashMap::new();
        a11y.insert("ref_fuzz".to_string(), (0, 0, 50, 50));
        let tile = spatial_tile(55, 0, 27, 25, vec![9.0]);

        // Confirm raw (no inflate) iou is zero — proves inflate is load-bearing
        assert_eq!(
            iou((0, 0, 50, 50), (55, 0, 27, 25)),
            0.0,
            "without inflate the patch must not overlap the element"
        );

        let result = fuse(&a11y, &nolabels(), &[], &[tile]);

        assert!(
            result[0].patch_embd.is_some(),
            "patch just outside raw bbox must attach via ±1 inflate margin"
        );
    }

    // ── Overview skipped ──────────────────────────────────────────────────────

    #[test]
    fn overview_tile_never_attached() {
        let mut a11y = HashMap::new();
        a11y.insert("ref_1".to_string(), (0, 0, 100, 100));

        let result = fuse(&a11y, &nolabels(), &[], &[overview_tile()]);

        assert!(
            result[0].patch_embd.is_none(),
            "overview tile must never be used for spatial embedding"
        );
    }

    // ── All-overview / no spatial patches ────────────────────────────────────

    #[test]
    fn all_overview_no_spatial_gives_no_embd_no_panic() {
        let mut a11y = HashMap::new();
        a11y.insert("ref_1".to_string(), (0, 0, 100, 100));
        let cv = vec![ScreenBox { x: 200, y: 200, w: 50, h: 50 }];

        let result = fuse(&a11y, &nolabels(), &cv, &[overview_tile()]);

        assert!(!result.is_empty());
        for elem in &result {
            assert!(elem.patch_embd.is_none(), "no spatial patches → no embd on any element");
        }
    }

    // ── Empty inputs ──────────────────────────────────────────────────────────

    #[test]
    fn empty_inputs_returns_empty_no_panic() {
        let result = fuse(&HashMap::new(), &nolabels(), &[], &[]);
        assert!(result.is_empty());
    }

    // ── Mean-pool over multiple overlapping patches ────────────────────────

    #[test]
    fn mean_pool_averages_multiple_overlapping_patches() {
        // Element at (0,0,50,50). Two patches both overlapping via inflate margin.
        // patch A at (0,0,27,25) embd=[2.0, 4.0]
        // patch B at (20,0,27,25) embd=[4.0, 8.0]
        // Expected mean: [3.0, 6.0]
        let mut a11y = HashMap::new();
        a11y.insert("ref_1".to_string(), (0, 0, 50, 50));

        let tile = TilePatches {
            is_overview:   false,
            tile_origin_x: 0,
            tile_origin_y: 0,
            patches: vec![
                PatchEmbedding { patch_x: 0,  patch_y: 0, patch_w: 27, patch_h: 25, embd: vec![2.0, 4.0] },
                PatchEmbedding { patch_x: 20, patch_y: 0, patch_w: 27, patch_h: 25, embd: vec![4.0, 8.0] },
            ],
        };

        let result = fuse(&a11y, &nolabels(), &[], &[tile]);
        let embd = result[0].patch_embd.as_ref().expect("patch_embd must be Some");
        assert!(
            (embd[0] - 3.0).abs() < 1e-5,
            "dim 0 mean must be 3.0, got {}", embd[0]
        );
        assert!(
            (embd[1] - 6.0).abs() < 1e-5,
            "dim 1 mean must be 6.0, got {}", embd[1]
        );
    }

    // ── Deterministic output order ────────────────────────────────────────

    #[test]
    fn deterministic_order_sorted_by_bbox() {
        // Insert boxes in an order that does NOT match sorted (y, x, w, h).
        // HashMap iteration is non-deterministic, so two fuse() calls may visit
        // boxes in different orders — the sort guarantees identical output both times.
        let mut a11y = HashMap::new();
        a11y.insert("ref_c".to_string(), (200, 300, 50, 50)); // y=300
        a11y.insert("ref_a".to_string(), (10,  100, 30, 20)); // y=100
        a11y.insert("ref_b".to_string(), (5,   200, 40, 30)); // y=200

        let result1 = fuse(&a11y, &nolabels(), &[], &[]);
        let result2 = fuse(&a11y, &nolabels(), &[], &[]);

        // Both runs must produce the same bbox sequence
        let bboxes1: Vec<_> = result1.iter().map(|e| e.bbox).collect();
        let bboxes2: Vec<_> = result2.iter().map(|e| e.bbox).collect();
        assert_eq!(bboxes1, bboxes2, "fuse() must be deterministic across calls");

        // And the sequence must be sorted ascending by (y, x, w, h)
        let sorted = {
            let mut v = bboxes1.clone();
            v.sort_by_key(|&(x, y, w, h)| (y, x, w, h));
            v
        };
        assert_eq!(bboxes1, sorted, "output must be sorted by (y, x, w, h)");
    }

    // ── Label provenance (Phase 1) ────────────────────────────────────────────

    #[test]
    fn resolve_label_a11y_wins_over_caption_and_ocr() {
        // Real a11y label is authoritative — captions/OCR never override it.
        let (label, src) = resolve_label(Some("Applications"), Some("menu icon"), Some("Aplicaticns"));
        assert_eq!(label.as_deref(), Some("Applications"));
        assert_eq!(src, LabelSource::A11y);
    }

    #[test]
    fn resolve_label_empty_a11y_falls_through_to_caption() {
        // The label-less case this whole layer exists to fix: a11y produced an
        // empty/whitespace string → the caption rescues the element.
        let (label, src) = resolve_label(Some("   "), Some("Submit button"), None);
        assert_eq!(label.as_deref(), Some("Submit button"));
        assert_eq!(src, LabelSource::Caption);
    }

    #[test]
    fn resolve_label_ocr_is_last_resort() {
        let (label, src) = resolve_label(None, None, Some("OK"));
        assert_eq!(label.as_deref(), Some("OK"));
        assert_eq!(src, LabelSource::Ocr);
    }

    #[test]
    fn resolve_label_all_absent_is_none() {
        let (label, src) = resolve_label(None, Some(""), Some("  "));
        assert!(label.is_none(), "no non-empty source → None");
        assert_eq!(src, LabelSource::None);
    }

    #[test]
    fn fuse_carries_a11y_label_onto_element() {
        let mut a11y = HashMap::new();
        a11y.insert("ref_1".to_string(), (0, 0, 100, 40));
        let mut labels = HashMap::new();
        labels.insert("ref_1".to_string(), "Applications".to_string());

        let result = fuse(&a11y, &labels, &[], &[]);

        assert_eq!(result[0].label.as_deref(), Some("Applications"));
        assert_eq!(result[0].label_source, LabelSource::A11y);
    }

    #[test]
    fn fuse_vision_only_element_is_unlabeled() {
        // A CV-only box carries no text — it must be None-labelled (not "" of
        // ambiguous provenance), yet still present and selectable downstream.
        let a11y = HashMap::new();
        let cv = vec![ScreenBox { x: 200, y: 200, w: 60, h: 40 }];

        let result = fuse(&a11y, &nolabels(), &cv, &[]);

        assert_eq!(result.len(), 1);
        assert!(result[0].label.is_none());
        assert_eq!(result[0].label_source, LabelSource::None);
    }

    #[test]
    fn fuse_both_element_keeps_a11y_label_after_cv_upgrade() {
        // CV overlaps an a11y box → sense upgrades to Both, but the label (which
        // CV cannot supply) must stay the a11y label, not get wiped.
        let mut a11y = HashMap::new();
        a11y.insert("ref_1".to_string(), (0, 0, 100, 100));
        let mut labels = HashMap::new();
        labels.insert("ref_1".to_string(), "Save".to_string());
        let cv = vec![ScreenBox { x: 50, y: 0, w: 100, h: 100 }]; // iou≈0.333 ≥ 0.30

        let result = fuse(&a11y, &labels, &cv, &[]);

        assert_eq!(result.len(), 1);
        assert!(matches!(result[0].sense, Sense::Both));
        assert_eq!(result[0].label.as_deref(), Some("Save"), "CV upgrade must not wipe the a11y label");
        assert_eq!(result[0].label_source, LabelSource::A11y);
    }
}
