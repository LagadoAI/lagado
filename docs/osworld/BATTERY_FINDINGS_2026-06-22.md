# Insight Battery — calc authoring conditions (2026-06-22)

Driver: `docs/osworld/battery_calc.py`. Model: Qwen2.5-Coder-7B-Instruct (:8080, unchanged weights).
Apply path: native session (uno_daemon) — proven floor untouched. Scorer: real `env.evaluate()`.
Logs: `/tmp/lagado_battery/calc_035f41ba.jsonl`, `p1_calc.log`. Anchor: `anchor_os.log`.

## What OSWorld structurally cannot give (the reason for the battery)
Binary end-state only · every task has a deterministic oracle (so it can NEVER test false-pass on
unclassifiable output) · clean canonical inputs · fixed conditions · no calibration of "done".

## ANCHOR — os-domain transfer (existing harness, existing ladder)
`os:4127319a` (count php lines), `os:e0df059f` (rename dir), `os:5ced85fc` (append+save) → **0/3**.
Agent reached *"couldn't form a valid grounded action — handing back"* — it FAIL-CLOSES (no fabrication)
but the current ladder does not route these to a working rung. Transfer past calc = 0 today. The trace
gives the *why* (no grounded action formed) — the localization the binary score hides.

## P1 — CONDITIONS vs CAPABILITY (task 035f41ba, N=3, same weights)

Task: fill Gross Profit = actual sale (after returns) − all expenses; then Sheet2 Year_Profit =
Year & "_" & GrossProfit. Real columns: A Year, B Sales, C Sales Return, D Discounts and Allowances,
**E Net Sales (subtotal)**, F Materials, G Labor, H Overhead, **I Total COGS (subtotal)**, J Gross Profit.
Gold J = 55000, 47662, 53451, 60819 (E, I left empty). Gold formula `=B2-C2-D2-SUM(F2:H2)`.

| Condition | gold | false-passes | Sheet1 gross-profit authored |
|---|---|---|---|
| **A** (bad: raw structure blob, A1 coords, raw formula, one-shot, no read-back) | **0/3** | **3/3** | col **I** (wrong), `=B2-C2-D2-E2-F2-G2-H2` — **double-counts E (Net Sales subtotal)**, and **clobbered every header** |
| **B** (good: labeled candidates, reason-then-emit, names→A1 fail-closed, read-back falsifiers + retry) | **3/3 (stdev 0.00)** | **0/3** | col **J** (right), `={Sales}-{Sales Return}-{Discounts and Allowances}-{Materials Charges}-{Labor Charges}-{Overhead}` → `=B2-C2-D2-F2-G2-H2` = **byte-identical to gold; skips E and I** |

**Condition B lands a DETERMINISTIC FULL-TASK GOLD (3/3, stdev 0.00, 0 false passes).** Same weights,
same scorer as A — only the authoring conditions differ. Sheet2 = `['2015_55000','2016_47662',...]` matches
gold exactly. This is the user's thesis settled: A's failure was **conditions, not capability**; the 7B,
given good conditions, solves the whole task reliably.

**Condition B's Sheet1 is exactly the gold** (right column, values 55000/47662/53451 match, subtotals
correctly skipped, headers intact). Same model, same scorer — only the authoring conditions changed.
This is the user's thesis CONFIRMED on the discriminating sub-task: the gross-profit error is a
**conditions** failure (machine coordinates + one-shot strip the semantics that prevent double-counting),
**not a capability** wall. Given names to reason over, the 7B got the arithmetic, the column, and the
subtotal-avoidance right.

### What it took to close the full gold (the Sheet2 cross-sheet string op)
The first B build nailed Sheet1 but fail-closed on Sheet2. Three driver-side fixes (all general, none
task-specific) closed it — each found by reading the attribution/read-back log, not guessing:
1. **Workbook-wide unique name resolution** — a bare `{Gross Profit}` on Sheet2 resolves to its UNIQUE
   referent anywhere in the workbook (Sheet1!J), else fail-closed. Fixed the resolve_fail.
2. **Reference-aligned extent** — a computed column spans the data extent of the sheets its formula
   REFERENCES, not the (empty, 1-row) fresh target sheet. Fixed the degenerate `A2:A1` range that also
   defeated the extent falsifier and produced a silent-wrong (all-zeros) pass.
