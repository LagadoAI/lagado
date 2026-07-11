"""battery_writer.py — the Writer plane's authoring + falsifier core, mirroring battery_calc.py's
role for Calc but scoped to a FIRST GENERATION: reason -> emit -> apply -> falsify -> ONE
feedback retry -> report. calc's accreted machinery (resample-at-divergence, compact-emit
dialect, multi-table regions, best-of-N static-defect redraws) encodes CALC-SPECIFIC failure
modes measured over weeks of real OSWorld runs; porting it here unmeasured would be fabricated
complexity, not earned rigor. This module is deliberately proportionate to what real Writer
tasks (docs/osworld read-only instruction survey, 2026-07-10) actually ask for.

INTEGRITY CONTRACT (same as battery_calc.py / calc_solve.py): the model sees ONLY the goal
instruction + the LIVE document's own structure (paragraph text/style/alignment/spacing) — never
a task id, never evaluator/gold knowledge. Falsifiers are SOUND: a fired falsifier PROVES a fault
(direct property read-back, e.g. "the goal asked for double spacing; paragraph 2 reads single");
an empty falsifier list means "no detected fault", NEVER "correct" — the harness under-claims
(exit 2, operated-but-unverified) rather than fabricate confidence. Per the advisor review this
module was built against (2026-07-10): Writer postconditions are mostly DIRECTLY OBSERVABLE
(unlike Calc's formula-correctness problem, which needed corroboration-by-agreement because the
right VALUE isn't computable from the goal alone) — so there is no corroborate() here; the
falsifiers themselves are the primary and ONLY verification signal. Genuinely headless-unreadable
postconditions (rendered PDF page count, a PageNumber field's computed digits) are left
UNVERIFIED on purpose rather than papered over with a fabricated check — see PAGE_UNVERIFIED_OPS.

ARCHITECTURAL SIMPLIFICATION vs Calc: Calc's op vocabulary addresses raw A1 cells, so
battery_calc.py owns a whole name->A1 resolution layer (resolve_col/resolve_ref) executed BEFORE
apply. Writer's ops address paragraphs via SCOPE DESCRIPTORS ("paragraph:2", "heading", "match:
...") that resolve_scope (writer_ops.py) already resolves LIVE, INSIDE the apply call, against
the document at that exact instant — no separate name-resolution layer is needed here. The
daemon's apply response surfaces a `matched` count (see writer_ops.py's op["_matched"]
convention) so a scope that resolved to nothing is still a visible, actionable fail.
"""

import json
import os
import re
import sys
import time

import requests

BRAIN = os.environ.get("LAGADO_BRAIN", "http://localhost:8080/completion")
CHAT = BRAIN.rsplit("/completion", 1)[0] + "/v1/chat/completions"
LOGDIR = "/tmp/lagado_battery_writer"

# Ops whose postcondition genuinely cannot be read back headless (no live view/layout pass) —
# applying one of these alone caps the run at exit 2 (operated, unverified), never a false exit 0.
UNVERIFIABLE_OPS = ("export_pdf", "insert_page_break")


# ── detection: paragraph candidates the model can refer to by scope ────────────────
def detect(g):
    """Live paragraph list + doc-level counts from the daemon's structure verb. Returns
    (paragraphs, n_tables, n_images) — paragraphs is the raw list of dicts the daemon returns
    (idx/text/style/align/ls_mode/ls_height/page_style)."""
    r = g.client("structure")
    if not r.get("ok"):
        return [], 0, 0
    return r.get("paragraphs", []), r.get("n_tables", 0), r.get("n_images", 0)


_LS_NAME = {100: "single", 150: "1.5", 200: "double"}


def candidate_cards(paragraphs, n_tables, n_images):
    lines = []
    for p in paragraphs:
        preview = p["text"][:80] + ("…" if len(p["text"]) > 80 else "")
        tags = []
        if "heading" in (p.get("style") or "").lower():
            tags.append("heading-style")
        if p.get("align") and p["align"] != "left":
            tags.append("align=%s" % p["align"])
        if p.get("ls_height") and p["ls_height"] != 100:
            tags.append("line-spacing=%s" % _LS_NAME.get(p["ls_height"], "%d%%" % p["ls_height"]))
        tagstr = (" [%s]" % ", ".join(tags)) if tags else ""
        lines.append("  paragraph %d%s: %r" % (p["idx"], tagstr, preview))
    lines.append("Document also contains %d table(s) and %d image(s)." % (n_tables, n_images))
    return "\n".join(lines)


