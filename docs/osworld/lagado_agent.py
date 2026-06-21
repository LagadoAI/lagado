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

# R1b's per-app reload table was the fragile patch (#3): every app needs a row. REMOVED — the universal
# response to "CLI config-set didn't achieve the goal" is R1c: do it through the app's own UI (GUI plane),
# which applies the change the way the app expects. An app-specific reload could return later as explicit
# OPTIONAL sugar, never as the foundation.


def _plan_bin(*args):
    out = subprocess.run([OSWORLD_PLAN_BIN, *args], capture_output=True, text=True, timeout=120)
    return json.loads(out.stdout.strip().splitlines()[-1])


# desktop shell + a11y/input-method daemons — a11y "applications" that are never the foreground app
# (optional sugar on top of the universal "most UI elements" floor).
_APP_SKIP = ("gnome-shell", "cinnamon", "muffin", "mutter", "plasmashell", "xfdesktop", "desktop", "panel",
             "ibus", "ibus-x11", "at-spi", "at-spi2-registryd", "gsd-xsettings", "xfsettingsd")


def _focused_app_node(root):
    """The foreground application <node> — UNIVERSAL floor: the app with the most UI elements. Scoping
    perception to this node's subtree is the general way to ignore OTHER apps' chrome (the dock/panel belong
    to gnome-shell/cinnamon, a different app), replacing the `cx<60` dock pixel-patch (#2). Returns the node
    or None."""
    best, best_n = None, -1
    for n in root.iter():
        if n.tag.split("}")[-1].lower() == "application":
            nm = (n.get("name", "") or "").strip().lower()
            if not nm or nm in _APP_SKIP or any(s in nm for s in ("ibus", "at-spi")):
                continue
            cnt = sum(1 for _ in n.iter())          # foreground app = most UI elements
            if cnt > best_n:
                best, best_n = n, cnt
    return best


def _app_name(obs):
    """Foreground application name (e.g. 'gimp', 'soffice') for the knowledge-frame menu planner."""
    xml = obs.get("accessibility_tree") if isinstance(obs, dict) else None
    if not xml:
        return ""
    try:
        root = ET.fromstring(xml)
    except Exception:
        return ""
    node = _focused_app_node(root)
    return (node.get("name", "") or "").strip() if node is not None else ""


def _modal_present(obs):
    """Universal blocking-modal detector (#1): AT-SPI marks a BLOCKING dialog with state modal=true (a
    non-modal dockable panel does NOT) — the precise, app-agnostic signal, no button-name vocabulary. The
    dialog container is filtered out of the actionable candidates, so scan the raw focused-app subtree."""
    xml = obs.get("accessibility_tree") if isinstance(obs, dict) else None
    if not xml:
        return False
    try:
        root = ET.fromstring(xml)
    except Exception:
        return False
    app = _focused_app_node(root)
    scope = app if app is not None else root
    for n in scope.iter():
        for k, v in n.attrib.items():
            if k.split("}")[-1].lower() == "modal" and str(v).strip().lower() == "true":
                return True
    return False


def _region_sig(obs, region):
    """Harness-derived signature of a screen region across senses — the a11y label-SET plus a noise-robust
    coarse grayscale grid of the pixels. region = (x0,x1,y0,y1) or None for the focused app. This is the
    independent observation the change-at-locus verifier reconciles against; the MODEL is never consulted."""
    a11y = _parse_a11y(obs)
    if region is None:
        labels = frozenset(c[0] for c in a11y)
    else:
        x0, x1, y0, y1 = region
        labels = frozenset(c[0] for c in a11y if x0 <= c[1] <= x1 and y0 <= c[2] <= y1)
    arr = None
    shot = obs.get("screenshot") if isinstance(obs, dict) else None
    if shot:
        try:
            import io, numpy as np
            from PIL import Image
            img = np.asarray(Image.open(io.BytesIO(shot)).convert("L"), dtype=np.int16)
            if region is not None:
                h, w = img.shape
                img = img[max(0, y0):min(h, y1), max(0, x0):min(w, x1)]
            if img.size:
                from PIL import Image as _I
                arr = np.asarray(_I.fromarray(img.astype("uint8")).resize((32, 32)), dtype=np.int16)  # coarse → ignore 1px noise
        except Exception:
            arr = None
    return (labels, arr)


