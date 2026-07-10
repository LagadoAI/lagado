"""calc_solve.py — task-blind spreadsheet solver: the proven battery-B pipeline (labeled
candidates → reason → emit-in-NAMES → fail-closed resolve → apply via the resident UNO daemon →
sound falsifiers → divergence resample → iterative escalation → read-only corroboration),
callable from the GENERAL agent's dispatch with exactly what that loop legitimately knows:

    calc_solve.py <base_url> <guest_file_path> <instruction>

NO task JSON, NO evaluator knowledge reaches this process — the entry point is structurally
incapable of leading (integrity contract 2026-07-10: benchmarks come FROM the harness).
Scoring stays with the caller (env.evaluate in the OSWorld runner; nothing here reads golds).

The final stdout line is one JSON verdict object:
    {"ok": bool, "self_report_done": bool, "declared_infeasible": str|null,
     "n_ops": int, "falsifiers": [...], "error": str|null}

Exit codes (the caller's routing contract):
    0 — ops applied, falsifiers clean, independently corroborated (honest done)
    3 — the model declared the task infeasible (caller translates to the FAIL answer)
    2 — operated but unverified (caller hands back honestly)
    1 — infrastructure failure (transport/daemon/open) → caller falls through to its floor
"""
import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import requests

import battery_calc as B
from run_session_task import Guest, deploy_daemon, pick_uno_python


class UrlGuest(Guest):
    """Guest transport over the OSWorld server's /execute endpoint from a bare base_url —
    the same channel Guest(env) wraps, without needing a DesktopEnv handle. sh()/client()
    are inherited unchanged (they only use self.py)."""

    def __init__(self, base_url):
        self.base = base_url.rstrip("/")

    def py(self, code):
        r = requests.post(self.base + "/execute",
                          json={"command": ["python3", "-c", code], "shell": False},
                          timeout=120)
        try:
            d = r.json()
        except Exception:
            return r.text
        return d.get("output", "") if isinstance(d, dict) else str(d)


def emit(verdict, code):
    print(json.dumps(verdict), flush=True)
    raise SystemExit(code)


def main():
    if len(sys.argv) != 4:
        raise SystemExit("usage: calc_solve.py <base_url> <guest_file_path> <instruction>")
    base_url, file_path, instruction = sys.argv[1], sys.argv[2], sys.argv[3]
    v = {"ok": False, "self_report_done": False, "declared_infeasible": None,
         "n_ops": 0, "falsifiers": [], "error": None}

    g = UrlGuest(base_url)
    unopy = pick_uno_python(g)
    if not unopy:
        v["error"] = "no guest python can import uno"
        emit(v, 1)
    # Clobber-avoidance (m1): the task-setup GUI soffice holds the file lock; the daemon must own
    # the file cleanly. Same pre-flight as every proven driver.
    g.sh("pkill -9 soffice; pkill -9 soffice.bin; true")
    g.sh("rm -f '%s/.~lock.%s#' 2>/dev/null; true"
         % (os.path.dirname(file_path), os.path.basename(file_path)))
    time.sleep(1)
    if not deploy_daemon(g, unopy):
        v["error"] = "uno daemon did not come up"
        emit(v, 1)

    # The instruction is the ONLY task knowledge run_core receives. The empty evaluator dict means
    # its infeasible branch scores locally as 0 — we ignore that score entirely and report the
    # declaration upward; the caller owns what FAIL means.
    task = {"instruction": instruction, "id": "solve000", "evaluator": {}}
    log = {"cond": "B", "run": 0, "id": "solve000", "steps": []}
    try:
        _score, log = B.run_core(g, task, "B", file_path, log, lambda: None)
    except Exception as e:
        v["error"] = "run_core: %r" % (e,)
        emit(v, 1)

    v["ok"] = True
    v["self_report_done"] = bool(log.get("self_report_done"))
    v["declared_infeasible"] = log.get("declared_infeasible")
    v["n_ops"] = int(log.get("n_ops") or 0)
    v["falsifiers"] = [f.get("falsifier") for f in log.get("falsifiers_fired", [])]
    if log.get("fatal"):
        v["ok"] = False
        v["error"] = log["fatal"]
        emit(v, 1)
    if v["declared_infeasible"] is not None:
        emit(v, 3)
    emit(v, 0 if v["self_report_done"] else 2)


if __name__ == "__main__":
    main()