# ── grammar / prompts ───────────────────────────────────────────────────────────────
GRAMMAR = (
    'root ::= "[" op ("," op)* "]"\n'
    'op ::= "find_replace(find=" str ", replace=" str ", match_case=" str ")"'
    ' | "set_paragraph_alignment(scope=" str ", align=" str ")"'
    ' | "set_line_spacing(scope=" str ", mode=" str ")"'
    ' | "set_tabstops(scope=" str ", stops=" str ")"'
    ' | "insert_tab(scope=" str ", after_word=" str ")"'
    ' | "format_chars(scope=" str ", match=" str ", within=" str ", bold=" str ", italic=" str "'
    ', underline=" str ", strike=" str ", subscript=" str ", superscript=" str ", font=" str "'
    ', size=" str ", color=" str ", highlight=" str ")"'
    ' | "format_text_where(predicate=" str ", bold=" str ", italic=" str ", underline=" str "'
    ', strike=" str ", font=" str ", size=" str ", color=" str ", highlight=" str ")"'
    ' | "set_case(scope=" str ", mode=" str ")"'
    ' | "insert_table(rows=" str ", cols=" str ", at=" str ")"'
    ' | "text_to_table(scope=" str ", delimiter=" str ")"'
    ' | "insert_image(path=" str ", at=" str ")"'
    ' | "insert_page_break(at=" str ")"'
    ' | "insert_page_number(position=" str ")"'
    ' | "split_paragraph_sentences(scope=" str ")"'
    ' | "dedup_lines(delimiter=" str ", field_index=" str ")"'
    ' | "append_text(text=" str ")"'
    ' | "set_default_font(name=" str ")"'
    ' | "export_pdf(name=" str ")"'
    ' | "infeasible(reason=" str ")"\n'
    'str ::= "\\"" [^"\\\\\\n\\r]* "\\""\n'
)

REASON_PROMPT = (
    "You are operating a word processor document.\n\n"
    "Goal: {instr}\n\n"
    "Paragraphs present (read from the document itself):\n{cards}\n\n"
    "Think step by step, then stop.")

EMIT_PROMPT = (
    "Goal: {instr}\n\n"
    "Paragraphs present:\n{cards}\n\n"
    "Your analysis:\n{reasoning}\n\n"
    "Now emit operations. A `scope` selects which paragraph(s) an operation applies to: "
    "\"all\" (every paragraph), \"paragraph:N\" (1-based index), \"paragraph:N-M\" (an inclusive "
    "range), \"first\", \"last\", \"heading\" (the title/heading paragraph), or \"match:<exact "
    "text>\" (every paragraph containing that text). Prefer \"match:\"/\"heading\"/\"last\" over a "
    "numeric index when an earlier operation may have added or split paragraphs above the target. "
    "Available operations:\n"
    "  find_replace(find=\"...\", replace=\"...\", match_case=\"true\")   replace all occurrences of text\n"
    "  set_paragraph_alignment(scope=\"...\", align=\"left|center|right|justify\")\n"
    "  set_line_spacing(scope=\"...\", mode=\"single|1.5|double\")\n"
    "  set_tabstops(scope=\"...\", stops=\"3in:left,6in:right\")   set custom tab stop positions/alignment on a paragraph\n"
    "  insert_tab(scope=\"...\", after_word=\"3\")   insert a literal TAB character right after the Nth word of the paragraph\n"
    "  format_chars(scope=\"...\", match=\"\", within=\"\", bold=\"\", italic=\"\", underline=\"\", strike=\"\", "
    "subscript=\"\", superscript=\"\", font=\"\", size=\"\", color=\"#rrggbb\", highlight=\"#rrggbb|none\")   style "
    "specific text: match=\"<exact text>\" targets every occurrence of that text (leave scope empty when using match); "
    "within=\"<substring>\" narrows styling to just that part of each match (e.g. one character inside a word); "
    "leave any style field \"\" to skip it; highlight=\"none\" removes highlighting\n"
    "  format_text_where(predicate=\"italic|bold|underline|highlighted|vowel_start|consonant_start\", bold=\"\", "
    "italic=\"\", underline=\"\", strike=\"\", font=\"\", size=\"\", color=\"#rrggbb\", highlight=\"#rrggbb|none\")   style "
    "EVERY run of text matching a condition across the whole document (vowel_start/consonant_start = the word's first letter)\n"
    "  set_case(scope=\"...\", mode=\"upper|lower|title|sentence\")   rewrite the text's letter case (title = capitalize "
    "the first letter of each word)\n"
    "  insert_table(rows=\"7\", cols=\"5\", at=\"cursor|end\")   insert a new empty table\n"
    "  text_to_table(scope=\"...\", delimiter=\",\")   convert delimiter-separated paragraph(s) into a real table\n"
    "  insert_image(path=\"/abs/path/to/file.png\", at=\"cursor|end\")   embed an image file\n"
    "  insert_page_break(at=\"cursor|end\")   start a new (blank) page\n"
    "  insert_page_number(position=\"footer-left|footer-center|footer-right|header-left|header-center|header-right\")\n"
    "  split_paragraph_sentences(scope=\"...\")   break a paragraph into one paragraph per sentence, with a blank "
    "line between each\n"
    "  dedup_lines(delimiter=\",\", field_index=\"1\")   remove paragraphs whose delimiter-separated field at "
    "field_index (0-based) repeats an earlier paragraph's value, keeping the first occurrence of each\n"
    "  append_text(text=\"...\")   add a new paragraph at the end of the document\n"
    "  set_default_font(name=\"...\")   change the document's DEFAULT font (new/unformatted text)\n"
    "  export_pdf(name=\"\")   export the current document as PDF next to it (name=\"\" keeps the document's own name)\n"
    "  infeasible(reason=\"...\")   ONLY if the request cannot be done in this application at all — emit it ALONE\n\n"
    "Emit ONLY the operations the goal needs, as a list of calls:")


