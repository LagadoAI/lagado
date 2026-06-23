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
  .venv/bin/python /home/alucard/projects/lagado/docs/osworld/battery_calc.py <task_json> [N=3] [cond=AB]
"""
import json, os, re, sys, time, statistics
import requests

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from run_session_task import (Guest, deploy_daemon, pick_uno_python, task_input_path)
from desktop_env.desktop_env import DesktopEnv

BRAIN = "http://localhost:8080/completion"
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

def detect(g, sheets_detail):
    """For each sheet: find the header row (not assumed row 1), build candidate columns (letter, header,
    samples), and the data-start row. Returns {sheet: {cols, rows, data_start, header_row}}."""
    out = {}
    for sd in sheets_detail:
        name = sd["name"]
        ncols = max(sd["extent"]["cols"], 1)
        nrows = sd["extent"]["rows"]
        lastcol = col_letter(ncols - 1)
        topr = g.client("read", {"sheet": name, "range": "A1:%s%d" % (lastcol, min(max(nrows, 1), 8))})
        top = topr.get("cells", []) if topr.get("ok") else []
        hrow = find_header_row(top)                       # 1-based
        headers = top[hrow - 1] if len(top) >= hrow else []
        samples = top[hrow:hrow + 3] if len(top) > hrow else []
        cands = []
        for c in range(ncols):
            hdr = headers[c] if c < len(headers) and headers[c] is not None else ""
            colsamp = [row[c] for row in samples if c < len(row)]
            cands.append({"letter": col_letter(c), "header": str(hdr), "samples": colsamp, "idx0": c})
        out[name] = {"cols": cands, "rows": nrows, "data_start": hrow + 1, "header_row": hrow}
    return out

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
    'str ::= "\\"" [^"\\\\]* "\\""\n'
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
    ' | "rename_sheet(old=" str ", new=" str ")"\n'
    'str ::= "\\"" [^"\\\\]* "\\""\n'
)

def candidate_cards(detected):
    lines = []
    for sheet, info in detected.items():
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
    "  rename_sheet(old=\"S\", new=\"S2\")                   rename a sheet\n\n"
    "Emit the operations as a list of calls:")

def compose_feedback(fails, fired):
    """Turn read-back faults into a concrete correction note for the next emit (the retry condition)."""
    lines = []
    for f in fails:
        if "apply error" in f.get("why", ""):
            lines.append("- the formula %r failed to apply (%s). Use DOUBLE quotes for text literals "
                         "(\"_\" not '_'), and valid function/sheet names." % (f.get("name"), f.get("why")))
        else:
            lines.append("- could not resolve the name %r (%s). A column on ANOTHER sheet must be qualified "
                         "as {Sheet1.Header}; check the exact header spelling." % (f.get("name"), f.get("why")))
    for f in fired:
        if f["falsifier"] == "error_values":
            lines.append("- the column %s contains error values %s — fix the formula (text literals use "
                         "double quotes; check function names)." % (f["range"], f.get("sample")))
        elif f["falsifier"] == "text_formula_numeric":
            lines.append("- the column %s used a text concatenation but produced NUMBERS %s — the text "
                         "literal is wrong; use DOUBLE quotes (\"_\") for the separator." % (f["range"], f.get("sample")))
        elif f["falsifier"] == "extent_shortfall":
            lines.append("- the column %s left %d cells empty — cover every data row." % (f["range"], f.get("empty")))
    return "\n".join(lines)

CHAT = "http://localhost:8080/v1/chat/completions"   # applies the GGUF's own chat template (model-agnostic)

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

def author_B(instr, detected, log, feedback=None, temperature=0.0):
    cards = candidate_cards(detected)
    seed = int(temperature * 1000) + 7            # vary seed with temp so the 2nd derivation is independent
    # call 1: REASON (no grammar — free reasoning)
    reasoning = _chat(REASON_PROMPT.format(instr=instr, cards=cards),
                      temperature=temperature, seed=seed, max_tokens=400).strip()
    log.setdefault("reasoning", reasoning)
    # call 2: EMIT (grammar-constrained). On retry, append the specific fault.
    emit = EMIT_PROMPT.format(instr=instr, cards=cards, reasoning=reasoning)
    if feedback:
        emit += ("\n\nYour PREVIOUS attempt had these problems — fix exactly these and re-emit ALL "
                 "operations:\n%s" % feedback)
    raw = _chat(emit, grammar=GRAMMAR_B, temperature=temperature, seed=seed, max_tokens=800)
    log.setdefault("emit_raw", [])
    log["emit_raw"].append(raw)
    return parse_B_nameops(raw)

def parse_B_nameops(text):
    """Parse name-level calls (UNRESOLVED — names stay in {braces}). Resolution happens at APPLY time
    against the live re-detected structure, so new sheets / just-set headers resolve."""
    nameops = []
    for verb, body in scan_calls(text, ("compute_column", "set_cell", "add_sheet", "rename_sheet")):
        kw = parse_kv(body)
        if verb == "add_sheet":
            nameops.append({"kind": "add_sheet", "name": kw.get("name")})
        elif verb == "rename_sheet":
            nameops.append({"kind": "rename_sheet", "old": kw.get("old"), "new": kw.get("new")})
        elif verb == "set_cell" and "value" in kw:
            nameops.append({"kind": "set_cell", "sheet": kw.get("sheet"), "cell": kw.get("cell"),
                            "value": coerce(kw["value"])})
        elif verb == "compute_column":
            nameops.append({"kind": "compute_column", "sheet": kw.get("sheet"),
                            "target": kw.get("target"), "formula": kw.get("formula", "")})
    return nameops

def apply_B(g, nameops, log):
    """Interleaved apply: each op applies through the session, then we RE-DETECT so later ops resolve
    against the live world. compute_column names are resolved here (exact+unique or FAIL-CLOSED).
    Returns (written_regions, fails) — written_regions = [(sheet, a1range)] for read-back."""
    live = live_detect(g)
    written, fails = [], []
    for nop in nameops:
        k = nop["kind"]
        if k == "add_sheet":
            if nop["name"] not in live:                 # idempotent (safe to re-run on retry)
                g.client("apply", {"op": {"op": "add_sheet", "name": nop["name"]}})
                live = live_detect(g)
        elif k == "rename_sheet":
            if nop.get("old") != nop.get("new"):
                g.client("apply", {"op": {"op": "rename_sheet", "old": nop["old"], "new": nop["new"]}})
                live = live_detect(g)
        elif k == "set_cell":
            g.client("apply", {"op": {"op": "set", "sheet": nop["sheet"], "cell": nop["cell"],
                                      "value": nop["value"]}})
            live = live_detect(g)  # a set_cell may have written a header → re-perceive
        elif k == "compute_column":
            sheet, target, formula = nop["sheet"], nop["target"], nop["formula"]
            # HARNESS OWNS SYNTAX: LibreOffice string literals need double quotes; LLMs often emit single
            # ('_'), which silently evaluates to 0. The model's quotes are only ever string literals here
            # (sheet refs come from {braces}→`!`), so normalizing ' → " is safe and general.
            formula = formula.replace("'", '"')
            tcol = resolve_name(sheet, target, live, [])     # throwaway fails — unresolved target → create
            if tcol is None:
                tcol = create_target_column(g, sheet, target, live)
                live = live_detect(g)
            ds = live.get(sheet, {}).get("data_start", 2)   # header-row-aware first data row
            refsheets = set()
            a1 = substitute_names(formula, sheet, live, fails, row=ds, refsheets=refsheets)
            # HARNESS OWNS SYNTAX: a compute_column body is ALWAYS a formula. The model inconsistently
            # omits the leading '=' (e.g. "{Sales}-{Sales Return}"); without it setFormula stores the
            # string as TEXT and fillAuto then series-increments the trailing digit ("B2-C2"→"B2-C3"…),
            # so the column never computes. Guarantee the '='. (VM-verified 2026-06-23: '=' present →
            # correct relative fill 75000,69539,…; absent → text series. fillAuto itself is fine.)
            if a1 is not None and not a1.lstrip().startswith("="):
                a1 = "=" + a1.lstrip()
            if a1 is None:
                continue  # fail-closed: a referenced name didn't resolve
            # Extent = data rows of the target OR any sheet the formula references (row-aligned). A
            # fresh target sheet has only its header (1 row); the referenced data sheet sets the span.
            cand = [live.get(sheet, {}).get("rows", 2)] + [live.get(s, {}).get("rows", 2) for s in refsheets]
            extent = max([r for r in cand if r and r >= 2] or [2])
            rng = "%s%d:%s%d" % (tcol, ds, tcol, extent)
            rr = g.client("apply", {"op": {"op": "set_formula_range", "sheet": sheet,
                                           "range": rng, "formula": a1}})
            if not rr.get("ok"):                        # apply-time error (bad formula syntax, etc.)
                fails.append({"name": a1, "range": rng, "why": "apply error: %s" % rr.get("error", "")[:80]})
                continue
            written.append((sheet, rng, a1))
            live = live_detect(g)
    log["resolve_fails"] = fails
    log["written_regions"] = written
    return written, fails

NAME_TOK = re.compile(r"\{([^}]*)\}")

def resolve_name(sheet, name, detected, fails):
    """Exact, unique header match → column letter. Ambiguous/missing → None (fail-closed, logged)."""
    if name is None:
        return None
    if name.strip().startswith("#"):                      # candidate-selection by index
        res = _index_col(sheet, name.strip()[1:], detected, fails)
        return res[1] if res else None
    want = name.strip().lower()
    info = detected.get(sheet)
    if not info:
        fails.append({"name": name, "why": "unknown sheet %r" % sheet}); return None
    hits = [c["letter"] for c in info["cols"] if c["header"].strip().lower() == want]
    if len(hits) == 1:
        return hits[0]
    lc = _letter_col(sheet, name.strip(), detected)    # column-letter target notation
    if lc:
        return lc[1]
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
    try:
        n = int(idx_str)
    except (ValueError, TypeError):
        fails.append({"name": "#%s" % idx_str, "why": "non-integer index"}); return None
    if info and 1 <= n <= len(info["cols"]):
        return (sheet, info["cols"][n - 1]["letter"])
    fails.append({"name": "#%s" % idx_str, "sheet": sheet, "why": "index out of range"}); return None

def _letter_col(sheet, token, detected):
    """Column-LETTER notation: {B} → column B IF B is a real column of `sheet` and no header equals 'B'.
    Letters are unambiguous (unique by construction) → sound, never a mis-bind. Returns (sheet,letter)|None."""
    info = detected.get(sheet)
    if not info or not re.fullmatch(r"[A-Za-z]+", token):
        return None
    L = token.upper()
    letters = [c["letter"] for c in info["cols"]]
    if L in letters and not any(c["header"].strip().upper() == L for c in info["cols"]):
        return (sheet, L)
    return None

def resolve_ref(token, default_sheet, detected, fails):
    """Resolve a formula reference to (sheet, letter), accepting ANY unambiguous notation — the harness
    owns notation so the model's choice of style can't break correctness. Order per sheet: exact unique
    HEADER → column LETTER ({B}) → index ({#N}); bare names also try WORKBOOK-WIDE unique header. Ambiguous
    /missing → None (fail-closed, logged). Sound: letters+indices are unique; headers fail-closed on dup."""
    token = token.strip()
    if "." in token:
        sh, _, hdr = token.partition(".")
        sh = sh.strip(); hdr = hdr.strip()
        if hdr.startswith("#"):
            return _index_col(sh, hdr[1:], detected, fails)
        info = detected.get(sh)
        if not info:
            fails.append({"name": token, "why": "unknown sheet %r" % sh}); return None
        hits = [c["letter"] for c in info["cols"] if c["header"].strip().lower() == hdr.lower()]
        if len(hits) == 1:
            return (sh, hits[0])
        lc = _letter_col(sh, hdr, detected)
        if lc:
            return lc
        fails.append({"name": token, "sheet": sh, "why": "%d header matches (need 1)" % len(hits)}); return None
    if token.startswith("#"):
        return _index_col(default_sheet, token[1:], detected, fails)
    want = token.lower()
    info = detected.get(default_sheet)
    if info:
        hits = [c["letter"] for c in info["cols"] if c["header"].strip().lower() == want]
        if len(hits) == 1:
            return (default_sheet, hits[0])
        if len(hits) > 1:
            fails.append({"name": token, "sheet": default_sheet, "why": "%d on-sheet matches" % len(hits)}); return None
    allhits = [(s, c["letter"]) for s, i in detected.items()
               for c in i["cols"] if c["header"].strip().lower() == want]
    if len(allhits) == 1:
        return allhits[0]
    lc = _letter_col(default_sheet, token, detected)   # column-letter notation, default sheet
    if lc:
        return lc
    fails.append({"name": token, "why": "%d workbook matches (need exactly 1)" % len(allhits)}); return None

def substitute_names(formula, default_sheet, detected, fails, row, refsheets=None):
    """Replace {Header} / {Sheet.Header} with A1 refs at the given row. Cross-sheet refs use the proven
    Excel `Sheet!Cell` syntax. Fail-closed: any token that doesn't uniquely resolve aborts (returns None).
    refsheets (if given) collects every sheet the resolved formula references (for extent alignment)."""
    aborted = [False]
    def repl(m):
        res = resolve_ref(m.group(1).strip(), default_sheet, detected, fails)
        if res is None:
            aborted[0] = True; return m.group(1)
        sh, letter = res
        if refsheets is not None:
            refsheets.add(sh)
        r = detected.get(sh, {}).get("data_start", row)   # each ref uses its own sheet's first data row
        ref = "%s%d" % (letter, r)
        return ref if sh == default_sheet else "%s!%s" % (sh, ref)
    out = NAME_TOK.sub(repl, formula)
    return None if aborted[0] else out

# ── READ-BACK + SOUND FALSIFIERS (falsify only; pass ≠ correct) ──────────────────
def falsify(g, written_regions):
    """written_regions = [(sheet, a1range, formula)]. Return list of FIRED falsifiers.
    Empty list = 'no detected fault' — NOT 'correct' (the oracle is the only correctness signal).
    All falsifiers are SOUND (they can only detect wrongness, never confirm correctness)."""
    fired = []
    for sheet, rng, formula in written_regions:
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
            tcol = resolve_name(nop["sheet"], nop["target"], live, [])
            if tcol:
                ds = live.get(nop["sheet"], {}).get("data_start", 2)
                der2_f[(nop["sheet"], tcol)] = substitute_names(nop["formula"].replace("'", '"'),
                                                                nop["sheet"], live, [], row=ds)
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

# ── one run of a condition ───────────────────────────────────────────────────────
def run_condition(env, task, cond, file_path, run_idx):
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
    r = g.client("open", {"file": file_path})
    if not r.get("ok"):
        log["fatal"] = "open failed: %s" % r.get("error"); return 0.0, log
    detail = r.get("structure", {}).get("detail", [])
    log["steps"].append("detect")

    instr = task["instruction"]
    resolve_fails, fired, written = [], [], []
    if cond == "A":
        detected = detect(g, detail)               # SAME fixed detector as B → A sees real headers too
        log["detected"] = {s: [(c["letter"], c["header"]) for c in i["cols"]] for s, i in detected.items()}
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
        log["detected"] = {s: [(c["letter"], c["header"]) for c in i["cols"]] for s, i in detected.items()}
        feedback = None
        attempt = 0
        for attempt in range(2):                  # reason→emit, then ONE read-back retry (the ReAct condition)
            log["steps"].append("attempt%d" % attempt)
            nameops = author_B(instr, detected, log, feedback)
            log["nameops"] = nameops
            written, resolve_fails = apply_B(g, nameops, log)
            fired = falsify(g, written)
            log["n_ops"] = len(nameops)
            if written and not resolve_fails and not fired:
                break                            # no detected fault — stop (NOT a correctness claim)
            feedback = compose_feedback(resolve_fails, fired)
            log.setdefault("feedbacks", []).append(feedback)
        log["attempts"] = attempt + 1

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

    g.client("reconcile", {"gui": True})
    g.client("close")
    time.sleep(4)
    score = env.evaluate() or 0.0
    log["score"] = score
    # P5 calibration pair + false-pass flag (the integrity core)
    log["false_pass"] = bool(harness_reports_done and score < 1.0)
    return score, log

# ── main ─────────────────────────────────────────────────────────────────────────
def main():
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

    env = DesktopEnv(provider_name="docker", action_space="pyautogui", screen_size=(1920, 1080),
                     headless=True, os_type="Ubuntu", require_a11y_tree=False)
    results = {"A": [], "B": []}
    logs = []
    try:
        for cond in [c for c in "AB" if c in conds]:
            for run in range(N):
                print("\n--- cond %s run %d/%d ---" % (cond, run + 1, N), flush=True)
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
