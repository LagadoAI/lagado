# Harness Work Plan (2026-07-03)

The steering summary for the harness phase. CLAUDE.md "Status (2026-07-03)" is the
short form; this file carries the work queue. Authoritative deep resume:
`docs/osworld/INVESTIGATION_PLAN_2026-06-23.md` (top "POST-CLEAR RESUME PLAN" block).

## Current state

- **Real OSWorld, held-out stress (30 never-opened calc tasks): 7/30 gold, 0 false-pass.**
  7/30 = golds achieved by model+harness together; model-alone attribution is NOT yet
  measured (that's the ablation matrix, below).
- **Fracture line = OP-VOCAB coverage** (pivots, sparklines, freeze, csv, transpose,
  reorder, resize, conditional, locale unbuilt) — NOT comprehension, NOT grounding
  (0 grounding mis-fires under stress).
- Branch: `Harness` (continuation of `deskew/class-not-instance`; `main` is pre-OSWorld).

## Three directions (2026-06-29 docs, adversarially reviewed 2026-07-03)

1. **Integrate-before-invent** — `docs/INTEGRATION_SURVEY_2026-06-29.md`. 3 of 4 layers
   have permissive external candidates (verdicts are ADOPT/eval and ADAPT — evaluations,
   not decided adoptions); the fusion dispatcher + selection loop is the white space and
   is already built. Treat the survey as hindsight validation of that call, not an open
   design question.
2. **Verifiable-evals integrity** — `docs/osworld/PROMPT_GROUNDING_AUDIT_2026-06-29.md`.
   Eight Class-B corrective-grounding sites reshape model output; each could be earning
   golds. The ablation matrix separates model from harness.
3. **Human-verifiable work** — `docs/osworld/watch_qwen.py` / `watch_session.py`. Live
   paced-op watching. Known gap (from review): the watcher sees the RESULT, not the
   ATTRIBUTION — needs a `--disable-grounding` mode to show model-only output.

## OPEN DECISION — the next bet (user's call, both have standing endorsements)

- **Option A: Ablation-first** (endorsed 2026-06-29, "NEXT(green-light)"): run the
  per-grounding ablation matrix on the baseline golds. Higher leverage intellectually —
  it answers "is op-vocab really the bottleneck, or is the harness carrying the model?"
- **Option B: Op-vocab-first** (endorsed 2026-06-23, post-clear goal): build the missing
  verbs → re-stress the held-out 30 → if the golds climb, the thesis (harness lets a 7B
  reach the ~72% floor) gains its strongest evidence; ablate afterwards on the larger
  gold set.

## Work queue

### Phase: Ablation baseline (if Option A)
1. Pre-register the matrix shape + which golds, BEFORE running (frozen, announced).
   Keep survivor predictions private until the matrix runs.
2. Determinism pre-check: re-run ONE gold 2x through `env.evaluate()`; if it doesn't
   reproduce, diagnose before scaling.
3. Run golds x 8 ablation sites (~56 evals on the current 7). Artifact-first: raw
   `env.evaluate()` returns per cell; the summary table is GENERATED from the artifact.

### Phase: Op-vocab completion (if Option B; eventually either way)
1. Build missing verbs (pivots done at turn-4; sparklines, freeze, csv, transpose,
   reorder, resize, conditional, locale remain) — each VM-verified mechanism-first.
2. Zero-regression rule: after each verb, re-run the existing golds.
3. Re-stress the full held-out 30 with the official evaluator only.

### Verification debts (from the 2026-07-03 adversarial review — batchable)
- `watch_qwen.py --disable-grounding [CLASS_A|CLASS_B|all]` so a human can compare
  model-only vs grounded output and judge attribution by eye.
- Test UQLM white-box scorers against llama.cpp logprobs IN ISOLATION before any
  confidence gating depends on them.
- Extend the prompt-grounding audit to the Rust prompts (`forge.rs`, `recovery.rs`,
  `operator.rs`) — the operator urgency nudge is low-risk (anti-loop, not leading)
  but the discipline claim is incomplete until Rust is covered.
- Publish the integration survey's 6 search angles + source list (coverage-gap check).

### Structural debts
- ~~Move executable tooling out of `docs/osworld/`~~ **DONE 2026-07-03 (d6039a3):** all
  tooling now at `lagado-agent/python/osworld/` (pure `git mv`, history preserved;
  `native_session.rs` include_str! updated, cargo check + py_compile green; path-moved
  notes added to the resume plan + osworld README). New PYTHONPATH:
  `/home/alucard/projects/OSWorld:/home/alucard/projects/lagado/lagado-agent/python/osworld`.
- README still frames app-first; rewrite pends the user's public-narrative decision.

## Success criteria
- 72% on the real OSWorld bench treated as a FLOOR (greed doctrine), zero false-pass
  maintained (currently held: 0 under extreme stress).
- Model-vs-harness attribution measured, not asserted (ablation matrix artifact).
- Every gold eventually carries a grounding-dependency vector (which Class-B sites it
  needed).
- Results human-verifiable live (watch tools, including grounding-off mode).