def _action_locus(kind, cx, cy):
    """The region to watch for an action's effect — DERIVED FROM THE ACTION ALONE (no model). A menu-open's
    effect appears below the clicked item; a hover's flyout appears to its right; a leaf-click/type can land
    anywhere (dialog/doc) → watch the whole app. Offsets are anchored to the action's coords, not the screen."""
    if kind == "open":
        return (cx - 150, cx + 360, cy + 5, 900)
    if kind == "hover":
        return (cx - 40, cx + 460, cy - 120, cy + 420)
    return None


def _effect_landed(pre, post):
    """Did the locus CHANGE between the pre- and post-action observations? The unfakeable floor — 'a change
    in the spot we expect' — reconciled from observation, not the model's say-so. Structural (a11y set) OR
    a meaningful pixel delta (>3% of the coarse grid shifted notably; robust to cursor/antialias noise)."""
    if pre[0] != post[0]:
        return True
    a, b = pre[1], post[1]
    if a is not None and b is not None and a.shape == b.shape:
        import numpy as np
        return float((np.abs(a - b) > 20).mean()) > 0.03
    return False


def _match_token(token, cands):
    """Best candidate whose label matches a menu-path TOKEN (deterministic, no model). Exact substring wins;
    else most word-overlap (≥1 word). Returns (label, cx, cy) or None. Used to FOLLOW a planned menu path on
    screen — the screen verifies the model's path; no match ⇒ caller fails closed."""
    tl = token.lower().strip()
    for c in cands:                         # exact substring (handles 'menu: Layer' ⊇ 'layer')
        if tl and tl in c[0].lower():
            return c
    tw = set(re.findall(r"[a-z0-9]+", tl))
    best, bestn = None, 0
    for c in cands:
        cw = set(re.findall(r"[a-z0-9]+", c[0].lower()))
        n = len(tw & cw)
        if n > bestn:
            best, bestn = c, n
    return best if bestn >= 1 else None


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
    # #2 — scope to the FOCUSED app's subtree: the dock/panel are a DIFFERENT app (gnome-shell/cinnamon), so
    # they fall away universally — no pixel-geometry rule. (Fallback to the whole tree on an early frame with
    # no app node yet.)
    scope = _focused_app_node(root)
    if scope is None:
        scope = root
    cands = []
    for node in filter_nodes(scope, "ubuntu"):
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


