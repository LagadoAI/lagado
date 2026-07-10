//! perception/dom.rs — the DOM floor's host side (production contract 2026-07-10).
//!
//! Deploys the stdlib CDP reader (`cdp_dom.py`) to the guest once per process and runs it over
//! the actuator's command channel. The page's visible interactive elements come back as LABELED
//! screen-pixel boxes (`arbiter::DomBox`) — the web equivalent of an a11y read, entering
//! `fuse()` as their own sense with honest `Dom` provenance.
//!
//! Fail-open by design: no browser, no debuggable page, a parse error — all return `None` and
//! perception proceeds on the other senses. A `pgrep` guard in the same command skips the
//! python spawn entirely when no Chromium-family process exists (the common non-web case).

use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::Value;

use crate::perception::arbiter::DomBox;
use crate::perception::Actuator;

const CDP_DOM_PY: &str = include_str!("../../python/guest/cdp_dom.py");
const GUEST_PATH: &str = "/tmp/lagado_cdp_dom.py";
const HEREDOC: &str = "LAGADO_CDP_DOM_EOF";

/// Once-per-process deploy latch. The guest file persists across steps; if the guest is
/// replaced mid-process the read simply fails (fail-open) until the next process deploys anew.
static DEPLOYED: AtomicBool = AtomicBool::new(false);

/// One DOM read: `Some((page_url, boxes))` when a debuggable page answered; `None` otherwise.
pub fn read_dom(act: &dyn Actuator) -> Option<(String, Vec<DomBox>)> {
    if !DEPLOYED.swap(true, Ordering::SeqCst) {
        act.run_command(&format!("cat > {GUEST_PATH} <<'{HEREDOC}'\n{CDP_DOM_PY}\n{HEREDOC}"));
    }
    // pgrep guard: skip the reader when no Chromium-family browser is running at all.
    let out = act.run_command(&format!(
        "pgrep -f 'chrom' >/dev/null 2>&1 && python3 {GUEST_PATH} 2>/dev/null \
         || echo '{{\"ok\": false, \"error\": \"no browser process\"}}'"));
    let line = out.lines().rev().find(|l| l.trim_start().starts_with('{'))?;
    parse_dom_json(line)
}

/// Parse the reader's JSON verdict line into (url, boxes). Pure — unit-tested without a guest.
pub fn parse_dom_json(line: &str) -> Option<(String, Vec<DomBox>)> {
    let v: Value = serde_json::from_str(line.trim()).ok()?;
    if !v.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        return None;
    }
    let url = v.get("url").and_then(Value::as_str).unwrap_or("").to_string();
    let els = v.get("elements")?.as_array()?;
    let boxes = els
        .iter()
        .filter_map(|e| {
            let x = e.get("x")?.as_i64()? as i32;
            let y = e.get("y")?.as_i64()? as i32;
            let w = e.get("w")?.as_i64()? as i32;
            let h = e.get("h")?.as_i64()? as i32;
            if w <= 0 || h <= 0 {
                return None;
            }
            let label = e.get("label").and_then(Value::as_str)
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_string);
            Some(DomBox { label, bbox: (x, y, w, h) })
        })
        .collect();
    Some((url, boxes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_reader_output_into_labeled_boxes() {
        let line = r#"{"ok": true, "url": "https://x.test/a", "title": "T", "dpr": 1,
            "elements": [
                {"tag": "a", "role": "", "label": "Sign in", "x": 812, "y": 40, "w": 64, "h": 22},
                {"tag": "input", "role": "searchbox", "label": "Search", "x": 100, "y": 80, "w": 300, "h": 28},
                {"tag": "div", "role": "button", "label": "", "x": 10, "y": 10, "w": 20, "h": 20}
            ]}"#;
        let (url, boxes) = parse_dom_json(line).unwrap();
        assert_eq!(url, "https://x.test/a");
        assert_eq!(boxes.len(), 3);
        assert_eq!(boxes[0].label.as_deref(), Some("Sign in"));
        assert_eq!(boxes[0].bbox, (812, 40, 64, 22));
        assert!(boxes[2].label.is_none(), "empty label → None, not empty string");
    }

    #[test]
    fn not_ok_or_malformed_is_none() {
        assert!(parse_dom_json(r#"{"ok": false, "error": "no browser process"}"#).is_none());
        assert!(parse_dom_json("not json").is_none());
        assert!(parse_dom_json(r#"{"ok": true}"#).is_none(), "ok without elements → None");
    }

    #[test]
    fn zero_area_elements_are_dropped() {
        let line = r#"{"ok": true, "url": "u", "elements":
            [{"label": "x", "x": 0, "y": 0, "w": 0, "h": 10}]}"#;
        let (_u, boxes) = parse_dom_json(line).unwrap();
        assert!(boxes.is_empty());
    }
}
