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
            elif isinstance(v, str) and v.startswith("="):
                # The app's type-into-cell semantics: a leading "=" IS a formula. Without this a
                # set_cell value of "=(C12-B12)/B12" lands as literal TEXT and silently scores 0.
                cell.setFormula(excel_to_calc(v))
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
    elif kind == "freeze_panes":
        # Freeze the first N rows / M columns (the app's View→Freeze Rows and Columns). Freeze is
        # VIEW state: with a real view (visible/watch mode) XViewFreezable.freezeAt applies it live
        # and the store persists it. Headless/hidden LO exposes NO spreadsheet view (the controller
        # has no freezeAt; MEASURED — and the setViewData route hard-crashes the pyuno bridge), so
        # there the pane record is written into the SAVED file by patch_xlsx_freeze at store time
        # (the daemon's reconcile owns that; it keys off freeze_panes ops in the op log).
        cols, rows = freeze_counts(op)
        if rows or cols:
            ctrl = doc.getCurrentController()
            if hasattr(ctrl, "freezeAt"):
                ctrl.setActiveSheet(resolve_sheet(op.get("sheet")))
                ctrl.freezeAt(cols, rows)
    elif kind == "export_csv":
        # Export ONE sheet as CSV with the app's default csv options (comma, double-quote, UTF-8 —
        # the GUI dialog defaults, "as shown"). The Text-csv filter exports the ACTIVE sheet, so
        # activate the named one first. The csv lands next to the document sharing its base name
        # (op["name"] overrides the basename; the .csv extension is harness-owned). storeToURL
        # (not storeAsURL) keeps the session attached to the original xlsx.
        sh = resolve_sheet(op.get("sheet"))
        doc.getCurrentController().setActiveSheet(sh)
        folder, base = doc.getURL().rsplit("/", 1)
        name = (op.get("name") or "").strip()
        name = name[:-4] if name.lower().endswith(".csv") else name
        # Sheet-name misbind grounding (Class B, ablatable): the model sometimes binds the FILE
        # name field to the sheet TAB it saw ("Sheet1"). A tab name is never the asked file name
        # in observed tasks — fall back to the document's own base name.
        if name and doc.Sheets.hasByName(name):
            name = ""
        name = name or base.rsplit(".", 1)[0]
        flt = uno.createUnoStruct("com.sun.star.beans.PropertyValue")
        flt.Name, flt.Value = "FilterName", "Text - txt - csv (StarCalc)"
        opts = uno.createUnoStruct("com.sun.star.beans.PropertyValue")
        opts.Name, opts.Value = "FilterOptions", "44,34,76"
        doc.storeToURL(folder + "/" + name + ".csv", (flt, opts))
    elif kind == "transpose_range":
        # Matrix-transpose a rectangular range's VALUES to a destination anchor (the app's Paste
        # Special→Transpose). DataArray round-trip: strings stay strings, numbers stay numbers;
        # formulas transfer as their computed values (what sheet_data compares). `dest` is the
        # TOP-LEFT cell of the transposed block (a range is tolerated — its first cell anchors).
        sh = resolve_sheet(op.get("sheet"))
        data = sh.getCellRangeByName(op["source"]).getDataArray()
        if data and data[0]:
            t = tuple(zip(*data))
            a = sh.getCellRangeByName(str(op["dest"]).split(":")[0]).getRangeAddress()
            sh.getCellRangeByPosition(a.StartColumn, a.StartRow,
                                      a.StartColumn + len(t[0]) - 1,
                                      a.StartRow + len(t) - 1).setDataArray(t)
    elif kind == "reorder_columns":
        # Rearrange existing columns into a named left-to-right order (the app's cut-column →
        # insert-column drag). Whole-column insert/move/delete so FORMATS AND FORMULAS TRAVEL
        # with the data (a values-only permute would strand date/number formats at the old
        # positions and break rendered comparisons). Fail-closed: the order must name exactly
        # the used columns, each matching one header — anything else raises (no guessing).
        sh = resolve_sheet(op.get("sheet"))
        cur = sh.createCursor()
        cur.gotoStartOfUsedArea(False)
        cur.gotoEndOfUsedArea(True)
        a = cur.getRangeAddress()
        c0, r0, c1, r1 = a.StartColumn, a.StartRow, a.EndColumn, a.EndRow
        want = [t.strip().strip("{}").strip() for t in str(op.get("order") or "").split(",") if t.strip()]
        norm = lambda s: "".join(str(s).split()).casefold()
        if len(want) != (c1 - c0 + 1):
            raise ValueError("order names %d columns; used area has %d" % (len(want), c1 - c0 + 1))
        for i, name in enumerate(want):
            headers = [sh.getCellByPosition(c, r0).getString() for c in range(c0, c1 + 1)]
            if norm(headers[i]) == norm(name):
                continue
            j = next((k for k, h in enumerate(headers) if norm(h) == norm(name)), None)
            if j is None:
                raise ValueError("column %r not found in headers %s" % (name, headers))
            sh.Columns.insertByIndex(c0 + i, 1)
            src = j + (1 if j >= i else 0)              # the insert shifted the source right
            sh.moveRange(sh.getCellByPosition(c0 + i, r0).CellAddress,
                         sh.getCellRangeByPosition(c0 + src, r0, c0 + src, r1).RangeAddress)
            sh.Columns.removeByIndex(c0 + src, 1)
    elif kind == "hide_rows_where":
        # Hide (never delete) every used row containing the matched content — the app's row-hide.
        # match text compares the DISPLAYED string; an N/A-ish match also accepts the real =NA()
        # error cell (error code 32767, which displays as #N/A but reads back as an error, not text).
        sh = resolve_sheet(op.get("sheet"))
        m = str(op.get("match") or "").strip()
        na_ish = m.upper().lstrip("#") in ("N/A", "NA")
        cur = sh.createCursor()
        cur.gotoStartOfUsedArea(False)
        cur.gotoEndOfUsedArea(True)
        a = cur.getRangeAddress()
        for r in range(a.StartRow, a.EndRow + 1):
            for c in range(a.StartColumn, a.EndColumn + 1):
                cell = sh.getCellByPosition(c, r)
                if (m and cell.getString().strip() == m) or (na_ish and cell.getError() == 32767):
                    sh.getRows().getByIndex(r).IsVisible = False
                    break
    elif kind == "format_cells_where":
        # Style every cell matching a PREDICATE the goal states, harness-scanned (the model cannot
        # enumerate scattered cells it never sees). "weekend" = date-formatted cells whose date
        # falls on Sat/Sun (weekday via the doc's own NullDate epoch); "max" = the largest numeric
        # value in the scan area; any other match = exact displayed text. op["range"] limits the
        # scan (the harness resolves a named column to its data span); absent → whole used area.
        import datetime as _dt
        sh = resolve_sheet(op.get("sheet"))
        m = str(op.get("match") or "").strip()
        weekend = m.casefold() == "weekend"
        want_max = m.casefold() in ("max", "maximum", "highest")
        fc = (op.get("font_color") or "").lstrip("#").strip()
        bg = (op.get("fill_color") or "").lstrip("#").strip()
        fmts = doc.getNumberFormats()
        nd = doc.NullDate
        epoch = _dt.date(nd.Year, nd.Month, nd.Day)
        rng = str(op.get("range") or "").strip()
        if rng:
            a = sh.getCellRangeByName(rng).getRangeAddress()
        else:
            cur = sh.createCursor()
            cur.gotoStartOfUsedArea(False)
            cur.gotoEndOfUsedArea(True)
            a = cur.getRangeAddress()
        cells = [(sh.getCellByPosition(c, r), c, r)
                 for r in range(a.StartRow, a.EndRow + 1)
                 for c in range(a.StartColumn, a.EndColumn + 1)]
        mx = None
        if want_max:
            nums = [cell.getValue() for cell, _c, _r in cells if cell.getType().value in ("VALUE", "FORMULA")]
            mx = max(nums) if nums else None
        def _letter(i):
            s = ""
            i += 1
            while i:
                i, rr = divmod(i - 1, 26)
                s = chr(65 + rr) + s
            return s
        matched = []
        for cell, _c, _r in cells:
            if weekend:
                if cell.getType().value != "VALUE":
                    continue
                if not (fmts.getByKey(cell.NumberFormat).Type & 2):   # NumberFormat.DATE bit
                    continue
                hit = (epoch + _dt.timedelta(days=int(cell.getValue()))).weekday() >= 5
            elif want_max:
                hit = mx is not None and cell.getType().value in ("VALUE", "FORMULA") and \
                    abs(cell.getValue() - mx) < 1e-12
            else:
                hit = bool(m) and cell.getString().strip() == m
            if hit:
                matched.append("%s%d" % (_letter(_c), _r + 1))
                if bg:
                    cell.CellBackColor = int(bg, 16)
                if fc:
                    cell.CharColor = int(fc, 16)
        # LO's xlsx export DROPS programmatic font colors (measured: live CharColor readback OK,
        # stored styles.xml has no color while bold survives) — record the matched cells so the
        # daemon's reconcile can re-impose the color on the SAVED file (patch_xlsx_font_color).
        op["_matched"] = matched
    elif kind == "set_decimal_separator":
        # Render ALL numbers with the asked decimal separator by giving numeric cells the GENERAL
        # format of a locale whose separator that is (comma → ru_RU) — VALUES untouched, natural
        # precision kept ("as-is"), and any 'as shown' export (the evaluator's own csv convert)
        # renders the comma. This is the app's real localized-display mechanism, not a text rewrite.
        sh = resolve_sheet(op.get("sheet"))
        sep = str(op.get("separator") or ",").strip() or ","
        loc = uno.createUnoStruct("com.sun.star.lang.Locale")
        loc.Language, loc.Country = ("ru", "RU") if sep == "," else ("en", "US")
        cur = sh.createCursor()
        cur.gotoStartOfUsedArea(False)
        cur.gotoEndOfUsedArea(True)
        a = cur.getRangeAddress()
        # Pass 1 — the display precision the VALUES themselves need ("as-is" = the numbers, not a
        # stale cell format): uniform decimals = the max any value requires (a consistent column,
        # the way a human formats one; 0.1-step data → one decimal everywhere, so 1.0 shows "1,0").
        cells, dec = [], 0
        for r in range(a.StartRow, a.EndRow + 1):
            for c in range(a.StartColumn, a.EndColumn + 1):
                cell = sh.getCellByPosition(c, r)
                # VALUE and FORMULA both render numerically (a "+1" column is formulas and must
                # localize too); a format on a text-result formula is inert. TEXT stays untouched.
                if cell.getType().value in ("VALUE", "FORMULA"):
                    cells.append(cell)
                    v = cell.getValue()
                    for k in range(0, 7):
                        if abs(v - round(v, k)) < 1e-9:
                            dec = max(dec, k)
                            break
        code = "0" if dec == 0 else "0" + sep + "0" * dec   # format code in the locale's own notation
        fmts = doc.getNumberFormats()
        key = fmts.queryKey(code, loc, False)
        if key == -1:
            key = fmts.addNew(code, loc)
        for cell in cells:
            cell.NumberFormat = key
    elif kind == "export_pdf":
        # Export as PDF next to the document (name follows the doc unless overridden), scaled to
        # fit N pages via the sheet's page style (the app's Format→Page→Scale). Document-level,
        # headless-safe.
        sh = resolve_sheet(op.get("sheet"))
        fit = int(float(str(op.get("fit_pages") or "1").strip() or 1))
        if fit:
            style = doc.StyleFamilies.getByName("PageStyles").getByName(sh.PageStyle)
            style.ScaleToPages = fit
        folder, base = doc.getURL().rsplit("/", 1)
        name = (op.get("name") or "").strip()
        name = name[:-4] if name.lower().endswith(".pdf") else name
        if name and doc.Sheets.hasByName(name):   # sheet-name misbind grounding — see export_csv
            name = ""
        name = name or base.rsplit(".", 1)[0]
        flt = uno.createUnoStruct("com.sun.star.beans.PropertyValue")
        flt.Name, flt.Value = "FilterName", "calc_pdf_Export"
        doc.storeToURL(folder + "/" + name + ".pdf", (flt,))
    elif kind == "create_chart":
        # Insert a chart so openpyxl reads it back with the right tagname (lineChart/barChart) + series refs.
        # `ranges` = ";"-joined A1 ranges (e.g. "B1:G1;B12:G12") → category + value; `type` line|bar|column.
        # NOTE: `uno` is already module-level (line ~23); a local `import uno` here would make uno function-
        # local and break every OTHER branch that uses it (UnboundLocalError). Import only Rectangle locally.
        from com.sun.star.awt import Rectangle
        sh = resolve_sheet(op["sheet"])
        charts = sh.Charts
        name = op.get("name", "Chart1")
        addrs = []
        for rname in (op.get("ranges") or "").split(";"):
            rname = rname.strip()
            if rname:
                addrs.append(sh.getCellRangeByName(rname).getRangeAddress())
        rect = Rectangle(); rect.X = 9000; rect.Y = 500; rect.Width = 14000; rect.Height = 9000
        # (colh=False, rowh=True) is correct for BOTH orientations (measured on the saved-xlsx
        # round-trip): rows → val=B12:G12 cat=B1:G1 (the proven 0a2e43bf shape); columns with
        # header-free ranges ("A2:A36;E2:E36") → val=E2:E36 cat=A2:A36. Flipping the flags per
        # orientation degrades to split/degenerate series refs, which the evaluator keys on.
        col_headers = str(op.get("col_headers", "false")).lower() in ("1", "true", "yes")
        row_headers = str(op.get("row_headers", "true")).lower() in ("1", "true", "yes")
        if charts.hasByName(name):
            charts.removeByName(name)
        charts.addNewByName(name, rect, tuple(addrs), col_headers, row_headers)
        chart = charts.getByName(name).getEmbeddedObject()
        ctype = (op.get("type") or "line").lower()
        svc = {"line": "com.sun.star.chart.LineDiagram", "bar": "com.sun.star.chart.BarDiagram",
               "column": "com.sun.star.chart.BarDiagram"}.get(ctype, "com.sun.star.chart.LineDiagram")
        diag = chart.createInstance(svc)
        # series orientation: ROWS = one series spanning a row (matches a total-row chart B12:G12). Default
        # auto-detect gave column-wise series; force it. com.sun.star.chart.ChartDataRowSource.ROWS = 0.
        rowsrc = str(op.get("data_in", "rows")).lower()
        try:
            diag.DataRowSource = 0 if rowsrc == "rows" else 1
        except Exception:
            pass
        # chart1 BarDiagram.Vertical is INVERTED vs the xlsx barDir it exports (MEASURED:
        # Vertical=True → barDir='bar' horizontal; False → 'col' vertical — the evaluator's
        # "direction" prop compares these verbatim).
        if ctype == "column":
            try: diag.Vertical = False
            except Exception: pass
        elif ctype == "bar":
            try: diag.Vertical = True
            except Exception: pass
        chart.setDiagram(diag)
        if op.get("title"):
            try:
                chart.HasMainTitle = True; chart.Title.String = op["title"]
            except Exception:
                pass
    elif kind == "create_pivot":
        # Build a DataPilot (the app's Pivot Table) so the saved xlsx carries an OOXML pivotTable part the
        # evaluator reads back via worksheet._pivots. Field columns are 0-based indices INTO the source range
        # (which always starts at column A=0). A column index appearing in BOTH row_fields and data_fields is
        # the count-by-self case (group by a column AND count it) — emitted as two specs on the same field
        # (ROW then DATA), which UNO accepts and openpyxl reads as row_fields=[i] + data_fields=["i;;..."].
        # The source range is auto-detected from the source sheet's used area (matches the gold's full A1:G..
        # range; the evaluator's left/right-bias trimming then normalizes both to the used columns).
        # NOTE: `uno` stays module-level (see create_chart note); only struct imports are local.
        from com.sun.star.table import CellRangeAddress, CellAddress
        src = resolve_sheet(op["source_sheet"])
        dest_name = op.get("dest_sheet") or "Sheet2"
        if not sheets.hasByName(dest_name):
            sheets.insertNewByName(dest_name, sheets.Count)
        dest = sheets.getByName(dest_name)
        cur = src.createCursor(); cur.gotoEndOfUsedArea(False)
        used = cur.RangeAddress
        sa = CellRangeAddress()
        sa.Sheet = src.RangeAddress.Sheet
        sa.StartColumn = 0; sa.StartRow = 0
        sa.EndColumn = used.EndColumn; sa.EndRow = used.EndRow
        dp = dest.DataPilotTables
        name = op.get("name") or ("PivotTable%d" % (dp.Count + 1))
        if dp.hasByName(name):
            dp.removeByName(name)
        out_col = dp.Count * 10                          # separate band per pivot (location is not scored)
        desc = dp.createDataPilotDescriptor()
        desc.setSourceRange(sa)
        fields = desc.DataPilotFields
        ORI = lambda v: uno.Enum("com.sun.star.sheet.DataPilotFieldOrientation", v)
        func = uno.Enum("com.sun.star.sheet.GeneralFunction", (op.get("data_func") or "sum").upper())
        for ci in op.get("row_fields", []):
            fields.getByIndex(int(ci)).Orientation = ORI("ROW")
        for ci in op.get("col_fields", []):
            fields.getByIndex(int(ci)).Orientation = ORI("COLUMN")
        for ci in op.get("data_fields", []):
            f = fields.getByIndex(int(ci)); f.Orientation = ORI("DATA"); f.Function = func
        oa = CellAddress(); oa.Sheet = dest.RangeAddress.Sheet; oa.Column = out_col; oa.Row = 0
        dp.insertNewByName(name, oa, desc)
    else:
        raise ValueError("unknown op kind: %r" % kind)


