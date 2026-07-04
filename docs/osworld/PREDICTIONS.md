# Pre-registered predictions (written BEFORE running — anti-cherry-pick / anti-rationalization)

Rule: predictions are committed before the run; the run CHECKS them; deviations are signal, not to be
re-explained away. 035f41ba is CONTAMINATED (hand-iterated all session) — reported separately, never in any
headline number.

## Test 0 — de-leaded 035f41ba, N=3 (gates the whole P1 interpretation)
Change under test (FULLER than first pass — user caught that my first de-lead still led): der1's reasoning
prompt is now goal + observed columns + "Think step by step, then stop." REMOVED both (i) the "watch for
SUBTOTALS / don't double-count" hint AND (ii) the decomposition schema "which are inputs / target /
computation" (that decomposes the problem FOR the model). Zero solution scaffolding now. Question: does the
gold survive with NO guidance on how to approach it?

PREDICTION (committed 2026-06-23, before run): **genuinely uncertain — predict 1–3/3, lean "still golds most
runs."** The INSTRUCTION still enumerates the inputs ("discounts, allowances, material and labor charges,
overhead") and defines the base ("sales after deducting the returns"), so the information is present in the
task itself — but with zero scaffolding the 7B must structure the whole solve unaided, so real risk it drops.
FALSIFIER: ≤1/3 → the gold was scaffold-dependent → "P1 thesis confirmed" was partly my prompt engineering,
and the conditions-vs-capability claim must be reframed before building anything further. A drop here is the
MOST important thing to learn and must NOT be rationalized away.

### RESULT (2026-06-23): **0/3 — PREDICTION FALSIFIED.** The gold was SCAFFOLD-DEPENDENT.
With the leading prompts removed, the model scores 0/3 on the very task previously called a deterministic
gold. ⇒ "P1 thesis confirmed / conditions-not-capability" was OVERSTATED — the "good conditions" included ME
decomposing the problem and hinting the subtotal trap. That was prompt engineering, and it was load-bearing.
NAMED CONFOUND (does not rescue the claim — cuts the same way): the neutral prompt (`/completion` raw, no chat
template) made the model DRIFT — reasoning began "I'll give you the next instruction. Assistant: Let's
start..." (free-completing a dialogue). So part of the drop is prompt FORM, not pure comprehension — but that
only sharpens the user's point: the result is dominated by prompt phrasing, which is fragile and not the moat.
WHAT SURVIVES (prompt-independent, real): the deterministic machinery — notation-robust resolution,
fail-closed, auto-create, read-back falsifiers, P4 zero-misbind, P3 abstain. WHAT DOES NOT: the claim that the
7B *comprehends* these tasks. Honest verdict: the deterministic harness is the moat; model comprehension is
weak and was being propped up. Next must NOT be "find a better neutral prompt" (prompt engineering by another
name). Legitimate-but-user-gated: invoke the model via its proper CHAT TEMPLATE (mechanical correctness,
applied uniformly, no content hint) and re-test — if still 0/3, it's a genuine comprehension limit; if it
golds, the issue was prompt FORM not leading. Ungameable alternative: (b) completed-unverified.

