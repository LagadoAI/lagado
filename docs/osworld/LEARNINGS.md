# OSWorld — What Each Finding Tells Us To Build (the learning ledger)

Principle (user, 2026-06-20): **NOTHING is a no-attempt.** Every result — pass, fail, "too hard", "the
model can't" — is a signal that tells us what to build or how to adjust. We account for *all* of it.
Companion data: [`RESULTS.md`](RESULTS.md), [`broad_results_2026-06-20.json`](broad_results_2026-06-20.json).

## The unifying architecture the findings point to
Every finding below resolves to ONE spine: **plane-exhaustion + switch, gated by GOAL-level verification.**

```
goal → CLI plane (discover → ground → operate → reground)         ← our home plane (built)
        └─ GOAL-LEVEL verify (did the GOAL happen? not just rc=0 / readback)
             ├─ met → DONE
             ├─ unmet, path remains → keep grounding
             └─ unmet, CLI exhausted → SWITCH to GUI plane
                  └─ GUI plane: a11y → CV → pixel (fallback order)   ← NOT built (the away plane)
                       └─ GOAL-LEVEL verify
                            ├─ met → DONE
                            └─ unmet, GUI exhausted → INFEASIBLE → last action = FAIL
```

The **goal-level verify failure after a plane exhausts** is the single mechanism that drives: the
CLI→GUI switch, the running-app reload, AND infeasibility detection. Build that spine and the findings fall
out of it.

## Findings → requirements (the ledger)

### F1 — Running-app caching (os/13584542 terminal-size, chrome/030eeff7 Do-Not-Track) → CMD_WRONG
The CLI sets the config (gsettings/dconf readback PASSES, rc=0) but the **running app cached the old value**,
so the GOAL fails. **Tells us to build:**
- **R1a — GOAL-LEVEL effect-verify** (not just rc/readback): does the *goal artifact* hold? This is the
  plane-switch trigger. (Our current effect-verify only re-reads the key — insufficient; it passed here.)
- **R1b — config-apply / app-reload:** after a config write to a running app, restart it so the change
  takes effect (`gnome-terminal-server`, chrome, …). Half-built (`_running_app_to_reload` in the adapter).
- **R1c** — if reload still doesn't satisfy the goal → that's the genuine CLI→GUI switch signal (F3).

### F2 — Infeasible tasks (29 = ~8% of OSWorld; gimp 10, os 5, vs_code 5, chrome 3, …) → win = refuse
OSWorld scores an infeasible task **1.0 iff the agent's LAST action is `FAIL`** (correct refusal); a wrong
`FAIL` on a *feasible* task scores 0. **The model CANNOT judge this** — Qwen answered FEASIBLE for both gimp
traps (it doesn't know GIMP can't do CMYK; it's over-optimistic). **Tells us to build:**
- **R2a — EMPIRICAL infeasibility detection, NOT model-judgment:** conclude infeasible only when *both*
  planes EXHAUST (CLI discover-operate finds no path AND GUI actuation finds no path) → emit `FAIL`. The
  plane-exhaustion IS the signal. (This is why F2 needs the same spine as F1/F3.)
- **R2b** — never rely on the model's prior to declare infeasible (false-positives forfeit feasible tasks).

### F3 — GUI_NEEDED (gimp palette/transparency, chrome reopen-tab/browse) → no CLI path
Genuinely need on-screen actuation. **Tells us to build:**
- **R3 — the GUI actuation plane:** a11y (AT-SPI elements) → CV (detected boxes) → pixel (coords) as the
  fallback ladder. This is the one big unbuilt half of the design and the bulk of the ~369 tasks.

### F4 — os home plane = 3/4 → the CLI machinery works
discover-then-operate + effect-verify + CLI-bias + path-grounding carry file/system/config tasks (incl. a
fresh compress-by-mtime task, solved 3-command). **Tells us:** the deterministic discover→ground→operate→
verify pattern is sound — extend it onto the GUI plane (same loop, different actuation), don't replace it.

### F5 — The model is over-optimistic about its own ability (F2 corollary)
It declares tasks feasible/decomposable that it can't actually do. **Tells us:** feasibility and success must
be determined **empirically by the harness** (attempt + goal-verify + plane-exhaustion), never by the model's
self-assessment. Reinforces the whole "harness is the moat, model is swappable" thesis.

### F6 — At the wall, the agent should LOOK UP how, not just fail (user idea, 2026-06-20)
When discover-operate stalls (the terminal-size hallucination), the agent should *consult knowledge* before
declaring exhaustion. This is "give it more tools" — but the right tool, gated by sovereignty. **Tells us to
build a DISCOVERY ESCALATION LADDER (feeds the reground):**
- **R6a — system introspection** (built): `gsettings list-recursively`, `dconf dump`, `ls`, `which`.
- **R6b — LOCAL DOCS (sovereignty-safe, cheap, do this next):** `man`, `tldr`, `<cmd> --help`, `apropos`,
  `info`. The terminal can read its own manuals — this would have grounded the gnome-terminal schema without
  any network. Air-gap-clean; just more discovery probes.
- **R6c — GATED web search (security-profile-controlled):** self-hosted SearXNG or browser-via-computer-use
  (see memory `lagado-react-search-grounding`: DDG instant-answer works, general search is bot-blocked).
  **OFF in Strict/air-gapped (regulated) mode — live egress breaks the sovereignty promise; ON in Open mode.**
The escalation: introspection → local docs → (if profile allows) web. Search ≠ a special mode; it's the
top rung of the SAME discover-then-operate ladder.


### F7 — GUI plane first-contact (2026-06-20): selection works, menu NAVIGATION is the wall
Wired our perception-fusion selection behind the switch (a11y candidates → el_N pick → click). Result on
gimp:3+chrome:3 = 0/6 BUT the plane ENGAGES: it selects correct top-level elements (File/Image/Edit/Select
menus, Show-Applications) and FAIL-CLOSES (`none`→escape, no hallucinated clicks). The wall is reactive
navigation: after clicking a menu, the next a11y obs lacks the opened submenu items → every submenu target
escapes (Open/Mode/Transparency/Export As). **Tells us to build:**
- **R7a — settle-after-click + re-observe:** confirm the click's effect (menu opened / dialog appeared) in a
  fresh a11y read BEFORE picking the next element (our observe-until-quiet, ported to the GUI plane).
- **R7b — REACTIVE GUI loop:** plan ONE GUI step from the LIVE screen, not a fixed upfront click-list — the
  planner's upfront GUI plans are long + partly hallucinated ("In the Export dialog select CMYK" as a click
  target). Observe→pick→act→observe.
