"""writer_ops.py — Shared UNO Writer doc-mutation primitives — the SINGLE apply implementation
for the Writer plane, mirroring uno_ops.py's role for Calc.

Pure document logic: given a connected, loaded Writer `doc` (com.sun.star.text.TextDocument),
apply one op to the LIVE in-memory model. Owns NO process lifecycle (no soffice launch/kill,
no store, no reload) — that belongs to the caller (writer_daemon.py, the resident session).

FIRST-GENERATION NOTE: unlike uno_ops.py (which accreted its op set and constants over weeks of
measured OSWorld runs), this module's constants were verified directly against a local headless
soffice (probe scripts, 2026-07-10) rather than against real task runs — enum values/property
names/round-trip behavior are CHECKED FACTS, but the op vocabulary itself is unvalidated against
the real evaluator. Treat gotchas noted inline as first-pass engineering judgment, not measured law.

DOCUMENT MODEL: Writer has no Sheets/Cells; the unit of address is the PARAGRAPH (an element of
doc.Text's enumeration) and, within it, character RUNS (TextPortions) and PROPERTIES set via a
text cursor. There is no structural "container must exist before content" ordering the way
add_sheet precedes fill — every op here operates against paragraphs/text that already exist in
the document, so no order_ops/apply_op_log analog is needed (the daemon applies ops one at a
time, each resolving SCOPE against the then-current live doc — see resolve_scope's docstring).

SCOPE (the addressing dialect every paragraph/character op shares, in an op's "scope" field):
  "all"                 every paragraph
  "first" / "last"      the first/last paragraph
  "heading"             the first paragraph whose style name contains "Heading" (falls back to
                         the first paragraph — a title is usually paragraph 1 even if unstyled)
  "paragraph:N"         1-based paragraph index N
  "paragraph:N-M"       1-based inclusive paragraph range
  "match:<text>"        every paragraph whose text contains <text> (verbatim substring)
An empty/absent scope defaults to "all". Unrecognized scope text resolves to an EMPTY list (fail
closed — never silently operate on the wrong paragraph).
"""

import re

import uno


# ── paragraph enumeration / scope resolution ──────────────────────────────────────
def _paragraphs(doc):
    """Every top-level PARAGRAPH element of the document body (tables are separate elements in
    the same enumeration and are skipped here — no op in this module targets table cells)."""
    out = []
    it = doc.Text.createEnumeration()
    while it.hasMoreElements():
        el = it.nextElement()
        if el.supportsService("com.sun.star.text.Paragraph"):
            out.append(el)
    return out


def resolve_scope(doc, scope):
    """scope string -> list of live paragraph objects. Resolved AGAINST THE CURRENT DOC EVERY
    CALL (never cached) — a paragraph index or match is only ever evaluated at the instant an op
    applies, so an earlier op in the same op-log that split/inserted/deleted paragraphs is
    already reflected. This is the paragraph-index-instability defense: resolution happens here,
    live, per-op — never pre-computed by a caller against a stale snapshot."""
    paras = _paragraphs(doc)
    s = (scope or "all").strip()
    if s in ("", "all"):
        return paras
    if s == "first":
        return paras[:1]
    if s == "last":
        return paras[-1:]
    if s == "heading":
        for p in paras:
            if "heading" in (p.ParaStyleName or "").lower():
                return [p]
        return paras[:1]
    m = re.match(r"paragraph:(\d+)(?:-(\d+))?$", s)
    if m:
        i0 = int(m.group(1))
        i1 = int(m.group(2)) if m.group(2) else i0
        return [p for idx, p in enumerate(paras, start=1) if i0 <= idx <= i1]
    m2 = re.match(r"match:(.*)$", s, re.S)
    if m2:
        needle = m2.group(1)
        return [p for p in paras if needle and needle in p.getString()]
    return []