Note on EMIT prompt: it lists the available operations + output format (the capability surface / "function
signatures") — kept, as the model must know its action set; it carries no answer hint. Flag for the user if
even that is considered leading.

## Test 0b — de-leaded 035f41ba via the CHAT TEMPLATE, N=3 (mechanical: is 0/3 real or a malformed call?)
Change: route the (already de-leaded) reasoning+emit calls through the model's chat endpoint
(/v1/chat/completions — llama.cpp applies the GGUF's own template) instead of raw /completion. Mechanical fix
applied uniformly to BOTH calls; NO content change, NO hints. One run only (no iterating).
PREDICTION (committed before run): **fixes the DRIFT (model emits real ops, not dialogue) but comprehension
stays hard un-led → predict ≤1/3.** Specifically: I lean that it will NOT reliably exclude the E/I subtotals
without the hint. Outcomes: golds 2-3/3 → 0/3 was a malformed-call artifact, model CAN do it asked correctly;
stays 0-1/3 → genuine comprehension/narrowness limit in the MODEL (template doesn't rescue) → stop blaming the
harness, the answer is a better/base model or (b) completed-unverified.

### ⚠️ CORRECTION (2026-06-23, after reading the RAW reasoning — the result below was MIS-DIAGNOSED).
"Comprehension ABSENT" is WRONG. I read a one-line log summary, not the raw reasoning. The actual JSONL shows
the de-leaded model REASONED CORRECTLY and COMPLETELY (Net Sales=Sales−Returns; COGS=Materials+Labor+Overhead;
Gross Profit=NetSales−COGS; Sheet2 concat) and EMITTED a complete helper-column decomposition (E, I, J + Sheet2).
It did NOT "get stuck at step 1." The 0/3 comes from THREE factors, none of them absent comprehension:
(1) the gold fills ONLY J and leaves Net Sales(E)/Total-COGS(I) EMPTY, but the evaluator `df.equals` the whole
sheet → the model's correct helper columns are extra cells → mismatch (DOMINANT); (2) a HARNESS fill bug —
`set_formula_range`'s `fillAuto` produced `B2-C3,B2-C4…` (only the last ref adjusts), wrong for every row past
the seed, masked on single-ref golds; (3) the model dropped ONE term, column D (Discounts and Allowances).
NET: the user's thesis (it's our approach + harness, not comprehension) is VINDICATED on this task. The
"genuine comprehension limit / stronger-or-base-model" conclusion below is WITHDRAWN. See INVESTIGATION_PLAN
"THE LOCATED FAILURE — CORRECTED 2026-06-23".

### RESULT (2026-06-23): **0/3 via chat template. [MIS-DIAGNOSED — see CORRECTION above.]**
All 3 runs authored gross profit = `{Sales}-{Sales Return}` (=B2-C2) — subtracted ONLY returns, OMITTED every
expense (gold = =B2-C2-D2-SUM(F2:H2)). Reasoning got stuck at "Step 1: calculate Net Sales" and called that
gross profit. ⇒ The chat template removed the drift (coherent reasoning, real ops) — so 0/3 is THE MODEL, not
a malformed call. Prediction held on headline (≤1/3 ✓, got 0/3) but my mechanism guess was WRONG: un-led the
model doesn't double-count subtotals, it UNDER-computes — meaning my leading prompt was supplying the WHOLE
multi-term formula, not just subtotal-avoidance. The prior gold was mine more than admitted.
INTEGRITY SURVIVOR: 0/3 but **0 false passes** — corroboration saw der1≠der2 (confused answer VARIED) and
ABSTAINED. The harness did not claim the wrong answer was done. Net: "conditions not capability" overstated;
deterministic machinery + integrity layer (resolve/fail-closed/P3/P4/corroboration-abstain) are the real,
surviving moat; the 7B's un-aided comprehension is not. Next is NOT a better prompt — it's a stronger/base
model or (b) completed-unverified as the standing stance.

## Test A — perspective-diverse corroboration, HONEST form
der2 stays SYMMETRIC with der1 (same neutral prompt, only different temperature/seed — a smarter der2 lens
would be a lead one level removed). So (a) = sampling-diversity corroboration; it can only catch errors that
VARY between samples.
PREDICTION for the 3 surviving false-passes: catches AT MOST 1 of 3.
- 0bf05a7d (copy Old ID → New ID): op-vocab gap, NOT a compute → der2 fails the SAME way → agrees → NOT caught.
- 42e0a640 (sum, results under TWO columns): error is target STRUCTURE, not input footprint → NOT caught.
- 1e8df695 (Profit next to CGOS): a formula error that MIGHT vary across samples → MAYBE caught.
Verdict bar (from the adversarial review): (a) is only worth keeping if it catches an error that (b)
completed-unverified would NOT — otherwise (b) dominates (0 claimed-false-passes by construction, ungameable).

## Test C — capability, NO favorable subset
Run the FULL libreoffice_calc set (47 tasks) or a SEEDED RANDOM sample — never the keyword-picked 16.
Report held-out (never-opened) tasks separately from 035f41ba. The keyword-excluded tasks (chart/pivot/format)
are EXPECTED op-vocab failures — they must fail-closed / not false-pass, and are reported, not hidden.
PREDICTION (committed before run): held-out gold rate is LOW (single digits %); the dominant failure is
authored-but-wrong + false-pass on compute tasks, plus op-vocab fail on chart/pivot/format. I do NOT expect
the 035f41ba result to generalize.

### PRE-COMMITTED CRITERION — the anti-treadmill rule (written 2026-06-23 BEFORE Wave-1 results)
The failure mode the user explicitly fears: an infinite verb-treadmill where every miss → "build another
verb," never reaching the real comprehension/emission question. So, committed in advance:
- A task that fails **with its required verb built AND the model actually emitting that verb with correct
  args** is a COMPREHENSION or EMISSION signal — report it as such, do NOT defer it to "yet another verb."
- Two distinct, separately-reported axes: (1) CAPABILITY (is the verb built + does it pass the scorer) vs
  (2) EMISSION (does the 7B pick the verb with right args — UNTESTED so far; the op-probe HAND-FED ops).
- Every sweep MUST tag each task by the capability its evaluator checks and report "addressable-with-built-
  verbs: gold X/Y" — never a raw gold count (which conflates "verb not built" with "built but failed").
- Wave-1 verbs are MECHANISM-verified (op-probe: they execute), NOT evaluator-verified (none has passed a
  real scorer yet). Color round-trip (ARGB/theme) is a known-unverified silent-fail risk.

### CAPABILITY MAP of the 30 held-out tasks (by evaluator rule, 2026-06-23, no-VM):
chart(W2)=4 · pivot(W3)=5 · format(W1)=4 · sheet_print(number/print fmt)=4 · sheet_name/zoom/freeze/
row_props/transpose/reorder/max-locate/pdf/infeasible = the long tail. Genuinely Wave-1-completable
(compute+sort+format, no missing sub-capability) ≈ only **3–6 tasks** (e.g. 51b11269 sort, 37608790 split,
4172ea6e maturity, abed40dc duplicates). NOTE: 2bd59342 is OSWorld-INFEASIBLE → must ABSTAIN (integrity
test). Even "format" tasks are bundled with unbuilt sub-caps (21ab7b40 needs max-cell-locate; 30e3e107
needs a pivot). ⇒ a full 30-task sweep after Wave-1 ALONE would show ~0–6 movement and muddy attribution
unless tagged. Build the cheap remaining verbs (sheet_print/number-format, max-locate helper) + Waves 2–3,
OR run a TARGETED mini-sweep on the 3–6 completable tasks to evaluator-verify the Wave-1 verbs first.

### Test C-fix — HELD-OUT sweep WITH the leading-'=' fill fix (committed BEFORE the run, 2026-06-23)
Set: the 30 NEVER-opened calc tasks (`battery_breadth.py heldout` — 47 minus the 16 keyword-SAMPLE minus
12382c62; validated: clean partition, no overlap, all resolve). Change vs the prior breadth run: the fill
fix (commit 5852a52) is now IN — every multi-reference formula and concatenation actually computes instead
of silently storing text. This is the FIRST clean measurement of where the calc loop stands once the
mechanical fill defect is removed; it ranks the NEXT bottleneck.
What the fix should move: tasks whose ONLY problem was the missing '=' (multi-ref formula columns) →
WRONG/ABSTAIN → GOLD. What it does NOT touch: (1) helper-column-vs-whole-sheet-gold shape (model fills
intermediates the gold leaves empty), (2) genuine comprehension (dropped/extra terms), (3) op-vocab ceiling
(chart/pivot/format/sort have no verb → must FAIL-CLOSED, never false-pass).
PREDICTION (committed before run):
- GOLD: **3–8 / 30** (10–27%). Honestly uncertain; lean low end. The fill fix is real but helper-shape +
  comprehension + op-vocab cap most tasks. A result of 0–2 would say the fix barely generalizes; ≥10 would
  surprise me (re-examine for any contamination).
- DOMINANT non-gold buckets (predicted order): helper-shape/whole-sheet mismatch ≈ WRONG(authored,oracle=0)
  and ABSTAIN(uncorroborated); op-vocab FAIL-CLOSED on chart/pivot/format; then genuine comprehension.
- FALSE PASSES (the integrity line): **≤ 2 / 30**, ideally 0. Any false pass is the most important thing to
  inspect — corroboration + falsifiers are supposed to hold this near zero even when the model is wrong.
- I do NOT expect a single task's behavior to dominate; report per-task attribution in full, failures included.
FALSIFIERS for my own claims: if GOLD ≫ prediction with no contamination → the fill bug was a bigger share of
all calc failure than I think (good, but re-scope). If FALSE PASSES > 2 → the integrity layer has a hole the
fill fix exposed (formulas now compute, so wrong-but-plausible values can slip the falsifiers) → integrity
work jumps the queue ahead of comprehension.

## ═══ PRE-REGISTERED: NEW-FRACTURE GAUNTLET (2026-07-03, FROZEN BEFORE FIRST RUN) ═══
Code state: commit d790183. Six NEVER-TOUCHED calc tasks chosen deliberately AGAINST the
post-op-vocab fracture line (compound emission, multi-table observation, semantic precision).
Protocol: host loop, N=1, cold — no code changes between selection and run; results reported
in full whatever they are.

| task | prediction | expected failure class |
|---|---|---|
| 04d9aeaf (Sheet2 + 4 headers + %-annual-changes ×3 cols) | MISS | compound emission; %-vs-last-year row anchoring |
| 7e429b8d (officer lookup across two tables) | MISS (low conf) | two-table observation; VLOOKUP capability exists — if detect merges tables, refs misbind |
| d681960f (grade students from scale table above) | MISS | stacked-tables observation + banding lookup semantics |
| 4de54231 (fill accel rows + "Header: value, ..." concat col) | MISS | compound + string-pattern precision |
| 4e6fcf72 (ages from birthdays) | MISS | date-anchor semantics (TODAY vs gold's fixed date) |
| f9584479 (fill missing total rows AND columns) | MISS (medium — total_row may carry the row half) | structure inference + compound |

Aggregate pre-registration: 0-2 golds / 6. Dominant classes predicted: compound emission (3),
observation limits (2), semantic precision (2). If MORE than 2 gold, the harness generalized
better than believed; if 0, the new fracture line is exactly where measured. Falsifier
expectation: 0 false-pass regardless.
