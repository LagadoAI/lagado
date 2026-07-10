"""battery_calc.py — the INSIGHT BATTERY for calc authoring conditions.

Tests the thesis the OSWorld score structurally cannot isolate: is the model's failure a
CAPABILITY wall or a CONDITIONS wall? Same weights (Qwen2.5-Coder-7B on :8080), same native-session
apply path, same real env.evaluate() — only the AUTHORING CONDITIONS change between A and B.

  Condition A (bad / current):  raw structure blob + A1 cell coords + raw formula + ONE-SHOT, no read-back.
  Condition B (good):           labeled column CANDIDATES (letter+header+samples) + REASON-then-emit +
                                names resolved deterministically (exact, FAIL-CLOSED on ambiguity) +
                                READ-BACK with SOUND FALSIFIERS (falsify only, never confirm).

Baked-in instrumentation (the probes OSWorld can't give):
  P2 attribution — every run logs WHICH step/falsifier fired (detect / reason / resolve / apply / falsify).
  P5 calibration — every run logs (harness self-report: "no detected fault") vs (ground truth: env score).
  False-pass (the integrity core) — counts runs where the harness reported done but the oracle says 0.

Floor untouched: this is a NEW driver over the proven native session (uno_daemon). Additive by construction.

Usage (from the OSWorld repo dir, its venv, the podman sock):
  DOCKER_HOST=unix:///run/podman/podman.sock PYTHONPATH=/home/alucard/projects/OSWorld \
  .venv/bin/python /home/alucard/projects/lagado/lagado-agent/python/osworld/battery_calc.py <task_json> [N=3] [cond=AB]
"""
import json, os, re, sys, time, statistics
import requests

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from run_session_task import (Guest, deploy_daemon, pick_uno_python, task_input_path, memory_ok)
# DesktopEnv is imported lazily in main(): only the A/B bench boots its own env. calc_solve.py
# imports THIS module for the authoring/falsifier core and must run without the OSWorld venv.

BRAIN = os.environ.get("LAGADO_BRAIN", "http://localhost:8080/completion")
LOGDIR = "/tmp/lagado_battery"

# ── column letters ────────────────────────────────────────────────────────────
def col_letter(idx0):
    s = ""
    n = idx0
    while True:
        s = chr(ord("A") + n % 26) + s
        n = n // 26 - 1
        if n < 0:
            break
    return s

# ── DETECTOR: enumerate columns as labeled candidates (letter + header + samples) ──
# NOTE: the daemon's structure() "headers" field reads the WRONG row (gotoEndOfUsedArea(False)
# collapses the cursor to the bottom-right cell, so headers come from the LAST row). That field was
# never exercised before (all prior golds were hand-driven with explicit A1 ranges). We do NOT touch
# the proven daemon — we read row 1 explicitly here via the read verb. Extent (cols/rows) IS correct
# (the bottom-right end cell gives the true dims), so we keep it.
def find_header_row(rows):
    """P4 coverage lever: don't ASSUME row 1. Header row = the first row with >=2 non-empty TEXT cells
    immediately followed by a row containing numeric data. Falls back to row 1. rows = 0-based list of
    typed row-lists. Returns a 1-based row index."""
    for i in range(len(rows) - 1):
        texts = sum(1 for v in rows[i] if isinstance(v, str) and v.strip())
        below_num = any(isinstance(v, (int, float)) for v in rows[i + 1])
        if texts >= 2 and below_num:
            return i + 1
    return 1

def _blank(v):
    return v is None or (isinstance(v, str) and not v.strip())

def segment_regions(grid, ncols, coltypes=(), colfmt=()):
    """MULTI-TABLE OBSERVATION (2026-07-04, the turn-11 fracture line): segment a sheet's typed grid into
    TABLE REGIONS — row-blocks split on fully-blank rows, then column-groups split on columns blank within
    the block (covers both stacked tables [347ef137, d681960f] and side-by-side tables [7e429b8d]). Each
    region gets its own title line (a lone leading text cell), header row, candidates (ABSOLUTE letters),
    and data span. Pure function over the grid — deterministic, no task knowledge. 1-based rows throughout.
    Returns a list of region dicts; a plain single-table sheet returns exactly one region."""
    nrows = len(grid)
    def cell(r, c):                                   # 1-based row, 0-based col
        row = grid[r - 1] if 0 < r <= nrows else []
        return row[c] if c < len(row) else None
    def row_blank(r):
        return all(_blank(cell(r, c)) for c in range(ncols))
    # row-blocks: maximal runs of non-blank rows
    blocks, r = [], 1
    while r <= nrows:
        if row_blank(r):
            r += 1; continue
        r0 = r
        while r <= nrows and not row_blank(r):
            r += 1
        blocks.append((r0, r - 1))
    regions = []
    for r0, r1 in blocks:
        # column-groups: maximal runs of columns non-blank somewhere within the block
        used = [any(not _blank(cell(r, c)) for r in range(r0, r1 + 1)) for c in range(ncols)]
        c = 0
        while c < ncols:
            if not used[c]:
                c += 1; continue
            c0 = c
            while c < ncols and used[c]:
                c += 1
            c1 = c - 1
            # region's own last data row (a block can outlive a narrow side table: 7e429b8d's A:B
            # ends at row 7 while the D:F table beside it runs to 12)
            rr1 = max(r for r in range(r0, r1 + 1)
                      if any(not _blank(cell(r, cc)) for cc in range(c0, c1 + 1)))
            # title: a lone leading text cell spanning nothing else (347ef137 'Personal Costs - 2019').
            # MULTI-COLUMN regions only: in a 1-column region EVERY row has exactly one cell, so the
            # heuristic ate the real header and promoted the first data VALUE to header (measured on
            # abed40dc: 'Names with Duplicates' became a title, 'Keira Daily' became the header, and
            # the model's value-reference then bound lexically — the true abed40dc breakage, earlier
            # misattributed to decode variance).
            title, hr0 = None, r0
            first_vals = [cell(r0, cc) for cc in range(c0, c1 + 1)]
            nonblank = [v for v in first_vals if not _blank(v)]
            if rr1 > r0 and c1 > c0 and len(nonblank) == 1 and isinstance(nonblank[0], str):
                title, hr0 = nonblank[0].strip(), r0 + 1
            sub = [[cell(r, cc) for cc in range(c0, c1 + 1)] for r in range(hr0, rr1 + 1)]
            hrow = hr0 + find_header_row(sub) - 1     # absolute 1-based header row
            headers = [cell(hrow, cc) for cc in range(c0, c1 + 1)]
            samples = [[cell(r, cc) for cc in range(c0, c1 + 1)]
                       for r in range(hrow + 1, min(hrow + 4, rr1 + 1))]
            cands = []
            for i, cc in enumerate(range(c0, c1 + 1)):
                hdr = headers[i] if headers[i] is not None else ""
                cands.append({"letter": col_letter(cc), "header": str(hdr),
                              "samples": [row[i] for row in samples], "idx0": cc,
                              "ntype": coltypes[cc] if cc < len(coltypes) else "number",
                              "fmt": colfmt[cc] if cc < len(colfmt) else None})
            reg = {"cols": cands, "row0": r0, "row1": rr1, "header_row": hrow,
                   "data_start": hrow + 1, "rows": max(rr1 - hrow, 0), "title": title}
            # SMALL-TABLE FULL CONTENT (measured on d681960f: a 6-row grade-scale table rendered as
            # 3 samples made the model declare the scale "not provided" and INVENT one — a compact
            # reference table's semantics ARE its rows). Carry every data row for tables ≤10 rows so
            # the card can show the whole mapping. Observation completeness, not leading.
            if 0 < reg["rows"] <= 10:
                reg["data"] = [[cell(r, cc) for cc in range(c0, c1 + 1)]
                               for r in range(hrow + 1, rr1 + 1)]
            regions.append(reg)
    return regions

def detect(g, sheets_detail):
    """For each sheet: find the header row (not assumed row 1), build candidate columns (letter, header,
    samples), and the data-start row. Returns {sheet: {cols, rows, data_start, header_row}}.
    MULTI-TABLE (2026-07-04): the full used grid is also segmented into table regions; when a sheet holds
    MORE than one table the region list rides along under 'regions' and region-aware consumers engage.
    Single-table sheets keep the exact legacy fields (the top-8 read basis unchanged) — floor untouched."""
    out = {}
    for sd in sheets_detail:
        name = sd["name"]
        ncols = max(sd["extent"]["cols"], 1)
        nrows = sd["extent"]["rows"]
        lastcol = col_letter(ncols - 1)
        topr = g.client("read", {"sheet": name, "range": "A1:%s%d" % (lastcol, min(max(nrows, 1), 400))})
        grid = topr.get("cells", []) if topr.get("ok") else []
        top = grid[:8]                                     # legacy basis — byte-identical to the old read
        hrow = find_header_row(top)                       # 1-based
        headers = top[hrow - 1] if len(top) >= hrow else []
        samples = top[hrow:hrow + 3] if len(top) > hrow else []
        coltypes = sd.get("coltypes", [])
        colfmt = sd.get("colfmt", [])
        cands = []
        for c in range(ncols):
            hdr = headers[c] if c < len(headers) and headers[c] is not None else ""
            colsamp = [row[c] for row in samples if c < len(row)]
            ntype = coltypes[c] if c < len(coltypes) else "number"
            cands.append({"letter": col_letter(c), "header": str(hdr), "samples": colsamp,
                          "idx0": c, "ntype": ntype, "fmt": colfmt[c] if c < len(colfmt) else None})
        info = {"cols": cands, "rows": nrows, "data_start": hrow + 1, "header_row": hrow}
        try:
            regions = segment_regions(grid, ncols, coltypes, colfmt)
        except Exception:
            regions = []                                   # observation must never break the floor
        if len(regions) > 1:
            info["regions"] = regions
        out[name] = info
    return out

def _sheet_cols(info):
    """The sheet's resolvable candidate columns: the UNION of region candidates on a multi-table sheet
    (each tagged with its region index), else the legacy flat list. Duplicate headers across regions stay
    duplicated — resolution fail-closes on them unless a region context disambiguates."""
    regs = (info or {}).get("regions") or []
    if len(regs) > 1:
        return [dict(c, region=i) for i, rg in enumerate(regs) for c in rg["cols"]]
    return (info or {}).get("cols", [])

def _region_of_col(info, letter, region_hint=None):
    """The region record owning a column letter on a multi-table sheet (region_hint wins when the letter
    exists in several stacked regions). None on single-table sheets — legacy geometry applies."""
    regs = (info or {}).get("regions") or []
    if len(regs) <= 1:
        return None
    if region_hint is not None and 0 <= region_hint < len(regs) and \
       any(c["letter"] == letter for c in regs[region_hint]["cols"]):
        return regs[region_hint]
    owners = [rg for rg in regs if any(c["letter"] == letter for c in rg["cols"])]
    return owners[0] if len(owners) == 1 else None

def _range_region(info, rng):
    """The region record containing an A1 range's anchor cell, for multi-table sheets; None otherwise.
    Geometry consumers (clamps, chart anchors, spans) must scope to the table the range lives in — the
    flat sheet extent would drag a range across a table boundary."""
    regs = (info or {}).get("regions") or []
    m = re.match(r"([A-Za-z]+)(\d+)", (rng or "").replace("$", ""))
    if len(regs) <= 1 or not m:
        return None
    col0, row0 = _col_idx(m.group(1)), int(m.group(2))
    for rg in regs:
        idxs = [c["idx0"] for c in rg["cols"]]
        if rg["row0"] <= row0 <= rg["row1"] and idxs and min(idxs) <= col0 <= max(idxs):
            return rg
    return None

def live_detect(g):
    """Re-perceive the WHOLE workbook from the live session (per-op observation). Used after each
    structural op so new sheets / freshly-set header cells become resolvable."""
    s = g.client("structure")
    return detect(g, s.get("detail", [])) if s.get("ok") else {}

# ════════════════════════════════════════════════════════════════════════════════
# CONDITION A — bad/current: raw structure blob, A1 coords, raw formula, one-shot.
# ════════════════════════════════════════════════════════════════════════════════
GRAMMAR_A = (
    'root ::= "[" op ("," op)* "]"\n'
    'op ::= "set_cell(sheet=" str ", cell=" str ", formula=" str ")"'
    ' | "set_cell(sheet=" str ", cell=" str ", value=" str ")"'
    ' | "add_sheet(name=" str ")"'
    ' | "rename_sheet(old=" str ", new=" str ")"\n'
    'str ::= "\\"" [^"\\\\\\n\\r]* "\\""\n'
)
PROMPT_A = (
    "You operate a spreadsheet by issuing operations. Available operations:\n"
    "  set_cell(sheet=\"S\", cell=\"A1\", formula=\"=...\")   set a cell to a formula\n"
    "  set_cell(sheet=\"S\", cell=\"A1\", value=\"...\")       set a cell to a literal\n"
    "  add_sheet(name=\"S\")                                  add a sheet\n"
    "  rename_sheet(old=\"S\", new=\"S2\")                     rename a sheet\n\n"
    "Goal: {instr}\n"
    "Workbook structure:\n{struct}\n\n"
    "Emit the operations that accomplish the goal, as a list of calls:"
)

def raw_struct_blob(detected):
    """The SAME real headers B sees, but as a flat reference blob (the BAD condition): the model must map
    names→column letters itself and author raw A1 formulas one-shot. Isolates CONDITIONS from perception."""
    lines = []
    for sheet, info in detected.items():
        cols = ", ".join("%s=%r" % (c["letter"], c["header"]) for c in info["cols"])
        lines.append("Sheet %r, data rows 2..%d, columns: %s" % (sheet, info["rows"], cols))
    return "\n".join(lines)

def author_A(instr, detected):
    struct = raw_struct_blob(detected)               # real headers (detector-fixed), raw-blob form
    r = requests.post(BRAIN, json={"prompt": PROMPT_A.format(instr=instr, struct=struct),
                                   "grammar": GRAMMAR_A, "temperature": 0, "n_predict": 1200}, timeout=200)
    raw = r.json().get("content", "")
    return parse_A(raw), {"raw": raw, "struct": struct}

def parse_A(text):
    ops = []
    for verb, body in scan_calls(text, ("set_cell", "add_sheet", "rename_sheet")):
        kw = parse_kv(body)
        if verb == "set_cell":
            o = {"op": "set", "sheet": kw.get("sheet"), "cell": kw.get("cell")}
            if "formula" in kw: o["formula"] = kw["formula"]
            elif "value" in kw: o["value"] = coerce(kw["value"])
            else: continue
            ops.append(o)
        elif verb == "add_sheet":
            ops.append({"op": "add_sheet", "name": kw.get("name")})
        elif verb == "rename_sheet":
            ops.append({"op": "rename_sheet", "old": kw.get("old"), "new": kw.get("new")})
    return ops

# ════════════════════════════════════════════════════════════════════════════════
# CONDITION B — good: labeled candidates, reason-then-emit, names→A1 (fail-closed), read-back.
# ════════════════════════════════════════════════════════════════════════════════
# Names are {Header} or {Sheet.Header}, BRACKET-DELIMITED so an operator inside a header
# ("Profit/Loss") never confuses the parser (the advisor's parser concern). compute_column fills a
# whole column; the harness resolves names→A1, finds the extent, and propagates (set_formula_range).
GRAMMAR_B = (
    'root ::= "[" op ("," op)* "]"\n'
    'op ::= "compute_column(sheet=" str ", target=" str ", formula=" str ")"'
    ' | "set_cell(sheet=" str ", cell=" str ", value=" str ")"'
    ' | "add_sheet(name=" str ")"'
    ' | "rename_sheet(old=" str ", new=" str ")"'
    ' | "copy_sheet(source=" str ", new=" str ", before=" str ")"'
    ' | "total_row(sheet=" str ", label=" str ", columns=" str ")"'
    ' | "format_cells(sheet=" str ", range=" str ", font_color=" str ", fill_color=" str ", bold=" str ")"'
    ' | "merge_cells(sheet=" str ", range=" str ")"'
    ' | "sort_range(sheet=" str ", range=" str ", key=" str ", order=" str ")"'
    ' | "set_number_format(sheet=" str ", range=" str ", format=" str ")"'
    ' | "create_chart(sheet=" str ", ranges=" str ", type=" str ", title=" str ", data_in=" str ")"'
    ' | "create_pivot(source=" str ", dest=" str ", rows=" str ", cols=" str ", data=" str ", func=" str ")"'
    ' | "freeze_panes(sheet=" str ", range=" str ", rows=" str ", cols=" str ")"'
    ' | "export_csv(sheet=" str ", name=" str ")"'
    ' | "transpose_range(sheet=" str ", source=" str ", dest=" str ")"'
    ' | "reorder_columns(sheet=" str ", order=" str ")"'
    ' | "hide_rows_where(sheet=" str ", match=" str ")"'
    ' | "format_cells_where(sheet=" str ", match=" str ", fill_color=" str ", font_color=" str ", range=" str ")"'
    ' | "set_decimal_separator(sheet=" str ", separator=" str ")"'
    ' | "export_pdf(sheet=" str ", name=" str ", fit_pages=" str ")"'
    ' | "set_zoom(sheet=" str ", percent=" str ")"'
    ' | "dedup_column(sheet=" str ", source=" str ", target=" str ")"'
    ' | "compute_row(sheet=" str ", label=" str ", range=" str ", formula=" str ")"'
    ' | "split_column(sheet=" str ", source=" str ", delimiter=" str ", targets=" str ")"'
    ' | "infeasible(reason=" str ")"\n'
    'str ::= "\\"" [^"\\\\\\n\\r]* "\\""\n'
)

def candidate_cards(detected):
    lines = []
    for sheet, info in detected.items():
        regs = info.get("regions") or []
        if len(regs) > 1:
            # MULTI-TABLE sheet (2026-07-04): one card per table, WITH absolute row spans — here the
            # rows are the load-bearing observation (which table a range lands in). The single-table
            # rendering below stays byte-identical (the A/B-measured brittleness case was spans on
            # single-table cards; multi-table sheets had no golds to regress).
            lines.append("Sheet %r contains %d SEPARATE tables:" % (sheet, len(regs)))
            for i, rg in enumerate(regs, 1):
                t = " titled %r" % rg["title"] if rg.get("title") else ""
                lines.append("Table %d%s (headers in row %d, data in rows %d-%d):"
                             % (i, t, rg["header_row"], rg["data_start"], rg["row1"]))
                for c in rg["cols"]:
                    samp = ", ".join(str(s) for s in c["samples"][:3] if s is not None)
                    lines.append("  column %s  header=%r  e.g. %s" % (c["letter"], c["header"], samp or "(empty)"))
                if rg.get("data"):
                    # small table → its rows ARE the observation (a scale/lookup table's semantics)
                    lines.append("  full contents:")
                    for ri, row in enumerate(rg["data"]):
                        cells = ", ".join("%s%d=%r" % (c["letter"], rg["data_start"] + ri, v)
                                          for c, v in zip(rg["cols"], row) if not _blank(v))
                        lines.append("    %s" % (cells or "(empty row)"))
            continue
        # A/B MEASURED 2026-07-03 (prompt-brittleness case study): stating the row SPAN here
        # ("data in rows 2-11") fixed 0326d92d's SUM anchors but DETERMINISTICALLY regressed
        # 37608790 (3/3 gold -> 0/3) and induced off-by-one chart/sort ranges on 3a7c8185.
        # Reverted to the bare count; range robustness is owned DETERMINISTICALLY at apply
        # (sort clamp/widen, chart edge trims) — not by prompt wording.
        lines.append("Sheet %r (%d data rows):" % (sheet, info["rows"]))
        for c in info["cols"]:
            samp = ", ".join(str(s) for s in c["samples"][:3] if s is not None)
            lines.append("  column %s  header=%r  e.g. %s" % (c["letter"], c["header"], samp or "(empty)"))
    return "\n".join(lines)

REASON_PROMPT = (
    # NEUTRAL to the bone: goal + observed data + a generic "think" trigger. NO solution schema (no "which
    # are inputs / target / computation" — that decomposes the problem FOR the model = leading). The model
    # must derive the entire approach itself. Anything beyond a task-agnostic CoT trigger is cheating.
    "You are operating a spreadsheet.\n\n"
    "Goal: {instr}\n\n"
    "Columns present (read from the sheet itself):\n{cards}\n\n"
    "Think step by step, then stop.")

