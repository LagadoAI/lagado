# OSWorld — Lagado Results Baseline (living record)

The honest, reproducible record of Lagado against the **real OSWorld benchmark** (xlang-ai), to study and
to compare against as we improve the harness. **Update this doc on every measured run.** Reproduction:
[`README.md`](README.md). Provider patches: [`provider_fedora_rootless.patch`](provider_fedora_rootless.patch).

## Config (pin + report on every run)
- **Brain:** Qwen2.5-Coder-7B-Instruct Q4_K_M on `:8080` (the *research* model for the ceiling; LFM2-8B is
  the shippable target — see memory `lagado-lfm2-brain-profile`). flash-attn + q8_0 KV cache → ~51 tok/s.
- **Observation:** `screenshot_a11y_tree`. **Action space:** pyautogui (we use the guest's arbitrary-python
  channel for the terminal plane). **max_steps:** 15. **Eval:** OSWorld's own world-state evaluators.
- **Env:** rootful podman + the 6-fix boot chain (see README). Each task **cold-boots** the guest (~5 min).
- **Plane:** TERMINAL/CLI only so far (discover-then-operate). GUI actuation plane (a11y/CV/pixel) = NOT
  BUILT → GUI-bound tasks are expected `GUI_NEEDED` failures (the away-plane spec).

## ⚠ Internal proxy vs. real OSWorld — the gap to remember
Our internal batteries (`osworld_stress` 11/11, `osworld_heldout` 8/8 = **19/19**) are FILE/SHELL proxies.
The **real OSWorld** is GUI-app heavy and much harder. Never quote the proxy as an OSWorld number.

## Harness build log + score progression (what moved the number)
| Commit | Change | Effect (internal osworld_stress unless noted) |
|---|---|---|
| 95d2bc7 | Gated raw-shell routing (undeclared/non-home → raw path) | 2/11 → 7/11 |
| aac45a9 | Planner action-type fix + git in guest | 7/11 → 9/11 |
| 4ce3b52 | Supervisor transition-only oscillation + multi-step examples | 9/11 → 11/11 |
| 9959695 | Held-out transfer battery + executable false-success fix | held-out 5/8→8/8 (with model swap) |
| 02deab0 | OSWorld bridge (osworld_plan) + LagadoAgent adapter | real-bench path opened |
| 499101f | Path-grounding (run from ~/Desktop) | **real os 1/3 → 2/3** |
| d09cd19 | Discover-then-operate (reground on ungrounding errors) | terminal task verified passable |
| ba33657 | Planner CLI bias for settings/config | os terminal verified 1.0 (focused) |
| 5b021a8 | Effect-verify (read-back config sets, exit-0-but-wrong) | general guard; terminal residual = app caching |

## Real OSWorld — per-task results

### os (terminal home plane)
| id | instruction | score | category | lesson |
|---|---|---|---|---|
| 28cc3b7e | turn volume to max | **1.0** | PASS | terminal plane solves a GUI-expected task (`amixer`/`pactl`) |
| 23393935 | recursively copy .jpg → cpjpg | **1.0** | PASS | path-grounding: run from `~/Desktop` (OSWorld working surface) |
| 13584542 | persist terminal size 132x43 | **0.0** | CMD_WRONG | model hallucinated schema (`org.gnome.terminal.legacy` vs real `…Terminal.Legacy`); discover→reground→`dconf write` VERIFIED 1.0 in isolation, but the full run is flaky: gnome-terminal caches its profile as a running dbus service (app-specific long tail, NOT a harness gap) |

**os = 2/3 reliable, 3/3 capable.** All three verified passable individually.

## Broad per-domain map — 2026-06-20 (commit 5b021a8, 10 tasks; raw: `broad_results_2026-06-20.json`)
NOTE: `calc` recorded 0 — the domain dir is `libreoffice_calc` not `calc` (runner glob bug; re-run pending).

| domain | PASS/N | breakdown |
|---|---|---|
| **os** (home plane) | **3/4** | 23393935 copy ✅, 28cc3b7e volume ✅, 37887e8c compress-by-mtime ✅, 13584542 terminal-size ❌ (app caching) |
| **gimp** (away) | **0/3** | 045bf3ff CMYK = CMD_WRONG (tried CLI, failed — winnable via ImageMagick/script-fu?); 06ca5602 palette + 2a729ded transparency = GUI_NEEDED |
| **chrome** (away) | **0/3** | 030eeff7 Do-Not-Track = CMD_WRONG (tried CLI — winnable via prefs file?); 06fe7178 reopen-tab + 0d8b7de3 browse-db = GUI_NEEDED |

