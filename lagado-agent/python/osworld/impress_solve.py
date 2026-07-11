"""impress_solve.py — task-blind Impress solver: the presentation analog of calc_solve.py. Same
pipeline shape (labeled candidates -> reason -> emit-in-NAMES -> fail-closed shape resolve ->
apply via the resident impress_daemon.py session -> sound falsifiers -> read-only corroboration),
callable from the GENERAL agent's dispatch with exactly what that loop legitimately knows:

    impress_solve.py <base_url> <guest_file_path> <instruction>

NO task JSON, NO evaluator knowledge reaches this process (same integrity contract as
calc_solve.py, 2026-07-10: benchmarks come FROM the harness, never leak INTO it). Scoring stays
with the caller; nothing here reads golds.

The final stdout line is one JSON verdict object:
    {"ok": bool, "self_report_done": bool, "declared_infeasible": str|null,
     "n_ops": int, "falsifiers": [...], "error": str|null}

Exit codes (identical routing contract to calc_solve.py):
    0 — ops applied, falsifiers clean, independently corroborated (honest done)
    3 — the model declared the task infeasible (caller translates to the FAIL answer)
    2 — operated but unverified (caller hands back honestly)
    1 — infrastructure failure (transport/daemon/open) -> caller falls through to its floor
"""
import base64
import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import requests

import battery_impress as B
from run_session_task import Guest, pick_uno_python

LAGADO_OSW = os.path.dirname(os.path.abspath(__file__))
DAEMON_FILES = ["impress_ops.py", "impress_daemon.py", "uno_client.py"]
GUEST_DIR = "/tmp"
SOCK = "/tmp/lagado_impress_session.sock"   # DISTINCT from calc's socket — coexistence, not collision


class UrlGuest(Guest):
    """Guest transport over the OSWorld server's /execute endpoint from a bare base_url — the
    same channel Guest(env) wraps, without needing a DesktopEnv handle (calc_solve.py's pattern).
    `client()` is overridden (not inherited) because it must target THIS plane's own socket path,
    distinct from the Calc daemon's — the two can be live on the same guest at once."""

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

    def client(self, verb, args=None):
        payload = json.dumps(args or {})
        code = ("import subprocess,json;"
                "r=subprocess.run(['python3','%s/uno_client.py',%r,%r,'--sock=%s'],capture_output=True,text=True,timeout=110);"
                "print(r.stdout);"
                "import sys;sys.stderr.write(r.stderr)" % (GUEST_DIR, verb, payload, SOCK))
        out = self.py(code)
        for ln in reversed(out.splitlines()):
            ln = ln.strip()
            if ln.startswith("{"):
                try:
                    return json.loads(ln)
                except Exception:
                    pass
        return {"ok": False, "error": "no JSON from uno_client (raw: %r)" % out[-400:]}


def deploy_impress_daemon(g, unopy):
    """Deploy + launch impress_daemon.py on its OWN socket/UNO port — the Impress analog of
    run_session_task.deploy_daemon(), duplicated (not shared) because the file list, socket
    path, and launch target differ from the Calc daemon's."""
    for fn in DAEMON_FILES:
        b64 = base64.b64encode(open(os.path.join(LAGADO_OSW, fn), "rb").read()).decode()
        g.py("import base64;open('%s/%s','wb').write(base64.b64decode(%r))" % (GUEST_DIR, fn, b64))
    g.sh("rm -f %s; setsid %s %s/impress_daemon.py --sock=%s --port=2003 > /tmp/impress_daemon.log 2>&1 < /dev/null &"
         % (SOCK, unopy, GUEST_DIR, SOCK))
    for _ in range(20):
        r = g.sh("cat /tmp/impress_daemon.log 2>/dev/null; true")
        if "DAEMON READY" in r.get("out", ""):
            return True
        if "Traceback" in r.get("out", "") or "Error" in r.get("out", ""):
            print("  impress daemon log:\n", r.get("out"))
            return False
        time.sleep(1)
    print("  impress daemon did not signal READY; log:\n",
         g.sh("cat /tmp/impress_daemon.log; true").get("out"))
    return False


def emit(verdict, code):
    print(json.dumps(verdict), flush=True)
    raise SystemExit(code)


def main():
    if len(sys.argv) != 4:
        raise SystemExit("usage: impress_solve.py <base_url> <guest_file_path> <instruction>")
    base_url, file_path, instruction = sys.argv[1], sys.argv[2], sys.argv[3]
    v = {"ok": False, "self_report_done": False, "declared_infeasible": None,
         "n_ops": 0, "falsifiers": [], "error": None}

    g = UrlGuest(base_url)
    unopy = pick_uno_python(g)
    if not unopy:
        v["error"] = "no guest python can import uno"
        emit(v, 1)
    # Clobber-avoidance (same m1 pattern as run_session_task/calc_solve): the task-setup GUI
    # soffice holds the file lock; the daemon must own the file cleanly.
    g.sh("pkill -9 soffice; pkill -9 soffice.bin; true")
    g.sh("rm -f '%s/.~lock.%s#' 2>/dev/null; true"
         % (os.path.dirname(file_path), os.path.basename(file_path)))
    time.sleep(1)
    if not deploy_impress_daemon(g, unopy):
        v["error"] = "impress daemon did not come up"
        emit(v, 1)

    task = {"instruction": instruction, "id": "solve000", "evaluator": {}}
    log = {"cond": "impress", "run": 0, "id": "solve000", "steps": []}
    try:
        _score, log = B.run_core(g, task, file_path, log, lambda: None)
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
    if log.get("fatal"):
        v["ok"] = False
        v["error"] = log["fatal"]
        emit(v, 1)
    if v["declared_infeasible"] is not None:
        emit(v, 3)
    emit(v, 0 if v["self_report_done"] else 2)


if __name__ == "__main__":
    main()