EMIT_PROMPT = (
    "Goal: {instr}\n\n"
    "Columns present:\n{cards}\n\n"
    "Your analysis:\n{reasoning}\n\n"
    "Now emit operations. Refer to columns by NAME in braces: {{Header}} for this sheet, "
    "{{Sheet.Header}} across sheets. Available operations:\n"
    "  compute_column(sheet=\"S\", target=\"Header\", formula=\"={{A}}-{{B}}-...\")  fill the target column\n"
    "  set_cell(sheet=\"S\", cell=\"A1\", value=\"...\")     set one literal cell\n"
    "  add_sheet(name=\"S\")                                add a sheet\n"
    "  rename_sheet(old=\"S\", new=\"S2\")                   rename a sheet\n"
    "  copy_sheet(source=\"S\", new=\"S2\", before=\"S3\")    duplicate a sheet WITH its data, placed before sheet S3 (before=\"\" to append)\n"
    "  total_row(sheet=\"S\", label=\"Total\", columns=\"{{Header1}},{{Header2}}\")  add a row that SUMs each named column\n"
    "  format_cells(sheet=\"S\", range=\"A1:C1\", font_color=\"#rrggbb\", fill_color=\"#rrggbb\", bold=\"true\")  style cells (leave a field \"\" to skip it)\n"
    "  merge_cells(sheet=\"S\", range=\"A1:C1\")             merge a cell range\n"
    "  sort_range(sheet=\"S\", range=\"A1:D9\", key=\"{{Header}}\", order=\"asc\")  sort a range by a column (order asc|desc)\n"
    "  set_number_format(sheet=\"S\", range=\"E2:E9\", format=\"0.00\")  set a number-format code on a range\n"
    "  create_chart(sheet=\"S\", ranges=\"A1:D1;A9:D9\", type=\"line\", title=\"\", data_in=\"rows\")  insert a chart;"
    " ranges=\"categories;values\" as A1 cell ranges (semicolon-separated), type=line|bar|column, data_in=rows|columns\n"
    "  create_pivot(source=\"S\", dest=\"S2\", rows=\"{{Header}}\", cols=\"\", data=\"{{Header}}\", func=\"sum\")  make a Pivot Table;"
    " source=the data sheet, dest=the new sheet for the pivot. rows=field whose values become ROW labels down the"
    " left; cols=field whose values become COLUMN headers across the top (use cols, not rows, when the goal says"
    " the values should be the column headers; cols=\"\" if none); data=field to aggregate; func=sum|count (to COUNT"
    " occurrences of a field, put it in BOTH rows and data with func=\"count\"). Emit a SEPARATE create_pivot for"
    " EACH pivot table the goal asks for (e.g. 'two pivot tables, one per X and one per Y' = two create_pivot calls, each grouping by JUST that one field: rows=\"{{X}}\", cols=\"\" then rows=\"{{Y}}\", cols=\"\")\n"
    "  freeze_panes(sheet=\"S\", range=\"A1:B1\", rows=\"\", cols=\"\")  freeze panes so cells stay visible when scrolling: give range EXACTLY as the goal names the cells to keep visible, OR give rows/cols counts (leave the unused fields \"\")\n"
    "  export_csv(sheet=\"S\", name=\"\")                    export the sheet as a CSV file next to the document (name=\"\" keeps the document's own file name)\n"
    "  transpose_range(sheet=\"S\", source=\"B2:F5\", dest=\"B8\")  paste the source range TRANSPOSED with its top-left cell at dest\n"
    "  reorder_columns(sheet=\"S\", order=\"{{H1}},{{H2}},{{H3}}\")  rearrange existing columns into this left-to-right order — name EVERY column\n"
    "  hide_rows_where(sheet=\"S\", match=\"N/A\")           hide (not delete) every row containing the matched cell text\n"
    "  format_cells_where(sheet=\"S\", match=\"weekend\", fill_color=\"#rrggbb\", font_color=\"\", range=\"\")  style every cell matching a predicate: \"weekend\" = dates on Saturday/Sunday, \"max\" = the largest number, any other match = exact cell text; range=\"{{Header}}\" or A1 range limits the scan (\"\" = whole sheet)\n"
    "  set_decimal_separator(sheet=\"S\", separator=\",\")    display ALL numbers with this decimal separator (localized format; values stay numbers)\n"
    "  export_pdf(sheet=\"S\", name=\"\", fit_pages=\"1\")      export as a PDF next to the document, scaled to fit N pages (name=\"\" keeps the document's file name)\n"
    "  set_zoom(sheet=\"S\", percent=\"100\")               set the sheet's view zoom percentage\n"
    "  dedup_column(sheet=\"S\", source=\"{{Header1}}\", target=\"{{Header2}}\")  copy the source column's UNIQUE values into the target column, keeping first-occurrence order\n"
    "  compute_row(sheet=\"S\", label=\"Growth\", range=\"C13:G13\", formula=\"=C12/B12-1\")  write a ROW of formulas: give the formula for the FIRST cell of the range; it is filled ACROSS with column-shifted references; the label lands in column A of that row (label=\"\" for none)\n"
    "  split_column(sheet=\"S\", source=\"{{Header}}\", delimiter=\" \", targets=\"{{H1}},{{H2}},{{H3}}\")  split each cell of the source column at the delimiter and fill the named target columns with the parts (as many parts as targets; the last target takes the remainder)\n"
    "  infeasible(reason=\"...\")                          ONLY if the request cannot be done in this application at all — emit it ALONE (no other operations) and state why\n\n"
    "Refer to columns by {{Header}} name where a name is asked; use A1 cell refs for ranges. "
    "Emit ONLY the operations the goal needs, as a list of calls:")

# ── ITERATIVE EMISSION (variable-matrix #1, built 2026-07-03 after the compound-collapse was
# measured on 4 independent tasks: reasoning right, the single list-EMIT keeps ~one op and drops
# the model's own remaining steps). One op per call → apply immediately → re-present LIVE state →
# continue or done(). DERIVED from the proven constants so the single-shot floor stays byte-
# identical: same op vocabulary, same docs, only the root rule and the framing differ.
GRAMMAR_STEP = GRAMMAR_B.replace('root ::= "[" op ("," op)* "]"', 'root ::= op | "done()"', 1)
# The FORCED step exists because the model rubber-stamps done() over a detected problem; infeasible()
# is the same escape hatch by another name (measured on 347ef137: forced step aimed at "emit the
# second chart" → infeasible("cannot be done")). A forced step admits neither.
GRAMMAR_STEP_FORCED = GRAMMAR_B.replace('root ::= "[" op ("," op)* "]"', 'root ::= op', 1) \
                               .replace(' | "infeasible(reason=" str ")"', '', 1)

def _ops_doc():
    return EMIT_PROMPT.split("Available operations:\n", 1)[1].rsplit("\nRefer to columns", 1)[0]

# ── COMPACT EMISSION (ADDITIVE, FLAG-GATED — LAGADO_COMPACT_EMIT=1) ─────────────────────────────
# The EMIT stage switches to a positional pipe-separated dialect (one op per line:
# verb|value|value|...) — same verbs, same field ORDER, derived MECHANICALLY from EMIT_PROMPT's
# own signature doc block so the two arms cannot drift apart. Flag OFF: every *_ACTIVE alias
# below binds the proven pythonic constant and the parse dispatcher routes to parse_B_nameops,
# so the default path is behavior-identical (EMIT_PROMPT / GRAMMAR_B themselves are untouched).
COMPACT_EMIT = os.environ.get("LAGADO_COMPACT_EMIT") == "1"

def _field_order():
    """Per-verb ordered field lists, read from EMIT_PROMPT's signature lines (the single source of
    truth for kwarg order). The signature = the `verb(...)` prefix of each ops-doc line; kwargs are
    taken in written order. Bounded at the FIRST ')' so kwarg-shaped text in a description
    (create_pivot's cols=\"\"/func=\"count\" prose) never leaks into the order."""
    orders = {}
    for ln in _ops_doc().split("\n"):
        m = re.match(r'\s*(\w+)\(([^)]*)\)', ln)
        if m:
            orders[m.group(1)] = [k for k, _v in re.findall(r'(\w+)="([^"]*)"', m.group(2))]
    return orders

FIELD_ORDER = _field_order()

def _grammar_verbs():
    """Verbs of the pythonic grammar, in GRAMMAR_B's own order (each op alternative is
    terminal-leading: \"verb(...\")."""
    return re.findall(r'"(\w+)\(', GRAMMAR_B)

def _compact_ops_doc():
    """EMIT_PROMPT's ops-doc block with each signature transformed line by line:
    verb(k=\"P\", k2=\"P2\")  description  ->  verb|P|P2  description
    (placeholders kept, descriptions kept verbatim)."""
    out = []
    for ln in _ops_doc().split("\n"):
        m = re.match(r'(\s*)(\w+)\(([^)]*)\)(.*)$', ln)
        if m:
            indent, verb, sig, rest = m.groups()
            vals = [v for _k, v in re.findall(r'(\w+)="([^"]*)"', sig)]
            ln = indent + verb + "|" + "|".join(vals) + rest
        out.append(ln)
    return "\n".join(out)

def _compact_alt(verb):
    """One line-rule alternative with FIXED arity: \"verb\" \"|\" val \"|\" val ..."""
    return '"%s"' % verb + ' "|" val' * len(FIELD_ORDER[verb])

def _compact_grammar():
    return ('root ::= line ("\\n" line)*\n'
            'line ::= ' + " | ".join(_compact_alt(v) for v in _grammar_verbs()) + "\n"
            'val ::= [^|\\n]*\n')

if COMPACT_EMIT:
    _no_order = [v for v in _grammar_verbs() if v not in FIELD_ORDER]
    assert not _no_order, "compact emission: grammar verbs missing a FIELD_ORDER: %r" % _no_order
    GRAMMAR_B_COMPACT = _compact_grammar()
    # step variants derived the same way the pythonic ones are (root swap; forced drops infeasible)
    GRAMMAR_STEP_COMPACT = GRAMMAR_B_COMPACT.replace(
        'root ::= line ("\\n" line)*', 'root ::= line | "done()"', 1)
    GRAMMAR_STEP_FORCED_COMPACT = GRAMMAR_B_COMPACT.replace(
        'root ::= line ("\\n" line)*', 'root ::= line', 1) \
        .replace(' | ' + _compact_alt("infeasible"), '', 1)
    EMIT_PROMPT_COMPACT = EMIT_PROMPT.replace(_ops_doc(), _compact_ops_doc(), 1).replace(
        "Emit ONLY the operations the goal needs, as a list of calls:",
        "Emit ONLY the operations the goal needs, ONE PER LINE as: verb|value|value|... "
        "(pipe-separated positional values in the documented order; leave a value empty to "
        "skip it; no quotes, no brackets).", 1)
    EMIT_PROMPT_ACTIVE, GRAMMAR_B_ACTIVE = EMIT_PROMPT_COMPACT, GRAMMAR_B_COMPACT
    GRAMMAR_STEP_ACTIVE, GRAMMAR_STEP_FORCED_ACTIVE = GRAMMAR_STEP_COMPACT, GRAMMAR_STEP_FORCED_COMPACT
else:
    EMIT_PROMPT_ACTIVE, GRAMMAR_B_ACTIVE = EMIT_PROMPT, GRAMMAR_B
    GRAMMAR_STEP_ACTIVE, GRAMMAR_STEP_FORCED_ACTIVE = GRAMMAR_STEP, GRAMMAR_STEP_FORCED

def _active_ops_doc():
    return _compact_ops_doc() if COMPACT_EMIT else _ops_doc()

def author_step(instr, g, reasoning, applied, problems, log, forced=False, temperature=0.0):
    """ONE emission step against the LIVE document (act → OBSERVE detected faults → act).
    Returns a nameop, or None for done()/empty. `problems` = the CURRENT detected faults/gaps —
    without them in view the model rubber-stamps done() over an incomplete document (measured).
    forced=True removes the done() escape (used ONCE per loop after a done()-over-problems).
    temperature>0 = the resample diversifier (the deterministic temp-0 draw IS the broken one)."""
    cards = candidate_cards(live_detect(g))
    applied_txt = "\n".join(
        "- %s(%s) — %s" % (o.get("kind"),
                           ", ".join("%s=%s" % (k, v) for k, v in o.items() if k != "kind"), note)
        for o, note in applied) or "(none yet)"
    prompt = ("Goal: {instr}\n\n"
              "Columns present (LIVE — re-read after every applied operation):\n{cards}\n\n"
              "Your analysis:\n{reasoning}\n\n"
              "Operations ALREADY APPLIED to the document:\n{applied}\n\n"
              "PROBLEMS DETECTED in the document right now:\n{problems}\n\n"
              "Available operations:\n" + _active_ops_doc() + "\n"
              "Refer to columns by {{Header}} name where a name is asked; use A1 cell refs for ranges. " +
              ("The detected problems above are UNRESOLVED — emit the ONE operation that addresses "
               "the first of them:" if forced else
               "Emit exactly ONE next operation the goal still needs, or done() when everything the "
               "goal asks is already applied:")).format(
                  instr=instr, cards=cards, reasoning=reasoning, applied=applied_txt,
                  problems=problems or "(none detected)")
    raw = _chat(prompt, grammar=GRAMMAR_STEP_FORCED_ACTIVE if forced else GRAMMAR_STEP_ACTIVE,
                temperature=temperature, seed=7 + int(temperature * 1000), max_tokens=300)
    log.setdefault("step_raw", []).append(raw)
    if raw.strip().startswith("done"):
        return None
    ops = parse_emitted_nameops(raw)
    return ops[0] if ops else None

def emit_per_reasoning_steps(instr, detected, reasoning, word, log):
    """PER-OWN-STEP EMISSION (Pile 2, 2026-07-05 — reason→emit collapse on COUNTED artifacts:
    535364ea's own reasoning plans TWO pivots in its '### Step' sections; the one-pass EMIT keeps
    one and every retry re-collapses). Re-run the SAME EMIT prompt against each of the model's OWN
    reasoning sections that mentions the artifact — every word of context is the model's; the
    harness only segments its text. Zero external content. Returns the artifact ops parsed from
    the segment emissions."""
    segs = [sg for sg in re.split(r"\n(?=#{2,4}\s|\bStep \d)", reasoning or "")
            if word in sg.lower()]
    if len(segs) < 2:
        return []
    cards = candidate_cards(detected)
    kind = "create_pivot" if word == "pivot" else "create_chart"
    out = []
    for sg in segs[:4]:
        raw = _chat(EMIT_PROMPT_ACTIVE.format(instr=instr, cards=cards, reasoning=sg.strip()),
                    grammar=GRAMMAR_B_ACTIVE, temperature=0.0, seed=7, max_tokens=800)
        log.setdefault("seg_emit_raw", []).append(raw[:300])
        for op_ in parse_emitted_nameops(raw):
            if op_.get("kind") == kind:
                out.append(op_)
    return out

def resample_divergence(g, instr, nameops, written, resolve_fails, fired, gaps, log):
    """PREFIX-COMMIT + RESAMPLE-AT-DIVERGENCE (2026-07-05 — the DSpark-shaped loop, user doctrine).
    Applied ops are COMMITTED truth; each LOCALIZED fault is a divergence point that gets ONE
    targeted single-op forced emission against the live document, instead of a full re-derivation
    (the full retry re-derives everything and its op-carrying drags junk forward — measured on
    d681960f, where withheld marks-clobbering ops were re-presented every attempt). Permanently
    rejected ops (overwrite-withheld, structurally unappliable) are DROPPED from the carried list —
    spec decode discards rejected tokens, it does not re-propose them. Paper-aligned mechanics:
    the corrected op ANCHORS the next cycle (DSpark's rejected-token rule), and faults are ORDERED
    by expected fix-rate (resolve fails name exact ops → first; gaps → missing deliverables →
    second; falsifiers → last); per-kind acceptance is logged as calibration data. Faults this
    stage cannot clean fall through to the UNCHANGED full-retry + iterative floor.
    Returns (nameops, written, resolve_fails, fired, gaps)."""
    reasoning = log.get("reasoning", "")
    applied = [(o, "already applied (committed)") for o in nameops]
    # ORDER = CAUSES BEFORE SYMPTOMS (measured on 0a2e43bf: resampling the "chart range empty"
    # fail FIRST patched a lone SUM cell, which poisoned total_row's last-data-row scan — the
    # Total landed one row low and a stable gold regressed). Gaps are missing WRITES (causes);
    # they resample first. An "entirely EMPTY range" fail is a symptom of those missing writes
    # and gets NO resample of its own — the op that owns it is already in nameops and the
    # idempotent dependency re-apply retries it once the writes exist.
    faults = []
    for gp in gaps:
        if gp == "conditional_format":
            continue                              # owned by the pre-apply withhold, not resample
        n = 1
        if gp.startswith(("chart_count:", "pivot_count:")):
            body = gp.partition("|")[0]           # tag may carry existing-item facts after '|'
            n = max(int(body.split(":")[1]) - int(body.split(":")[2]), 1)
        faults.extend([("gap", gp)] * n)
    faults += [("fail", f) for f in resolve_fails if "entirely EMPTY" not in f.get("why", "")]
    faults += [("fired", f) for f in fired]
    steps = 0
    seg_done = False
    for kind, f in faults:
        if steps >= 5:
            break                                 # rail, not a policy — utility scheduling comes later
        if kind == "gap" and str(f).startswith(("chart_count:", "pivot_count:")) and not seg_done:
            # counted-artifact collapse: recover from the model's OWN reasoning sections (its
            # decomposition already exists there — measured), one emission per section.
            seg_done = True
            word = "pivot" if str(f).startswith("pivot") else "chart"
            for nop2 in emit_per_reasoning_steps(instr, live_detect(g), reasoning, word, log):
                if any(_op_key(o) == _op_key(nop2) for o, _n in applied):
                    continue
                w2, f2 = apply_B(g, [nop2], log, instr)
                written += w2
                steps += 1
                applied.append((nop2, "per-step emission: %s" % ("failed" if f2 else "applied")))
                log.setdefault("resample_acc", []).append(["seg", str(f)[:40],
                                                           "fail" if f2 else "applied"])
            continue
        problem = (compose_feedback([f], []) if kind == "fail" else
                   compose_feedback([], [f]) if kind == "fired" else gap_feedback([f]))
        if not problem:
            continue
        nop = author_step(instr, g, reasoning, applied, problem, log, forced=True)
        if nop is not None and any(_op_key(o) == _op_key(nop) for o, _n in applied):
            # the deterministic draw IS the broken/echoed one — diversify (temp mirror of best-of-N)
            nop = author_step(instr, g, reasoning, applied, problem, log, forced=True,
                              temperature=0.35)
        dup = nop is not None and any(_op_key(o) == _op_key(nop) for o, _n in applied)
        if nop is None or nop.get("kind") == "infeasible" or dup:
            log.setdefault("resample_acc", []).append([kind, str(f)[:60], "open"])
            continue                              # divergence stays open for the fallback paths
        if nop.get("kind") == "format_cells" and \
           "conditional_format" in emit_gaps(reasoning, [o for o, _n in applied] + [nop]):
            log.setdefault("resample_acc", []).append([kind, str(f)[:60], "withheld"])
            continue
        w2, f2 = apply_B(g, [nop], log, instr)
        written += w2
        steps += 1
        note = ("resample FAILED: %s" % str(f2[-1].get("why", ""))[:60]) if f2 else "resampled: applied"
        applied.append((nop, note))
        log.setdefault("resample_acc", []).append([kind, str(f)[:60],
                                                   "fail" if f2 else "applied"])
    log["resample_steps"] = steps
    if not steps:
        return nameops, written, resolve_fails, fired, gaps    # nothing resampled — state unchanged
    # DROP permanently rejected ops (never applied, can never apply) before the dependency re-apply
    rejected = set(map(tuple, log.get("rejected_keys", [])))
    def _keyt(o):
        k = _op_key(o)
        return tuple(k) if isinstance(k, (list, tuple)) else (k,)
    nameops = [o for o, _n in applied if _keyt(o) not in rejected]
    # DEPENDENCY RE-APPLY (idempotent, the turn-9 mechanism): a resampled op may satisfy a
    # dependency an earlier fail-closed op was waiting on.
    w3, refails = apply_B(g, merge_nameops([], nameops), log, instr)
    written += w3
    # RE-VERIFY: the observation decides the exit, not the resample's say-so.
    fired = falsify(g, written) + falsify_empty_named_targets(g, instr, nameops) + \
        falsify_style_contract(instr, nameops) + falsify_pivot_orientation(instr, nameops) + falsify_text_decimals(g, instr, written)
    gaps = emit_gaps(log.get("reasoning", ""), nameops, instr)
    if "conditional_format" in gaps:
        gaps.remove("conditional_format")
    return nameops, written, refails, fired, gaps

