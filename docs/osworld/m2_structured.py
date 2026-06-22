"""
M2 (structured) — the spreadsheet's own tools served on a SILVER PLATTER, not me leading the model.

The model does NOT compute values and it is NOT steered by a prose prompt. A GBNF grammar enumerates the
app's ops as a DECODING CONSTRAINT (no KV bloat, no steering): the model SELECTS an op and fills typed slots
with just-enough discretion; UNO applies it through the real app (the formula engine computes). This is the
back-door exposing the function surface in a structured way. Shape-tested in Python on the ACTUAL OSWorld
calc tasks (host-only proxy = faithful for value tasks, since UNO actually computes); destination = Rust.

Run: .venv/bin/python m2_structured.py [N]
"""
import sys, re, json, requests
import pandas as pd
from m2_uno import task_io, structure, apply_ops, predicted_score, EXDIR
import glob

BRAIN_COMPLETION = "http://localhost:8080/completion"   # native llama.cpp endpoint (accepts GBNF `grammar`)

# The SILVER PLATTER: a GBNF that enumerates the app's ops. Top alternation is TERMINAL-LEADING (the
# llama.cpp gotcha — bare rule-ref alternation is silently dropped). The model picks ops + fills slots; it
# cannot emit a malformed op or an unknown verb. Formula CONTENT is the model's (the legitimate comprehension
# part — a human writing the formula also chooses the columns); the grammar guarantees well-formedness.
# GBNF GOTCHA (bisected): a rule whose alternatives continue on NEW lines starting with `|` is silently
# DROPPED (the whole grammar reverts to unconstrained — symptom: model emits prose). Keep each rule on ONE
# line. Also: no `ws` rule (a `[ \t\n]*` lets the model fill unbounded whitespace and never reach the ops).
GRAMMAR = (
    'root ::= "[" op ("," op)* "]"\n'
    'op ::= "set_cell(sheet=" str ", cell=" str ", formula=" str ")"'
    ' | "set_cell(sheet=" str ", cell=" str ", value=" str ")"'
    ' | "add_sheet(name=" str ")"'
    ' | "add_sheet(name=" str ", index=" int ")"'
    ' | "rename_sheet(old=" str ", new=" str ")"\n'
    'str ::= "\\"" [^"\\\\]* "\\""\n'
    'int ::= [0-9]+\n'
)

# The MENU is the silver platter: it PRESENTS the available tools (what a UI would show), it does NOT steer
# to an answer (no "use =B-C-D-SUM(...)"). The model selects ops + authors the formula itself (its job).
PROMPT = (
    "You operate a spreadsheet by issuing operations. Available operations:\n"
    "  set_cell(sheet=\"S\", cell=\"A1\", formula=\"=...\")   set a cell to a formula (the app computes it)\n"
    "  set_cell(sheet=\"S\", cell=\"A1\", value=\"...\")       set a cell to a literal value\n"
    "  add_sheet(name=\"S\", index=0)                        add a sheet (index 0 = first)\n"
    "  rename_sheet(old=\"S\", new=\"S2\")                     rename a sheet\n"
    "Use formulas for any computation (the spreadsheet evaluates them).\n\n"
    "Goal: {instr}\n"
    "Workbook:\n{struct}\n\n"
    "Emit the operations that accomplish the goal, as a list of calls:"
)

KV = re.compile(r'(\w+)="((?:[^"\\]|\\.)*)"|(\w+)=(\d+)')


def _scan_calls(text):
    """Yield (verb, body) for each `verb(...)`, respecting quotes so a `)` inside a formula
    (e.g. SUM(F2:H2)) doesn't terminate the op early."""
    for m in re.finditer(r'(set_cell|add_sheet|rename_sheet)\(', text):
        verb = m.group(1)
        i = m.end()
        depth, in_q, esc, start = 1, False, False, i
        while i < len(text) and depth > 0:
            c = text[i]
            if esc: esc = False
            elif c == '\\': esc = True
            elif c == '"': in_q = not in_q
            elif not in_q and c == '(': depth += 1
            elif not in_q and c == ')': depth -= 1
            i += 1
        yield verb, text[start:i - 1]


def parse_ops(text):
    ops = []
    for verb, body in _scan_calls(text):
        kw = {}
        for m in KV.finditer(body):
            if m.group(1) is not None:
                kw[m.group(1)] = m.group(2).replace('\\"', '"')
            else:
                kw[m.group(3)] = m.group(4)
        if verb == "set_cell":
            o = {"op": "set", "sheet": kw.get("sheet"), "cell": kw.get("cell")}
            if "formula" in kw: o["formula"] = kw["formula"]
            elif "value" in kw:
                v = kw["value"]
                try: o["value"] = int(v)
                except ValueError:
                    try: o["value"] = float(v)
                    except ValueError: o["value"] = v
            else: continue
            ops.append(o)
        elif verb == "add_sheet":
            o = {"op": "add_sheet", "name": kw.get("name")}
            if "index" in kw: o["index"] = int(kw["index"])
            ops.append(o)
        elif verb == "rename_sheet":
            ops.append({"op": "rename_sheet", "old": kw.get("old"), "new": kw.get("new")})
    return ops


def author_structured(instr, struct):
    """Grammar-constrained selection — NO leading prompt, NO steering. Returns ops JSON text for apply_ops."""
    r = requests.post(BRAIN_COMPLETION, json={
        "prompt": PROMPT.format(instr=instr, struct=struct),
        "grammar": GRAMMAR, "temperature": 0, "n_predict": 1400,
    }, timeout=200)
    raw = r.json().get("content", "")
    ops = parse_ops(raw)
    return json.dumps(ops), raw


if __name__ == "__main__":
    N = next((int(a) for a in sys.argv[1:] if a.isdigit()), 12)
    files = sorted(glob.glob(EXDIR + "/*.json"))
    results = []
    print("=== M2 STRUCTURED (silver-platter GBNF, no leading prompt) | host-only over up to %d calc tasks ===" % N, flush=True)
    for tf in files:
        if len(results) >= N: break
        t = task_io(tf)
        if not t: continue
        try:
            ops_text, raw = author_structured(t["instr"], structure(t["inl"]))
            ok, why = apply_ops(ops_text, t["inl"])
            score, det = predicted_score(t["goldl"]) if ok else (0.0, why)
        except Exception as e:
            score, det = 0.0, "EXC %s" % e
        results.append(dict(tid=t["tid"], instr=t["instr"][:58], score=score, detail=det))
        print("  [%s] score=%.0f  %s" % (t["tid"], score, t["instr"][:58]), flush=True)

    passed = sum(1 for r in results if r["score"] >= 1.0)
    print("\n=== STRUCTURED (silver-platter): %d/%d ===" % (passed, len(results)), flush=True)
    for r in results:
        print("   %s  %.0f  %s" % (r["tid"], r["score"], "" if r["score"] >= 1 else r["detail"]), flush=True)