# ── measurement / enum helpers ─────────────────────────────────────────────────────
def _to_1_100mm(s):
    """'3in' / '2.5cm' / '10mm' / a bare number (assumed inches) -> 1/100 mm (UNO's Position unit
    for TabStop). VERIFIED conversion factors: 1in=2540, 1cm=1000, 1mm=100."""
    s = (s or "").strip().lower()
    m = re.match(r"([\d.]+)\s*(in|cm|mm)?$", s)
    if not m:
        return 0
    val = float(m.group(1))
    unit = m.group(2) or "in"
    return int(round(val * {"in": 2540, "cm": 1000, "mm": 100}[unit]))


def _enum_val(v):
    """Normalize a live property read that may come back as a plain int OR a uno.Enum instance
    (MEASURED, 2026-07-10: para.ParaAdjust reads back as plain int; portion.CharPosture reads
    back as a uno.Enum with a .value string — pyuno is inconsistent about which). Returns the
    Enum's .value string if present, else the raw value unchanged."""
    return getattr(v, "value", v)


def _truthy(s):
    return str(s or "").strip().lower() in ("1", "true", "yes", "on")


# ── character-property application (shared by format_chars / format_text_where) ────
def _apply_char_props(target, op):
    """Set whatever character properties `op` specifies onto `target` (a text cursor OR a
    TextPortion — both are valid property-set ranges in UNO). Absent/empty fields are left
    untouched (same "" = skip idiom as calc's format_cells). VERIFIED constants (probe,
    2026-07-10): CharWeight BOLD=150.0/NORMAL=100.0 (plain floats, not an enum — matches
    uno_ops.py's own convention); CharUnderline/CharStrikeout are short constants, SINGLE=1/
    NONE=0. SUBSCRIPT/SUPERSCRIPT GOTCHA (probe-measured, 2026-07-10): the textbook LO-Basic
    figure of CharEscapement=-33/+33 (percent) with CharEscapementHeight=58 round-trips a saved
    .docx as a manual <w:position>/<w:sz> nudge, NOT real subscript/superscript — verified by
    diffing against what the ACTUAL .uno:SubScript/.uno:SuperScript toolbar dispatch leaves on
    the cursor: CharEscapement=-14000/+14000 (Height still 58). Only THAT value round-trips as
    the genuine <w:vertAlign w:val="subscript|superscript"/> element, the one python-docx's
    `run.font.subscript`/`.superscript` reads — this LO build's CharEscapement scale for the
    named preset is NOT the plain percentage the IDL docs imply, and getting it wrong produces a
    document that LOOKS right (renders shifted/small) but FALSIFIES on that exact property read.
    CharColor/CharHighlight are plain RGB ints, -1 = automatic/none (CharHighlight -1 verified to
    clear a saved docx's <w:highlight> — the property python-docx's run.font.highlight_color
    reads, NOT CharBackColor, which is a different property serializing to <w:shd> shading)."""
    b = str(op.get("bold", "")).strip().lower()
    if b:
        target.CharWeight = 150.0 if _truthy(b) or b == "bold" else 100.0
    i = str(op.get("italic", "")).strip().lower()
    if i:
        target.CharPosture = uno.Enum("com.sun.star.awt.FontSlant",
                                       "ITALIC" if _truthy(i) or i == "italic" else "NONE")
    u = str(op.get("underline", "")).strip().lower()
    if u:
        target.CharUnderline = 1 if _truthy(u) or u in ("underline", "single") else 0
    st = str(op.get("strike", "")).strip().lower()
    if st:
        target.CharStrikeout = 1 if _truthy(st) or st.startswith("strike") else 0
    sub = str(op.get("subscript", "")).strip().lower()
    sup = str(op.get("superscript", "")).strip().lower()
    if sub and _truthy(sub):
        target.CharEscapement = -14000    # verified magic value — see docstring gotcha above
        target.CharEscapementHeight = 58
    elif sup and _truthy(sup):
        target.CharEscapement = 14000
        target.CharEscapementHeight = 58
    elif (sub and not _truthy(sub)) or (sup and not _truthy(sup)):
        target.CharEscapement = 0
        target.CharEscapementHeight = 100
    font = str(op.get("font", "")).strip()
    if font:
        target.CharFontName = font
    size = str(op.get("size", "")).strip()
    if size:
        target.CharHeight = float(size)
    color = (op.get("color") or "").strip()
    if color:
        target.CharColor = int(color.lstrip("#"), 16)
    hi = (op.get("highlight") or "").strip()
    if hi:
        target.CharHighlight = -1 if hi.lower() in ("none", "off", "remove", "clear") \
            else int(hi.lstrip("#"), 16)


