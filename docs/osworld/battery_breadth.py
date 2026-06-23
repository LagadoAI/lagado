"""(a) — BREADTH: lift the proven Condition-B loop across the calc value/formula suite.

Runs the full good-conditions loop (detect candidates → reason→emit in names → resolve fail-closed →
read-back/falsify → retry) on many real OSWorld calc tasks, scored by the real env.evaluate(). This is the
honest transfer number beyond the single 035f41ba proof, and it validates the P4 coverage levers
(header-row detection + {#N}) on REAL varied sheets. Per-task attribution localizes every non-gold.

Usage (OSWorld dir, its venv, podman sock):
  DOCKER_HOST=unix:///run/podman/podman.sock PYTHONPATH=/home/alucard/projects/OSWorld \
  .venv/bin/python battery_breadth.py <id1> <id2> ...     # task-id prefixes; default = the 16-task sample
"""
import json, os, sys, glob, time, traceback, signal
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from battery_calc import run_condition
from run_session_task import task_input_path, memory_ok
from desktop_env.desktop_env import DesktopEnv

PER_TASK = 420                                    # hard per-task ceiling so one hang can't wedge the sweep
class _Timeout(Exception): pass
signal.signal(signal.SIGALRM, lambda s, f: (_ for _ in ()).throw(_Timeout()))

EX = "evaluation_examples/examples/libreoffice_calc"
SAMPLE = ["01b269ae", "035f41ba", "04d9aeaf", "0bf05a7d", "1273e544", "1e8df695", "26a8440e",
          "357ef137", "42e0a640", "4de54231", "4e6fcf72", "7e429b8d", "7efeb4b1", "d681960f",
          "ecb0df7a", "f9584479"]

# HELD-OUT = the 30 calc tasks NEVER opened/referenced in any driver/doc (the 47-task suite minus the 16
# keyword-SAMPLE above and 12382c62, the known chart-break). The clean transfer set: no task here informed
# any prompt, threshold, or fix. Run with `battery_breadth.py heldout`. (anti-cherry-pick: the WHOLE set,
# chart/pivot/format included — those are EXPECTED op-vocab fails, reported not hidden.)
HELDOUT = ["0326d92d", "0a2e43bf", "0cecd4f3", "1334ca3e", "1954cced", "1d17d234", "1de60575", "21ab7b40",
           "21df9241", "2bd59342", "30e3e107", "347ef137", "37608790", "3a7c8185", "3aaa4e37", "4172ea6e",
           "4188d3a4", "4f07fbe9", "51719eea", "51b11269", "535364ea", "6054afcb", "6e99a1ad", "7a4e4bc8",
           "8b1ce5f2", "a01fbce3", "a9f325aa", "aa3a8974", "abed40dc", "eb03d19a"]

def attribution(score, log):
    if score >= 1.0:
        return "GOLD"
    if log.get("fatal"):
        return "SETUP-FAIL(%s)" % log["fatal"][:40]
    if log.get("resolve_fails"):
        return "FAIL-CLOSED(resolve)"
    if log.get("falsifiers_fired"):
        return "FALSIFIER(%s)" % ",".join(sorted({f["falsifier"] for f in log["falsifiers_fired"]}))
    if log.get("self_report_done"):
        return "SILENT-WRONG(false-pass)"        # the integrity-concerning bucket
    if log.get("corroborated") is False:
        return "ABSTAIN(uncorroborated)"          # honest: wrote something, but re-derivation disagreed
    return "WRONG(authored, oracle=0)"

def main():
    argv = sys.argv[1:]
    if argv == ["heldout"]:
        ids = HELDOUT
    else:
        ids = argv or SAMPLE
    files = []
    for i in ids:
        g = sorted(glob.glob("%s/%s*.json" % (EX, i)))
        if g:
            files.append(g[0])
    print("=== (a) BREADTH — Condition B across %d calc value/formula tasks ===" % len(files), flush=True)
    env = DesktopEnv(provider_name="docker", action_space="pyautogui", screen_size=(1920, 1080),
                     headless=True, os_type="Ubuntu", require_a11y_tree=False)
    results = []
    try:
        for tf in files:
            task = json.load(open(tf)); tid = task["id"][:8]
            print("\n── [%s] %s" % (tid, task.get("instruction", "")[:78]), flush=True)
            if not memory_ok():               # FAIL FAST before each boot — never thrash toward OOM
                print("   stopping sweep early — memory floor breached (see message above).", flush=True)
                break
            score, log, attr = 0.0, {}, "?"
            try:
                file_path = task_input_path(task)
                env.reset(task_config=task); time.sleep(2)
                signal.alarm(PER_TASK)
                try:
                    score, log = run_condition(env, task, "B", file_path, 0)
                finally:
                    signal.alarm(0)
                attr = attribution(score, log)
            except _Timeout:
                attr = "TIMEOUT(>%ds)" % PER_TASK
            except Exception as e:
                attr = "EXC(%s)" % str(e)[:50]
                log = {"exc": traceback.format_exc()[-300:]}
            # EMISSION axis (advisor): capture WHICH verbs the model emitted + the ops, so a non-gold can be
            # split into "verb not built" vs "verb built but model emitted wrong/none" vs "emitted+wrong answer".
            nameops = log.get("nameops") or []
            verbs = sorted({o.get("kind") for o in nameops if isinstance(o, dict)})
            results.append({"id": tid, "score": score, "attr": attr,
                            "instr": task.get("instruction", "")[:70],
                            "emitted_verbs": verbs, "n_ops": len(nameops)})
            print("   score=%s  %s  emitted=%s" % (score, attr, verbs), flush=True)
            json.dump(results, open("/tmp/lagado_battery/breadth.json", "w"), indent=1, default=str)
            open("/tmp/lagado_battery/breadth_logs.jsonl", "a").write(json.dumps(log, default=str) + "\n")
    finally:
        env.close()

    gold = sum(1 for r in results if r["score"] >= 1.0)
    print("\n" + "=" * 64, flush=True)
    print("  BREADTH (Condition B): %d/%d GOLD" % (gold, len(results)), flush=True)
    from collections import Counter
    for attr, n in Counter(r["attr"].split("(")[0] for r in results).most_common():
        print("    %-22s %d" % (attr, n), flush=True)
    print("\n  per task:", flush=True)
    for r in results:
        print("    %-9s %.0f  %-26s %s" % (r["id"], r["score"], r["attr"], r["instr"]), flush=True)
    # the integrity check across the whole breadth sweep:
    fp = sum(1 for r in results if "SILENT-WRONG" in r["attr"])
    print("\n  FALSE PASSES across sweep: %d  (integrity: should be 0)" % fp, flush=True)

if __name__ == "__main__":
    sys.exit(main())