_OCR = None
def _ocr_candidates(obs):
    """CV plane (R7c): OCR the OSWorld screenshot → text candidates [(label, cx, cy)] with pixel centers.
    Used when a11y is BLIND to a real on-screen target (native-app menus — F9). easyocr, CPU, lazy-init."""
    global _OCR
    shot = obs.get("screenshot") if isinstance(obs, dict) else None
    if not shot:
        return []
    try:
        import io, numpy as np
        from PIL import Image
        img = np.array(Image.open(io.BytesIO(shot)).convert("RGB"))
        if _OCR is None:
            import easyocr
            _OCR = easyocr.Reader(["en"], gpu=False, verbose=False)
        cands = []
        for bbox, text, conf in _OCR.readtext(img):
            if conf < 0.4 or not str(text).strip():
                continue
            xs = [p[0] for p in bbox]; ys = [p[1] for p in bbox]
            cands.append((f"text: {str(text).strip()}"[:80], int(sum(xs) / 4), int(sum(ys) / 4)))
        return cands
    except Exception as e:
        import sys; print(f"[OCR] failed: {e}", file=sys.stderr, flush=True)
        return []


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

    def _enter_gui(self, instruction, obs):
        """Enter the GUI plane (fresh state). Used both for a planned GUI step and for the OUTCOME-DRIVEN
        switch (R1c): CLI ran but the goal-verify says UNMET → switch to GUI even with no planned gui_steps."""
        self.last_category = "GUI_NEEDED"
        self._instruction = instruction
        self._mode = "gui"
        self._gui_count = 0
        self._stuck = 0
        self._last_hash = None
        self._gui_log = []
        self._menu_path = None
        self._path_planned = False
        self._path_idx = 0
        self._anchor_x = None
        self._path_tries = 0
        self._last_pick = None
        self._pending = None        # change-at-locus verification of the last action
        self._verify_tries = 0
        return self._gui_step(obs)

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
            # ── GOAL-LEVEL verify: the SAFE source is the harness-derived read-back (`_readback_check`, run
            # inside `_run_grounded`) — deterministic and guaranteed read-only because the HARNESS builds the
            # `gsettings get`/`dconf read`, not the model. Brain-authored shell verify was REJECTED here: it is
            # both noisy (~0/3 semantically right) AND unsafe (the model ignores 'read-only' and writes checks
            # containing cp/gzip/rm → running them mutates the scored end-state). So R1c (the outcome-driven
            # CLI→GUI switch) stays GATED on a trustworthy+safe goal-verify — the real bottleneck of the spine,
            # which a brain-written shell command cannot be. ──
            self.last_category = "CMD_RAN"
            self.last_trace = "discover-then-operate | " + " | ".join(log)
            self.thoughts.append(self.last_trace[:400])
            if not gui_steps:
                self._done = True
                return self.last_trace[:400], ["DONE"]
            # mixed plan → fall through to the GUI plane for the remaining steps
        elif cmds and self.runner is None:
            self._done = True
            return f"terminal one-shot: {len(cmds)} cmd(s)", [_guest_command_action(c) for c in cmds] + ["DONE"]

        # ── SWITCH TRIGGER → GUI PLANE (reactive a11y loop; CV/pixel are later fallback rungs) ──
        if gui_steps:
            return self._enter_gui(instruction, obs)

        self._done = True
        return "no actionable plan", ["FAIL"]

    def _next_pick(self, obs_or_cands):
        """Run the reactive selection (--next: el_N | done | none) over a candidate set. Returns 'done',
        a (label, cx, cy) tuple, or None (no useful pick)."""
        cands = obs_or_cands
        if not cands:
            return None
        ranked = _rank_for(self._instruction, cands, cap=50)
        res = _plan_bin("--next", self._instruction, *[c[0] for c in ranked])
        tok, idx = res.get("token", "none"), res.get("index", -1)
        if tok == "done":
            return "done"
        if tok == "none" or idx is None or idx < 0 or idx >= len(ranked):
            return None
        return ranked[idx]

    def _plan_menu_path(self, app):
        """Knowledge-frame menu path (F13): the brain names the right menu only when asked 'in <app>, what is
        the menu PATH' (Layer > Transparency > …, 5/5) — NOT 'which menu matches the goal' (lexically mis-picks
        Image, 9/9), and NOT grounded in the menu bar (listing 'Image' re-primes the mis-pick, 5/5 wrong)."""
        try:
            res = _plan_bin("--menupath", self._instruction, app or "this application")
            return [t for t in res.get("path", []) if t]
        except Exception:
            return []

    def _gui_step(self, obs):
        """GUI step. MENU tasks → plan the menu PATH once (knowledge frame), then FOLLOW it deterministically:
        the menubar token CLICK-opens its dropdown, each submenu-PARENT token HOVER-opens its flyout (GTK opens
        on dwell, not click), the LAST token CLICKs the leaf. Tokens are matched on screen — a11y for the
        menubar, region-clipped CV (OCR) for the a11y-blind menu items — so red-herrings off the open menu
        can't be picked. A token that never appears fails CLOSED → the reactive a11y→CV ladder. Non-menu tasks
        use the ladder directly. (Fixes F13's two walls: lexical mis-pick of the top menu + CV pollution.)"""
        import sys
        MAX_GUI, STUCK_LIMIT = 16, 4
        if self._gui_count >= MAX_GUI:
            self._mode = "done"; self._done = True
            self.last_trace = "gui | " + " | ".join(self._gui_log)[:400]
            return "gui step budget", ["DONE"]
        self._gui_count += 1
        last = getattr(self, "_last_pick", None)
        a11y = _parse_a11y(obs)

        # rung 0 — CLEAR THE WAY, UNIVERSALLY (#1): a BLOCKING modal (state modal=true — GIMP 'Convert to RGB?',
        # a leaf-activated 'Convert to Indexed', any native confirm) GRABS input → activate its DEFAULT action
        # with ENTER (proven to clear it). No button-name vocabulary, no coordinates — every native dialog
        # honours its default button on Enter. If Enter doesn't clear it after 2 tries, fall through: the
        # dialog's buttons are in a11y, so the path/selection plane picks one (also universal).
        if _modal_present(obs):
            self._enter_tries = getattr(self, "_enter_tries", 0) + 1
            if self._enter_tries <= 2:
                self._gui_log.append(f"[modal] Enter (default) try {self._enter_tries}")
                print(f"[GUI][modal] modal=true → Enter (default action) try {self._enter_tries}", file=sys.stderr, flush=True)
                return "gui[modal] Enter", ["pyautogui.press('enter')"]
            # Enter didn't clear it → fall through; the path/selection plane will choose a dialog button
        else:
            self._enter_tries = 0

        # ── plan the menu PATH once (knowledge frame), as soon as the MENU BAR is visible ──
        # Lock ONLY after we've seen a populated menu bar — else an init-race (a11y not ready / modal still
        # covering the bar on step 1) plans an EMPTY path and locks reactive forever (the F13-path bug).
        if not getattr(self, "_path_planned", False):
            menubar = [c[0].split(":", 1)[1].strip() for c in a11y if c[0].lower().startswith("menu:")]
            # require the REAL app menu bar (≥3 menus) — a lone 'System' panel menu on an early frame is not
            # it (the app's a11y tree populates a step or two after the window appears).
            if len(menubar) >= 3:
                app = _app_name(obs)
                path = self._plan_menu_path(app)
                # accept only if the first token is an actual menu-bar menu (else not a menu task → reactive)
                if path and _match_token(path[0], [(m, 0, 0) for m in menubar]):
                    self._menu_path = path; self._path_idx = 0; self._anchor_x = None
                    self._gui_log.append(f"[path] {' > '.join(path)}")
                    print(f"[GUI][path] planned: {' > '.join(path)}", file=sys.stderr, flush=True)
                else:
                    self._menu_path = []
                self._path_planned = True
            # menu bar not visible yet → don't lock; settle and retry planning next step (bounded — if the
            # bar never shows in a11y, give up planning and use the reactive ladder)
            else:
                self._menubar_waits = getattr(self, "_menubar_waits", 0) + 1
                if self._menubar_waits <= 3:
                    return "gui await-menubar", ["WAIT"]
                self._menu_path = []; self._path_planned = True

        # ── FOLLOW the planned path, CONFIRM-GATED (change-at-locus floor) ──
        path = getattr(self, "_menu_path", None) or []
        if path:
            # (1) RECONCILE the previous action: did its effect land at the harness-derived locus? The model is
            # never asked 'did it work' — the harness observes the spot the action should have changed. Advance
            # only on a confirmed change; no change after a beat = the action did nothing → fail closed.
            pend = getattr(self, "_pending", None)
            if pend is not None:
                if _effect_landed(pend["pre"], _region_sig(obs, pend["locus"])):
                    self._path_idx += 1; self._pending = None; self._verify_tries = 0
                    self._gui_log.append(f"[verify] {pend['label']} landed")
                    print(f"[GUI][verify] '{pend['label']}' effect LANDED at locus → advance", file=sys.stderr, flush=True)
                else:
                    self._verify_tries = getattr(self, "_verify_tries", 0) + 1
                    if self._verify_tries == 1:                 # maybe a slow effect → re-observe once
                        print(f"[GUI][verify] '{pend['label']}' no effect (try 1) → settle", file=sys.stderr, flush=True)
                        return "gui verify-wait", ["WAIT"]
                    if self._verify_tries == 2:                 # rule out a transient MISS → re-attempt below
                        print(f"[GUI][verify] '{pend['label']}' still no effect (try 2) → re-attempt", file=sys.stderr, flush=True)
                        self._pending = None                    # act-logic re-emits the SAME (un-advanced) idx
                    else:
                        # persistent no-op after settle + re-attempt. For a LEAF that means the operation is
                        # ALREADY SATISFIED (e.g. greyed 'Add Alpha Channel' when alpha already exists) — audit
                        # logic: a transaction netting to zero is already reconciled, not an error → SKIP, don't
                        # fail. For a navigation HOP, no submenu = a real nav failure → fail-closed.
                        if self._path_idx == len(path) - 1:
                            self._gui_log.append(f"[verify] {pend['label']} already satisfied → skip")
                            print(f"[GUI][verify] '{pend['label']}' no-op after retry → ALREADY SATISFIED → skip", file=sys.stderr, flush=True)
                            self._path_idx += 1; self._pending = None; self._verify_tries = 0
                        else:
                            print(f"[GUI][verify] '{pend['label']}' nav hop produced no submenu → fail-closed", file=sys.stderr, flush=True)
                            self._menu_path = []; self._pending = None; self._verify_tries = 0
            # (2) ACT on the current token (set a pending verification; DON'T advance until it's confirmed)
            path = getattr(self, "_menu_path", None) or []
            if path and self._path_idx < len(path):
                idx, token = self._path_idx, path[self._path_idx]
                if idx == 0:                          # menubar → CLICK to open the dropdown
                    cand = _match_token(token, [c for c in a11y if c[0].lower().startswith("menu:")])
                    if cand:
                        label, cx, cy = cand
                        self._anchor_x = cx; self._last_pick = label; self._path_tries = 0
                        loc = _action_locus("open", cx, cy)
                        self._pending = {"locus": loc, "pre": _region_sig(obs, loc), "label": token}
                        print(f"[GUI][path] step {self._gui_count}: open menu '{token}' @({cx},{cy})", file=sys.stderr, flush=True)
                        return f"gui[path] open '{token}'", [f"pyautogui.moveTo({cx}, {cy}); pyautogui.click({cx}, {cy})"]
                else:                                 # submenu item → match in the region-clipped OCR
                    ax = self._anchor_x
                    cv = _ocr_candidates(obs)
                    region = [c for c in cv if (ax - 220) < c[1] < (ax + 580) and c[2] > 88] if ax is not None else cv
                    cand = _match_token(token, region)
                    if cand:
                        label, cx, cy = cand
                        is_leaf = (idx == len(path) - 1)
                        self._last_pick = label; self._path_tries = 0
                        kind = "click" if is_leaf else "hover"
                        loc = _action_locus(kind, cx, cy)
                        self._pending = {"locus": loc, "pre": _region_sig(obs, loc), "label": token}
                        if is_leaf:                   # LAST token → CLICK the leaf (activates / opens dialog)
                            print(f"[GUI][path] step {self._gui_count}: CLICK leaf '{token}' @({cx},{cy})", file=sys.stderr, flush=True)
                            return f"gui[path] click '{token}'", [f"pyautogui.moveTo({cx}, {cy}); pyautogui.click({cx}, {cy})"]
                        print(f"[GUI][path] step {self._gui_count}: HOVER '{token}' @({cx},{cy})", file=sys.stderr, flush=True)
                        return f"gui[path] hover '{token}'", [f"pyautogui.moveTo({cx}, {cy}); time.sleep(0.6)"]
                # token not visible yet → re-perceive a few times (the flyout needs a beat); then fail CLOSED
                self._path_tries = getattr(self, "_path_tries", 0) + 1
                print(f"[GUI][path] token '{token}' not visible (try {self._path_tries})", file=sys.stderr, flush=True)
                if self._path_tries < 3:
                    return "gui path-wait", ["WAIT"]
                print(f"[GUI][path] FAIL-CLOSED on '{token}' → reactive ladder", file=sys.stderr, flush=True)
                self._menu_path = []; self._pending = None
            elif path and self._path_idx >= len(path):   # all tokens acted + confirmed → done
                self._mode = "done"; self._done = True
                self.last_trace = "gui path-done | " + " | ".join(self._gui_log)[:400]
                return "gui done", ["DONE"]

        # ── reactive a11y→CV ladder (non-menu tasks, or a failed/abandoned path) ──
        sel, plane = self._next_pick(a11y), "a11y"
        if sel == "done":
            self._mode = "done"; self._done = True
            self.last_trace = "gui done | " + " | ".join(self._gui_log)[:400]
            return "gui done", ["DONE"]
        if sel is None or (isinstance(sel, tuple) and sel[0] == last):
            print("[GUI] a11y blind/stuck → CV(OCR) fallback", file=sys.stderr, flush=True)
            cvsel = self._next_pick(_ocr_candidates(obs))
            if cvsel == "done":
                self._mode = "done"; self._done = True
                self.last_trace = "gui done(cv) | " + " | ".join(self._gui_log)[:400]
                return "gui done", ["DONE"]
            if isinstance(cvsel, tuple) and cvsel[0] != last:
                sel, plane = cvsel, "cv"
        if not isinstance(sel, tuple) or sel[0] == last:
            self._stuck = getattr(self, "_stuck", 0) + 1
            print(f"[GUI] neither plane advanced (stuck {self._stuck})", file=sys.stderr, flush=True)
            if self._stuck >= STUCK_LIMIT:
                self._mode = "done"; self._done = True
                self.last_trace = "gui stuck | " + " | ".join(self._gui_log)[:400]
                return "gui no-progress", ["DONE"]
            return "gui settle", ["WAIT"]
        self._stuck = 0
        label, cx, cy = sel
        self._last_pick = label
        self._gui_log.append(f"[{plane}] {label} @({cx},{cy})")
        print(f"[GUI][{plane}] step {self._gui_count}: '{label}' @({cx},{cy})", file=sys.stderr, flush=True)
        return f"gui[{plane}] '{label}'", [f"pyautogui.moveTo({cx}, {cy}); pyautogui.click({cx}, {cy})"]