def _para_cursor(doc, para):
    cur = doc.Text.createTextCursorByRange(para.Start)
    cur.gotoEndOfParagraph(True)
    return cur


# ── the single apply implementation ────────────────────────────────────────────────
def apply_writer_op(doc, op):
    """Apply ONE op to the live Writer doc. Returns None on success; raises on a malformed op
    (mirrors uno_ops.apply_one_op — the caller/daemon decides whether a raise wedges the session
    or is just dropped from the log). `infeasible` is never dispatched here — the authoring layer
    intercepts a sole infeasible() declaration before anything reaches apply (same discipline as
    calc's run_core)."""
    kind = op.get("op")
    text = doc.Text

    if kind == "find_replace":
        rd = doc.createReplaceDescriptor()
        rd.SearchString = str(op.get("find") or "")
        rd.ReplaceString = str(op.get("replace") or "")
        rd.SearchCaseSensitive = _truthy(op.get("match_case", "true")) if "match_case" in op else True
        op["_matched"] = doc.replaceAll(rd)

    elif kind == "set_paragraph_alignment":
        name = {"left": "LEFT", "right": "RIGHT", "center": "CENTER", "centre": "CENTER",
                "justify": "BLOCK", "justified": "BLOCK", "block": "BLOCK"}.get(
            str(op.get("align") or "left").strip().lower(), "LEFT")
        adj = uno.Enum("com.sun.star.style.ParagraphAdjust", name)
        scoped = resolve_scope(doc, op.get("scope"))
        op["_matched"] = len(scoped)
        for para in scoped:
            para.ParaAdjust = adj

    elif kind == "set_line_spacing":
        mode = str(op.get("mode") or "single").strip().lower()
        height = {"single": 100, "1": 100, "1.0": 100, "1.5": 150, "onehalf": 150,
                  "one-and-a-half": 150, "double": 200, "2": 200, "2.0": 200}.get(mode)
        if height is None:
            m = re.match(r"([\d.]+)\s*%?$", mode)
            height = int(round(float(m.group(1)))) if m else 100
        scoped = resolve_scope(doc, op.get("scope"))
        op["_matched"] = len(scoped)
        for para in scoped:
            ls = para.ParaLineSpacing
            ls.Mode = 0   # com.sun.star.style.LineSpacingMode.PROP (percent-of-single) — verified default
            ls.Height = height
            para.ParaLineSpacing = ls

    elif kind == "set_tabstops":
        stops = []
        for tok in str(op.get("stops") or "").split(","):
            tok = tok.strip()
            if not tok:
                continue
            pos_s, _, al_s = tok.partition(":")
            ts = uno.createUnoStruct("com.sun.star.style.TabStop")
            ts.Position = _to_1_100mm(pos_s)
            ts.Alignment = uno.Enum("com.sun.star.style.TabAlign",
                                    {"left": "LEFT", "right": "RIGHT", "center": "CENTER",
                                     "decimal": "DECIMAL"}.get((al_s or "left").strip().lower(), "LEFT"))
            stops.append(ts)
        scoped = resolve_scope(doc, op.get("scope"))
        op["_matched"] = len(scoped)
        for para in scoped:
            para.ParaTabStops = tuple(stops)

    elif kind == "insert_tab":
        n = int(float(op.get("after_word") or 1))
        inserted = 0
        for para in resolve_scope(doc, op.get("scope")):
            words = list(re.finditer(r"\S+", para.getString()))
            if n > len(words):
                continue
            cur = doc.Text.createTextCursorByRange(para.Start)
            cur.goRight(words[n - 1].end(), False)
            text.insertString(cur, "\t", False)
            inserted += 1
        op["_matched"] = inserted

    elif kind == "format_chars":
        scope = op.get("scope") or "all"
        match = str(op.get("match") or "")
        within = str(op.get("within") or "")
        targets = []
        if match:
            sd = doc.createSearchDescriptor()
            sd.SearchString = match
            sd.SearchCaseSensitive = True
            found = doc.findAll(sd)
            for i in range(found.getCount()):
                rng = found.getByIndex(i)
                if within:
                    base = rng.getString()
                    off = base.find(within)
                    if off < 0:
                        continue
                    c = doc.Text.createTextCursorByRange(rng.Start)
                    c.goRight(off, False)
                    c.goRight(len(within), True)
                    targets.append(c)
                else:
                    targets.append(doc.Text.createTextCursorByRange(rng))
        else:
            targets = [_para_cursor(doc, para) for para in resolve_scope(doc, scope)]
        op["_matched"] = len(targets)
        for t in targets:
            _apply_char_props(t, op)

    elif kind == "format_text_where":
        # Predicate-scanned styling (calc's format_cells_where analog): the model states a
        # CONDITION over content the harness scans (it never enumerates the matching runs
        # itself). "italic"/"bold"/"underline"/"highlighted" match existing character
        # formatting; "vowel_start"/"consonant_start" match the first letter of each word.
        pred = str(op.get("predicate") or "").strip().lower()
        vowels = set("aeiouAEIOU")
        matched = 0
        if pred in ("italic", "bold", "underline", "highlighted"):
            for para in _paragraphs(doc):
                pit = para.createEnumeration()
                while pit.hasMoreElements():
                    portion = pit.nextElement()
                    if not portion.getString():
                        continue
                    hit = False
                    if pred == "italic":
                        hit = _enum_val(portion.CharPosture) == "ITALIC"
                    elif pred == "bold":
                        hit = float(portion.CharWeight) >= 150.0
                    elif pred == "underline":
                        hit = int(portion.CharUnderline) != 0
                    elif pred == "highlighted":
                        hit = int(portion.CharHighlight) != -1
                    if hit:
                        _apply_char_props(portion, op)
                        matched += 1
        elif pred in ("vowel_start", "consonant_start"):
            want_vowel = pred == "vowel_start"
            for para in _paragraphs(doc):
                s = para.getString()
                for m in re.finditer(r"[A-Za-z]+", s):
                    w = m.group(0)
                    if (w[0] in vowels) != want_vowel:
                        continue
                    cur = doc.Text.createTextCursorByRange(para.Start)
                    cur.goRight(m.start(), False)
                    cur.goRight(len(w), True)
                    _apply_char_props(cur, op)
                    matched += 1
        op["_matched"] = matched

    elif kind == "set_case":
        mode = str(op.get("mode") or "").strip().lower()
        scoped = resolve_scope(doc, op.get("scope"))
        op["_matched"] = len(scoped)
        for para in scoped:
            s = para.getString()
            if mode == "upper":
                new = s.upper()
            elif mode == "lower":
                new = s.lower()
            elif mode in ("title", "titlecase"):
                # Word-level, MINIMAL edit: touch only each word's first letter (preserves
                # per-run formatting elsewhere in the paragraph — a whole-paragraph setString
                # would clobber it, see set_case's docstring note in battery_writer.py).
                cur = doc.Text.createTextCursorByRange(para.Start)
                for m in re.finditer(r"\b[a-zA-Z]", s):
                    if m.group(0).islower():
                        c = doc.Text.createTextCursorByRange(para.Start)
                        c.goRight(m.start(), False)
                        c.goRight(1, True)
                        c.setString(m.group(0).upper())
                continue
            elif mode == "sentence":
                def _cap(mm):
                    return mm.group(0).upper()
                new = re.sub(r"(^\s*[a-z])|([.!?]\s+[a-z])", _cap, s)
            else:
                continue
            if new != s:
                _para_cursor(doc, para).setString(new)

    elif kind == "insert_table":
        rows = int(float(op.get("rows") or 2))
        cols = int(float(op.get("cols") or 2))
        tbl = doc.createInstance("com.sun.star.text.TextTable")
        tbl.initialize(rows, cols)
        at = str(op.get("at") or "end").strip().lower()
        anchor = _cursor_anchor(doc, at)
        text.insertTextContent(anchor, tbl, False)

    elif kind == "text_to_table":
        delim = str(op.get("delimiter") or ",")
        paras = resolve_scope(doc, op.get("scope") or "all")
        rows_data = [[c.strip() for c in p.getString().split(delim)] for p in paras if p.getString().strip()]
        op["_matched"] = len(rows_data)
        if not rows_data:
            return
        ncols = max(len(r) for r in rows_data)
        tbl = doc.createInstance("com.sun.star.text.TextTable")
        tbl.initialize(len(rows_data), ncols)
        anchor = doc.Text.createTextCursorByRange(paras[0].Start)
        text.insertTextContent(anchor, tbl, False)
        for r, row in enumerate(rows_data):
            for c in range(ncols):
                cellname = "%s%d" % (_col_letter(c), r + 1)
                tbl.getCellByName(cellname).setString(row[c] if c < len(row) else "")
        # Remove the source lines (now duplicated by the table) — clear their text; an empty
        # paragraph left behind is a cosmetic remainder, not a data error.
        for p in paras:
            _para_cursor(doc, p).setString("")

    elif kind == "insert_image":
        path = str(op.get("path") or "")
        img = doc.createInstance("com.sun.star.text.TextGraphicObject")
        img.AnchorType = uno.Enum("com.sun.star.text.TextContentAnchorType", "AS_CHARACTER")
        img.GraphicURL = uno.systemPathToFileUrl(path)   # deprecated but VERIFIED functional (probe, 2026-07-10)
        anchor = _cursor_anchor(doc, str(op.get("at") or "end"))
        text.insertTextContent(anchor, img, False)

    elif kind == "insert_page_break":
        anchor = _cursor_anchor(doc, str(op.get("at") or "end"))
        from com.sun.star.text.ControlCharacter import PARAGRAPH_BREAK
        text.insertControlCharacter(anchor, PARAGRAPH_BREAK, False)
        anchor.BreakType = uno.Enum("com.sun.star.style.BreakType", "PAGE_BEFORE")

    elif kind == "insert_page_number":
        pos = str(op.get("position") or "footer-left").strip().lower()
        area, align = (pos.split("-", 1) + ["left"])[:2]
        style = _page_style(doc)
        on_prop, area_text_prop = ("HeaderIsOn", "HeaderText") if area == "header" else ("FooterIsOn", "FooterText")
        setattr(style, on_prop, True)
        area_text = getattr(style, area_text_prop)
        cur = area_text.createTextCursor()
        cur.gotoStart(False)
        cur.gotoEnd(True)
        area_text.insertString(cur, "", True)   # clear any prior content before (re)inserting the field
        field = doc.createInstance("com.sun.star.text.TextField.PageNumber")
        area_text.insertTextContent(area_text.createTextCursorByRange(area_text.End), field, False)
        adj = uno.Enum("com.sun.star.style.ParagraphAdjust",
                       {"left": "LEFT", "center": "CENTER", "right": "RIGHT"}.get(align, "LEFT"))
        pit = area_text.createEnumeration()
        while pit.hasMoreElements():
            pit.nextElement().ParaAdjust = adj

    elif kind == "split_paragraph_sentences":
        op["_matched"] = 0
        for para in resolve_scope(doc, op.get("scope") or "first"):
            s = para.getString()
            sentences = [x.strip() for x in re.split(r"(?<=[.!?])\s+", s) if x.strip()]
            if len(sentences) < 2:
                continue
            op["_matched"] += len(sentences)
            cur = doc.Text.createTextCursorByRange(para.Start)
            cur.gotoEndOfParagraph(True)
            cur.setString(sentences[0])
            from com.sun.star.text.ControlCharacter import PARAGRAPH_BREAK
            end = doc.Text.createTextCursorByRange(para.End)
            for sent in sentences[1:]:
                text.insertControlCharacter(end, PARAGRAPH_BREAK, False)   # blank line
                text.insertControlCharacter(end, PARAGRAPH_BREAK, False)
                text.insertString(end, sent, False)

    elif kind == "dedup_lines":
        # Snapshot the KEYS before deleting anything, then delete in ONE pass keyed on that
        # snapshot — a delete merges/shifts neighboring paragraphs, so re-enumerating (or
        # trusting a stale paragraph reference) mid-loop is the failure mode to avoid; the
        # snapshot fixes identity by ORIGINAL 0-based order, then each delete re-resolves the
        # CURRENT live paragraph list by that same order (unaffected by earlier removals, since
        # removing paragraph i only ever shifts indices AFTER i, never before it).
        delim = str(op.get("delimiter") or ",")
        field_idx = int(float(op.get("field_index") or 0))
        keys, seen, to_delete = [], set(), []
        for para in _paragraphs(doc):
            s = para.getString()
            parts = [p.strip() for p in s.split(delim)]
            key = parts[field_idx] if field_idx < len(parts) else s
            keys.append(key)
        for orig_idx, key in enumerate(keys):
            if not key:
                continue
            if key in seen:
                to_delete.append(orig_idx)
            else:
                seen.add(key)
        removed = 0
        for orig_idx in to_delete:
            live = _paragraphs(doc)
            pos = orig_idx - removed
            if 0 <= pos < len(live):
                _delete_paragraph(doc, live, pos)
                removed += 1
        op["_matched"] = removed

    elif kind == "append_text":
        cur = doc.Text.createTextCursorByRange(text.End)
        from com.sun.star.text.ControlCharacter import PARAGRAPH_BREAK
        text.insertControlCharacter(cur, PARAGRAPH_BREAK, False)
        text.insertString(cur, str(op.get("text") or ""), False)

    elif kind == "set_default_font":
        name = str(op.get("name") or "")
        styles = doc.StyleFamilies.getByName("ParagraphStyles")
        for style_name in ("Default Paragraph Style", "Standard"):
            if styles.hasByName(style_name):
                styles.getByName(style_name).CharFontName = name

    elif kind == "export_pdf":
        folder, base = doc.getURL().rsplit("/", 1)
        name = (op.get("name") or "").strip()
        name = name[:-4] if name.lower().endswith(".pdf") else name
        name = name or base.rsplit(".", 1)[0]
        flt = uno.createUnoStruct("com.sun.star.beans.PropertyValue")
        flt.Name, flt.Value = "FilterName", "writer_pdf_Export"
        doc.storeToURL(folder + "/" + name + ".pdf", (flt,))

    else:
        raise ValueError("unknown op kind: %r" % kind)


