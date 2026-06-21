"""
run_lagado.py — drive the Lagado harness across real OSWorld tasks and produce a NARROW-IN-PRESERVING
per-domain map. For each task it records: instruction, plan (command vs GUI plane), the full discover-
then-operate trace, the score, and an auto-CATEGORY. Saved incrementally to JSON so the broad run yields
a PRE-SORTED failure map (no re-running to diagnose).

Usage:
  DOCKER_HOST=unix:///run/podman/podman.sock python run_lagado.py os:4 calc:3 gimp:3 chrome:3
  (a bare domain name defaults to 3 tasks)
"""
import sys, os, glob, json, traceback, logging, time
logging.basicConfig(level=logging.WARNING)
from desktop_env.desktop_env import DesktopEnv
from mm_agents.lagado_agent import LagadoAgent

MAX_STEPS = 15
RESULTS = "/tmp/osworld_broad_results.json"

specs = sys.argv[1:] or ["os:3"]
plan = []
for s in specs:
    dom, _, n = s.partition(":")
    for tf in sorted(glob.glob(f"evaluation_examples/examples/{dom}/*.json"))[:int(n or 3)]:
        plan.append((dom, tf))
print(f"=== Lagado × OSWorld broad map | {len(plan)} tasks across {len(specs)} domains ===", flush=True)

agent = LagadoAgent()
env = DesktopEnv(provider_name="docker", action_space="pyautogui", screen_size=(1920, 1080),
                 headless=True, os_type="Ubuntu", require_a11y_tree=True)

def _runner(cmd):
    py = ("import subprocess as _s, json as _j; r=_s.run(%r, shell=True, capture_output=True, text=True); "
          "print(_j.dumps({'out': r.stdout, 'err': r.stderr, 'rc': r.returncode}))" % cmd)
    res = env.controller.execute_python_command(py)
    raw = res.get("output", "") if isinstance(res, dict) else str(res)
    try:
        return json.loads(raw.strip().splitlines()[-1])
    except Exception:
        return {"out": raw, "err": "", "rc": 0}
agent.runner = _runner

results = []
for dom, tf in plan:
    task = json.load(open(tf))
    tid = task.get("id", os.path.basename(tf))[:8]
    instr = task.get("instruction", "")
    print(f"\n══════ [{dom}/{tid}] {instr[:100]}", flush=True)
    score, category, trace, kinds = 0.0, "ERROR", "", []
    try:
        obs = env.reset(task_config=task)
        agent.reset()
        done, step = False, 0
        while not done and step < MAX_STEPS:
            response, actions = agent.predict(instr, obs)
            for action in actions:
                obs, reward, done, info = env.step(action)
                if done:
                    break
            step += 1
        score = env.evaluate() or 0.0
        kinds = [s.get("kind") for s in getattr(agent, "last_plan", [])]
        trace = getattr(agent, "last_trace", "")
        cat = getattr(agent, "last_category", None)
        if score and score >= 1.0:
            category = "PASS"
        elif cat == "GUI_NEEDED":
            category = "GUI_NEEDED"          # away plane: needs a11y/CV/pixel actuation
        elif cat == "CMD_RAN":
            category = "CMD_WRONG"           # terminal plane ran but didn't achieve goal
        else:
            category = "OTHER"
    except Exception as e:
        trace = f"{e}\n{traceback.format_exc()[:300]}"
        category = "EXC"
    print(f"   ⟹ {category}  score={score}  plan={kinds}", flush=True)
    results.append({"domain": dom, "id": tid, "instruction": instr, "score": score,
                    "category": category, "plan_kinds": kinds, "trace": trace[:1500]})
    json.dump(results, open(RESULTS, "w"), indent=1)   # incremental save

env.close()

# ── the map ──
import collections
by_dom = collections.defaultdict(lambda: [0, 0])
cats = collections.Counter()
for r in results:
    by_dom[r["domain"]][0] += 1 if r["category"] == "PASS" else 0
    by_dom[r["domain"]][1] += 1
    cats[r["category"]] += 1
print("\n══════ PER-DOMAIN (home/away map) ══════")
for dom, (p, n) in sorted(by_dom.items()):
    print(f"   {dom:10s} {p}/{n}")
print("══════ FAILURE CATEGORIES ══════")
for c, n in cats.most_common():
    print(f"   {c:12s} {n}")
print(f"\nfull per-task map (instruction+plan+trace+category) → {RESULTS}")