- **R7c — CV/pixel fallback:** when a11y yields no candidate for a real target, fall to CV-detected boxes
  then pixel (the a11y→CV→pixel ladder; CV/pixel still TBD).


### F8 — R7 reactive loop WORKS; the wall is now menubar-menu OPENING (2026-06-20)
Reactive GUI loop + settle/no-progress retest (gimp:3+chrome:3 = 0/6). The LOOP IS SOUND: it reasons to the
right menu (Colors/Image for color-mode tasks), re-observes each step, and the no-progress detector stops
cleanly when a click has no effect (no flailing). And it DROVE REAL WEB NAVIGATION on chrome/0d8b7de3
(link→combo-box→menu-item) — clicks DO register and change the screen. **The isolated wall:** clicking a
GIMP MENUBAR menu (`menu: Colors` @(372,76)) does NOT open it — the submenu items never enter the a11y
candidate set, so the loop re-picks the same menu → no-progress stop. Menubar popups specifically (web
elements work). **Tells us to build R8 — menu-open interaction:**
- **R8a — diagnose:** does the click open the menu but a11y miss the transient popup (timing/focus), or does
  the click not register on the menubar at all? (focused probe: click menu, dump a11y, check for submenu).
- **R8b — likely fix: KEYBOARD menu nav** (Alt+<mnemonic> / arrow keys) — robust for menubars where
  click-popups are flaky; OR window-focus-first + move-then-click + longer settle while the popup is open.
- This is a narrow GTK-actuation fix, NOT a loop-logic problem (R7 logic verified).


### F9 — R8 diagnosed: GIMP menu items are INVISIBLE to a11y → CV/pixel is REQUIRED, not a click fix
R8a probe on the real `menu:Image @(269,76)`, 4 interactions: plain click → +0 (didn't register);
moveTo+click+settle / F10+Down / Alt+i → screen CHANGES (+6/+7 elements, incl. an Image-menu tooltip) but
**ZERO menu-items ever enter the a11y tree.** The menu responds; its items just aren't AT-SPI-exposed (GIMP
coverage gap, or the popup closes during the a11y query — transient-popup race). **No a11y interaction can
navigate a GIMP menu.** Contrast: chrome web nav worked (rich web a11y). **This is the a11y-coverage wall the
ladder exists for. Tells us:**
- **R8 (menu-click fix) is WRONG** — there are no a11y nodes to select, regardless of how we open the menu.
- **R7c (CV/pixel plane) is now REQUIRED, not optional:** when a11y yields no candidate for a real on-screen
  target, fall to CV — read the menu from the SCREENSHOT (which reliably captures the open popup), OCR/match
  the item text, click by PIXEL. Needs OCR (tesseract) or a grounding/parse model (OmniParser-style) — this
  is the 'captioning is a required sense' finding (memory lagado-axblind-probe-finding) made concrete.
- VALIDATES the a11y→CV→pixel ladder empirically: a11y for accessible apps (web/chrome), CV+OCR for native
  apps (GIMP, likely libreoffice). The CV plane is the next real build, gated behind a11y-yields-nothing.


