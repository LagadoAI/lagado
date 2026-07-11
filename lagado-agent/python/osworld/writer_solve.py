"""writer_solve.py — task-blind Writer document solver, the writer_ops/writer_daemon/
battery_writer analog of calc_solve.py: labeled paragraph candidates -> reason -> emit typed
operations -> apply via the resident Writer UNO daemon -> sound falsifiers -> one feedback
retry -> report, callable from the GENERAL agent's dispatch with exactly what that loop
legitimately knows:

    writer_solve.py <base_url> <guest_file_path> <instruction>

NO task JSON, NO evaluator knowledge reaches this process — same integrity contract as
calc_solve.py (2026-07-10): benchmarks come FROM the harness, never INTO the solver.

The final stdout line is one JSON verdict object:
    {"ok": bool, "self_report_done": bool, "declared_infeasible": str|null,
     "n_ops": int, "falsifiers": [...], "unverifiable": bool, "error": str|null}

Exit codes (mirrors calc_solve.py's contract):
    0 — ops applied, falsifiers clean, nothing genuinely unverifiable touched (honest done)
    3 — the model declared the task infeasible
    2 — operated but unverified (a fault was detected, OR the op touched something headless
        cannot read back, e.g. export_pdf/insert_page_break — never a fabricated pass)
    1 — infrastructure failure (transport/daemon/open) -> caller falls through to its floor
"""
import base64
import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import requests

import battery_writer as B
from run_session_task import Guest, pick_uno_python

LAGADO_OSW = os.path.dirname(os.path.abspath(__file__))
DAEMON_FILES = ["writer_ops.py", "writer_daemon.py", "uno_client.py"]
GUEST_DIR = "/tmp"
SOCK = "/tmp/lagado_writer_session.sock"


class UrlGuest(Guest):
    """Guest transport over the OSWorld server's /execute endpoint from a bare base_url — same
    shape as calc_solve.py's UrlGuest (sh()/client() inherited unchanged; they only use self.py)."""

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


def deploy_writer_daemon(g, unopy):
    """Same deploy pattern as run_session_task.py's deploy_daemon, targeting the WRITER daemon's
    own files/socket so it never collides with a concurrently-deployed Calc daemon in the same
    guest (distinct GUEST_DIR filenames, distinct SOCK, distinct UNO port owned by writer_daemon.py)."""
    for fn in DAEMON_FILES:
        b64 = base64.b64encode(open(os.path.join(LAGADO_OSW, fn), "rb").read()).decode()
        g.py("import base64;open('%s/%s','wb').write(base64.b64decode(%r))" % (GUEST_DIR, fn, b64))
    g.sh("rm -f %s; setsid %s %s/writer_daemon.py --sock=%s > /tmp/writer_daemon.log 2>&1 < /dev/null &"
         % (SOCK, unopy, GUEST_DIR, SOCK))
    for _ in range(20):
        r = g.sh("cat /tmp/writer_daemon.log 2>/dev/null; true")
        if "DAEMON READY" in r.get("out", ""):
            return True
        if "Traceback" in r.get("out", "") or "Error" in r.get("out", ""):
            print("  writer daemon log:\n", r.get("out"))
            return False
        time.sleep(1)
    print("  writer daemon did not signal READY; log:\n",
          g.sh("cat /tmp/writer_daemon.log; true").get("out"))
    return False


def emit(verdict, code):
    print(json.dumps(verdict), flush=True)
    raise SystemExit(code)


def main():
    if len(sys.argv) != 4:
        raise SystemExit("usage: writer_solve.py <base_url> <guest_file_path> <instruction>")
    base_url, file_path, instruction = sys.argv[1], sys.argv[2], sys.argv[3]
    v = {"ok": False, "self_report_done": False, "declared_infeasible": None,
         "n_ops": 0, "falsifiers": [], "unverifiable": False, "error": None}

    g = UrlGuest(base_url)
    unopy = pick_uno_python(g)
    if not unopy:
        v["error"] = "no guest python can import uno"
        emit(v, 1)
    # Clobber-avoidance (same discipline as calc_solve.py m1): the task-setup GUI soffice holds
    # the file lock; the daemon must own the file cleanly. A GLOBAL kill is safe here — nothing
    # of ours is running yet in this fresh guest invocation.
    g.sh("pkill -9 soffice; pkill -9 soffice.bin; true")
    g.sh("rm -f '%s/.~lock.%s#' 2>/dev/null; true"
         % (os.path.dirname(file_path), os.path.basename(file_path)))
    time.sleep(1)
    if not deploy_writer_daemon(g, unopy):
        v["error"] = "writer uno daemon did not come up"
        emit(v, 1)

    task = {"instruction": instruction, "id": "wsolve000"}
    log = {"id": "wsolve000", "steps": []}
    try:
        log = B.run_core(g, task, file_path, log)
    except Exception as e:
        v["error"] = "run_core: %r" % (e,)
        emit(v, 1)
    finally:
        try:
            os.makedirs(B.LOGDIR, exist_ok=True)
            with open(os.path.join(B.LOGDIR, "solve_%d.json" % int(time.time())), "w") as f:
                json.dump(log, f, default=str)
        except Exception:
            pass

    v["ok"] = True
    v["self_report_done"] = bool(log.get("self_report_done"))
    v["declared_infeasible"] = log.get("declared_infeasible")
    v["n_ops"] = int(log.get("n_ops") or 0)
    v["falsifiers"] = [f.get("falsifier") for f in log.get("falsifiers_fired", [])]
    v["unverifiable"] = bool(log.get("unverifiable"))
    if log.get("fatal"):
        v["ok"] = False
        v["error"] = log["fatal"]
        emit(v, 1)
    if v["declared_infeasible"] is not None:
        emit(v, 3)
    emit(v, 0 if v["self_report_done"] else 2)


if __name__ == "__main__":
    main()
