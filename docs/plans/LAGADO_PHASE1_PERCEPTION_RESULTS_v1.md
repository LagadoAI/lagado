# Lagado — Phase 1 (Multi-Sense Perception) Build Results

**Date:** 2026-06-17 · **Source plan:** `research/lagado_perception_build_plan.md` (multi-sense fusion + DOM).
**Scope of this doc:** what was built, measured, and decided for Phase 1 (CV fusion). Phase 0 (latency
gate) and Phase 2 (OmniParser captions) and Phase 3 (DOM) are NOT done — status at the bottom.

---

## Headline result

**The CV pick-rate gate PASSES: a11y + capped-CV is identical to a11y-only.** A 10-task live-VM
battery run twice (CV off vs CV on), same hardened reset, same order, produced the **exact same
5/10 pass set — every pass and every failure matched**. The label-aware cap means CV introduces
**zero selection regression**. The plan's gate criterion ("do not proceed until a11y+CV ≥ a11y-only
on pick rate") is satisfied — they are equal.

This is the expected outcome by design: in Phase 1 CV boxes are **inert to selection** (they carry
no label, so they match nothing in `goal_matches_any` / `best_match_token` / relevance ranking). CV
becomes *selectable* only in Phase 2 when captions give the boxes labels. Phase 1's deliverable is
the **proven live pipeline + the cap infra + the Phase-2 seam**, not a capability gain.

---

## What was built (committed)

- **Phase 1a — arbiter owns label provenance** (`perception/arbiter.rs`). `FusedElement` carries
  `label: Option<String>` + `LabelSource{A11y,Caption,Ocr,None}`; `resolve_label()` implements the
  priority chain (a11y > caption > OCR > None). Labels flow THROUGH `fuse()` rather than being looked
  up externally. Phase-2 captions slot in without re-plumbing. +7 tests.
