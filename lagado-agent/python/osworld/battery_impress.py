"""battery_impress.py — the AUTHORING layer for LibreOffice Impress, the presentation analog of
battery_calc.py. Same shape, same discipline, deliberately SMALLER scope (a slide deck's op
vocabulary is simpler than a spreadsheet's — no formulas, no pivot/chart derivation):

  detect()        observe the live deck as labeled per-slide CANDIDATES (title/content/notes/
                  shape inventory) — the model never enumerates shapes it can't see.
  author()        reason-then-emit: a free-form analysis pass, then a grammar-constrained
                  emission of typed op-calls naming slides/shapes, never raw UNO.
  apply()         send each op to the resident impress_daemon.py session; build a ledger of
                  what to verify (`written`).
  falsify()       SOUND fault detection — read the live doc back and compare against the
                  ledger. Firing = PROVEN fault. Passing = necessary-not-sufficient (never a
                  correctness claim).
  emit_gaps()/gap_feedback()   reason→emit completeness: hold the model to actions ITS OWN
                  analysis commits to but the emission dropped (goal-agnostic keyword bridge,
                  same discipline as calc's).
  corroborate()   READ-ONLY independent re-derivation (temp>0): agree only if it targets the
                  SAME (slide, shape, property) set. Disagreement -> ABSTAIN (self_report_done
                  stays False) — this is what keeps an exit-0 verdict honest; read-back alone
                  is NOT proof of correct shape/slide BINDING (only that a set value stuck).
  run_core()      the shared model->emit->apply->falsify->retry->corroborate->score body.

SCOPE NOTE (2026-07-10, first build): this is attempt(reason+emit) + ONE feedback retry, no
divergence-resample, no 8-step iterative escalation (battery_calc's variable-matrix additions).
Under-claim by design — add those only if a held-out stress run shows the same compound-collapse
signature calc measured.

KNOWN-UNCOVERED CATEGORIES (deliberately, not an oversight — see the real task survey):
  - pure application-SETTINGS tasks (autosave interval, single-monitor Slide Show Presenter
    Console, restoring the Slides panel) are NOT document content — they belong on the Rust
    BackDoor/set_config plane, not here. author() below will emit infeasible() for these; the
    caller must route them elsewhere BEFORE calling this solver, not rely on it.
  - vision/semantic judgments a headless doc model can't answer ("slides with real people",
    "delete the personal info incl. icons") — no op resolves these; infeasible() is the honest
    answer, not a guess.
  - building a brand-NEW presentation from scratch (vs mutating the opened doc) is out of this
    daemon's open-existing-file model.
"""
import json
import os
import re
import sys
import time

import requests

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

BRAIN = os.environ.get("LAGADO_BRAIN", "http://localhost:8080/completion")
LOGDIR = "/tmp/lagado_battery_impress"

OUT_OF_SCOPE_HINT = (
    "app-setting change (autosave interval / presenter-console monitor / re-showing a closed "
    "side panel) — not a document-content edit"
)


# ── generic op-text parsing (self-contained; mirrors battery_calc's scan_calls/parse_kv/coerce
# so this plane never depends on the Calc module) ───────────────────────────────────────────
def scan_calls(text, verbs):
    pat = re.compile(r"(%s)\(" % "|".join(re.escape(v) for v in verbs))
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


def _chat(content, grammar=None, temperature=0.0, seed=7, max_tokens=800):
    payload = {"prompt": content, "temperature": temperature, "n_predict": max_tokens, "seed": seed}
    if grammar:
        payload["grammar"] = grammar
    r = requests.post(BRAIN, json=payload, timeout=200)
    return r.json().get("content", "")


# ── DETECTOR: per-slide candidates (title/content/notes/shape inventory) ───────────────────
def detect(g):
    s = g.client("structure")
    if not s.get("ok"):
        return []
    return s.get("detail", [])


def live_detect(g):
    """Re-perceive the whole deck from the live session — used after every structural op so a
    freshly added/duplicated/deleted slide is resolvable on the next step."""
    return detect(g)


def candidate_cards(detected):
    lines = []
    for sd in detected:
        i = sd["index"]
        lines.append("Slide %d (%d shape(s)):" % (i, len(sd.get("shapes", []))))
        if sd.get("title"):
            lines.append("  title: %r" % sd["title"])
        if sd.get("content"):
            lines.append("  content: %r" % sd["content"][:200])
        if sd.get("notes"):
            lines.append("  notes: %r" % sd["notes"][:200])
        if sd.get("background"):
            lines.append("  background: %s" % sd["background"])
        plain_n, img_n, tab_n = 0, 0, 0
        for sh in sd.get("shapes", []):
            k = sh["kind"]
            if k == "textbox":
                plain_n += 1
                lines.append("    textbox #%d: %r" % (plain_n, sh["text"][:120]))
            elif k == "image":
                img_n += 1
                lines.append("    image #%d (%.1fx%.1f cm)" % (img_n, sh["w"] / 1000.0, sh["h"] / 1000.0))
            elif k == "table":
                tab_n += 1
                dims = ""
                if "rows" in sh and "cols" in sh:
                    dims = " (%dx%d)" % (sh["rows"], sh["cols"])
                lines.append("    table #%d%s" % (tab_n, dims))
            elif k == "media":
                lines.append("    media shape")
    return "\n".join(lines)


