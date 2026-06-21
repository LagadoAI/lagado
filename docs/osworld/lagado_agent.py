"""
LagadoAgent — the OSWorld adapter for the Lagado harness.

Control inversion: OSWorld's loop calls `predict(instruction, obs)` and executes the returned action
strings on the guest via `python -c`. We bridge to the Rust harness's planner (`osworld_plan` bin, which
runs OUR decomposition on the brain at :8080) and emit actions.

MVP = the TERMINAL plane (our proven home): OSWorld's action channel runs ARBITRARY python on the guest,
so a "command" step executes directly as `subprocess.run(cmd, shell=True)` — no GUI terminal needed, and
it counts because OSWorld scores the guest END-STATE, not the method. GUI steps (a11y/CV/pixel plane) are
flagged but not yet actuated here — the per-domain score reveals exactly where the terminal carries vs.
where plane-transition is required (the home/away map). That GUI plane is the next build.

Contract matched to mm_agents/agent.py: `action_space`, `observation_type`, `reset()`, `predict()`.
"""
import json
import logging
import os
import subprocess

logger = logging.getLogger("desktopenv.agent")

# the compiled Rust bridge that runs OUR planner/decomposition against the brain on :8080
OSWORLD_PLAN_BIN = os.environ.get(
    "LAGADO_OSWORLD_PLAN_BIN",
    "/home/alucard/projects/lagado/target/debug/osworld_plan",
)


def _guest_command_action(cmd: str) -> str:
    """A guest action string that runs a shell command directly (the terminal plane). Wrapped by
    OSWorld's pkgs_prefix as `python -c "...; {this}"`, so it must be valid python. repr() keeps the
    command intact through the python -c quoting.

    WORKING-DIRECTORY GROUNDING: OSWorld places user files/folders on the Desktop (the GUI working
    surface; user='user', home=/home/user), but our subprocess runs in the server's cwd. So the planner's
    relative names ('photos', 'cpjpg') don't resolve → file ops silently match nothing (the os/23393935
    miss). Run from ~/Desktop (fall back to ~), so relative names resolve; absolute/~ paths are unaffected."""
    grounded = "cd ~/Desktop 2>/dev/null || cd ~; " + cmd
    return (
        "import subprocess as _sp; "
        f"_r = _sp.run({grounded!r}, shell=True, capture_output=True, text=True); "
        "print(_r.stdout); print(_r.stderr)"
    )


class LagadoAgent:
    def __init__(
        self,
        observation_type: str = "screenshot_a11y_tree",
        action_space: str = "pyautogui",
        max_steps: int = 15,
        **kwargs,
    ):
        self.observation_type = observation_type
        self.action_space = action_space
        self.max_steps = max_steps
        # per-episode state
        self._plan = None          # cached decomposition for the current instruction
        self._emitted = False      # we return the whole terminal script in one predict()
        self.actions = []
        self.observations = []
        self.thoughts = []

    def reset(self, runtime_logger=None):
        self._plan = None
        self._emitted = False
        self.actions = []
        self.observations = []
        self.thoughts = []

    def _plan_goal(self, instruction: str):
        """Call the Rust bridge → OUR planner decomposes the OSWorld instruction."""
        try:
            out = subprocess.run(
                [OSWORLD_PLAN_BIN, instruction],
                capture_output=True, text=True, timeout=120,
            )
            data = json.loads(out.stdout.strip().splitlines()[-1])
            return data
        except Exception as e:
            logger.error("osworld_plan bridge failed: %s", e)
            return {"steps": [], "n": 0, "all_command": False}

    def predict(self, instruction: str, obs: dict):
        """Return (response, actions). MVP: decompose once, run command steps via the guest's python
        channel, end with DONE. GUI steps are surfaced in the response but not yet actuated."""
        if self._emitted:
            return "done", ["DONE"]

        if self._plan is None:
            self._plan = self._plan_goal(instruction)
        steps = self._plan.get("steps", [])

        actions = []
        gui_unhandled = 0
        for s in steps:
            if s.get("kind") == "command":
                actions.append(_guest_command_action(s["payload"]))
            else:
                # GUI plane (a11y/CV/pixel) not yet actuated in the MVP — count it for the home/away map.
                gui_unhandled += 1

        self._emitted = True
        if not actions and gui_unhandled:
            # nothing we can do from the terminal — honest FAIL (this task needs the GUI plane)
            response = f"plan needs GUI plane ({gui_unhandled} non-command steps); terminal MVP cannot actuate"
            return response, ["FAIL"]

        actions.append("DONE")
        response = (
            f"terminal-plane plan: {len(actions)-1} command step(s)"
            + (f", {gui_unhandled} GUI step(s) skipped" if gui_unhandled else "")
        )
        self.actions.extend(actions)
        self.thoughts.append(response)
        return response, actions