def _chat(content, grammar=None, temperature=0.0, seed=7, max_tokens=800):
    body = {"messages": [{"role": "user", "content": content}], "temperature": temperature,
            "seed": seed, "max_tokens": max_tokens}
    if grammar:
        body["grammar"] = grammar
    r = requests.post(CHAT, json=body, timeout=200)
    return r.json()["choices"][0]["message"]["content"]


# ── parsing (verb(k="v", ...) calls -> op dicts) ────────────────────────────────────
def scan_calls(text, verbs):
    pat = re.compile(r"(%s)\(" % "|".join(verbs))
    for m in pat.finditer(text):
        verb = m.group(1)
        i = m.end()
        depth, in_q, esc, start = 1, False, False, m.end()
        while i < len(text) and depth > 0:
            c = text[i]
            if esc:
                esc = False
            elif c == "\\":
                esc = True
            elif c == '"':
                in_q = not in_q
            elif not in_q and c == "(":
                depth += 1
            elif not in_q and c == ")":
                depth -= 1
            i += 1
        yield verb, text[start:i - 1]


KV = re.compile(r'(\w+)="((?:[^"\\]|\\.)*)"')


def parse_kv(body):
    return {m.group(1): m.group(2).replace('\\"', '"') for m in KV.finditer(body)}


VERBS = ("find_replace", "set_paragraph_alignment", "set_line_spacing", "set_tabstops",
         "insert_tab", "format_chars", "format_text_where", "set_case", "insert_table",
         "text_to_table", "insert_image", "insert_page_break", "insert_page_number",
         "split_paragraph_sentences", "dedup_lines", "append_text", "set_default_font",
         "export_pdf", "infeasible")


def parse_ops(text):
    ops = []
    for verb, body in scan_calls(text, VERBS):
        kw = parse_kv(body)
        kw["op"] = verb
        ops.append(kw)
    return ops


# ── authoring ────────────────────────────────────────────────────────────────────────
def author(instr, cards, log, feedback=None, temperature=0.0, additive=False):
    seed = int(temperature * 1000) + 7
    reasoning = _chat(REASON_PROMPT.format(instr=instr, cards=cards),
                      temperature=temperature, seed=seed, max_tokens=400).strip()
    log.setdefault("reasoning", reasoning)
    emit = EMIT_PROMPT.format(instr=instr, cards=cards, reasoning=reasoning)
    if feedback:
        if additive:
            emit += ("\n\nYour PREVIOUS attempt was INCOMPLETE. Keep your operations exactly as "
                     "written AND ALSO emit the operations these notes ask for:\n%s" % feedback)
        else:
            emit += ("\n\nYour PREVIOUS attempt had these problems. Keep your operations EXACTLY as "
                     "written (same verbs, same targets) and change ONLY what these notes say:\n%s" % feedback)
    raw = _chat(emit, grammar=GRAMMAR, temperature=temperature, seed=seed, max_tokens=800)
    log.setdefault("emit_raw", []).append(raw)
    return parse_ops(raw)


