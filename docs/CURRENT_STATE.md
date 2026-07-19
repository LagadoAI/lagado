# Lagado harness — current state (2026-07-14)

**Supersedes**: `HARNESS_COMPLETE_MAP_2026-07-06.md` and `RUST_HARNESS_COMPLETE_MAP_2026-07-08.md`
(both kept, both stamped SUPERSEDED — they're the diagnostic record that *motivated* several
of the changes below, not current fact). Compiled from a direct line-by-line read of the harness
source (`lagado-agent/src/**`, `lagado-agent/python/osworld/**`, `lagado-agent/python/reflex/**`)
on 2026-07-14, cross-checked against `config.rs`'s feature flags where behavior is gated.

This file answers one question: **if you changed nothing else, what does the shipped binary
actually do today, and what is built-but-not-yet-trusted?** Everything below is either (a) directly
observed in the code, or (b) marked UNVERIFIED-THIS-PASS where I read the flag/config but did not
trace the live call site to confirm wiring.

---

## 1. The perception stack IS a fused, multi-sense architecture — say that plainly

This is not a grab-bag of half-built senses. `perception/arbiter.rs::fuse()` takes up to four
inputs — AT-SPI2 accessibility boxes, classical-CV proposals (Canny + connected components),
browser-DOM boxes (CDP read), and per-patch vision embeddings — and IoU-deduplicates them into
one ordered `FusedElement` set, with explicit label-provenance priority (a11y > DOM > caption >
OCR > none) and a documented, tested edge-fuzz/mean-pool contract for patch attachment. This is a
genuine "richest-first" perception ladder, not a fallback stub with extra fields. It is heavily
unit-tested (label rescue, DOM/CV self-dedup, deterministic ordering, overview-tile exclusion).

**What's live by default vs gated** (from `config.rs`, cross-referenced against the flags'
own doc-comments):

| Sense | Default | Note |
|---|---|---|
| a11y (AT-SPI2) | **ON**, always | the floor; every other sense fuses onto this |
| CV (Canny/connected-components) | **ON** by default since the 2026-07-08 sensorimotor redesign | `cv_enabled()`; kill-switch `LAGADO_CV_DISABLE=1`. Selection safety is mechanism-guaranteed (label-less boxes can't goal-match; `LATE_BAND_CAP` sheds them first). **UNVERIFIED THIS PASS**: I read the flag and its comment, not the live `agent_loop` call site that would confirm CV boxes actually reach `fuse()` today (the 07-08 complete-map found them computed-and-discarded; the flag's comment says that was fixed same day — reconcile before relying on this row for a decision). |
| DOM (browser CDP) | **OFF** | `dom_enabled()`, `LAGADO_DOM=1`. Built (`arbiter.rs` has full `DomBox`/`LabelSource::Dom` support, tested), gated pending A/B on official tasks. |
| Vision (caption/OCR) | **NOT BUILT** | arbiter's `LabelSource::Caption`/`::Ocr` variants exist; no captioner or OCR is wired to populate them. Patch embeddings (`patch_embd`) attach but aren't consumed downstream yet. |

## 2. The execution core — proven, adversarially tested

`agent.rs`'s sequencer is the most-hardened part of the codebase:
- **Goal decomposition** splits only on explicit sequential markers (deterministic — the model
  cannot be trusted to decompose, measured directly: it emits a spurious `complete` even when
  handed its own unfinished plan).
- **Effect confirmation** (`EffectClass::Open` vs `::Activate`) is direction-aware: a menu that
  toggles itself *shut* does not read as "opened." `observe_until_quiet` replaced a fixed settle
  timer with an actually-observed quiescence signal (a11y churn OR pixel-delta above a noise
  floor), with a far-outer timeout as backstop only.
- **`complete_goal` fails closed on an empty check set** — this is the fix for the real
  integrity bug an adversarial audit caught (2026-07-10): the false-pass generator was this
  exact function claiming success vacuously when `goal_postconditions()` returned nothing.
  Confirmed fixed in the code as read.