def emit_gaps(reasoning, nameops, instr=""):
    """EMIT-COMPLETENESS GROUNDING (2026-06-23, the reason→emit bridge — membrane: the reasoning is the model's
    rich representation; the emit is a lossy conversion that can DROP a committed action). Detect actions the
    model's OWN reasoning commits to but the emitted ops don't cover. NOT leading (the model already reasoned
    it) — we hold the model to its own analysis. Returns a list of gap tags. Charts first (the observed gap:
    reasoning describes a 'line chart over B12:G12' but emits only total_row)."""
    r = (reasoning or "").lower()
    gaps = []
    if any(n.get("kind") == "infeasible" for n in nameops):
        return gaps                                   # a declared-infeasible emission gets no op nags
    # Gated on the INSTRUCTION also asking for a chart: reasoning about pivot tables often uses
    # chart vocabulary, and a misfired chart-nag pushes the emission AWAY from the actual goal
    # (measured on 30e3e107 — a pivot task nagged toward create_chart).
    # WORD-BOUNDED chart vocabulary ("Demographic" contains "graph" — measured misfire).
    _chartword = r"\bcharts?\b|\bgraphs?\b|\bplot|\bsparkline|\bbars?\b"
    if re.search(_chartword, r) and re.search(_chartword, (instr or "").lower()) and \
       not any(n.get("kind") == "create_chart" for n in nameops):
        gaps.append("chart")
    # CHART-COUNT completeness (goal-grounded, measured on 347ef137: the goal says "create two column
    # bar charts", the emission carries ONE — compound-collapse dropped the second and nothing
    # detected it). Fires only when the INSTRUCTION itself states an explicit chart count and the
    # emission has fewer DISTINCT create_chart ops. Deterministic string check; a wrong firing nags.
    mcount = re.search(r"\b(two|three|four|2|3|4)\b[^.]{0,40}\bcharts\b", (instr or "").lower())
    if mcount and any(n.get("kind") == "create_chart" for n in nameops):
        wantn = {"two": 2, "three": 3, "four": 4, "2": 2, "3": 3, "4": 4}[mcount.group(1)]
        gotn = len({(n.get("title"), n.get("ranges")) for n in nameops if n.get("kind") == "create_chart"})
        if gotn < wantn:
            have = "; ".join("%r over ranges %s" % (n.get("title") or "(untitled)", n.get("ranges"))
                             for n in nameops if n.get("kind") == "create_chart")
            gaps.append("chart_count:%d:%d|%s" % (wantn, gotn, have))
    # PIVOT-COUNT completeness (Pile 2, 2026-07-05 — the chart_count machinery on its second
    # ≥2-task class: 535364ea "two pivot tables" → one emitted; 30e3e107 "three pivot tables" →
    # one emitted). Goal-stated numeral vs distinct create_pivot ops; fact-only feedback.
    mpc = re.search(r"\b(two|three|four|2|3|4)\b[^.]{0,40}\bpivot tables?\b", (instr or "").lower())
    if mpc and any(n.get("kind") == "create_pivot" for n in nameops):
        wantn = {"two": 2, "three": 3, "four": 4, "2": 2, "3": 3, "4": 4}[mpc.group(1)]
        gotn = len({_op_key(n) for n in nameops if n.get("kind") == "create_pivot"})
        if gotn < wantn:
            have = "; ".join(sorted({"rows=%s cols=%s data=%s" % (n.get("rows"), n.get("cols") or "(none)",
                                     n.get("data")) for n in nameops if n.get("kind") == "create_pivot"}))
            gaps.append("pivot_count:%d:%d|%s" % (wantn, gotn, have))
    if "pivot" in r and not any(n.get("kind") == "create_pivot" for n in nameops):
        gaps.append("pivot")
    # TOTAL-ROW completeness: the model's reasoning commits to a labeled total/sum ROW but no total_row op was
    # emitted (the observed 0a2e43bf miss: it emitted create_chart and DROPPED total_row). Gated tight — needs an
    # explicit row-add phrase AND a total/sum word, and is suppressed on pivot tasks (a pivot owns its own totals)
    # — so it holds the model to its OWN analysis without firing on unrelated 'total' mentions. NOT leading.
    if re.search(r"new row|total row|row called|row named|row labeled|add a row|a row at|row at the bottom"
                 r"|underneath row|row underneath|row beneath|beneath row", r) and \
       any(w in r for w in ("total", "sum")) and \
       not any(n.get("kind") == "total_row" for n in nameops) and \
       not any(n.get("kind") == "create_pivot" for n in nameops):
        gaps.append("total_row")
    # INCOMPLETE-TOTAL completeness (2026-07-10, observed on a Total-row+chart miss): the model emitted an
    # AGGREGATE formula in ONE cell (=SUM(B2:B10) at B12) and its OWN reasoning describes filling it ACROSS the
    # columns ("drag the fill handle down to C12:G12", "for each rep"), but it never emitted the across-fill — so
    # only one column of the total row is written and everything downstream (the chart over that row) is starved.
    # Hold it to its analysis: total_row fills every data column in ONE op. Gated tight — needs a lone-ish
    # aggregate set_cell, NO total_row already, AND an explicit fill-across phrase — so it cannot nag a finished
    # single-value task (those emit compute_column/total_row, not a bare aggregate set_cell). Feedback-only. NOT leading.
    _agg = re.compile(r"=\s*(sum|average|avg|count|counta|max|min)\s*\(", re.I)
    if any(n.get("kind") == "set_cell" and _agg.search(str(n.get("value") or n.get("formula") or ""))
           for n in nameops) and \
       not any(n.get("kind") == "total_row" for n in nameops) and \
       re.search(r"fill handle|drag .{0,25}(across|down|right|to cell|to the|to c[0-9])|"
                 r"copy .{0,25}(across|the formula|to the)|for each (column|rep|month|category|item|row)|"
                 r"across (all|the) (column|cell)|apply .{0,25}to (all|each|every)", r):
        gaps.append("incomplete_total")
    # WRITES-DROPPED: the reasoning ENTERS FORMULAS into cells (the app gesture: "enter the formula
    # =SUM(...)", "type ... in cell") but the emit contains NO cell-writing op at all — the whole
    # computation was lost in the reason→emit conversion (observed 0326d92d: charts emitted, the
    # Total/Growth rows they chart never written). Held to its own analysis; gated on an explicit
    # formula-entry phrase AND zero write ops AND something else emitted (else the empty-emission
    # path already retries).
    # EVERY op that writes cell data counts — an incomplete list here NAGS A FINISHED TASK and the
    # additive retry then injects a damaging extra op (measured: dedup_column done, gap fired,
    # retry added a truncated COUNTIF set_cell that broke sheet_data).
    # format_cells_where added 2026-07-05: a conditional format IS the write the reasoning's
    # condition-formula gestures at — its absence here nagged a FINISHED style-only solution
    # (8b1ce5f2) and the additive retry injected a self-referential set_cell that corrupted B2
    # (the same abed40dc failure mode this comment already warns about).
    write_kinds = ("compute_column", "set_cell", "total_row", "dedup_column", "transpose_range",
                   "compute_row", "split_column",
                   "reorder_columns", "sort_range", "set_decimal_separator", "format_cells_where")
    if nameops and re.search(r"enter the formula|=sum\(|=average\(|type the formula|input the formula"
                             r"|calculate .{0,40}(new column|column)|new column", r) and \
       not any(n.get("kind") in write_kinds for n in nameops):
        gaps.append("writes_dropped")
    # STYLE-DROPPED: the reasoning commits to a highlight/color styling step but the emit contains
    # NO style op at all (observed 21ab7b40: compute emitted, the green-font highlight silently
    # lost — and the claim gate corroborates only WRITTEN cells, so this class can even self-report
    # done). Held to its own analysis.
    if nameops and re.search(r"highlight|font color|background color|\bgreen\b|\bred\b|\bbold\b", r) and \
       not any(n.get("kind") in ("format_cells", "format_cells_where") for n in nameops):
        gaps.append("style_dropped")
    # GOAL-LITERAL completeness (grounded in the INSTRUCTION text itself, not the reasoning):
    # a QUOTED string the goal asks to write ('write "Demographic Profile"') or an EXPLICIT merge
    # range ('Merge cells A1:C1') that no emitted op carries = a missing deliverable. Deterministic
    # string checks; a wrong firing only nags (the op values check covers sheet names etc.).
    if nameops and instr:
        opblob = " ".join(str(v) for o in nameops for v in o.values()).lower()
        # WRITE-VERB-GATED: instructions quote column NAMES constantly ('format column "spent"');
        # only a quote following an explicit write verb is content-to-write. The ungated version
        # nagged name-references into stray set_cells and broke sheet_data on golds (measured,
        # round-2 sweep). Coverage check case-insensitive for the same reason.
        for mlit in re.finditer(r'(?:write|enter|type|says?|labell?ed|titled)\s+"([^"]{3,60})"', instr):
            if mlit.group(1).lower() not in opblob:
                gaps.append("goal_literal:%s" % mlit.group(1))
        mm = re.search(r"[Mm]erge (?:the )?cells? ([A-Za-z]+\d+:[A-Za-z]+\d+)", instr)
        if mm and not any(n.get("kind") == "merge_cells" and
                          (n.get("range") or "").replace("$", "").upper() == mm.group(1).upper()
                          for n in nameops):
            gaps.append("merge_range:%s" % mm.group(1))
    # CONDITIONAL-STYLE fidelity: the reasoning commits to styling only cells matching a CONDITION
    # (weekend days / conditional formatting) but the emit is a blanket format_cells — which would
    # paint EVERY cell in the range and (unlike a missing op) cannot be undone by a retry. Hold the
    # model to its own predicate: withhold the blanket op (the caller does this pre-apply) and ask
    # for format_cells_where. Gated on an explicit conditional phrase in the model's OWN analysis.
    if re.search(r"conditional formatting|weekend|saturday|sunday|the highest|the largest|maximum value", r) and \
       any(n.get("kind") == "format_cells" for n in nameops) and \
       not any(n.get("kind") == "format_cells_where" for n in nameops):
        gaps.append("conditional_format")
    return gaps

def gap_feedback(gaps):
    lines = []
    if "chart" in gaps:
        lines.append("- your analysis describes a CHART but you did NOT emit create_chart(...). Keep your other "
                     "operations and ALSO emit: create_chart(sheet=\"S\", ranges=\"<categories>;<values>\", "
                     "type=\"line|bar|column\", data_in=\"rows\") — ranges are A1 cell ranges (the category/label "
                     "range, then the value range, semicolon-separated), e.g. the header row then the data row.")
    if "pivot" in gaps:
        lines.append("- your analysis describes a PIVOT TABLE but you did NOT emit create_pivot(...). Keep your "
                     "other operations and ALSO emit: create_pivot(source=\"<data sheet>\", dest=\"<new sheet>\", "
                     "rows=\"{Header}\", cols=\"\", data=\"{Header}\", func=\"sum|count\") — name the columns that go "
                     "on each axis; to COUNT occurrences put the field in BOTH rows and data with func=\"count\".")
    if "total_row" in gaps:
        lines.append("- your analysis describes adding a TOTAL/summary row but you did NOT emit total_row(...). "
                     "Keep your other operations and ALSO emit: total_row(sheet=\"S\", label=\"Total\", "
                     "columns=\"{Header1},{Header2}\") — name the columns to SUM into the total row.")
    if "incomplete_total" in gaps:
        lines.append("- you wrote an aggregate formula in ONE cell, but your analysis describes filling it "
                     "ACROSS every data column (a full total row). A single-cell total leaves the other columns "
                     "empty. Emit total_row(sheet=\"S\", label=\"<the row label>\", columns=\"{Header1},{Header2},...\") "
                     "which computes the total for EVERY named column in one operation, instead of the single-cell aggregate.")
    if "conditional_format" in gaps:
        lines.append("- your analysis styles only the cells matching a CONDITION, but format_cells(range=...) "
                     "styles EVERY cell in the range. Emit format_cells_where(sheet=\"S\", match=\"<your "
                     "condition: weekend, or an exact cell text>\", fill_color=\"#rrggbb\", font_color=\"\") "
                     "INSTEAD of format_cells, and keep your other operations.")
    if "style_dropped" in gaps:
        lines.append("- your analysis includes a HIGHLIGHT/styling step but you emitted no style operation. "
                     "Keep your other operations and ALSO emit it: format_cells_where(sheet=\"S\", "
                     "match=\"weekend|max|<exact text>\", fill_color=\"\", font_color=\"#rrggbb\", "
                     "range=\"{Header}\") styles the cells matching your condition (\"max\" = the largest "
                     "number in range); format_cells(sheet=\"S\", range=\"A1:C1\", ...) styles a fixed range.")
    if "writes_dropped" in gaps:
        lines.append("- your analysis ENTERS FORMULAS into cells, but you emitted NO operation that writes "
                     "any cell — the computation never happened. Keep your other operations and ALSO emit "
                     "the writes your analysis describes: total_row(sheet=\"S\", label=\"...\", "
                     "columns=\"{Header1},{Header2}\") computes a SUM row for you (correct rows guaranteed); "
                     "individual formula cells use set_cell(sheet=\"S\", cell=\"A1\", value=\"=FORMULA\").")
    for gp in gaps:
        if gp.startswith("chart_count:"):
            body, _, have = gp.partition("|")
            wantn, gotn = body.split(":")[1], body.split(":")[2]
            lines.append("- the goal asks for %s charts; the document currently has %s DISTINCT "
                         "one(s)%s." % (wantn, gotn, (" — existing: %s" % have) if have else ""))
        elif gp.startswith("pivot_count:"):
            body, _, have = gp.partition("|")
            wantn, gotn = body.split(":")[1], body.split(":")[2]
            lines.append("- the goal asks for %s pivot tables; the emission has %s DISTINCT "
                         "one(s)%s." % (wantn, gotn, (" — existing: %s" % have) if have else ""))
        elif gp.startswith("goal_literal:"):
            lines.append("- the goal asks for the text \"%s\" to be written, but no operation writes it. "
                         "Keep your other operations and ALSO emit set_cell(...) with that exact text at "
                         "the cell the goal names." % gp.split(":", 1)[1])
        elif gp.startswith("merge_range:"):
            lines.append("- the goal asks to merge cells %s but no merge_cells(...) operation does. Keep "
                         "your other operations and ALSO emit merge_cells(sheet=\"S\", range=\"%s\")."
                         % (gp.split(":", 1)[1], gp.split(":", 1)[1]))
    return "\n".join(lines)

def _balance_trailing_parens(f):
    """HARNESS OWNS SYNTAX (Pile 2, 2026-07-05 — 37608790): grammar-constrained draws sometimes
    append surplus trailing ')' (=TRIM(RIGHT(...))))  — LibreOffice stores the unparseable formula
    as TEXT and the column never computes. Strip only SURPLUS closers at the very END, counted
    outside string literals; balanced or under-closed formulas pass through unchanged."""
    if not isinstance(f, str) or not f.lstrip().startswith("="):
        return f
    def balance(s):
        d, inq = 0, False
        for ch in s:
            if ch == '"':
                inq = not inq
            elif not inq:
                d += (ch == "(") - (ch == ")")
        return d
    while balance(f) < 0 and f.rstrip().endswith(")"):
        f = f.rstrip()[:-1]
    return f

def _norm_quotes(f):
    """HARNESS OWNS SYNTAX: LibreOffice string literals need DOUBLE quotes; models emit single —
    and GRAMMAR_B's str rule makes '"' unemittable, so single quotes are the only channel. Convert
    ' -> " EXCEPT apostrophes that QUOTE A SHEET NAME ('Retail Price'!A2:B23 — the cross-sheet
    dialect the formula engine needs intact)."""
    prot = {}
    def _keep(m):
        k = "\x00%d\x00" % len(prot)
        prot[k] = m.group(0)
        return k
    f = re.sub(r"'[^']+'(?=!)", _keep, f)
    f = f.replace("'", '"')
    for k, v in prot.items():
        f = f.replace(k, v)
    return f

def compose_feedback(fails, fired):
    """Turn read-back faults into a concrete correction note for the next emit (the retry condition)."""
    lines = []
    for f in fails:
        why = f.get("why", "")
        if "apply error" in why:
            lines.append("- the formula %r failed to apply (%s). Use DOUBLE quotes for text literals "
                         "(\"_\" not '_'), and valid function/sheet names." % (f.get("name"), why))
        elif "EMPTY" in why:
            # An OBSERVATION fault (e.g. a chart bound to unwritten cells) — relay it verbatim;
            # the name-resolution template below would mislead the retry toward {Sheet.Header}
            # qualification when the actual problem is missing data ops.
            lines.append("- %s." % why)
        elif "0 header matches" in why:
            lines.append("- no column named %r exists on the sheet (%s). If the goal requires that column, "
                         "FIRST emit the operation that creates/fills it (compute_column for a computed "
                         "column), THEN the operation that uses it." % (f.get("name"), why))
        else:
            lines.append("- could not resolve the name %r (%s). A column on ANOTHER sheet must be qualified "
                         "as {Sheet1.Header}; check the exact header spelling." % (f.get("name"), why))
    for f in fired:
        if f["falsifier"] == "error_values":
            hint = ""
            if "#NAME?" in str(f.get("sample", "")):
                hint = (" #NAME? means a function name in the formula is NOT recognized by this "
                        "application — that function does not exist here.")
            lines.append("- the column %s contains error values %s — fix the formula (text literals use "
                         "double quotes; check function names).%s" % (f["range"], f.get("sample"), hint))
        elif f["falsifier"] == "text_formula_numeric":
            lines.append("- the column %s used a text concatenation but produced NUMBERS %s — the text "
                         "literal is wrong; use DOUBLE quotes (\"_\") for the separator." % (f["range"], f.get("sample")))
        elif f["falsifier"] == "extent_shortfall":
            lines.append("- the column %s left %d cells empty — cover every data row." % (f["range"], f.get("empty")))
        elif f["falsifier"] == "named_target_empty":
            lines.append("- the goal names the column %s but it is still ENTIRELY EMPTY — keep your other "
                         "operations and ALSO emit the operation that fills it." % f["range"])
        elif f["falsifier"] == "style_contract":
            lines.append("- the goal names the color %s for the property %s; no applied operation "
                         "sets that property to that color." % tuple(f["range"].split(" ", 1)))
        elif f["falsifier"] == "column_fill_incomplete":
            lines.append("- the goal names the column %s; most of its data rows are still empty." % f["range"])
        elif f["falsifier"] == "text_decimals":
            lines.append("- the written text at %s embeds a number with %s."
                         % (f["range"], f.get("sample")))
        elif f["falsifier"] == "pivot_orientation":
            lines.append("- the goal says a field's values should be COLUMN headers (or row labels); "
                         "the created pivot has %s." % f["range"])
        elif f["falsifier"] == "structural_target_holes":
            lines.append("- the column %s has empty cells in rows where the other columns hold data." % f["range"])
    for f in fired:
        if f.get("rows"):
            for i, row in enumerate(f["rows"]):
                lines.append("- observed row %d near %s: %s" % (i + 1, f["range"], str(row)[:140]))
    return "\n".join(lines)

CHAT = BRAIN.rsplit("/completion", 1)[0] + "/v1/chat/completions"   # applies the GGUF's own chat template (model-agnostic)

def _chat(content, grammar=None, temperature=0.0, seed=7, max_tokens=800):
    """Invoke the model via its native chat template (correct way to address an instruct model — the raw
    /completion call drifts because the model can't find the turn boundary). NO system message added beyond
    the model's own default; the user content is unmodified. grammar is llama.cpp's passthrough extension."""
    body = {"messages": [{"role": "user", "content": content}], "temperature": temperature,
            "seed": seed, "max_tokens": max_tokens}
    if grammar:
        body["grammar"] = grammar
    r = requests.post(CHAT, json=body, timeout=200)
    return r.json()["choices"][0]["message"]["content"]

def static_defects(nameops, instr, detected):
    """STATIC emission defects — internal checks only, the evaluator is NEVER consulted.
    Counts: (1) formula-valued fields with unbalanced parentheses (a truncated draw);
    (2) goal-named EXISTING empty columns no emitted op touches (the coverage gap that cost
    37608790/abed40dc golds on bad draws); (3) an empty emission. Used to choose between
    temp-0 draws (llama.cpp batching makes them vary run-to-run)."""
    if not nameops:
        return 99
    d = 0
    for o in nameops:
        for v in o.values():
            if isinstance(v, str) and v.startswith("=") and v.count("(") != v.count(")"):
                d += 1
    low = (instr or "").lower()
    opblob = " ".join(str(v) for o in nameops for v in o.values()).lower()
    # goal-stated chart count vs drawn charts (measured on 347ef137: the two-chart draw EXISTS at
    # temp-0 — one draw carried both, the next kept one; a shortfall marks the draw defective so
    # best-of-N hunts for the complete one). Internal check only; counts only when charts were drawn.
    mcount = re.search(r"\b(two|three|four|2|3|4)\b[^.]{0,40}\bcharts\b", low)
    if mcount:
        wantn = {"two": 2, "three": 3, "four": 4, "2": 2, "3": 3, "4": 4}[mcount.group(1)]
        gotn = len({(o.get("title"), o.get("ranges")) for o in nameops if o.get("kind") == "create_chart"})
        if 0 < gotn < wantn:
            d += wantn - gotn
    mpc = re.search(r"\b(two|three|four|2|3|4)\b[^.]{0,40}\bpivot tables?\b", low)
    if mpc:
        wantn = {"two": 2, "three": 3, "four": 4, "2": 2, "3": 3, "4": 4}[mpc.group(1)]
        gotn = len({_op_key(o) for o in nameops if o.get("kind") == "create_pivot"})
        if 0 < gotn < wantn:
            d += wantn - gotn
    for sheet, info in (detected or {}).items():
        for c in _sheet_cols(info):
            h = str(c.get("header") or "").strip()
            if len(h) < 3 or h.lower() not in low:
                continue
            samples = c.get("samples") or []
            if samples and any(s not in (None, "") for s in samples):
                continue                          # column already has data
            if h.lower() not in opblob:
                d += 1                            # goal-named empty column left untouched
    return d

def author_B(instr, detected, log, feedback=None, temperature=0.0, additive=False):
    cards = candidate_cards(detected)
    seed = int(temperature * 1000) + 7            # vary seed with temp so the 2nd derivation is independent
    # call 1: REASON (no grammar — free reasoning)
    reasoning = _chat(REASON_PROMPT.format(instr=instr, cards=cards),
                      temperature=temperature, seed=seed, max_tokens=400).strip()
    log.setdefault("reasoning", reasoning)
    # call 2: EMIT (grammar-constrained). On retry, append the specific fault.
    emit = EMIT_PROMPT_ACTIVE.format(instr=instr, cards=cards, reasoning=reasoning)
    if feedback:
        # Two retry stances: CORRECTIVE (fix in place, change nothing else) vs ADDITIVE (the attempt
        # was incomplete — the old "change ONLY what these notes say" preamble actively FORBADE the
        # model from emitting the ops a gap/empty-range note asks it to add; observed on 0326d92d).
        if additive:
            emit += ("\n\nYour PREVIOUS attempt was INCOMPLETE. Keep your operations exactly as "
                     "written AND ALSO emit the operations these notes ask for:\n%s" % feedback)
        else:
            emit += ("\n\nYour PREVIOUS attempt had these problems. Keep your operations EXACTLY as written "
                     "(same verbs, same targets, same structure) and change ONLY what these notes say:\n%s" % feedback)
    raw = _chat(emit, grammar=GRAMMAR_B_ACTIVE, temperature=temperature, seed=seed, max_tokens=800)
    log.setdefault("emit_raw", [])
    log["emit_raw"].append(raw)
    ops = parse_emitted_nameops(raw)
    # STATIC BEST-OF-N (the variance lever, 2026-07-03): a defective draw (truncated formula, goal-
    # named column left untouched, goal-count chart shortfall) costs a gold another draw wins.
    # Re-draw on DETECTED static defects only; keep the fewest-defect draw (tie → earliest). Judged
    # by internal checks alone — the evaluator plays no part. Since --parallel 1 the temp-0 decode
    # is DETERMINISTIC (measured 2026-07-04: 18 byte-identical redraws across seeds), so the
    # re-draws take TEMPERATURE — diversity only ever escalates on a defective temp-0 draw.
    best, best_d = ops, static_defects(ops, instr, detected)
    for t, retemp in ((1, 0.35), (2, 0.7)):
        if best_d == 0:
            break
        raw2 = _chat(emit, grammar=GRAMMAR_B_ACTIVE, temperature=max(temperature, retemp),
                     seed=seed + 1000 * t, max_tokens=800)
        log["emit_raw"].append(raw2)
        ops2 = parse_emitted_nameops(raw2)
        d2 = static_defects(ops2, instr, detected)
        if d2 < best_d:
            best, best_d = ops2, d2
    log.setdefault("emit_defects", []).append(best_d)
    return best