### F10 — R7c CV ladder WORKS mechanically; native-menu nav hits the spatial-grounding frontier
a11y→CV ladder verified: a11y stalls on the GIMP menu → CV(OCR) fires → reads menu text off the screenshot
→ clicks by pixel. The MECHANISM is real (F9 answered: CV sees what a11y can't). BUT it LOOPS
(Colors→Mode→Colors→Mode). Two frontier problems: (1) MENU TRANSIENCE — the menu likely closes before the
next screenshot, so the open submenu isn't in the captured pixels; (2) SPATIAL AMBIGUITY — `text:Mode`
@(1750,482) is the WRONG 'Mode' (right-side layer-mode dock, not Image→Mode menu item); OCR finds several
'Mode's and our text-match+el_N selection has NO spatial reasoning to pick the one IN the just-opened menu.
**This is the GUI frontier the leaderboard-toppers solve with specialized GROUNDING models (UI-TARS,
OmniParser+planner) that understand spatial layout — a grounding-model-shaped hole, NOT a loop-logic bug.**
Tells us — STRATEGIC FORK (not a small fix):
- **R10a — accept native-app deep-GUI (GIMP menus) as out-of-current-scope**; double down where we WIN: the
  CLI home plane (os 3/4) + accessible GUI (chrome web nav worked). Note: regulated-market tasks skew
  doc/form/file, not GIMP image-editing — this may be the right product focus anyway.
- **R10b — invest in a spatial GROUNDING model** (OmniParser-style screen-parse, or a small grounding model)
  for the CV rung — the field's answer, but a bigger build + a model dependency.
- **R10c — menu-transience handling** (hold-open / capture-while-open) — narrower, may partially help, but
  spatial ambiguity remains.


### F11 — CORRECTION to F10: the GIMP wall is an UNHANDLED MODAL DIALOG, not spatial grounding
Exact diagnosis (screenshot saved gimp_modal_wall_2026-06-20.png): after loading the image, GIMP shows a
MODAL **'Convert to RGB Working Space?'** dialog (embedded color profile) with buttons Keep/Convert/Help/
Don't-ask-again. **The modal GRABS input → clicking the Image menu does NOTHING; the menu can't open while
the dialog is up.** The agent never dismissed the modal → looped clicking an unreachable menubar. OCR
'confirmed': no Image-menu items on screen (the 'menu region' text was the Crop tool-options dock; 'Mode'
@1750 was the right-dock layer-mode — a red herring). **F10's 'spatial-grounding frontier' was PREMATURE —
the CV/a11y mechanisms work; the immediate wall is a startup MODAL.** Tells us to build:
- **R11 — modal/blocker handling (TRACTABLE, not a grounding model):** detect a dialog with action buttons
  (Keep/Convert/OK/Close/Cancel) — they ARE in the a11y tree — and DISMISS it FIRST, before pursuing the
  goal. A 'clear the way' rung at the top of the GUI loop (and re-check after each step — modals appear
  mid-task). Then RE-DIAGNOSE menu nav (it may just work once nothing blocks; or reveal the next real layer).
- LESSON: diagnose with the actual SCREENSHOT before concluding 'frontier'. The wall was mundane + fixable.


### F12 — BREAKTHROUGH: F9 was ALSO confounded by the modal; the a11y→CV ladder is VALIDATED
Clean re-diagnosis (dismiss modal FIRST, then moveTo+click Image): the Image menu OPENS (screenshot
gimp_menu_OPEN_2026-06-20.png — Duplicate/Mode/Canvas Size/Scale Image/Merge Visible Layers/Image
Properties all visible, no modal). **F9's 'GIMP menus a11y-invisible' was right about a11y (a11y STILL shows
0 menu-items even with the menu open) but the menu never OPENED before because the MODAL was blocking +
bare-click didn't open it.** With modal cleared + moveTo+click: menu opens, a11y=0 items, but **CV/OCR reads
ALL the items** (Merge Visible Layers, Flatten Image, Image Properties…). **The a11y→CV ladder is exactly
right and empirically proven:** a11y for the menubar (in a11y), CV/OCR for the menu ITEMS (a11y-blind).
Two-line fix: (1) R11 modal-first clears blockers; (2) **moveTo+click** (not bare click) so menus actually
open. THE WALL IS BROKEN — the chain is modal-clear → moveTo-open → a11y/CV-select → click. Lesson
(third time): SCREENSHOT FIRST. 'grounding frontier'(F10) and 'a11y-blind-unfixable'(F9) were BOTH the same
unhandled modal + a bare-click. The hard wall was two mundane bugs.

### F13 — LIVE: dock-launcher disruption fixed; a11y→CV ladder navigates INTO the menu; next wall = plane oscillation
The F12 breakthrough proved each piece works in ISOLATION (clean diag). Integrating in the live run hit one
confound then exposed the real next wall — both now diagnosed with screenshots, not theory.
- **Confound (FIXED): the dock-launcher click.** In the run, step 1 kept clicking the GIMP dock icon
  'push-button: GNU Image Manipulation Program' @(35,541) — the model ranks it goal-relevant ("image"). A
  screenshot (dock_1_after_dock_click.png) CONFIRMED the cause: clicking an already-open app's dock icon
  pops a **window-preview overlay + shifts focus**, so the subsequent modal needed 2 tries and the menu
  never opened. A PROMPT rule ("don't click launchers") did NOT stop the model. **Fix = DETERMINISTIC:
  drop far-left push-buttons (cx<60, the Ubuntu dock strip) from `_parse_a11y` candidates.** After: step 1
  is sane, **modal dismisses on try 1**, `[GUI][a11y] menu: Image` opens it, and **`[GUI][cv] step: 'Mode'`
  — the menu OPENS and CV reads the items and picks the correct first hop (Image→Mode→…).** The full ladder
  fires LIVE: a11y for the menubar, CV/OCR for the a11y-blind menu items. The dock wall is broken.