- **Reform is bounded and conservative**: a corrected shell command can never introduce chaining/
  redirection/substitution the original didn't have (`reform_is_conservative`), and a
  deterministic equivalence-class substitution (`python`→`python3` etc., checked against what's
  actually installed) runs *before* the LLM reform is even tried.

## 3. OSWorld harness — one domain proven, others honestly unbuilt

Per `CLAUDE.md`'s own last-updated status (2026-07-11): **19/47 (~40%) on LibreOffice Calc**,
the one domain with a full plane (`uno_ops.py`, 22 op kinds, resident daemon, sound falsifiers,
independent-re-derivation corroboration). Every other domain scoring near-zero in the full
369-task run is an honest *"no plane yet"* build-map, not a comprehension verdict — confirmed by
reading `writer_ops.py`/`impress_ops.py`, which exist, are similarly rigorous, but are explicitly
marked **UNVALIDATED against a live guest** in their own docstrings (built 2026-07-10, probe-tested
offline only) and are gated off by default (`writer_solver_enabled()`, `impress_solver_enabled()`
both default OFF pending A/B).

The Calc plane's integrity machinery (read directly, not summarized from docs) is real: sound-only
falsifiers (can prove a fault, never confirm correctness), a divergence-resample step before full
retry, and read-only corroboration via an independent second derivation that must agree on which
columns/cells it touched. `battery_p3.py`'s adversarial test (a deliberately wrong-but-plausible
formula) demonstrates the corroboration step catching what falsifiers alone would miss.

## 4. Gated-pending-measurement capabilities (built, off by default)

All five behind explicit env flags, each with a `config.rs` comment stating the ablation
contract ("joins the default path only after its A/B delta on official tasks is measured"):

- `LAGADO_BACKDOOR` — typed `set_config`/`run_sibling` route-around-the-GUI executor
  (`back_door.rs`), with a read-back falsifier per backend (dconf/gsettings/ini-file).
- `LAGADO_DOM` — browser CDP perception sense (see §1).
- `LAGADO_CALC_SOLVER` / `LAGADO_WRITER_SOLVER` / `LAGADO_IMPRESS_SOLVER` — route a
  focused document through the respective UNO plane instead of the general GUI-click loop.

This is a real, working ablation discipline — not neglect. The gap it leaves is that the
*shipped* default-path surface is narrower than "everything in the repo," which matters for
anyone (a hiring manager, a technical co-founder, future-you in six months) trying to answer
"what does this actually do right now" from the code alone.

## 5. The reflex/CfC layer is real, disciplined, and currently unconnected to the Rust harness

`python/reflex/` (settle-monitor CfC, `train_cfc.py` etc.) is genuinely good ML engineering —
timer-null baseline, fail-closed promotion gate, held-out cross-validation, a documented instance
of catching the model learning a clock instead of reading pixels. But per the 2026-07-08 Rust
audit (preserved in the superseded map, and not contradicted by anything read this pass):
**`liquid.rs` is a model-roster stub that always returns the 8B. No CfC, no temporal/continuous-
time mechanism exists in the Rust harness.** The reflex work and the Rust agent are two separate
systems today, not one integrated pipeline.

## 6. What this doc replaces in `CLAUDE.md`

`CLAUDE.md`'s "Status" and "Harness doctrine" sections (as of 2026-07-14) are a chronological
accretion of session-by-session findings from 2026-06-14 through 2026-07-11 — each entry was
correct when written, several are now superseded in place by later entries in the *same* file
(e.g. "7/30 gold" superseded by the 24/368 full run, the "0 false-pass" integrity claim
superseded by the ≥6 audit finding). That history has real value and is preserved verbatim in
`docs/plans/` and the dated map files — it should not be read as current-state documentation.
This file is current-state; CLAUDE.md should point here first.
