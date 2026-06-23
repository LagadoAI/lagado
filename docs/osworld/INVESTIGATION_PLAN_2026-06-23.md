# Investigation plan — locate the failure MOMENT, get HARD DATA on every interaction variable

USER DIRECTIVE (2026-06-23, the governing thesis for the next phase):
- **Comprehension is NOT assumed the problem.** A model limit exists somewhere, but we are NOT there.
  Until proven otherwise, **assume the failure is OUR APPROACH — how we ask the model to interact with the
  data.** Do not conclude "comprehension limit" (that was overstated).
- **Locate the EXACT moment the failure occurs, then build for that moment.**
- **Keep what is good; remove what hurts the overall goal.**
- **Enumerate EVERY variable that could assist, and get HARD DATA on each** (ablation/measurement, not theory).
- Discipline (hard rules, learned this session): NO leading prompts (no answer hints, no solution schema);
  prompts aren't the lever and aren't where the game is — INTERACTION DESIGN is. 035f41ba is CONTAMINATED
  (hand-iterated) → never in a headline number; test on HELD-OUT (never-opened) tasks. Pre-register
  predictions. Report ALL results incl. failures. Stop OSWorld runs with SIGINT (-2), never kill -9.

## THE LOCATED FAILURE — CORRECTED 2026-06-23 (the prior account below was a MISREAD)

⚠️ **SUPERSEDED.** The earlier text said the model "collapsed to step 1 (gross profit = B2-C2), omitting
every expense." That was wrong — it was a misreading of a one-line log SUMMARY, never the raw reasoning.
Reading the actual JSONL (`/tmp/lagado_battery/calc_035f41ba.jsonl`, no VM needed) overturns it.

WHAT THE MODEL ACTUALLY DID (de-leaded, chat template, all 3 runs):
- Reasoning was **complete and correct**: Net Sales = Sales−Returns; Total COGS = Materials+Labor+Overhead;
  Gross Profit = NetSales−COGS; then Sheet2 = Year & "_" & GrossProfit.
- Emit (runs 0,2) was **complete**: it authored all of E (Net Sales), I (Total COGS), J (Gross Profit) as a
  sensible HELPER-COLUMN decomposition, plus Sheet2. No collapse. (Run 1 = a grammar non-termination repeat.)

WHY IT STILL SCORED 0 — three INDEPENDENT factors, NONE of them "comprehension absent":
1. **Helper-column vs whole-sheet-gold mismatch (dominant/structural).** Gold fills ONLY J
   (`=B2-C2-D2-SUM(F2:H2)`) and leaves E & I EMPTY. The evaluator (`compare_table` → `sheet_data`) does
   `df.equals` over the WHOLE sheet, so the model's filled E/I (its correct intermediates) are extra cells →
   mismatch. The model's good practice is punished by a single-combined-column gold.
2. **Harness fill bug.** `set_formula_range` (uno_ops.py:137) seeds the top cell then `fillAuto(TO_BOTTOM,1)`;
   the readback shows `B2-C2 → B2-C3 → B2-C4` — only the LAST ref adjusts per row (looks like series-
   extension on the trailing number, not a relative formula copy). Every row past the seed is wrong. MASKED
   on single-reference golds (where "only ref adjusts" == correct), which is why prior golds didn't expose it.
   SYMPTOM is fact (in the log); fillAuto root-cause is a hypothesis to confirm on the guest.
3. **One dropped term.** Model omitted column D (Discounts and Allowances) from the expenses → J short by D.
   A genuine but SINGLE-term comprehension slip — not a collapse.