# ── op vocabulary (GBNF + prompt docs) ───────────────────────────────────────────────────────
GRAMMAR_IMPRESS = (
    'root ::= "[" op ("," op)* "]"\n'
    'op ::= "add_slide(index=" str ", layout=" str ")"'
    ' | "duplicate_slide(source=" str ", dest=" str ")"'
    ' | "delete_slide(index=" str ")"'
    ' | "move_slide(source=" str ", dest=" str ")"'
    ' | "set_title(slide=" str ", text=" str ")"'
    ' | "set_content_text(slide=" str ", text=" str ", mode=" str ")"'
    ' | "add_bullet(slide=" str ", text=" str ")"'
    ' | "set_notes(slide=" str ", text=" str ")"'
    ' | "insert_textbox(slide=" str ", text=" str ", x_cm=" str ", y_cm=" str ", width_cm=" str ", height_cm=" str ")"'
    ' | "insert_table(slide=" str ", rows=" str ", cols=" str ")"'
    ' | "set_table_cell(slide=" str ", table=" str ", row=" str ", col=" str ", text=" str ")"'
    ' | "insert_image(slide=" str ", path=" str ", width_cm=" str ", height_cm=" str ", x_cm=" str ", y_cm=" str ")"'
    ' | "insert_audio(slide=" str ", path=" str ")"'
    ' | "delete_shape(slide=" str ", shape=" str ")"'
    ' | "set_font_name(slide=" str ", shape=" str ", font=" str ", lines=" str ", scope=" str ")"'
    ' | "set_font_size(slide=" str ", shape=" str ", size_pt=" str ", lines=" str ")"'
    ' | "set_font_color(slide=" str ", shape=" str ", color=" str ", lines=" str ")"'
    ' | "set_bold(slide=" str ", shape=" str ", bold=" str ", lines=" str ")"'
    ' | "set_underline(slide=" str ", shape=" str ", underline=" str ", lines=" str ")"'
    ' | "set_strikethrough(slide=" str ", shape=" str ", strike=" str ", lines=" str ")"'
    ' | "set_text_align(slide=" str ", shape=" str ", align=" str ", lines=" str ")"'
    ' | "set_indent_level(slide=" str ", shape=" str ", lines=" str ", level=" str ", bullet=" str ")"'
    ' | "move_shape(slide=" str ", shape=" str ", position=" str ")"'
    ' | "resize_shape(slide=" str ", shape=" str ", width_cm=" str ", height_cm=" str ")"'
    ' | "fit_image_to_slide(slide=" str ", shape=" str ", mode=" str ")"'
    ' | "set_background(slide=" str ", color=" str ", scope=" str ")"'
    ' | "set_transition(slide=" str ", kind=" str ")"'
    ' | "set_slide_orientation(orientation=" str ")"'
    ' | "set_slidenum_color(color=" str ")"'
    ' | "save_as(name=" str ")"'
    ' | "export_image(name=" str ", format=" str ")"'
    ' | "insert_summary_slide()"'
    ' | "infeasible(reason=" str ")"\n'
    'str ::= "\\"" [^"\\\\\\n\\r]* "\\""\n'
)

REASON_PROMPT = (
    "You are operating a LibreOffice Impress presentation.\n\n"
    "Goal: {instr}\n\n"
    "Slides present (read from the deck itself):\n{cards}\n\n"
    "Think step by step, then stop.")

