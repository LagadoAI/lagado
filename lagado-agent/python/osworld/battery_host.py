#!/usr/bin/env python3
"""HOST-side FAST loop — the same battery core (battery_calc.run_core) against a LOCAL soffice daemon, scored by
the REAL OSWorld metric funcs. NO VM. Same brain (:8080), same reason→emit, same apply, same scoring; differs
ONLY in host-LO vs guest-LO (which matters solely for render-type tasks: sheet_print/CSV/PDF). ~1 min/task vs
~50 min in the VM — built to RE-MEASURE with error bars (run each task N× to quantify temp-0 variance).

Run with the OSWorld .venv python (it has the metric deps); the daemon is spawned with /usr/bin/python3 (uno):
  cd /home/alucard/projects/OSWorld && PYTHONPATH=/home/alucard/projects/OSWorld:/home/alucard/projects/lagado/lagado-agent/python/osworld \
    .venv/bin/python /home/alucard/projects/lagado/lagado-agent/python/osworld/battery_host.py <ids... | heldout> [N]
"""
import json, os, sys, glob, time, shutil, subprocess, signal, traceback
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import requests
import uno_client
import battery_calc
from desktop_env.evaluators import metrics

HERE = os.path.dirname(os.path.abspath(__file__))
OSW = "/home/alucard/projects/OSWorld"
EX = os.path.join(OSW, "evaluation_examples/examples/libreoffice_calc")
CACHE = os.path.join(OSW, "cache")
SYS_PY = "/usr/bin/python3"                       # the interpreter that can import uno
WORK = "/tmp/lagado_host"
LOGDIR = "/tmp/lagado_battery"
HELDOUT = battery_calc.__dict__.get("HELDOUT")    # reuse if present


