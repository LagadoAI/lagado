//! grammar.rs — GBNF constraint generation for llama-server.
//!
//! When the model picks a screen element, constrain output to valid ref_ids.
//! Phase 1: stub interface. Phase 2: dynamic GBNF from perception output.

/// Generate a GBNF grammar that constrains the model to output
/// only one of the given selector ref_ids.
pub fn selector_grammar(ref_ids: &[String]) -> String {
    if ref_ids.is_empty() {
        return String::new();
    }
    // Phase 2: generate proper GBNF
    // For now return empty (no constraint) so existing inference is unaffected
    tracing::debug!("grammar: {} ref_ids available (stub, no constraint)", ref_ids.len());
    String::new()
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
}
