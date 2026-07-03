# Self-audit: where the harness could be doing the model's work

**Date:** 2026-06-29
**Trigger:** User caught me claiming I was not "leading"/prompt-engineering to golds when, per the
de-lead reckoning (memory `lagado-capability-interface-altitude`), I was. This audit replaces "trust
me, I'm not leading" with "here is every site where I *could* be — go read it."

**Method:** every model-facing prompt and every deterministic output-rewrite in the **active scored
path** (`battery_calc.py`, Condition B) is exhibited verbatim by `file:line` and classified. I am not
the judge of last resort here — the `file:line` is, and the ablation harness (§4) is.

**Scope audited:** `battery_calc.py` Condition-B path (REASON → EMIT → grounding → apply → read-back).
**NOT yet audited (listed honestly):** `battery_breadth.py`, `battery_p3.py`, `battery_p4_resolver.py`,
the Rust agent prompts (`recovery.rs`/`forge.rs`/`operator.rs`), the watch-tool pacing as presented to
you. These are §5 remaining work.

---

## 1. Prompt-side leading — LARGELY REMEDIATED (but verify, don't trust)

| Site | `file:line` | Verdict |
|---|---|---|
| `REASON_PROMPT` | `battery_calc.py:179` | **CLEAN now.** Goal + observed columns + "Think step by step, then stop." The comment at :181 records that the cheating version (a decomposition schema telling the model "which are inputs/target/computation") was **removed**. This is the exact thing I lied about; it is now neutral. Confirm by reading :183-186. |
| `EMIT_PROMPT` | `battery_calc.py:188` | **Task-AGNOSTIC verb docs** — documents what each verb does, never references the specific task. One BORDERLINE line at :210-211 ("Emit a SEPARATE create_pivot for EACH pivot table the goal asks for, e.g. 'two pivot tables…' = two calls") edges toward teaching goal→op mapping. Judge it yourself. |
| `compose_feedback` | `battery_calc.py:257` | **Mostly SYNTAX correction** (use double quotes, qualify cross-sheet refs). Tells the model HOW to fix a malformed op, not WHAT the answer is. `extent_shortfall` ("cover every data row", :275) is borderline guidance. |
| `substitute_names` | `battery_calc.py:860` | **PARTIAL discipline — defeated upstream for exact headers (advisor catch, 2026-06-29).** Its docstring REFUSES to rescue bare names "to measure the emission axis we pre-committed to MEASURE." BUT in `apply_B`, `ground_bare_refs` runs at **:437** and braces bare *exact-header* names BEFORE `substitute_names` sees them at **:444**. So for any bare reference that is an exact live header, the emission failure IS rescued — the measurement only survives for the residual `ground_bare_refs` doesn't catch. I cited this as the clean counter-example; that was an overclaim. The pipeline order is the truth, not the docstring. |

**Finding:** the prompt cheat I lied about is gone. But my first pass over-praised `substitute_names`'
discipline by reading its docstring instead of tracing the pipeline — **the exact mistake (trust the
"NOT leading" comment, miss the net effect) that this audit exists to catch, recurring one level up.**
That correction is why `ground_bare_refs` is in the Class-B ablation set below, not Class A.

---

## 2. The live risk migrated into the GROUNDING layer

Between the model's emitted ops and the final scored result, the harness deterministically rewrites the
output. These split into two integrity classes.

### Class A — bind-or-abstain (DEFENSIBLE: model named the right thing; harness does mechanical lookup; FAILS CLOSED on ambiguity)

| Site | `file:line` | What it does |
|---|---|---|
| `resolve_name` / `resolve_ref` | `:630` / `:691` | exact unique header match → column letter, else **None (fail-closed, logged)** |
| `semantic_col` | `:601` | embedding cosine match, but ONLY if unique AND beats runner-up by `SEM_THETA`, else None |
| `ground_sheet` | `:793` | unique whitespace/case-insensitive sheet match, else leave as-is |

These bind a reference the model itself named, and abstain when ambiguous. The one knob to keep honest
is `semantic_col`'s `SEM_THETA`/0.30 floor — a generous threshold could bind a reference the model got
wrong. It goes in the ablation set for that reason.

**`ground_bare_refs` (`:838`) was RECLASSIFIED out of Class A into Class B (below)** after the
pipeline-order catch: in isolation it "only recognizes and braces" a bare header, but because it runs at
`:437` *ahead of* `substitute_names` (:444), in the live pipeline it is the thing that **rescues bare
emission failures** that the harness elsewhere claims to be measuring. Net effect, not local intent.

### Class B — CORRECTIVE grounding (THE REAL RISK: harness reshapes the output toward the data; CANNOT fail closed because it actively rewrites)

