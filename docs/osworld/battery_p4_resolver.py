"""P4 — sloppy-sheet robustness: does name resolution FAIL-CLOSED or SILENTLY MIS-BIND?

The user's narrowness fear: a tight header-structure approach crumbles on sloppy human spreadsheets.
The HARD INVARIANT is: on any ambiguous/missing reference, fail-closed — NEVER silently bind to the
wrong column (a silent mis-bind turns a visible 0 into an invisible wrong answer the user trusts).

Mis-binding happens (if anywhere) in the RESOLVER, so we adjudicate it there directly and exhaustively
across degradation classes — no guest boot needed for the invariant, which is the rigorous place to test
it. (A live end-to-end fail-closed confirmation on a physically-degraded sheet runs separately.)

For each degraded view of the real 035f41ba sheet, the model asks for the SAME canonical names the task
needs; we classify each outcome as RESOLVES-RIGHT / FAIL-CLOSED / **MIS-BIND** (resolves to a wrong column).
The claim is: MIS-BIND count = 0 across every class.

Run: .venv/bin/python battery_p4_resolver.py
"""
import os, sys, copy
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from battery_calc import resolve_ref, col_letter

# Real 035f41ba Sheet1 headers (row 1), col index 0-based → header.
CLEAN = ["Year", "Sales", "Sales Return", "Discounts and Allowances", "Net Sales",
         "Materials Charges", "Labor Charges", "Overhead", "Total Cost of Goods Sold", "Gross Profit"]
# The names the task's gross-profit formula references (what the model would emit), → the RIGHT column.
NEEDED = {"Sales": "B", "Sales Return": "C", "Discounts and Allowances": "D",
          "Materials Charges": "F", "Labor Charges": "G", "Overhead": "H"}

def detected_from_headers(headers, sheet="Sheet1", rows=10):
    """Build the {sheet:{cols,rows}} view the live detector (row-1 read) would produce."""
    cols = [{"letter": col_letter(i), "header": (h if h is not None else ""), "samples": [], "idx0": i}
            for i, h in enumerate(headers)]
    return {sheet: {"cols": cols, "rows": rows}}

def classify(detected, name, right_letter):
    fails = []
    res = resolve_ref(name, "Sheet1", detected, fails)
    if res is None:
        return "FAIL-CLOSED"
    _sheet, letter = res
    return "RESOLVES-RIGHT" if letter == right_letter else "MIS-BIND(%s≠%s)" % (letter, right_letter)

# ── degradation classes (each a view the detector would see) ──────────────────────
def variants():
    out = {}
    out["clean"] = (detected_from_headers(CLEAN), NEEDED)
    # 1) TITLE ROW above headers → detector reads row 1 = a title, real headers hidden in row 2.
    title = [""] * len(CLEAN); title[0] = "Income Statement 2015-2023"
    out["title_row"] = (detected_from_headers(title), NEEDED)
    # 2) DUPLICATE header: a second column also named "Sales".
    dup = list(CLEAN); dup[4] = "Sales"     # Net Sales → Sales (now two "Sales")
    out["dup_header"] = (detected_from_headers(dup), NEEDED)
    # 3) UNITS in label: "Sales" → "Sales ($)" (model still emits bare {Sales}).
    units = list(CLEAN); units[1] = "Sales ($)"
    out["units_label_bareref"] = (detected_from_headers(units), NEEDED)
    # 3b) UNITS in label, but the model copies the EXACT header off the candidate card.
    out["units_label_exactref"] = (detected_from_headers(units), {**NEEDED, "Sales ($)": "B", **{k: v for k, v in NEEDED.items() if k != "Sales"}})
    # 4) BLANK SPACER column inserted between Year and Sales (shifts everything right by 1).
    spacer = [CLEAN[0], ""] + CLEAN[1:]
    shifted = {n: col_letter("ABCDEFGHIJ".index(L) + 1) for n, L in NEEDED.items()}
    out["blank_spacer"] = (detected_from_headers(spacer), shifted)
    # 5) SYNONYM: "Overhead" → "OH" (model emits {Overhead}).
    syn = list(CLEAN); syn[7] = "OH"
    out["synonym_overhead"] = (detected_from_headers(syn), NEEDED)
    # 6) CASE/whitespace noise: "  sales  " (resolver is case-insensitive + trims → should still resolve).
    noisy = list(CLEAN); noisy[1] = "  SALES  "
    out["case_whitespace"] = (detected_from_headers(noisy), NEEDED)
    return out

def main():
    print("=== P4 resolver adjudication — fail-closed vs MIS-BIND across sloppiness classes ===\n")
    total_misbind = 0
    for vname, (detected, needed) in variants().items():
        outcomes = {}
        for name, right in needed.items():
            outcomes[name] = classify(detected, name, right)
        misbind = [n for n, o in outcomes.items() if o.startswith("MIS-BIND")]
        total_misbind += len(misbind)
        resolved = sum(1 for o in outcomes.values() if o == "RESOLVES-RIGHT")
        failclosed = sum(1 for o in outcomes.values() if o == "FAIL-CLOSED")
        verdict = "★ MIS-BIND!" if misbind else ("all resolve" if failclosed == 0 else "fail-closed (safe)")
        print("  %-22s resolve=%d fail-closed=%d mis-bind=%d   %s" % (
            vname, resolved, failclosed, len(misbind), verdict))
        for n, o in outcomes.items():
            if o != "RESOLVES-RIGHT":
                print("        %-26s -> %s" % (n, o))
    print("\n  TOTAL MIS-BINDS across all classes: %d  (the invariant: must be 0)" % total_misbind)
    return 0 if total_misbind == 0 else 1

if __name__ == "__main__":
    sys.exit(main())
