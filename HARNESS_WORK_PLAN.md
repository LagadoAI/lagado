# Harness Work Plan (2026-07-11)

The steering summary for the harness phase — the live work queue. CLAUDE.md carries the
architecture; this file carries what's next. The full-run record and its adversarial audit are in
`docs/osworld/FULL_369_RESULTS_2026-07-10.md`.

## Current state

- **Full OSWorld run (all 369 tasks, official `env.evaluate()` only): 24/368 scored.** But that
  number is NOT yet trustworthy — see the audit below. The one domain built out end-to-end
  (LibreOffice Calc) reached **19/47 ≈ 40%** on the official grader; every other domain sitting near
  zero is an honest build-map (no plane built yet), not a comprehension verdict.
- **OP-vocab is built** (22 calc op kinds — pivots, charts, freeze, csv, transpose, reorder,
  conditional, locale, dedup, sort, zoom, pdf, …) and reachable from the general loop via the
  calc-solver rung. The old "op-vocab is the fracture line" framing is retired.
- Branch: `Harness` (now the default). `main` is pre-OSWorld.

## THE GOVERNING EVENT — the 2026-07-10 adversarial audit (Opus, told to REFUTE us)

An independent model, briefed to overturn our conclusions, dug the raw traces and found our
integrity claim was **wrong in count, identity, and direction.** This reframes everything downstream.

1. **False passes: ≥6, not the 1 we reported** (multi_apps 7e287123/02ce9a50/68a25bd4/a503b07f,
   vs_code 6ed0a554, libreoffice_calc 6054afcb) — and a FLOOR, not a ceiling (117/368 tasks have no
   trace coverage at all).
2. **The generator (FIXED):** `complete_goal` claimed success VACUOUSLY when `goal_postconditions()`
   returned empty — which it does for any goal not create/delete/git/exec-shaped. Calc broadly
   exposed (20/47 goals get no derivable check). Now fail-closed.
3. **The one we flagged (deec51c9) was NOT a false pass** — it was a false-FAIL (over-cautious FAIL on
   a feasible task = lost gold). Opposite direction. Also FIXED: a sub-plane no longer declares a
   whole-agent FAIL.
4. **The atlas's false-pass detector never actually ran** — it greps the trace for a string that only
   ever goes to stdout. The integrity guard was structurally blind.
5. **The failure atlas categorizes by DOMAIN, not evidence** — `no_plane 134` / `composite_fail 96`
   are the domain histogram relabeled, polluted by 3 setup failures + ~23 blank-output no-engage tasks.
   The fix-classes are NOT measurements.
6. **The run mixed 3 flag configs** — 24/368 and even calc 19/47 are not single-config numbers.
7. **The "build Writer+Impress next" priority is NOT supported by this data.** Browser/web appears in
   ~25 of 101 multi_apps tasks vs Impress's ~8; chrome actuation may be the bigger lever. Unknowable
   until the instrumentation can answer where each task actually died.

## Work queue — in the audit's own recommended order (do NOT reorder)

### Phase 1 — Instrumentation first (BEFORE building any new plane)
The capability signal is currently contaminated; building on it repeats the mistake.
1. **Atlas reads stdout, not just chronos** — so the false-pass detector actually fires. Cross-check
   EVERY completion assertion against its real score.
2. **Per-task stderr + clean per-run chronos** — categorize failures by evidence (which plane engaged,
   where it died), not by domain default. Fix the 70-char-prefix trace join (it pulls stale segments).
3. **Separate infra/setup failures and blank-output no-engage tasks** out of the capability denominator.

### Phase 2 — Re-audit
4. With honest instrumentation, re-run the failure atlas on the full 369 and get the TRUE false-pass
   count and evidence-based fix-classes. Only this tells us the real build priority.

### Phase 3 — Build the priority the re-audit actually names (not the guessed one)
- Writer + Impress UNO planes are BUILT but UNVALIDATED (flag-gated default-OFF). Validate via the
  engineering-iteration loop: run a few real tasks → read the failure trace → refine → re-run.
  First checks: the Impress color-name→RGB table (flagged false-pass risk) and Writer
  subscript/highlight serialization (already found empirically during the build).
- Chrome CDP actuation (the DOM floor gives sight; add action) — candidate top lever per the audit.
- Rebuild the binary ONCE for the Rust routing (already cargo-check-clean, not yet built).

### Integrity discipline (permanent)
- Official evaluator only; the harness never grades itself. Frozen prompts, held-out where possible.
- Zero false-pass is the bar and it is MEASURED adversarially, not asserted. "I can't verify this" is
  an honest handback, never a silent success.
- No leading: fixes are general mechanisms keyed to structure, never keyword blocklists of failures
  we happened to see (a mistake made and reverted this cycle — see the integrity commits).

## Success criteria
- **72% on real OSWorld with a ≤7B model, treated as a FLOOR** (greed doctrine), zero false-pass held.
- Every reported number reproducible from a single, logged flag config — no more config mixtures.
- Model-vs-harness attribution measured, not asserted.
- Results human-verifiable and audit-survivable — the number means what it says.