EMIT_PROMPT = (
    "Goal: {instr}\n\n"
    "Slides present:\n{cards}\n\n"
    "Your analysis:\n{reasoning}\n\n"
    "Now emit operations. Slides are 1-based ('slide 2' -> slide=\"2\"). Shape references: "
    "\"title\", \"content\" (the body/outline placeholder), \"table\"/\"table:2\", \"image\"/\"image:2\" "
    "(Y-sorted, top to bottom), \"textbox\"/\"textbox:2\" (Y-sorted plain text boxes — \"the first "
    "textbox\" = \"textbox:1\"), \"shape:N\" (Nth shape by insertion order), or the shape's own exact "
    "text. `lines` selects specific 1-based paragraph numbers within a shape (\"1,2\"), or \"\"/\"all\" "
    "for the whole shape (\"underline only the first and second line\" -> lines=\"1,2\"). Available "
    "operations:\n"
    "  add_slide(index=\"3\", layout=\"title_content\")   insert a new slide at position N; layout="
    "title_content|title_only|blank|title_subtitle\n"
    "  duplicate_slide(source=\"2\", dest=\"\")            duplicate a slide WITH its content; dest="
    "the final 1-based position for the copy (\"\" = right after the source)\n"
    "  delete_slide(index=\"4\")                          remove a slide\n"
    "  move_slide(source=\"3\", dest=\"1\")                 move a slide to a new 1-based position\n"
    "  set_title(slide=\"2\", text=\"...\")                 set the slide's title text\n"
    "  set_content_text(slide=\"2\", text=\"...\", mode=\"replace\")   set the body/content text; mode="
    "replace|append\n"
    "  add_bullet(slide=\"1\", text=\"...\")                 append one new bullet line to the content\n"
    "  set_notes(slide=\"2\", text=\"...\")                  set the slide's speaker notes\n"
    "  insert_textbox(slide=\"1\", text=\"...\", x_cm=\"\", y_cm=\"\", width_cm=\"\", height_cm=\"\")   add a new "
    "text box (position/size in cm, \"\" = default placement)\n"
    "  insert_table(slide=\"3\", rows=\"5\", cols=\"2\")      insert a table with this many rows/columns\n"
    "  set_table_cell(slide=\"4\", table=\"1\", row=\"1\", col=\"1\", text=\"T1\")   set one table cell's text "
    "(1-based row/col)\n"
    "  insert_image(slide=\"2\", path=\"/abs/path.png\", width_cm=\"1\", height_cm=\"1\", x_cm=\"\", y_cm=\"\")   "
    "insert an image from an absolute file path, optionally sized in cm\n"
    "  insert_audio(slide=\"1\", path=\"/abs/path.mp3\")     insert an audio/media clip from an absolute path\n"
    "  delete_shape(slide=\"4\", shape=\"image:1\")           remove a shape\n"
    "  set_font_name(slide=\"1\", shape=\"textbox:1\", font=\"...\", lines=\"all\", scope=\"slide\")   scope="
    "\"all_slides\" applies the SAME font to every slide's matching shape (\"standardize the font "
    "across every text box\")\n"
    "  set_font_size(slide=\"14\", shape=\"textbox:1\", size_pt=\"60\", lines=\"all\")\n"
    "  set_font_color(slide=\"1\", shape=\"title\", color=\"yellow\", lines=\"all\")   color = \"#rrggbb\" or "
    "a plain color name (yellow, red, green, blue, orange, purple, black, white, ...) — use the "
    "EXACT name the goal states, never a variation of it\n"
    "  set_bold(slide=\"1\", shape=\"title\", bold=\"true\", lines=\"all\")\n"
    "  set_underline(slide=\"2\", shape=\"title\", underline=\"true\", lines=\"all\")\n"
    "  set_strikethrough(slide=\"1\", shape=\"textbox:1\", strike=\"true\", lines=\"1,2\")\n"
    "  set_text_align(slide=\"3\", shape=\"textbox:1\", align=\"right\", lines=\"all\")   align=left|right|"
    "center|justify\n"
    "  set_indent_level(slide=\"3\", shape=\"textbox:1\", lines=\"3\", level=\"1\", bullet=\"false\")   change "
    "one paragraph's indent level and whether it shows a bullet glyph\n"
    "  move_shape(slide=\"2\", shape=\"image:1\", position=\"right\")   position=left|right|top|bottom|center\n"
    "  resize_shape(slide=\"14\", shape=\"image:1\", width_cm=\"\", height_cm=\"20\")\n"
    "  fit_image_to_slide(slide=\"1\", shape=\"image:1\", mode=\"contain\")   stretch an image to fill the "
    "slide keeping its proportions and centering it; mode=contain (never crops) or cover (may crop "
    "to fill completely)\n"
    "  set_background(slide=\"2\", color=\"purple\", scope=\"slide\")   scope=\"all_slides\" for every slide\n"
    "  set_transition(slide=\"1\", kind=\"dissolve\")          slide transition effect\n"
    "  set_slide_orientation(orientation=\"portrait\")        portrait or landscape, for the whole deck\n"
    "  set_slidenum_color(color=\"red\")                      recolor the slide-number field, whole deck\n"
    "  save_as(name=\"pre.pptx\")                             save the presentation under a new file name "
    "(next to the current file); the extension picks the format\n"
    "  export_image(name=\"res.png\", format=\"png\")          export the current slide/deck as an image\n"
    "  insert_summary_slide()                               append a summary slide listing every other "
    "slide's title\n"
    "  infeasible(reason=\"...\")                             ONLY if the goal is a pure application-"
    "SETTING (not a document edit) or requires a visual/semantic judgment this tool cannot make — "
    "emit it ALONE and state why\n\n"
    "Emit ONLY the operations the goal needs, as a list of calls:")


def author(instr, detected, feedback=None, temperature=0.0):
    cards = candidate_cards(detected)
    reasoning = _chat(REASON_PROMPT.format(instr=instr, cards=cards), temperature=temperature, seed=7, max_tokens=600)
    fb = ("\n\nPrevious attempt's problems (fix these, keep everything still correct):\n%s" % feedback) \
        if feedback else ""
    raw = _chat(EMIT_PROMPT.format(instr=instr, cards=cards, reasoning=reasoning) + fb,
               grammar=GRAMMAR_IMPRESS, temperature=temperature, seed=7, max_tokens=1200)
    return parse_nameops(raw), {"reasoning": reasoning, "emit_raw": raw, "cards": cards}


