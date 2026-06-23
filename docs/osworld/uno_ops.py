"""Shared UNO doc-mutation primitives — the SINGLE apply implementation.

This module is pure document logic: given a connected, loaded Calc `doc` (and its
`Sheets`), apply one op or an ordered op-log to the LIVE in-memory model. It owns
NO process lifecycle — no soffice launch, no kill, no lock-clean, no store, no
reload. Those belong to the caller, which differs by context:

  - the stateless one-shot (`api_plane::build_guest_apply`) wraps these with
    kill_soffice / clear_lock / private-headless-connect / storeToURL / GUI-reload;
  - the resident `uno_daemon.py` wraps these with its OWN Popen-scoped soffice,
    a dedicated UNO port + UserInstallation profile, and per-op reads.

Both callers share THIS body so the live session and the proven one-shot can never
drift in how an op mutates the doc. The logic here is a faithful mirror of the
op-dispatch loop in `build_guest_apply` (excel_to_calc + resolve_sheet + the
structural-first ordering + the per-op set/fill/set_formula_range handlers).

It is valid both as an importable module (the daemon `import`s it) and as inline
source spliced verbatim into the one-shot heredoc (P2). It imports `uno` at module
load only for the FillDirection enum; `import uno` needs no running office.
"""

import uno


# Structural ops create the containers content ops write into; they must land first.
STRUCTURAL = ("add_sheet", "rename_sheet", "copy_sheet")


def excel_to_calc(f):
    """Excel-A1 -> Calc-A1 for setFormula: sheet refs '!'->'.', arg sep ','->';' (outside strings)."""
    out, in_str = [], False
    for ch in f:
        if ch == '"':
            in_str = not in_str
            out.append(ch)
        elif not in_str and ch == '!':
            out.append('.')
        elif not in_str and ch == ',':
            out.append(';')
        else:
            out.append(ch)
    return "".join(out)


def make_resolve_sheet(doc):
    """Return a resolve_sheet(name) closure over `doc`/`doc.Sheets`.

    Tolerate a placeholder/unknown sheet name (the model often copies the prompt's
    "S"): exact match wins; a single-sheet book resolves unambiguously to its only
    sheet; else active/first.
    """
    sheets = doc.Sheets

    def resolve_sheet(name):
        if name and sheets.hasByName(name):
            return sheets.getByName(name)
        if sheets.Count == 1:
            return sheets.getByIndex(0)
        try:
            return doc.CurrentController.ActiveSheet
        except Exception:
            return sheets.getByIndex(0)

    return resolve_sheet


def order_ops(ops):
    """Dependency-correct ordering (deterministic, harness-owned): a container must
    exist before it's populated. STRUCTURAL ops first (stable within the group), then
    content ops. Models author in goal-narrative order ('fill the column ... in a new
    sheet'), not dependency order; this fixes it so a write never lands on a not-yet-
    created sheet."""
    return [o for o in ops if o.get("op") in STRUCTURAL] + \
           [o for o in ops if o.get("op") not in STRUCTURAL]