def _nameop_from_kw(verb, kw):
    """kw dict → nameop dict, the SINGLE construction shared by BOTH emission dialects (pythonic
    parse_kv kwargs and compact positional fields) — the compact parser must produce EXACTLY the
    dicts the pythonic parser produces, so the mapping (incl. defaults/coerce) lives once.
    Returns None for an unknown verb or a set_cell with no value."""
    if verb == "add_sheet":
        return {"kind": "add_sheet", "name": kw.get("name")}
    if verb == "rename_sheet":
        return {"kind": "rename_sheet", "old": kw.get("old"), "new": kw.get("new")}
    if verb == "copy_sheet":
        return {"kind": "copy_sheet", "source": kw.get("source"), "new": kw.get("new"),
                "before": kw.get("before", "")}
    if verb == "set_cell" and "value" in kw:
        return {"kind": "set_cell", "sheet": kw.get("sheet"), "cell": kw.get("cell"),
                "value": coerce(kw["value"])}
    if verb == "compute_column":
        return {"kind": "compute_column", "sheet": kw.get("sheet"),
                "target": kw.get("target"), "formula": kw.get("formula", "")}
    if verb == "total_row":
        return {"kind": "total_row", "sheet": kw.get("sheet"),
                "label": kw.get("label", "Total"), "columns": kw.get("columns", "")}
    if verb == "format_cells":
        return {"kind": "format_cells", "sheet": kw.get("sheet"), "range": kw.get("range"),
                "font_color": kw.get("font_color", ""), "fill_color": kw.get("fill_color", ""),
                "bold": kw.get("bold", "")}
    if verb == "merge_cells":
        return {"kind": "merge_cells", "sheet": kw.get("sheet"), "range": kw.get("range")}
    if verb == "sort_range":
        return {"kind": "sort_range", "sheet": kw.get("sheet"), "range": kw.get("range"),
                "key": kw.get("key", ""), "order": kw.get("order", "asc")}
    if verb == "set_number_format":
        return {"kind": "set_number_format", "sheet": kw.get("sheet"),
                "range": kw.get("range"), "format": kw.get("format", "")}
    if verb == "create_chart":
        return {"kind": "create_chart", "sheet": kw.get("sheet"),
                "ranges": kw.get("ranges", ""), "type": kw.get("type", "line"),
                "title": kw.get("title", ""), "data_in": kw.get("data_in", "rows")}
    if verb == "create_pivot":
        return {"kind": "create_pivot", "source": kw.get("source"), "dest": kw.get("dest", "Sheet2"),
                "rows": kw.get("rows", ""), "cols": kw.get("cols", ""),
                "data": kw.get("data", ""), "func": kw.get("func", "sum")}
    if verb == "freeze_panes":
        return {"kind": "freeze_panes", "sheet": kw.get("sheet"), "range": kw.get("range", ""),
                "rows": kw.get("rows", "0"), "cols": kw.get("cols", "0")}
    if verb == "export_csv":
        return {"kind": "export_csv", "sheet": kw.get("sheet"), "name": kw.get("name", "")}
    if verb == "transpose_range":
        return {"kind": "transpose_range", "sheet": kw.get("sheet"),
                "source": kw.get("source", ""), "dest": kw.get("dest", "")}
    if verb == "reorder_columns":
        return {"kind": "reorder_columns", "sheet": kw.get("sheet"), "order": kw.get("order", "")}
    if verb == "hide_rows_where":
        return {"kind": "hide_rows_where", "sheet": kw.get("sheet"), "match": kw.get("match", "")}
    if verb == "format_cells_where":
        return {"kind": "format_cells_where", "sheet": kw.get("sheet"), "match": kw.get("match", ""),
                "fill_color": kw.get("fill_color", ""), "font_color": kw.get("font_color", ""),
                "range": kw.get("range", "")}
    if verb == "set_decimal_separator":
        return {"kind": "set_decimal_separator", "sheet": kw.get("sheet"),
                "separator": kw.get("separator", ",")}
    if verb == "export_pdf":
        return {"kind": "export_pdf", "sheet": kw.get("sheet"), "name": kw.get("name", ""),
                "fit_pages": kw.get("fit_pages", "1")}
    if verb == "set_zoom":
        return {"kind": "set_zoom", "sheet": kw.get("sheet"), "percent": kw.get("percent", "100")}
    if verb == "infeasible":
        return {"kind": "infeasible", "reason": kw.get("reason", "")}
    if verb == "compute_row":
        return {"kind": "compute_row", "sheet": kw.get("sheet"), "label": kw.get("label", ""),
                "range": kw.get("range", ""), "formula": kw.get("formula", "")}
    if verb == "split_column":
        return {"kind": "split_column", "sheet": kw.get("sheet"), "source": kw.get("source", ""),
                "delimiter": kw.get("delimiter", " "), "targets": kw.get("targets", "")}
    if verb == "dedup_column":
        return {"kind": "dedup_column", "sheet": kw.get("sheet"),
                "source": kw.get("source", ""), "target": kw.get("target", "")}
    return None

def parse_B_nameops(text):
    """Parse name-level calls (UNRESOLVED — names stay in {braces}). Resolution happens at APPLY time
    against the live re-detected structure, so new sheets / just-set headers resolve."""
    nameops = []
    verbs = ("compute_column", "set_cell", "add_sheet", "rename_sheet", "copy_sheet", "total_row",
             "format_cells", "merge_cells", "sort_range", "set_number_format", "create_chart", "create_pivot",
             "freeze_panes", "export_csv", "transpose_range", "reorder_columns", "hide_rows_where",
             "format_cells_where", "set_decimal_separator", "export_pdf", "infeasible", "dedup_column",
             "set_zoom", "compute_row", "split_column")
    for verb, body in scan_calls(text, verbs):
        op = _nameop_from_kw(verb, parse_kv(body))
        if op is not None:
            nameops.append(op)
    return nameops

def parse_compact_nameops(text):
    """COMPACT-dialect parser: one op per line, `verb|value|value|...` positional in FIELD_ORDER.
    Blank lines skipped; trailing whitespace tolerated; a done() line (step grammar) is skipped.
    Same construction as the pythonic parser via _nameop_from_kw."""
    nameops = []
    for ln in text.split("\n"):
        ln = ln.rstrip()
        if not ln.strip() or ln.strip().startswith("done"):
            continue
        parts = ln.split("|")
        verb = parts[0].strip()
        fields = FIELD_ORDER.get(verb)
        if fields is None:
            continue
        kw = {f: parts[i + 1] for i, f in enumerate(fields) if i + 1 < len(parts)}
        op = _nameop_from_kw(verb, kw)
        if op is not None:
            nameops.append(op)
    return nameops

def parse_emitted_nameops(text):
    """DISPATCHER — every emission-parse site (single-shot, best-of-N redraw, per-step, per-segment)
    routes through here; the flag picks the dialect. nameops shape identical either way."""
    return parse_compact_nameops(text) if COMPACT_EMIT else parse_B_nameops(text)

def _op_key(o):
    """Identity of an op for cross-attempt dedup. Same key = the same intended op (a later attempt CORRECTS
    it); a different key = an ADDITIONAL op (kept). Distinct pivots differ by their field spec, so two pivots
    on one sheet are not collapsed."""
    k = o.get("kind")
    if k == "create_pivot":
        return (k, o.get("dest"), o.get("rows"), o.get("cols"), o.get("data"))
    if k in ("compute_column", "set_cell"):
        return (k, o.get("sheet"), o.get("target") or o.get("cell"))
    if k in ("format_cells", "merge_cells", "set_number_format", "sort_range"):
        return (k, o.get("sheet"), o.get("range"))
    if k == "add_sheet":
        return (k, o.get("name"))
    if k == "rename_sheet":
        return (k, o.get("old"), o.get("new"))
    if k == "copy_sheet":
        return (k, o.get("source"), o.get("new"))
    if k in ("export_csv", "export_pdf"):
        return (k, o.get("sheet"), o.get("name"))
    if k == "transpose_range":
        return (k, o.get("sheet"), o.get("source"), o.get("dest"))
    if k in ("hide_rows_where", "format_cells_where"):
        return (k, o.get("sheet"), o.get("match"))
    if k == "dedup_column":
        return (k, o.get("sheet"), o.get("source"), o.get("target"))
    if k == "create_chart":
        # keyed by TITLE when present (two titled charts coexist; a retry correcting the SAME
        # chart's ranges collides and replaces it). Untitled charts key by RANGES so two untitled
        # charts (per-year bars) coexist — the cost: an untitled retry with changed ranges adds a
        # second chart instead of replacing (visible MISS, never a false pass).
        return (k, o.get("sheet"), o.get("title") or o.get("ranges") or "")
    # total_row, freeze_panes, reorder_columns, set_decimal_separator: one per sheet
    return (k, o.get("sheet"))

def merge_nameops(carried, new):
    """Retain ops the model committed in an EARLIER attempt but DROPPED on a gap/fault retry. The reason->emit
    conversion is lossy: it silently sheds an un-nagged op (observed 0a2e43bf — emits create_chart, then on the
    total_row nag re-emits only total_row, losing the chart). Union carried+new keyed by op identity (new wins
    on collision = a correction); charts/pivots go LAST so they bind to the now-final data (a chart over a total
    row needs the row to exist first). NOTE: interface-plane repair of ONE loss class (dropped ops) — it patches
    the conversion tax, it does NOT remove the serialization, so it is not 'the membrane'."""
    by_key, order = {}, []
    for o in (carried + new):
        key = _op_key(o)
        if key not in by_key:
            order.append(key)
        by_key[key] = o
    merged = [by_key[k] for k in order]
    # Ops that must see the FINAL data: charts/pivots bind to it, an export snapshots it.
    viz = ("create_chart", "create_pivot", "export_csv", "export_pdf")
    return [o for o in merged if o.get("kind") not in viz] + [o for o in merged if o.get("kind") in viz]

CHART_TYPE_PHRASES = [
    # VERB-DIALECT grounding for the chart TYPE (turn-5 lesson: the goal's own phrasing wins over our
    # enum translation; measured on 347ef137 — the goal's "column bar charts" is the standard name for
    # a VERTICAL bar chart, the model translated it to type="bar" = horizontal). Longest phrase first;
    # only an EXACT "<kind> chart(s)" phrase in the goal overrides a CONFLICTING emitted type.
    # Class B, ablatable.
    ("column bar chart", "column"), ("vertical bar chart", "column"), ("horizontal bar chart", "bar"),
    ("column chart", "column"), ("bar chart", "bar"), ("line chart", "line"), ("pie chart", "pie"),
]

def ground_chart_type(nop, instr):
    low = (instr or "").lower()
    for phrase, ctype in CHART_TYPE_PHRASES:
        if phrase in low or phrase.replace("chart", "charts") in low:
            if nop.get("type") and nop["type"] != ctype:
                nop["type"] = ctype
            return