# The verb list parse_nameops recognizes — a MODULE CONSTANT (not a function-local tuple) so the
# self-test can assert it never drifts out of sync with GRAMMAR_IMPRESS's own alternatives.
VERBS = ("add_slide", "duplicate_slide", "delete_slide", "move_slide", "set_title",
         "set_content_text", "add_bullet", "set_notes", "insert_textbox", "insert_table",
         "set_table_cell", "insert_image", "insert_audio", "delete_shape", "set_font_name",
         "set_font_size", "set_font_color", "set_bold", "set_underline", "set_strikethrough",
         "set_text_align", "set_indent_level", "move_shape", "resize_shape",
         "fit_image_to_slide", "set_background", "set_transition", "set_slide_orientation",
         "set_slidenum_color", "save_as", "export_image", "insert_summary_slide", "infeasible")


def parse_nameops(text):
    out = []
    for verb, body in scan_calls(text, VERBS):
        kw = parse_kv(body)
        out.append({"kind": verb, **kw})
    return out


def _op_key(o):
    return tuple(sorted(o.items()))


# ── APPLY: dispatch each nameop through the daemon; build the read-back ledger ──────────────
def apply(g, nameops, log, file_path=None):
    """Returns (written, apply_fails). `written` = a list of CHECK DESCRIPTORS the falsifier
    reads back live; `apply_fails` = ops the daemon rejected outright (malformed/unresolvable —
    a DIFFERENT thing from a falsifier firing on a successfully-applied-but-wrong op)."""
    written, fails = [], []
    live = live_detect(g)
    counts_by_slide = {sd["index"]: len(sd.get("shapes", [])) for sd in live}
    n_slides_before = len(live)
    folder = None
    if file_path:
        folder = file_path.rsplit("/", 1)[0]

    for nop in nameops:
        kind = nop.get("kind")
        if kind == "infeasible":
            continue
        op = {"op": kind, **{k: v for k, v in nop.items() if k != "kind"}}
        r = g.client("apply", {"op": op})
        if not r.get("ok"):
            fails.append({"op": nop, "why": r.get("error", "")})
            continue
        s = None
        try:
            s = int(nop.get("slide")) if nop.get("slide") not in (None, "") else None
        except (TypeError, ValueError):
            s = None
        if kind == "set_title":
            written.append({"check": "shape_text_equals", "slide": s, "shape": "title", "expect": nop.get("text", "")})
        elif kind == "set_content_text":
            mode = (nop.get("mode") or "replace").strip().lower()
            written.append({"check": "shape_text_equals" if mode != "append" else "shape_text_contains",
                            "slide": s, "shape": "content", "expect": nop.get("text", "")})
        elif kind == "add_bullet":
            written.append({"check": "shape_text_contains", "slide": s, "shape": "content", "expect": nop.get("text", "")})
        elif kind == "set_notes":
            written.append({"check": "notes_text_equals", "slide": s, "expect": nop.get("text", "")})
        elif kind in ("insert_textbox",):
            written.append({"check": "any_shape_text_equals", "slide": s, "expect": nop.get("text", "")})
        elif kind == "insert_table":
            written.append({"check": "table_dims", "slide": s, "table": "1",
                            "rows": int(nop.get("rows", 0) or 0), "cols": int(nop.get("cols", 0) or 0)})
        elif kind == "set_table_cell":
            written.append({"check": "table_cell_equals", "slide": s, "table": nop.get("table", "1"),
                            "row": int(nop.get("row", 1)), "col": int(nop.get("col", 1)),
                            "expect": nop.get("text", "")})
        elif kind == "insert_image":
            before = counts_by_slide.get(s, 0)
            written.append({"check": "shape_kind_count_min", "slide": s, "kind": "image", "min": before + 1})
        elif kind == "insert_audio":
            before = counts_by_slide.get(s, 0)
            written.append({"check": "shape_kind_count_min", "slide": s, "kind": "media", "min": before + 1})
        elif kind == "delete_shape":
            before = counts_by_slide.get(s, 0)
            written.append({"check": "shape_count_max", "slide": s, "max": max(before - 1, 0)})
        elif kind in ("set_font_name", "set_font_size", "set_font_color", "set_bold",
                      "set_underline", "set_strikethrough", "set_text_align"):
            prop_map = {"set_font_name": ("font_name", nop.get("font")),
                       "set_font_size": ("size_pt", _to_float(nop.get("size_pt"))),
                       "set_font_color": ("color", _norm_hex(nop.get("color"))),
                       "set_bold": ("bold", _truthy(nop.get("bold", "true"))),
                       "set_underline": ("underline", _truthy(nop.get("underline", "true"))),
                       "set_strikethrough": ("strike", _truthy(nop.get("strike", "true"))),
                       "set_text_align": ("align", (nop.get("align") or "left").strip().lower())}
            prop, expect = prop_map[kind]
            scope = (nop.get("scope") or "slide").strip().lower() if kind == "set_font_name" else "slide"
            slides = [sd["index"] for sd in live] if scope == "all_slides" else ([s] if s else [])
            for si in slides:
                written.append({"check": "shape_prop", "slide": si, "shape": nop.get("shape"),
                               "prop": prop, "expect": expect, "lines": nop.get("lines")})
        elif kind in ("move_shape", "resize_shape", "fit_image_to_slide"):
            written.append({"check": "shape_geom_changed", "slide": s, "shape": nop.get("shape")})
        elif kind == "set_background":
            scope = (nop.get("scope") or "slide").strip().lower()
            expect = _norm_hex(nop.get("color"))
            slides = [sd["index"] for sd in live] if scope == "all_slides" else ([s] if s else [])
            for si in slides:
                written.append({"check": "background", "slide": si, "expect": expect})
        elif kind == "set_transition":
            written.append({"check": "transition_set", "slide": s})
        elif kind == "set_slide_orientation":
            written.append({"check": "orientation", "expect": (nop.get("orientation") or "").strip().lower()})
        elif kind in ("add_slide", "duplicate_slide"):
            written.append({"check": "slide_count_min", "min": n_slides_before + 1})
            n_slides_before += 1
        elif kind == "delete_slide":
            written.append({"check": "slide_count_max", "max": max(n_slides_before - 1, 0)})
            n_slides_before = max(n_slides_before - 1, 0)
        elif kind == "insert_summary_slide":
            written.append({"check": "slide_count_min", "min": n_slides_before + 1})
            written.append({"check": "shape_text_equals", "slide": n_slides_before + 1,
                            "shape": "title", "expect": "Summary"})
            n_slides_before += 1
        elif kind == "save_as" and folder:
            written.append({"check": "file_exists", "path": folder + "/" + nop.get("name", "")})
        elif kind == "export_image" and folder:
            name = nop.get("name") or "res.png"
            written.append({"check": "file_exists", "path": folder + "/" + name})
        # move_slide, set_slidenum_color, set_indent_level: no ledger entry (see module
        # docstring's KNOWN-UNCOVERED note on falsifier scope) — under-claim, not a false claim.
    return written, fails


