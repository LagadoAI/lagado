#!/usr/bin/env python3
"""WATCH SESSION — ONE visible LibreOffice window, kept open across SEVERAL tasks, each one provably Qwen's.

Same anti-doubt design as watch_qwen.py, extended to a resident session:
  - Builds a brand-new file from YOUR data (spec.json). No gold/answer file exists anywhere.
  - Opens ONE visible window and keeps it open.
  - Reads instructions, one at a time, from a control file: /tmp/lagado_watch/cmds.jsonl
    Each line is {"instruction": "..."} (run it on the LIVE doc) or {"quit": true} (close).
  - For EACH instruction it RE-PERCEIVES the live doc (so task 2 sees task 1's new columns), calls Qwen on
    :8080, prints Qwen's literal reasoning + emitted verbs, then applies them one at a time (paced) in the
    SAME window while you watch.
  - No scoring. You judge with your eyes. And because every instruction triggers a fresh call to the local
    model on :8080, killing that server mid-session makes the next instruction produce NOTHING — the proof
    that the work comes from Qwen, not from whoever typed the command.

Run (in YOUR terminal if you want zero doubt I'm in the loop):
  cd /home/alucard/projects/OSWorld && PYTHONPATH=/home/alucard/projects/OSWorld:/home/alucard/projects/lagado/docs/osworld \
    .venv/bin/python /home/alucard/projects/lagado/docs/osworld/watch_session.py /tmp/lagado_watch/spec_demo.json

Feed it tasks (from any terminal):
  echo '{"instruction":"Sort the rows by Profit descending"}' >> /tmp/lagado_watch/cmds.jsonl
  echo '{"quit":true}' >> /tmp/lagado_watch/cmds.jsonl
"""
import os, sys, json, time, subprocess, traceback

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
WORK = "/tmp/lagado_watch"
CMDS = os.path.join(WORK, "cmds.jsonl")


def build_xlsx(path, sheets):
    import openpyxl
    wb = openpyxl.Workbook()
    first = True
    for sd in sheets:
        ws = wb.active if first else wb.create_sheet()
        ws.title = sd.get("name", ws.title)
        first = False
        if sd.get("headers"):
            ws.append(list(sd["headers"]))
        for row in sd.get("rows", []):
            ws.append(list(row))
    wb.save(path)


def show_structure(g):
    import battery_calc
    live = battery_calc.live_detect(g)
    print("  Qwen now PERCEIVES:", flush=True)
    for s, info in live.items():
        print("    %s: %s" % (s, [(c["letter"], c["header"], c.get("ntype")) for c in info["cols"]]), flush=True)
    return live


def run_instruction(g, instr, idx):
    import battery_calc
    print("\n" + "#" * 72, flush=True)
    print("# TASK %d — INSTRUCTION (from the control file, sent verbatim to Qwen):" % idx, flush=True)
    print("#   %r" % instr, flush=True)
    print("#" * 72, flush=True)
    detected = show_structure(g)
    log = {"steps": []}
    print("\n  --- calling Qwen on :8080 (reason, then emit) ---", flush=True)
    t0 = time.time()
    new_ops = battery_calc.author_B(instr, detected, log)
    dt = time.time() - t0
    print("\n  >>> QWEN'S REASONING (verbatim, took %.1fs of local GPU inference):\n%s" %
          (dt, log.get("reasoning", "(none)")), flush=True)
    print("\n  >>> QWEN'S EMITTED VERB-CALLS (verbatim, grammar-constrained):", flush=True)
    for raw in log.get("emit_raw", []):
        print(raw, flush=True)
    print("\n  >>> PARSED into %d op(s) — these fill the cells, one at a time:" % len(new_ops), flush=True)
    for o in new_ops:
        print("      %s" % (o,), flush=True)
    print("\n  --- applying in the OPEN window (%ss between ops) ---" %
          os.environ.get("LAGADO_WATCH_PAUSE", "?"), flush=True)
    written, fails = battery_calc.apply_B(g, new_ops, log)
    print("\n  --- TASK %d DONE. Read-back of what landed: ---" % idx, flush=True)
    for sheet, rng, f in written:
        rb = g.client("read", {"sheet": sheet, "range": rng})
        vals = [row[0] if row else None for row in rb.get("cells", [])] if rb.get("ok") else []
        print("    %s!%s  <= %s   ->  %s" % (sheet, rng, f, vals[:14]), flush=True)
    if fails:
        print("    fail-closed (a name didn't resolve — nothing guessed): %s" % fails, flush=True)
    print("\n  (window still open — give me the next task, or 'quit')", flush=True)


def main():
    if len(sys.argv) < 2:
        print("usage: watch_session.py <spec.json>", file=sys.stderr); sys.exit(2)
    spec = json.load(open(sys.argv[1]))
    sheets = spec.get("sheets") or [{"name": "Sheet1", "headers": spec.get("headers"),
                                     "rows": spec.get("rows", [])}]
    os.makedirs(WORK, exist_ok=True)
    path = os.path.join(WORK, "watch_input.xlsx")
    build_xlsx(path, sheets)

    # Fresh control file each session. Seed the spec's instruction as task 1 if it has one.
    open(CMDS, "w").close()
    if spec.get("instruction"):
        open(CMDS, "a").write(json.dumps({"instruction": spec["instruction"]}) + "\n")

    os.environ["LAGADO_VISIBLE"] = "1"
    os.environ.setdefault("DISPLAY", ":0")
    os.environ.setdefault("LAGADO_WATCH_PAUSE", "2.5")
    idle_cap = int(os.environ.get("LAGADO_SESSION_IDLE", "900"))   # auto-close after this many idle seconds

    # Scoped pre-clean of OUR OWN strays only — never a global `pkill soffice` (would kill the user's office).
    subprocess.run("pkill -9 -f lagado_uno_daemon_profile; pkill -9 -f lagado_watch_sock; true",
                   shell=True, stderr=subprocess.DEVNULL)
    time.sleep(0.5)

    import battery_calc  # noqa: F401  (validates the module imports before we open a window)
    from battery_host import HostGuest

    print("Building file from YOUR data and opening ONE visible window...", flush=True)
    print("  control file: %s   (append {\"instruction\":\"...\"} or {\"quit\":true})" % CMDS, flush=True)
    sock = "/tmp/lagado_watch_sock.sock"
    g = None
    try:
        g = HostGuest(sock, 2300)
        r = g.client("open", {"file": path})
        if not r.get("ok"):
            print("open failed: %s" % r.get("error"), flush=True); return
        print("Window open. Sales data loaded. Watching control file for tasks...\n", flush=True)

        processed = 0
        idx = 0
        last_activity = time.time()
        while True:
            lines = []
            if os.path.exists(CMDS):
                with open(CMDS) as fh:
                    lines = [ln for ln in fh.read().splitlines() if ln.strip()]
            if processed < len(lines):
                ln = lines[processed]; processed += 1
                try:
                    cmd = json.loads(ln)
                except Exception:
                    print("  (skipping unparseable control line: %r)" % ln, flush=True); continue
                if cmd.get("quit"):
                    print("\nQUIT received — closing the window.", flush=True); break
                if cmd.get("instruction"):
                    idx += 1
                    run_instruction(g, cmd["instruction"], idx)
                    last_activity = time.time()
            else:
                if time.time() - last_activity > idle_cap:
                    print("\nIdle %ds with no new task — auto-closing." % idle_cap, flush=True); break
                time.sleep(1)
    except Exception as e:
        print("session error: %s\n%s" % (e, traceback.format_exc()[-400:]), flush=True)
    finally:
        if g:
            g.kill()


if __name__ == "__main__":
    main()
