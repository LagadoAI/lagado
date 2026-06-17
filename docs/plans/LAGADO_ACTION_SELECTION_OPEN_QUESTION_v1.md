# Action Selection — the open architectural question (for the advisor, fresh context)

**Status 2026-06-17.** Written to survive a compaction. Resume = read this → ask the advisor
the question in §6 → make a plan. Do NOT start coding the fix before the advisor pass.

## 1. Where we are (proven this session)

- **VM control channel FULLY PROVEN.** `harness_proof` green all 8 stages (boot→ssh→X→a11y
  perception→QMP screendump→click-by-selector→delta→shutdown). Guest = Ubuntu 24.04 + XFCE,
  reproducible via `vm-provision/build-guest.sh`. See CLAUDE.md "VM control channel".
- **First autonomous walk ran end-to-end** (`first_walk` bin): `hydra::run` → 1.2B classifier →
  `agent_loop` → 8B brain (:8080) → real VM perception (`SshPerceptor`/tine) + actuation
  (`SshActuator`/xdotool), auto-approved HITL. Commit `8531a74`.
- The Board is live (ColBERT embedder :8082) but the board slice was empty this run (fresh
  memory) → recency floor. Supervisor wired as outer bound (commit `5eceb5d`).

## 2. What the walk exposed

Goal: "Click the Applications menu in the top panel." `Applications` was `ref_1` at (0,0).
- **Run 1 (no grounding help):** 8B picked `ref_5` "Show Desktop" — WRONG. Cutoff stopped it.
- **Isolation tests (model queried directly):** the 8B is **label-blind** — it does NOT match
  goal words to element labels. It picks by POSITION. Apps first→picks ref_5; apps last→picks
  ref_6; apps absent→picks the date. Only when apps is the SOLE option, or when an explicit
  "the element labeled X is ref_N" hint is injected, does it pick right.
- **ColBERT mean-pool cosine does NOT rescue this:** ranking the labels by cosine to the goal
  put "Show Desktop" FIRST for both "click Applications" and "open web browser". Compressed
  cosines [0.93–0.97] don't separate short labels. (Token overlap also collides: "Applications
  **menu**" vs "Directory **Menu**" — fixed only by coverage weighting.)
- **Run 2 (with the token-overlap hint):** 8B clicked the CORRECT `ref_1`, menu opened — goal
  achieved. BUT it then re-clicked ref_1 five times (toggling the menu) and never emitted
  `done`. The static-goal hint kept pointing at ref_1 even after the menu was open.

## 2.1 v2 walk — grammar rail WIRED + run live (2026-06-17, commit b355886)

The spec §2 design is now built and ran end-to-end against the VM (same goal). Result
isolates the problem cleanly:

- **Rail works (new, real):** every action was a valid `click(selector="el_N")` — ZERO
  parse failures, ZERO hallucinated/off-screen targets. `el_N → coord` resolution worked
  (arbiter fuse → build_candidates → set_targets → cache → xdotool). The supervisor caught
  the stall at 14 steps and escalated to human cleanly. GBNF validated against live
  llama-server (accepted; emitted `click(selector="el_0")` on a clean 3-item probe).
- **Label-blindness PERSISTS (confirmed, as predicted):** "Applications" is at (0,0) → it
  sorts to `el_0`. The model's FIRST pick was `el_4`; it NEVER picked `el_0`. Pick
  distribution `el_5`×7, `el_4`×4, `el_22`, `el_12`, `el_11` — mid-list position clustering
  (NOT even the first-item bias that helped on the short probe). Wandered into opening
  Thunar; goal never achieved; 48/48 screen cells changed.

**Conclusion: the grammar is a safety RAIL, empirically necessary but not sufficient — it
makes the model pick a VALID index, not the RIGHT one.** §6(b) is now data, not theory.

## 2.2 Offline candidate-ordering experiments (2026-06-17) — THE DECISIVE RESULT

Direct `/completion` probes against the live 8B (LFM2-8B-A1B, temp 0.2, grammar-constrained,
real system prompt, N=12/condition, goal "Click the Applications menu in the top panel").
Every condition was 12/12 IDENTICAL — temp 0.2 + grammar ≈ deterministic, so this is a stable,
exploitable property, not noise.

