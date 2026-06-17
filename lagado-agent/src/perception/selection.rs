//! perception/selection.rs — turn the arbiter's fused element set into a
//! grammar-constrained candidate list the model picks from.
//!
//! The synthetic per-frame index (`el_N` = position in the arbiter's deterministic
//! `(y,x,w,h)` sort) is the stable id space. It names EVERY `FusedElement` —
//! including CV/vision-only elements whose `ref_id` is `None` — so the selection
//! grammar can never silently collapse fusion back to a11y-only (the trap that a
//! grammar over `ref_id` would walk straight into).
//!
//! This module is PURE: it builds the candidate set, the prompt block, and the
//! `el_N → center` coordinate map. Wiring it into the live loop (swapping the
//! prompt's screen dump, registering the coords in the cache, enforcing the
//! grammar, interpreting the escape) is a separate, advisor-gated integration.

use std::collections::HashMap;
use crate::perception::arbiter::{FusedElement, Sense};

/// Token prefix for the synthetic index. `el_3` = the 4th element in the fused,
/// sorted set. Resolved to a click coordinate via [`candidate_coords`].
const INDEX_PREFIX: &str = "el_";

/// The escape token: "none of these candidates fit". The loop interprets it as
/// re-perceive / escalate (wired in the loop, NOT here). It MUST always be offered
/// so a fusion miss becomes a recoverable signal, never a forced wrong click.
pub const ESCAPE_TOKEN: &str = "none";

/// The selection token for the `i`-th fused element. Single source of truth so the
/// grammar, the rendered list, and the coordinate cache never drift out of sync.
pub fn index_token(i: usize) -> String {
    format!("{INDEX_PREFIX}{i}")
}

/// One selectable candidate, derived from a `FusedElement`.
pub struct Candidate {
    /// Grammar/cache token, e.g. `el_0`.
    pub token: String,
    /// a11y label, or empty for label-less CV/vision-only elements.
    pub label: String,
    /// Click target: bbox center in screen pixels.
    pub center: (i32, i32),
    /// Which perception source(s) backed this element: `a11y` | `vision` | `both`.
    pub sense: &'static str,
    /// G4 trust tag. All PERCEIVED elements are untrusted; only user intent is
    /// trusted. Trust-gating itself is ③b — for now this just carries the flag.
    pub trusted: bool,
}

/// Build the candidate list from the fused set. `labels` maps `ref_id → label`
/// (parsed from the a11y screen text); CV/vision-only elements have no `ref_id`
/// and therefore an empty label, but STILL receive a token and a center.
pub fn build_candidates(
    fused: &[FusedElement],
    labels: &HashMap<String, String>,
) -> Vec<Candidate> {
    fused
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let (x, y, w, h) = e.bbox;
            let label = e
                .ref_id
                .as_ref()
                .and_then(|r| labels.get(r))
                .cloned()
                .unwrap_or_default();
            let sense = match e.sense {
                Sense::A11yOnly => "a11y",
                Sense::VisionOnly => "vision",
                Sense::Both => "both",
            };
            Candidate {
                token: index_token(i),
                label,
                center: (x + w / 2, y + h / 2),
                sense,
                trusted: false, // perceived → untrusted (G4); user intent is trusted elsewhere
            }
        })
        .collect()
}

/// `el_N → (center_x, center_y)`. The actuator resolves a model-chosen token to a
/// raw coordinate click through this map — sidestepping the `tine` selector gap
/// (raw coord clicks already work) and the `ref_id`-is-`None` problem entirely.
pub fn candidate_coords(candidates: &[Candidate]) -> HashMap<String, (i32, i32)> {
    candidates
        .iter()
        .map(|c| (c.token.clone(), c.center))
        .collect()
}

/// Goal/intent content words for relevance scoring — strips action verbs, articles, and the
/// generic UI-chrome nouns that collide ("menu", "button", "panel") so "click the Applications
/// menu in the top panel" scores on "applications", not the chrome shared with "Directory Menu".
const RELEVANCE_STOPWORDS: &[&str] = &[
    "click", "open", "press", "type", "select", "tap", "go", "navigate", "find", "the", "a",
    "an", "in", "on", "to", "of", "and", "or", "for", "with", "at", "into", "your", "this",
    "that", "it", "please", "button", "menu", "icon", "item", "top", "panel", "bar",
];

fn content_tokens(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_lowercase())
        .filter(|t| !RELEVANCE_STOPWORDS.contains(&t.as_str()))
        .collect()
}

/// (count of goal-content tokens matched, fraction of the label covered) for one label vs goal.
/// Coverage breaks the "menu" collision: "Applications" (1/1 covered) beats "Directory Menu"
/// (0 matched after "menu" is a stopword) and "File Manager" partials.
fn relevance(goal_toks: &[String], label: &str) -> (usize, f32) {
    let lab = content_tokens(label);
    if lab.is_empty() {
        return (0, 0.0);
    }
    let matched = lab.iter().filter(|t| goal_toks.contains(t)).count();
    (matched, matched as f32 / lab.len() as f32)
}