def apply_B(g, nameops, log, instr=""):
    """Interleaved apply: each op applies through the session, then we RE-DETECT so later ops resolve
    against the live world. compute_column names are resolved here (exact+unique or FAIL-CLOSED).
    Returns (written_regions, fails) — written_regions = [(sheet, a1range)] for read-back."""
    live = live_detect(g)
    written, fails = [], []
    _wpause = os.environ.get("LAGADO_WATCH_PAUSE")     # watch-mode: pace each op so a human can see it land
    for nop in nameops:
        if _wpause:
            try:
                print("    [apply] %s %s" % (nop.get("kind"),
                      {kk: vv for kk, vv in nop.items() if kk != "kind"}), flush=True)
                time.sleep(float(_wpause))
            except Exception:
                pass
        k = nop["kind"]
        # Single-book placeholder tolerance for EVERY op: a draw that copies the docs' "S"
        # placeholder (or any unknown name) binds to the only live sheet — the daemon has this
        # tolerance, but host-side resolution paths (total_row's column resolve, dedup, sort key)
        # silently lost the op without it (measured: total_row sheet='S' → "unknown sheet 'S'").
        # Only EXISTING-sheet fields; never new-name fields (dest/new/name keep the model's word).
        if len(live) == 1:
            bindable = ("sheet", "source") if k in ("create_pivot", "copy_sheet") else ("sheet",)
            for f in bindable:
                if nop.get(f) and nop[f] not in live:
                    nop[f] = list(live)[0]
        for f in EXISTING_SHEET_FIELDS.get(k, []):       # ground existing-sheet refs to live tabs (grounding)
            if nop.get(f):
                nop[f] = ground_sheet(nop[f], live)
        if k == "add_sheet":
            if nop["name"] not in live:                 # idempotent (safe to re-run on retry)
                g.client("apply", {"op": {"op": "add_sheet", "name": nop["name"]}})
                live = live_detect(g)
        elif k == "rename_sheet":
            if nop.get("old") != nop.get("new"):
                g.client("apply", {"op": {"op": "rename_sheet", "old": nop["old"], "new": nop["new"]}})
                live = live_detect(g)
        elif k == "copy_sheet":
            if nop.get("source") in live and nop.get("new") not in live:   # idempotent (safe on retry)
                g.client("apply", {"op": {"op": "copy_sheet", "source": nop["source"], "new": nop["new"],
                                          "before": nop.get("before", "")}})
                live = live_detect(g)
        elif k == "set_cell":
            v = nop["value"]
            if isinstance(v, str) and v.startswith("="):
                v = _balance_trailing_parens(_norm_quotes(v))   # same syntax ownership as compute
            # DUPLICATE-HEADER WITHHOLD (Pile 2, 2026-07-05 — 37608790: the model wrote 'First
            # Name'/'Last Name' into B2/C2, one row BELOW the real headers, then computed around
            # them). A TEXT set equal to the target column's own detected header, landing anywhere
            # but that header's row, duplicates structure the sheet already declares — withheld,
            # fact relayed. Ablatable.
            if isinstance(v, str) and v.strip() and not v.startswith("="):
                mtc = re.match(r"([A-Za-z]+)(\d+)$", (nop.get("cell") or "").replace("$", ""))
                info_t = live.get(nop.get("sheet")) or {}
                if mtc:
                    tl, trow = mtc.group(1).upper(), int(mtc.group(2))
                    rg_t = _range_region(info_t, nop["cell"])
                    hrow_t = rg_t["header_row"] if rg_t else info_t.get("header_row", 1)
                    cands_t = (rg_t or info_t).get("cols", [])
                    hdr_t = next((c["header"] for c in cands_t if c["letter"] == tl), "")
                    if trow != hrow_t and str(hdr_t).strip() and \
                       str(hdr_t).strip().casefold() == v.strip().casefold():
                        log.setdefault("rejected_keys", []).append(_op_key(nop))
                        fails.append({"name": nop.get("cell"),
                                      "why": "not applied: %r is already the header of column %s (row %d)"
                                             % (v.strip(), tl, hrow_t)})
                        continue
            # GOAL-ECHO WITHHOLD (Pile 2, 2026-07-05 — 1334ca3e: no verb existed for a zoom request,
            # so the model fabricated the request's own words into virgin A1, corrupting an otherwise
            # untouched sheet). A multi-word TEXT whose words ALL come from the instruction, and which
            # the instruction does NOT quote as content-to-write, is narration — not document content.
            # Withheld, fact relayed. Ablatable.
            if isinstance(v, str) and v.strip() and not v.startswith("="):
                words_v = re.findall(r"[a-z]{3,}", v.lower())
                low_i = (instr or "").lower()
                quoted = [q.lower() for q in re.findall(r'"([^"]{3,60})"', instr or "")]
                if len(words_v) >= 3 and all(w in low_i for w in words_v) and \
                   v.strip().lower() not in quoted:
                    log.setdefault("rejected_keys", []).append(_op_key(nop))
                    fails.append({"name": nop.get("cell"),
                                  "why": "not applied: %r repeats the instruction wording; the "
                                         "instruction does not ask for this text in a cell" % v.strip()})
                    continue
            # OVERWRITE WITHHOLD — MULTI-TABLE SHEETS ONLY (measured on d681960f: the model wrote a
            # grade literal INTO the Marks column, clobbering observed data on a task that says
            # "don't touch irrelevant regions"). A set_cell landing on a cell that already holds a
            # DIFFERENT non-blank value is withheld for the whole run, the observed value relayed.
            # Fail-closed on purpose: a re-emission channel was tried and measured UNSOUND — retry's
            # op-carrying re-presents withheld ops verbatim, indistinguishable from deliberate
            # re-emission, and the marks got clobbered anyway. Region-gated so the single-table
            # floor is byte-identical; ablatable.
            if len((live.get(nop.get("sheet"), {}).get("regions") or [])) > 1 and \
               _op_key(nop) not in [tuple(k) for k in log.get("applied_set_keys", [])]:
                # applied_set_keys guard: an op WE already applied this run re-arriving via the
                # idempotent dependency re-apply is OUR OWN write — the cell now holds the formula's
                # RESULT, which never string-matches the op's formula (measured on 7e429b8d: the
                # committed F2 VLOOKUP got withheld as "overwriting" its own displayed value and
                # then dropped as junk). First-writes still withhold; re-applies never do.
                rr0 = g.client("read", {"sheet": nop["sheet"],
                                        "range": "%s:%s" % (nop["cell"], nop["cell"])})
                cur = ((rr0.get("cells") or [[None]])[0] or [None])[0]
                if not _blank(cur) and str(cur) != str(nop.get("value")):
                    log.setdefault("overwrite_withheld", []).append(
                        ["set_cell", nop.get("sheet"), nop.get("cell"), str(nop.get("value"))])
                    # PERMANENT withhold → a REJECTED op in the prefix-commit sense: record its key
                    # so the resample stage can DROP it instead of carrying it forever (the measured
                    # junk-drag: merge re-presented withheld marks-clobbering ops every attempt).
                    log.setdefault("rejected_keys", []).append(_op_key(nop))
                    # FACT-ONLY feedback (user flag 2026-07-05: tuning this wording to steer the next
                    # emission = leading with prompts; the earlier "write to an EMPTY cell instead"
                    # phrasing induced a value="" clear — which the withhold itself already blocks.
                    # The harness relays the observation; the mechanism owns the protection).
                    fails.append({"name": nop.get("cell"),
                                  "why": "not applied: cell %s on %s already holds %r"
                                         % (nop["cell"], nop["sheet"], cur)})
                    continue
            # FILL-SHAPE GROUNDING (Class B, ablatable; multi-table sheets only this round). A
            # FORMULA set into a table column's FIRST data cell, carrying a same-row RELATIVE
            # scalar reference, above an entirely-EMPTY remaining span, is the app's own
            # enter-formula-then-fill-down gesture — the model narrates "drag the fill handle",
            # an intent no emitted op carries (measured, 7e429b8d). The harness owns the geometry
            # the fill handle would: set_formula_range over the table's data span (relative refs
            # adjust exactly as fillAuto). Guards make it unable to clobber: empty-below only,
            # region-top only, ≥3-row span only; aggregates (=AVERAGE(E2:E12)) never match — a
            # range ref is not a same-row scalar.
            filled = False
            mcell = re.match(r"([A-Za-z]+)(\d+)$", (nop.get("cell") or "").replace("$", ""))
            if isinstance(v, str) and v.startswith("=") and mcell and \
               len((live.get(nop.get("sheet"), {}).get("regions") or [])) > 1:
                rgs = _range_region(live.get(nop.get("sheet")), nop["cell"])
                rowc = int(mcell.group(2))
                if rgs and rowc == rgs["data_start"] and rgs["row1"] - rowc >= 2:
                    same_row_scalar = any(
                        m.group(2) == "" and int(m.group(3)) == rowc
                        for m in re.finditer(r"(?<![:$\w])([A-Za-z]{1,3})(\$?)(\d+)(?![:\w])", v))
                    below = "%s%d:%s%d" % (mcell.group(1), rowc + 1, mcell.group(1), rgs["row1"])
                    rrb = g.client("read", {"sheet": nop["sheet"], "range": below})
                    empties = [x for row in (rrb.get("cells") or []) for x in row]
                    if same_row_scalar and empties and all(_blank(x) for x in empties):
                        span = "%s%d:%s%d" % (mcell.group(1), rowc, mcell.group(1), rgs["row1"])
                        rr = g.client("apply", {"op": {"op": "set_formula_range",
                                                       "sheet": nop["sheet"], "range": span,
                                                       "formula": v}})
                        if rr.get("ok"):
                            written.append((nop["sheet"], span, v))
                            filled = True
            # ROW-FILL-SHAPE (Pile 2, 2026-07-05 — 0326d92d: the TRANSPOSE of the fill-handle
            # gesture). A FORMULA seeded in a row DIRECTLY UNDER the table's last data row, whose
            # relative refs all live in the seed's own COLUMN (=SUM(B2:B10) at B12), with every
            # cell to its right across the table's columns EMPTY, is the app's fill-RIGHT gesture.
            # Same guards as the column form: empty-only span, adjacent-row-only, refs elsewhere
            # never match. Works on single-table sheets too (flat geometry).
            if not filled and isinstance(v, str) and v.startswith("=") and mcell:
                info_r = live.get(nop.get("sheet")) or {}
                rowc = int(mcell.group(2))
                seedc = mcell.group(1).upper()
                rg_r = next((cr for cr in (info_r.get("regions") or []) if cr["row1"] + 1 == rowc), None)
                last_used = rg_r["row1"] if rg_r else info_r.get("rows", 0)
                colset = (rg_r or info_r).get("cols", [])
                letters_r = [c["letter"] for c in colset]
                if last_used and rowc == last_used + 1 and seedc in letters_r and \
                   letters_r.index(seedc) < len(letters_r) - 1:
                    refs = re.findall(r"(?<![:$\w])([A-Za-z]{1,3})\$?\d+(?![\w])", v)
                    refs_ok = bool(refs) and all(r_.upper() == seedc for r_ in refs)
                    endc = letters_r[-1]
                    right = "%s%d:%s%d" % (col_letter(_col_idx(seedc) + 1), rowc, endc, rowc)
                    rrr = g.client("read", {"sheet": nop["sheet"], "range": right})
                    rvals = [x for row in (rrr.get("cells") or []) for x in row]
                    if refs_ok and rvals and all(_blank(x) for x in rvals):
                        # AGGREGATE-EXTENT alignment (sort-clamp family; measured 0326d92d: the
                        # model's =SUM(C2:C10) under a table whose data ends at row 11 skips the
                        # last data row). A range in the seed's own column that starts at the
                        # table's data start and ends short extends to rowc-1 — geometry, not
                        # semantics: the seed sits directly UNDER the table it aggregates.
                        ds_r = (rg_r or {}).get("data_start", info_r.get("data_start", 2))
                        def _extend(mr):
                            c1_, r1_, c2_, r2_ = mr.group(1), int(mr.group(2)), mr.group(3), int(mr.group(4))
                            if c1_.upper() == seedc and c2_.upper() == seedc and \
                               r1_ == ds_r and r1_ < r2_ < rowc - 1:
                                return "%s%d:%s%d" % (c1_, r1_, c2_, rowc - 1)
                            return mr.group(0)
                        v = re.sub(r"([A-Za-z]{1,3})(\d+):([A-Za-z]{1,3})(\d+)", _extend, v)
                        span = "%s%d:%s%d" % (seedc, rowc, endc, rowc)
                        rr = g.client("apply", {"op": {"op": "set_formula_range",
                                                       "sheet": nop["sheet"], "range": span,
                                                       "formula": v}})
                        if rr.get("ok"):
                            written.append((nop["sheet"], span, v))
                            filled = True
            if not filled:
                rr = g.client("apply", {"op": {"op": "set", "sheet": nop["sheet"], "cell": nop["cell"],
                                               "value": v}})
                # plain single-cell writes were INVISIBLE to the read-back net (falsify/corroborate
                # scan `written`; only the fill-shape expansions appended) — measured on 4f07fbe9:
                # the goal-named decimal contract could not fire on a write it never saw
                if rr.get("ok"):
                    written.append((nop["sheet"], nop["cell"], v))
            log.setdefault("applied_set_keys", []).append(list(_op_key(nop)))
            live = live_detect(g)  # a set_cell may have written a header → re-perceive
        elif k == "compute_column":
            sheet, target, formula = nop["sheet"], nop["target"], nop["formula"]
            # HARNESS OWNS SYNTAX: LibreOffice string literals need double quotes; LLMs often emit single
            # ('_'), which silently evaluates to 0. Quoted SHEET names ('Retail Price'!...) are protected.
            # And a SPACED live sheet name emitted bare (Retail Price!A:B) gets its required quotes.
            for s_ in live:
                if " " in s_ and ("%s!" % s_) in formula and ("'%s'!" % s_) not in formula:
                    formula = formula.replace("%s!" % s_, "'%s'!" % s_)
            formula = _balance_trailing_parens(_norm_quotes(formula))
            # GROUND the model's NATURAL column references (2026-06-23): it names columns by the header it
            # perceived; we bind those names against the LIVE structure rather than demand the {brace} dialect.
            # ground_bare_refs braces only SOUND occurrences (guarded for literals/function-position/longest-
            # first); the resolver below still owns binding soundness (unique-or-fail-closed). No prompt, no
            # grammar, no retry-nag, no training — the model's first natural emission is accepted as-is.
            formula = ground_bare_refs(formula, sheet, live)
            tres = resolve_col(sheet, target, live, [], write_target=True)   # throwaway fails — unresolved target → create
            tcol, treg = (tres if tres else (None, None))
            if tcol is None and len((live.get(sheet, {}).get("regions") or [])) > 1:
                # LIVE-READ write-target disambiguation (Class B, ablatable; the resolve-time samples
                # window goes stale the moment an earlier op writes into the column — measured on
                # 7e429b8d: attempt-0's F2 formula made the form column look non-empty). Among the
                # duplicate-header hits, a column whose region span is EMPTY or filled ONLY in its top
                # cell is the one fill that clobbers nothing; exactly one such column binds.
                want = (target or "").strip().lower()
                hits = [c for c in _sheet_cols(live.get(sheet, {}))
                        if c["header"].strip().lower() == want]
                fillable = []
                for c in hits:
                    rgc = _region_of_col(live.get(sheet), c["letter"], region_hint=c.get("region"))
                    if not rgc:
                        continue
                    rr0 = g.client("read", {"sheet": sheet, "range": "%s%d:%s%d"
                                            % (c["letter"], rgc["data_start"], c["letter"], rgc["row1"])})
                    vals = [v for row in (rr0.get("cells") or []) for v in row]
                    if vals and (all(_blank(v) for v in vals) or
                                 (not _blank(vals[0]) and all(_blank(v) for v in vals[1:]))):
                        fillable.append(c)
                if len(fillable) == 1:
                    tcol, treg = fillable[0]["letter"], fillable[0].get("region")
            if tcol is None:
                if len((live.get(sheet, {}).get("regions") or [])) > 1:
                    # multi-table sheet: a flat append would land the new column outside every table —
                    # fail-closed; the resolve fail drives the retry feedback instead. Structurally
                    # unappliable → rejected in the prefix-commit sense (resample may drop it).
                    log.setdefault("rejected_keys", []).append(_op_key(nop))
                    fails.append({"name": target, "sheet": sheet,
                                  "why": "target not found on a multi-table sheet (no auto-create)"})
                    live = live_detect(g)
                    continue
                tcol = create_target_column(g, sheet, target, live)
                live = live_detect(g)
            rg = _region_of_col(live.get(sheet), tcol, region_hint=treg)
            regs = live.get(sheet, {}).get("regions") or []
            ridx = treg if treg is not None else (regs.index(rg) if rg in regs else None)
            # header-row-aware first data row — the target's own TABLE on a multi-table sheet
            ds = rg["data_start"] if rg else live.get(sheet, {}).get("data_start", 2)
            refsheets = set()
            a1 = substitute_names(formula, sheet, live, fails, row=ds, refsheets=refsheets, region=ridx)
            # HARNESS OWNS SYNTAX: a compute_column body is ALWAYS a formula. The model inconsistently
            # omits the leading '=' (e.g. "{Sales}-{Sales Return}"); without it setFormula stores the
            # string as TEXT and fillAuto then series-increments the trailing digit ("B2-C2"→"B2-C3"…),
            # so the column never computes. Guarantee the '='. (VM-verified 2026-06-23: '=' present →
            # correct relative fill 75000,69539,…; absent → text series. fillAuto itself is fine.)
            if a1 is not None and not a1.lstrip().startswith("="):
                a1 = "=" + a1.lstrip()
            if a1 is None:
                continue  # fail-closed: a referenced name didn't resolve
            # UNIT-IN-FORMAT normalization (Pile 1 — the ground_result_date_type family; 21df9241:
            # the evaluator compares dtype; gold stores NUMBERS with the unit in the FORMAT, the
            # model emits ROUND(...)&" M" TEXT). Exactly that shape becomes the numeric ROUND with
            # the suffix as a number-format code — value unchanged, only its TYPE. Ablatable.
            unit_fmt = None
            mu = re.match(r'^=\s*ROUND\((.+?)\s*/\s*(1000000000|1000000|1000)\s*,\s*(\d+)\s*\)'
                          r'\s*&\s*"([^"]{1,8})"\s*$', a1)
            if mu:
                # v2 (gold-form verified 21df9241): "change the REPRESENTATION" is the app's
                # format-scaling — value stays the RAW expression, the format's comma count IS the
                # divisor (each ',' ÷1000). Derived entirely from the model's own formula.
                nd = int(mu.group(3))
                commas = {"1000": ",", "1000000": ",,", "1000000000": ",,,"}[mu.group(2)]
                unit_fmt = "0" + ("." + "0" * nd if nd else "") + commas + ('"%s"' % mu.group(4))
                a1 = "=" + mu.group(1).strip()
            else:
                mu2 = re.match(r'^=\s*(ROUND\(.+,\s*(\d+)\s*\))\s*&\s*"([^"]{1,8})"\s*$', a1)
                if mu2:
                    nd = int(mu2.group(2))
                    unit_fmt = "0" + ("." + "0" * nd if nd else "") + ('"%s"' % mu2.group(3))
                    a1 = "=" + mu2.group(1)
            # Extent = data rows of the target OR any sheet the formula references (row-aligned). A
            # fresh target sheet has only its header (1 row); the referenced data sheet sets the span.
            # On a multi-table sheet the target's own TABLE bounds the span — the flat extent would
            # run the fill into the next stacked table.
            if rg:
                extent = rg["row1"]
            else:
                cand = [live.get(sheet, {}).get("rows", 2)] + [live.get(s, {}).get("rows", 2) for s in refsheets]
                extent = max([r for r in cand if r and r >= 2] or [2])
            rng = "%s%d:%s%d" % (tcol, ds, tcol, extent)
            rr = g.client("apply", {"op": {"op": "set_formula_range", "sheet": sheet,
                                           "range": rng, "formula": a1}})
            if not rr.get("ok"):                        # apply-time error (bad formula syntax, etc.)
                fails.append({"name": a1, "range": rng, "why": "apply error: %s" % rr.get("error", "")[:80]})
                continue
            written.append((sheet, rng, a1))
            if unit_fmt:
                g.client("apply", {"op": {"op": "set_number_format", "sheet": sheet,
                                          "range": rng, "format": unit_fmt}})
            ground_result_date_type(g, sheet, a1, rng, live)
            live = live_detect(g)
        elif k == "total_row":
            # HARNESS-LEVEL verb (expands into existing `set` ops — daemon untouched): write the label in
            # the first column of the row UNDER the data, then SUM each named column over the data span.
            # Find the TRUE last data row by scanning a sum column (detect's row count can overshoot into
            # blank rows, which would place the total one row too low — verified on the op-probe).
            sheet = nop.get("sheet")
            info = live.get(sheet, {})
            ds = info.get("data_start", 2)
            cinfo = info.get("cols", [])
            first_letter = cinfo[0]["letter"] if cinfo else "A"
            toks = [t.strip().strip("{}").strip() for t in (nop.get("columns") or "").split(",") if t.strip()]
            rescols = [resolve_col(sheet, t, live, fails) for t in toks]
            letters = [rc[0] for rc in rescols if rc]
            probe_col = letters[0] if letters else first_letter
            # multi-table: the named columns' OWN table bounds the scan and anchors the label column —
            # a flat 500-row scan would walk into the next stacked table and sum it too
            trg = _region_of_col(info, probe_col, region_hint=next((rc[1] for rc in rescols if rc), None))
            scan_end = ds + 500
            if trg:
                ds = trg["data_start"]
                scan_end = trg["row1"]
                first_letter = trg["cols"][0]["letter"]
            label = nop.get("label", "Total")
            rb = g.client("read", {"sheet": sheet, "range": "%s%d:%s%d" % (probe_col, ds, probe_col, scan_end)})
            vals = [row[0] if row else None for row in rb.get("cells", [])] if rb.get("ok") else []
            # read the label column too so a re-apply (op-accumulation) OVERWRITES a prior total row instead of
            # stacking a new one below it: a row whose first cell already == the label is a previous total, not data.
            lbcol = g.client("read", {"sheet": sheet, "range": "%s%d:%s%d" % (first_letter, ds, first_letter, scan_end)})
            firsts = [row[0] if row else None for row in lbcol.get("cells", [])] if lbcol.get("ok") else []
            last = ds - 1
            for i, v in enumerate(vals):
                fv = firsts[i] if i < len(firsts) else None
                if v is not None and v != "" and str(fv).strip() != str(label).strip():
                    last = ds + i
            if last < ds:                                   # nothing read — fall back to detect's count
                last = ds + info.get("rows", 1) - 1
            trow = last + 1
            g.client("apply", {"op": {"op": "set", "sheet": sheet, "cell": "%s%d" % (first_letter, trow),
                                      "value": label}})
            for letter in letters:
                f = "=SUM(%s%d:%s%d)" % (letter, ds, letter, last)
                cell = "%s%d" % (letter, trow)
                rr = g.client("apply", {"op": {"op": "set", "sheet": sheet, "cell": cell, "formula": f}})
                if rr.get("ok"):
                    written.append((sheet, cell, f))
            live = live_detect(g)
        elif k == "compute_row":
            # HARNESS-LEVEL row-of-formulas verb (2026-07-06, 0326d92d class: month-on-month growth
            # ROW — compute_column covers columns, nothing covered rows). The model gives the FIRST
            # cell's formula; the harness fills the rest of the range with column-shifted copies
            # (deterministic host-side re-referencing, never fillAuto). Label lands in column A.
            sheet = nop.get("sheet")
            rng = (nop.get("range") or "").strip().replace("$", "")
            m = re.match(r"([A-Za-z]+)(\d+):([A-Za-z]+)(\d+)$", rng)
            base = (nop.get("formula") or "").strip()
            if not m or not base or m.group(2) != m.group(4):
                fails.append({"name": rng or "(range)", "why": "compute_row needs a single-row A1 "
                              "range like C13:G13 and the first cell's formula"})
            else:
                c0, row_n, c1 = m.group(1).upper(), int(m.group(2)), m.group(3).upper()
                def _cn(cs):
                    n = 0
                    for ch in cs:
                        n = n * 26 + (ord(ch) - 64)
                    return n
                n0, n1 = _cn(c0), _cn(c1)
                if not base.startswith("="):
                    base = "=" + base
                label = (nop.get("label") or "").strip()
                if label:
                    g.client("apply", {"op": {"op": "set", "sheet": sheet,
                                              "cell": "A%d" % row_n, "value": label}})
                for off in range(0, n1 - n0 + 1):
                    f = _shift_a1_cols(base, off)
                    nn, out = n0 + off, ""
                    k2 = nn
                    while k2:
                        k2, r2 = divmod(k2 - 1, 26)
                        out = chr(65 + r2) + out
                    cell = "%s%d" % (out, row_n)
                    rr = g.client("apply", {"op": {"op": "set", "sheet": sheet, "cell": cell, "formula": f}})
                    if rr.get("ok"):
                        written.append((sheet, cell, f))
                    else:
                        fails.append({"name": cell, "why": "apply error: %s" % rr.get("error", "")[:80]})
            live = live_detect(g)
        elif k == "split_column":
            # HARNESS-LEVEL text-split verb (2026-07-06, 37608790 class: 'information mixed in one
            # field'). Read the source column, split each cell at the delimiter into as many parts
            # as there are named targets (last target keeps the remainder), write each part under
            # its target header. Deterministic host-side; targets resolve like any named column.
            sheet = nop.get("sheet")
            info = live.get(sheet, {})
            ds = info.get("data_start", 2)
            delim = nop.get("delimiter") or " "
            src_tok = (nop.get("source") or "").strip().strip("{}").strip()
            src = resolve_col(sheet, src_tok, live, fails)
            tg_toks = [t.strip().strip("{}").strip() for t in (nop.get("targets") or "").split(",") if t.strip()]
            tgs = [resolve_col(sheet, t, live, fails) for t in tg_toks]
            if src and all(tgs) and tg_toks:
                sletter, sreg = src
                trg = _region_of_col(info, sletter, region_hint=sreg)
                row0 = trg["data_start"] if trg else ds
                row1 = trg["row1"] if trg else ds + max(info.get("rows", 1), 1) - 1
                rb = g.client("read", {"sheet": sheet, "range": "%s%d:%s%d" % (sletter, row0, sletter, row1)})
                vals = [row[0] if row else None for row in rb.get("cells", [])] if rb.get("ok") else []
                nparts = len(tgs)
                for i, v in enumerate(vals):
                    if v is None or str(v).strip() == "":
                        continue
                    parts = str(v).split(delim, nparts - 1)
                    for (tl, _tr), part in zip(tgs, parts):
                        cell = "%s%d" % (tl, row0 + i)
                        val = part.strip()
                        num = re.match(r"^-?\d+(\.\d+)?$", val)
                        op2 = {"op": "set", "sheet": sheet, "cell": cell}
                        if num:
                            op2["formula"] = "=" + val
                        else:
                            op2["value"] = val
                        rr = g.client("apply", {"op": op2})
                        if rr.get("ok"):
                            written.append((sheet, cell, val))
            live = live_detect(g)
        elif k in ("format_cells", "merge_cells", "set_number_format", "freeze_panes", "export_csv",
                   "transpose_range", "reorder_columns", "hide_rows_where", "format_cells_where",
                   "set_decimal_separator", "export_pdf", "set_zoom"):
            if k == "format_cells_where":
                # A {Header} (or bare header) range resolves to that column's data span — the model
                # names the column it means; the harness owns the geometry (same fail-open contract
                # as ground_bare_refs: unresolved → scan the whole sheet as before).
                rspec = (nop.get("range") or "").strip()
                if rspec and not re.match(r"^[A-Za-z]+\d+(:[A-Za-z]+\d+)?$", rspec.replace("$", "")):
                    fres = resolve_col(nop.get("sheet"), rspec.strip("{}").strip(), live, [])
                    letter, freg = (fres if fres else (None, None))
                    info = live.get(nop.get("sheet")) or {}
                    frg = _region_of_col(info, letter, region_hint=freg) if letter else None
                    if letter and frg:                  # multi-table: the named column's OWN table span
                        nop["range"] = "%s%d:%s%d" % (letter, frg["data_start"], letter, frg["row1"])
                    elif letter and info:
                        ds = info.get("data_start", 2)
                        nop["range"] = "%s%d:%s%d" % (letter, ds, letter, ds + info.get("rows", 0) - 1)
                    else:
                        nop["range"] = ""
            op = {"op": k}
            op.update({kk: vv for kk, vv in nop.items() if kk != "kind"})
            if k in ("format_cells", "set_number_format"):       # not merge (a merge range is intentional);
                                                                 # not freeze/export/transpose (no data range /
                                                                 # explicit source+dest are intentional)
                op["range"] = clamp_range_to_data(op.get("range"), nop.get("sheet"), live)
            rr = g.client("apply", {"op": op})
            if not rr.get("ok"):
                fails.append({"name": nop.get("range"), "why": "apply error: %s" % rr.get("error", "")[:80]})
            live = live_detect(g)
        elif k == "dedup_column":
            sheet = nop.get("sheet")
            sres = resolve_col(sheet, (nop.get("source") or "").strip("{}").strip(), live, fails)
            tgt = resolve_name(sheet, (nop.get("target") or "").strip("{}").strip(), live, fails)
            src = sres[0] if sres else None
            if src is None or tgt is None:
                live = live_detect(g)
                continue                                # fail-closed; the resolve fail drives retry
            info = live.get(sheet) or {}
            drg = _region_of_col(info, src, region_hint=sres[1])
            ds = drg["data_start"] if drg else info.get("data_start", 2)
            r1 = drg["row1"] if drg else ds + info.get("rows", 0) - 1
            rr = g.client("apply", {"op": {"op": "dedup_column", "sheet": sheet, "source": src,
                                           "target": tgt, "row0": ds, "row1": r1}})
            if not rr.get("ok"):
                fails.append({"name": nop.get("target"), "why": "apply error: %s" % rr.get("error", "")[:80]})
            live = live_detect(g)
        elif k == "sort_range":
            sheet = nop.get("sheet")
            rng = (nop.get("range") or "").replace("$", "")
            # ROW-INTEGRITY grounding: sorting a SUBSET of the used columns tears rows apart
            # (observed: range A1:E36 on a 6-column sheet left column F unsorted — silent data
            # corruption). The app's own sort extends the selection to the whole table; mirror
            # that: WIDEN the range's column span to the sheet's used columns (rows untouched,
            # never shrink). Deterministic geometry, no task knowledge.
            info = live.get(sheet) or {}
            srg = _range_region(info, rng)              # multi-table: widen/clamp within the range's OWN table
            scols = srg["cols"] if srg else info.get("cols", [])
            letters = [c.get("letter") for c in scols if c.get("letter")]
            m0 = re.match(r"([A-Za-z]+)(\d+):([A-Za-z]+)(\d+)$", rng)
            if m0 and letters:
                lo = min(letters + [m0.group(1).upper()], key=_col_idx)
                hi = max(letters + [m0.group(3).upper()], key=_col_idx)
                # ...and CLAMP the end row to the observed data extent: an over-long range drags
                # empty rows into the sort and scrambles them against the real data (observed:
                # a 2-37 range on 2-36 data). Never clamp above the model's start row.
                last = srg["row1"] if srg else info.get("data_start", 2) + info.get("rows", 0) - 1
                end = int(m0.group(4))
                if int(m0.group(2)) <= last < end:
                    end = last
                rng = "%s%s:%s%d" % (lo, m0.group(2), hi, end)
                nop["range"] = rng
            key = (nop.get("key") or "").strip().strip("{}").strip()
            op = {"op": "sort_range", "sheet": sheet, "range": nop.get("range"),
                  "ascending": "true" if (nop.get("order", "asc") or "asc").lower().startswith("a") else "false"}
            m = re.match(r"([A-Za-z]+)(\d+):([A-Za-z]+)\d+$", rng)
            if m:
                start_col = _col_idx(m.group(1))
                op["has_header"] = "true" if int(m.group(2)) == 1 else "false"
                kl = resolve_name(sheet, key, live, [])
                if kl:
                    op["key_index"] = _col_idx(kl) - start_col
            rr = g.client("apply", {"op": op})
            if not rr.get("ok"):
                fails.append({"name": key or rng, "why": "apply error: %s" % rr.get("error", "")[:80]})
            live = live_detect(g)
        elif k == "create_chart":
            # Host-side sheet tolerance (mirrors the daemon's resolve_sheet): an unknown/placeholder
            # sheet ("S") on a single-sheet book binds to the only live sheet — every read/extent
            # lookup below depends on it (an unresolved name here anchored a chart to the header row).
            if nop.get("sheet") not in live and len(live) == 1:
                nop["sheet"] = list(live)[0]
            ground_chart_type(nop, instr)               # goal-phrase dialect ("column bar chart" = vertical)
            grounded = ground_chart_ranges(nop.get("ranges", ""), nop.get("sheet"), live)
            # ORIENTATION from geometry: the ranges themselves declare it — all single-column parts
            # = column series, all single-row parts = row series. The model's data_in label is used
            # only when the shape is ambiguous (observed: column ranges labeled data_in="rows").
            gparts = [p.strip().replace("$", "") for p in grounded.split(";") if p.strip()]
            gm = [re.match(r"([A-Za-z]+)(\d+):([A-Za-z]+)(\d+)$", p) for p in gparts]
            data_in = nop.get("data_in", "rows")
            if gparts and all(m and m.group(1).upper() == m.group(3).upper() for m in gm):
                data_in = "columns"
            elif gparts and all(m and m.group(2) == m.group(4) for m in gm):
                data_in = "rows"
            def _reparse():
                return [re.match(r"([A-Za-z]+)(\d+):([A-Za-z]+)(\d+)$", p) for p in gparts]
            # COLUMN-part trailing trim: a vertical series that runs past the data's end (off-by-one
            # emissions) charts empty cells and shifts the saved refs off the gold — clamp each
            # column part's end to its last non-empty cell. Observation-based, end-only.
            if gparts and all(m and m.group(1).upper() == m.group(3).upper() for m in gm):
                newparts = []
                for p, m in zip(gparts, gm):
                    rr_ = g.client("read", {"sheet": nop.get("sheet"), "range": p})
                    colvals = [row[0] if row else None for row in (rr_.get("cells") or [])]
                    filled = [i for i, v in enumerate(colvals) if v not in (None, "")]
                    if filled and filled[-1] < len(colvals) - 1:
                        p = "%s%s:%s%d" % (m.group(1), m.group(2), m.group(3),
                                           int(m.group(2)) + filled[-1])
                    newparts.append(p)
                gparts = newparts
                grounded = ";".join(gparts)
                gm = _reparse()
            # COLUMN-pair SPAN UNIFICATION (measured on 347ef137): cat A3:A14 (label tail trimmed —
            # the totals row has a blank label) with val I3:I15 (the grand total IS numeric, so the
            # empty-trim keeps it) = mismatched series lengths; LO then saves a chart the evaluator
            # can't match. Two vertical parts sharing a start row must be row-aligned — both end at
            # the SHORTER extent (rows without a category label aren't categories). Mirror of the
            # row-pair unification below.
            if len(gm) == 2 and all(m and m.group(1).upper() == m.group(3).upper() for m in gm) and \
               gm[0].group(2) == gm[1].group(2) and gm[0].group(4) != gm[1].group(4):
                lo_end = min(int(gm[0].group(4)), int(gm[1].group(4)))
                gparts = ["%s%s:%s%d" % (m.group(1), m.group(2), m.group(3), lo_end) for m in gm]
                grounded = ";".join(gparts)
                gm = _reparse()
            # ROW-pair SPAN UNIFICATION: same start column, different end columns (cat B1:G1 with
            # val B12:F12 — a sloppy draw) dodges the same-span grounding below and saves refs the
            # evaluator can't match (measured, round-2 sweep). Rebuild both parts over the UNION
            # span; the numeric edge-trim below then clamps to what actually exists.
            if len(gm) == 2 and all(m and m.group(2) == m.group(4) for m in gm) and \
               gm[0].group(1).upper() == gm[1].group(1).upper() and \
               gm[0].group(3).upper() != gm[1].group(3).upper():
                hi = max(gm[0].group(3).upper(), gm[1].group(3).upper(), key=_col_idx)
                gparts = ["%s%s:%s%s" % (m.group(1), m.group(2), hi, m.group(4)) for m in gm]
                grounded = ";".join(gparts)
                gm = _reparse()
            # ROW-pair grounding to observed data. (1) A value row that is entirely EMPTY while the
            # sheet has a real bottom data row = the model mis-anchored the row (observed: chart at
            # row 13 for a total row written at 12) — re-anchor the value row to the live last data
            # row (Class B, ablatable). (2) EDGE-TRIM both parts' column span to the value row's
            # numeric extent: value series are NUMBERS — an edge cell that is empty (a Growth row
            # skipping Jan) or text (the row's own label) belongs outside the series.
            if len(gm) == 2 and all(m and m.group(2) == m.group(4) for m in gm) and \
               gm[0].group(1) == gm[1].group(1) and gm[0].group(3) == gm[1].group(3):
                info_ = live.get(nop.get("sheet")) or {}
                crg = _range_region(info_, gparts[0])   # multi-table: re-anchor within the chart's OWN table
                cds = crg["data_start"] if crg else info_.get("data_start", 2)
                last_row = crg["row1"] if crg else info_.get("data_start", 2) + info_.get("rows", 0) - 1
                rr_ = g.client("read", {"sheet": nop.get("sheet"), "range": gparts[1]})
                row_ = (rr_.get("cells") or [[]])[0]
                if info_ and row_ and all(v in (None, "") for v in row_) and \
                   int(gm[1].group(2)) != last_row and last_row >= cds:
                    gparts[1] = "%s%d:%s%d" % (gm[1].group(1), last_row, gm[1].group(3), last_row)
                    grounded = ";".join(gparts)
                    gm = _reparse()
                    rr_ = g.client("read", {"sheet": nop.get("sheet"), "range": gparts[1]})
                    row_ = (rr_.get("cells") or [[]])[0]
                numeric = [i for i, v in enumerate(row_)
                           if v not in (None, "") and isinstance(v, (int, float))]
                if numeric and (numeric[0] > 0 or numeric[-1] < len(row_) - 1):
                    b0 = _col_idx(gm[0].group(1))
                    lo_l, hi_l = col_letter(b0 + numeric[0]), col_letter(b0 + numeric[-1])
                    gparts = ["%s%s:%s%s" % (lo_l, m.group(2), hi_l, m.group(4)) for m in gm]
                    grounded = ";".join(gparts)
                    gm = _reparse()
            # EMPTY-RANGE FALSIFIER (fail-closed): a chart binding cells that hold NOTHING is an
            # objective world-state fault (observed: charting a Total row that was never written).
            # Don't create it; the fault feedback drives the retry to write the data it described.
            empty_part = None
            for p in gparts:
                r_ = g.client("read", {"sheet": nop.get("sheet"), "range": p})
                vals = [v for row in (r_.get("cells") or []) for v in row]
                if vals and all(v in (None, "") for v in vals):
                    empty_part = p
                    break
            if empty_part:
                fails.append({"name": empty_part, "why": "chart range %s is entirely EMPTY — the data it "
                              "should display has not been written; emit the operations that produce those "
                              "cells (keep the chart too)" % empty_part})
                live = live_detect(g)
                continue
            # deterministic chart name: per TITLE when present (retry with same title REPLACES
            # itself via uno_ops remove+add); untitled falls back to the ranges signature so two
            # untitled charts coexist.
            cname = "CHT_" + (re.sub(r"\W+", "_", nop.get("title") or "") or
                              re.sub(r"\W+", "_", grounded) or "default")
            op = {"op": "create_chart", "sheet": nop.get("sheet"), "ranges": grounded,
                  "type": nop.get("type", "line"), "title": nop.get("title", ""),
                  "data_in": data_in, "name": cname}
            # EXTENT-AWARE PLACEMENT (2026-07-04, user direction): a chart goes BESIDE the data —
            # right of the used columns, vertically aligned with its own range's top row (matches
            # how the human-made golds sit; two per-table charts land beside their own tables
            # instead of stacking on the data). Position is unscored; this is document hygiene.
            info_p = live.get(nop.get("sheet")) or {}
            mtop = re.match(r"[A-Za-z]+(\d+)", gparts[0]) if gparts else None
            op["anchor_col"] = len(info_p.get("cols", [])) + 1      # one clear column right of the data
            op["anchor_row"] = max((int(mtop.group(1)) if mtop else 1) - 1, 0)
            rr = g.client("apply", {"op": op})
            if not rr.get("ok"):
                fails.append({"name": nop.get("ranges"), "why": "apply error: %s" % rr.get("error", "")[:80]})
            live = live_detect(g)
        elif k == "create_pivot":
            # Resolve each field NAME to a 0-based column index against the SOURCE sheet (fail-closed: an
            # unresolved field aborts the pivot rather than building a wrong one). uno_ops then builds the
            # DataPilot; the source range is auto-detected there. Count-by-self = the same column landing in
            # both rows and data (the model is told to do this for func="count").
            source = nop.get("source")
            if source not in live:                          # model omitted/missed it → the data sheet (not dest)
                source = next((s for s in live if s != nop.get("dest")), source)
            dest = nop.get("dest") or "Sheet2"
            def _idxs(spec):
                out = []
                for t in (spec or "").split(","):
                    t = t.strip().strip("{}").strip()
                    if not t:
                        continue
                    letter = resolve_name(source, t, live, fails)
                    if letter is None:
                        return None                         # fail-closed on any unresolved field
                    out.append(_col_idx(letter))
                return out
            rows_i, cols_i, data_i = _idxs(nop.get("rows")), _idxs(nop.get("cols")), _idxs(nop.get("data"))
            if rows_i is None or cols_i is None or data_i is None:
                live = live_detect(g); continue
            # deterministic name keyed to the field spec → a re-apply (op-accumulation) OVERWRITES the same
            # pivot instead of creating a duplicate; two DISTINCT pivots still get distinct names. (Name is not
            # scored — the evaluator keys pivots by source range/fields, not name.)
            sig = lambda xs: "-".join(map(str, xs)) or "x"
            pvt_name = "PVT_%s_%s_%s_%s" % (source, sig(rows_i), sig(cols_i), sig(data_i))
            op = {"op": "create_pivot", "source_sheet": source, "dest_sheet": dest, "name": pvt_name,
                  "row_fields": rows_i, "col_fields": cols_i, "data_fields": data_i,
                  "data_func": (nop.get("func") or "sum")}
            rr = g.client("apply", {"op": op})
            if not rr.get("ok"):
                fails.append({"name": "pivot %s" % dest, "why": "apply error: %s" % rr.get("error", "")[:80]})
            live = live_detect(g)
    log["resolve_fails"] = fails
    log["written_regions"] = written
    return written, fails