3. **Quote normalization (harness owns syntax)** — the model emitted `'_'` (single quotes), which
   LibreOffice silently evaluates to **0** (not an error face, so F1 couldn't see it). Normalizing `'`→`"`
   before apply is general syntax-ownership, consistent with the thesis (harness owns mechanical exactness).
   Plus a new SOUND falsifier: a text formula (`&`/`CONCATENATE`) that yields a NUMBER fired the retry loop.

Honest note on the path: fix #2 momentarily RE-INTRODUCED a false pass (silent all-zeros that passed the
incomplete falsifiers) — caught only because the battery counts false passes against the oracle. That is the
integrity instrument doing its job: it forced the sound text-formula falsifier rather than letting a
plausible-but-wrong result stand.

## P5 — calibration / integrity (the core OSWorld cannot measure)
- **Bad conditions FABRICATE done:** A self-reports done on all 3 runs; oracle says 0 → **3/3 false passes.**
  The naive signal "ops authored + no structural fault = done" is dangerous exactly as predicted.
- **Good conditions stay honest:** B fail-closes on the unresolvable Sheet2 name, self-reports NOT done →
  **0 false passes.** Fail-closed resolution + "falsifiers may falsify, never confirm" held: the harness
  never claimed a pass the oracle would reject.
- Falsifiers are sound-but-incomplete by design: A's wrong-but-well-formed formula fired NO falsifier
  (errors/extent can't see semantic wrongness). Correctness came only from the oracle — as intended.

## Driver bugs found + fixed (honest record)
1. Detector returned EMPTY headers — daemon `structure()` reads its headers field from the wrong row
   (`gotoEndOfUsedArea(False)` collapses to the bottom-right cell). Latent: never exercised because all
   prior golds were hand-driven with explicit A1 ranges. Fixed in the driver (explicit row-1 read);
   **daemon left byte-identical.**
2. Stale `detected` → new-sheet names unresolvable. Fixed: resolve at APPLY time against the live
   re-detected structure (uses the session's per-op observation properly).
3. Self-report counted fail-closed runs as false passes. Fixed: a resolve_fail ⇒ not done ⇒ not a
   false pass (honest failure).

## P1 de-confound (advisor-caught) — the clean A/B
The first A/B was confounded: Condition A still ran on the buggy detector (empty headers), so A was
*also* header-blind. Re-ran A with the SAME fixed detector B uses (real headers, still raw-blob + A1
coords + one-shot). Result: **A still 0/3.** Its formula became `=E2-(F2+G2+H2+I2)` — with headers it found
column J, but one-shot coordinate authoring **swept in subtotal column I (Total COGS)** alongside the
individual costs → double-count. B's name-mapping took exactly the instruction's enumerated expenses
{Materials, Labor, Overhead}. **The lever is named SELECTIVE MAPPING, not header visibility** — sharper than
the first framing, and de-confounded. (Honest: "A false-passes" was specific to the earlier harness; with
the current falsifier set, clean-A's broken Sheet2 trips a falsifier so it self-reports not-done. The
gross-profit error itself is a plausible-wrong no sound falsifier catches — that is P3's target.)

## P4 — sloppy-sheet robustness (narrowness fear, made countable)
`battery_p4_resolver.py` adjudicates resolution across 8 degradation classes at the RESOLVER (where a
mis-bind would occur). **TOTAL MIS-BINDS = 0.** The hard invariant holds: never a silent wrong binding.

| class | outcome |
|---|---|
| clean, case/whitespace noise, blank-spacer column | **resolve correctly** (robust) |
| title-row (headers not row 1), duplicate header, units-in-label (bare ref), synonym | **fail-closed (safe)** |

The narrowness risk is converted from *silently wrong* (catastrophic) to *fail-closed* (safe, recoverable).
Residual narrowness is **coverage, not safety** — the fail-closed classes are the next lever (header-row
detection + candidate-selection-by-index so the model picks "Sales ($)"/"OH" off the card), and they fail
SAFELY until built. Note units-in-label resolves when the model copies the exact header off the candidate
card — evidence the candidate-selection lever would lift coverage.

## P3 — false-pass on a plausible-wrong result (the integrity case OSWorld cannot construct)
`battery_p3.py`, gross-profit column, ground truth = known gold J. A wrong-but-well-formed formula (drops
Overhead) is the test; an independent re-derivation (temp 0.6, fresh reason→emit) is the discriminator.

| scenario | oracle correct | sound falsifiers | naive "done" | would-false-pass | corroborated → **VERIFIED** |
|---|---|---|---|---|---|
| CLEAN  | True  | none fired | True | **False** | agree → **True** (no cry-wolf) |
| POISON | False | **none fired** | True | **True** | **disagree → False** (honest abstain) |

POISON drops one term → values `[57000, 49562, …]`: plausible, numeric, full extent, no error → **passes
every sound falsifier**, so a naive "no-fault == done" harness FALSE-PASSES with confidence. Independent
re-derivation got the right formula, the values disagreed, so corroboration → **VERIFIED=False**: the
harness ABSTAINS instead of fabricating a pass. On CLEAN both derivations agree → VERIFIED=True (doesn't
cry wolf). **Honest caveat:** corroboration is confidence, not proof — two derivations sharing the SAME
error (correlated) would falsely corroborate; the mechanism REDUCES false-passes, bounded by error
correlation. And the POISON was injected (this tests the verification layer's honesty, not the model's
error rate). The general no-oracle integrity property: certify only the corroborated, abstain on contested.

## Robustness lesson — "we are not prompt engineers" (P4 coverage build)
Building P4 coverage levers surfaced that the 035f41ba gold was NOT robust — it rode on a borderline model
choice. The model picks column NOTATION on a knife's edge: sometimes `{Sales}` (header), sometimes `{B}`
(column letter). The golden 3/3 happened to win the coin flip; a downstream change flipped it to `{B}`, which
the resolver REJECTED (header-only) → fail-closed → gold lost. A host-side probe confirmed: given the cards,
the model emits `{Sales}` deterministically 4/4 — so the live difference was real but the deeper point is the
choice is FRAGILE. Lesson: correctness was leaning on prompt phrasing, which is not the moat.

**Structural fix (deterministic, not prompt):**
- **Notation-robust resolution** — the resolver accepts ANY unambiguous notation for a column: exact header,
  column LETTER (`{B}`), or index (`{#N}`). `={B}-{C}-{D}-{F}-{G}-{H}` and `={Sales}-{Sales Return}-…` both
  resolve to `=B2-C2-D2-F2-G2-H2`. The harness owns NOTATION; the model owns only the comprehension (which
  columns). Sound: letters/indices are unique; headers fail-closed on dup; a non-existent letter fail-closes.
- **Auto-create target column** — "fill the Year_Profit column" no longer depends on the model remembering to
  add the header; if the target doesn't exist the harness creates it (first empty col, header at the header
  row). Target-only — input refs never auto-create (no-mis-bind invariant preserved).
- **Reverted a self-inflicted false pass** — a `written_all` union-across-retries I added turned a partial
  completion into a false pass; reverted (under-report on partial is safe; over-report is not). The integrity
  counter caught my own regression again.
Result: 035f41ba B back to **3/3 stdev 0.00, 0 false passes — and now robust** to notation + missing-header.
P4 coverage shipped: **header-row detection** (don't assume row 1). `{#N}` index kept as latent resolver
support (not advertised in-prompt — advertising it destabilized the 7B's base behavior; a clean grammar-
constrained version is the future robust form).

## (a) BREADTH + corroboration in the loop — the sobering honest number
First breadth sweep (10/16, golden harness, NO corroboration): **2/10 GOLD, 5/10 FALSE-PASS, 3/10 honest-wrong.**
The single-task gold does NOT replicate broadly — comprehension on diverse schemas is the frontier, and it is
NOT gated on enumeration (42e0a640 enumerates its inputs and still fails). The dominant problem is the FALSE
PASS (50%): sound falsifiers pass, oracle says wrong, harness claims done. (Sweep hung on task 11 — no per-task
timeout; added a 420s SIGALRM ceiling.)

Wired **corroboration** (P3 mechanism) into the main loop and re-ran the 5 prior false-pass tasks + gold anchor:
| task | before | after corroboration |
|---|---|---|
| 04d9aeaf, 26a8440e | FALSE-PASS | **ABSTAIN** (caught) |
| 1e8df695, 42e0a640, 0bf05a7d | FALSE-PASS | still FALSE-PASS |
| 035f41ba (anchor) | GOLD | **GOLD** (no false-abstain) |

**False passes 5 → 3: corroboration is a PARTIAL win.** It catches VARYING errors (the injected P3 poison) but
NOT CORRELATED ones — der1 (temp 0) and der2 (temp 0.6) reach the SAME wrong column footprint, agree, and the
false pass survives. The 3 survivors are two distinct problems: **0bf05a7d = op-vocabulary ceiling** (copy/
transform, not compute — no verifier fixes a capability gap); **1e8df695/42e0a640 = consistent comprehension/
structure errors** (same mistake twice). HONEST BOUND CONFIRMED: corroboration-by-temperature reduces, never
eliminates, false passes.

**Corroboration impl notes (load-bearing):** v1 = applying der2 to der1's ranges then "restoring" der1
CORRUPTED the scored doc (gold→0/ABSTAIN). FIX = READ-ONLY structural comparison: compare the SET of columns
each derivation REFERENCES per target (`formula_refset`, ranges EXPANDED so SUM(F2:H2)≡F+G+H — equivalent forms
agree, dropped/added column disagrees). NEVER modifies the doc. The integrity counter caught my corruption
regression (gold anchor → 0) — instrument working again.

**Forward levers (ranked):** (a) PERSPECTIVE-DIVERSE corroboration — der2 a different LENS (refute / "which
columns are NOT inputs"), not a temperature wobble — to break the correlation; (b) STOP OVER-CLAIMING — with no
oracle, "no-fault + corroborated" still isn't correct → report COMPLETED-UNVERIFIED, never assert an
unconfirmable pass (makes 0-claimed-false-passes true by construction); (c) capability (op-vocab copy/transform
+ comprehension), the slower frontier.

## OPERATIONAL / INFRA lesson (cost me ~10 turns)
NEVER `kill -9` an OSWorld run — it skips `env.close()` and LEAKS the guest: a root-owned `qemu` + `conmon`
orphan keeps holding ports 5000/8006/8081/9222, and every later `DesktopEnv` boot HANGS (sleeps forever, no
container). podman forgets the container so it can't clean up; the orphans are root-owned (rootful podman) so a
non-root `kill` fails, and the `--privileged --pid=host` rescue container is (correctly) blocked as a sandbox
escape → needed the USER to `sudo kill`. **RIGHT WAY: `kill -2` (SIGINT) → KeyboardInterrupt → `finally:
env.close()` stops the container cleanly (verified: exits ~10s, ports free, no leak).** Battery runner now has a
per-task SIGALRM timeout so one hang can't wedge a sweep.

## Status & next
- ✅ **P1 DONE** — conditions thesis confirmed; Condition B = deterministic full-task gold (3/3), 0 false passes.
- ✅ Built + proven: workbook-wide unique resolution · reference-aligned extent · quote normalization ·
  sound text-formula falsifier · read-back + retry loop. All general, daemon byte-identical, floor untouched.
- ✅ **P3 DONE** — plausible-wrong passes every sound falsifier (naive harness false-passes); independent
  re-derivation disagrees → VERIFIED=False (honest abstain), while CLEAN stays VERIFIED=True. No-oracle
  integrity property demonstrated; bounded by error correlation.
- ✅ **P4 DONE** — 0 mis-binds across 8 sloppiness classes; narrowness risk = fail-closed (safe), not
  silent-wrong. Residual is coverage (header-row detection + candidate-selection), which fails safely until built.
- ⏭ **Transfer** — lift the proven loop (candidates → reason→emit → resolve fail-closed → read-back/falsify →
  retry → corroborate) as the dynamic-intensity ladder across OSWorld; the anchor (os 0/3) is the starting line.
- ⏭ **Coverage levers** (earned by P4): header-row detection (don't assume row 1) + candidate-selection-by-index.
- ⏭ Live end-to-end fail-closed confirmation on a physically-degraded sheet (P4 invariant proven at the
  resolver + detector-modeled; a guest run would confirm the full detector→resolve→apply path).
