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


def _readback_check(cmd: str):
    """For a config-SET command, derive the GET/READ that confirms the value actually took (the
    exit-0-but-wrong guard). Returns (read_cmd, expected_value) or None. Value/key taken from the END so a
    schema with an inline $(...) (spaces) still parses; re-running the same $() re-resolves the UUID."""
    c = cmd.split(";")[-1].strip()
    toks = c.split()
    if "gsettings" in c and "set" in toks:
        rest = toks[toks.index("set") + 1:]
        if len(rest) >= 3:
            value, key, schema = rest[-1], rest[-2], " ".join(rest[:-2])
            return (f"gsettings get {schema} {key}", value.strip("\"'"))
    if "dconf" in c and "write" in toks:
        rest = toks[toks.index("write") + 1:]
        if len(rest) >= 2:
            return (f"dconf read {rest[0]}", " ".join(rest[1:]).strip("\"'"))
    return None


import xml.etree.ElementTree as ET
try:
    from mm_agents.accessibility_tree_wrap.heuristic_retrieve import filter_nodes, component_ns_ubuntu
except Exception:
    filter_nodes, component_ns_ubuntu = None, "https://accessibility.ubuntu.example.org/ns/component"


def _parse_a11y(obs):
    """OSWorld a11y XML → candidate elements [(label, cx, cy)] for the GUI plane. Reuses OSWorld's
    filter_nodes (visible/actionable) + screencoord/size → center. This is the candidate source our
    selection discipline picks from (a11y first; CV/pixel are the later fallback rungs)."""
    xml = obs.get("accessibility_tree") if isinstance(obs, dict) else None
    if not xml or filter_nodes is None:
        return []
    try:
        root = ET.fromstring(xml)
    except Exception:
        return []
    cands = []
    for node in filter_nodes(root, "ubuntu"):
        name = node.get("name", "") or ""
        role = node.tag.split("}")[-1]
        coord = node.get("{%s}screencoord" % component_ns_ubuntu)
        size = node.get("{%s}size" % component_ns_ubuntu)
        if not coord or not size:
            continue
        try:
            x, y = map(int, coord.strip("()").split(", "))
            w, h = map(int, size.strip("()").split(", "))
        except Exception:
            continue
        if w <= 0 or h <= 0:
            continue
        label = f"{role}: {name}".strip()[:80]
        cands.append((label, x + w // 2, y + h // 2))
    return cands


def _rank_for(target: str, cands, cap=40):
    """Rank candidates by token-overlap with the target; keep top `cap` and order so the MOST relevant is
    LAST (late-band — the model attends to the highest-numbered el_N, per selection.rs)."""
    tt = set(re.findall(r"[a-z0-9]+", target.lower()))
    def score(c):
        ct = set(re.findall(r"[a-z0-9]+", c[0].lower()))
        return len(tt & ct)
    ranked = sorted(cands, key=score)        # ascending → best last
    return ranked[-cap:] if len(ranked) > cap else ranked


def _click_target(step):
    """Extract the click/type target text from a planner GUI step."""
    t = step.get("payload", "") if isinstance(step, dict) else str(step)
    return re.sub(r"^(click|type|press|hit)\s+(the\s+)?", "", t.strip(), flags=re.I)


def _running_app_to_reload(cmd: str):
    """A config write (gsettings/dconf) to a GUI app's settings only takes effect on the app's NEXT
    launch — a RUNNING instance has cached the old config (the os/13584542 terminal-size + chrome DNT
    misses). Return the process to restart so the change applies, or None. General-ish: map the gsettings
    schema / dconf path to the app's server process."""
    low = cmd.lower()
    APPS = {
        "terminal": "gnome-terminal-server", "nautilus": "nautilus", "gedit": "gedit",
        "nemo": "nemo", "eog": "eog", "gnome-text-editor": "gnome-text-editor",
    }
    if "gsettings" in low or "dconf" in low:
        for kw, proc in APPS.items():
            if kw in low:
                return proc
    return None


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
        # EFFECT-VERIFY (exit-0-but-wrong): a config-set can "succeed" rc=0 without the value taking (wrong
        # schema/path the model hallucinated). Read it back; mismatch ⇒ ungrounded ⇒ discover→reground.
        if not ungrounded and rc == 0:
            chk = _readback_check(cmd)
            if chk:
                got = self.runner(chk[0]).get("out", "")
                if chk[1] not in got:
                    ungrounded = True
                    err = (err + f" [effect-verify: {chk[0]} -> {got.strip()[:60]!r} != {chk[1]!r}]").strip()
                    print(f"[EFFECT-FAIL] {chk[0]} -> {got.strip()[:60]!r} expected {chk[1]!r}", file=sys.stderr, flush=True)
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
        # GUI plane in progress (iterative — one element pick per OSWorld step)
        if getattr(self, "_mode", None) == "gui":
            return self._gui_step(obs)
        if self._done:
            return "done", ["DONE"]

        plan = _plan_bin(instruction)
        steps = plan.get("steps", [])
        cmds = [s["payload"] for s in steps if s.get("kind") == "command"]
        gui_steps = [s for s in steps if s.get("kind") != "command"]
        self.last_plan = steps                 # per-task map (narrow-in preserved)
        self.last_category = None

        # CLI plane FIRST (our home base) — run any command steps via the guest runner
        if cmds and self.runner is not None:
            log = []
            for c in cmds:
                self._run_grounded(instruction, "cd ~/Desktop 2>/dev/null || cd ~; " + c, log)
            self.last_category = "CMD_RAN"
            self.last_trace = "discover-then-operate | " + " | ".join(log)
            self.thoughts.append(self.last_trace[:400])
            if not gui_steps:
                self._done = True
                return self.last_trace[:400], ["DONE"]
            # mixed plan → fall through to the GUI plane for the remaining steps (the SWITCH)
        elif cmds and self.runner is None:
            self._done = True
            return f"terminal one-shot: {len(cmds)} cmd(s)", [_guest_command_action(c) for c in cmds] + ["DONE"]

        # ── SWITCH TRIGGER → GUI PLANE (reactive a11y loop; CV/pixel are later fallback rungs) ──
        if gui_steps:
            self.last_category = "GUI_NEEDED"
            self._instruction = instruction
            self._mode = "gui"
            self._gui_count = 0
            self._stuck = 0
            self._last_hash = None
            self._gui_log = []
            return self._gui_step(obs)

        self._done = True
        return "no actionable plan", ["FAIL"]

    def _gui_step(self, obs):
        """REACTIVE GUI step (R7): read the LIVE a11y tree → pick the ONE next element toward the GOAL
        (grammar-constrained el_N | done | none) → click. SETTLE/no-progress: if the candidate set is
        unchanged for several steps (a click had no effect — e.g. a menu didn't open) → stop. Fail-closed:
        `none` ⇒ WAIT (re-observe). a11y is rung 1; CV/pixel fallback when a11y is empty is the next build."""
        import sys
        MAX_GUI, STUCK_LIMIT = 12, 3
        if self._gui_count >= MAX_GUI:
            self._mode = "done"; self._done = True
            self.last_trace = "gui (reactive) | " + " | ".join(self._gui_log)[:400]
            return "gui step budget reached", ["DONE"]
        self._gui_count += 1

        cands = _parse_a11y(obs)
        chash = hash(tuple(sorted(c[0] for c in cands)))
        if chash == self._last_hash:
            self._stuck += 1
        else:
            self._stuck = 0
            self._last_hash = chash
        if self._stuck >= STUCK_LIMIT:                 # clicks aren't changing the screen → give up cleanly
            self._mode = "done"; self._done = True
            print(f"[GUI] no-progress x{self._stuck} → stop", file=sys.stderr, flush=True)
            self.last_trace = "gui (reactive, stuck) | " + " | ".join(self._gui_log)[:400]
            return "gui no-progress", ["DONE"]
        if not cands:
            print("[GUI] no a11y candidates → WAIT (CV/pixel fallback TBD)", file=sys.stderr, flush=True)
            return "gui: no a11y elements", ["WAIT"]

        ranked = _rank_for(self._instruction, cands, cap=50)
        labels = [c[0] for c in ranked]
        res = _plan_bin("--next", self._instruction, *labels)
        tok, idx = res.get("token", "none"), res.get("index", -1)
        if tok == "done":
            self._mode = "done"; self._done = True
            print("[GUI] model says done", file=sys.stderr, flush=True)
            self.last_trace = "gui (reactive, done) | " + " | ".join(self._gui_log)[:400]
            return "gui done", ["DONE"]
        if tok == "none" or idx is None or idx < 0 or idx >= len(ranked):
            print(f"[GUI] none among {len(ranked)} → WAIT", file=sys.stderr, flush=True)
            return "gui: none (settle)", ["WAIT"]
        label, cx, cy = ranked[idx]
        self._gui_log.append(f"el_{idx} {label} @({cx},{cy})")
        print(f"[GUI] step {self._gui_count}: el_{idx} '{label}' @({cx},{cy})", file=sys.stderr, flush=True)
        return f"gui click '{label}' @({cx},{cy})", [f"pyautogui.click({cx}, {cy})"]
