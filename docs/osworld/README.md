> **⚠ HISTORICAL MVP snapshot** (terminal-plane-only, before the GUI/UNO/DOM planes). Setup/env steps may
> still apply; the architecture description is superseded. Current: `docs/osworld/FULL_369_RESULTS_2026-07-10.md` + `HARNESS_WORK_PLAN.md`.

# Running Lagado against the REAL OSWorld benchmark

> **PATHS MOVED (2026-07-03):** the executable tooling formerly in this directory lives at
> `lagado-agent/python/osworld/`; only docs and result artifacts remain here. Read path
> references below with that substitution.

The comparable-to-field-SOTA number (the official OSWorld suite, xlang-ai). Our internal batteries
(`osworld_stress` 11/11 + `osworld_heldout` 8/8 with Qwen) are FILE/SHELL proxies — NOT comparable to
OSWorld (GUI-app heavy). This is the apples-to-apples path.

## Architecture (control inversion)
OSWorld's loop calls `agent.predict(instruction, obs)` and executes the returned action strings on ITS
own Ubuntu guest via `python -c`. We bridge to our Rust harness:

- **`lagado-agent/src/bin/osworld_plan.rs`** — CLI: takes an instruction, runs OUR planner
  (`agent::plan_goal`, now `pub`) on the brain at :8080, prints the decomposition as JSON (each step
  classified command/click/type/key).
- **`docs/osworld/lagado_agent.py`** (deploy to `OSWorld/mm_agents/lagado_agent.py`) — the `LagadoAgent`
  adapter. `predict()` calls the bridge and emits actions.

**Key unlock:** OSWorld's action channel runs ARBITRARY python on the guest (not just pyautogui), so a
"command" step executes directly as `subprocess.run(cmd, shell=True)` — our TERMINAL plane, no GUI
terminal needed, and it counts (OSWorld scores guest END-STATE, not method). This MVP actuates the
terminal plane only; GUI steps (a11y/CV/pixel plane) are flagged but not yet actuated — the per-domain
score (`show_result.py --detailed`) reveals exactly where the terminal carries (`os` domain) vs. where
plane-transition is required (calc/writer/gimp/chrome…). The GUI plane is the next build.

## Environment setup (Fedora, NO host sudo needed)
```bash
# Python 3.12 (OSWorld pyproject requires >=3.12; numpy 1.26 caps at 3.12 — not 3.11, not 3.14)
curl -LsSf https://astral.sh/uv/install.sh | sh
cd ~/projects && git clone https://github.com/xlang-ai/OSWorld && cd OSWorld
uv python install 3.12 && uv venv --python 3.12 .venv && source .venv/bin/activate
uv pip install -r requirements.txt

# Container runtime = rootless podman (NO docker/sudo). docker-py talks to it via DOCKER_HOST.
systemctl --user enable --now podman.socket
export DOCKER_HOST=unix:///run/user/1000/podman/podman.sock
```

### TWO required patches to OSWorld (Fedora rootless-podman gotchas)
1. **SELinux bind-mount** (host is Enforcing): `desktop_env/providers/docker/provider.py` — the qcow2
   volume mount needs an SELinux relabel or the container gets "Permission denied" on `/System.qcow2`
   and falls back to `BOOT=example.com` → "No boot disk specified". Change `"mode": "ro"` →
   `"mode": "ro,z"`.
2. **IPv6 broken in sandbox**: registry pulls fail on IPv6 (`dial tcp [2600:...]`) — works over IPv4.
   (podman picked IPv4 fine after the first retry; no perm change needed.)

The guest = an 11.4 GB Ubuntu qcow2 (downloaded to `OSWorld/docker_vm_data/Ubuntu.qcow2`) booted in a
QEMU-in-container (`happysixd/osworld-docker`, KVM via `/dev/kvm`). RAM: it requests 4 GB
(`provider.py` `RAM_SIZE`); on a 15 GB box, stop the Lagado `:2222` guest first.

## Run
```bash
# brain on :8080 (Qwen2.5-Coder-7B for the ceiling, or LFM2-8B to ship)
# build the bridge:
cd ~/projects/lagado/lagado-agent && cargo build --bin osworld_plan
# deploy the adapter + point run.py at LagadoAgent, then:
cd ~/projects/OSWorld && source .venv/bin/activate
export DOCKER_HOST=unix:///run/user/1000/podman/podman.sock
python quickstart.py --provider_name docker --headless True   # smoke: boots guest, right-clicks, exits
# then a 20-30 task slice (drop the 8 Google-Drive tasks → 361 permitted), pin max_steps=15:
python run.py --provider_name docker --headless --observation_type screenshot_a11y_tree --max_steps 15 ...
python show_result.py --observation_type screenshot_a11y_tree --result_dir ./results --detailed
```

Verified leaderboard is NOT self-submit: schedule a meeting with maintainers, hand them the agent +
writeup, they run it. Local results are ours immediately. See memory `lagado-osworld-real-bench`.
