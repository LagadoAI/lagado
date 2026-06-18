# Lagado — Execution-Hardening Arc (2026-06-18)

**Scope:** one session, from the multi-sense-perception plan through the act≠effect line. All committed
(`2658429..302ca51`), pushed, 254 lib tests green. Suite at the real **9/10 ceiling** (only the
structurally-unwinnable no-mail-app task fails). This is the §2.20 record; CLAUDE.md has the summary.

## The through-line

Every bug fixed this arc was the **same shape: a deterministic decision about the world made without
observing the world.** `structural_change` asserted "any change = progress"; a stability-only settle
asserted "this frame is settled"; a fixed 3s ceiling asserted "3s is enough." The arc replaced each
guess with an observed signal. The final commit (observe-until-quiet) removed the last clock from the
inner control.

## What was built (in order), each verified live

1. **Phase 1 multi-sense perception.** Arbiter owns label provenance (`LabelSource` a11y>caption>OCR>None,
   `resolve_label`); live CV via `cv_proposer::propose_frame` fused in `agent.rs` (fail-open); the
   label-aware **`LATE_BAND_CAP=64`** (sheds inert label-less CV first; never drops a goal-matching
   labeled target — honors the §5 lossy-shortlist lesson). **Gate PASSED: a11y+CV == a11y-only, zero
   selection regression** — CV is inert until Phase-2 captions, ships as free coverage.