| # | candidate list | "Applications" at | model picked | correct |
|---|---|---|---|---|
| A | 6 items (live repro, spatial order) | row 1 (el_0) | "Directory Menu" (last) | 0/12 |
| B | 6 items | row 5 (el_4) | **Applications** | 12/12 |
| C | 3 items | row 1 (el_0) | "Show Desktop" (row 2) | 0/12 |
| D | 3 items | row 3 / last (el_2) | **Applications** | 12/12 |
| E | 6 items, 1-INDEXED tokens | row 1 (el_1) | "Directory Menu" (last) | 0/12 |
| F | 6 items, Apps row 1 but TOKEN el_1 (first grammar-alt el_0 = last row) | row 1 | "Directory Menu" (el_0, last) | 0/12 |
| G | 7 items, SACRIFICIAL first row | row 2 (el_1) | "Directory Menu" (last) | 0/12 |
| H | 6 items, BLANK-LINE separator after header | row 1 (el_0) | "Directory Menu" (last) | 0/12 |
| I | 6 items, Apps LAST + "menu" decoy moved to front | row 6 / last (el_5) | **Applications** | 12/12 |

**Findings (rule-in / rule-out):**
1. **NOT label-blind, NOT a capability floor.** The model matches "Applications" 36/36 whenever
   it's in the LATE portion (B, D, I), and does partial matching ("menu" → "Directory Menu").
   §6(e) answered: a bigger model is NOT the bottleneck.
2. **NOT list length / top-k.** The short 3-item list (C) STILL fails when the answer is early.
   Shortening is not the lever — Opus's "World 1" is rejected.
3. **NOT the token or grammar-alternative ordering.** 1-indexing (E) doesn't help; F decouples
   the visual row from the token and the dead slot tracks the ROW, not the token/grammar order.
4. **It's a LATE-LIST / primacy-skip bias.** The model under-attends to EARLY candidates and
   label-matches among LATER ones. G is decisive: even a sacrificial first row doesn't rescue an
   answer at row 2 of 7. C's tiny-list wrong-pick (el_1, not the "menu" decoy) is minor residue.
5. **VERIFIED FIX (deterministic, cheap): place the goal-relevant candidate LAST / in the late
   portion** → 12/12 (I), and it beats the "menu" decoy once the decoy is early. The model's OWN
   label-reading does the selection; the deterministic layer only has to get the right candidate
   into the attended (late) zone — it does NOT need to be a perfect 1-pick ranker (place the
   relevance top-k in the LAST rows and let the model choose among them). Determinism on the
   RAILS (ordering), strategy left to the model — exactly the doctrine.

**Residual tension (still real):** the ranker that decides "late placement" is the lossy step
(§5). But the bar is lower than feared — it must merely surface the right candidate into the
late band, not rank it #1. Open: does the late-bias hold across goals/screens, longer lists, and
multiple plausible candidates? (next experiment).

## 3. The flawed fix (committed as a labeled checkpoint, NOT to build on)

`agent::most_relevant_ref(screen, goal, exclude)` — token-overlap (coverage-weighted, stopword-
filtered) picks the single best-matching element; injected as a prompt hint the model follows.
Plus a stale-hint exclusion (drop the last-clicked ref once it changed the screen).

**Why it's wrong (the user caught it):** it makes the harness DECIDE, not RANK — determinism on
STRATEGY, not rails (violates the core doctrine). It does not scale:
1. **Senses without text labels.** CV-proposer boxes / vision patches / future audio have NO
   label to token-match → the heuristic goes blind on exactly the elements extra senses exist
   to recover.
2. **Tool choice.** Real decisions are {which tool} × {which target} × {params}. A single
   "most relevant element" hint says nothing about click vs type vs web-search vs MCP, and
   biases everything toward clicking a UI element.
3. **Kills model agency.** Always-present hint → model can't choose "none / done / re-perceive"
   (literally why it re-clicked the open menu).

## 4. The proposed shape (spec already has it — `selector_grammar` + escape; we bypassed it)

Deterministic layer produces a **ranked, trust-tagged SHORTLIST of candidates across all
senses** (the Board applied to the live action space). The **MODEL picks** tool + target +
escape ("none / done / re-perceive") from the few — grammar-constrained to valid candidate
indices. Ranking is multi-signal and **per-sense pluggable** (text for a11y, spatial/visual for
CV, the Board for memory), so a new sense adds a ranking contributor, not a rewrite. Small
models ARE reliable at one clean local choice among few options.

## 5. The unresolved tension (why we need the advisor, not just to build §4)

**Every ranking signal is individually lossy, AND the model is weak — so how is the shortlist
reliable enough that the model's pick is right?**
- token overlap: fails on no-label candidates, paraphrases.
- ColBERT cosine: doesn't separate short labels (verified).
- **IoU-dedup arbiter (`arbiter.rs`, TASK 6) is NOT wired into the live path** — `first_walk`
  was a11y-only. So multi-sense fusion is unproven live, and dedup itself is lossy (MATCH_
  THRESHOLD=0.30, ±1 patch fuzz — can merge distinct elements or split one).
