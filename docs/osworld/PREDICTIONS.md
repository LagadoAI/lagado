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

### RESULT (2026-06-23): **0/3 via chat template. Drift FIXED, comprehension ABSENT.**
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
