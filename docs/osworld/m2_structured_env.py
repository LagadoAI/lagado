"""
M2 structured authoring tested on the REAL OSWorld benchmark (env.evaluate) — NOT the host-only proxy.

Per task: boot the guest (env.reset) → author ops via the silver-platter GBNF (no leading prompt) → apply
on a host copy through real LibreOffice/UNO (the formula engine computes) → kill the original guest instance,
push the corrected file, reload-into-focus (M1) → env.evaluate(). The number is OSWorld's own evaluator.

Run: DOCKER_HOST=unix:///run/podman/podman.sock .venv/bin/python m2_structured_env.py [N]
"""
import sys, os, glob, json, shutil, subprocess, base64, logging
logging.basicConfig(level=logging.WARNING)
from desktop_env.desktop_env import DesktopEnv
from m1_reconcile import make_helpers, kill_app, reload_into_focus
from m2_structured import author_structured
from m2_uno import task_io, structure, EXDIR

SYS_PY = "/usr/bin/python3"
WORK = "/tmp/m2_work.xlsx"
OPSF = "/tmp/m2_ops.json"

N = next((int(a) for a in sys.argv[1:] if a.isdigit()), 4)
files = sorted(glob.glob(EXDIR + "/*.json"))
tasks = []
for tf in files:
    if len(tasks) >= N:
        break
    t = task_io(tf)
    if t:
        tasks.append((tf, t))

print("=== M2 STRUCTURED on REAL OSWorld env.evaluate | %d calc tasks ===" % len(tasks), flush=True)
env = DesktopEnv(provider_name="docker", action_space="pyautogui", screen_size=(1920, 1080),
                 headless=True, os_type="Ubuntu", require_a11y_tree=False)
results = []
for tf, t in tasks:
    task = json.load(open(tf))
    score, nops, applied = 0.0, 0, False
    try:
        env.reset(task_config=task)
        gpy, sh = make_helpers(env)
        ops_text, _raw = author_structured(t["instr"], structure(t["inl"]))
        ops = json.loads(ops_text)
        nops = len(ops)
        json.dump(ops, open(OPSF, "w"))
        shutil.copy(t["inl"], WORK)
        ap = subprocess.run([SYS_PY, "uno_apply.py", WORK, OPSF], capture_output=True, text=True, timeout=120)
        applied = "APPLIED" in ap.stdout
        base = os.path.basename(t["guest_path"])
        stem = os.path.splitext(base)[0]
        kill_app(sh, "soffice", stem)
        sh("rm -f '/home/user/.~lock.%s#' 2>/dev/null; true" % base)
        b64 = base64.b64encode(open(WORK, "rb").read()).decode()
        gpy("import base64; open(%r,'wb').write(base64.b64decode(%r)); print('W')" % (t["guest_path"], b64))
        reload_into_focus(sh, t["guest_path"], "soffice --calc", stem)
        sh("sleep 4; true")
        score = env.evaluate() or 0.0
    except Exception as e:
        print("   (exc %s: %s)" % (t["tid"], str(e)[:160]), flush=True)
    results.append((t["tid"], score, nops, applied))
    print("  [%s] REAL env.evaluate=%.0f (ops=%d applied=%s)  %s"
          % (t["tid"], score, nops, applied, t["instr"][:50]), flush=True)
    json.dump([{"tid": a, "score": b, "ops": c, "applied": d} for a, b, c, d in results],
              open("/tmp/m2_structured_env.json", "w"), indent=1)
env.close()

p = sum(1 for _, s, _, _ in results if s >= 1.0)
print("\n=== STRUCTURED on REAL OSWorld: %d/%d ===" % (p, len(results)), flush=True)
for tid, s, n, a in results:
    print("   %s  %.0f" % (tid, s), flush=True)
