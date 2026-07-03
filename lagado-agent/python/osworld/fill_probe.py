"""fill_probe.py — ISOLATE the calc fill defect on the real guest (verify-first, no fix yet).

Background: on 035f41ba the readback showed `B2-C2, B2-C3, B2-C4 …` — only the LAST ref
advancing per row. Two hypotheses:
  H1 (missing '='): the model emitted formula="{Sales}-{Sales Return}" → substitute → "B2-C2"
     with NO leading '='. setFormula("B2-C2") stores TEXT, and fillAuto then increments the
     trailing digit as a TEXT SERIES (B2-C3, B2-C4 …) — never a formula, never computes.
  H2 (fillAuto relative-copy bug): even a real "=B2-C2" fills wrong (=B2-C3 …).

This probe writes to SCRATCH columns on a real open doc and reads back the COMPUTED values
(the daemon's read returns the value for FORMULA cells, the literal text for TEXT cells), so
the two hypotheses are distinguishable:
  col L  formula="B2-C2"   (NO '=', exactly what the harness currently sends)
  col M  formula="=B2-C2"  ('=' prepended — the candidate fix)
Known data (035f41ba): B2,C2 = 78000,3000 → 75000 ; B3,C3 = 73423,3884 → 69539.
  • H1 true  → L = text strings "B2-C2","B2-C3",… ; M = numbers 75000, 69539, … (CORRECT) → fix = prepend '='
  • H2 true  → M3 = 74116 (=B2-C3, wrong) → fillAuto itself is broken, prepend-'=' is NOT enough

Throwaway probe over the proven native session. Does NOT score, does NOT touch the floor.

Run (from OSWorld repo dir, its venv, podman sock):
  DOCKER_HOST=unix:///run/podman/podman.sock PYTHONPATH=/home/alucard/projects/OSWorld \
  .venv/bin/python /home/alucard/projects/lagado/lagado-agent/python/osworld/fill_probe.py
"""
import json, os, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from run_session_task import Guest, deploy_daemon, pick_uno_python, task_input_path
from desktop_env.desktop_env import DesktopEnv

TASK = "/home/alucard/projects/OSWorld/evaluation_examples/examples/libreoffice_calc/035f41ba-6653-43ab-aa63-c86d449d62e5.json"


def readcol(g, col):
    r = g.client("read", {"sheet": "Sheet1", "range": "%s2:%s10" % (col, col)})
    if not r.get("ok"):
        return "READ FAIL: %s" % r.get("error")
    return [row[0] if row else None for row in r.get("cells", [])]


def main():
    task = json.load(open(TASK))
    file_path = task_input_path(task)
    env = DesktopEnv(provider_name="docker", action_space="pyautogui", screen_size=(1920, 1080),
                     headless=True, os_type="Ubuntu", require_a11y_tree=False)
    try:
        env.reset(task_config=task)
        time.sleep(2)
        g = Guest(env)
        unopy = pick_uno_python(g)
        print("uno python:", unopy, flush=True)
        g.sh("pkill -9 soffice; pkill -9 soffice.bin; true")
        g.sh("rm -f '%s/.~lock.%s#' 2>/dev/null; true" % (os.path.dirname(file_path), os.path.basename(file_path)))
        time.sleep(1)
        assert deploy_daemon(g, unopy), "daemon not ready"
        r = g.client("open", {"file": file_path})
        assert r.get("ok"), "open failed: %s" % r.get("error")

        # exactly what the harness currently sends (NO '=')
        a = g.client("apply", {"op": {"op": "set_formula_range", "sheet": "Sheet1",
                                      "range": "L2:L10", "formula": "B2-C2"}})
        # candidate fix: leading '=' prepended
        b = g.client("apply", {"op": {"op": "set_formula_range", "sheet": "Sheet1",
                                      "range": "M2:M10", "formula": "=B2-C2"}})
        print("apply L (no '='):", a, flush=True)
        print("apply M (with '='):", b, flush=True)

        L = readcol(g, "L")
        M = readcol(g, "M")
        print("\n=== READBACK (computed values; FORMULA→value, TEXT→string) ===", flush=True)
        print("L (no '='): ", L, flush=True)
        print("M (with '='):", M, flush=True)

        expected = [75000, 69539]  # rows 2,3 hand-computed (B-C)
        print("\n=== VERDICT ===", flush=True)
        l_is_text = any(isinstance(x, str) for x in L)
        m_correct = (isinstance(M[0], (int, float)) and abs(M[0] - expected[0]) < 0.5 and
                     isinstance(M[1], (int, float)) and abs(M[1] - expected[1]) < 0.5)
        print("L holds TEXT (series-fill bug reproduced): %s" % l_is_text, flush=True)
        print("M computes & is row-correct (75000, 69539 …): %s" % m_correct, flush=True)
        if l_is_text and m_correct:
            print(">>> H1 CONFIRMED: missing '=' → text stored → fillAuto text-series. FIX = prepend '='.", flush=True)
        elif l_is_text and not m_correct:
            print(">>> PARTIAL: '=' makes it a formula but M is still wrong → fillAuto relative-copy ALSO broken. M=%s" % M, flush=True)
        else:
            print(">>> UNEXPECTED — inspect L/M above.", flush=True)
        g.client("close")
    finally:
        env.close()


if __name__ == "__main__":
    main()