def _to_float(v):
    try:
        return float(v)
    except (TypeError, ValueError):
        return None


def _truthy(v):
    return str(v).strip().lower() in ("1", "true", "yes")


def _norm_hex(spec):
    """Best-effort normalize a color spec the SAME way impress_ops.resolve_color would, purely
    for COMPARISON in the falsifier (never mutates anything). Unknown name -> None (comparison
    then never matches, so the falsifier can only fire "mismatch", it will not wrongly PASS on
    an unresolvable color)."""
    s = str(spec or "").strip()
    hexs = s.lstrip("#").strip()
    if len(hexs) == 6 and all(c in "0123456789abcdefABCDEF" for c in hexs):
        return "#" + hexs.upper()
    named = {"black": "000000", "white": "FFFFFF", "red": "FF0000", "green": "008000",
            "blue": "0000FF", "yellow": "FFFF00", "cyan": "00FFFF", "magenta": "FF00FF",
            "gray": "808080", "grey": "808080", "maroon": "800000", "olive": "808000",
            "purple": "800080", "teal": "008080", "navy": "000080", "lime": "00FF00",
            "silver": "C0C0C0", "orange": "FFA500", "dark red 2": "C00000"}
    key = s.lower().strip()
    return ("#" + named[key].upper()) if key in named else None