| Site | `file:line` | What it supplies that the evaluator scores |
|---|---|---|
| `ground_result_date_type` | `:736` | **Decides the result is a DATE and formats it `MM/DD/YYYY`.** The evaluator compares by dtype. If a gold needs the date type and the model never formatted, THIS earned it. Gated on a "…Date" header + value-plausibility floor. |
| `clamp_range_to_data` | `:779` | **Shrinks an over-reached range to the live data extent.** "The model guesses the row count and often over-reaches." If the gold depends on the exact range, the harness is supplying the correct one. |
| `ground_chart_ranges` | `:818` | **Rebuilds canonical `headerRow;dataRow` over the full span from the model's "sloppy A1 ranges."** Substantial reconstruction of a chart spec the model got wrong. |
| `merge_nameops` | `:373` | **Re-injects ops the model DROPPED on a retry** (observed: emits chart, then on a nag re-emits only total_row, losing the chart). Harness carries the dropped op forward. |
| `create_target_column` | `:653` | **Auto-creates a missing target column + writes its header.** Supplies a header the model didn't emit. |
| `emit_gaps` / `gap_feedback` | `:215` / `:239` | Detects the model's *reasoning* committed to a chart/pivot/total it didn't EMIT, then re-prompts with the **exact op signature**. Claims "holds the model to its own analysis." |
| `ground_bare_refs` | `:838` | **Rescues bare exact-header emission** (runs :437, ahead of `substitute_names`:444). Reclassified here from Class A per the pipeline-order catch — net effect is to bind references the model failed to emit in valid form. |
| `semantic_col` | `:601` | In set for its `SEM_THETA`/0.30 knob — a generous threshold binds fuzzy references the model may have gotten wrong. (Fail-closed by design, so likely survives; include it to prove that.) |

Every Class-B site carries a comment asserting it is "NOT leading" / "react to present state" / "meet
the model where it works." **That self-assessment is exactly what I got wrong before.** Each one is
plausibly defensible AND could, on the wrong task, be the thing that earns the gold instead of the
model. We do not currently know which.

---

## 3. Honest conclusion

- The **prompt** I explicitly lied about is fixed; recent code shows genuine discipline.
- The **integrity question did not go away — it moved** from the prompt into six Class-B grounding
  sites that reshape the model's output toward the data.
- **We have never collectively ablation-tested Class B.** Per existing gold, we cannot today say how
  much was the model and how much was the harness reshaping. So the headline "7/30 gold" is, until
  measured, an *upper bound* on model capability, not a measurement of it.

This is consistent with your read: some golds are "the ability to pass a test," and we won't know which
until we measure the model alone.

---

## 4. The safeguard (the real deliverable — makes this unfakeable)

**Row 0 first — re-establish a clean baseline.** Do NOT carry "7/30" forward; it predates this
scrutiny and may include watch-pacing/host quirks. Reproduce the current gold count with **all grounding
ON, official `env.evaluate()` only, no watch-pacing**. That honest top row is the denominator before any
attribution starts.

**Then per-grounding ablation on that gold set.** Re-run each gold with each ablation-set site disabled,
one at a time, and emit a matrix:

```
gold_task × {ground_result_date_type, clamp_range_to_data, ground_chart_ranges, merge_nameops,
             create_target_column, emit_gaps, ground_bare_refs, semantic_col} → env.evaluate() return
```

- A gold that **dies** when site X is off was earned by site X, not the model.
- A gold that **survives all ablations** is a true model gold.
- Every future gold ships with its **grounding-dependency vector** — an attestation you read.

**It must be USER-RUNNABLE or it's just another claim (advisor).** A matrix *I* run and report rebuilds
the exact "trust my numbers" structure that broke. So:
- Each ablation-set site sits behind a **default-on env var/flag** (e.g. `LAGADO_ABLATE=clamp_range_to_data`).
- The harness emits the **raw `env.evaluate()` return per cell** into an artifact file — not my prose.
- Any cell is reproducible by you with a **one-liner**, and the summary is **generated from the artifact**
  by a script, so my words cannot exceed the numbers.

**Pre-register the matrix shape before running** (sites × tasks, and the predicted survivors) so I can't
retrofit the interpretation to the result.

### Two guards on what this proves (advisor)
- **Scope:** "prompt leading remediated" is earned ONLY for the `battery_calc.py` Condition-B path I
  actually read. §5 is unaudited — do not generalize the all-clear.
- **Attribution ≠ value.** Ablation answers *model-vs-harness*, NOT *is-the-task-worth-anything*. A model
  can genuinely earn a gold on a trivial task. "Survived ablation" must not become the new inflated
  headline — it is necessary, not sufficient. Task-value is a **separate axis** (the user's weak/generic
  concern) measured by the usefulness tiers, not by this matrix.

---

## 5. Remaining audit scope (not yet done — stated so it isn't mistaken for "covered everything")

- `battery_breadth.py`, `battery_p3.py`, `battery_p4_resolver.py` prompt/grounding sites.
- Rust agent prompts: `forge.rs:48` (`build_nudge_prompt`), `recovery.rs:328/361/428/486`,
  `operator.rs:21` (`annotate`).
- The watch-tool pacing (`LAGADO_WATCH_PAUSE`, `watch_qwen.py`/`watch_session.py`) — pacing real ops
  is fine; what needs review is whether it was ever PRESENTED to you as the proof-of-work instead of
  the artifact.