- **Phase 1b — live CV sense wired** (`agent.rs` ~584, `perception/cv_proposer.rs`).
  `propose_frame()` tiles the decoded frame on the delta grid and runs the classical-CV proposer per
  cell. Read from `FRAME_PATH`, **fail-open** to a11y-only on any frame error.
  - Measured: **~60 ms / frame** (classical CV, cheap; per-frame is fine on cost).
  - Measured: **648–956 CV boxes** on a real 1280×800 desktop (→ ~540 after IoU-dedup). This is the
    position-bias flood the plan warned of.
  - **`LATE_BAND_CAP = 64`** (`perception/selection.rs`) bounds the rendered list. It is
    **label-aware**: a tertiary sort key sinks label-less boxes below labeled ones, and the cap
    drains the *front* (least-relevant), so it sheds inert CV boxes first and **never drops a
    goal-matching labeled target** (which always sorts to the relevance tail). This honors the §5
    lossy-shortlist lesson by construction. `selector_grammar` now takes the rendered count (was the
    raw fused set) so the grammar offers exactly the tokens shown. +6 tests.
  - `LATE_BAND_CAP` is a **tunable**, model-dependent (attention band), and should become
    governor-supplied (invariant #9) rather than frozen.
- **Phase 1c — `LAGADO_CV_DISABLE` toggle** (`config.rs::cv_enabled`). Kill-switch + the gate's
  on/off measurement instrument. Honored in all build profiles.
- **Harness** — `stress_test` gained a QMP **frame feed** (keeps `FRAME_PATH` fresh headless, since
  production refreshes it via the Tauri `capture_frame` IPC which headless has no UI to drive), a
  **hardened reset** (SIGKILL app set + alt+F4 dialogs, drop probe files), execution-verified
  `verify_cmd` tasks (filesystem-checked click→type→Enter chains), and a complexity ladder.

All unit tests green (242 lib). Bins build.

---

## Complexity ceiling found (NOT CV-related — present in BOTH modes)

The battery surfaced real system failure modes, independent of CV:

1. **`menu-then-terminal` fails (1 click)** while `menu-then-filemanager` and `menu-then-browser`
   PASS. The agent opens the menu but does not complete the 2nd-step Terminal launch. A real
   2nd-step selection/sequencer issue specific to the terminal path — **worth a focused look**, but
   it is not CV (fails identically with CV off).
2. **Modal-dialog trap.** Launching the Mail Reader (no mail app configured) pops a "Choose
   Preferred Application" modal. The agent gets **stuck at 0 clicks** — it cannot recover from an
   unexpected modal. A genuine robustness gap.
3. **State leak between tasks.** Firefox "Restore Session" and the mail chooser leaked across resets
   (mitigated by the hardened reset + reorder, not fully eliminated). The agent also can't recover
   when it starts on a non-clean screen (0-click escalate).
4. **Typing chains not yet cleanly measured.** `term-type-touch` / `term-type-echo` (click→type→
   Enter, filesystem-verified) were poisoned by the leaked mail modal in the first battery. Tasks
   were reordered (typing before mail, mail last) + Firefox SIGKILL added; the clean re-run is the
   immediate next measurement.

---

## ⚠ CV is NOT production-ready yet (open integration gap)

The live loop reads `FRAME_PATH`, but nothing in the agent core refreshes it per perception step —
in production it is refreshed only when the **UI polls the Tauri `capture_frame` IPC**. So today CV
would read **whatever frame the UI last captured: potentially stale and spatially misaligned with
the fresh a11y tree** → CV boxes at coordinates that no longer match the screen. The dormant
`visual_context` read (`agent.rs:436`) has the same latent coupling.

**Fix (next):** co-capture the frame in the perceptor — give `SshPerceptor.read_screen()` the QMP
handle so the a11y tree and the frame come from one moment. Until then, CV is harness-measurable but
not safe to ship enabled by default.

---

## Status of the rest of the plan

- **Phase 0 (CPU latency gate for OmniParser/Florence-2/EasyOCR)** — NOT run. The toolchain is
  entirely absent and the system Python is **3.14**, where torch/openvino/easyocr wheels likely do
  not exist yet. Needs a separate 3.11/3.12 venv. Gates Phase 2's per-frame-vs-fire-on-gap fork.
- **Phase 2 (OmniParser semantic sense)** — NOT started. This is where CV/vision boxes finally
  become *selectable* (captions → labels → the selection rails can match them). The arbiter seam
  (`LabelSource::Caption`) is already in place.
- **Phase 3 (DOM perceptor/actuator)** — NOT started (greenfield, own session).

## The ceiling is planning, not perception — TWO walls (diagnosed 2026-06-17)

Clean isolated runs (fresh VM, no state leak) show the perception/selection FLOOR is solid but the
agent hits two distinct **planning** walls, in this order. Action-typing fixes the second; only the
router fixes the first. They must not be conflated.

**Wall 1 — precondition (hit FIRST, blocks everything below).** `Launch the Terminal Emulator` on a
bare desktop fail-closes immediately (0 clicks): "Terminal Emulator" is inside the closed Applications
menu, so no candidate matches → stall → handback. The agent cannot infer the intermediate step ("open
the menu first"). This is what kills `implicit-terminal`, and (via state leak leaving the menu open →
the toggle click closing it) `menu-then-terminal`. **Fix = intent→capability router** (open the
container that holds the target). NOTE: a curated capability map is hand-built — a scale/maintenance
commitment for a CPU-consumer product. **This is an architectural decision for the user, not a silent pick.**

**Wall 2 — click-only execution loop (hit AFTER a target is reachable).** The sequencer feeds every
sub-goal to the same click-selection path, and `fail_closed`/`goal_matches_any` test for a *clickable
label* match. A `type the command: …` or `press Enter` sub-goal has no label → killed before the model
can act. **The agent literally cannot type or press keys as deliberate steps.** Also `decompose_goal`
only splits on connective markers (" then ", "; ", …), so "Launch X, type Y" conflates a click and a
keystroke into one sub-goal. **Fix = action-typed decomposition + action-aware execution** (Click via
the existing selection path; Type/Key as deterministic one-shot harness actions that bypass selection
and the grammar; `fail_closed` guards Click steps only; a no-selector "type into focused" path).
Verify against an EXPLICIT-step task that removes Wall 1:
`"Open the Applications menu, then click Terminal Emulator, then type touch /tmp/probe, then press Enter"`.

Implication: action-typing greens **zero** of the *current* failing tasks (all hit Wall 1 first), but
it is a real, foundational capability gap — the router needs it too (after opening the menu and clicking
the app, you still have to type). It is bounded and committable; the router is bigger and gated on the
user's capability-map decision.

## Immediate next steps

1. Clean re-run of the reordered typing tasks (the highest untested complexity).
2. Investigate `menu-then-terminal` 2nd-step failure (non-CV; affects the core sequencer).
3. Production frame-sync (co-capture in the perceptor) before CV ships enabled.
4. Phase 0 environment (3.11/3.12 venv) to unblock Phase 2.
