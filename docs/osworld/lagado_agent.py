"""
LagadoAgent — the OSWorld adapter for the Lagado harness (CLI/terminal control plane).

DISCOVER-THEN-OPERATE (iterative ReAct): the planner (Rust `osworld_plan` → our brain) decomposes the
task into command steps; the adapter RUNS each on the guest via a `runner` (wired to the OSWorld
controller). When a command fails with an UNGROUNDING signal (e.g. `No such schema` — the model assumed a
config identifier that's false on THIS machine), the harness runs DETERMINISTIC discovery (introspect the
real schemas/paths/UUIDs), then RE-GROUNDS the command with those facts (`osworld_plan --reground`) and
retries. This is the anti-hallucination mechanism: the terminal plane introspects itself instead of
one-shot guessing. OSWorld scores guest END-STATE, so running via the runner counts.

If no runner is wired (legacy/one-shot), falls back to emitting command actions through env.step.
"""
import json, logging, os, re, subprocess

logger = logging.getLogger("desktopenv.agent")

OSWORLD_PLAN_BIN = os.environ.get(
    "LAGADO_OSWORLD_PLAN_BIN", "/home/alucard/projects/lagado/target/debug/osworld_plan")

# stderr fragments that mean "the command assumed a fact that's false on this machine" → discover+reground
UNGROUNDED = ("no such schema", "no such file", "no such directory", "not found",
              "command not found", "does not exist", "unrecognized", "is not a valid")


def _plan_bin(*args):
    out = subprocess.run([OSWORLD_PLAN_BIN, *args], capture_output=True, text=True, timeout=120)
    return json.loads(out.stdout.strip().splitlines()[-1])


def _guest_command_action(cmd: str) -> str:
    """Legacy one-shot path: a guest action string that runs a shell command from ~/Desktop (OSWorld's
    working surface) so the planner's relative names resolve; absolute/~ paths unaffected."""
    grounded = "cd ~/Desktop 2>/dev/null || cd ~; " + cmd
    return ("import subprocess as _sp; "
            f"_r = _sp.run({grounded!r}, shell=True, capture_output=True, text=True); "
            "print(_r.stdout); print(_r.stderr)")


def _discovery_probes(cmd: str, err: str):
    """DETERMINISTIC discovery for a failed command — introspect the ACTUAL system facts the model got
    wrong. General over the common ungrounding classes (gsettings/dconf config, missing file, missing
    command); generic --help fallback otherwise."""
    low = (cmd + " " + err).lower()
    probes = []
    if "gsettings" in cmd or "dconf" in cmd or "schema" in low:
        m = re.search(r'org\.gnome\.([A-Za-z]+)', cmd)
        kw = (m.group(1) if m else "").lower() or "settings"
        probes.append(f"gsettings list-schemas 2>/dev/null | grep -iE '{kw}'")
        if "terminal" in low:
            probes.append("echo default-profile-uuid: $(gsettings get org.gnome.Terminal.ProfilesList default 2>/dev/null)")
            probes.append("dconf dump /org/gnome/terminal/ 2>/dev/null | head -20")
        else:
            probes.append(f"dconf dump / 2>/dev/null | grep -iE '{kw}' | head -20")
    if "no such file" in low or "no such directory" in low:
        for tok in cmd.replace("'", " ").replace('"', " ").split():
            if "/" in tok:
                base = os.path.basename(tok.rstrip("/")) or tok
                probes.append(f"find ~ -iname {base!r} 2>/dev/null | head")
                break
    if "command not found" in low or " not found" in low:
        first = cmd.split()[0] if cmd.split() else ""
        if first:
            probes.append(f"compgen -c 2>/dev/null | grep -i {first} | head; apt-cache search {first} 2>/dev/null | head -3")
    if not probes:
        first = cmd.split()[0] if cmd.split() else "true"
        probes.append(f"{first} --help 2>&1 | head -15")
    return probes


class LagadoAgent:
    def __init__(self, observation_type="screenshot_a11y_tree", action_space="pyautogui",
                 max_steps=15, **kwargs):
        self.observation_type = observation_type
        self.action_space = action_space
        self.max_steps = max_steps
        self.runner = None          # set by the runner: runner(cmd) -> {"out","err","rc"}
        self._done = False
        self.actions, self.observations, self.thoughts = [], [], []

    def reset(self, runtime_logger=None):
        self._done = False
        self.actions, self.observations, self.thoughts = [], [], []

    # ── the discover-then-operate execution of ONE command on the guest ──────────────────────────────
    def _run_grounded(self, instruction, cmd, log):
        import sys
        res = self.runner(cmd)
        out, err, rc = res.get("out", ""), res.get("err", ""), res.get("rc", 0)
        log.append(f"$ {cmd}\n  rc={rc} {('err='+err.strip()[:80]) if err.strip() else 'ok'}")
        print(f"[CMD] {cmd}\n  rc={rc} out={out.strip()[:120]!r} err={err.strip()[:160]!r}", file=sys.stderr, flush=True)
        ungrounded = rc != 0 and any(s in err.lower() for s in UNGROUNDED)
        if not ungrounded:
            return rc == 0
        # DISCOVER → REGROUND → retry (bounded to one round)
        probes = _discovery_probes(cmd, err)
        discovery = "\n".join(f"$ {p}\n{self.runner(p).get('out','').strip()}" for p in probes)
        log.append(f"  ↳ discover:\n{discovery[:300]}")
        try:
            corrected = _plan_bin("--reground", instruction, cmd, err.strip()[:200], discovery[:1200]).get("command", "")
        except Exception as e:
            log.append(f"  reground failed: {e}")
            return False
        if not corrected or corrected == cmd:
            return False
        res2 = self.runner(corrected)
        import sys
        print(f"[DISCOVER]\n{discovery[:500]}\n[REGROUND] {corrected}\n  rc={res2.get('rc')} err={res2.get('err','').strip()[:160]!r}", file=sys.stderr, flush=True)
        log.append(f"  ↳ regrounded: $ {corrected}\n  rc={res2.get('rc')} {res2.get('err','').strip()[:80]}")
        return res2.get("rc", 1) == 0

    def predict(self, instruction, obs):
        if self._done:
            return "done", ["DONE"]

        plan = _plan_bin(instruction)
        steps = plan.get("steps", [])
        cmds = [s["payload"] for s in steps if s.get("kind") == "command"]
        gui = sum(1 for s in steps if s.get("kind") != "command")
        self._done = True

        if not cmds and gui:
            return f"plan needs GUI plane ({gui} non-command steps)", ["FAIL"]

        if self.runner is None:
            # legacy one-shot: emit command actions through env.step
            return f"terminal one-shot: {len(cmds)} cmd(s)", [_guest_command_action(c) for c in cmds] + ["DONE"]

        # ITERATIVE discover-then-operate via the guest runner
        log = []
        # run from the OSWorld working surface so relative names resolve
        self.runner("cd() { builtin cd \"$@\"; }; :")  # noop warm-up
        for c in cmds:
            self._run_grounded(instruction, "cd ~/Desktop 2>/dev/null || cd ~; " + c, log)
        resp = "discover-then-operate | " + " | ".join(log)[:400]
        self.thoughts.append(resp)
        return resp, ["DONE"]