2. **Board-informed planner (Wall 1)** — `plan_goal`: an upstream LLM step, informed by learned skills
   (`skill_library.retrieve`), that expands an IMPLICIT goal's preconditions ("Launch Terminal" →
   `[Click Applications menu, Click Terminal Emulator]`). Explicit "X then Y" keeps the deterministic
   `decompose_goal`. **store-vs-INFLUENCE:** memory shapes the PLAN, never the click (inv #10). Fixed the
   implicit-task fail-closed stalls (implicit-terminal/browser/filemanager ✗→✓).
3. **Action-aware executor (Wall 2)** — sub-goals carry a `SubAction` (`classify_subgoal`): Click → the
   selection loop; **Type/Key → deterministic one-shot through the safety gate, fire-and-advance** (no
   selection/fail-closed; type → `selector="focused"`). The loop was click-only before. Plus the
   intent-classifier **deterministic fast-path** (`hydra::opens_with_action_verb`): an action-verb-leading
   message routes Interactive without asking the weak 1.2B (which misrouted long action chains to
   `Intent::Chat` → a one-shot that silently did nothing). Verified filesystem: term-type-echo created
   the file.
4. **Strategist directives** — lexical-union ranker (`tokens_match`: substring/prefix ∪ exact, max; the
   directive's premise that the ranker was embedding-based was FALSE — it's already lexical, ColBERT
   stays out of the action path) + the memory-isolation guard test (`build_executor_prompt` is
   inv-#10-isolated by construction).
5. **act≠effect — the spine.** Reframe (advisor): the failures were NOT success-detection but
   **entry-state** (menu already open → the toggle click closes it) and **transition-race** (read
   mid-paint). Probe A confirmed: menu-then-terminal passes on clean entry, fails on leaked-open entry.
   Build order = settle → postcondition → (precondition banked):
   - **Settle gate** then **manifest-settle** (`read_settled_screen`): wait for the effect to change from
     baseline AND stabilize. Closed the transition race.
   - **§2.15 POSTCONDITION** (`effect_confirmed` / `EffectClass`): advance on the action-class structural
     signature, not any delta. `Open` confirms only when elements APPEAR (direction-aware: toggling an
     open menu shut no longer false-advances); `Activate` = any-change catch-all. **Fixed menu-then-terminal.**
   - **observe-until-quiet** (`observe_until_quiet`) replaced the fixed settle ceiling: terminate on an
     OBSERVED signal — N consecutive settled observations — never a clock. `settling_active` = a11y churn
     OR frame-delta pixels > noise (reuses `DeltaDetector`). The cost path: poll the cheap frame-delta each
     interval, read a11y (ssh) only at frame-quiet checkpoints. Only the far-outer backstop is a clock.
     **Fixed the term-type cold-start race: 6/6 across two 3× runs (was 2/2-then-fail).**
6. **CV production frame-sync** — `Perceptor::capture_frame()` (default no-op; `SshPerceptor` does a QMP
   screendump to `FRAME_PATH`), called in the loop at the perception instant on the settled state, so CV
   reads an in-sync image not a stale UI-polled one. Harness QMP feeds removed (capture_frame replaces
   them; a feed would starve it for the single-client QMP socket). **CV is now production-ready, shippable
   enabled** — verified zero degradation to a11y-only across a run.

## Acceptance — honest status

- ✅ term-type-touch reliable: **6/6** across two 3× runs (the headline race, gone).
- ✅ 9/10 ceiling holds; every task that ran passed 3/3 — no regression.
- ✅ in-progress-vs-stuck discrimination has explicit unit tests (`settling_active`).
- ✅ doctrine: inner control terminates on observed quiet; only the far-outer backstop is a clock.
- ✅ "Launch dissolves?" checked empirically (`advance_focus` instrumentation): the Terminal-click advance
  fires on `focus==Terminal`. One harmless `focus==Desktop` blank-gap advance, only on a goal's last step.
- ⚠️ **NOT exercised:** an extreme injected-slow-action test (30s); the cold-starts verified are
  multi-second (Firefox launch is the slowest and passed 3/3). A live hung-app escalation (the chain
  settle→no-confirm→should_cutoff is in place + unit-discriminated, not driven live).
- ⚠️ Suite runs slow (observe-until-quiet polling + the Option-2-able menu oscillation) — timed out partway
  at runs=3, but every task run passed.

## Breadth probes (2026-06-18) — the honest picture beyond menu-launch

The 10-task suite is almost all "open menu → launch app" on one XFCE VM; 9/10 there is NOT general
competence. Added 4 probes that hit surfaces never exercised. **Result: 11/14 — three of four new
surfaces broke**, confirming the agent is far narrower than the suite implied:

- ✅ **Type fidelity** (`probe-type-quoted`, `echo 'ok done'`): a quoted multi-word string survived intact.
- ✗ **Sequencer depth** (`probe-chain-2files`, two `touch`→Enter pairs = 6 sub-goals): the SECOND file
  was never created. The chain advanced through sub-goal 5 ("type touch /tmp/lagado_b") per the trace but
  the second command didn't land — the deterministic Type/Key chain does not hold reliably past the first
  command (focus/timing after the first Enter). **Real limit: multi-action chains break beyond ~4 steps.**
- ✗ **Submenu navigation** (`probe-submenu-settings`, "Open the Settings Manager"): a **FALSE PASS** — the
  predicate "Settings" matched incidentally while `focus=Thunar` (it opened the *file manager*, not
  Settings). Two findings: submenu nav is unproven/broken, AND weak success predicates manufacture false
  greens (a verification-quality lesson for the whole suite).
- ✗ **Modal-dialog recovery** (`probe-dialog-recovery`, Mail chooser → Escape → recover to terminal): 6
  clicks, 78s, ended in Thunar — **the agent cannot recover from an unexpected modal** (hole #5 confirmed,
  not a one-off). The mail chooser isn't an "unwinnable task" to exclude; it's a representative trap the
  agent has no escape behavior for.

Implication: the real next work is breadth/robustness (chain depth, submenu reveal, modal escape,
better predicates), not another rail tuned to the menu. Option 2 (toggle-oscillation cleanup) is
correctly deprioritized against these.

## Banked / next

- **Option 2 — precondition already-satisfied SKIP** (cleans the toggle oscillation). Now SAFE to build —
  the postcondition is its safety net. **Non-negotiable from the advisor:** skip ONLY on a UNIQUE
  `best_match_token` in a SETTLED candidate set (never mere presence) — a coincidental/ambiguous match
  must refuse to skip and fall through to the normal click. The postcondition net does NOT cover a wrong
  skip that coincidentally confirms; uniqueness-on-settled is what makes it safe.
- **§2.15 `Launch` class** — mostly dissolved into observe-until-quiet; build the focus-to-new-window gate
  only if a real blank-gap failure appears.
- **Slow-action + hung-app** live acceptance tests (the two un-exercised cases above).
- **Phase 2** — OmniParser captions (where CV finally becomes selectable; the `LabelSource::Caption` seam
  is in place). Gated by Phase 0 (CPU latency; toolchain absent on Python 3.14 — needs a 3.11/3.12 venv).
