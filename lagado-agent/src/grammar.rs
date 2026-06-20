//! grammar.rs — GBNF constraint generation for llama-server.
//!
//! When the model picks a screen element, constrain output to valid ref_ids.
//! Phase 1: stub interface. Phase 2: dynamic GBNF from perception output.

/// Generate a GBNF grammar over the rendered candidate set: a single interaction
/// bracket-call whose `selector` is constrained to one synthetic index token
/// (`el_0`, `el_1`, …) or the mandatory escape (`none`).
///
/// Takes the COUNT of candidates actually rendered into the prompt (`el_0..el_{n-1}`),
/// NOT the raw fused set — the candidate list is ranked and capped (`LATE_BAND_CAP`)
/// before it reaches the model, so the grammar must offer exactly the tokens that exist.
/// Keying on the per-frame INDEX (not `FusedElement.ref_id`, which is `None` for
/// CV/DOM/vision-only elements) names every element; the actuator resolves
/// `el_N` → bbox-center → coord click.
///
/// Scope: the GUI-interaction subset (`click`/`type`/`key`/`wait`/`done`). Whether
/// to also admit the 44 native/MCP `invoke` tools inside the constrained set is an
/// open integration decision (constraining them out would forbid them mid-task) —
/// deliberately NOT baked in here.
///
/// `none` (the escape) is always offered: a fusion miss becomes a recoverable
/// "none of these fit" signal, never a forced wrong click. `n == 0` → empty
/// grammar (no constraint), so callers fall back to unconstrained decoding.
pub fn selector_grammar(n: usize) -> String {
    if n == 0 {
        return String::new();
    }
    use std::fmt::Write;
    use crate::perception::selection::{index_token, ESCAPE_TOKEN};

    // target alternation: every rendered candidate's synthetic index + the escape.
    let mut targets: Vec<String> = (0..n)
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

/// GBNF for the file-ops CAPABILITY layer — constrains output to LFM2's native PYTHONIC call format
/// (`[move(source_dir="…", selector="*.pdf", dest="…")]`), well-formed by construction: valid verb, the
/// required params in order, enum-constrained `mode`/`filter`/`recursive`, and — the load-bearing part —
/// every SOURCE path slot bound to a path that actually appears in the current observe listing (the model
/// cannot emit an off-screen source). Dest/selector/new_name/content stay freeform (they may be new).
/// `observed` empty → source slots fall back to freeform (no path enumeration possible).
pub fn capability_grammar(observed: &[String]) -> String {
    use std::fmt::Write;
    // Alternation = the bound source vocabulary. ONLY GBNF-safe paths (no chars that could break a
    // string literal) — an odd path must NEVER silently invalidate the whole grammar and drop the
    // constraint (the failure that contaminated the 1.2B A/B). Unsafe paths are skipped, not escaped.
    let pathalt: String = observed.iter()
        .filter(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || "/._- ".contains(c)))
        .map(|p| format!("\"{p}\""))
        .collect::<Vec<_>>().join(" | ");
    // SOURCE slots bind to an EXACT observed path; DEST slots bind to an observed path OR a new child
    // under one (`<observed>/<seg>` — e.g. a new Scans/ folder or total.txt file). Binding dest too was
    // the fix for unbound-slot hallucination (gen2.5 emitted dest="documents" / "C:/Users/…").
    let srcq = if pathalt.is_empty() { "strq".to_string() } else { "src".to_string() };
    let dstq = if pathalt.is_empty() { "strq".to_string() } else { "dst".to_string() };

    let mut g = String::new();
    let _ = writeln!(g, "root ::= \"[\" call \"]\"");
    // INLINE alternatives — each `call` alternative MUST be terminal-leading. llama.cpp's GBNF SILENTLY
    // DROPS a grammar whose top alternation is bare rule references (`call ::= c_move | c_copy | …`) →
    // the constraint vanishes and the model runs free (this dead grammar made every prior capability run
    // unconstrained). Verified by bisection: terminal-leading alternatives enforce; bare references don't.
    let _ = writeln!(g, "{}", concat!(
        r#"call ::= "move(source_dir=" {S} ", selector=" strq ", dest=" {D} new_opt ")""#,
        r#" | "copy(source_dir=" {S} ", selector=" strq ", dest=" {D} rec_opt new_opt ")""#,
        r#" | "rename(path=" {S} ", new_name=" strq ")""#,
        r#" | "make_folder(path=" {D} ")""#,
        r#" | "write_file(path=" {D} content_opt ")""#,
        r#" | "delete(source_dir=" {S} ", selector=" strq filter_opt ")""#,
        r#" | "extract_to_file(mode=\"value\", source=" {S} ", pattern=" strq ", dest_file=" {D} ")""#,
        r#" | "extract_to_file(mode=\"count\", source_dir=" {S} ", selector=" strq ", dest_file=" {D} ")""#,
        r#" | "extract_to_file(mode=\"list\", source_dir=" {S} ", selector=" strq ", dest_file=" {D} ")""#,
    ).replace("{S}", &srcq).replace("{D}", &dstq));
    let _ = writeln!(g, r#"new_opt ::= ( ", new_name=" strq )?"#);
    let _ = writeln!(g, r#"rec_opt ::= ( ", recursive=true" )?"#);
    let _ = writeln!(g, r#"content_opt ::= ( ", content=" strq )?"#);
    let _ = writeln!(g, r#"filter_opt ::= ( ", filter=\"empty\"" | ", filter=\"larger_than_1k\"" )?"#);
    if !pathalt.is_empty() {
        let _ = writeln!(g, r#"dpath ::= {pathalt}"#);
        let _ = writeln!(g, r#"dst ::= "\"" dpath ( "/" seg )? "\"""#);
        let _ = writeln!(g, r#"seg ::= [A-Za-z0-9._/-]+"#);
        let _ = writeln!(g, r#"src ::= "\"" dpath "\"""#);
    }
    let _ = writeln!(g, r#"strq ::= "\"" [^"\\]* "\"""#);
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
        assert!(selector_grammar(0).is_empty());
    }

    #[test]
    fn selector_grammar_constrains_to_index_tokens_and_escape() {
        // Two rendered candidates → tokens el_0 and el_1 (a CV/vision-only element is
        // just another index; its inclusion is decided upstream in build_candidates).
        let g = selector_grammar(2);
        assert!(g.contains("\"el_0\""), "el_0 must be selectable");
        assert!(g.contains("\"el_1\""), "el_1 must be selectable (not dropped)");
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