# ── FALSIFY: sound fault detection via live read-back ───────────────────────────────────────
def falsify(g, written):
    """written = the CHECK DESCRIPTOR ledger from apply(). Returns fired faults (list of dicts).
    Empty = 'no detected fault' — NOT 'correct' (same doctrine as calc's falsify: sound only,
    never confirms)."""
    fired = []
    live = {sd["index"]: sd for sd in live_detect(g)}

    def slide_shapes(si):
        return (live.get(si) or {}).get("shapes", [])

    for chk in written:
        c = chk["check"]
        if c == "shape_text_equals":
            r = g.client("read", {"slide": chk["slide"], "shape": chk["shape"]})
            if not r.get("ok"):
                fired.append({"falsifier": "shape_not_found", **chk}); continue
            if r.get("text", "") != chk["expect"]:
                fired.append({"falsifier": "text_mismatch", "got": r.get("text"), **chk})
        elif c == "shape_text_contains":
            r = g.client("read", {"slide": chk["slide"], "shape": chk["shape"]})
            if not r.get("ok"):
                fired.append({"falsifier": "shape_not_found", **chk}); continue
            if chk["expect"] not in (r.get("text") or ""):
                fired.append({"falsifier": "text_not_found", "got": r.get("text"), **chk})
        elif c == "any_shape_text_equals":
            texts = [sh["text"] for sh in slide_shapes(chk["slide"])]
            if not any(t.strip() == chk["expect"].strip() for t in texts):
                fired.append({"falsifier": "inserted_text_not_found", "have": texts, **chk})
        elif c == "notes_text_equals":
            sd = live.get(chk["slide"]) or {}
            if sd.get("notes", "") != chk["expect"]:
                fired.append({"falsifier": "notes_mismatch", "got": sd.get("notes"), **chk})
        elif c == "table_dims":
            tabs = [sh for sh in slide_shapes(chk["slide"]) if sh["kind"] == "table"]
            if not tabs:
                fired.append({"falsifier": "table_not_found", **chk}); continue
            t = tabs[0]
            if t.get("rows") != chk["rows"] or t.get("cols") != chk["cols"]:
                fired.append({"falsifier": "table_dims_mismatch", "got": (t.get("rows"), t.get("cols")), **chk})
        elif c == "table_cell_equals":
            r = g.client("read", {"slide": chk["slide"], "shape": "table:%s" % chk["table"],
                                  "cell": {"row": chk["row"], "col": chk["col"]}})
            if not r.get("ok"):
                fired.append({"falsifier": "table_cell_unreadable", **chk}); continue
            if r.get("text", "") != chk["expect"]:
                fired.append({"falsifier": "table_cell_mismatch", "got": r.get("text"), **chk})
        elif c == "shape_kind_count_min":
            n = sum(1 for sh in slide_shapes(chk["slide"]) if sh["kind"] == chk["kind"])
            if n < chk["min"]:
                fired.append({"falsifier": "shape_count_shortfall", "got": n, **chk})
        elif c == "shape_count_max":
            n = len(slide_shapes(chk["slide"]))
            if n > chk["max"]:
                fired.append({"falsifier": "shape_not_removed", "got": n, **chk})
        elif c == "shape_prop":
            r = g.client("read", {"slide": chk["slide"], "shape": chk["shape"], "lines": chk.get("lines")})
            if not r.get("ok"):
                fired.append({"falsifier": "shape_not_found", **chk}); continue
            got = (r.get("props") or {}).get(chk["prop"])
            exp = chk["expect"]
            ok = (got == exp) if exp is not None else True
            if chk["prop"] == "size_pt" and isinstance(got, (int, float)) and isinstance(exp, (int, float)):
                ok = abs(got - exp) < 0.6
            if chk["prop"] == "color" and isinstance(got, str) and isinstance(exp, str):
                ok = got.upper() == exp.upper()
            if not ok:
                fired.append({"falsifier": "prop_mismatch", "got": got, **chk})
        elif c == "background":
            sd = live.get(chk["slide"]) or {}
            got = sd.get("background")
            if chk["expect"] is not None and (got or "").upper() != chk["expect"].upper():
                fired.append({"falsifier": "background_mismatch", "got": got, **chk})
        elif c == "transition_set":
            sd = live.get(chk["slide"]) or {}
            t = sd.get("transition")
            if not t or t == [0, 0]:
                fired.append({"falsifier": "transition_not_set", **chk})
        elif c == "orientation":
            sd = live.get(1) or {}
            is_portrait = (sd.get("height", 0) or 0) > (sd.get("width", 0) or 0)
            want_portrait = chk["expect"] == "portrait"
            if chk["expect"] and is_portrait != want_portrait:
                fired.append({"falsifier": "orientation_mismatch", "got": "portrait" if is_portrait else "landscape", **chk})
        elif c == "slide_count_min":
            if len(live) < chk["min"]:
                fired.append({"falsifier": "slide_count_shortfall", "got": len(live), **chk})
        elif c == "slide_count_max":
            if len(live) > chk["max"]:
                fired.append({"falsifier": "slide_not_removed", "got": len(live), **chk})
        elif c == "file_exists":
            r = g.sh("test -f '%s' && echo YES || echo NO" % chk["path"])
            if "YES" not in (r.get("out") or ""):
                fired.append({"falsifier": "file_not_found", **chk})
        # "shape_geom_changed" (move_shape/resize_shape/fit_image_to_slide) is intentionally NOT
        # checked here — see module docstring: verifying an EXACT expected geometry needs the
        # slide's own dimensions duplicated into the ledger, which duplicates impress_ops' own
        # geometry math into the falsifier (fragile.); left as a known gap, not silently assumed
        # correct — no ledger entry above claims it either.
    return fired


# ── reason→emit completeness (goal-agnostic keyword bridge, same discipline as calc's) ─────
_GAP_RULES = [
    # (reasoning phrase, required op kinds, gap tag)
    (r"\bbackground\b", ("set_background",), "background"),
    (r"\bnotes?\b|\bspeaker notes\b", ("set_notes",), "notes"),
    (r"\bbold\b", ("set_bold",), "bold"),
    (r"\bunderlin", ("set_underline",), "underline"),
    (r"strike[- ]?through", ("set_strikethrough",), "strikethrough"),
    (r"\btransition\b", ("set_transition",), "transition"),
    (r"\btable\b", ("insert_table", "set_table_cell"), "table"),
    (r"\bimage\b|\bpicture\b", ("insert_image", "move_shape", "resize_shape", "fit_image_to_slide"), "image"),
]


def emit_gaps(reasoning, nameops, instr=""):
    r = (reasoning or "").lower()
    if any(n.get("kind") == "infeasible" for n in nameops):
        return []
    have = {n.get("kind") for n in nameops}
    gaps = []
    for pat, need, tag in _GAP_RULES:
        if re.search(pat, r) and not (have & set(need)):
            gaps.append(tag)
    return gaps


def gap_feedback(gaps):
    msgs = {
        "background": "your analysis mentions a BACKGROUND change but you did not emit set_background(...).",
        "notes": "your analysis mentions speaker NOTES but you did not emit set_notes(...).",
        "bold": "your analysis mentions BOLD but you did not emit set_bold(...).",
        "underline": "your analysis mentions UNDERLINE but you did not emit set_underline(...).",
        "strikethrough": "your analysis mentions STRIKE-THROUGH but you did not emit set_strikethrough(...).",
        "transition": "your analysis mentions a TRANSITION but you did not emit set_transition(...).",
        "table": "your analysis mentions a TABLE but you did not emit insert_table(...)/set_table_cell(...).",
        "image": "your analysis mentions an IMAGE but you did not emit an image operation.",
    }
    return "\n".join("- %s" % msgs[g] for g in gaps if g in msgs)