# ── apply ────────────────────────────────────────────────────────────────────────────
def apply_ops(g, ops, log):
    """Apply each op through the daemon, in emitted order. Returns (applied, fails) — applied is
    the list of ops that made it through apply() without a transport/op error (each carrying the
    daemon's `matched` count when present); fails is a list of {op, why} dicts — a daemon-level
    apply error OR a scope/match that resolved to ZERO paragraphs (the writer_ops.py `_matched`
    diagnostic — the resolve-fails-before-apply signal calc's resolve_col gives, surfaced here
    AFTER apply instead, since Writer resolves scope live inside apply itself)."""
    applied, fails = [], []
    for op in ops:
        kind = op.get("op")
        if kind == "infeasible":
            applied.append(dict(op))
            continue
        payload = {k: v for k, v in op.items() if k != "op"}
        payload["op"] = kind
        r = g.client("apply", {"op": payload})
        if not r.get("ok"):
            fails.append({"op": kind, "why": "apply error: %s" % r.get("error", "")[:160]})
            log.setdefault("apply_errors", []).append([kind, r.get("error")])
            continue
        matched = r.get("matched")
        entry = dict(op)
        if matched is not None:
            entry["_matched"] = matched
        applied.append(entry)
        if matched == 0:
            fails.append({"op": kind, "why": "matched 0 paragraphs/occurrences for scope=%r match=%r"
                                             % (op.get("scope"), op.get("match"))})
    return applied, fails


# ── falsifiers (SOUND — detect faults, never confirm correctness) ──────────────────
def _resolve_scope_local(paragraphs, scope):
    """Mirror of writer_ops.resolve_scope's dialect, over the daemon's plain paragraph DICTS
    (not live UNO objects) — lets the battery re-check a scope's target paragraphs AFTER the
    fact, purely from a fresh structure() read, without needing the daemon to echo back which
    indices it touched for every op kind."""
    s = (scope or "all").strip()
    if s in ("", "all"):
        return list(paragraphs)
    if s == "first":
        return paragraphs[:1]
    if s == "last":
        return paragraphs[-1:]
    if s == "heading":
        for p in paragraphs:
            if "heading" in (p.get("style") or "").lower():
                return [p]
        return paragraphs[:1]
    m = re.match(r"paragraph:(\d+)(?:-(\d+))?$", s)
    if m:
        i0, i1 = int(m.group(1)), int(m.group(2)) if m.group(2) else int(m.group(1))
        return [p for p in paragraphs if i0 <= p["idx"] <= i1]
    m2 = re.match(r"match:(.*)$", s, re.S)
    if m2:
        needle = m2.group(1)
        return [p for p in paragraphs if needle and needle in p["text"]]
    return []


_ALIGN_WORDS = {"left": "left", "right": "right", "center": "center", "centre": "center",
                "justify": "justify", "justified": "justify", "block": "justify"}
_LS_HEIGHT = {"single": 100, "1": 100, "1.0": 100, "1.5": 150, "onehalf": 150,
              "one-and-a-half": 150, "double": 200, "2": 200, "2.0": 200}