def freeze_counts(op):
    """(cols, rows) to freeze, from either dialect of a freeze_panes op. A `range` names the
    block the goal wants kept visible ("freeze the range A1:B1") — its END cell defines the
    frozen extent (B1 → 2 columns, 1 row), matching the app's put-cursor-after-the-block
    freeze semantics. Bare counts (`rows`/`cols`) pass through. Deterministic geometry, no
    task knowledge."""
    import re as _re
    rng = str(op.get("range") or "").strip().replace("$", "")
    m = _re.match(r"[A-Za-z]+\d+(?::([A-Za-z]+)(\d+))?$", rng) if rng else None
    if m:
        end_col = (m.group(1) or _re.match(r"([A-Za-z]+)", rng).group(1)).upper()
        end_row = int(m.group(2) or _re.match(r"[A-Za-z]+(\d+)", rng).group(1))
        cols = 0
        for ch in end_col:
            cols = cols * 26 + (ord(ch) - 64)
        return cols, end_row
    return (int(float(str(op.get("cols") or "0").strip() or 0)),
            int(float(str(op.get("rows") or "0").strip() or 0)))


def patch_xlsx_freeze(path, sheet_name, cols, rows):
    """Write a frozen-pane record directly into a SAVED xlsx (stdlib-only — guest-safe, no openpyxl).

    Freeze state lives in the VIEW; headless/hidden LO has no view, so a store from a headless
    session silently drops it. This patches the stored file with the exact record a GUI save
    would write (<pane xSplit/ySplit state="frozen"/> inside <sheetView>) — precisely what the
    evaluator's openpyxl freeze rule reads, and what a subsequent GUI open+re-save round-trips.
    sheet_name None/'' or unknown → first sheet. Idempotent: an existing pane record is replaced."""
    import os as _os
    import re as _re
    import zipfile as _zip
    cols = int(float(str(cols or 0).strip() or 0))
    rows = int(float(str(rows or 0).strip() or 0))
    if not (cols or rows):
        return
    with _zip.ZipFile(path) as z:
        names = z.namelist()
        contents = {n: z.read(n) for n in names}
    wb = contents["xl/workbook.xml"].decode("utf-8")
    rels = contents["xl/_rels/workbook.xml.rels"].decode("utf-8")
    sheets = []                                       # (name, r:id) in workbook order, attr-order-proof
    for tag in _re.findall(r"<sheet\b[^>]*>", wb):
        nm = _re.search(r'name="([^"]*)"', tag)
        rid = _re.search(r'r:id="([^"]*)"', tag)
        if nm and rid:
            sheets.append((nm.group(1), rid.group(1)))
    if not sheets:
        return
    rid = next((r for n, r in sheets if n == (sheet_name or "")), sheets[0][1])
    tgt = None
    for tag in _re.findall(r"<Relationship\b[^>]*>", rels):
        if 'Id="%s"' % rid in tag:
            m = _re.search(r'Target="([^"]*)"', tag)
            tgt = m.group(1) if m else None
            break
    if not tgt:
        return
    sheet_path = tgt.lstrip("/") if tgt.startswith("/") else "xl/" + tgt
    if sheet_path not in contents:
        return
    xml = contents[sheet_path].decode("utf-8")

    def letter(i):                                    # 0-based column index -> A1 letter
        s = ""
        i += 1
        while i:
            i, r = divmod(i - 1, 26)
            s = chr(65 + r) + s
        return s
    attrs = (('xSplit="%d" ' % cols) if cols else "") + (('ySplit="%d" ' % rows) if rows else "")
    active = "bottomRight" if (cols and rows) else ("bottomLeft" if rows else "topRight")
    pane = '<pane %stopLeftCell="%s%d" activePane="%s" state="frozen"/>' % (
        attrs, letter(cols), rows + 1, active)
    xml = _re.sub(r"<pane\b[^>]*/>", "", xml)         # replace any prior pane record
    if _re.search(r"<sheetView\b[^>]*/>", xml):       # self-closing view: expand it
        xml = _re.sub(r"(<sheetView\b[^>]*)/>", lambda m: m.group(1) + ">" + pane + "</sheetView>",
                      xml, count=1)
    else:                                             # pane must be the FIRST child (schema order)
        xml = _re.sub(r"(<sheetView\b[^>]*>)", lambda m: m.group(1) + pane, xml, count=1)
    contents[sheet_path] = xml.encode("utf-8")
    tmp = path + ".panetmp"
    with _zip.ZipFile(tmp, "w", _zip.ZIP_DEFLATED) as z:
        for n in names:
            z.writestr(n, contents[n])
    _os.replace(tmp, path)