NAME_TOK = re.compile(r"\{([^}]*)\}")

EMB_URL = BRAIN.rsplit("/completion", 1)[0] + "/v1/embeddings"   # the brain serves embeddings too (--embeddings --pooling last)
SEM_THETA = 0.08                                  # min top1-top2 cosine margin to bind (else fail-closed/abstain)
_emb_cache = {}

def _embed(text):
    """Embed via the BRAIN's OWN latent space (last-token pooling). Returns a vector or None if embeddings are
    not enabled on the server (graceful degradation → semantic fallback simply no-ops, lexical path unchanged)."""
    if text in _emb_cache:
        return _emb_cache[text]
    try:
        r = requests.post(EMB_URL, json={"input": text}, timeout=20).json()
        v = r["data"][0]["embedding"]
    except Exception:
        v = None
    _emb_cache[text] = v
    return v

def _cos(a, b):
    d = sum(x * y for x, y in zip(a, b)); na = sum(x * x for x in a) ** 0.5; nb = sum(y * y for y in b) ** 0.5
    return d / (na * nb) if na and nb else 0.0

def semantic_col(sheet, name, live):
    """LATENT-BINDING FALLBACK (2026-06-23, the membrane's first inward rung — FUTURE_RESEARCH R1b). Fires ONLY
    after lexical resolution (exact header / letter / index) has FAILED. Binds the model's natural reference to
    the nearest live header IN THE BRAIN'S OWN EMBEDDING SPACE (cosine), but ONLY when the top match is UNIQUE
    and beats the runner-up by SEM_THETA — else returns None (FAIL-CLOSED, abstain, exactly where overlapping or
    terse headers make it ambiguous). SEPARATE deterministic resolver, NEVER injected into the action prompt
    (inv #10). No-ops cleanly if the server has no embeddings endpoint. Returns a column letter or None."""
    info = live.get(sheet)
    if not info:
        return None
    # VALUE-AS-REFERENCE REJECTION (Pile 2, 2026-07-05 — abed40dc): a token equal to an OBSERVED
    # cell value is a value-reference, not a column-reference; binding it to a column is a mis-bind
    # by construction (measured: source="Keira Daily" — a data cell — latent-bound to a column and
    # the dedup landed rows off). Lexical header matches never reach here; this only guards the
    # semantic fallback. Fail-closed: the fact lands in feedback and the retry re-references.
    want_v = name.strip().casefold()
    observed = set()
    for c in _sheet_cols(info):
        for s_ in (c.get("samples") or []):
            if isinstance(s_, str) and s_.strip():
                observed.add(s_.strip().casefold())
    for rg_ in (info.get("regions") or []):
        for row_ in (rg_.get("data") or []):
            for s_ in row_:
                if isinstance(s_, str) and s_.strip():
                    observed.add(s_.strip().casefold())
    if want_v in observed:
        return None
    cols = [(c["letter"], c["header"].strip()) for c in _sheet_cols(info) if c["header"].strip()]
    if not cols:
        return None
    qv = _embed(name.strip())
    if qv is None:
        return None
    scored = []
    for letter, hdr in cols:
        hv = _embed(hdr)
        if hv is None:
            return None
        scored.append((_cos(qv, hv), letter))
    scored.sort(reverse=True)
    if len(scored) == 1:
        return scored[0][1] if scored[0][0] >= 0.30 else None      # lone header: absolute floor
    if scored[0][0] - scored[1][0] >= SEM_THETA:
        return scored[0][1]
    return None

def resolve_name(sheet, name, detected, fails, region=None):
    """Exact, unique header match → column letter. Ambiguous/missing → None (fail-closed, logged)."""
    res = resolve_col(sheet, name, detected, fails, region)
    return res[0] if res else None

def resolve_col(sheet, name, detected, fails, region=None, write_target=False):
    """resolve_name with region identity: → (letter, region_idx|None). On a multi-table sheet the search
    space is the UNION of region candidates; a `region` context restricts duplicate headers to that table
    FIRST (row-aligned binding — a compute over table 2 must mean table 2's 'Marks'), else duplicates
    across tables stay fail-closed exactly like duplicates within a sheet."""
    if name is None:
        return None
    if name.strip().startswith("#"):                      # candidate-selection by index
        res = _index_col(sheet, name.strip()[1:], detected, fails)
        return (res[1], None) if res else None
    want = name.strip().lower()
    info = detected.get(sheet)
    if not info:
        fails.append({"name": name, "why": "unknown sheet %r" % sheet}); return None
    cands = _sheet_cols(info)
    hits = [c for c in cands if c["header"].strip().lower() == want]
    if len(hits) > 1 and region is not None:
        rhits = [c for c in hits if c.get("region") == region]
        if rhits:
            hits = rhits
    if len(hits) > 1 and write_target:
        # Class B grounding (ablatable, WRITE targets only — 7e429b8d: 'Officer Name' heads both the
        # filled lookup table AND the empty form column): among duplicate headers, exactly ONE column
        # observed empty = the only fill that doesn't clobber source data — bind to it. Read targets
        # never take this path (they stay fail-closed on duplicates).
        empt = [c for c in hits if all(_blank(s) for s in (c.get("samples") or [None]))]
        if len(empt) == 1:
            hits = empt
    if len(hits) == 1:
        return (hits[0]["letter"], hits[0].get("region"))
    lc = _letter_col(sheet, name.strip(), detected)    # column-letter target notation
    if lc:
        return (lc[1], region)
    sem = semantic_col(sheet, name, detected)          # latent-binding fallback (margin-gated, fail-closed)
    if sem:
        return (sem, None)
    fails.append({"name": name, "sheet": sheet, "why": "%d header matches (need exactly 1)" % len(hits)})
    return None

def create_target_column(g, sheet, name, live):
    """If a compute_column targets a column that doesn't exist, CREATE it (deterministic): place at the
    first empty-header column else append one past the last, set its header at the sheet's header row.
    'Fill the X column' shouldn't depend on the model remembering to add the header. Target-only — input
    references never auto-create (they stay fail-closed, preserving the no-mis-bind invariant)."""
    info = live.get(sheet, {})
    cols = info.get("cols", [])
    hrow = info.get("header_row", 1)
    letter = next((c["letter"] for c in cols if not str(c["header"]).strip()), None)
    if letter is None:
        letter = col_letter(len(cols))            # next column after the last
    g.client("apply", {"op": {"op": "set", "sheet": sheet, "cell": "%s%d" % (letter, hrow), "value": name}})
    return letter

def _index_col(sheet, idx_str, detected, fails):
    """Candidate-selection by index: {#N} → the Nth (1-based) detected column of `sheet`. Always lands on
    a REAL column (or fail-closed if out of range) — the model cannot invent a column."""
    info = detected.get(sheet)
    cols = _sheet_cols(info)
    try:
        n = int(idx_str)
    except (ValueError, TypeError):
        fails.append({"name": "#%s" % idx_str, "why": "non-integer index"}); return None
    if info and 1 <= n <= len(cols):
        return (sheet, cols[n - 1]["letter"])
    fails.append({"name": "#%s" % idx_str, "sheet": sheet, "why": "index out of range"}); return None

def _letter_col(sheet, token, detected):
    """Column-LETTER notation: {B} → column B IF B is a real column of `sheet` and no header equals 'B'.
    Letters are unambiguous (unique by construction) → sound, never a mis-bind. Returns (sheet,letter)|None."""
    info = detected.get(sheet)
    if not info or not re.fullmatch(r"[A-Za-z]+", token):
        return None
    L = token.upper()
    cols = _sheet_cols(info)
    letters = [c["letter"] for c in cols]
    if L in letters and not any(c["header"].strip().upper() == L for c in cols):
        return (sheet, L)
    return None

def resolve_ref(token, default_sheet, detected, fails, region=None):
    """Resolve a formula reference to (sheet, letter), accepting ANY unambiguous notation — the harness
    owns notation so the model's choice of style can't break correctness. Order per sheet: exact unique
    HEADER → column LETTER ({B}) → index ({#N}); bare names also try WORKBOOK-WIDE unique header. Ambiguous
    /missing → None (fail-closed, logged). Sound: letters+indices are unique; headers fail-closed on dup."""
    res = resolve_ref_full(token, default_sheet, detected, fails, region)
    return (res[0], res[1]) if res else None

def resolve_ref_full(token, default_sheet, detected, fails, region=None):
    """resolve_ref with region identity: → (sheet, letter, region_idx|None). `region` = the referencing
    op's table context on a multi-table sheet — duplicate headers bind within it first (row-aligned),
    else fail-closed as ever."""
    token = token.strip()
    if "." in token:
        sh, _, hdr = token.partition(".")
        sh = sh.strip(); hdr = hdr.strip()
        if hdr.startswith("#"):
            res = _index_col(sh, hdr[1:], detected, fails)
            return (res[0], res[1], None) if res else None
        info = detected.get(sh)
        if not info:
            fails.append({"name": token, "why": "unknown sheet %r" % sh}); return None
        hits = [c for c in _sheet_cols(info) if c["header"].strip().lower() == hdr.lower()]
        if len(hits) == 1:
            return (sh, hits[0]["letter"], hits[0].get("region"))
        lc = _letter_col(sh, hdr, detected)
        if lc:
            return (lc[0], lc[1], None)
        fails.append({"name": token, "sheet": sh, "why": "%d header matches (need 1)" % len(hits)}); return None
    if token.startswith("#"):
        res = _index_col(default_sheet, token[1:], detected, fails)
        return (res[0], res[1], None) if res else None
    want = token.lower()
    info = detected.get(default_sheet)
    if info:
        hits = [c for c in _sheet_cols(info) if c["header"].strip().lower() == want]
        if len(hits) > 1 and region is not None:
            rhits = [c for c in hits if c.get("region") == region]
            if rhits:
                hits = rhits
        if len(hits) == 1:
            return (default_sheet, hits[0]["letter"], hits[0].get("region"))
        if len(hits) > 1:
            fails.append({"name": token, "sheet": default_sheet, "why": "%d on-sheet matches" % len(hits)}); return None
    allhits = [(s, c["letter"], c.get("region")) for s, i in detected.items()
               for c in _sheet_cols(i) if c["header"].strip().lower() == want]
    if len(allhits) == 1:
        return allhits[0]
    lc = _letter_col(default_sheet, token, detected)   # column-letter notation, default sheet
    if lc:
        return (lc[0], lc[1], region)
    sem = semantic_col(default_sheet, token, detected)  # latent-binding fallback (margin-gated, fail-closed)
    if sem:
        return (default_sheet, sem, None)
    fails.append({"name": token, "why": "%d workbook matches (need exactly 1)" % len(allhits)}); return None

CELLREF = re.compile(r"(?:([^\s!(){}'\"+\-*/,=<>]+)!)?\$?([A-Za-z]{1,3})\$?(\d+)")

def ground_result_date_type(g, sheet, a1, rng, live):
    """GROUND THE OUTPUT TYPE (L2, 2026-06-23, user direction — the same move as ground_bare_refs, applied to
    the RESULT). The model correctly computes a maturity DATE but stores a bare serial; the evaluator compares
    by dtype (pandas Timestamp vs float), so a correct value in the wrong type mismatches. Rather than make the
    model remember to format (human-like procedure-recall) or parse the operation symbolically, we REACT TO
    PRESENT STATE: a result column the sheet DECLARES as a date (target or a referenced source header carries a
    date word, OR a referenced column's live number-format is date-typed) whose values are valid non-trivial
    date serials IS a date → format it so. NOTE this file imports its dates as General serials (LibreOffice
    drops the xlsx date format on load), so the DECLARED NAME is the surviving signal — the structural format
    perception stays as belt-and-suspenders for files that keep it. Self-falsifying on values: date−days→~120
    fails the ≥1000 floor → correctly stays numeric. Reads the RESOLVED A1 refs (covers braced-then-resolved
    header refs and raw A2-style refs). Only ACTS on a positive match; silent + harmless otherwise."""
    seen = set()
    def hdr_of(s2, l2):
        col = next((c for c in _sheet_cols(live.get(s2, {})) if c["letter"] == l2), None)
        return (col.get("header") if col else "") or ""
    def is_date_word(h):
        return "date" in h.lower()
    target_letter = re.match(r"([A-Za-z]+)", rng).group(1)
    # GROUND on what the sheet DECLARES + what it PERCEIVES: a column named "…Date" (target or a referenced
    # source) is a date column, OR a referenced column whose live number-format is date-typed (when LibreOffice
    # preserves it — this file imports the dates as General, so the declared name is the surviving signal).
    declared_date = is_date_word(hdr_of(sheet, target_letter))
    for m in CELLREF.finditer(a1):
        s2 = (m.group(1) or sheet).strip()
        l2 = m.group(2).upper()
        if (s2, l2) in seen:
            continue
        seen.add((s2, l2))
        col = next((c for c in _sheet_cols(live.get(s2, {})) if c["letter"] == l2), None)
        if col and (col.get("ntype") == "date" or is_date_word(col.get("header") or "")):
            declared_date = True
    if not declared_date:
        return
    rbres = g.client("read", {"sheet": sheet, "range": rng})
    resvals = [row[0] for row in rbres.get("cells", []) if row and isinstance(row[0], (int, float))]
    # value plausibility: every result is a valid, non-trivial date serial (≥1000 ≈ year 1902, ≤ Excel max).
    # Keeps YEAR(date)→2010 (passes range — but a "…Date" column of years is itself a date-ish col; acceptable)
    # and especially date−date→~120 (FAILS the ≥1000 floor) correctly NUMERIC.
    if resvals and all(1000 <= v <= 2958465 for v in resvals):
        g.client("apply", {"op": {"op": "set_number_format", "sheet": sheet, "range": rng,
                                  "format": "MM/DD/YYYY"}})

def clamp_range_to_data(rng, sheet, live):
    """GROUND a format/style range's BOTTOM row to the perceived data extent (2026-06-23). The model guesses
    the row count and often over-reaches by a row ("C2:C9" on 7-row data); formatting an EMPTY cell EXTENDS the
    used area, so the CSV/sheet_print export gains a phantom trailing ",,," row → row-count mismatch → 0 (the
    6e99a1ad failure). Clamp the end row to the live last used row. Only shrinks an over-reach; never grows a
    range. Leaves non-rectangular / unparseable ranges untouched (fail-open)."""
    m = re.match(r"([A-Za-z]+)(\d+):([A-Za-z]+)(\d+)$", (rng or "").replace("$", ""))
    last = live.get(sheet, {}).get("rows")
    rg = _range_region(live.get(sheet), rng)
    if rg:
        last = rg["row1"]                       # multi-table: clamp to the range's OWN table, not the sheet
    if not m or not last:
        return rng
    if int(m.group(4)) > last:
        return "%s%s:%s%d" % (m.group(1), m.group(2), m.group(3), last)
    return rng

def ground_sheet(name, live):
    """GROUND a SHEET reference (2026-06-23): the model names a sheet the way it perceived/read it — often
    copied from the prompt ("Sheet 1", "Sheet 2") — while the live tab is "Sheet1"/"Sheet2". Same move as
    ground_bare_refs, for sheet identifiers: bind the model's natural spelling to the actual live sheet.
    Exact match wins; else a UNIQUE whitespace/case-insensitive match binds; else leave as-is (fail-OPEN to the
    daemon's own placeholder tolerance — e.g. "S"→the lone sheet). Sound: only a unique normalized match binds,
    and it can only ever bind to a REAL live sheet. Applied to EXISTING-sheet refs only, never NEW-name fields
    (a sheet being CREATED must keep the name the model chose). Fixes the silent mis-resolve where the daemon's
    exact-only hasByName fell back to the active/first sheet (the 0cecd4f3-class fragility)."""
    if not name or name in live:
        return name
    def norm(s):
        return "".join(str(s).split()).casefold()
    hits = [s for s in live if norm(s) == norm(name)]
    return hits[0] if len(hits) == 1 else name