class HostGuest:
    """Daemon client with the SAME interface battery_calc.run_core expects (client/sh). Spawns a local
    uno_daemon under SYS_PY (uno) and proxies verbs over its Unix socket."""
    def __init__(self, sock, port):
        self.sock = sock
        self.proc = subprocess.Popen([SYS_PY, os.path.join(HERE, "uno_daemon.py"),
                                      "--sock=%s" % sock, "--port=%d" % port],
                                     stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        t0 = time.time()
        while time.time() - t0 < 70:
            line = self.proc.stdout.readline()
            if not line:
                raise RuntimeError("daemon exited early")
            if "DAEMON READY" in line:
                return
        raise RuntimeError("daemon did not signal READY")

    def client(self, verb, args=None):
        req = {"verb": verb}
        if args:
            req.update(args)
        try:
            return uno_client.call(self.sock, req)
        except Exception as e:
            return {"ok": False, "error": "client error: %s" % e}

    def sh(self, cmd, timeout=60):
        try:
            r = subprocess.run(cmd, shell=True, capture_output=True, text=True, timeout=timeout)
            return {"ok": r.returncode == 0, "out": r.stdout + r.stderr}
        except Exception as e:
            return {"ok": False, "out": str(e)}

    def kill(self):
        try:
            self.client("close")
        except Exception:
            pass
        try:
            self.proc.send_signal(signal.SIGINT); self.proc.wait(timeout=10)
        except Exception:
            try: self.proc.kill()
            except Exception: pass


def fetch_input(task):
    """Local writable master copy of the task's INPUT doc (download config). Cached in WORK."""
    for c in task.get("config", []):
        if c.get("type") == "download":
            f = c["parameters"]["files"][0]
            base = os.path.basename(f["path"])
            master = os.path.join(WORK, "%s__%s" % (task["id"][:8], base))
            if not os.path.exists(master):
                r = requests.get(f["url"], timeout=180); r.raise_for_status()
                open(master, "wb").write(r.content)
            return master, base
    # no download → an 'open' of a file already on disk (rare in this set)
    for c in task.get("config", []):
        if c.get("type") == "open":
            p = c["parameters"]["path"]
            local = os.path.join(CACHE, task["id"], os.path.basename(p))
            if os.path.exists(local):
                return local, os.path.basename(p)
    return None, None


def _gold(task, exp_cfg):
    """Local cached gold path for a cloud_file expected config."""
    if exp_cfg and exp_cfg.get("type") == "cloud_file":
        return os.path.join(CACHE, task["id"], exp_cfg.get("dest", ""))
    return None


def host_score(task, result_file):
    """Replicate DesktopEnv.evaluate() on local files: result=our saved xlsx, expected=cached gold, calling the
    SAME metric funcs with the SAME options. Render-type funcs (compare_pdfs/check_pdf_pages) are out of scope
    on host (host-LO render ≠ guest); they return None → reported as RENDER-SKIP, not a false 0."""
    ev = task["evaluator"]
    func = ev["func"]
    RENDER = {"compare_pdfs", "check_pdf_pages", "compare_image_list"}
    def one(fname, opt, exp):
        if fname in RENDER:
            return None
        mf = getattr(metrics, fname)
        exp_file = _gold(task, exp)
        o = opt or {}
        try:
            return float(mf(result_file, exp_file, **o)) if exp_file is not None else float(mf(result_file, **o))
        except Exception as e:
            return ("ERR", "%s: %s" % (type(e).__name__, str(e)[:80]))
    if isinstance(func, list):
        opts = ev.get("options", [{}] * len(func))
        exps = ev.get("expected", [None] * len(func))
        conj = ev.get("conj", "and")
        vals = []
        for i, fn in enumerate(func):
            v = one(fn, opts[i] if i < len(opts) else {}, exps[i] if i < len(exps) else None)
            if v is None:
                return None                      # any render metric → whole task is render-skip on host
            if isinstance(v, tuple):
                return v
            vals.append(v)
            if conj == "and" and v == 0.0:
                return 0.0
            if conj == "or" and v == 1.0:
                return 1.0
        return sum(vals) / len(vals) if conj == "and" else max(vals)
    return one(func, ev.get("options", {}), ev.get("expected"))


def run_one(task, run_idx, port):
    """One host run: fresh input copy → daemon → shared run_core → host scoring."""
    master, base = fetch_input(task)
    if not master:
        return None, {"fatal": "no input file"}
    run_path = os.path.join(WORK, "%s_r%d_%s" % (task["id"][:8], run_idx, base))
    shutil.copy(master, run_path)
    # Pre-clean ANY stray soffice carrying OUR markers (daemon profile or host temp). Orphans accumulate when a
    # visible recovery dialog blocks clean shutdown, and LibreOffice's single-instance reuse then makes the next
    # launch join the orphan instead of our fresh recovery-off profile. Scoped to our paths — host-only file.
    subprocess.run("pkill -9 -f lagado_uno_daemon_profile; pkill -9 -f lagado_host; true",
                   shell=True, stderr=subprocess.DEVNULL)
    time.sleep(0.5)
    sock = "/tmp/lagado_host_%s_%d.sock" % (task["id"][:8], run_idx)
    log = {"cond": "B", "run": run_idx, "id": task["id"][:8], "steps": [], "host": True}
    g = None
    try:
        g = HostGuest(sock, port)
        score, log = battery_calc.run_core(g, task, "B", run_path, log,
                                           lambda: host_score(task, run_path))
    except Exception as e:
        log["fatal"] = "host run: %s" % e
        log["exc"] = traceback.format_exc()[-300:]
        score = 0.0
    finally:
        if g:
            g.kill()
        subprocess.run("pkill -f 'lagado_host_%s_%d'  ; true" % (task["id"][:8], run_idx),
                       shell=True, stderr=subprocess.DEVNULL)
    return score, log


def main():
    argv = sys.argv[1:]
    N = 1
    ids = []
    for a in argv:
        if a.isdigit():
            N = int(a)
        elif a == "heldout" and HELDOUT:
            ids = list(HELDOUT)
        else:
            ids.append(a)
    os.makedirs(WORK, exist_ok=True); os.makedirs(LOGDIR, exist_ok=True)
    # Host-side runs are WATCHED by default (demo + own-eyes proof): show the real LibreOffice window whenever a
    # display exists. Opt out with LAGADO_HEADLESS=1 (e.g. bulk variance sweeps). No display → headless anyway.
    if not os.environ.get("LAGADO_HEADLESS") and os.environ.get("DISPLAY"):
        os.environ.setdefault("LAGADO_VISIBLE", "1")
    print("    mode: %s" % ("VISIBLE (watch the app)" if os.environ.get("LAGADO_VISIBLE") else "headless"), flush=True)
    files = []
    for i in ids:
        gg = sorted(glob.glob("%s/%s*.json" % (EX, i)))
        if gg:
            files.append(gg[0])
    print("=== HOST BATTERY (no VM) — %d tasks × N=%d ===" % (len(files), N), flush=True)
    logf = os.path.join(LOGDIR, "host_logs.jsonl")
    summary = []
    for ti, tf in enumerate(files):
        task = json.load(open(tf)); tid = task["id"][:8]
        scores = []
        render_skip = False
        for run in range(N):
            score, log = run_one(task, run, 2200 + (ti * 7 + run) % 500)
            if score is None:
                render_skip = True
                print("  [%s] run %d: RENDER-SKIP (host-LO render ≠ guest)" % (tid, run), flush=True)
                open(logf, "a").write(json.dumps(log, default=str) + "\n")
                break
            if isinstance(score, tuple):
                print("  [%s] run %d: SCORE-ERR %s" % (tid, run, score[1]), flush=True)
                open(logf, "a").write(json.dumps(log, default=str) + "\n")
                continue
            scores.append(score)
            nm = log.get("nameops") or []
            print("  [%s] run %d: score=%.2f self_done=%s false_pass=%s emitted=%s"
                  % (tid, run, score, log.get("self_report_done"), log.get("false_pass"),
                     sorted({o.get('kind') for o in nm})), flush=True)
            open(logf, "a").write(json.dumps(log, default=str) + "\n")
        if render_skip:
            summary.append((tid, "RENDER-SKIP", scores)); continue
        if not scores:
            summary.append((tid, "ERR", [])); continue
        golds = sum(1 for s in scores if s >= 1.0)
        verdict = "GOLD" if golds == len(scores) else ("FLAKY %d/%d" % (golds, len(scores)) if golds else "MISS")
        summary.append((tid, verdict, scores))
        print("  [%s] => %s  scores=%s" % (tid, verdict, scores), flush=True)
    print("\n" + "=" * 60, flush=True)
    print("HOST SUMMARY (N=%d):" % N, flush=True)
    for tid, verdict, scores in summary:
        print("  %-9s %-12s %s" % (tid, verdict, scores), flush=True)
    fp = sum(1 for _t, _v, ss in summary for s in ss if False)  # false-pass tracked per-log
    print("\n  (variance: any FLAKY = temp-0 nondeterminism; GOLD across N = stable)", flush=True)


if __name__ == "__main__":
    main()
