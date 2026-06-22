# Lagado Plane-Governor — unified spec (2026-06-21)

**Problem (user):** the picker+switcher across perception/actuation planes *already exists* in the
codebase but **disjointed** — scattered across `agent.rs`, `supervisor.rs`, `perception/*`, `vm/*`, and the
OSWorld Python adapter. This spec joins them into one deterministic governor with the **CLI as launch pad**.

## The shape (converged with the user)
- **CLI = launch pad.** Most reliable plane; does most intra-OS work (file/system/launch) + the
  route-around-the-API **back-door** (config-file / gsettings-dconf / D-Bus / sibling-CLI) + **discovery**
  (run CLI → see the world) + is the vantage from which the governor **determines what's next**.
- **PICK** — deterministic `f(task, findings) → starting plane`. Task-appropriate, not a fixed order, not a
  lock: a settings goal starts at the back-door, a semantic in-app op at the API (if present) else a11y, a
  genuine GUI interaction at a11y. The pick is a *hypothesis*.
- **SWITCH** — deterministic `f(stalled_plane, task, findings) → next feasible plane`, fired mid-task by the
  "this plane isn't working" signal. The pick can be wrong; the switch corrects. Neither locks.
- **Loop:** pick → run → goal-verify/stall → on not-working, switch to next feasible (re-evaluating
  feasibility, incl. switch-BACK to a cheaper plane if the world changed) → repeat until goal met or all
  feasible planes exhausted → then honestly infeasible. The model NEVER chooses the plane (rails).
- **In-app planes ordered by visibility:** API → a11y → CV → pixel. Do NOT fall to the CLI for *in-app*
  work (least visibility) — the CLI's role on the in-app axis is the **back-door** (route-around), not a
  low-vis click surface.

## The plane set
| plane | what | perceive | act | reuse |
|---|---|---|---|---|
| **CLI** (launch pad) | shell: file/system/launch + discovery | `discover_environment` | `Actuator::run_command`, `tools::executor::dispatch`, capability verbs | A: battle-tested |
| **BackDoor** (via CLI) | route-around-the-API: config-file / dconf-gsettings / D-Bus / sibling-CLI | (CLI reads) | `osworld_plan --reground` (prefers `dconf write`), `deterministic_reform` (`command -v`) | A: prompt-only today → needs a `set_config` verb |
| **API** | app's programmatic surface (UNO/CDP/code-CLI) | app query | typed API call | **MISSING in Rust** (prototype: host `uno_apply.py`) |
| **A11y** | AT-SPI element tree | `Perceptor::read_screen` (`Ssh/Linux`) | `Actuator::click/type/key` + `selection` | B: live, behind the trait |
| **CV** | CV/OCR proposals | `cv_proposer::propose_frame` | (coords via selection) | B: built but **inert** (gated off) |
| **Pixel** | raw pixel-delta / coordinate | `FrameProcessor`/`DeltaDetector` | (raw coord) | B: built (used as settle signal) |

## What's REUSED (the governor is assembly, not invention)
- **Switch engine = `Supervisor::observe(StepOutcome, hash) → Directive`** (`supervisor.rs:157`). Designed
  AS this: "governor builds the ordered `Vec<EscalationTier>`, the supervisor walks it." `escalate()`
  (`:219`) = advance-to-next-plane + clean-slate reset.
- **Canonical "switch the PLANE, not retry" signal = `StepOutcome::PerceptionBlind`** (`supervisor.rs:26`).
  `TierKind::Sense` (`:45`) = the perception-plane bump (generalize to carry the target plane).
- **Plane abstraction seed = `Perceptor`/`Actuator` traits + `PerceptionCache`** (`perception/mod.rs:12,24,84`),
  generic across mock/host/VM. **`DynamicActuator`/`DynamicPerceptor`** (`vm/mod.rs:58,65`) = an existing
  ad-hoc 2-plane switch (VM↔host on `vm_port`) — the pattern to generalize from `vm_port` to `(task,health)`.
- **Per-step "did it work" = `effect_confirmed`/`effect_class`** (`agent.rs:1531/1521`), `structural_change`
  (`:1363`); anti-false-stall = `observe_until_quiet`/`settling_active` (`:1453/1384`) — MUST settle before
  judging "not working."
- **CLI self-heal = `decide_reapproach`/`diagnose_command`** (`agent.rs:790/758`); `Escalate(_)` = "CLI plane
  exhausted on this sub-goal" = a switch trigger. Tactical layer below = `recovery.rs` (7-mode dispatcher).
- **Goal-verify = `goal_satisfied`/`goal_postconditions`/`command_postcondition`** (`agent.rs:274/1215/1176`)
  + the SAFE harness-built readback (Python `_readback_check`; brain-authored shell verify REJECTED).
- **Pick seeds = `classify_subgoal`/`SubAction`** (CLI `Command` vs GUI `Click`, `agent.rs:417/388`) +
  `plan_goal` (CLI-biased, `:607`) + Python `predict` (CLI-first, `:425`).

## What's MISSING (the connective tissue to BUILD)
1. **No named `Plane` set / `PlaneId`** — surfaces are implicit (`run_command` vs `click`). → build `PlaneId`.
2. **No `pick(task, findings) → plane`** — everything is per-STEP; no task-level, feasibility-aware start. → build.
3. **No feasibility gate** — `escalate()` walks the ladder blindly; no "is the next plane applicable here?". → build `plane_applicable`.
4. **No unified discovery `Findings`** — env listing, `command -v` probe, focused-app are split. → build `Findings`.
5. **No routing-correction in Rust** — Python `_is_desktop_config`+focused-app ("settings plan but an app is
   focused → in-app → switch") has no Rust analog. → fold into `classify_task`.
6. **Goal-verify-fail → HANDBACK, not SWITCH** (`verify_or_handback`, `agent.rs:287`). THE core seam: on a
   feasible alternative existing, switch instead of handing back. → governor owns this edge.
7. **No first-class back-door capability** — config/dconf/sibling is prompt behavior, not a `set_config` verb.
8. **No switch-BACK / re-pick** — both designs are monotonic forward-escalation; can't return to a cheaper
   plane when the world changes. → `switch()` re-evaluates feasibility, not just `tier_idx+1`.
9. **API plane doesn't exist in Rust** — only host `uno_apply.py`. → eventual plane impl.

## Build order
1. **`plane.rs` decision core (THIS step):** `PlaneId`, `TaskKind`, `Findings`, `classify_task` (with the
   routing-correction), `plane_applicable` (feasibility), `preferred_order(kind)`, `pick`, `switch`
   (feasibility-aware, switch-back capable) — PURE + unit-tested. Joins the scattered decision logic in one
   place. Reuses `StepOutcome` as the switch trigger.
2. **Integrate:** in `agent_loop`, replace the goal-verify-fail HANDBACK with `governor.switch()` when a
   feasible alternative exists; feed the supervisor's `PerceptionBlind`/escalate into it; thread `Findings`
   from `discover_environment`.
3. **Plane impls behind one trait:** wrap CLI(+back-door)/a11y/CV/pixel as `Plane { perceive, act,
   applicable }` (generalizing `DynamicActuator`); add the `set_config` back-door verb.
4. **API plane:** port `uno_apply.py` into a Rust API plane (the M2 work), registered as the top in-app plane.

Step 1 is the decision brain; 2–4 are wiring. The governor is **rails** — the model never picks the plane.