def falsify(g, applied, instr):
    """Read the live doc back and check each applied op's OWN stated postcondition. Returns fired
    fault dicts. Sound by construction: every check reads a property/text the op claimed to set
    and compares it to what the op asked for — it can only detect a mismatch, never assert one
    that isn't there."""
    fired = []
    paragraphs, n_tables, n_images = detect(g)
    for op in applied:
        kind = op.get("op")
        if kind == "find_replace":
            find = op.get("find") or ""
            if not find:
                continue
            r = g.client("read", {"what": "match", "match": find})
            if r.get("ok") and r.get("count", 0) > 0:
                fired.append({"falsifier": "find_replace_incomplete",
                              "why": "%r still appears %d time(s) after find_replace"
                                     % (find, r["count"])})
        elif kind == "set_paragraph_alignment":
            want = _ALIGN_WORDS.get(str(op.get("align") or "left").strip().lower(), "left")
            for p in _resolve_scope_local(paragraphs, op.get("scope")):
                if p.get("align") != want:
                    fired.append({"falsifier": "alignment_mismatch",
                                  "why": "paragraph %d asked for align=%s but reads align=%s"
                                         % (p["idx"], want, p.get("align"))})
        elif kind == "set_line_spacing":
            mode = str(op.get("mode") or "single").strip().lower()
            want = _LS_HEIGHT.get(mode)
            if want is None:
                m = re.match(r"([\d.]+)\s*%?$", mode)
                want = int(round(float(m.group(1)))) if m else 100
            for p in _resolve_scope_local(paragraphs, op.get("scope")):
                if p.get("ls_height") != want:
                    fired.append({"falsifier": "line_spacing_mismatch",
                                  "why": "paragraph %d asked for line-spacing=%s but reads %s%%"
                                         % (p["idx"], mode, p.get("ls_height"))})
        elif kind == "set_case":
            mode = str(op.get("mode") or "").strip().lower()
            for p in _resolve_scope_local(paragraphs, op.get("scope")):
                s = p.get("text") or ""
                ok = True
                if mode == "upper":
                    ok = s == s.upper()
                elif mode == "lower":
                    ok = s == s.lower()
                elif mode in ("title", "titlecase"):
                    ok = all(w[:1].isupper() for w in re.findall(r"[A-Za-z]+", s))
                if not ok:
                    fired.append({"falsifier": "case_mismatch",
                                  "why": "paragraph %d asked for case=%s but text is %r"
                                         % (p["idx"], mode, s[:60])})
        elif kind == "format_chars":
            match = op.get("match") or ""
            if not match:
                continue
            r = g.client("read", {"what": "match", "match": match})
            if not r.get("ok") or r.get("count", 0) == 0:
                fired.append({"falsifier": "format_target_absent",
                              "why": "text %r is no longer present to have been styled" % match})
                continue
            pidx = next((i for i in r.get("paragraphs", []) if i), None)
            if pidx:
                pr = g.client("read", {"what": "portions", "index": pidx})
                if pr.get("ok"):
                    fired.extend(_check_char_props(pr.get("portions", []), match, op.get("within"), op))
        elif kind == "insert_page_number":
            pos = str(op.get("position") or "footer-left").strip().lower()
            area = "header" if pos.startswith("header") else "footer"
            r = g.client("read", {"what": "page_areas"})
            if r.get("ok") and not r.get("%s_has_page_field" % area):
                fired.append({"falsifier": "page_number_missing",
                              "why": "no page-number field found in the %s after insert_page_number" % area})
        elif kind == "set_default_font":
            want = str(op.get("name") or "").strip()
            r = g.client("read", {"what": "default_font"})
            if want and r.get("ok") and (r.get("font") or "").strip().lower() != want.lower():
                fired.append({"falsifier": "default_font_mismatch",
                              "why": "default font reads %r, not the requested %r" % (r.get("font"), want)})
        elif kind == "insert_table":
            if n_tables < 1:
                fired.append({"falsifier": "table_missing", "why": "no table found after insert_table"})
        elif kind == "text_to_table":
            if n_tables < 1:
                fired.append({"falsifier": "table_missing", "why": "no table found after text_to_table"})
        elif kind == "insert_image":
            if n_images < 1:
                fired.append({"falsifier": "image_missing", "why": "no image found after insert_image"})
        elif kind == "dedup_lines":
            delim = str(op.get("delimiter") or ",")
            field_idx = int(float(op.get("field_index") or 0))
            seen = set()
            for p in paragraphs:
                parts = [x.strip() for x in (p.get("text") or "").split(delim)]
                key = parts[field_idx] if field_idx < len(parts) else p.get("text")
                if not key:
                    continue
                if key in seen:
                    fired.append({"falsifier": "duplicate_remains",
                                  "why": "paragraph %d still duplicates an earlier field value %r"
                                         % (p["idx"], key)})
                seen.add(key)
    return fired