- **Next wall (DIAGNOSED, not yet fixed): a11y/CV plane OSCILLATION on menu DESCENT.** After CV picks
  `Mode`, the next step's a11y plane re-offers the top-level `menu: Image` @(269,76), the model RE-PICKS it
  (it matches the goal), which TOGGLES the menu shut — undoing CV's descent. Loop: Image→(CV)Mode→Image→
  (CV)Mode… never reaches the Mode SUBMENU (RGB/Grayscale/Indexed). Also CV then matched a stale 'Mode'
  @(1750,482) = the right-dock layer-mode, a red herring. **Root cause: menu descent is a free per-step
  pick that can bounce back to the menubar; it needs to be a COMMITTED mode — once a menu is open and we're
  descending, suppress the menubar-parent candidates (or prefer the CV submenu items) until the path
  completes or dead-ends.** Plus: the menu-item click must land on the submenu-PARENT precisely to open the
  child submenu.
- **Task-choice note:** judged on `045bf3ff` "turn image into CYMK mode" — a BAD target: stock GIMP's
  Image→Mode has only RGB/Grayscale/Indexed, **no CMYK** (needs a plugin/export), so even perfect
  navigation can't complete it. Use `06ca5602` (Palette-Based → Image→Mode→**Indexed**, reachable) and
  `2a729ded` (transparent bg → Layer→Transparency→Add Alpha Channel) as the clean navigation judges.