def _cursor_anchor(doc, at):
    if (at or "").strip().lower() == "cursor":
        try:
            vc = doc.getCurrentController().getViewCursor()
            return doc.Text.createTextCursorByRange(vc.Start)
        except Exception:
            pass   # headless has no view (the calc freeze_panes finding) — fall through to end
    return doc.Text.createTextCursorByRange(doc.Text.End)


def _page_style(doc):
    """The page style governing the document's content (its own FIRST paragraph names it via
    PageDescName; 'Standard' is the verified always-present fallback — see module docstring)."""
    styles = doc.StyleFamilies.getByName("PageStyles")
    for para in _paragraphs(doc):
        name = getattr(para, "PageDescName", "") or ""
        if name and styles.hasByName(name):
            return styles.getByName(name)
    return styles.getByName("Standard")


def _col_letter(idx0):
    s = ""
    n = idx0
    while True:
        s = chr(ord("A") + n % 26) + s
        n = n // 26 - 1
        if n < 0:
            break
    return s


def _delete_paragraph(doc, paras, idx):
    """Remove paras[idx]'s own text AND one paragraph break (else an empty line remains). Built
    from EXPLICIT neighbor anchors (gotoRange between two known XTextRange points), not relative
    goLeft/gotoEndOfParagraph navigation — MEASURED (2026-07-10): a goLeft(1)-then-
    gotoEndOfParagraph() sequence silently mis-selected on the last-paragraph case (produced a
    merge that dropped the break but left BOTH paragraphs' text concatenated, uncleared) — the
    boundary position is apparently ambiguous for relative paragraph navigation. Anchor-to-anchor
    gotoRange has no such ambiguity: it selects exactly the span between two concrete points.
    Normal case (a next paragraph exists): select [this para's Start .. next para's Start] — its
    own text plus the trailing break. Last-paragraph case: select [previous para's End .. this
    para's End] — the leading break plus its own text, leaving the previous paragraph untouched.
    Sole paragraph in the document: nothing to merge with — just clear its own text."""
    para = paras[idx]
    if idx + 1 < len(paras):
        cur = doc.Text.createTextCursorByRange(para.Start)
        cur.gotoRange(paras[idx + 1].Start, True)
    elif idx > 0:
        cur = doc.Text.createTextCursorByRange(paras[idx - 1].End)
        cur.gotoRange(para.End, True)
    else:
        cur = doc.Text.createTextCursorByRange(para.Start)
        cur.gotoRange(para.End, True)
    cur.setString("")