def _check_char_props(portions, match, within, op):
    """Given the portions of the paragraph a format_chars(match=...) op targeted, find the run(s)
    covering `match` (or `within` inside it) and verify the requested properties actually landed.
    Sound: reads the SAME properties the op claimed to set and compares against what was asked."""
    fired = []
    needle = within or match
    target = next((p for p in portions if needle and needle in (p.get("text") or "")), None)
    if target is None:
        return [{"falsifier": "format_target_absent",
                "why": "no run in the read-back paragraph contains %r" % needle}]
    checks = (
        ("bold", lambda o: str(o.get("bold", "")).strip().lower() in ("1", "true", "yes", "bold"),
         lambda t: t.get("bold")),
        ("italic", lambda o: str(o.get("italic", "")).strip().lower() in ("1", "true", "yes", "italic"),
         lambda t: t.get("italic")),
        ("underline", lambda o: str(o.get("underline", "")).strip().lower() not in ("", "0", "false", "no"),
         lambda t: t.get("underline")),
        ("strike", lambda o: str(o.get("strike", "")).strip().lower() not in ("", "0", "false", "no"),
         lambda t: t.get("strike")),
    )
    for field, want_fn, have_fn in checks:
        raw = op.get(field, "")
        if raw == "":
            continue
        if bool(want_fn(op)) != bool(have_fn(target)):
            fired.append({"falsifier": "char_property_mismatch",
                          "why": "%s=%r requested for %r but read-back shows %s=%s"
                                 % (field, raw, needle, field, have_fn(target))})
    sub = str(op.get("subscript", "")).strip().lower()
    if sub in ("1", "true", "yes") and target.get("escapement", 0) >= 0:
        fired.append({"falsifier": "char_property_mismatch",
                      "why": "subscript requested for %r but read-back escapement is %s (not negative)"
                             % (needle, target.get("escapement"))})
    sup = str(op.get("superscript", "")).strip().lower()
    if sup in ("1", "true", "yes") and target.get("escapement", 0) <= 0:
        fired.append({"falsifier": "char_property_mismatch",
                      "why": "superscript requested for %r but read-back escapement is %s (not positive)"
                             % (needle, target.get("escapement"))})
    color = (op.get("color") or "").strip().lstrip("#")
    if color and target.get("color") != int(color, 16):
        fired.append({"falsifier": "char_property_mismatch",
                      "why": "color #%s requested for %r but read-back color is %s"
                             % (color, needle, hex(target.get("color", 0)))})
    hi = (op.get("highlight") or "").strip()
    if hi:
        want_hi = -1 if hi.lower() in ("none", "off", "remove", "clear") else int(hi.lstrip("#"), 16)
        if target.get("highlight") != want_hi:
            fired.append({"falsifier": "char_property_mismatch",
                          "why": "highlight=%r requested for %r but read-back highlight is %s"
                                 % (hi, needle, target.get("highlight"))})
    return fired


# ── reasoning->emission completeness gap check (lightweight, goal-grounded) ────────
def emit_gaps(reasoning, ops, instr=""):
    """Hold the model to its OWN reasoning (the same discipline as calc's emit_gaps): a narrow,
    word-gated set of checks for an action the model's analysis clearly commits to but the
    emission never covers. NOT leading — every check fires only on the model's own stated words
    plus the absence of the matching op kind."""
    r = (reasoning or "").lower()
    gaps = []
    if any(o.get("op") == "infeasible" for o in ops):
        return gaps
    checks = (
        ("table", r"\btable\b", ("insert_table", "text_to_table")),
        ("page_break", r"\bnew page\b|\bpage break\b|\bblank page\b", ("insert_page_break",)),
        ("page_number", r"\bpage number\b", ("insert_page_number",)),
        ("font", r"\bfont\b", ("format_chars", "format_text_where", "set_default_font")),
        ("case", r"\buppercase\b|\blowercase\b|\bcapitali[sz]e\b", ("set_case",)),
        ("highlight", r"\bhighlight", ("format_chars", "format_text_where")),
        ("spacing", r"\bline spacing\b|\bdouble.space\b|\bsingle.space\b", ("set_line_spacing",)),
        ("image", r"\bimage\b|\bpicture\b|\bscreenshot\b", ("insert_image",)),
    )
    for tag, pat, kinds in checks:
        if re.search(pat, r) and not any(o.get("op") in kinds for o in ops):
            gaps.append(tag)
    return gaps


