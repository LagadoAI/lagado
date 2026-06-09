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

/// Generate a GBNF grammar that forces a binary CHAT/INTERACTIVE/REASONING choice.
/// Used by hydra intent classifier to guarantee clean output.
pub fn intent_grammar() -> String {
    // Phase 2: return GBNF that forces one of these three tokens
    // r#"root ::= ("CHAT" | "INTERACTIVE" | "REASONING")"#.to_string()
    String::new() // stub
}