- If the shortlist top-k drops the right candidate, the model can't recover it. If it keeps too
  many, the label-blind model is back to guessing by position.
The user's framing: "this seems lossy and I know what you'll say — how else then? That's why
we seek the advisor." We must NOT trade one scaling trap (single hint) for another (a lossy
ranked shortlist the weak model still can't choose within).

## 6. THE QUESTION FOR THE ADVISOR

**UPDATE 2026-06-17 (after reading spec §2 + arbiter.rs + grammar.rs).** Spec §2
("Selector grammar over the FUSED set") is a LOCKED design we bypassed, not a gap — it already
answers (a) and (c) on paper, and partly reframes the whole question:
- Grammar constrains over a **synthetic per-frame index** the arbiter assigns to EVERY
  `FusedElement` (the deterministic `(y,x,w,h)` sort = stable id space) — NOT over `ref_id`
  (which is `Option`, `None` for CV/DOM/vision-only → grammar-over-ref_id silently collapses
  fusion back to a11y-only). Index → bbox-center → coord click (also dodges the `tine` selector
  gap; raw coord clicks already work).
- Mandatory `none-of-these → re-perceive` escape production = BOTH the agency fix (decline /
  complete / re-perceive) AND the perception-escalation trigger (§3).
- Vision/VLM patches are ENRICHMENT (embeddings on boxes), not a selection vocabulary.
- State of parts: `arbiter.rs::fuse(a11y,cv,patches)→Vec<FusedElement>` is FULLY BUILT + 16
  tests, just not called live. `grammar.rs::selector_grammar()` is a STUB returning `""` (no
  constraint). So "wire arbiter + build grammar" = connect existing parts, not new design.

**So the grammar is a SAFETY RAIL, not a selection-quality mechanism.** Constraining to valid
indices stops hallucinated/off-screen targets (G4 security win) but does NOT make the pick
correct: the walk proved the 8B picks by POSITION, so a grammar over 6 valid indices just makes
it pick the wrong *valid* index. THAT is the surviving open problem.

Given: a deliberately small, **label-blind** local model (picks by position, can't match goal→
label); multiple **lossy** perception senses (a11y/tine, CV boxes w/o labels, vision patches),
fused by the (built-but-unwired, lossy) IoU arbiter; a growing **tool** space (UI + native +
MCP); and the sovereignty constraint (no big model, single-turn-fresh, determinism on rails not
strategy). The grammar-over-fused-index + escape spine is taken as given (spec §2). The real
questions:

(b) **Selection quality within the shortlist.** What makes the model's pick CORRECT when it
    can't read labels and every ranking signal is individually weak (token overlap dies on
    no-label candidates; ColBERT cosine doesn't separate short labels — both verified)? Options
    to weigh: vote/union across signals so the right candidate is never dropped; confidence-
    gated escalation (re-perceive a richer sense tier when shortlist confidence is low); putting
    the *label next to the index* in the prompt so the rail also carries selection signal; or
    accepting that position-blindness needs a different model affordance entirely.
(d) **Completion / "done."** How is goal-satisfaction recognized reliably when the model is weak
    at it (re-clicked the open menu 5×) and neither the grammar nor the deterministic layer
    should decide goal-satisfaction? Does the escape production + a deterministic "screen
    stopped changing / loop detected" signal suffice, or is done-detection its own organ?
(e) **Model size.** Does any of this change if we accept a SLIGHTLY larger local model, or must
    it hold at 8B-A1B (1B active)? Is label-blindness a size problem or a prompt/affordance
    problem?
(a')/(c') (now mostly settled by spec §2 — confirm or refute): grammar-over-fused-index + escape
    is the right spine; fuse-then-rank (arbiter produces the candidate set the ranker scores and
    the grammar enumerates). Push back if there's a better decomposition.

## 7. Key file pointers for the fix

- `lagado-agent/src/agent.rs`: `agent_loop` (the live loop), `most_relevant_ref` (flawed hint),
  `classify_step_outcome` + supervisor wiring (outer bound), prompt build (~line 564).
- `lagado-agent/src/perception/arbiter.rs`: IoU fusion (TASK 6, NOT wired live).
- `lagado-agent/src/perception/{cv_proposer.rs, frame.rs}`, `vision/mod.rs`: the unused senses.
- `lagado-agent/src/grammar.rs`: GBNF (intent grammar built; `selector_grammar` NOT built).
- `perceive.py`: a11y/tine reader (`--focused` emits `ref_N "label" (x,y,w,h)`).
- `docs/plans/LAGADO_HARNESS_BUILD_SPEC_v1.md` §2: the `selector_grammar`-over-FusedElement-
  index design we bypassed (read this — it's half the answer).