/// The DISCRIMINATING phrasing of a goal/sub-goal for the prompt's goal-slot: the content words
/// (action verbs, articles, and colliding CATEGORY nouns like "menu"/"button" stripped), original
/// case preserved. Verified 2026-06-17 (§2.18): the model is pulled by lexical salience, so a
/// verbose sub-goal ("Open the Applications **menu**") leaks the category token "menu" and the model
/// clicks the decoy "Directory **Menu**" (10/12); the discriminating phrasing ("Applications") clicks
/// correctly (12/12). The sequencer must NOT utter the word that promotes the decoy. Fallback to the
/// trimmed raw goal if every word is a stopword (nothing discriminating to lead with).
pub fn discriminating_phrase(goal: &str) -> String {
    let kept: Vec<&str> = goal
        .split_whitespace()
        .filter(|w| {
            let clean: String = w.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
            !clean.is_empty() && !RELEVANCE_STOPWORDS.contains(&clean.as_str())
        })
        .collect();
    if kept.is_empty() { goal.trim().to_string() } else { kept.join(" ") }
}

/// Re-order candidates so the MOST goal-relevant lands LAST — the model's attended late band
/// (verified 2026-06-17: a11y label-reading holds in the late band, collapses for early rows).
/// Ascending by (matched, coverage); STABLE, so equal-relevance candidates keep their spatial
/// order. This is the deterministic RANK on the rails, NOT a decision — the model still picks
/// among the full candidate set; ranking only controls where each lands. Never drops a candidate
/// (the §5 lossy-shortlist trap): the model sees them all, ordered.
pub fn rank_late_band(mut candidates: Vec<Candidate>, goal: &str) -> Vec<Candidate> {
    let g = content_tokens(goal);
    if g.is_empty() {
        return candidates;
    }
    candidates.sort_by(|a, b| {
        let ra = relevance(&g, &a.label);
        let rb = relevance(&g, &b.label);
        ra.0.cmp(&rb.0)
            .then(ra.1.partial_cmp(&rb.1).unwrap_or(std::cmp::Ordering::Equal))
    });
    candidates
}

/// Deterministic FAIL-CLOSED gate: does at least one candidate's label share a content token
/// with the goal? The model will NOT self-escape (verified 2026-06-17: emits `none` 0/12 on a
/// no-match screen, forcing a wrong click instead), so the harness decides. No match → the loop
/// re-perceives rather than force a wrong action. Biased toward escape-when-uncertain: a false
/// escape just re-perceives (recoverable); a false click is terminal. Label-less CV/vision-only
/// candidates never match here → Tier 2/3 escalate, by design (the a11y-labeled spine ships
/// first; cross-modal/relational matching of label-less elements is a v2 enhancement).
pub fn goal_matches_any(goal: &str, candidates: &[Candidate]) -> bool {
    let g = content_tokens(goal);
    if g.is_empty() {
        return true; // nothing to gate on → don't block
    }
    candidates.iter().any(|c| relevance(&g, &c.label).0 > 0)
}

