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

### calc / gimp / chrome (GUI domains — away plane)
*(broad run in progress — fill from `/tmp/osworld_broad_results.json`. Expectation: mostly `GUI_NEEDED`,
which quantifies the away-plane build.)*

## Broad per-domain map (home/away)
*(to fill on broad-run completion — per-domain PASS/N + failure-category counts)*

| date | commit | domain | PASS/N | notes |
|---|---|---|---|---|
| 2026-06-20 | 5b021a8 | os | 2/3 | terminal task = app-caching residual |

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