def gap_feedback(gaps):
    lines = []
    hints = {
        "table": "your analysis describes a TABLE but you did not emit insert_table(...) or text_to_table(...).",
        "page_break": "your analysis describes a new/blank PAGE but you did not emit insert_page_break(...).",
        "page_number": "your analysis describes a PAGE NUMBER but you did not emit insert_page_number(...).",
        "font": "your analysis describes a FONT change but you did not emit format_chars(...)/set_default_font(...).",
        "case": "your analysis describes a CASE change but you did not emit set_case(...).",
        "highlight": "your analysis describes HIGHLIGHTING but you did not emit a format op with highlight=....",
        "spacing": "your analysis describes LINE SPACING but you did not emit set_line_spacing(...).",
        "image": "your analysis describes an IMAGE but you did not emit insert_image(...).",
    }
    for g in gaps:
        if g in hints:
            lines.append("- %s Keep your other operations and ALSO emit it." % hints[g])
    return "\n".join(lines)


def compose_feedback(fails, fired):
    lines = []
    for f in fails:
        lines.append("- %s" % f.get("why", ""))
    for f in fired:
        lines.append("- %s" % f.get("why", ""))
    return "\n".join(lines)


# ── the shared model->emit->apply->falsify->retry body ─────────────────────────────
def run_core(g, task, file_path, log):
    """Analogous to battery_calc.py's run_core, without a score_fn — writer_solve.py is the
    task-blind caller and owns exit-code translation; this function only reports what the
    harness itself observed. Returns log (mutated) with:
      log["self_report_done"]  bool  — ops applied, no fault detected, nothing unverifiable touched
      log["declared_infeasible"]  str|None
      log["n_ops"], log["falsifiers_fired"], log["unverifiable"]  bool
    """
    r = g.client("open", {"file": file_path})
    if not r.get("ok"):
        log["fatal"] = "open failed: %s" % r.get("error")
        return log
    log["steps"] = ["open"]

    instr = task["instruction"]
    paragraphs, n_tables, n_images = detect(g)
    cards = candidate_cards(paragraphs, n_tables, n_images)
    log["detected_paragraphs"] = len(paragraphs)

    feedback, additive = None, False
    applied, fails, fired, gaps = [], [], [], []
    for attempt in range(2):
        log["steps"].append("attempt%d" % attempt)
        ops = author(instr, cards, log, feedback, additive=additive)
        if len(ops) == 1 and ops[0].get("op") == "infeasible":
            log["declared_infeasible"] = ops[0].get("reason", "")
            log["n_ops"] = 0
            break
        ops = [o for o in ops if o.get("op") != "infeasible"]
        gaps = emit_gaps(log.get("reasoning", ""), ops, instr)
        applied, fails = apply_ops(g, ops, log)
        log["n_ops"] = len([o for o in applied if o.get("op") != "infeasible"])
        fired = falsify(g, applied, instr)
        if applied and not fails and not fired and not gaps:
            log["steps"].append("clean")
            break
        feedback = (compose_feedback(fails, fired) + "\n" + gap_feedback(gaps)).strip()
        additive = bool(gaps)
    log["attempts"] = attempt + 1
    log["applied"] = applied
    log["falsifiers_fired"] = fired
    log["resolve_fails"] = fails

    if "declared_infeasible" not in log:
        no_fault = bool(applied) and not fails and not fired and not gaps
        has_unverifiable = any(o.get("op") in UNVERIFIABLE_OPS for o in applied)
        log["unverifiable"] = has_unverifiable
        log["self_report_done"] = no_fault and not has_unverifiable
    else:
        log["self_report_done"] = True   # mirrors calc's infeasible-declaration bookkeeping

    g.client("reconcile", {"gui": not log.get("host")})
    if os.environ.get("LAGADO_VISIBLE"):
        time.sleep(int(os.environ.get("LAGADO_VISIBLE_HOLD", "10")))
    g.client("close")
    time.sleep(4)
    return log