# ── self-test (no live guest/UNO connection required) ───────────────────────────────
if __name__ == "__main__":
    import sys as _sys

    failures = []

    def _check(label, got, want):
        if got != want:
            failures.append("%s: got %r, want %r" % (label, got, want))

    # measurement conversion
    _check("1in->1/100mm", _to_1_100mm("1in"), 2540)
    _check("2.5cm->1/100mm", _to_1_100mm("2.5cm"), 2500)
    _check("10mm->1/100mm", _to_1_100mm("10mm"), 1000)
    _check("bare number assumed inches", _to_1_100mm("3"), 3 * 2540)
    _check("garbage -> 0", _to_1_100mm("nonsense"), 0)

    # truthy parsing
    for v in ("1", "true", "True", "yes", "on"):
        _check("truthy(%r)" % v, _truthy(v), True)
    for v in ("0", "false", "no", "", "off"):
        _check("truthy(%r)" % v, _truthy(v), False)

    # enum normalization: a live property may come back as a plain value or a uno.Enum-shaped
    # object exposing .value (see _enum_val's docstring for why both forms are seen in practice).
    class _FakeEnum:
        def __init__(self, v):
            self.value = v
    _check("_enum_val(plain int)", _enum_val(3), 3)
    _check("_enum_val(enum-like)", _enum_val(_FakeEnum("ITALIC")), "ITALIC")

    # scope dialect: exercised against resolve_scope ITSELF (not a re-implementation) by
    # substituting a fake paragraph enumerator — this is the real dispatch/regex logic under test.
    class _FakePara:
        def __init__(self, text, style="Standard"):
            self._text = text
            self.ParaStyleName = style

        def getString(self):
            return self._text

    fakes = [_FakePara("Title Heading", style="Heading 1"), _FakePara("First body paragraph."),
             _FakePara("Second body paragraph with a needle inside."), _FakePara("Third and last.")]

    _orig_paragraphs = _paragraphs
    globals()["_paragraphs"] = lambda _doc: fakes
    try:
        _check("scope=all", resolve_scope(None, "all"), fakes)
        _check("scope=(empty)", resolve_scope(None, ""), fakes)
        _check("scope=first", resolve_scope(None, "first"), fakes[:1])
        _check("scope=last", resolve_scope(None, "last"), fakes[-1:])
        _check("scope=heading", resolve_scope(None, "heading"), [fakes[0]])
        _check("scope=paragraph:2", resolve_scope(None, "paragraph:2"), [fakes[1]])
        _check("scope=paragraph:2-3", resolve_scope(None, "paragraph:2-3"), fakes[1:3])
        _check("scope=match:needle", resolve_scope(None, "match:needle"), [fakes[2]])
        _check("scope=garbage -> fail-closed empty", resolve_scope(None, "not a real scope"), [])
    finally:
        globals()["_paragraphs"] = _orig_paragraphs

    if failures:
        print("FAILED (%d):" % len(failures))
        for f in failures:
            print(" -", f)
        _sys.exit(1)
    print("writer_ops.py self-test: all checks passed (no live guest required)")
