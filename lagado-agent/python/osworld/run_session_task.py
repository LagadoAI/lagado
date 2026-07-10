"""run_session_task.py — drive ONE OSWorld calc task through the NATIVE SESSION (resident UNO
daemon), hand-driven from an op-log, scored by the real env.evaluate(). No model, no agent.rs.

This is the Native Session Plane P2a bench (spec §9): prove the daemon works in the REAL guest +
the real evaluator before any Rust integration. It mirrors P1's host-local gate, lifted to the guest:
deploy uno_daemon/uno_ops/uno_client → launch the daemon → open → apply the hand-written op-log →
reconcile (GUI reload for the evaluator) → score.

CLOBBER-AVOIDANCE (m1): env.reset() opens the input in a GUI soffice that holds the file lock. We
GLOBAL-kill that here AT THE DRIVER (before the daemon launches — safe, nothing of ours is running
yet) and clear the lock, so the daemon owns the file cleanly. The daemon itself NEVER global-pkills
(host-safety); its reconcile spawns the corrected file into a fresh GUI for the evaluator's
activate_window + ctrl+s.

Usage (from the OSWorld repo dir, with its venv + the podman socket):
  DOCKER_HOST=unix:///run/podman/podman.sock \
  /home/alucard/projects/OSWorld/.venv/bin/python \
    /home/alucard/projects/lagado/lagado-agent/python/osworld/run_session_task.py \
    <task_json> <oplog_json> [repeat=1]
"""

import base64
import json
import os
import sys
import time

# DesktopEnv is imported lazily in main(): only the bench entry point boots an env. Importers of
# the shared pieces (Guest, deploy_daemon, pick_uno_python) — e.g. calc_solve.py driving a guest
# by URL from the general agent — must not require the OSWorld package/venv.

LAGADO_OSW = os.path.dirname(os.path.abspath(__file__))
DAEMON_FILES = ["uno_ops.py", "uno_daemon.py", "uno_client.py"]
GUEST_DIR = "/tmp"
SOCK = "/tmp/lagado_session.sock"

# OOM PREVENTION (2026-06-23): a 3G nested VM on a 15Gi host will thrash zram → OOM if the host is
# already tight (the classic cause: llama-server mmap'ing 7GB of weights it doesn't need with full GPU
# offload — launch the brain with --no-mmap, see start_brain.sh). The runners call preflight_memory()
# before every boot so they FAIL FAST with a clear message instead of driving the box into OOM.
MIN_FREE_MB = 4500   # 3G guest + container/qemu overhead + margin


def free_memory_mb():
    """Host MemAvailable in MB (kernel's estimate of reclaimable+free), or None if unreadable."""
    try:
        with open("/proc/meminfo") as f:
            for line in f:
                if line.startswith("MemAvailable:"):
                    return int(line.split()[1]) // 1024
    except Exception:
        pass
    return None


def memory_ok(min_mb=MIN_FREE_MB):
    """True if there's enough headroom to boot a VM without OOM risk. Prints a diagnosis when not."""
    avail = free_memory_mb()
    if avail is not None and avail < min_mb:
        print("\n*** MEMORY PRE-FLIGHT FAIL: only %d MB available (<%d MB floor) — booting a 3G VM here "
              "will thrash zram toward OOM. Free memory first:\n"
              "    - relaunch the brain lean:  lagado-agent/python/osworld/start_brain.sh   (--no-mmap frees ~7GB)\n"
              "    - reclaim stale VM disk:    podman volume prune -f\n"
              "    - close heavy apps.\n*** Aborting before boot (no OOM)." % (avail, min_mb), flush=True)
        return False
    return True


def task_input_path(task):
    """The guest path of the input doc: prefer the 'open' config, fall back to the download."""
    for c in task.get("config", []):
        if c.get("type") == "open":
            return c["parameters"]["path"]
    for c in task.get("config", []):
        if c.get("type") == "download":
            return c["parameters"]["files"][0]["path"]
    raise SystemExit("could not find input path in task config")


class Guest:
    """Thin wrapper over the OSWorld /execute channel (120 s cap per call)."""

    def __init__(self, env):
        self.env = env

    def py(self, code):
        r = self.env.controller.execute_python_command(code)
        return (r.get("output", "") if isinstance(r, dict) else str(r))

    def sh(self, cmd, timeout=110):
        code = ("import subprocess as _s,json as _j;"
                "r=_s.run(%r,shell=True,capture_output=True,text=True,timeout=%d);"
                "print(_j.dumps({'out':r.stdout[-2000:],'err':r.stderr[-1000:],'rc':r.returncode}))"
                % (cmd, timeout))
        raw = self.py(code)
        for ln in reversed(raw.splitlines()):
            ln = ln.strip()
            if ln.startswith("{"):
                try:
                    return json.loads(ln)
                except Exception:
                    pass
        return {"out": raw, "err": "", "rc": -1}

    def client(self, verb, args=None):
        """Run uno_client.py <verb> <json-args> in the guest, return the parsed JSON response."""
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