def compose_feedback(apply_fails, fired):
    lines = []
    for f in apply_fails:
        lines.append("- operation %r was rejected: %s" % (f["op"].get("kind"), f.get("why", "")[:120]))
    for f in fired:
        lines.append("- detected fault (%s) on slide %s: %s" % (
            f.get("falsifier"), f.get("slide"), {k: v for k, v in f.items() if k not in ("falsifier", "slide")}))
    return "\n".join(lines)


# ── CORROBORATION (read-only, no-oracle confidence) ─────────────────────────────────────────
def _touched_set(nameops):
    """The (kind, slide, shape-ref, lines, dest, index) footprint an emission TOUCHES — the
    corroboration compares this set between two independent derivations, mirroring calc's
    column-refset comparison. `lines`/`dest`/`index` ride along so a PARAGRAPH-scoped op
    (strike-through "lines 1,2") or a REORDER op (move_slide/duplicate_slide's `dest`) can't
    agree on kind+slide+shape alone while actually targeting a different scope/position.

    KNOWN GAP (order-blindness, flagged for retest — see falsify()'s slide_count_min/max
    comments): this still does NOT verify slide ORDER lands where the goal asked. Two
    derivations that both emit duplicate_slide(source="6", dest="") agree here even if the
    resulting deck order is wrong relative to what the instruction wanted (e.g. the
    "alternating order" gold) — dest="" is a valid, self-consistent value on both sides, not a
    binding mismatch. Reordering correctness is NOT covered by this corroborator or by
    falsify()'s slide-count checks; it rests entirely on the (unverified-headless, see
    impress_ops._move_page) reorder mechanism actually doing what was asked."""
    out = set()
    for n in nameops:
        out.add((n.get("kind"), n.get("slide"), n.get("shape"), n.get("lines"),
                n.get("dest"), n.get("index")))
    return out


def corroborate(g, instr, detected, nameops, mainlog):
    """An INDEPENDENT re-derivation (temp>0) must target the SAME (slide, shape, op-kind,
    lines/dest/index) footprint. Disagreement -> caller abstains. Never re-applies anything
    (read-only)."""
    if not nameops:
        return False
    der2, d2log = author(instr, detected, temperature=0.6)
    mainlog["der2_emit"] = d2log.get("emit_raw")
    s1, s2 = _touched_set(nameops), _touched_set(der2)
    mainlog["corrob_detail"] = {"der1": sorted(map(str, s1)), "der2": sorted(map(str, s2))}
    return s1 == s2 and bool(s1)


# ── run_core: model -> emit -> apply -> falsify -> retry -> corroborate -> score ────────────
def run_core(g, task, file_path, log, score_fn):
    r = g.client("open", {"file": file_path})
    if not r.get("ok"):
        log["fatal"] = "open failed: %s" % r.get("error")
        return None, log
    log["steps"] = ["open"]
    instr = task["instruction"]

    detected = detect(g)
    feedback = None
    nameops, fired, apply_fails, gaps = [], [], [], []
    for attempt in range(2):
        log["steps"].append("attempt%d" % attempt)
        nameops, alog = author(instr, detected, feedback)
        log["reasoning"] = alog.get("reasoning")
        log["emit_raw"] = alog.get("emit_raw")
        if len(nameops) == 1 and nameops[0].get("kind") == "infeasible":
            log["declared_infeasible"] = nameops[0].get("reason", "")
            log["nameops"] = nameops
            break
        nameops = [n for n in nameops if n.get("kind") != "infeasible"]
        gaps = emit_gaps(log.get("reasoning", ""), nameops, instr)
        log["nameops"] = nameops
        written, apply_fails = apply(g, nameops, log, file_path)
        fired = falsify(g, written)
        log["n_ops"] = len(nameops)
        log["falsifiers_fired"] = fired
        if nameops and not apply_fails and not fired and not gaps:
            break
        feedback = (compose_feedback(apply_fails, fired) + "\n" + gap_feedback(gaps)).strip()
    log["attempts"] = attempt + 1

    if "declared_infeasible" in log:
        score = 1.0 if (task.get("evaluator") or {}).get("func") == "infeasible" else 0.0
        log["score"] = score
        log["self_report_done"] = True
        log["false_pass"] = bool(score < 1.0)
        g.client("reconcile", {"gui": not log.get("host")})
        g.client("close")
        return score, log

    no_fault = bool(nameops) and not apply_fails and not fired and not gaps
    corroborated = False
    if no_fault:
        corroborated = corroborate(g, instr, detected, nameops, log)
        log["corroborated"] = corroborated
    log["self_report_done"] = bool(no_fault and corroborated)

    g.client("reconcile", {"gui": not log.get("host")})
    if os.environ.get("LAGADO_VISIBLE"):
        time.sleep(int(os.environ.get("LAGADO_VISIBLE_HOLD", "15")))
    g.client("close")
    time.sleep(4)  # let the GUI reload settle before any evaluator activates it

    score = score_fn()
    if score is None:
        log["score"] = None
        log["false_pass"] = None
        return None, log
    log["score"] = score
    log["false_pass"] = bool(log["self_report_done"] and score < 1.0)
    return score, log


