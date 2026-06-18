//! grammar.rs — GBNF constraint generation for llama-server.
//!
//! When the model picks a screen element, constrain output to valid ref_ids.
//! Phase 1: stub interface. Phase 2: dynamic GBNF from perception output.

/// Generate a GBNF grammar over the FUSED element set: a single interaction
/// bracket-call whose `selector` is constrained to one synthetic index token
/// (`el_0`, `el_1`, …) or the mandatory escape (`none`).
///
/// Key design (spec §2): the grammar is keyed on the per-frame INDEX, not on
/// `FusedElement.ref_id`. `ref_id` is `None` for CV/DOM/vision-only elements, so a
/// grammar over `ref_id` would silently drop exactly the elements fusion exists to
/// recover. Indexing by position in the arbiter's deterministic `(y,x,w,h)` sort
/// names every element. The actuator resolves `el_N` → bbox-center → coord click.
///
/// Scope: the GUI-interaction subset (`click`/`type`/`key`/`wait`/`done`). Whether
/// to also admit the 44 native/MCP `invoke` tools inside the constrained set is an
/// open integration decision (constraining them out would forbid them mid-task) —
/// deliberately NOT baked in here.
///
/// `none` (the escape) is always offered: a fusion miss becomes a recoverable
/// "none of these fit" signal, never a forced wrong click. Empty input → empty
/// grammar (no constraint), so callers fall back to unconstrained decoding.
pub fn selector_grammar(elements: &[crate::perception::arbiter::FusedElement]) -> String {
    if elements.is_empty() {
        return String::new();
    }
    use std::fmt::Write;
    use crate::perception::selection::{index_token, ESCAPE_TOKEN};

    // target alternation: every fused element's synthetic index + the escape.
    let mut targets: Vec<String> = (0..elements.len())
        .map(|i| format!("\"{}\"", index_token(i)))
        .collect();
    targets.push(format!("\"{ESCAPE_TOKEN}\""));
    let target_rule = targets.join(" | ");

    let mut g = String::new();
    let _ = writeln!(g, "root ::= click | type | key | wait | done");
    let _ = writeln!(g, r#"click ::= "click(selector=\"" target "\")""#);
    let _ = writeln!(g, r#"type ::= "type(selector=\"" target "\", text=\"" freetext "\")""#);
    let _ = writeln!(g, r#"key ::= "key(key=\"" freetext "\")""#);
    let _ = writeln!(g, r#"wait ::= "wait(ms=" [0-9]+ ")""#);
    let _ = writeln!(g, r#"done ::= "done(reason=\"" freetext "\")""#);
    let _ = writeln!(g, "target ::= {target_rule}");
    let _ = writeln!(g, r#"freetext ::= [^"\\]*"#);
    g
}

/// GBNF that forces the intent classifier to emit exactly one label token.
/// Eliminates the silent "unparseable output → CHAT default" failure mode:
/// without this, the 1.2B echoes message words ("Escape", "Search") that parse
/// to no label and fall through to CHAT, so an action request silently no-ops.
pub fn intent_grammar() -> String {
    r#"root ::= ("CHAT" | "INTERACTIVE" | "REASONING")"#.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intent_grammar_lists_all_three_labels() {
        let g = intent_grammar();
        assert!(!g.is_empty());
        assert!(g.contains("CHAT"));
        assert!(g.contains("INTERACTIVE"));
        assert!(g.contains("REASONING"));
    }

    #[test]
    fn selector_grammar_empty_without_refs() {
        assert!(selector_grammar(&[]).is_empty());
    }

    #[test]
    fn selector_grammar_constrains_to_index_tokens_and_escape() {
        use crate::perception::arbiter::{FusedElement, LabelSource, Sense};
        let fused = vec![
            FusedElement { ref_id: Some("ref_1".into()), bbox: (0, 0, 10, 10), sense: Sense::A11yOnly, patch_embd: None, label: None, label_source: LabelSource::None },
            FusedElement { ref_id: None, bbox: (50, 50, 10, 10), sense: Sense::VisionOnly, patch_embd: None, label: None, label_source: LabelSource::None },
        ];
        let g = selector_grammar(&fused);
        // every element's synthetic index is a valid target — including the vision-only one
        assert!(g.contains("\"el_0\""), "el_0 must be selectable");
        assert!(g.contains("\"el_1\""), "vision-only el_1 must be selectable (not dropped)");
        // the mandatory escape
        assert!(g.contains("\"none\""), "escape token must always be offered");
        // the interaction verbs
        for verb in ["root", "click", "type", "key", "wait", "done", "target"] {
            assert!(g.contains(verb), "grammar must define {verb}");
        }
        // it must NOT enumerate a token for a non-existent third element
        assert!(!g.contains("\"el_2\""), "no token beyond the candidate count");
    }
}