### F14 — RESOLVED: menu descent works LIVE via knowledge-frame path-planning + deterministic follow
F13 left two quantified walls (lexical menu mis-pick + CV pollution). A boot-free probe found the root and
the fix: the model picks the WRONG menu when asked to SELECT ("which of these menus matches 'transparent'?"
→ Image, **9/9 wrong**, independent of ranking position — not a late-band artifact) but the RIGHT one when
asked as KNOWLEDGE ("in GIMP, what is the menu PATH?" → `Layer > Transparency > Add Alpha Channel`, **5/5**).
NOTE: grounding the knowledge prompt in the menu bar RE-PRIMES the mis-pick (listing 'Image' → `Image >
Adjustments > Transparency`, 5/5 wrong) — so name the APP, do NOT list the menus.
- **Build:** `osworld_plan --menupath GOAL APP` (knowledge frame, temp 0.1) → path tokens. Adapter PLANS the
  path once (when the real menu bar is visible), then FOLLOWS it deterministically: token0 = menubar CLICK
  (opens dropdown), middle tokens = submenu-parent HOVER (GTK flyout opens on dwell, not click), last token
  = leaf CLICK. Each token matched on screen (`_match_token`: substring → word-overlap) — a11y for the
  menubar, **region-clipped CV** (anchored on the opened menu's x, y>menubar) for the a11y-blind items, so
  off-menu red-herrings can't be picked. Token never appears → fail CLOSED to the reactive ladder.
- **Two integration bugs found by INSTRUMENTING (not guessing):** (1) `_app_name` grabbed the first a11y
  application = `ibus-x11` (input-method daemon), feeding the planner a junk app → lexical Image path. Fix =
  skip shell/daemon apps, pick the foreground app (most UI elements). (2) planning fired on an early frame
  where only the desktop panel's `System` menu was in a11y (`menubar=['System']`) → premature empty lock.
  Fix = require ≥3 menus before planning; bounded-WAIT otherwise.
- **LIVE RESULT (gimp/2a729ded):** `app='gimp'` → `planned: Layer > Transparency > Add Alpha Channel` →
  `open menu 'Layer' @(320,76)` → `HOVER 'Transparency' @(371,318)` → `CLICK leaf 'Add Alpha Channel'
  @(686,317)`. The correct menu opened (NOT Image), the flyout hovered, the leaf clicked, no red-herrings.
  **Both F13 walls resolved, the descent mechanism is proven end-to-end.**
- **Still 0.0 (expected, NOT a descent failure):** transparency is MULTI-OPERATION (add-alpha → select bg →
  delete); the follower does ONE menu op then stops. NEXT LAYER = multi-operation menu sequencing. A clean
  full PASS wants a single-op task (palette Image→Mode→Indexed→Convert) — blocked upstream by the planner
  ROUTING bug ("set image to Palette-Based" → desktop `gsettings`, never reaches GUI).

## PATCHWORK AUDIT (2026-06-21, user directive) — "if a solution is task-specific it is fragile by nature"
Principle: build the UNIVERSAL floor first (lower fidelity, works everywhere); add task-specific solutions
as OPTIONAL sugar ON TOP — never as the foundation. Building rules tuned to THESE OSWorld tasks = building
for the test. The test exists to EXPOSE weakness, not to be hardcoded around. Audit of accreted patches:

| # | Patch | Why fragile | Universal floor | Status |
|---|---|---|---|---|
| 1 | `_DISMISS` word list (ok/yes/convert/keep/…) | English, finite; `convert/keep` are GIMP-dialog words | dialog present → press **default button / Enter** (no vocab) | **FIXED ✓live** |
| 2 | dock filter `cx<60` push-buttons | hardcoded px geometry for the Ubuntu/Cinnamon dock | restrict a11y to the **focused app's subtree** (dock = different app) | **FIXED ✓live** |
| 3 | `RUNNING_APP_RELOAD` table (gnome-terminal-server, nautilus) | per-app; every app needs a row | CLI unmet → do it via the **app's own UI** (GUI plane); table = optional sugar | **REMOVED** + brain-verify REJECTED → F15 |
| 4 | menu region-clip `anchor_x−220…+580, y>88` | magic px offsets, toolkit/res-tuned | derive region from the **opened menu element's bbox** | open |
| 5 | `UNGROUNDED` stderr-string list | English, tool-specific | `rc≠0` is the signal; list = refinement only | open |
| 6 | magic consts (`cd ~/Desktop`, `≥3 menus`, `sleep 0.6`, `conf<0.4`, `MAX_GUI=16`) | benchmark/res assumptions | derive or mark as tunables, not load-bearing | open |

**The pattern (the sweet tooth):** a specific task failed → I added a specific RULE (filter THIS button,
dismiss THESE words, reload THIS app) instead of the general invariant it's an instance of. The template
done RIGHT = `_app_name` (universal floor: pick the app with the most UI elements; thin daemon skip-list as
optional sugar). Every patch above should be refactored to that shape. Keep this table updated as patches
are universalized or new ones are caught.

### F15 — the universal R1 refactor (#3): brain-authored shell goal-verify is UNSAFE + noisy → the spine's real bottleneck
Doing R1 the universal way (CLI goal-verify gates the CLI→GUI switch; drop the per-app reload table) exposed
that the **goal-verify SIGNAL is the bottleneck of the whole spine**, and a brain-written shell check cannot
be it:
- **Noisy:** asked to write a check for a goal, the brain was **0/3 semantically correct** — volume → checked
  a `volume-max` *ceiling key* not the set volume; terminal → `stty size` of the runner's pty not the profile
  default; palette → a `org.gimp.GIMP` schema that doesn't exist. So `met=False` is noise; switching on it
  would send a PASSING CLI task on a wasteful GUI excursion.
- **UNSAFE (the killer):** the model **ignores "read-only"** — the copy-goal check contained `cp --parents`
  and the compress check `gzip -f`/`rm`/`>> file`. Running a brain "check" on the guest MUTATES the scored
  end-state. A trust-positive/negative split does NOT fix this (a mutating check mutates whether it passes or
  fails). So the whole "brain writes a verify command we execute" approach is rejected.
- **What shipped (#3):** (a) the per-app reload TABLE is REMOVED (universal-first — no app-specific hacks);
  (b) brain `--verify` is NOT wired (the Rust mode is left dead/documented, not called); (c) the SAFE
  goal-verify remains `_readback_check` — HARNESS-derived, deterministic, guaranteed read-only because the
  harness builds the `gsettings get`/`dconf read`, not the model (config-scoped, per-command, real).
- **R1c (outcome-driven CLI→GUI switch) stays GATED** on a trustworthy+safe goal-verify, which is now the
  named open problem. The universal verify must be **harness-derived or PERCEPTION-based** (does the screen/
  a11y show the goal achieved? — read-only by nature), never a model-authored shell command. That's the next
  real build for the spine — not another patch.

### F16 — the verification spine, built + VALIDATED: change-at-locus floor catches a no-op the model would have claimed
The systemic truth (user's audit lens): the model is an unreliable narrator — of the world and of itself —
so truth must be GROUNDED BY OBSERVATION, not asked of the model. Reconciliation, not self-certification:
the model never observes/reports/judges its own result; it emits a CLAIM (action), the harness observes
reality, deterministic code reconciles. "You can't fake the balance."
- **BUILT — change-at-locus floor (harness-derived).** After each action the harness checks whether the
  effect the ACTION TYPE implies actually landed at the locus the ACTION defines — model never consulted.
  `_region_sig` (a11y label-set + noise-robust 32×32 grayscale grid) + `_effect_landed` (a11y-set change OR
  >3% pixel delta) + `_action_locus` (open→dropdown-below; hover→flyout-right; leaf→whole-app). The path-
  follower is now CONFIRM-GATED: advance a hop only on observed effect; no change after a beat = fail-closed.
- **LIVE (gimp transparency):** `open Layer → [verify] LANDED` (dropdown appeared) → `HOVER Transparency →
  [verify] LANDED` (flyout appeared) → `CLICK leaf 'Add Alpha Channel' → [verify] NO effect → fail-closed`.
- **The leaf 'no effect' was a TRUE CATCH, not a false negative (diagnosed, not guessed):** the measured
  delta was ~1% on BOTH the whole-app AND a concentrated menu-region locus (so not a locus/threshold miss),
  a11y unchanged, no modal. Screenshot: **'Add Alpha Channel' is GREYED OUT** — the dog PNG already has an
  alpha channel, so GIMP disables it. The click hit a dead item; the menu stayed open; genuinely zero effect.
  **The harness observed that the planned action did NOTHING and refused to advance — exactly where a model
  relying on self-report would have asserted "done."** This is the spine working as a truth instrument.
- **Deeper finding:** the floor catches PLAN↔WORLD mismatches, not just hops — a disabled/already-satisfied/
  inapplicable action surfaces as "no observed effect." Here it also reveals the plan is wrong for the state:
  alpha already exists, so the real task is select-background + delete, not Add Alpha Channel.
- **What the no-effect signal now ENABLES (next builds, observation-grounded):** (1) precondition-SKIP (BUILT + validated, retry-then-skip) — a
  no-op on a planned step often means it's ALREADY SATISFIED (alpha exists) → move on, don't fail (the banked
  Option-2 idea, now grounded in observation not a model guess); (2) RE-PLAN toward the next sub-goal when an
  op is moot/exhausted. The floor is the trigger both need. No leaf-locus fix needed — the floor is correct.
- **BOTH BUILT + validated.** Precondition-skip: settle → re-attempt (rule out a transient miss) → persistent
  no-op on a LEAF = already satisfied → skip; a nav HOP with no submenu = real failure → fail-closed.
  RE-PLAN (document-changed proxy, NON-fragile per the user's "easy ≠ narrow ≠ fragile" check): when a path
  completes, compare WHOLE-APP **pixels** (no guessed canvas region) between two MENU-CLOSED states — the
  path-start baseline and now, after a double-Escape closes both menu levels — so chrome can't pollute it.
  Changed → progress → done; no change → RE-PLAN a DIFFERENT op (`--menupath` told the tried paths), budget 3.
  PIXEL-only is deliberate: the work-product is visual, the whole-app a11y set churns on any UI interaction
  (it false-positived 'progress' on a no-op → caught + fixed). Live: Add Alpha (skip) → RE-PLAN Make
  Background Transparent (LANDED but doc unchanged → progress check is the honest backstop) → RE-PLAN Color
  to Alpha — the loop HUNTS the solution space. Separates 'the action fired' from 'the work-product moved'.
  Open: re-plan model quality (some hallucinated items) + reaching a real op within budget (MAX_GUI 16→28).

### F17 — broad multi-task run surfaced a STATE-LEAK reset bug (the value of the broad test)
First broad re-run after the verification-spine build (gimp:3 chrome:3 os:3): scores 2/9 (os copy+volume
PASS; all 7 GUI tasks 0.0). But the per-task TRACES were MISLABELED — a chrome task's recorded trace showed
a GIMP `Layer > Transparency` path; os/terminal showed a chrome natural-products trace. Root cause:
`agent.reset()` cleared only `_done` + the lists, NOT the GUI/plane/plan/path/verify/re-plan/reporting state
(`_mode`, `_menu_path`, `_path_planned`, `last_trace`, `_instruction`, `_tried_paths`, `_doc_baseline`…). So
every task after the first INHERITED the previous task's state — worst case it leaked into the runs, not just
the reporting, making the gimp/chrome GUI results UNRELIABLE. **A single-task run starts fresh, so ONLY a
broad/multi-task run exposes this** — exactly the user's reason for demanding the broad test. Fixed:
`reset()` now fully clears per-task state. The os CLI passes are unaffected (don't touch GUI state). VALID
re-run in progress. LESSON: multi-task contamination is invisible to single-task iteration — run broad
periodically to catch it. Standing baseline (2026-06-20): os 3/4, gimp 0/3, chrome 0/3.

### F18 — palette retry REFUTED the "leaf-grounding works, one fix away" thread (exhausts A on GIMP)
The leaf-settle + whole-tree `modal=true` scan fix did NOT fire. Trace `gimp/06ca5602` ("set the image to
Palette-Based"): plan `Image > Mode > Indexed` → open Image (LANDED) → hover Mode (LANDED) → CLICK leaf
`text: Indexed:_` @(597,193) (LANDED) → progress **no change** → RE-PLAN ×2 (`Convert to Indexed`, then
`Palette Based`). **No `[GUI][modal] modal=true → Enter` line appears after ANY leaf click** — the
Convert-to-Indexed dialog was never caught (opened-then-Escaped, or never opened); the conversion was never
committed. Score 0.0. This run KILLS the "one fix away" optimism — the prior-session belief that leaf
selection was sound and only the Escape-cancels-dialog timing remained. Three findings:
  1. **Fix didn't fire** — modal never detected post-leaf even with the whole-tree scan. Whether the dialog
     opened-but-isn't-modal vs never-opened is GIMP-mechanics depth — deliberately NOT instrumenting (that
     depth is exactly what fork B questions the value of).
  2. **Re-plan spun on the IDENTICAL physical action 3×** — same coord (597,193), same OCR label `Indexed:_`,
     for three different planned leaves. A "re-plan" that resolves to the same click carries zero new
     information and just burns the budget. (Native GTK menu items are a11y-blind → labels are OCR; the
     submenu only exposes one "Indexed…" item, so every path collapses to it.)
  3. **Scoring confound** — the result file `palette_computer.png` (pulled 14:46) is GENUINELY TRUNCATED:
     `PIL.OSError: image file is truncated`, while both source images open fine. So this task's 0.0 has an
     IO/evaluator confound layered on top of the actuation miss — partly unscoreable as run.
DECISION: this was the last "sure-to-help" A-item on the GIMP/creative-app path and it didn't land →
**A is exhausted on this terrain.** Fork B (pivot to a11y-friendly document/form tasks where the product
actually lives) is now the recommended direction. Awaiting user instruction.

### F19 — GLOBAL SWEEP (33 tasks, 10 domains) + API-COVERAGE PROBE → the build-map names the build
First whole-surface measurement (the "entire build → 72%" reframe; 72% = human baseline, SOTA now ~82% incl.
Holo3-35B-A3B = a 3B-ACTIVE MoE → validates small-model+harness thesis). **Denominator: 369 tasks; 72% = 266;
multi_apps = 101 = 27% (single-app max = 72.6% → 72% REQUIRES cracking multi_apps).**

SWEEP RESULT (33 sampled): **PASS 4** (os×3, vs_code×1 — ALL command-plane), **GUI_NEEDED 17**, **CMD_WRONG 11**,
**EXC 1**. Per-domain: os 3/3, vs_code 1/3, all else 0. Scorecard = 12% — but per the ceiling-mindset
([[osworld-ceiling-mindset]]) this is a BUILD-MAP, not a verdict. The read:
- The 4 PASSes are all command-plane; vs_code win = `code --install-extension` in ONE call. Where the command
  plane had the RIGHT VERB → deterministic pass.
- The 11 CMD_WRONG are ENCOURAGING: the router already CHOSE command (correct instinct), just fired the wrong
  verb (H2O-subscript, impress align, tbird dark-mode, vlc cone — all have real scripting paths). Routing
  works; the VERB VOCABULARY is missing.
- Most of the 17 GUI_NEEDED fell to clicking ONLY because no command verb was found (gimp CMYK/palette/transp →
  Script-Fu; calc total/gross/sheet → UNO; "play video" → `vlc <file>`). With a verb library they route to
  command, not pixels.
- → Both big failure classes are ONE root cause: **missing app-automation verb vocabulary.** NOT a missing
  CV/pixel plane (that's the residual).

API-COVERAGE PROBE (`probe_app_apis.py` — each app's interface ACTUALLY INVOKED from the command plane):
**0 ABSENT.** REACHABLE+clean: **libreoffice UNO** (★ calc+writer+impress = 117 tasks = 32% of bench), **chrome
CDP** :9222 (46), **vs_code `code` CLI** (23), **os** shell (24). REACHABLE-interpreter-but-proof-typo: **gimp
Script-Fu** (ran; my exact convert-indexed proof tripped on wrong constant CONVERT-NO-DITHER→CONVERT-DITHER-NONE
— reachability real, decisive verb-proof PENDING a one-char re-run). PRESENT/command-launchable: **vlc** (`vlc
<file>` + dbus-send; rich MPRIS control unproven). REACHABLE-but-shallow: **thunderbird** (--version runs; hard
tasks need prefs.js/config editing). → ~64% of the surface sits behind 5 proven-reachable APIs; multi_apps =
compositions of the same apps (same verbs per step).

**THE BUILD (data-named):** per-app automation VERB LIBRARY over the reachable APIs, **UNO FIRST** (32% of the
bench, reachable+rich+product-relevant), then CDP / code-CLI / Script-Fu / shell; model SELECTS typed verbs
(capability layer [[lagado-capability-layer]]), MCP-wrapped so benchmark work = product work. vlc/thunderbird =
config-file tail. CV/pixel grounding = the residual, NOT the main build. This is the "how not if" frame as a
bounded engineering program. Open: 1 EXC (multi_apps crash, empty plan) to fix; gimp decisive verb-proof re-run.

**REFINEMENT (user's lateral throw — "OS-level back doors for API-like access via CLI"):** below the per-app
APIs sits a GENERIC, largely SELF-DESCRIBING OS-level plane that collapses much bespoke work:
- **D-Bus** (`gdbus`/`busctl`/`qdbus`) — near-universal app/service control, and INTROSPECTABLE (`busctl
  introspect` enumerates callable methods at runtime → agent DISCOVERS the API, no hardcode; aligns
  discover-then-operate + [[lagado-no-hardcode-invariant]]). MPRIS = every media player; freedesktop.* = system.
- **gsettings/dconf** — GNOME/GTK config, introspectable.
- **Direct config files** — the truly universal back door: chrome Preferences(JSON), tbird prefs.js, vlcrc(INI),
  vscode settings.json. Edit→(restart)→state changed, any app, no API.
- **Sibling CLI tools** — ImageMagick (`convert -colors` = GIMP palette/CMYK w/o GIMP), pandoc, pdftk, jq —
  bypass the GUI app entirely for file transforms.
- (xdotool/AT-SPI actions = synthetic input, a BRIDGE not semantic API; breaks on Wayland.)
This maps onto the 11 CMD_WRONG directly (chrome DNT→Preferences, tbird dark→prefs.js, vlc cone→vlcrc, vscode
line-length→settings.json, vlc play→`vlc <file>`) — one reusable generic bridge reclaims the settings/control/
media cluster across ALL domains. LIMIT: does NOT cover semantic ops (calc formulas, image mode) → those still
need rich API (UNO/Script-Fu) or sibling CLI. **→ Build = GENERALITY LADDER, not per-app libraries:** (1)
config/control plane (D-Bus-introspected + gsettings + config-file editor) → (2) sibling-CLI tools → (3) app
rich-API (UNO first, 32%) → (4) GUI/a11y/CV residual. Cheapest+most-general first; layer 1 discovers its own
verbs. [PROVISIONAL — pending advisor skeptic pass on the build direction.]

**ADVISOR SKEPTIC PASS + PROOF — the premise reachable→executes→scores-1.0 was only proven at link 1.** Advisor:
"reachable ≠ verb-executes ≠ scores 1.0; prove one currently-FAILING task FAIL→PASS to a real env.evaluate()
1.0 before committing; deeper risk = the 11 CMD_WRONG mean the model ALREADY chose command + authored the WRONG
call → if the bottleneck is SEMANTIC AUTHORING not channel-availability, a verb library doesn't fix it." Ran the
gimp-palette proof (`proof_gimp_palette.py`):
- **VERB PROVEN:** file `RGB → P` via gimp-headless with the CORRECTED constant **CONVERT-DITHER-NONE** (origin
  typo CONVERT-NO-DITHER fixed). The API verb produces the correct end-state on disk. NOT inference.
- **env.evaluate() = 0.0 anyway** — the gimp evaluator does `shift+ctrl+e` to **EXPORT FROM THE RUNNING GIMP
  WINDOW**; my relaunched instance wasn't up-and-focused when the hotkeys fired → `palette_computer.png` never
  created (404). Disk file being indexed doesn't help — the evaluator reads the INSTANCE, not the disk.
- Incidental: **ImageMagick NOT installed** (`convert: not found`) → sibling-CLI layer needs provisioning;
  gimp-headless is the working fallback.
**→ THE CENTRAL BUILD CONSTRAINT (layer-revealed — this is the real finding):** OSWorld evaluators reconcile by
SAVING/EXPORTING FROM THE LIVE APP INSTANCE (gimp shift+ctrl+e export; calc ctrl+s save). So "operate via API" is
NOT "produce the right file" — the RUNNING INSTANCE must reflect the change when the evaluator pokes it. Build
pattern = **operate-on-file → reliable RELOAD-INTO-FOCUS** (or a live API endpoint, which these instances don't
expose: gimp has no live API; soffice would need `--accept=socket`). Reliable reload+focus is the FIRST hard
engineering problem, and chasing the clean 1.0 IS that engineering → belongs in the build session.
**BUILD NUANCES (carry forward):** the **calc evaluator saves-OVER the existing file** (ctrl+s + compare_table),
so an openpyxl/UNO-correct DISK file is ROBUST there (more so than gimp's NEW-file export); a channel-proof can
source target values from the GROUND-TRUTH xlsx (URL in task json) to ISOLATE channel from semantic-authoring;
container may LACK openpyxl → use headless-UNO or provision.

**DECISION: chose B** — premise empirically CHARACTERIZED (verb proven + constraint named, NOT inference) over
grinding boots in a 200k-context session; the build session resumes at this depth via this record (same depth,
cheaper context, reusable artifacts). **BUILD MILESTONES (order):** (M1) reliable reload→evaluator-reconciliation
— the PROVEN blocker; gimp-palette + a calc task driven to a real env.evaluate()==1.0 = milestone #1, producing
REUSABLE reload machinery not throwaway proofs. (M2) model SEMANTIC-AUTHORING via typed verbs (UNO calc
gross-profit — the advisor's key risk: can the model author the RIGHT call, not just reach the channel). Then
build the generality ladder (config/control plane → sibling-CLI → UNO-first rich-API → GUI/CV residual);
multi_apps orchestration is necessary-not-sufficient from reliable verbs. OPEN: 1 EXC (multi_apps/0c825995 crash,
empty plan) to fix; provision ImageMagick or commit to gimp-headless.

## Build order (data-driven)
1. **R1a — goal-level effect-verify** (the spine's trigger; cheap; unlocks F1 + is prerequisite for F2/F3).
2. **R1b — config-apply/app-reload** (finishes the running-app class on the CLI plane).
3. **R7c — CV/pixel plane (OCR/grounding)** — REQUIRED for native-app menus (a11y-blind; F9), gated by a11y-yields-nothing. The big build.
4. **R2a — empirical infeasibility** (falls out once both planes can exhaust → FAIL) — +8% of the bench.

Update this ledger as new runs surface new finding-classes. Every measured failure is a line item here.