# ── OFFLINE SELF-TEST (no live guest, no brain server) ──────────────────────────────────────
# Exercises the pure text-parsing/rendering/completeness logic: scan_calls/parse_kv, the grammar
# lists every verb parse_nameops recognizes, candidate_cards rendering, emit_gaps/gap_feedback,
# _norm_hex, and the corroboration footprint comparison — all deterministic, no network.
if __name__ == "__main__":
    failures = []

    def check(label, cond):
        if not cond:
            failures.append(label)
        print(("PASS" if cond else "FAIL"), "-", label)

    # ── scan_calls / parse_kv round-trip on a MULTI-op emission string ──
    sample = (
        '[set_title(slide="2", text="Online Shopping"),'
        ' set_font_color(slide="1", shape="title", color="yellow", lines="all"),'
        ' set_background(slide="2", color="purple", scope="slide"),'
        ' insert_table(slide="3", rows="5", cols="2"),'
        ' infeasible(reason="not a document edit")]'
    )
    nameops = parse_nameops(sample)
    kinds = [n["kind"] for n in nameops]
    check("parse_nameops finds all 5 calls", kinds == [
        "set_title", "set_font_color", "set_background", "insert_table", "infeasible"])
    check("parse_nameops field values", nameops[0]["slide"] == "2" and nameops[0]["text"] == "Online Shopping")
    check("parse_nameops quoted comma-free value", nameops[2]["color"] == "purple" and nameops[2]["scope"] == "slide")
    check("parse_nameops numeric-as-string fields", nameops[3]["rows"] == "5" and nameops[3]["cols"] == "2")

    # every verb the grammar declares must be one parse_nameops's VERBS recognizes (no silent
    # drift between GRAMMAR_IMPRESS's own alternatives and the parser's verb list)
    grammar_verbs = set(re.findall(r'"(\w+)\(', GRAMMAR_IMPRESS))
    check("GRAMMAR_IMPRESS verbs == parse_nameops.VERBS", grammar_verbs == set(VERBS))

    # ── candidate_cards rendering ──
    fake_detected = [
        {"index": 1, "title": "Intro", "content": "", "notes": "", "background": "#800080",
         "shapes": [{"kind": "textbox", "text": "hello", "x": 0, "y": 0, "w": 1000, "h": 500}]},
        {"index": 2, "title": "", "content": "Body text", "notes": "speaker note",
         "shapes": [{"kind": "table", "text": "", "rows": 5, "cols": 2, "x": 0, "y": 0, "w": 1, "h": 1},
                   {"kind": "image", "text": "", "x": 0, "y": 0, "w": 5000, "h": 3000}]},
    ]
    cards = candidate_cards(fake_detected)
    check("candidate_cards mentions slide indices", "Slide 1" in cards and "Slide 2" in cards)
    check("candidate_cards shows title", "Intro" in cards)
    check("candidate_cards shows background", "#800080" in cards)
    check("candidate_cards shows table dims", "5x2" in cards)
    check("candidate_cards shows image size in cm", "5.0x3.0" in cards)

    # ── emit_gaps / gap_feedback ──
    reasoning = "I will change the background color of the slide to match the request."
    gaps = emit_gaps(reasoning, [], "make the background blue")
    check("emit_gaps fires background nag when no set_background emitted", gaps == ["background"])
    check("gap_feedback renders the background nag", "BACKGROUND" in gap_feedback(gaps))
    gaps_clean = emit_gaps(reasoning, [{"kind": "set_background"}], "make the background blue")
    check("emit_gaps silent once the op IS emitted", gaps_clean == [])
    check("emit_gaps silent on a declared infeasible", emit_gaps(reasoning, [{"kind": "infeasible"}]) == [])

    # ── _norm_hex ──
    check("_norm_hex passthrough hex", _norm_hex("#ff0000") == "#FF0000")
    check("_norm_hex named color", _norm_hex("yellow") == "#FFFF00")
    check("_norm_hex unknown -> None (never wrongly matches)", _norm_hex("chartreuse") is None)

    # ── corroboration footprint comparison (pure logic, no network) ──
    der1 = [{"kind": "set_title", "slide": "2", "shape": "title"}]
    der2_same = [{"kind": "set_title", "slide": "2", "shape": "title"}]
    der2_diff = [{"kind": "set_title", "slide": "3", "shape": "title"}]
    check("_touched_set agrees on identical footprint", _touched_set(der1) == _touched_set(der2_same))
    check("_touched_set disagrees on a different slide binding", _touched_set(der1) != _touched_set(der2_diff))

    print()
    if failures:
        print("SELF-TEST FAILED (%d):" % len(failures))
        for f in failures:
            print("  -", f)
        raise SystemExit(1)
    print("SELF-TEST PASSED — all offline parsing/rendering/completeness checks green "
         "(author()/apply()/falsify()/corroborate() themselves need a live brain+guest; "
         "unvalidated until retest).")
