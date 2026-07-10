"""
osworld_run.py — thin runner that tests the ENTIRE Rust Lagado harness on OSWorld.

Python ONLY boots the env and scores. The actual agent is the Rust `osworld_run` binary, which drives the
guest over its HTTP API (the OsworldPerceptor/OsworldActuator) running the FULL agent_loop (plane-governor +
planes + supervisor). Per task: env.reset (boots guest) → hand the guest's http_server URL to the Rust agent
→ Rust drives the guest end-to-end → env.evaluate().

Run: DOCKER_HOST=unix:///run/podman/podman.sock .venv/bin/python osworld_run.py libreoffice_calc:2 [os:2 ...]
"""
import sys, os, glob, json, subprocess, logging
logging.basicConfig(level=logging.WARNING)
from desktop_env.desktop_env import DesktopEnv

LAGADO = "/home/alucard/projects/lagado"
BIN = os.environ.get("LAGADO_OSWORLD_RUN_BIN", LAGADO + "/target/debug/osworld_run")
LD = LAGADO + "/lagado-agent/vendored/llama.cpp-2/build/bin"
EXDIR = "evaluation_examples/examples"
PER_TASK_TIMEOUT = 240

specs = sys.argv[1:] or ["libreoffice_calc:2"]
plan = []
for s in specs:
    dom, _, n = s.partition(":")
    # `dom:<task-id-prefix>` (non-digit or >=5 chars) targets a specific task; else `dom:<count>`.
    if n and (not n.isdigit() or len(n) >= 5):
        for tf in sorted(glob.glob(f"{EXDIR}/{dom}/{n}*.json")):
            plan.append((dom, tf))
    else:
        for tf in sorted(glob.glob(f"{EXDIR}/{dom}/*.json"))[:int(n or 3)]:
            plan.append((dom, tf))
# RESUME-SAFE campaign mode: per-task results append to LAGADO_RESULTS (jsonl);
# task ids already present are SKIPPED on startup — a crash costs nothing.
RESULTS_JSONL = os.environ.get("LAGADO_RESULTS", "")
done_ids = set()
if RESULTS_JSONL and os.path.exists(RESULTS_JSONL):
    for ln in open(RESULTS_JSONL):
        try:
            done_ids.add(json.loads(ln)["id"])
        except Exception:
            pass
if done_ids:
    plan = [(d, tf) for d, tf in plan
            if json.load(open(tf)).get("id", os.path.basename(tf))[:8] not in done_ids]
    print(f"resume: {len(done_ids)} done, {len(plan)} remaining", flush=True)
print(f"=== WHOLE Lagado HARNESS × OSWorld | {len(plan)} tasks ===", flush=True)

env = DesktopEnv(provider_name="docker", action_space="pyautogui", screen_size=(1920, 1080),
                 headless=True, os_type="Ubuntu", require_a11y_tree=False)
results = []
for dom, tf in plan:
    task = json.load(open(tf)); tid = task.get("id", os.path.basename(tf))[:8]; instr = task.get("instruction", "")
    print(f"\n══════ [{dom}/{tid}] {instr[:90]}", flush=True)
    score, agent_out = 0.0, ""
    try:
        env.reset(task_config=task)
        base_url = env.controller.http_server          # http://<vm_ip>:<server_port>
        r = subprocess.run([BIN, base_url, instr],
                           capture_output=True, text=True, timeout=PER_TASK_TIMEOUT,
                           env={**os.environ, "LD_LIBRARY_PATH": LD})
        agent_out = (r.stdout or "").strip()[-300:]
        if r.returncode != 0:
            agent_out += f"  [rc={r.returncode}] {(r.stderr or '')[-200:]}"
        # With OSW_TRACE, surface the bin's stderr tail (the timing/plane trace, incl.
        # reconciled_via_session=) so we can see which plane handled the task.
        if os.environ.get("OSW_TRACE") and r.stderr:
            for ln in r.stderr.splitlines():
                if any(k in ln for k in ("reconciled_via_session", "native session", "one-shot floor",
                                         "session deploy", "api session", "api_plane", "calc solver")):
                    print(f"   trace: {ln.strip()}", flush=True)
        # The agent may DECLARE infeasibility (model verdict from app truth, never task knowledge).
        # OSWorld's contract for that answer is the literal FAIL action through env.step — an
        # infeasible-func task scores 1 on it, a feasible task scores 0 (a wrong declaration can
        # only lose, never false-pass).
        if "LAGADO_DECLARES: FAIL" in (r.stdout or ""):
            try:
                env.step("FAIL")
            except Exception as e:
                print(f"   (FAIL declaration step error: {e})", flush=True)
        score = env.evaluate() or 0.0
    except subprocess.TimeoutExpired:
        agent_out = "(agent timed out)"
        try: score = env.evaluate() or 0.0
        except Exception: pass
    except Exception as e:
        agent_out = f"(exc {e})"
    print(f"   agent: {agent_out}", flush=True)
    print(f"   ⟹ score={score}", flush=True)
    results.append({"domain": dom, "id": tid, "score": score, "instruction": instr})
    json.dump(results, open("/tmp/osworld_whole_harness.json", "w"), indent=1)
    if RESULTS_JSONL:
        with open(RESULTS_JSONL, "a") as rf:
            rf.write(json.dumps({"domain": dom, "id": tid, "score": score}) + "\n")
    import subprocess as _sp   # per-task volume prune: the known 3.6GB/task leak
    _sp.run("podman volume prune -f", shell=True, capture_output=True)
env.close()

p = sum(1 for r in results if r["score"] >= 1.0)
print(f"\n=== WHOLE HARNESS on OSWorld: {p}/{len(results)} ===", flush=True)
for r in results:
    print(f"   {r['domain']}/{r['id']}  {r['score']:.0f}", flush=True)
