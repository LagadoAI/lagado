#!/usr/bin/env python3
"""WATCH MODE — a single task, in ONE visible LibreOffice window, that YOU can verify is Qwen's work.

Why this exists: a headless batch proves the scorer passes, but it can't prove to a watching human that the
MODEL did the work (vs. the harness pulling up a pre-made answer). This runner removes every place that doubt
could hide:
  1. The task comes from YOU (a spec JSON: instruction + raw data). A brand-new file is built from your numbers,
     so there is NO cached gold anywhere to swap in. The answer must be computed live or not at all.
  2. It uses the SAME functions the real eval uses (battery_calc.detect / author_B / apply_B) — not a demo path.
  3. It prints Qwen's LITERAL output: the reasoning text and the exact verb-calls it emitted. Those same verbs
     are what fill the cells, one at a time (paced by LAGADO_WATCH_PAUSE), in front of you.
  4. There is NO scoring against a gold (there is none). Correctness is judged by your eyes on the open window.

Run (OSWorld .venv has openpyxl/requests; daemon is spawned under system python for uno):
  cd /home/alucard/projects/OSWorld && PYTHONPATH=/home/alucard/projects/OSWorld:/home/alucard/projects/lagado/docs/osworld \
    .venv/bin/python /home/alucard/projects/lagado/docs/osworld/watch_qwen.py <spec.json>

spec.json:
  {"instruction": "Add a Total column that sums Q1..Q4 for each region",
   "sheets": [{"name": "Sheet1",
               "headers": ["Region", "Q1", "Q2", "Q3", "Q4"],
               "rows": [["North", 10, 20, 30, 40], ["South", 5, 15, 25, 35]]}]}
"""
import os, sys, json, time, signal, traceback

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
WORK = "/tmp/lagado_watch"


def build_xlsx(path, sheets):
    """Build a fresh workbook from the user's dictated data. Nothing here is an 'answer' — only inputs."""
    import openpyxl
    wb = openpyxl.Workbook()
    first = True
    for sd in sheets:
        ws = wb.active if first else wb.create_sheet()
        ws.title = sd.get("name", ws.title)
        first = False
        headers = sd.get("headers")
        if headers:
            ws.append(list(headers))
        for row in sd.get("rows", []):
            ws.append(list(row))
    wb.save(path)


def show_input(sheets):
    print("=" * 72, flush=True)
    print("INPUT (what YOU gave me — built into a brand-new file, no answer in it):", flush=True)
    for sd in sheets:
        print("  sheet %r:" % sd.get("name", "Sheet1"), flush=True)
        if sd.get("headers"):
            print("    headers: %s" % (sd["headers"],), flush=True)
        for r in sd.get("rows", [])[:12]:
            print("    row: %s" % (r,), flush=True)
    print("=" * 72, flush=True)


def main():
    if len(sys.argv) < 2:
        print("usage: watch_qwen.py <spec.json>", file=sys.stderr); sys.exit(2)
    spec = json.load(open(sys.argv[1]))
    instr = spec["instruction"]
    sheets = spec.get("sheets") or [{"name": "Sheet1", "headers": spec.get("headers"),
                                     "rows": spec.get("rows", [])}]

    os.makedirs(WORK, exist_ok=True)
    path = os.path.join(WORK, "watch_input.xlsx")
    build_xlsx(path, sheets)

    # Visible, paced, single window. Set BEFORE importing/spawning so the daemon launches visible.
    os.environ["LAGADO_VISIBLE"] = "1"
    os.environ.setdefault("DISPLAY", ":0")
    os.environ.setdefault("LAGADO_WATCH_PAUSE", "2.5")
    hold = int(os.environ.get("LAGADO_WATCH_HOLD", "600"))

    # Scoped pre-clean of OUR OWN stray daemons only — NEVER a global `pkill soffice` (would kill the user's
    # own LibreOffice). Matches the daemon's safety doctrine.
    import subprocess
    subprocess.run("pkill -9 -f lagado_uno_daemon_profile; pkill -9 -f lagado_watch_sock; true",
                   shell=True, stderr=subprocess.DEVNULL)
    time.sleep(0.5)

    import battery_calc
    from battery_host import HostGuest

    show_input(sheets)
    print("\nINSTRUCTION sent to Qwen:\n  %r\n" % instr, flush=True)
    print("Opening ONE visible LibreOffice window... (watch the screen)\n", flush=True)

    sock = "/tmp/lagado_watch_sock.sock"
    g = None
    try:
        g = HostGuest(sock, 2300)
        r = g.client("open", {"file": path})
        if not r.get("ok"):
            print("open failed: %s" % r.get("error"), flush=True); return
        detail = r.get("structure", {}).get("detail", [])
        detected = battery_calc.detect(g, detail)
        print("Qwen PERCEIVES this structure:", flush=True)
        for s, info in detected.items():
            print("  %s: %s" % (s, [(c["letter"], c["header"], c.get("ntype")) for c in info["cols"]]), flush=True)

        log = {"steps": []}
        print("\n--- calling Qwen on :8080 (reason, then emit) ---", flush=True)
        new_ops = battery_calc.author_B(instr, detected, log)

        print("\n>>> QWEN'S REASONING (verbatim):\n%s\n" % log.get("reasoning", "(none)"), flush=True)
        print(">>> QWEN'S EMITTED VERB-CALLS (verbatim, grammar-constrained):", flush=True)
        for raw in log.get("emit_raw", []):
            print(raw, flush=True)
        print("\n>>> PARSED into %d op(s) — these are what fill the cells, one at a time:" % len(new_ops), flush=True)
        for o in new_ops:
            print("    %s" % (o,), flush=True)
        print("\n--- applying (watch the window; %ss between ops) ---" % os.environ["LAGADO_WATCH_PAUSE"], flush=True)

        written, fails = battery_calc.apply_B(g, new_ops, log)

        print("\n--- DONE. Read-back of what landed: ---", flush=True)
        for sheet, rng, f in written:
            rb = g.client("read", {"sheet": sheet, "range": rng})
            vals = [row[0] if row else None for row in rb.get("cells", [])] if rb.get("ok") else []
            print("  %s!%s  <= %s   ->  %s" % (sheet, rng, f, vals[:12]), flush=True)
        if fails:
            print("  fail-closed (a name didn't resolve, nothing guessed): %s" % fails, flush=True)

        print("\n" + "=" * 72, flush=True)
        print("WINDOW IS OPEN — inspect it yourself. Holding for up to %ds (or until you tell me to close)." % hold,
              flush=True)
        print("=" * 72, flush=True)
        t0 = time.time()
        while time.time() - t0 < hold:
            time.sleep(2)
    except Exception as e:
        print("watch run error: %s\n%s" % (e, traceback.format_exc()[-400:]), flush=True)
    finally:
        if g:
            g.kill()


if __name__ == "__main__":
    main()