def patch_xlsx_font_color(path, sheet_name, cells, color):
    """Write an explicit font color onto SPECIFIC cells of a SAVED xlsx (stdlib-only, guest-safe).

    LO's xlsx export DROPS programmatic font colors entirely (measured: CharColor reads back in
    the live doc; the stored styles.xml carries no <color> while bold on the same cell survives).
    For each target cell: clone its font with the rgb added, clone its xf pointing at the new
    font, repoint the cell's style index. sheet_name ''/unknown → first sheet."""
    import os as _os
    import re as _re
    import zipfile as _zip
    rgb = "FF" + str(color).lstrip("#").upper()[-6:]
    with _zip.ZipFile(path) as z:
        names = z.namelist()
        contents = {n: z.read(n) for n in names}
    wb = contents["xl/workbook.xml"].decode("utf-8")
    rels = contents["xl/_rels/workbook.xml.rels"].decode("utf-8")
    sheets = []
    for tag in _re.findall(r"<sheet\b[^>]*>", wb):
        nm = _re.search(r'name="([^"]*)"', tag)
        rid = _re.search(r'r:id="([^"]*)"', tag)
        if nm and rid:
            sheets.append((nm.group(1), rid.group(1)))
    if not sheets:
        return
    rid = next((r for n, r in sheets if n == (sheet_name or "")), sheets[0][1])
    tgt = None
    for tag in _re.findall(r"<Relationship\b[^>]*>", rels):
        if 'Id="%s"' % rid in tag:
            m = _re.search(r'Target="([^"]*)"', tag)
            tgt = m.group(1) if m else None
            break
    if not tgt:
        return
    spath = tgt.lstrip("/") if tgt.startswith("/") else "xl/" + tgt
    if spath not in contents:
        return
    sxml = contents[spath].decode("utf-8")
    styles = contents["xl/styles.xml"].decode("utf-8")
    fonts = _re.findall(r"<font>.*?</font>|<font/>", styles)
    xfs_m = _re.search(r"<cellXfs[^>]*>(.*?)</cellXfs>", styles, _re.S)
    if not xfs_m:
        return
    xf_list = _re.findall(r"<xf\b[^>]*/>|<xf\b.*?</xf>", xfs_m.group(1))
    orig_s = {}
    for coord in cells:
        m = _re.search(r'<c r="%s"(?:\s+s="(\d+)")?' % coord, sxml)
        if m:
            orig_s[coord] = int(m.group(1)) if m.group(1) else 0
    if not orig_s:
        return
    new_xf_of, add_fonts, add_xfs = {}, [], []
    for s_idx in sorted(set(orig_s.values())):
        xf = xf_list[s_idx] if s_idx < len(xf_list) else xf_list[0]
        fm = _re.search(r'fontId="(\d+)"', xf)
        f_idx = int(fm.group(1)) if fm else 0
        font = fonts[f_idx] if f_idx < len(fonts) else "<font/>"
        nf = _re.sub(r"<color\b[^/]*/>", "", font)
        if nf.startswith("<font>"):
            nf = nf.replace("<font>", '<font><color rgb="%s"/>' % rgb, 1)
        else:
            nf = '<font><color rgb="%s"/></font>' % rgb
        fid = len(fonts) + len(add_fonts)
        add_fonts.append(nf)
        nxf = xf
        if 'fontId="' in nxf:
            nxf = _re.sub(r'fontId="\d+"', 'fontId="%d"' % fid, nxf, count=1)
        else:
            nxf = nxf.replace("<xf ", '<xf fontId="%d" ' % fid, 1)
        if 'applyFont="' in nxf:
            nxf = _re.sub(r'applyFont="[^"]*"', 'applyFont="1"', nxf, count=1)
        else:
            nxf = nxf.replace("<xf ", '<xf applyFont="1" ', 1)
        new_xf_of[s_idx] = len(xf_list) + len(add_xfs)
        add_xfs.append(nxf)
    styles = _re.sub(r'(<fonts count=")(\d+)(")',
                     lambda m: m.group(1) + str(int(m.group(2)) + len(add_fonts)) + m.group(3),
                     styles, count=1)
    styles = styles.replace("</fonts>", "".join(add_fonts) + "</fonts>", 1)
    styles = _re.sub(r'(<cellXfs count=")(\d+)(")',
                     lambda m: m.group(1) + str(int(m.group(2)) + len(add_xfs)) + m.group(3),
                     styles, count=1)
    styles = styles.replace("</cellXfs>", "".join(add_xfs) + "</cellXfs>", 1)
    for coord, s_idx in orig_s.items():
        sxml = _re.sub(r'<c r="%s"(?:\s+s="\d+")?' % coord,
                       '<c r="%s" s="%d"' % (coord, new_xf_of[s_idx]), sxml, count=1)
    contents[spath] = sxml.encode("utf-8")
    contents["xl/styles.xml"] = styles.encode("utf-8")
    tmp = path + ".fcolortmp"
    with _zip.ZipFile(tmp, "w", _zip.ZIP_DEFLATED) as z:
        for n in names:
            z.writestr(n, contents[n])
    _os.replace(tmp, path)