# ── self-test (no live guest/model connection required) ────────────────────────────
if __name__ == "__main__":
    import sys as _sys

    failures = []

    def _check(label, got, want):
        if got != want:
            failures.append("%s: got %r, want %r" % (label, got, want))

    # scan_calls/parse_kv/parse_ops round-trip on a hand-built emission string, covering
    # nested-quote escaping and multiple call kinds in one blob (the real GRAMMAR shape).
    sample = ('[find_replace(find="World", replace="Writer", match_case="true"),'
             ' set_paragraph_alignment(scope="heading", align="center"),'
             ' format_chars(scope="", match="H2O", within="2", bold="", italic="", underline="",'
             ' strike="", subscript="true", superscript="", font="", size="", color="", highlight=""),'
             ' infeasible(reason="real-time co-editing is not observable in this sandbox")]')
    ops = parse_ops(sample)
    _check("parsed op count", len(ops), 4)
    _check("op0 kind", ops[0]["op"], "find_replace")
    _check("op0 find", ops[0]["find"], "World")
    _check("op1 scope", ops[1]["scope"], "heading")
    _check("op2 within", ops[2]["within"], "2")
    _check("op2 subscript", ops[2]["subscript"], "true")
    _check("op3 kind", ops[3]["op"], "infeasible")
    _check("op3 reason", ops[3]["reason"], "real-time co-editing is not observable in this sandbox")

    # a quoted-escape edge case: an embedded escaped quote inside a value.
    esc_ops = parse_ops(r'[append_text(text="she said \"hi\" today")]')
    _check("escaped-quote round-trip", esc_ops[0]["text"], 'she said "hi" today')

    # candidate_cards renders every paragraph + doc-level counts, no live guest needed.
    paras = [{"idx": 1, "text": "Title", "style": "Heading 1", "align": "center", "ls_mode": 0, "ls_height": 100},
            {"idx": 2, "text": "Body paragraph text here.", "style": "Standard", "align": "left",
             "ls_mode": 0, "ls_height": 200}]
    cards = candidate_cards(paras, n_tables=1, n_images=0)
    _check("cards mentions paragraph 1", "paragraph 1" in cards, True)
    _check("cards tags heading style", "heading-style" in cards, True)
    _check("cards tags line-spacing", "line-spacing=double" in cards, True)
    _check("cards mentions table count", "1 table(s)" in cards, True)

    # _resolve_scope_local mirrors writer_ops.resolve_scope's dialect over plain dicts (the
    # falsifier-side re-check path — exercised directly here, no live doc).
    _check("local scope all", len(_resolve_scope_local(paras, "all")), 2)
    _check("local scope paragraph:2", _resolve_scope_local(paras, "paragraph:2")[0]["idx"], 2)
    _check("local scope heading", _resolve_scope_local(paras, "heading")[0]["idx"], 1)
    _check("local scope match", _resolve_scope_local(paras, "match:Body")[0]["idx"], 2)
    _check("local scope garbage -> empty", _resolve_scope_local(paras, "garbage!!"), [])

    # emit_gaps/gap_feedback: the model's OWN reasoning commits to an action the emission drops.
    reasoning = "I will insert a table to hold this data, then also add a page number at the bottom."
    gaps = emit_gaps(reasoning, [{"op": "append_text", "text": "x"}], instr="")
    _check("gap: table detected", "table" in gaps, True)
    _check("gap: page_number detected", "page_number" in gaps, True)
    fb = gap_feedback(gaps)
    _check("gap feedback mentions TABLE", "TABLE" in fb, True)
    # a covered action must NOT gap-nag (sound in the other direction too).
    gaps2 = emit_gaps(reasoning, [{"op": "insert_table"}, {"op": "insert_page_number"}], instr="")
    _check("no gaps once covered", gaps2, [])

    # _check_char_props: sound char-formatting falsifier on canned portion read-back.
    portions_ok = [{"text": "H", "bold": False, "italic": False, "underline": False, "strike": False,
                   "color": -1, "highlight": -1, "escapement": 0},
                  {"text": "2", "bold": False, "italic": False, "underline": False, "strike": False,
                   "color": -1, "highlight": -1, "escapement": -14000}]
    fired_ok = _check_char_props(portions_ok, "H2O", "2", {"subscript": "true"})
    _check("subscript matches -> no falsifier fired", fired_ok, [])
    portions_bad = [dict(portions_ok[1], escapement=0)]   # subscript was asked for but never applied
    fired_bad = _check_char_props(portions_bad, "2", "2", {"subscript": "true"})
    _check("subscript missing -> falsifier fires", len(fired_bad) >= 1, True)

    if failures:
        print("FAILED (%d):" % len(failures))
        for f in failures:
            print(" -", f)
        _sys.exit(1)
    print("battery_writer.py self-test: all checks passed (no live guest/model required)")