def pick_uno_python(g):
    """Find a guest interpreter that can `import uno` (the daemon needs it)."""
    for interp in ("python3", "/usr/lib/libreoffice/program/python", "/usr/bin/python3"):
        r = g.sh("%s -c 'import uno; print(\"ok\")' 2>&1" % interp)
        if r.get("rc") == 0 and "ok" in r.get("out", ""):
            return interp
    return None


def deploy_daemon(g, unopy):
    for fn in DAEMON_FILES:
        b64 = base64.b64encode(open(os.path.join(LAGADO_OSW, fn), "rb").read()).decode()
        g.py("import base64;open('%s/%s','wb').write(base64.b64decode(%r))" % (GUEST_DIR, fn, b64))
    # Launch detached (setsid → survives the /execute shell exit); dedicated sock.
    g.sh("rm -f %s; setsid %s %s/uno_daemon.py --sock=%s > /tmp/daemon.log 2>&1 < /dev/null &"
         % (SOCK, unopy, GUEST_DIR, SOCK))
    for _ in range(20):
        r = g.sh("cat /tmp/daemon.log 2>/dev/null; true")
        if "DAEMON READY" in r.get("out", ""):
            return True
        if "Traceback" in r.get("out", "") or "Error" in r.get("out", ""):
            print("  daemon log:\n", r.get("out"))
            return False
        time.sleep(1)
    print("  daemon did not signal READY; log:\n", g.sh("cat /tmp/daemon.log; true").get("out"))
    return False


def run_once(env, task, ops, file_path):
    g = Guest(env)
    stem = os.path.splitext(os.path.basename(file_path))[0]

    unopy = pick_uno_python(g)
    if not unopy:
        print("  FATAL: no guest python can import uno"); return None
    print("  uno python:", unopy)

    # CLOBBER-AVOIDANCE: kill the reset()-opened GUI + its lock BEFORE the daemon opens the file.
    g.sh("pkill -9 soffice; pkill -9 soffice.bin; true")
    g.sh("rm -f '%s/.~lock.%s#' 2>/dev/null; true" % (os.path.dirname(file_path), os.path.basename(file_path)))
    time.sleep(1)

    if not deploy_daemon(g, unopy):
        return None

    r = g.client("open", {"file": file_path})
    if not r.get("ok"):
        print("  open failed:", r); return None
    print("  opened; sheets:", r.get("structure", {}).get("sheets"))

    for i, op in enumerate(ops):
        r = g.client("apply", {"op": op})
        print("  apply[%d] %s -> %s" % (i, op.get("op"), "ok" if r.get("ok") else r.get("error")))
        if not r.get("ok"):
            print("  (op rejected — continuing to reconcile what applied)")

    # reconcile WITH the guest GUI reload (the evaluator activates the window + ctrl+s)
    r = g.client("reconcile", {"gui": True})
    print("  reconcile:", r)
    g.client("close")
    time.sleep(4)  # let the GUI window come up before the evaluator activates it

    score = env.evaluate() or 0.0
    return score


def main():
    from desktop_env.desktop_env import DesktopEnv   # lazy: see header note
    if len(sys.argv) < 3:
        raise SystemExit("usage: run_session_task.py <task_json> <oplog_json> [repeat]")
    task = json.load(open(sys.argv[1]))
    ops = json.load(open(sys.argv[2]))
    repeat = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    file_path = task_input_path(task)
    print("task:", task["id"], "| input:", file_path, "| ops:", len(ops), "| repeat:", repeat)

    scores = []
    env = DesktopEnv(provider_name="docker", action_space="pyautogui",
                     screen_size=(1920, 1080), headless=True, os_type="Ubuntu",
                     require_a11y_tree=False)
    try:
        for run in range(repeat):
            print("\n=== run %d/%d ===" % (run + 1, repeat))
            env.reset(task_config=task)
            time.sleep(2)
            s = run_once(env, task, ops, file_path)
            print("  SCORE:", s)
            scores.append(s)
    finally:
        env.close()

    print("\n==== %s : scores = %s ====" % (task["id"], scores))
    ok = [s for s in scores if s == 1.0]
    print("gold %d/%d" % (len(ok), len(scores)))
    return 0 if (scores and all(s == 1.0 for s in scores)) else 1


if __name__ == "__main__":
    sys.exit(main())