# Op fields that REFERENCE an existing sheet (to be grounded). NEW-name fields (add/rename/copy "new",
# "name") are deliberately absent — a sheet being created keeps the model's chosen name.
EXISTING_SHEET_FIELDS = {
    "rename_sheet": ["old"], "copy_sheet": ["source", "before"], "set_cell": ["sheet"],
    "compute_column": ["sheet"], "total_row": ["sheet"], "format_cells": ["sheet"],
    "merge_cells": ["sheet"], "set_number_format": ["sheet"], "sort_range": ["sheet"],
    "create_pivot": ["source"], "freeze_panes": ["sheet"], "export_csv": ["sheet"],
    "transpose_range": ["sheet"], "reorder_columns": ["sheet"], "hide_rows_where": ["sheet"],
    "format_cells_where": ["sheet"], "set_decimal_separator": ["sheet"], "export_pdf": ["sheet"],
    "set_zoom": ["sheet"],
    "dedup_column": ["sheet"],
    "compute_row": ["sheet"], "split_column": ["sheet"],
}

def ground_chart_ranges(ranges, sheet, live):
    """GROUND imprecise chart ranges to the structured chart (2026-06-23, the reason→emit bridge for args). The
    model gestures at 'the totals row over these columns with the header as categories' but encodes sloppy A1
    ranges (e.g. 'B1:B12;C12:G12' for what should be cat=B1:G1, val=B12:G12). Extract the COLUMN SPAN + the DATA
    ROW (the referenced row that isn't the header) from whatever it emitted, and rebuild canonical
    'headerRow ; dataRow' over the full span. Fires only when a header row + a distinct data row + ≥2 columns are
    present; else leaves the ranges untouched (fail-open). Grounds the intent, not the typo."""
    # COLUMN-oriented ranges (every part a single-column vertical range, e.g. "A2:A36;E2:E36" for a
    # dates-vs-quantity line chart) are already structured — the row-rebuild below would shred them
    # into per-cell series. Leave them untouched (fail-open; this grounding is for ROW-shaped intent).
    parts = [p.strip().replace("$", "") for p in (ranges or "").split(";") if p.strip()]
    if parts:
        m2 = [re.match(r"([A-Za-z]+)(\d+):([A-Za-z]+)(\d+)$", p) for p in parts]
        if all(m and m.group(1).upper() == m.group(3).upper() for m in m2):
            # Column-oriented (each part one vertical column) — already structured; the row-rebuild
            # below would shred it. ONE grounding applies: a part that STARTS at the header row is
            # the model including the label in the series — shift its start to the first data row
            # (the saved chart keys header-free refs; the gold shape). Multi-table: each part's OWN
            # table decides its header row (a part anchored in the second stacked table must shift
            # against THAT table's header, not the sheet's).
            out = []
            for p, m in zip(parts, m2):
                rg = _range_region(live.get(sheet), p)
                hrow = rg["header_row"] if rg else live.get(sheet, {}).get("header_row", 1)
                ds = rg["data_start"] if rg else live.get(sheet, {}).get("data_start", hrow + 1)
                start = ds if int(m.group(2)) == hrow else int(m.group(2))
                out.append("%s%d:%s%s" % (m.group(1), start, m.group(3), m.group(4)))
            return ";".join(out)
    refs = re.findall(r"([A-Za-z]+)(\d+)", ranges or "")
    if not refs:
        return ranges
    cols = sorted({_col_idx(c) for c, _ in refs})
    rows = sorted({int(r) for _, r in refs})
    hrow = live.get(sheet, {}).get("header_row", 1)
    rg = _range_region(live.get(sheet), "%s%d" % (col_letter(cols[0]), rows[0]))
    if rg:
        hrow = rg["header_row"]                 # multi-table: the referenced table's own header row
    datarows = [r for r in rows if r != hrow]
    if not datarows or len(cols) < 2:
        return ranges
    drow = max(datarows)
    c0, c1 = col_letter(cols[0]), col_letter(cols[-1])           # both 0-based (_col_idx returns n-1)
    return "%s%d:%s%d;%s%d:%s%d" % (c0, hrow, c1, hrow, c0, drow, c1, drow)

def ground_bare_refs(formula, sheet, live):
    """GROUNDING (2026-06-23, user direction): meet the model where it works. The model names a column by the
    header it PERCEIVED ("Loan Issue Date") — that is the grounded, correct thing to do; the {braces} are OUR
    dialect. Instead of coercing the dialect (prompt/grammar/retry/training — all bend the model toward us),
    we GROUND the natural reference: wrap each SOUND bare occurrence of a live-detected header in braces so the
    existing notation-robust resolver binds it (unique-or-fail-closed — soundness stays where it already is).
    This pass only RECOGNIZES the reference form; it does not decide bindings. The GUARDS are the entire mis-
    bind surface (the advisor's break cases): skip a name inside a "string literal", in function position
    (name immediately followed by '('), or already braced; match the LONGEST header first so a short header
    ("Sales") never lands inside a longer one ("Sales Tax"). Returns the formula with natural refs braced."""
    cols = _sheet_cols(live.get(sheet, {}))
    headers = sorted({c["header"].strip() for c in cols if c["header"].strip()}, key=len, reverse=True)
    out = formula
    for h in headers:
        pat = re.compile(r"(?<![\w{])" + re.escape(h) + r"(?![\w}])(?!\s*\()", re.I)
        spans = [m.span() for m in re.finditer(r'"[^"]*"', out)]   # string-literal spans on CURRENT text
        for m in reversed(list(pat.finditer(out))):                # right-to-left keeps indices valid
            if any(a <= m.start() < b for a, b in spans):          # inside a literal → never ground
                continue
            out = out[:m.start()] + "{" + m.group(0) + "}" + out[m.end():]
    return out

def substitute_names(formula, default_sheet, detected, fails, row, refsheets=None, region=None):
    """Replace {Header} / {Sheet.Header} with A1 refs at the given row. Cross-sheet refs use the proven
    Excel `Sheet!Cell` syntax. Fail-closed: any braced token that doesn't uniquely resolve aborts (None).
    NOTE (2026-06-23): we deliberately do NOT resolve BARE (unbraced) column names. A bare name is an
    EMISSION failure (the model didn't emit a valid reference); rescuing it in the resolver would (a) move
    interpretation into the harness and (b) hide the emission axis we pre-committed to MEASURE. If brace-
    friction proves to dominate, the disciplined fix is at the GRAMMAR (make an unbraced ref unemittable),
    not a post-hoc guess here. refsheets (if given) collects every sheet the formula references."""
    aborted = [False]
    def repl(m):
        res = resolve_ref_full(m.group(1).strip(), default_sheet, detected, fails, region=region)
        if res is None:
            aborted[0] = True; return m.group(1)
        sh, letter, reg = res
        if refsheets is not None:
            refsheets.add(sh)
        r = detected.get(sh, {}).get("data_start", row)   # each ref uses its own sheet's first data row
        rg = _region_of_col(detected.get(sh), letter, region_hint=reg)
        if rg is not None:                                # multi-table: the ref's own TABLE sets the row
            r = rg["data_start"]
        ref = "%s%d" % (letter, r)
        return ref if sh == default_sheet else "%s!%s" % (sh, ref)
    out = NAME_TOK.sub(repl, formula)
    return None if aborted[0] else out

def falsify_style_contract(instr, nameops):
    """STYLE-CONTRACT falsifier (2026-07-05 — 21ab7b40 FALSE-PASS: goal said 'green (#00ff00)
    FONT', the op painted FILL; values corroborated, the claim gate passed, score 0). A hex color
    the GOAL states verbatim next to a property word (font/fill/background) is a named deliverable;
    if no applied style op carries that hex in that property, the claim is blocked. Op-log-grounded
    and goal-verbatim — detects absence only, never confirms; wrong firings under-claim."""
    fired = []
    low = (instr or "").lower()
    for m in re.finditer(r"#[0-9a-f]{6}", low):
        hexv = m.group(0)
        ctx = low[max(m.start() - 30, 0):m.end() + 30]
        prop = "font_color" if "font" in ctx else \
               ("fill_color" if ("fill" in ctx or "background" in ctx or "highlight" in ctx) else None)
        if prop is None:
            continue
        ok = any(n.get("kind") in ("format_cells", "format_cells_where") and
                 (n.get(prop) or "").strip().lower() == hexv for n in nameops)
        if not ok:
            fired.append({"falsifier": "style_contract", "range": "%s %s" % (hexv, prop)})
    return fired


def _shift_a1_cols(formula, offset):
    """Shift every RELATIVE A1 column reference in a formula right by `offset` columns —
    the deterministic host-side row-fill (the fillAuto only-last-ref-adjusts bug class is
    avoided by never using fillAuto). Absolute columns ($A1) stay; function names with
    digits (LOG10) are excluded by the boundary guards."""
    def rep(m):
        dollar, col, row = m.group(1), m.group(2), m.group(3)
        if dollar:
            return m.group(0)
        n = 0
        for ch in col:
            n = n * 26 + (ord(ch) - 64)
        n += offset
        if n < 1:
            return m.group(0)
        out = ""
        while n:
            n, r = divmod(n - 1, 26)
            out = chr(65 + r) + out
        return out + row
    return re.sub(r"(?<![A-Za-z0-9_$])(\$?)([A-Z]{1,3})(\d+)(?![A-Za-z0-9_(])", rep, formula)

def falsify_text_decimals(g, instr, written):
    """DECIMAL-RENDER CONTRACT (2026-07-06 — 4f07fbe9 sweep miss: goal states 'decimal digits
    to 2' for a number shown inside a text; the written text embedded the number with ONE
    decimal; value-compare scored 0 silently). When the GOAL states a decimal-digit count and
    a written cell holds TEXT with an embedded decimal number of a DIFFERENT digit count, the
    named rendering requirement is unmet. Read-back grounded; detects mismatch only."""
    low = (instr or "").lower()
    m = re.search(r"decimal (?:digits?|places?)[^0-9]{0,10}(\d)", low) or         re.search(r"(\d)\s+decimal (?:digits?|places?)", low)
    if not m:
        return []
    want = int(m.group(1))
    fired = []
    for sheet, where, _what in written:
        cell = where if re.match(r"^[A-Za-z]+\d+$", str(where)) else None
        if not cell:
            continue
        rb = g.client("read", {"sheet": sheet, "range": "%s:%s" % (cell, cell)})
        if not rb.get("ok") or not rb.get("cells"):
            continue
        v = rb["cells"][0][0] if rb["cells"][0] else None
        if not isinstance(v, str):
            continue
        for num in re.findall(r"\d+\.(\d+)", v):
            if len(num) != want:
                fired.append({"falsifier": "text_decimals",
                              "range": "%s!%s shows %r" % (sheet, cell, v[:60]),
                              "sample": "%d decimal digit(s), goal states %d" % (len(num), want)})
    return fired

def falsify_pivot_orientation(instr, nameops):
    """PIVOT-ORIENTATION CONTRACT (2026-07-06 — 1de60575 sweep miss: goal said 'the promotion
    names as the column headers', the pivot was built with the field in rows and cols empty;
    the official evaluator compares the pivot object's col_fields → 0). Goal-verbatim phrase →
    slot contract, op-log-grounded like the style contract: if the goal states that values
    become COLUMN headers and an applied create_pivot has an empty cols slot (or the row-labels
    mirror image), the named deliverable is absent. Detects absence only; never confirms."""
    fired = []
    low = (instr or "").lower()
    pivots = [n for n in nameops if n.get("kind") == "create_pivot"]
    if not pivots:
        return fired
    if re.search(r"as (the )?column (headers?|labels?)", low):
        for n in pivots:
            if not (n.get("cols") or "").strip():
                fired.append({"falsifier": "pivot_orientation",
                              "range": "rows=%s cols=(empty)" % (n.get("rows") or "(empty)")})
    if re.search(r"as (the )?row (headers?|labels?)", low):
        for n in pivots:
            if not (n.get("rows") or "").strip():
                fired.append({"falsifier": "pivot_orientation",
                              "range": "cols=%s rows=(empty)" % (n.get("cols") or "(empty)")})
    return fired

# ── READ-BACK + SOUND FALSIFIERS (falsify only; pass ≠ correct) ──────────────────
def falsify_empty_named_targets(g, instr, nameops=()):
    """INSTRUCTION-NAMED TARGET COMPLETENESS (sound, goal-grounded). A live header the GOAL ITSELF
    names verbatim whose column is still ENTIRELY EMPTY after apply = an unfinished deliverable
    invisible to write-corroboration (observed 37608790: 1 of 3 named columns filled → the claim
    gate corroborated the one write and FALSE-PASSED). Fires only on exact header-in-goal matches
    (≥3 chars, case-insensitive) with a fully empty data span — no inference from reasoning text.
    Wrong firings only UNDER-claim and nag (never a false pass)."""
    fired = []
    any_named = [False]
    low = (instr or "").lower()
    # Content an op WROTE (a title via set_cell) can be re-detected as a "header" over an empty
    # column — that is a delivered artifact, not an unfilled target (measured: 'Demographic
    # Profile' nagged as an empty column right after being written).
    written_texts = {str(v).strip().lower() for o in (nameops or ()) for v in o.values()
                     if isinstance(v, str) and len(v.strip()) >= 3}
    for sheet, info in live_detect(g).items():
        cols_all = _sheet_cols(info)                  # multi-table: every table's columns are named targets
        headers_all = [str(cc.get("header") or "").strip() for cc in cols_all]
        for c in cols_all:
            rg = _region_of_col(info, c["letter"], region_hint=c.get("region"))
            ds = rg["data_start"] if rg else info.get("data_start", 2)
            last = rg["row1"] if rg else ds + info.get("rows", 0) - 1
            if last < ds:
                continue
            h = str(c.get("header") or "").strip()
            if len(h) < 3 or h.lower() in written_texts:
                continue
            named = h.lower() in low
            if not named:
                # UNIQUE-WORD binding: the goal says "Billions (B) in Column C" while the header
                # reads "in billions (B)" — verbatim-substring misses it. A distinctive header
                # word (≥5 chars) that appears in the goal AND in exactly ONE header binds
                # deterministically. Misfires only nag/under-claim, never false-pass.
                words = [w for w in re.findall(r"[a-z]{5,}", h.lower())]
                for w in words:
                    if w in low and sum(1 for hh in headers_all if w in hh.lower()) == 1:
                        named = True
                        break
            if named:
                any_named[0] = True
            if not named:
                continue
            r = g.client("read", {"sheet": sheet,
                                  "range": "%s%d:%s%d" % (c["letter"], ds, c["letter"], last)})
            vals = [v for row in (r.get("cells") or []) for v in row]
            if vals and all(v in (None, "") for v in vals):
                fired.append({"falsifier": "named_target_empty",
                              "range": "'%s' (column %s on %s)" % (h, c["letter"], sheet)})
            elif len(vals) >= 3 and vals[0] not in (None, "") and \
                    sum(1 for v in vals[1:] if v in (None, "")) > len(vals[1:]) / 2:
                # PARTIAL FILL (measured on 7e429b8d): the model authors a correct formula, sets it in
                # the FIRST data cell and says "drag the fill handle down" — an intent no emitted op
                # carries; the column's remaining rows stay empty and nothing detected it. Threshold =
                # MAJORITY of the rows below the top still empty ("top cell only" stopped matching the
                # moment a resample added a second cell — 2-of-11 filled read as clean). A goal-named
                # column left mostly empty = an unfinished deliverable. Under-claims and nags only —
                # never a false pass.
                fired.append({"falsifier": "column_fill_incomplete",
                              "range": "'%s' (column %s on %s, rows %d-%d)" % (h, c["letter"], sheet, ds, last)})
    # ── CONTRACT COMPILER v1 (2026-07-05 — the f9584479 under-specified-goal FALSE-PASS class:
    # 'fill the missing totals' names no deliverable, falsifiers had nothing to bind, corroboration
    # shared the model's assumptions). When a WRITE-shaped goal binds to NO named header anywhere,
    # the STRUCTURE speaks for the user: holes in a live-headed column at rows where ≥2 sibling
    # columns hold data are structural deliverables — the claim stays blocked while they exist.
    # Absence-detection only; wrong firings under-claim, never a false pass.
    if not any_named[0] and re.search(r"\b(fill|complete|calculat|comput|missing|total)\w*\b", low):
        for sheet, info in live_detect(g).items():
            for rg in (info.get("regions") or [info]):
                cols_r = rg.get("cols", [])
                ds_r = rg.get("data_start", 2)
                last_r = rg.get("row1", ds_r + rg.get("rows", 0) - 1)
                if last_r < ds_r or len(cols_r) < 3:
                    continue
                lc0, lc1 = cols_r[0]["letter"], cols_r[-1]["letter"]
                rr_ = g.client("read", {"sheet": sheet,
                                        "range": "%s%d:%s%d" % (lc0, ds_r, lc1, last_r)})
                grid_ = rr_.get("cells") or []
                for ci, c in enumerate(cols_r):
                    if not str(c.get("header") or "").strip():
                        continue
                    holes = sum(1 for row in grid_
                                if ci < len(row) and _blank(row[ci]) and
                                sum(1 for j, x in enumerate(row) if j != ci and not _blank(x)) >= 2)
                    if holes:
                        fired.append({"falsifier": "structural_target_holes",
                                      "range": "'%s' (column %s on %s, %d empty cell(s) beside "
                                               "filled rows)" % (c["header"], c["letter"], sheet, holes)})
    return fired

def falsify(g, written_regions):
    """written_regions = [(sheet, a1range, formula)]. Return list of FIRED falsifiers.
    Empty list = 'no detected fault' — NOT 'correct' (the oracle is the only correctness signal).
    All falsifiers are SOUND (they can only detect wrongness, never confirm correctness)."""
    fired = []
    for sheet, rng, formula in written_regions:
        # LITERAL single-cell writes (plain set_cell text, in the ledger since the 2026-07-06
        # visibility fix) are NOT formula results: F1 would fire on legit text like "#1" or
        # "Errors", F3 on deliberate value="" clears. The formula falsifiers scan only what a
        # FORMULA produced; literal writes are covered by the contract falsifiers (text_decimals).
        if not str(formula).startswith("="):
            continue
        r = g.client("read", {"sheet": sheet, "range": rng})
        if not r.get("ok"):
            fired.append({"falsifier": "read_failed", "range": rng}); continue
        cells = [row[0] for row in r.get("cells", []) if row]
        # F1: error face where a formula was applied
        errs = [v for v in cells if isinstance(v, str) and (v.startswith("#") or "Err" in v)]
        if errs:
            fired.append({"falsifier": "error_values", "range": rng, "sample": errs[:3]})
        # F2 (SOUND): a TEXT formula (& / CONCATENATE) that yields a NUMBER did not concatenate as written
        # — the failure mode behind the silent '_'→0 collapse. Text op MUST produce text.
        is_text_op = ("&" in formula) or ("CONCAT" in formula.upper()) or ("TEXT(" in formula.upper())
        if is_text_op and cells and all(isinstance(v, (int, float)) for v in cells):
            fired.append({"falsifier": "text_formula_numeric", "range": rng, "sample": cells[:3]})
        # RESULT-PREVIEW (Pile 2, 2026-07-05 — 37608790: the model authors a formula BLIND and
        # retries blind; a human looks at what the formula produced next to its inputs. Attach the
        # OBSERVED left-context rows to the fired fault — pure observation, the model's own output
        # beside its own input; no instruction content.)
        if fired and fired[-1]["range"] == rng and \
           fired[-1]["falsifier"] in ("error_values", "text_formula_numeric"):
            m0 = re.match(r"([A-Za-z]+)(\d+)", rng.replace("$", ""))
            if m0:
                r0 = int(m0.group(2))
                ctx = g.client("read", {"sheet": sheet,
                                        "range": "A%d:%s%d" % (r0, m0.group(1), r0 + 1)})
                fired[-1]["rows"] = (ctx.get("cells") or [])[:2]
        # F3: extent shortfall — empty cells inside the written range
        empties = sum(1 for v in cells if v is None or v == "")
        if empties:
            fired.append({"falsifier": "extent_shortfall", "range": rng, "empty": empties})
    return fired

# ── shared op-text parsing ───────────────────────────────────────────────────────
def scan_calls(text, verbs):
    pat = re.compile(r"(%s)\(" % "|".join(verbs))
    for m in pat.finditer(text):
        verb = m.group(1); i = m.end(); depth, in_q, esc, start = 1, False, False, m.end()
        while i < len(text) and depth > 0:
            c = text[i]
            if esc: esc = False
            elif c == "\\": esc = True
            elif c == '"': in_q = not in_q
            elif not in_q and c == "(": depth += 1
            elif not in_q and c == ")": depth -= 1
            i += 1
        yield verb, text[start:i - 1]

KV = re.compile(r'(\w+)="((?:[^"\\]|\\.)*)"')
def parse_kv(body):
    return {m.group(1): m.group(2).replace('\\"', '"') for m in KV.finditer(body)}

def coerce(v):
    try: return int(v)
    except ValueError:
        try: return float(v)
        except ValueError: return v

# ── CORROBORATION (P3 mechanism, in the loop): no-oracle confidence via independent re-derivation ──
def region_values(g, written):
    out = {}
    for sheet, rng, _f in written:
        r = g.client("read", {"sheet": sheet, "range": rng})
        out[(sheet, rng)] = ([row[0] if row else None for row in r.get("cells", [])] if r.get("ok") else None)
    return out

def values_agree(a, b):
    if a is None or b is None or len(a) != len(b):
        return False
    for x, y in zip(a, b):
        if isinstance(x, (int, float)) and isinstance(y, (int, float)):
            if abs(x - y) > 1e-6:
                return False
        elif str(x) != str(y):
            return False
    return True

def _col_idx(letters):
    n = 0
    for ch in letters.upper():
        n = n * 26 + (ord(ch) - 64)
    return n - 1

def formula_refset(a1, default_sheet):
    """The SET of (sheet, COLUMN) a resolved A1 formula references — ranges EXPANDED (SUM(F2:H2)→F,G,H),
    rows ignored (propagation makes them uniform). Row-agnostic column footprint = what the formula
    actually depends on. Equivalent forms (F+G+H vs SUM(F:H)) collapse to the same set."""
    refs = set()
    if not a1:
        return refs
    for m in re.finditer(r"(?:([A-Za-z0-9_]+)!)?([A-Z]+)\d+:([A-Z]+)\d+", a1):
        sh = m.group(1) or default_sheet
        a, b = _col_idx(m.group(2)), _col_idx(m.group(3))
        for ci in range(min(a, b), max(a, b) + 1):
            refs.add((sh, col_letter(ci)))
    singles = re.sub(r"(?:[A-Za-z0-9_]+!)?[A-Z]+\d+:[A-Z]+\d+", " ", a1)  # blank ranges first
    for m in re.finditer(r"(?:([A-Za-z0-9_]+)!)?([A-Z]+)\d+", singles):
        refs.add((m.group(1) or default_sheet, m.group(2).upper()))
    return refs

