"""Membrane-fluency panel — the standing instrument for every model/quant decision.

Measures what leaderboards can't: fluency in THIS harness's medium.
  A. BINDING: fuzzy natural reference -> live column header, by cosine in the
     candidate's OWN embedding space (last-token pooling, R1b lever). Scored on a
     FROZEN reference set with fail-closed semantics: bind only if margin
     (top1 - top2) >= THETA; abstaining on the ambiguous cases is CORRECT.
  B. EMISSION SMOKE: one grammar-shaped op emission, temp 0.
(C. battery golds run separately via battery_host.py — this script covers A+B.)

The reference set is FROZEN (v1.1, 2026-07-06 — two expectations aligned to
R1b's documented correct-abstain findings before any candidate comparison). Never tune it toward a candidate.
Baseline: Qwen2.5-Coder-7B Q4_K_M, measured at creation, stored alongside.

Run:  python3 fluency_panel.py [--port 8080] [--out baseline.json]
"""
import argparse
import json
import urllib.request

THETA = 0.08   # SEM_THETA — the battery's fail-closed margin

# FROZEN v1. expect=None means genuinely ambiguous/unbindable: abstain is correct.
CASES = [
    ("the movie titles", ["Garbage Movie titles", "Release Year", "Budget", "Box Office"],
     "Garbage Movie titles"),
    ("amount spent", ["Purpose", "Date", "Spent ($)", "Person"], "Spent ($)"),
    ("how many items were sold", ["Product", "Units Sold", "Unit Price", "Total"],
     "Units Sold"),
    ("employee salaries", ["Name", "Department", "Annual Salary", "Start Date"],
     "Annual Salary"),
    ("the profit column", ["Revenue", "Costs", "Net Profit", "Quarter"], "Net Profit"),
    ("customer emails", ["Customer Name", "E-mail Address", "Phone", "City"],
     "E-mail Address"),
    # ambiguity / distractor traps — abstain (margin < THETA or negative top) is correct.
    # loan-dates + terse "Rank" are DOCUMENTED correct-abstain cases (R1b, 2026-06-23:
    # genuine semantic overlap / anti-correlated terse token — fail-closed is the win).
    ("when the loan was issued",
     ["Loan Issue Date", "Loan Due Date", "Payment Date", "Amount"], None),
    ("Rank", ["Rank", "Score", "Name"], None),
    ("the date", ["Loan Issue Date", "Loan Due Date", "Payment Date"], None),
    ("the weather forecast", ["Product", "Units Sold", "Unit Price", "Total"], None),
]

EMISSION_PROMPT = ('Emit exactly one op: set cell B2 on sheet Sheet1 to 42. '
                   'Format: [set_cell(sheet="Sheet1", cell="B2", value="42")]')


def post(url, payload):
    req = urllib.request.Request(url, json.dumps(payload).encode(),
                                 {"Content-Type": "application/json"})
    return json.load(urllib.request.urlopen(req, timeout=120))


def embed(port, texts):
    d = post("http://localhost:%d/v1/embeddings" % port, {"input": texts})
    return [e["embedding"] for e in d["data"]]


def cos(a, b):
    num = sum(x * y for x, y in zip(a, b))
    na = sum(x * x for x in a) ** 0.5
    nb = sum(x * x for x in b) ** 0.5
    return num / (na * nb + 1e-12)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=8080)
    ap.add_argument("--out", default="")
    args = ap.parse_args()

    model = post("http://localhost:%d/v1/embeddings" % args.port,
                 {"input": ["x"]}).get("model", "?")
    print("=== MEMBRANE-FLUENCY PANEL v1 — model: %s ===" % model)

    results, ok = [], 0
    for ref, headers, expect in CASES:
        vecs = embed(args.port, [ref] + headers)
        sims = sorted(((cos(vecs[0], v), h) for v, h in zip(vecs[1:], headers)),
                      reverse=True)
        margin = sims[0][0] - sims[1][0]
        bound = sims[0][1] if margin >= THETA else None
        correct = (bound == expect)
        ok += correct
        results.append({"ref": ref, "bound": bound, "expect": expect,
                        "margin": round(margin, 4), "top": round(sims[0][0], 4),
                        "correct": correct})
        print("  %-28s -> %-22s margin=%+.3f  %s" %
              (ref[:28], str(bound)[:22], margin, "OK" if correct else "WRONG"))
    print("BINDING: %d/%d correct (incl. correct abstentions)" % (ok, len(CASES)))

    d = post("http://localhost:%d/v1/chat/completions" % args.port,
             {"messages": [{"role": "user", "content": EMISSION_PROMPT}],
              "temperature": 0, "max_tokens": 48})
    emitted = d["choices"][0]["message"]["content"].strip()
    em_ok = emitted.startswith("[set_cell(") and 'value="42"' in emitted
    print("EMISSION: %s  %r" % ("OK" if em_ok else "WRONG", emitted[:80]))

    report = {"model": model, "theta": THETA, "binding_correct": ok,
              "binding_total": len(CASES), "emission_ok": em_ok, "cases": results}
    if args.out:
        json.dump(report, open(args.out, "w"), indent=1)
        print("saved -> %s" % args.out)


if __name__ == "__main__":
    main()
