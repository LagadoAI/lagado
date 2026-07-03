"""TURN-3 PROBE: confirm braces are the SOLE gap before building a grammar fix (advisor's blocking check).

Hand-inject BRACED nameops (no model, no author_B) for the two brace-friction tasks and score against the
REAL env.evaluate(). This isolates emission-friction (bare-name → {braced}) from any second gap (date
formatting on 4172ea6e, gold's notion of "clean" on a9f325aa). Each variant: reset → open via daemon →
apply_B(injected) → reconcile → evaluate. Reuses the exact apply/score path from battery_calc.run_condition.

Usage (OSWorld dir, its venv, podman sock):
  DOCKER_HOST=unix:///run/podman/podman.sock PYTHONPATH=/home/alucard/projects/OSWorld \
  .venv/bin/python /home/alucard/projects/lagado/lagado-agent/python/osworld/brace_inject_probe.py
"""
import json, os, sys, glob, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from battery_calc import (Guest, deploy_daemon, pick_uno_python, apply_B, detect, live_detect, falsify)
from run_session_task import task_input_path, memory_ok
from desktop_env.desktop_env import DesktopEnv

EX = "evaluation_examples/examples/libreoffice_calc"

# Each probe: (task-id-prefix, label, [nameops]). Sheet name is filled from live detection at run time
# (the model emitted "Sheet1" / the CJK default — we resolve the real single sheet from `detected`).
PROBES = [
    ("a9f325aa", "braced", [
        {"kind": "compute_column", "target": "Clean Movie Titles",
         "formula": "=PROPER(TRIM(SUBSTITUTE({Garbage Movie titles}, '  ', ' ')))"}]),
    ("4172ea6e", "braced-general", [
        {"kind": "compute_column", "target": "Maturity Date",
         "formula": "={Loan Issue Date} + {Length of Loan in Days}"}]),
    ("4172ea6e", "braced+dateformat", [
        {"kind": "compute_column", "target": "Maturity Date",
         "formula": "={Loan Issue Date} + {Length of Loan in Days}"},
        {"kind": "set_number_format", "range": "C2:C10", "format": "MM/DD/YYYY"}]),
]

def run_inject(env, task, file_path, nameops_tmpl):
    g = Guest(env)
    log = {"steps": []}
    unopy = pick_uno_python(g)
    if not unopy:
        return 0.0, {"fatal": "no uno python"}
    g.sh("pkill -9 soffice; pkill -9 soffice.bin; true")
    g.sh("rm -f '%s/.~lock.%s#' 2>/dev/null; true" % (os.path.dirname(file_path), os.path.basename(file_path)))
    time.sleep(1)
    if not deploy_daemon(g, unopy):
        return 0.0, {"fatal": "daemon not ready"}
    r = g.client("open", {"file": file_path})
    if not r.get("ok"):
        return 0.0, {"fatal": "open failed: %s" % r.get("error")}
    detail = r.get("structure", {}).get("detail", [])
    detected = detect(g, detail)
    sheet = next(iter(detected))                       # single-sheet tasks → the live sheet name
    log["detected"] = {s: [(c["letter"], c["header"]) for c in i["cols"]] for s, i in detected.items()}
    nameops = [dict(n, sheet=sheet) for n in nameops_tmpl]
    written, resolve_fails = apply_B(g, nameops, log)
    log["resolve_fails"] = resolve_fails
    log["written"] = written
    fired = falsify(g, written)
    log["falsifiers"] = fired
    log["readback"] = {}
    for s, rng, _f in written:
        rb = g.client("read", {"sheet": s, "range": rng})
        if rb.get("ok"):
            log["readback"]["%s!%s" % (s, rng)] = [row[0] if row else None for row in rb.get("cells", [])]
    g.client("reconcile", {"gui": True})
    g.client("close")
    time.sleep(4)
    score = env.evaluate() or 0.0
    return score, log

def main():
    if not memory_ok():
        raise SystemExit(1)
    env = DesktopEnv(provider_name="docker", action_space="pyautogui", screen_size=(1920, 1080),
                     headless=True, os_type="Ubuntu", require_a11y_tree=False)
    results = []
    try:
        for tid, label, nameops in PROBES:
            tf = sorted(glob.glob("%s/%s*.json" % (EX, tid)))
            if not tf:
                print("!! no task json for", tid); continue
            task = json.load(open(tf[0]))
            file_path = task_input_path(task)
            print("\n=== %s [%s] ===" % (tid, label), flush=True)
            print("    %s" % task["instruction"][:90], flush=True)
            env.reset(task_config=task)
            time.sleep(2)
            score, log = run_inject(env, task, file_path, nameops)
            results.append((tid, label, score))
            print("    SCORE=%s  resolve_fails=%s  falsifiers=%s" % (
                score, log.get("resolve_fails"), [f.get("falsifier") for f in log.get("falsifiers", [])]), flush=True)
            for k, v in (log.get("readback") or {}).items():
                print("    readback %s = %s" % (k, v[:6]), flush=True)
            if log.get("fatal"):
                print("    FATAL:", log["fatal"], flush=True)
    finally:
        env.close()
    print("\n" + "=" * 56, flush=True)
    for tid, label, score in results:
        print("  %-9s %-18s  %s" % (tid, label, "GOLD" if score >= 1.0 else "score=%s" % score), flush=True)

if __name__ == "__main__":
    main()