def apply_one_op(doc, resolve_sheet, op):
    """Apply ONE op to the live doc. `resolve_sheet` is a closure from make_resolve_sheet(doc).

    Returns None on success; raises on a malformed op (the caller decides whether a
    raise wedges the session or just drops the op from the log)."""
    sheets = doc.Sheets
    kind = op.get("op")
    if kind == "add_sheet":
        name = op["name"]
        idx = op.get("index", sheets.Count)
        if not sheets.hasByName(name):
            sheets.insertNewByName(name, idx)
    elif kind == "rename_sheet":
        if sheets.hasByName(op["old"]):
            sheets.getByName(op["old"]).Name = op["new"]
    elif kind == "copy_sheet":
        # Duplicate a sheet WITH its data (the app's "Move/Copy Sheet"). Append the copy (copyByName's
        # index arg proved unreliable for mid-list inserts), THEN moveByName it into place — explicit
        # reposition, deterministic. `before` puts the copy immediately before a named sheet (tolerant of
        # "Sheet 2"/"Sheet2"/case). Absent/unknown before → leave appended at the end.
        src, dest = op["source"], op["new"]
        before = (op.get("before") or "").strip()
        if sheets.hasByName(src) and not sheets.hasByName(dest):
            sheets.copyByName(src, dest, sheets.Count)         # append (reliable)
            if before:
                norm = lambda s: s.replace(" ", "").lower()
                names = [sheets.getByIndex(i).Name for i in range(sheets.Count)]
                tgt = next((i for i, nm in enumerate(names)
                            if nm == before or norm(nm) == norm(before)), None)
                if tgt is not None:
                    sheets.moveByName(dest, tgt)               # move the copy to just before the target
    elif kind == "set":
        sh = resolve_sheet(op["sheet"])
        cell = sh.getCellRangeByName(op["cell"]).getCellByPosition(0, 0)
        if "formula" in op and op["formula"] is not None:
            cell.setFormula(excel_to_calc(str(op["formula"])))
        else:
            v = op.get("value")
            if isinstance(v, (int, float)):
                cell.setValue(float(v))
            else:
                cell.setString("" if v is None else str(v))
    elif kind == "fill":
        # The human "Fill Down/Up/Left/Right" gestures, generalized: forward-fill blanks
        # along the axis, carrying the last non-empty value. The app sees the cells; the
        # model never enumerates them. getType().value is the enum NAME
        # ("EMPTY"/"VALUE"/"TEXT"/"FORMULA") — string compare avoids pyuno enum quirks.
        sh = resolve_sheet(op["sheet"])
        addr = sh.getCellRangeByName(op["range"]).getRangeAddress()
        c0, r0, c1, r1 = addr.StartColumn, addr.StartRow, addr.EndColumn, addr.EndRow
        direction = op.get("direction", "down")

        def fill_line(cells):
            carry = None  # (is_numeric, value)
            for c in cells:
                t = c.getType().value
                if t == "EMPTY":
                    if carry is not None:
                        if carry[0]:
                            c.setValue(carry[1])
                        else:
                            c.setString(carry[1])
                elif t == "VALUE":
                    carry = (True, c.getValue())
                else:
                    carry = (False, c.getString())

        if direction in ("down", "up"):
            for col in range(c0, c1 + 1):
                rows = range(r0, r1 + 1) if direction == "down" else range(r1, r0 - 1, -1)
                fill_line([sh.getCellByPosition(col, row) for row in rows])
        else:  # left / right
            for row in range(r0, r1 + 1):
                cols = range(c0, c1 + 1) if direction == "right" else range(c1, c0 - 1, -1)
                fill_line([sh.getCellByPosition(col, row) for col in cols])
    elif kind == "set_formula_range":
        # Apply a formula across the range with RELATIVE-REF adjustment — the app's "Fill
        # Down a formula". Set the seed on the top-left cell, then let UNO fillAuto
        # propagate it (adjusting refs like a fill-handle drag), so a whole computed column
        # is ONE op, not N.
        sh = resolve_sheet(op["sheet"])
        rng = sh.getCellRangeByName(op["range"])
        a = rng.getRangeAddress()
        sh.getCellByPosition(a.StartColumn, a.StartRow).setFormula(excel_to_calc(str(op["formula"])))
        if a.EndRow > a.StartRow:
            rng.fillAuto(uno.Enum("com.sun.star.sheet.FillDirection", "TO_BOTTOM"), 1)
        elif a.EndColumn > a.StartColumn:
            rng.fillAuto(uno.Enum("com.sun.star.sheet.FillDirection", "TO_RIGHT"), 1)
    elif kind == "format_cells":
        # ADDITIVE op-vocab (Wave 1): cell STYLE — font color / fill color / bold over a range.
        # Colors are hex ("#00ff00" or "00ff00"); empty/absent => leave that property untouched.
        # The evaluator's `style`/`check_cell` rules read these (font_color, bgcolor, font_bold).
        sh = resolve_sheet(op["sheet"])
        rng = sh.getCellRangeByName(op["range"])
        fc = (op.get("font_color") or "").lstrip("#").strip()
        bg = (op.get("fill_color") or "").lstrip("#").strip()
        bold = str(op.get("bold", "")).strip().lower() in ("1", "true", "yes", "bold")
        if fc:
            rng.CharColor = int(fc, 16)
        if bg:
            rng.CellBackColor = int(bg, 16)
        if bold:
            rng.CharWeight = 150.0   # com.sun.star.awt.FontWeight.BOLD
    elif kind == "merge_cells":
        sh = resolve_sheet(op["sheet"])
        sh.getCellRangeByName(op["range"]).merge(True)
    elif kind == "set_number_format":
        # Apply a number-format code (e.g. "0.00", "0.00%", "0") to a range so values render/compare
        # as the expected numeric type.
        sh = resolve_sheet(op["sheet"])
        rng = sh.getCellRangeByName(op["range"])
        fmts = doc.getNumberFormats()
        loc = uno.createUnoStruct("com.sun.star.lang.Locale")
        code = str(op["format"])
        key = fmts.queryKey(code, loc, False)
        if key == -1:
            key = fmts.addNew(code, loc)
        rng.NumberFormat = key
    elif kind == "sort_range":
        # Sort a range by one key column. `key_index` = 0-based column WITHIN the range; `has_header`
        # keeps the first row fixed; `ascending` toggles order. Done by VALUE read->sort->write (the UNO
        # SortDescriptor silently no-ops on this LO build); deterministic, numbers-before-text, and it
        # matches exactly what the evaluator compares (computed cell values).
        sh = resolve_sheet(op["sheet"])
        a = sh.getCellRangeByName(op["range"]).getRangeAddress()
        c0, r0, c1, r1 = a.StartColumn, a.StartRow, a.EndColumn, a.EndRow
        has_header = str(op.get("has_header", "true")).strip().lower() in ("1", "true", "yes")
        key = int(op.get("key_index", 0))
        asc = str(op.get("ascending", "true")).strip().lower() in ("1", "true", "yes", "asc")
        dr0 = r0 + (1 if has_header else 0)
        rows = []
        for r in range(dr0, r1 + 1):
            row = []
            for c in range(c0, c1 + 1):
                cell = sh.getCellByPosition(c, r)
                t = cell.getType().value
                if t == "VALUE":
                    row.append((1, cell.getValue()))
                elif t == "EMPTY":
                    row.append((2, None))
                else:
                    row.append((0, cell.getString()))
            rows.append(row)

        def keyf(row):
            typ, val = row[key]
            return (0, val) if typ == 1 else (1, str(val if val is not None else ""))
        rows.sort(key=keyf, reverse=not asc)
        for i, row in enumerate(rows):
            r = dr0 + i
            for j, (typ, val) in enumerate(row):
                cell = sh.getCellByPosition(c0 + j, r)
                if typ == 1:
                    cell.setValue(val)
                elif typ == 2:
                    cell.setString("")
                else:
                    cell.setString(val)
    else:
        raise ValueError("unknown op kind: %r" % kind)


def apply_op_log(doc, ops):
    """Apply a whole op-log to the live doc in dependency-correct order (structural first).

    Used by the one-shot (full log at once) and by the daemon's replay-on-restart. The
    daemon's per-step `apply` path calls `apply_one_op` directly (one op, already authored
    in loop order)."""
    resolve_sheet = make_resolve_sheet(doc)
    for op in order_ops(ops):
        apply_one_op(doc, resolve_sheet, op)