**Categories:** PASS 3 · CMD_WRONG 3 · GUI_NEEDED 4.

### The actionable split (the plan this map specs)
- **os home plane carries (3/4, 75%)** — the general machinery (discover-operate, effect-verify, CLI bias,
  path-grounding) works on file/system/config tasks. The 1 miss = app-caching long tail.
- **GUI_NEEDED (4)** — tasks with NO CLI path (palette, transparency, reopen-tab, browse). These REQUIRE the
  **GUI actuation plane (a11y/CV/pixel)** — the one big unbuilt capability. This is the away-plane spec.
- **CMD_WRONG on GUI domains (gimp CMYK, chrome DNT)** — the model TRIED the CLI and failed, but these may be
  **CLI-winnable** (ImageMagick `convert -colorspace CMYK`; chrome `Preferences` JSON / `--enable-features`).
  Cheaper near-term win: extend the terminal plane's TOOLING + discovery to claw GUI tasks onto the home plane.

### Priority read
1. **CLI-plane tooling expansion** (cheap, home turf): image tools (ImageMagick/script-fu), browser config
   (prefs file), more discovery — converts a slice of "GUI" tasks (the CMD_WRONG class) into terminal wins.
2. **GUI actuation plane** (big build, the moat's other half): a11y/CV/pixel — required for GUI_NEEDED, and
   the bulk of the ~369 across libreoffice/gimp/chrome/vlc/vscode. Measure its size next (run libreoffice_calc
   + more per domain to size the GUI_NEEDED population).

## Tracking over time
| date | commit | os | gimp | chrome | calc | notes |
|---|---|---|---|---|---|---|
| 2026-06-20 | 5b021a8 | 3/4 | 0/3 | 0/3 | (bug) | first broad map; GUI plane is the spec |
| 2026-06-20 | 51807fb | 3/4 | 0/3 | 0/3 | (bug) | GUI plane wired: engages+selects top-level menus, fail-closes; wall = reactive menu navigation (R7) |
| 2026-06-20 | d76c331 | 3/4 | 0/3 | 0/3 | (bug) | R7 reactive loop works (reasons to right menu, drove web nav, no-progress stops clean); wall = menubar-menu OPEN (R8) |
| 2026-06-20 | (diag) | 3/4 | 0/3 | 0/3 | (bug) | R8a: GIMP menu items INVISIBLE to a11y (4 interactions, 0 menu-items) → CV/pixel plane REQUIRED (F9), not a click fix |
| 2026-06-20 | a77aebc | 3/4 | 0/3 | 0/3 | (bug) | R7c CV ladder MECHANISM works (a11y-stuck→OCR→pixel-click verified) but native-menu nav loops: menu transience + spatial ambiguity = grounding-model frontier (F10) |
| 2026-06-20 | (R11+) | 3/4 | ? | — | (bug) | BREAKTHROUGH: modal-first (R11) + moveTo+click → GIMP menu OPENS, a11y=0 items but CV/OCR reads them; a11y→CV ladder VALIDATED (F12). retest running |

## Failure-category taxonomy (the narrow-in keys)
- **PASS** — covered by the current harness.
- **GUI_NEEDED** — plan needs the GUI plane we haven't built (a11y/CV/pixel actuation). The away-plane spec.
- **CMD_WRONG** — terminal plane engaged but didn't achieve the goal (grounding / effect / app-behavior).
- **OTHER / EXC** — anything surprising; investigate from the saved trace.

## Standing lessons (so we don't relearn them)
1. Hallucination in the CLI plane = wrong *machine-specific identifiers* from training priors, not ignorance
   → fix with **real discovery** (list schemas, read UUIDs, dump config), then operate (prefer `dconf write`
   by path — schema-agnostic).
2. **exit-0-but-wrong** is real for config sets → read the value back (effect-verify).
3. Some failures are **app behavior** (gnome-terminal profile caching), not harness — don't hand-chase the
   long tail; the harness must generalize.
4. The terminal is the base it works FROM; it transitions to a11y/CV/pixel AS NEEDED (memory
   `lagado-osworld-real-bench`). The GUI plane is the next big build.
