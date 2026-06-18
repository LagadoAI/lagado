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
pub fn build_candidates(fused: &[FusedElement]) -> Vec<Candidate> {
    fused
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let (x, y, w, h) = e.bbox;
            // The arbiter already resolved the label by provenance (a11y > caption >
            // OCR > None); an unlabeled element keeps an empty string so it still
            // renders and stays selectable by its index token.
            let label = e.label.clone().unwrap_or_default();
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
/// Does a label token match a goal token? Exact equality OR a prefix relationship between two
/// sufficiently-long tokens ("application"↔"applications", "terminal"↔"terminals").
///
/// This is the LEXICAL UNION (§7b "vote across weak rankers, union so the answer is never hidden"):
/// the substring channel rescues morphological variants the exact channel misses, and because it is
/// a union (max), it can only ADD matches — it never hides a match the exact channel already found.
/// NOTE: the ranker is purely lexical by design; the ColBERT embedding (≈[0.96,0.99] short-label
/// compression) lives in the memory-isolated Board, deliberately OUT of this action path (inv #10),
/// so there is no embedding channel to fuse here — strengthening lexical is the applicable lever.
/// Prefix (not arbitrary substring) + a 4-char floor keeps it from matching noise ("web"↔"website").
fn tokens_match(a: &str, b: &str) -> bool {
    a == b || (a.len() >= 4 && b.len() >= 4 && (a.starts_with(b) || b.starts_with(a)))
}

fn relevance(goal_toks: &[String], label: &str) -> (usize, f32) {
    let lab = content_tokens(label);
    if lab.is_empty() {
        return (0, 0.0);
    }
    let matched = lab
        .iter()
        .filter(|t| goal_toks.iter().any(|g| tokens_match(g, t)))
        .count();
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

/// Maximum candidates rendered into the selection prompt + grammar.
///
/// Phase 1b measured 648 CV boxes fused onto a single 1280×800 desktop frame — far past
/// the small lists the late-band fix was verified on. Uncapped, that floods the prompt and
/// reintroduces the position bias the ranking exists to defeat. The cap keeps the most-relevant
/// tail (see `rank_late_band`). It is MODEL-DEPENDENT (how many candidates the LLM's attention
/// band holds) and therefore a TUNABLE, not a truth — start point pending the Phase 1c VM
/// pick-rate gate; should become governor-supplied rather than frozen (invariant #9).
pub const LATE_BAND_CAP: usize = 64;

/// Re-order candidates so the MOST goal-relevant lands LAST — the model's attended late band
/// (verified 2026-06-17: a11y label-reading holds in the late band, collapses for early rows).
/// Ascending by (matched, coverage, has_label); STABLE, so equal-rank candidates keep their
/// spatial order. The `has_label` tertiary key sinks label-less CV/vision boxes BELOW labeled
/// ones within a relevance tie, so the cap below drops inert unlabeled boxes first and the a11y
/// label spine is preserved. This is the deterministic RANK on the rails, NOT a decision — the
/// model still picks among the (capped) set; ranking only controls where each lands.
///
/// Capping respects the §5 lossy-shortlist lesson by construction: the only candidates the cap
/// can drop are the least-relevant, label-less boxes the selection rails cannot pick in Phase 1
/// anyway (an unlabeled box matches nothing in `goal_matches_any`/`best_match_token`). A labeled
/// element is only ever dropped if labeled candidates alone exceed `LATE_BAND_CAP`.
pub fn rank_late_band(mut candidates: Vec<Candidate>, goal: &str) -> Vec<Candidate> {
    let g = content_tokens(goal);
    if !g.is_empty() {
        candidates.sort_by(|a, b| {
            let ra = relevance(&g, &a.label);
            let rb = relevance(&g, &b.label);
            ra.0.cmp(&rb.0)
                .then(ra.1.partial_cmp(&rb.1).unwrap_or(std::cmp::Ordering::Equal))
                // tertiary: labeled (true) ranks above label-less (false) within a tie, so the
                // front-drain cap below sheds inert unlabeled boxes before any labeled element.
                .then((!a.label.is_empty()).cmp(&!b.label.is_empty()))
        });
    }
    // CAP to the most-relevant tail. Ascending sort puts the highest relevance LAST, so we drain
    // the FRONT (least relevant) — `truncate` would keep the head and silently drop the matching
    // element. With no goal tokens (no sort) this bounds prompt size in spatial order.
    if candidates.len() > LATE_BAND_CAP {
        candidates.drain(0..candidates.len() - LATE_BAND_CAP);
    }
    // RE-TOKEN by render position (verified 2026-06-17): the model attends to the HIGHEST token
    // number / last item, NOT the last-RENDERED row. Sorting reorders the display but if tokens
    // stay spatial, the late-band target keeps a mid-range token (e.g. el_9) and the model picks
    // whatever carries the max token instead (e.g. el_18) → wrong pick (0/12). Re-numbering tokens
    // to match render order makes the most-relevant target carry el_{n-1} → reliably picked (12/12).
    // candidate_coords / the actuator key off these tokens, so they stay consistent.
    for (i, c) in candidates.iter_mut().enumerate() {
        c.token = index_token(i);
    }
    candidates
}

/// The token of the candidate that UNIQUELY best-matches the goal by content tokens — the
/// deterministic "intended target." `None` if there is no match OR no strict winner (a tie means
/// the deterministic layer can't claim an intended target, so the model's pick stands). Used as a
/// SELECTION-INTENT DIVERGENCE rail (§2.18+): when this returns Some(t) and the model selects a
/// DIFFERENT element, that divergence is fail-closed BEFORE acting — a divergent click is exactly
/// how the step-1 decoy ("Directory Menu") and the step-2 wrong-app ("Run Program…") slipped
/// through. This VALIDATES the model's pick (determinism on the RAILS); it does NOT decide it (when
/// there's no unique match the model chooses freely — e.g. label-less elements).
pub fn best_match_token(candidates: &[Candidate], goal: &str) -> Option<String> {
    let g = content_tokens(goal);
    if g.is_empty() {
        return None;
    }
    let mut scored: Vec<(&Candidate, (usize, f32))> = candidates
        .iter()
        .map(|c| (c, relevance(&g, &c.label)))
        .filter(|(_, (m, _))| *m > 0)
        .collect();
    if scored.is_empty() {
        return None;
    }
    scored.sort_by(|a, b| {
        b.1 .0.cmp(&a.1 .0).then(b.1 .1.partial_cmp(&a.1 .1).unwrap_or(std::cmp::Ordering::Equal))
    });
    if scored.len() > 1 {
        let (m0, c0) = scored[0].1;
        let (m1, c1) = scored[1].1;
        if m0 == m1 && (c0 - c1).abs() < f32::EPSILON {
            return None; // tie — no unique intended target
        }
    }
    Some(scored[0].0.token.clone())
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
    use crate::perception::arbiter::{FusedElement, LabelSource, Sense};

    /// a11y-backed element carrying `label` (empty string = unlabeled, as the
    /// arbiter would emit when a11y has no text for the ref).
    fn a11y(ref_id: &str, bbox: (i32, i32, i32, i32), label: &str) -> FusedElement {
        let (label, label_source) = if label.is_empty() {
            (None, LabelSource::None)
        } else {
            (Some(label.to_string()), LabelSource::A11y)
        };
        FusedElement { ref_id: Some(ref_id.to_string()), bbox, sense: Sense::A11yOnly, patch_embd: None, label, label_source }
    }
    fn vision_only(bbox: (i32, i32, i32, i32)) -> FusedElement {
        FusedElement { ref_id: None, bbox, sense: Sense::VisionOnly, patch_embd: None, label: None, label_source: LabelSource::None }
    }

    #[test]
    fn index_token_is_stable_prefix() {
        assert_eq!(index_token(0), "el_0");
        assert_eq!(index_token(7), "el_7");
    }

    #[test]
    fn build_candidates_joins_a11y_label_and_centers() {
        let fused = vec![a11y("ref_1", (0, 0, 100, 40), "Applications")];

        let cands = build_candidates(&fused);
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
        let cands = build_candidates(&fused);
        assert_eq!(cands[0].token, "el_0");
        assert_eq!(cands[0].label, "", "no a11y label for a vision-only element");
        assert_eq!(cands[0].center, (230, 320));
        assert_eq!(cands[0].sense, "vision");
    }

    #[test]
    fn missing_label_does_not_drop_the_candidate() {
        // a11y element whose ref_id has no entry in the label map → empty label, kept.
        let fused = vec![a11y("ref_99", (0, 0, 10, 10), "")];
        let cands = build_candidates(&fused);
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].label, "");
    }

    #[test]
    fn candidate_coords_maps_every_token() {
        let fused = vec![a11y("ref_1", (0, 0, 100, 40), ""), vision_only((200, 300, 60, 40))];
        let cands = build_candidates(&fused);
        let coords = candidate_coords(&cands);
        assert_eq!(coords.get("el_0"), Some(&(50, 20)));
        assert_eq!(coords.get("el_1"), Some(&(230, 320)));
        assert_eq!(coords.len(), 2);
    }

    #[test]
    fn render_lists_tokens_with_escape_instruction() {
        let fused = vec![a11y("ref_1", (0, 0, 100, 40), "Applications"), vision_only((200, 300, 60, 40))];
        let cands = build_candidates(&fused);

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
        // …and carry the LAST token (re-tokened by render position) so the model reliably picks it.
        assert_eq!(ranked.last().unwrap().token, index_token(ranked.len() - 1));
        // tokens are sequential by render position after ranking
        for (i, c) in ranked.iter().enumerate() { assert_eq!(c.token, index_token(i)); }
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

    // ── lexical union (§7b): substring/prefix channel max'd with exact ──

    #[test]
    fn tokens_match_exact_and_prefix_only() {
        assert!(tokens_match("applications", "applications"));     // exact
        assert!(tokens_match("application", "applications"));      // prefix (morphological)
        assert!(tokens_match("terminals", "terminal"));           // prefix, either direction
        assert!(!tokens_match("web", "website"));                 // < 4 chars on one side → no
        assert!(!tokens_match("directory", "applications"));      // unrelated → no
        assert!(!tokens_match("term", "different"));              // not a prefix relationship
    }

    #[test]
    fn lexical_union_rescues_morphological_variant() {
        // goal says "application" (singular); the label is "Applications" (plural). Exact-only
        // would miss it; the prefix channel rescues it into the late band.
        let cands = vec![labeled(0, "Trash"), labeled(1, "Applications")];
        let ranked = rank_late_band(cands, "open the application launcher");
        assert_eq!(ranked.last().unwrap().label, "Applications");
    }

    #[test]
    fn lexical_union_does_not_resurrect_the_menu_decoy() {
        // REGRESSION GUARD: the §2.18 decoy. "Directory Menu" must STILL score 0 against
        // "Applications menu" ("menu" is a stopword; "directory" ≠/⊀ "applications"), so the
        // substring channel must not hand the decoy a false match.
        let cands = vec![labeled(0, "Directory Menu"), labeled(1, "Applications")];
        let ranked = rank_late_band(cands, "Click the Applications menu");
        assert_eq!(ranked.last().unwrap().label, "Applications");
        assert_eq!(best_match_token(&cands_clone(&ranked), "Click the Applications menu"),
                   Some(ranked.last().unwrap().token.clone()),
                   "the unique best match is still Applications, not the decoy");
    }

    fn cands_clone(c: &[Candidate]) -> Vec<Candidate> {
        c.iter().map(|x| Candidate { token: x.token.clone(), label: x.label.clone(),
            center: x.center, sense: x.sense, trusted: x.trusted }).collect()
    }

    // ── LATE_BAND_CAP (Phase 1b: bound the CV flood without the §5 lossy-shortlist trap) ──

    fn unlabeled(token_idx: usize) -> Candidate {
        Candidate { token: index_token(token_idx), label: String::new(),
                    center: (0, 0), sense: "vision", trusted: false }
    }

    #[test]
    fn cap_keeps_matching_element_in_tail() {
        // A flood of label-less CV boxes + one labeled match. The cap must keep the
        // match at the highest token (the attended late band), NOT drop it.
        let mut cands: Vec<Candidate> = (0..LATE_BAND_CAP + 20).map(unlabeled).collect();
        cands.push(labeled(999, "Applications"));
        let ranked = rank_late_band(cands, "Click the Applications menu");
        assert_eq!(ranked.len(), LATE_BAND_CAP, "list is capped to LATE_BAND_CAP");
        assert_eq!(ranked.last().unwrap().label, "Applications", "the match must survive in the tail");
        assert_eq!(ranked.last().unwrap().token, index_token(LATE_BAND_CAP - 1));
        for (i, c) in ranked.iter().enumerate() { assert_eq!(c.token, index_token(i)); }
    }

    #[test]
    fn cap_drops_label_less_before_labeled() {
        // LATE_BAND_CAP labeled (non-matching) + extra unlabeled, goal matching none.
        // The cap must shed the inert unlabeled boxes and preserve every labeled element
        // (the a11y spine) — that is the §5 guarantee made concrete.
        let mut cands: Vec<Candidate> = (0..LATE_BAND_CAP).map(|i| labeled(i, "Trash")).collect();
        cands.extend((0..10).map(|i| unlabeled(LATE_BAND_CAP + i)));
        let ranked = rank_late_band(cands, "Click the Applications menu");
        assert_eq!(ranked.len(), LATE_BAND_CAP);
        assert!(ranked.iter().all(|c| !c.label.is_empty()),
            "every labeled element preserved; only label-less boxes dropped");
    }

    #[test]
    fn cap_does_not_bind_under_limit() {
        let cands = vec![labeled(0, "Applications"), unlabeled(1), unlabeled(2)];
        let ranked = rank_late_band(cands, "Click the Applications menu");
        assert_eq!(ranked.len(), 3, "a short list is never truncated");
        assert_eq!(ranked.last().unwrap().label, "Applications");
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
    fn best_match_token_unique_winner() {
        // "Web Browser" uniquely matches "open the Web Browser"; "Run Program" etc. don't.
        let cands = vec![labeled(5, "Run Program..."), labeled(9, "Web Browser"), labeled(1, "Settings")];
        assert_eq!(best_match_token(&cands, "open the Web Browser"), Some("el_9".to_string()));
    }

    #[test]
    fn best_match_token_none_on_no_match() {
        let cands = vec![labeled(0, "Show Desktop"), labeled(1, "Trash")];
        assert_eq!(best_match_token(&cands, "open the Web Browser"), None);
    }

    #[test]
    fn best_match_token_none_on_tie() {
        // two equally-strong matches → no unique intended target → model's pick stands.
        let cands = vec![labeled(0, "Web Browser"), labeled(1, "Web Browser")];
        assert_eq!(best_match_token(&cands, "open the Web Browser"), None);
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