⇒ **Corrected failure account:** the model COMPREHENDED this task (one-term slip aside). The 0/3 is dominated
by INTERACTION + a HARNESS DEFECT — exactly the user's thesis. The real levers are now (1) the pure fill-bug
fix [harness defect, unambiguous], (2) output-SHAPE: write only the requested target as one combined formula
(or deterministically INLINE the model's own helpers — algebra, not leading), (3) the residual term-
completeness gap (1 of 6 terms). Iterative ReAct (old "prime suspect #1") would NOT have fixed any of these.
Full evidence + corrected ranking: this section + PREDICTIONS.md Test 0b CORRECTION.

## VARIABLE MATRIX — every controllable interaction variable, each to be MEASURED (held-out, ablated one at a time)
⚠️ RE-AIMED 2026-06-23 by the corrected log read: variable #1 (iterative ReAct) is NO LONGER the prime
suspect — it would not fix the helper-column shape, the fill bug, or the dropped term. New top priorities:
**fill-bug fix (pure defect)**, then **#5 output granularity / shape** (single combined target vs helper
columns — incl. deterministic algebraic INLINING of the model's own helpers), then **#2 reason→emit fidelity
re-scoped to "term completeness"** (did all required input terms make it in — here D was dropped). The matrix
below is kept for the remaining variables; re-rank against the corrected failure account above.
| # | Variable | Hypothesis (why it may be the lever) | How to get hard data |
|---|---|---|---|
| 1 | **Loop structure: one-shot vs iterative ReAct** (emit step → OBSERVE result → continue) | PRIME SUSPECT — directly addresses step-1-collapse; let it compute Net Sales, see it, continue to subtract expenses | gold rate one-shot vs iterative, held-out |
| 2 | **reason→emit fidelity** — does the full multi-step plan survive into the ops? | the emit is a separate grammar call that may drop later steps | capture full reasoning plan; diff vs emitted ops; count steps dropped |
| 3 | **Intermediate observation in-context** — model sees computed values of prior steps | grounding each step in real results may prevent stalling | with/without read-back-into-prompt |
| 4 | **Decomposition granularity** — one goal vs generic step-wise (NON-leading) vs per-column | smaller asks may complete where one big ask collapses | gold rate per granularity |
| 5 | **Output granularity** — whole formula vs term-by-term / op-by-op | term-by-term can't collapse to step 1 | measure |
| 6 | **Observation richness** — header+samples vs +full column vs +SOUND derivation cues (not leading) | more signal, no hint | measure |
| 7 | **Model** — 7B-Coder-Instruct vs stronger vs BASE (non-instruct) | isolates the TRUE model limit vs interaction; base avoids assistant-mode narrowing | swap model, same harness |
| 8 | **Sampling** — best-of-N / self-consistency vote | aggregate may recover the full formula | N-sample, measure |
| 9 | **Interaction modality** — emit-ops vs query-then-act (model can ASK the data) | a different engagement mode entirely | prototype + measure |

Run protocol: HELD-OUT random sample of libreoffice_calc (NOT the keyword-16, NOT 035f41ba); pre-register a
prediction per cell; ablate ONE variable at a time vs a fixed baseline; report the full table incl. failures;
per-task timeout (SIGALRM 420s) so nothing wedges the sweep.

## KEEP (good, prompt-independent, proven — do NOT remove)
Native session (uno_daemon, host-owned op log, floor byte-identical) · notation-robust resolve
(header/letter/index, fail-closed) · auto-create target · header-row detection · P4 zero-misbind ·
P3/read-only corroboration (integrity HELD: 0 false passes even under the comprehension failure) · sound
falsifiers · per-task timeout · chat endpoint (fixed the drift; model-agnostic via GGUF template).

## REMOVE / NEVER (hurts the goal)
Leading prompts (subtotal hint, decomposition schema — DONE removed) · any prompt-phrasing dependence
(robustness must come from deterministic mechanism) · cherry-picked task selection / contaminated-task
headline claims · kill -9 on OSWorld runs (leaks root qemu → boots hang).

## CURRENT STATE (for resume)
Drivers: docs/osworld/battery_calc.py (main loop, chat endpoint, read-only corroboration, neutral prompts),
battery_breadth.py (sweep + timeout + attribution), battery_p3.py, battery_p4_resolver.py. Records:
BATTERY_FINDINGS_2026-06-22.md, PREDICTIONS.md (Test 0/0b results). Last pushed commit: 2b9fbcf (+ uncommitted
chat-endpoint change in battery_calc.py + PREDICTIONS 0b result at compaction time → commit before clear).
Honest headline: the deterministic + integrity harness is the moat and holds; the 035f41ba "gold" was
scaffold-supplied; un-led failure is a single-shot multi-step COLLAPSE (interaction), and the model limit is
NOT yet established — that is what the variable matrix is for.