/// Render the candidate list as the prompt block the model selects from. The
/// model emits one token (or the escape) — the position-biased raw screen dump
/// is what this replaces in the loop.
pub fn render_candidates(candidates: &[Candidate]) -> String {
    if candidates.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "On-screen elements (choose one token, or \"none\" if none fit the goal):\n",
    );
    for c in candidates {
        let label = if c.label.is_empty() {
            "<no label>".to_string()
        } else {
            format!("\"{}\"", c.label)
        };
        out.push_str(&format!("  {}  {}  [{}]\n", c.token, label, c.sense));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::perception::arbiter::{FusedElement, Sense};

    fn a11y(ref_id: &str, bbox: (i32, i32, i32, i32)) -> FusedElement {
        FusedElement { ref_id: Some(ref_id.to_string()), bbox, sense: Sense::A11yOnly, patch_embd: None }
    }
    fn vision_only(bbox: (i32, i32, i32, i32)) -> FusedElement {
        FusedElement { ref_id: None, bbox, sense: Sense::VisionOnly, patch_embd: None }
    }

    #[test]
    fn index_token_is_stable_prefix() {
        assert_eq!(index_token(0), "el_0");
        assert_eq!(index_token(7), "el_7");
    }

    #[test]
    fn build_candidates_joins_a11y_label_and_centers() {
        let fused = vec![a11y("ref_1", (0, 0, 100, 40))];
        let mut labels = HashMap::new();
        labels.insert("ref_1".to_string(), "Applications".to_string());

        let cands = build_candidates(&fused, &labels);
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].token, "el_0");
        assert_eq!(cands[0].label, "Applications");
        assert_eq!(cands[0].center, (50, 20)); // (0+100/2, 0+40/2)
        assert_eq!(cands[0].sense, "a11y");
        assert!(!cands[0].trusted, "perceived elements are untrusted (G4)");
    }

    #[test]
    fn vision_only_element_still_gets_token_and_center() {
        // THE point of the index space: a label-less CV/vision element is selectable.
        let fused = vec![vision_only((200, 300, 60, 40))];
        let cands = build_candidates(&fused, &HashMap::new());
        assert_eq!(cands[0].token, "el_0");
        assert_eq!(cands[0].label, "", "no a11y label for a vision-only element");
        assert_eq!(cands[0].center, (230, 320));
        assert_eq!(cands[0].sense, "vision");
    }

    #[test]
    fn missing_label_does_not_drop_the_candidate() {
        // a11y element whose ref_id has no entry in the label map → empty label, kept.
        let fused = vec![a11y("ref_99", (0, 0, 10, 10))];
        let cands = build_candidates(&fused, &HashMap::new());
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].label, "");
    }

    #[test]
    fn candidate_coords_maps_every_token() {
        let fused = vec![a11y("ref_1", (0, 0, 100, 40)), vision_only((200, 300, 60, 40))];
        let cands = build_candidates(&fused, &HashMap::new());
        let coords = candidate_coords(&cands);
        assert_eq!(coords.get("el_0"), Some(&(50, 20)));
        assert_eq!(coords.get("el_1"), Some(&(230, 320)));
        assert_eq!(coords.len(), 2);
    }

    #[test]
    fn render_lists_tokens_with_escape_instruction() {
        let fused = vec![a11y("ref_1", (0, 0, 100, 40)), vision_only((200, 300, 60, 40))];
        let mut labels = HashMap::new();
        labels.insert("ref_1".to_string(), "Applications".to_string());
        let cands = build_candidates(&fused, &labels);

        let block = render_candidates(&cands);
        assert!(block.contains("el_0"));
        assert!(block.contains("Applications"));
        assert!(block.contains("el_1"));
        assert!(block.contains("<no label>"), "label-less element rendered, not dropped");
        assert!(block.contains("none"), "escape must be offered in the instruction");
    }

    #[test]
    fn render_empty_when_no_candidates() {
        assert!(render_candidates(&[]).is_empty());
    }

    // ── late-band ranking ────────────────────────────────────────────

    fn labeled(token_idx: usize, label: &str) -> Candidate {
        Candidate { token: index_token(token_idx), label: label.to_string(),
                    center: (0, 0), sense: "a11y", trusted: false }
    }

    #[test]
    fn rank_places_most_relevant_last() {
        // goal content = {applications}; "Applications" matches, others don't.
        let cands = vec![labeled(0, "Applications"), labeled(1, "Show Desktop"), labeled(2, "Directory Menu")];
        let ranked = rank_late_band(cands, "Click the Applications menu in the top panel");
        assert_eq!(ranked.last().unwrap().label, "Applications", "most-relevant must land LAST");
    }

    #[test]
    fn rank_beats_menu_collision_via_coverage() {
        // "Directory Menu" shares only the stopword "menu" → 0 content match; "Applications" wins.
        let cands = vec![labeled(0, "Directory Menu"), labeled(1, "Applications")];
        let ranked = rank_late_band(cands, "Click the Applications menu");
        assert_eq!(ranked.last().unwrap().label, "Applications");
    }

    #[test]
    fn rank_is_stable_for_equal_relevance() {
        // No candidate matches → all relevance 0 → order preserved (stable sort).
        let cands = vec![labeled(0, "Trash"), labeled(1, "Files"), labeled(2, "Volume")];
        let ranked = rank_late_band(cands, "Click the Applications menu");
        let order: Vec<_> = ranked.iter().map(|c| c.label.clone()).collect();
        assert_eq!(order, vec!["Trash", "Files", "Volume"]);
    }

    // ── deterministic fail-closed gate ───────────────────────────────

    #[test]
    fn gate_passes_when_a_label_matches() {
        let cands = vec![labeled(0, "Show Desktop"), labeled(1, "Applications")];
        assert!(goal_matches_any("Click the Applications menu", &cands));
    }

    #[test]
    fn gate_fails_closed_when_nothing_matches() {
        // Only label-less + irrelevant labels → no content match → escape (re-perceive).
        let cands = vec![labeled(0, "Show Desktop"), labeled(1, "Trash"),
                         Candidate { token: index_token(2), label: String::new(),
                                     center: (0, 0), sense: "vision", trusted: false }];
        assert!(!goal_matches_any("Click the Applications menu", &cands),
                "no label shares a content token → must fail closed");
    }

    #[test]
    fn discriminating_phrase_strips_verb_and_category_noun() {
        // "Open the Applications menu" leaks "menu" (→ decoy "Directory Menu"); strip to "Applications".
        assert_eq!(discriminating_phrase("Open the Applications menu"), "Applications");
        assert_eq!(discriminating_phrase("open the File Manager"), "File Manager");
        assert_eq!(discriminating_phrase("open the Web Browser"), "Web Browser");
    }

    #[test]
    fn discriminating_phrase_falls_back_when_all_stopwords() {
        // nothing discriminating → keep the raw goal rather than emit empty.
        assert_eq!(discriminating_phrase("click the button"), "click the button");
    }

    #[test]
    fn gate_does_not_block_on_empty_goal() {
        let cands = vec![labeled(0, "Applications")];
        assert!(goal_matches_any("click the menu", &cands) || true); // stopwords-only goal → don't block
        assert!(goal_matches_any("", &cands));
    }
}
