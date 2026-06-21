"""
run_lagado.py — drive the Lagado harness against real OSWorld tasks and score them.

Boots ONE DesktopEnv (docker provider), then for each task: reset → (predict→step)* → evaluate.
Records per-task world-state score (0/1). Start with the os domain (our terminal home plane).

Usage:
  DOCKER_HOST=unix:///run/podman/podman.sock python run_lagado.py os 5
"""
import sys, os, glob, json, traceback, logging

logging.basicConfig(level=logging.WARNING)
from desktop_env.desktop_env import DesktopEnv
from mm_agents.lagado_agent import LagadoAgent

DOMAIN = sys.argv[1] if len(sys.argv) > 1 else "os"
N = int(sys.argv[2]) if len(sys.argv) > 2 else 5
MAX_STEPS = 15

task_files = sorted(glob.glob(f"evaluation_examples/examples/{DOMAIN}/*.json"))[:N]
print(f"=== Lagado × OSWorld | domain={DOMAIN} | {len(task_files)} tasks | max_steps={MAX_STEPS} ===")

agent = LagadoAgent(observation_type="screenshot_a11y_tree", action_space="pyautogui", max_steps=MAX_STEPS)
env = DesktopEnv(
    provider_name="docker",
    action_space="pyautogui",
    screen_size=(1920, 1080),
    headless=True,
    os_type="Ubuntu",
    require_a11y_tree=True,
)

results = []
for tf in task_files:
    task = json.load(open(tf))
    tid = task.get("id", os.path.basename(tf))[:8]
    instr = task.get("instruction", "")
    print(f"\n══════ [{DOMAIN}/{tid}] {instr[:110]}")
    score = 0.0
    try:
        obs = env.reset(task_config=task)
        agent.reset()
        done = False
        step = 0
        while not done and step < MAX_STEPS:
            response, actions = agent.predict(instr, obs)
            print(f"   step {step+1}: {response[:90]}")
            for action in actions:
                obs, reward, done, info = env.step(action)
                if done:
                    break
            step += 1
        score = env.evaluate()
    except Exception as e:
        print(f"   ERROR: {e}")
        traceback.print_exc()
    print(f"   ⟹ score: {score}")
    results.append((f"{DOMAIN}/{tid}", score))

env.close()
passed = sum(1 for _, s in results if s and s >= 1.0)
print(f"\n══════ RESULT: {passed}/{len(results)} solved (domain={DOMAIN})")
for name, s in results:
    print(f"   {'✅' if s and s>=1.0 else '❌'} {name}: {s}")
