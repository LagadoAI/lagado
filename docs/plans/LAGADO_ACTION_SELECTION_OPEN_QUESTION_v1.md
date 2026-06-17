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

Given: a deliberately small, **label-blind** local model (picks by position, can't match goal→
label); multiple **lossy** perception senses (a11y/tine, CV boxes w/o labels, vision patches),
fused by a **lossy** IoU arbiter that isn't even wired yet; a growing **tool** space (UI +
native + MCP); and the sovereignty constraint (no big model, single-turn-fresh, determinism on
rails not strategy) —

**How should the agent select the next action (tool + target) reliably?** Specifically:
(a) Is "deterministic ranks a shortlist, model picks (grammar-constrained) + escape" the right
    spine, or is there a better decomposition?
(b) How do we keep the shortlist from being lossy in the failure direction (right candidate
    dropped) when each ranking signal is individually weak — vote/union across signals?
    confidence-gated escalation (re-perceive a richer sense tier on low shortlist confidence)?
(c) Where does the IoU arbiter belong relative to ranking — fuse-then-rank, or rank-per-sense-
    then-merge? Does fusion need to happen before the model sees candidates at all?
(d) How does completion ("done") get recognized reliably when the model is weak at it and the
    deterministic layer shouldn't decide goal-satisfaction either?
(e) Does any of this change if we accept a SLIGHTLY larger local model, or must it hold at 8B-
    A1B (1B active)?

## 7. Key file pointers for the fix

- `lagado-agent/src/agent.rs`: `agent_loop` (the live loop), `most_relevant_ref` (flawed hint),
  `classify_step_outcome` + supervisor wiring (outer bound), prompt build (~line 564).
- `lagado-agent/src/perception/arbiter.rs`: IoU fusion (TASK 6, NOT wired live).
- `lagado-agent/src/perception/{cv_proposer.rs, frame.rs}`, `vision/mod.rs`: the unused senses.
- `lagado-agent/src/grammar.rs`: GBNF (intent grammar built; `selector_grammar` NOT built).
- `perceive.py`: a11y/tine reader (`--focused` emits `ref_N "label" (x,y,w,h)`).
- `docs/plans/LAGADO_HARNESS_BUILD_SPEC_v1.md` §2: the `selector_grammar`-over-FusedElement-
  index design we bypassed (read this — it's half the answer).