def corroborate(g, instr, detected, der1_written, mainlog):
    """READ-ONLY no-oracle confidence: an INDEPENDENT re-derivation (temp>0) must reference the SAME columns
    for each target. NEVER modifies the scored doc (structural footprint comparison, not value re-apply —
    that corrupted the doc). Catches the dominant false-pass mode (a dropped/added input column). Confidence,
    not proof; a conservative disagreement (abstain on a correct answer) is SAFE — it never creates a pass."""
    if not der1_written:
        return False
    d2log = {}
    der2 = author_B(instr, detected, d2log, temperature=0.6)
    mainlog["der2_emit"] = d2log.get("emit_raw")
    live = live_detect(g)
    der2_f = {}
    for nop in der2:
        if nop["kind"] == "compute_column":
            tres = resolve_col(nop["sheet"], nop["target"], live, [])
            if tres:
                tcol, treg = tres
                rg = _region_of_col(live.get(nop["sheet"]), tcol, region_hint=treg)
                ds = rg["data_start"] if rg else live.get(nop["sheet"], {}).get("data_start", 2)
                der2_f[(nop["sheet"], tcol)] = substitute_names(nop["formula"].replace("'", '"'),
                                                                nop["sheet"], live, [], row=ds, region=treg)
    agree, detail = True, []
    for (sheet, rng, f1) in der1_written:
        col = re.match(r"[A-Za-z]+", rng).group(0)
        f2 = der2_f.get((sheet, col))
        s1, s2 = formula_refset(f1, sheet), formula_refset(f2, sheet)
        detail.append({"col": col, "der1_refs": sorted("%s!%s" % r for r in s1),
                       "der2_refs": sorted("%s!%s" % r for r in s2)})
        if f2 is None or s1 != s2:
            agree = False
    mainlog["corrob_detail"] = detail
    return agree

# ── settle wait (ADDITIVE, default = the original fixed sleep) ────────────────────
def settle_wait(g, log, floor_s=4.0):
    """Wait for the reconcile GUI reload's visible soffice window to SETTLE before the
    evaluator activates it (the TURN-16 seam). Default behavior is byte-identical to the
    original `time.sleep(floor_s)`. With LAGADO_SETTLE_MONITOR=1 on the VM path (host-side
    guest screenshots available), the PROMOTED CfC settle monitor (reflex/, gate 2026-07-06:
    FS 0 vs baseline 2, latency 1.98 vs 2.23 s) watches the guest and releases on observed
    settle — earlier when the window is up fast, LATER (up to LAGADO_SETTLE_MAX, default 15 s)
    when it is still churning at the 4 s mark. FAIL-OPEN everywhere: any monitor problem
    falls back to finishing the fixed sleep; env.evaluate() is never reached early on a
    monitor error."""
    env = getattr(g, "env", None)
    if os.environ.get("LAGADO_SETTLE_MONITOR", "1") == "0" or log.get("host") or env is None:
        time.sleep(floor_s)
        return
    t0 = time.time()
    mon = None
    info = {"mode": "cfc", "ticks": 0, "settled": False}
    try:
        rdir = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)),
                                             "..", "reflex"))
        if rdir not in sys.path:
            sys.path.insert(0, rdir)
        from settle_client import SettleMonitor, TickFeaturizer
        mon = SettleMonitor()
        fz = TickFeaturizer()
        cap_s = float(os.environ.get("LAGADO_SETTLE_MAX", "15"))
        last = time.time()
        while time.time() - t0 < cap_s:
            png = env.controller.get_screenshot()
            sense = g.sh("DISPLAY=:0 wmctrl -l 2>/dev/null | md5sum | cut -d' ' -f1; "
                         "DISPLAY=:0 wmctrl -l 2>/dev/null | wc -l; "
                         "pgrep -c soffice.bin; pgrep -c gimp; true").get("out", "")
            now = time.time()
            dt, last = max(now - last, 1e-3), now
            if png is None:
                raise RuntimeError("no guest screenshot")
            dump = os.environ.get("LAGADO_SETTLE_DUMP", "")
            if dump:
                os.makedirs(dump, exist_ok=True)
                open(os.path.join(dump, "tick_%02d_%.2fs.png"
                                  % (info["ticks"], now - t0)), "wb").write(png)
            lines = [x.strip() for x in sense.splitlines() if x.strip()]
            whash = lines[0] if lines else ""
            wcount = int(lines[1]) if len(lines) > 1 and lines[1].isdigit() else 0
            pcount = sum(int(x) for x in lines[2:4] if x.isdigit())
            feats = fz.step(png, whash, wcount, pcount)
            if feats is None:
                continue                       # first frame primes the featurizer (train parity)
            p, settled = mon.tick(feats, dt)
            if p is None:
                raise RuntimeError("settle monitor unavailable (fail-open)")
            info["ticks"] += 1
            if settled:
                info["settled"] = True
                break
        # FLOOR CLAMP (2026-07-06 adversarial review): the promoted v1 model is input-underweighted
        # (replay: churn p=0.743 vs quiet p=0.741) — its release is a clock, and early release is
        # the one failure fail-open cannot catch (a confident wrong answer, not an error). Until a
        # v2 passes the timer-null gate, the monitor may only EXTEND the wait (adaptive ceiling
        # kept), never shorten it below the proven fixed floor. Telemetry unchanged.
        remaining = floor_s - (time.time() - t0)
        if remaining > 0:
            info["floor_clamped"] = True
            time.sleep(remaining)
        info["s"] = round(time.time() - t0, 2)
        log["settle_wait"] = info
    except Exception as e:
        info["mode"], info["error"] = "cfc_failopen", str(e)[:120]
        info["s"] = round(time.time() - t0, 2)
        log["settle_wait"] = info
        remaining = floor_s - (time.time() - t0)
        if remaining > 0:
            time.sleep(remaining)                 # the deterministic floor stands
    finally:
        if mon is not None:
            mon.close()

# ── one run of a condition ───────────────────────────────────────────────────────
def run_condition(env, task, cond, file_path, run_idx):
    """VM path: build the guest, bring up the daemon, then run the SHARED core scored by env.evaluate()."""
    g = Guest(env)
    log = {"cond": cond, "run": run_idx, "id": task["id"][:8], "steps": []}
    unopy = pick_uno_python(g)
    if not unopy:
        log["fatal"] = "no uno python"; return 0.0, log
    g.sh("pkill -9 soffice; pkill -9 soffice.bin; true")
    g.sh("rm -f '%s/.~lock.%s#' 2>/dev/null; true" % (os.path.dirname(file_path), os.path.basename(file_path)))
    time.sleep(1)
    if not deploy_daemon(g, unopy):
        log["fatal"] = "daemon not ready"; return 0.0, log
    return run_core(g, task, cond, file_path, log, lambda: env.evaluate())

def run_core(g, task, cond, file_path, log, score_fn):
    """The model→emit→apply→corroborate→score body, IDENTICAL for VM and host. `g` is an already-connected
    daemon client (Guest over the OSWorld env, or a HostGuest over a local soffice); `score_fn()` returns the
    REAL evaluator score (env.evaluate() on the VM; the metric funcs on the produced xlsx on host). Keeping this
    one body shared is why a host result is a faithful proxy for a VM result — same brain, same emission, same
    apply, same scoring — differing ONLY in host-LO vs guest-LO (which matters only for render-type tasks)."""
    r = g.client("open", {"file": file_path})
    if not r.get("ok"):
        log["fatal"] = "open failed: %s" % r.get("error"); return 0.0, log
    detail = r.get("structure", {}).get("detail", [])
    log["steps"].append("detect")

    instr = task["instruction"]
    resolve_fails, fired, written = [], [], []
    if cond == "A":
        detected = detect(g, detail)               # SAME fixed detector as B → A sees real headers too
        log["detected"] = {s: [(c["letter"], c["header"], c.get("ntype")) for c in i["cols"]] for s, i in detected.items()}
        ops, ainfo = author_A(instr, detected)
        log["author"] = ainfo
        log["ops"] = ops
        log["n_ops"] = len(ops)
        for o in ops:
            rr = g.client("apply", {"op": o})
            if not rr.get("ok"):
                log["steps"].append("apply_reject:%s:%s" % (o.get("op"), rr.get("error", "")[:60]))
            if o.get("op") == "set" and "formula" in o:
                written.append((o.get("sheet"), o.get("cell"), o.get("formula", "")))
        log["steps"].append("apply")
        fired = falsify(g, written)
    else:
        detected = detect(g, detail)
        log["detected"] = {s: [(c["letter"], c["header"], c.get("ntype")) for c in i["cols"]] for s, i in detected.items()}
        feedback = None
        additive = False
        attempt = 0
        carried = []
        for attempt in range(2):                  # reason→emit, then ONE read-back retry (the ReAct condition)
            log["steps"].append("attempt%d" % attempt)
            new_ops = author_B(instr, detected, log, feedback, additive=additive)
            nameops = merge_nameops(carried, new_ops)   # retain ops the lossy retry-emit dropped (interface-plane repair)
            # Gap check BEFORE apply: a missing op (chart/pivot/total_row) is retry-recoverable, but a
            # WRONG-CLASS op like a blanket style where the analysis says conditional CANNOT be unpainted
            # — withhold it this attempt; the gap feedback asks for the conditional verb instead.
            # A SOLE infeasible declaration ends the attempt loop — nothing to apply or falsify;
            # scoring mirrors the official env below. Mixed with real ops it is noise: drop it and
            # proceed (a genuine declaration has nothing else to say).
            if len(nameops) == 1 and nameops[0].get("kind") == "infeasible":
                log["declared_infeasible"] = nameops[0].get("reason", "")
                log["nameops"] = nameops
                written, resolve_fails, fired, gaps = [], [], [], []
                break
            nameops = [n for n in nameops if n.get("kind") != "infeasible"]
            gaps = emit_gaps(log.get("reasoning", ""), nameops, instr)   # reason→emit completeness (membrane bridge)
            if "conditional_format" in gaps:
                nameops = [n for n in nameops if n.get("kind") != "format_cells"]
            carried = nameops
            log["nameops"] = nameops
            written, resolve_fails = apply_B(g, nameops, log, instr)
            fired = falsify(g, written) + falsify_empty_named_targets(g, instr, nameops) + \
                falsify_style_contract(instr, nameops) + falsify_pivot_orientation(instr, nameops) + falsify_text_decimals(g, instr, written)
            log["n_ops"] = len(nameops)
            if nameops and not resolve_fails and not fired and not gaps:
                break                            # emitted ops, no detected fault — stop (NOT a correctness
                                                 # claim). Covers STRUCTURAL-only tasks (rename/copy/format)
                                                 # which write no compute_column → empty `written` → used to
                                                 # retry needlessly. (no_fault below still gates self-report.)
            # ══ DIVERGENCE RESAMPLE (DSpark-shaped, 2026-07-05) ══ prefix committed, each localized
            # fault gets ONE targeted single-op resample BEFORE any full re-derivation; permanently
            # rejected ops are dropped from the carried list. Cleans → skip the full retry entirely;
            # doesn't → the unchanged full-retry + iterative floor proceeds with the updated state.
            if attempt == 0 and nameops:
                nameops, written, resolve_fails, fired, gaps = resample_divergence(
                    g, instr, nameops, written, resolve_fails, fired, gaps, log)
                carried = nameops
                log["nameops"] = nameops
                if nameops and not resolve_fails and not fired and not gaps:
                    log["steps"].append("resample_clean")
                    break
            feedback = (compose_feedback(resolve_fails, fired) + "\n" + gap_feedback(gaps)).strip()
            # ADD-type notes (missing ops / empty chart data / unfilled named targets) need the
            # additive retry stance.
            additive = bool(gaps) or any("EMPTY" in f.get("why", "") for f in resolve_fails) or \
                any(f.get("falsifier") in ("named_target_empty", "column_fill_incomplete") for f in fired)
            log.setdefault("feedbacks", []).append(feedback)
        log["attempts"] = attempt + 1

        # ══ ITERATIVE-EMISSION ESCALATION (variable #1) ══ The single-shot floor above is
        # UNTOUCHED; this engages ONLY when it ends with detected faults/gaps — the measured
        # compound-collapse signature. One op per call, applied immediately, LIVE state
        # re-presented each step. Deterministic rails: 8-step cap, duplicate-proposal stop,
        # stop after 2 consecutive apply failures, infeasible ignored mid-flight.
        if (resolve_fails or fired or gaps) and "declared_infeasible" not in log:
            log["steps"].append("iterative")
            applied = [(o, "already applied (may be incomplete or faulty)") for o in nameops]
            problems = (compose_feedback(resolve_fails, fired) + "\n" + gap_feedback(gaps)).strip()
            consec_errors = 0
            steps_taken = 0
            forced_used = False
            for _it in range(8):
                nop = author_step(instr, g, log.get("reasoning", ""), applied, problems, log)
                if nop is None and problems and not forced_used:
                    # done() rubber-stamped over OPEN problems (measured): one forced step with no
                    # done() escape, aimed at the first problem. Once per loop — a rail, not a whip.
                    forced_used = True
                    nop = author_step(instr, g, log.get("reasoning", ""), applied, problems, log,
                                      forced=True)
                if nop is None or nop.get("kind") == "infeasible":
                    break
                key = _op_key(nop)
                if any(_op_key(o) == key for o, _n in applied):
                    # First echo of an applied op: tell it (the note lands in the next step's
                    # applied list) and ask again; a second echo means it has nothing new — stop.
                    if any(n == "DUPLICATE of an applied operation" for _o, n in applied):
                        break
                    applied.append((nop, "DUPLICATE of an applied operation"))
                    continue
                # same pre-apply withhold as the single-shot path: a BLANKET style op against a
                # conditional/extreme-value styling analysis cannot be unpainted.
                if nop.get("kind") == "format_cells" and \
                   "conditional_format" in emit_gaps(log.get("reasoning", ""),
                                                     [o for o, _n in applied] + [nop]):
                    applied.append((nop, "WITHHELD: your analysis styles only cells matching a "
                                         "condition — use format_cells_where"))
                    continue
                w2, f2 = apply_B(g, [nop], log, instr)
                written += w2
                steps_taken += 1
                if f2:
                    consec_errors += 1
                    note = "FAILED: %s" % str(f2[-1].get("why", ""))[:70]
                    resolve_fails = f2
                else:
                    consec_errors = 0
                    note = "applied"
                    if w2:
                        rb = g.client("read", {"sheet": w2[-1][0], "range": w2[-1][1]})
                        head = [row[0] if row else None for row in (rb.get("cells") or [])][:3]
                        note = "applied; first values: %s" % head
                    resolve_fails = []
                applied.append((nop, note))
                if consec_errors >= 2:
                    break
                # OBSERVE: recompute the detected faults on the live document — the loop's exit is
                # the OBSERVATION going clean, not the model's say-so.
                fired = falsify(g, written) + falsify_empty_named_targets(g, instr, nameops) + \
                    falsify_style_contract(instr, [o for o, _n in applied]) + falsify_pivot_orientation(instr, [o for o, _n in applied])
                gaps = emit_gaps(log.get("reasoning", ""), [o for o, _n in applied], instr)
                problems = (compose_feedback(resolve_fails, fired) + "\n" + gap_feedback(gaps)).strip()
                if not problems:
                    break
            nameops = [o for o, _n in applied]
            log["nameops"] = nameops
            log["iter_steps"] = steps_taken
            # DEPENDENCY RE-APPLY: an op that failed fail-closed before its dependency existed
            # (a pivot over a Revenue column created two steps later) never re-ran — the duplicate
            # guard rightly blocks re-proposing it. The op vocabulary is idempotent by design
            # (guarded creates, replace-by-name charts, deterministic pivot names, overwrite
            # writes), so ONE full re-apply in dependency order resolves every such case.
            if steps_taken:
                w3, _refails = apply_B(g, merge_nameops([], nameops), log, instr)
                written += w3
            fired = falsify(g, written) + falsify_empty_named_targets(g, instr, nameops) + \
                falsify_style_contract(instr, nameops) + falsify_pivot_orientation(instr, nameops) + falsify_text_decimals(g, instr, written)

    log["falsifiers_fired"] = fired
    no_fault = (len(written) > 0 and len(fired) == 0 and len(resolve_fails) == 0)

    # diagnostic read-back of der1's actual written values (so silent-wrong is inspectable)
    log["readback"] = {}
    for sheet, rng, _f in written:
        rb = g.client("read", {"sheet": sheet, "range": rng})
        if rb.get("ok"):
            log["readback"]["%s!%s" % (sheet, rng)] = [row[0] if row else None for row in rb.get("cells", [])]

    # CORROBORATION (Condition B): the honest "done" requires an INDEPENDENT re-derivation to AGREE on the
    # values — else ABSTAIN (no-fault-detected is NOT correct). This is the P3 mechanism, in the loop.
    corroborated = True
    if cond == "B" and no_fault:
        corroborated = corroborate(g, instr, detected, written, log)   # READ-ONLY — scored doc untouched
        log["corroborated"] = corroborated
    harness_reports_done = no_fault and corroborated   # claim done ONLY when corroborated
    log["self_report_done"] = harness_reports_done

    # GUI reload is ONLY needed by the GUEST evaluator (it scores a live window via activate-by-title + ctrl+s).
    # On the HOST, score_fn reads the saved file directly, so a GUI reload spawns a stray detached window per task
    # (reparents to systemd, escapes per-run cleanup → windows pile up). Gate it OFF on host. `log["host"]` is set
    # only by battery_host; the VM path (run_condition) never sets it, so the guest still gets its window.
    g.client("reconcile", {"gui": not log.get("host")})
    if os.environ.get("LAGADO_VISIBLE"):          # watch-mode: hold the finished doc on screen before closing
        time.sleep(int(os.environ.get("LAGADO_VISIBLE_HOLD", "25")))
    g.client("close")
    settle_wait(g, log)   # was: time.sleep(4) — identical unless LAGADO_SETTLE_MONITOR=1
    if "declared_infeasible" in log:
        # Mirror the official env exactly (desktop_env.evaluate): an infeasible-func task with a FAIL
        # declaration scores 1; a declaration on a FEASIBLE task scores 0 — a wrong declaration can
        # only LOSE, never false-pass, and it IS the false claim, so it sets the false_pass flag.
        # (The VM runner must translate this into a literal "FAIL" action for env.evaluate.)
        score = 1.0 if (task.get("evaluator") or {}).get("func") == "infeasible" else 0.0
        log["score"] = score
        log["self_report_done"] = True
        log["false_pass"] = bool(score < 1.0)
        return score, log
    score = score_fn()
    if isinstance(score, tuple):      # host scorer error sentinel ("ERR", msg) — pass it through
        log["score_err"] = score[1]   # (comparing it crashed the claim gate; measured). Never a
        log["false_pass"] = None      # verdict: the caller reports SCORE-ERR.
        return score, log
    if score is None:                 # host render-skip sentinel (compare_pdfs etc.) — pass it
        log["score"] = None           # through so the caller reports RENDER-SKIP, not a false 0
        log["false_pass"] = None
        return None, log
    log["score"] = score
    # P5 calibration pair + false-pass flag (the integrity core)
    log["false_pass"] = bool(harness_reports_done and score < 1.0)
    return score, log

# ── main ─────────────────────────────────────────────────────────────────────────
def main():
    from desktop_env.desktop_env import DesktopEnv   # lazy: see header note
    if len(sys.argv) < 2:
        raise SystemExit("usage: battery_calc.py <task_json> [N=3] [cond=AB]")
    task = json.load(open(sys.argv[1]))
    N = int(sys.argv[2]) if len(sys.argv) > 2 else 3
    conds = sys.argv[3] if len(sys.argv) > 3 else "AB"
    file_path = task_input_path(task)
    os.makedirs(LOGDIR, exist_ok=True)
    logf = os.path.join(LOGDIR, "calc_%s.jsonl" % task["id"][:8])
    print("=== BATTERY P1 A/B | task %s | N=%d | conds=%s ===" % (task["id"][:8], N, conds), flush=True)
    print("    instruction: %s" % task["instruction"][:100], flush=True)

    if not memory_ok():                               # FAIL FAST before any boot — never thrash toward OOM
        raise SystemExit(1)
    env = DesktopEnv(provider_name="docker", action_space="pyautogui", screen_size=(1920, 1080),
                     headless=True, os_type="Ubuntu", require_a11y_tree=False)
    results = {"A": [], "B": []}
    logs = []
    try:
        for cond in [c for c in "AB" if c in conds]:
            for run in range(N):
                print("\n--- cond %s run %d/%d ---" % (cond, run + 1, N), flush=True)
                if not memory_ok():
                    print("    stopping early — memory floor breached.", flush=True); break
                env.reset(task_config=task)
                time.sleep(2)
                score, log = run_condition(env, task, cond, file_path, run)
                results[cond].append(score)
                logs.append(log)
                open(logf, "a").write(json.dumps(log, default=str) + "\n")
                print("    score=%s  self_report_done=%s  false_pass=%s  fired=%s" % (
                    score, log.get("self_report_done"), log.get("false_pass"),
                    [f["falsifier"] for f in log.get("falsifiers_fired", [])]), flush=True)
                if cond == "B" and log.get("resolve_fails"):
                    print("    resolve_fails: %s" % log["resolve_fails"], flush=True)
    finally:
        env.close()

    print("\n" + "=" * 64, flush=True)
    for cond in [c for c in "AB" if c in conds]:
        s = results[cond]
        if not s: continue
        golds = sum(1 for x in s if x >= 1.0)
        fp = sum(1 for L in logs if L["cond"] == cond and L.get("false_pass"))
        var = statistics.pstdev(s) if len(s) > 1 else 0.0
        print("  COND %s:  gold %d/%d  mean=%.2f  stdev=%.2f  FALSE_PASSES=%d" % (
            cond, golds, len(s), statistics.mean(s), var, fp), flush=True)
    print("  log: %s" % logf, flush=True)
    # P2 attribution summary
    print("\n  attribution (where B failed):", flush=True)
    for L in logs:
        if L["cond"] == "B" and L.get("score", 0) < 1.0:
            why = "resolve_fail" if L.get("resolve_fails") else (
                  "falsifier:" + ",".join(f["falsifier"] for f in L.get("falsifiers_fired", [])) if L.get("falsifiers_fired")
                  else "silent_wrong(passed falsifiers, oracle=0)")
            print("    run %d: %s" % (L["run"], why), flush=True)

if __name__ == "__main__":
    sys.exit(main())