def patch_xlsx_font_rgb(path):
    """Normalize LO's theme-black font serialization to explicit rgb in a SAVED xlsx (stdlib-only).

    LibreOffice exports default-black fonts as <color theme="1"/>; Excel-authored golds carry
    <color rgb="FF000000"/>. openpyxl reads the theme form as a non-string sentinel, so the
    evaluator's font_color comparison fails on EVERY untouched cell — a pure serialization
    dialect difference (both are black). Rewrites only theme-1 font colors in styles.xml.
    Always applied at store time (dialect normalization, not an op; ablatable)."""
    import os as _os
    import zipfile as _zip
    with _zip.ZipFile(path) as z:
        names = z.namelist()
        if "xl/styles.xml" not in names:
            return
        contents = {n: z.read(n) for n in names}
    xml = contents["xl/styles.xml"].decode("utf-8")
    new = xml.replace('<color theme="1"/>', '<color rgb="FF000000"/>')
    if new == xml:
        return
    contents["xl/styles.xml"] = new.encode("utf-8")
    tmp = path + ".fonttmp"
    with _zip.ZipFile(tmp, "w", _zip.ZIP_DEFLATED) as z:
        for n in names:
            z.writestr(n, contents[n])
    _os.replace(tmp, path)


def apply_op_log(doc, ops):
    """Apply a whole op-log to the live doc in dependency-correct order (structural first).

    Used by the one-shot (full log at once) and by the daemon's replay-on-restart. The
    daemon's per-step `apply` path calls `apply_one_op` directly (one op, already authored
    in loop order)."""
    resolve_sheet = make_resolve_sheet(doc)
    for op in order_ops(ops):
        apply_one_op(doc, resolve_sheet, op)
