"""session_drive.py — P2c validation #1: drive a calc task through the RUST NativeSession driver
with a HAND op-log (no model), scored by the real env.evaluate().

Python only boots the env + scores; the Rust `session_drive` binary deploys the daemon, applies the
op-log via the SAME NativeSession code wired into agent.rs, and reconciles. This proves the Rust
driver end-to-end against the real guest, isolated from model variability.

Run: DOCKER_HOST=unix:///run/podman/podman.sock PYTHONPATH=/home/alucard/projects/OSWorld \
     /home/alucard/projects/OSWorld/.venv/bin/python session_drive.py <task-id-prefix> [repeat=1]
"""
import glob
import json
import os
import subprocess
import sys

from desktop_env.desktop_env import DesktopEnv

LAGADO = "/home/alucard/projects/lagado"
BIN = os.environ.get("LAGADO_SESSION_DRIVE_BIN", LAGADO + "/target/debug/session_drive")
LD = LAGADO + "/lagado-agent/vendored/llama.cpp-2/build/bin"
EXDIR = "/home/alucard/projects/OSWorld/evaluation_examples/examples/libreoffice_calc"
OPLOGS = LAGADO + "/docs/osworld/oplogs"
PER_TASK_TIMEOUT = 240


def task_input_path(task):
    for c in task.get("config", []):
        if c.get("type") == "open":
            return c["parameters"]["path"]
    for c in task.get("config", []):
        if c.get("type") == "download":
            return c["parameters"]["files"][0]["path"]
    raise SystemExit("no input path in task config")


def main():
    if len(sys.argv) < 2:
        raise SystemExit("usage: session_drive.py <task-id-prefix> [repeat]")
    tid = sys.argv[1]
    repeat = int(sys.argv[2]) if len(sys.argv) > 2 else 1

    tf = sorted(glob.glob(f"{EXDIR}/{tid}*.json"))
    if not tf:
        raise SystemExit(f"no task JSON for {tid}")
    task = json.load(open(tf[0]))
    oplog = f"{OPLOGS}/{tid}.json"
    if not os.path.exists(oplog):
        raise SystemExit(f"no op-log at {oplog}")
    file_path = task_input_path(task)
    print(f"task {task['id'][:8]} | input {file_path} | oplog {oplog} | repeat {repeat}", flush=True)

    env = DesktopEnv(provider_name="docker", action_space="pyautogui", screen_size=(1920, 1080),
                     headless=True, os_type="Ubuntu", require_a11y_tree=False)
    scores = []
    try:
        for run in range(repeat):
            print(f"\n=== run {run + 1}/{repeat} ===", flush=True)
            env.reset(task_config=task)
            base_url = env.controller.http_server  # http://<vm_ip>:<server_port>
            r = subprocess.run([BIN, base_url, file_path, oplog],
                               capture_output=True, text=True, timeout=PER_TASK_TIMEOUT,
                               env={**os.environ, "LD_LIBRARY_PATH": LD})
            print((r.stdout or "").strip(), flush=True)
            if r.returncode != 0:
                print(f"  [rc={r.returncode}] {(r.stderr or '')[-400:]}", flush=True)
            score = env.evaluate() or 0.0
            print(f"  SCORE: {score}", flush=True)
            scores.append(score)
    finally:
        env.close()

    gold = sum(1 for s in scores if s == 1.0)
    print(f"\n==== {task['id'][:8]} : scores={scores} | gold {gold}/{len(scores)} ====", flush=True)
    return 0 if (scores and all(s == 1.0 for s in scores)) else 1


if __name__ == "__main__":
    sys.exit(main())
