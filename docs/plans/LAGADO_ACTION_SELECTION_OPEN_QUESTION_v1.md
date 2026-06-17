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

## 2.3 Discrimination + layout probe (2026-06-17) — good-world BUT fragile

Counterbalanced decoy-in-band test (Opus's discrimination cut) + layout probe. N=12, live 8B,
grammar-constrained. target="Applications" (exact goal substring), decoy="Directory Menu"
(shares "menu"); fillers share no goal words.

**Discrimination — does LABEL beat POSITION within the late band?**
| cond | arrangement | picked | verdict |
|---|---|---|---|
| C1 | target row5, decoy row6 (last) — unconfounded | Applications 12/12 | LABEL wins |
| C2 | decoy row5, target row6 (last) — confounded ctrl | Applications 12/12 | (control) |
| C3 | target row4, filler row5, decoy row6 (last) — hard | Applications 12/12 | LABEL wins |
→ **GOOD WORLD: within the attended band the model selects by label, beating the last-position
decoy even when the decoy holds the prime last slot (C3). Ranker needs RECALL into the band,
not exact #1 ordering. Union-of-weak-rankers is viable.**

**Layout probe — is the safe zone stable, or does it move with prepended context?**
| cond | setup | picked |
|---|---|---|
| LP-base | Applications first (el_0), decoy last; list above goal | decoy (el_5) |
| LP-goalrep | + goal repeated immediately before action token | decoy (el_5) — no change |
| LP-below | list placed BELOW the goal (closest to decision point) | decoy (el_5) — no change |
| **LP-memabove** | **identical list to C2** (decoy row5, target last) **+ 30-line memory block above** | **decoy (el_4) 12/12** |
→ **CONTROLLED FLIP: C2 (no memory) picked target-last 12/12; LP-memabove (same list, +30 lines
above) picked the DECOY 12/12. One variable — prepended context — flipped a winning config.**

**Synthesis:** good world is REAL but CONTEXT-SENSITIVE. The prompt is reassembled every step with
variable-length memory above the candidate list (§2), so a fixed "place the answer last / recall
into band" rule passes a clean probe and SILENTLY ROTS in production. Repeating the goal or moving
the list next to the decision point does NOT fix the early-position blind spot. **Late-placement
is therefore NOT safe to wire.**

**Implication → flip SELECT to VERIFY (Opus):** judge one candidate at a time `{act, skip}`,
deterministic ranker proposes order, accept first `act`, escape if all top-k skipped. No list →
no row to mis-attend; candidate sits at end of context (strongest attention); should be immune to
BOTH the intra-list position artifact AND the memory-above flip. NEXT EXPERIMENT: prototype
verify-mode offline; stress it under the same memory-above condition; check acquiescence (says
`act` too eagerly) + ranker recall. Done-detection must be its OWN signal (the same bias that
grabs the last candidate is why the agent re-clicked an open menu — selection and completion
cannot be one call).

## 2.4 Verify-mode probe (2026-06-17) — NEGATIVE: binary act/skip is context-dominated

Opus's select->verify flip, instrumented to its spec: dedicated lean verify prompt (NOT the
action SYS prompt), every cell under memory-above (0/15/30 prepended lines), acquiescence as a
gradient on the proven attractor "Directory Menu", false-act-on-top-1 logged. N=16, live 8B,
grammar `root ::= "act" | "skip"`.

**Acquiescence gradient (act-rate; want Applications=act, others=skip):**
| candidate | mem=0 | mem=15 | mem=30 |
|---|---|---|---|
| Trash (clearly wrong) | 16/16 | 7/16 | 16/16 |
| Directory Menu (ATTRACTOR) | 16/16 | 1/16 | 15/16 |
| Show Desktop (neutral wrong) | 15/16 | 2/16 | 0/16 |
| Applications (CORRECT) | 0/16 | 16/16 | 16/16 |

The act/skip decision is dominated by **prepended-context length, not the label**. mem=0 is
INVERTED (acts on all wrong, skips the correct answer); mem=30 acts on Trash 16/16 + attractor
15/16. Only mem=15 looks sane — not guaranteeable in production.

**Sequence sim (judge top-1, widen on skip, mem=30):**
- GOOD ranker (Applications top-1): pick Applications 15/16, false-act-on-top-1 0/16 — but only
  because the ranker pre-solved it.
- BAD ranker (attractor top-1): pick "Directory Menu" 14/16, **false-act-on-top-1 = 14/16 (terminal)**.

**Conclusion:** verify-mode does NOT escape the fragility — it relocates it from "which row" to
"yes/no", where it is WORSE (no escape token; confident wrong commit), exactly as Opus predicted.
The verifier is ACQUIESCENT and does not function as an independent check — it rubber-stamps the
ranker's top-1 (redundant with a good ranker, terminal-wrong with a bad one). Opus's premise
"recognition is intact wherever it attends" is NOT supported by isolated binary verify; the
select-mode "good world" (C1/C3) may be a list-internal comparison effect, not a transferable
per-candidate judgment. The memory-above gradient was decisive — a clean short prompt would have
shown a lucky pass.

**Where this leaves the architecture:** neither late-placement (context-fragile, §2.3) nor binary
verify (context-dominated, here) is robust ALONE. Common root cause across BOTH: the binary/row
outcome flips with **variable prepended-context length**. Candidate next direction (for Opus):
the model's usable recognition appears to live in *list-internal comparison among few* (C1/C3),
so a SMALL recall-into-band top-k presented in a LAYOUT-STABLE / length-bounded prompt (control
the prepended context so the attended zone doesn't move) — i.e. the lever may be prompt-layout
stability, not select-vs-verify. Open for the skeptic thread.

## 2.5 Fixed-trailer + semantic-memory test (2026-06-17) — THE ROOT CAUSE

Opus's fixed-trailer / variable-memory discrimination (decision block byte-identical at the
prompt's end; memory varied above). Two layouts (goal-last / candidates-last) × mem 0/15/30,
then a semantic-content sweep. N=12, live 8B, grammar-constrained.

**Step 1 — neutral filler, fixed trailer:** the decoy-FLIP is GONE. When the model picks an
element it picks the TARGET (never the decoy), across mem=0/15/30 and both layouts. Residual
failure = premature `done(reason=...)` (worst at mem=15) — a COMPLETION artifact, not selection.
So prepended *length* with neutral content does NOT corrupt selection.

**Step 2 — semantic memory sweep (fixed trailer, target=el_5 last, decoy=el_4), ×30 lines:**
| prepended memory | target el_5 | decoy el_4 |
|---|---|---|
| neutral filler (control) | 12/12 | 0/12 |
| DECOY-priming ("you often use the Directory Menu…") | **0/12** | **12/12** |
| GOAL-priming ("the Applications menu launches…") | 12/12 | 0/12 |

**ROOT CAUSE: semantically-related prepended memory CONTROLS selection, fully overriding the
candidate labels.** Prime the decoy in memory → picks the decoy 12/12, ignoring the correct
"Applications" in the list. The LP-memabove flip (§2.3) was THIS (its memory mentioned
"documents/spreadsheets"), not prepended length. §7e resolved: NOT a clean layout fix, NOT a
flat capability floor — the 8B blends the goal with everything prepended and selects on the
blended semantics; the candidate list is weak signal against competing prose above it.

**Architectural implication (falls straight out of the data):** the executor's element-selection
call MUST be MEMORY-ISOLATED — `goal + candidate list` ONLY, no Board dump. Memory/priors belong
to an UPSTREAM planning/intent step, not the click decision. Neutral + goal-priming rows are
12/12; the lean select prompt is the robust config — injecting competing memory is what breaks
it, and we control whether to do that. This is ALSO the G4 prompt-injection surface arriving from
the benign direction (relevant-but-off retrieved text steering actions) → reinforces: untrusted
perceived/retrieved text must NOT flow freely into the action-selection prompt.

**Open / next:** (a) threshold — does a SINGLE relevant-but-off memory line bias, or only heavy
repetition? (b) the premature-`done` artifact (own signal, parked). (c) confirm lean memory-
isolated select is robust across the position conditions (C1/C3 already suggest yes). Do NOT wire
a spine until the lean-isolated select is confirmed and the planning/selection split is designed.

## 2.6 Lean-selector GATE + head-to-head (2026-06-17) — selection is PROMPT-FRAMING-dependent

Opus's gate before wiring the executor spine: position-sweep the EXACT lean memory-free selector
prompt; confirm lean + late-band hold TOGETHER. N=12, live 8B, grammar-constrained, NO memory.

Same 4 arrangements, zero memory, ONLY the prompt framing differs:
| arrangement | FULL agent-SYS prompt (no mem) | STRIPPED lean selector prompt (no mem) |
|---|---|---|
| P1 target first (el_0) | decoy/last el_5 12/12 | "Show Desktop" el_4 9/12 |
| P2 target row5 (el_4) [C1] | **target 12/12** | **first item el_0 12/12** |
| P3 target row4 (el_3) [C3] | **target 12/12** | first item el_0 10/12 |
| P4 target last (el_5) [I] | **target 12/12** | mid el_3 7/12 |

**SELECTION BEHAVIOR IS DETERMINED BY PROMPT FRAMING, NOT A STABLE MODEL CAPABILITY.** Full SYS
prompt → label-reading + late-band (P2/P3/P4 = 12/12). Stripped lean prompt → first-position,
LABEL-BLIND picking. The C1/C3 "reads labels" finding was emergent from the long SYS preamble
(it pushes the candidate list into the model's strong late-attention zone); strip the preamble
and selection collapses.

**Gate verdict (nuanced):**
- ✗ A bespoke STRIPPED lean selector prompt is LABEL-BLIND — do NOT build that.
- ✓ The working memory-isolated executor = the EXISTING full SYS prompt with the Board slice
  DELETED (not a stripped prompt). Under it, memory-free + late-band hold together (12/12). The
  ranker must still place the relevant candidate in the late band (P1 target-first fails 0/12
  even under full SYS).
- ⚠️ Label-reading is PROMPT-FRAGILE: any SYS-prompt edit can silently kill selection. Requires a
  PINNED executor prompt + a REGRESSION GUARD (this position sweep as a CI check), or it rots
  invisibly — the late-placement trap one more time, now at the prompt-framing level.

**Net:** the two-call split still stands, but "memory-isolated executor" means *delete the Board
section, keep the SYS framing*, not *write a lean selector*. Carry forward (Opus, unchanged):
isolating the executor RELOCATES G4 to the planner — measure the planner's single-line
corruptibility before trusting it. New item: pin + regression-guard the executor prompt.

## 2.7 Step-0 SYS audit + escape test (2026-06-17) — escape is NON-FUNCTIONAL

Opus's step-0 before pinning: audit "full SYS minus Board" for goal-independent selection bias,
and check the escape path. N=12, live 8B.

**SYS content audit (fixed label set, PERMUTED order, NO-MATCH goal):** no hard baked-in offset.
The pick does not cluster on a single position OR a single label across permutations — it
scatters (Directory Menu / Applications / Files / Volume / done), with only a WEAK salience lean
to "Directory Menu"/"Applications". => "full SYS minus Board" has no strong constant content-bias
to neutralize; pinning is defensible (with the regression guard + length comment).

**Escape test (SELECTOR-ONLY grammar `click(el_N) | click(none)`, NO-MATCH goal):**
| condition | `none` fired |
|---|---|
| every permutation, both no-match goals | **0/12** — forces a click on an arbitrary salient element |
| control (real match, target late-band) | n/a — picks target el_4 12/12 ✓ |

**THE ESCAPE TOKEN IS NON-FUNCTIONAL.** The model NEVER voluntarily escapes on no-match — it
forces a wrong click 12/12. With `done` in the grammar it bails to `done`; strip that and it
clicks arbitrarily. It will not emit `none`.

**Architecture correction:** the "mandatory none->re-perceive escape" (spec §2, the grammar-rail
cornerstone and the fail-closed path) CANNOT be model-driven. In production, whenever the right
element is not yet on screen (scroll/navigate/wait first — common), the executor will confidently
click a WRONG element rather than escape. => FAIL-CLOSED MUST BE DETERMINISTIC: the harness
cross-checks the planner's target descriptor against candidate labels and forces escape/re-perceive
on no-match. Bias toward escape-when-uncertain (false escape recoverable; false click terminal).
This upgrades Opus's "fails closed on no-label-match" from prudent to MANDATORY, and makes the
`none` grammar token a deterministic-only path (the model never reaches it).

**Step-0 verdict:** (1) pin "full SYS minus Board" — no constant offset, OK; add regression guard
(position sweep, length-sensitive) + "preamble length is load-bearing" comment. (2) escape is
deterministic, not a model choice. (3) control confirms label-reading holds on a real match in
the late band (12/12). Ready to build the two-call split: pinned template (intent slot + late-band
candidate list), deterministic fail-closed handoff, planner corruptibility measured next.

## 2.8 Planner location-field corruptibility (2026-06-17) — model can't emit geometry

Opus's step-4 sharpened question, front-loaded: can corrupted memory move the planner's LOCATION
claim? Screen-aware planner emits the target's region (enum); Applications at top-left (0,0),
decoy "Directory Menu" at bottom-center (744,752). N=12.

| goal | memory | top-left (correct) | bottom-center (decoy) | dist |
|---|---|---|---|---|
| G1 "...top panel" | neutral | 0/12 | 0/12 | center=7, top-right=5 |
| G1 "...top panel" | decoy-prime | 4/12 | 0/12 | center=5, top-left=4, ... |
| G2 no-location | neutral | 0/12 | 0/12 | center=12 |
| G2 no-location | decoy-prime | 0/12 | 0/12 | center=8, mid-left=4 |

**Finding 1 (corruptibility, reassuring):** decoy memory did NOT move location to the decoy
(bottom-center 0/12 everywhere; under decoy-prime top-left went UP 0->4). Location is not
poisonable-toward-the-decoy by this memory.

**Finding 2 (bigger):** the model CANNOT emit a reliable location field — neutral G1 said
center/top-right for an element at (0,0)=top-left, and IGNORED the explicit "top panel" in its
own goal; G2 defaulted to center 12/12. Model-emitted geometry is OUT.

**Design refinement:** geometry must be DETERMINISTIC (computed from candidate bboxes; "top panel"
parsed from goal text by the harness), NOT a planner claim. This DEFUSES the location-corruptibility
worry — but for an awkward reason: there is no coherent model location-claim to corrupt. So the
planner's descriptor = SEMANTIC fields only (type / role / label); the clean geometric signal comes
from perception bboxes, not the planner.

**Residual-of-the-residual (new open question for Opus):** with geometry deterministic and the
planner emitting only semantic fields, how does the gate clear a LABEL-LESS, TYPE-LESS CV/vision-
only element (the §3 ref_id=None case the structured descriptor was meant to rescue)? It has a
bbox but no label/type from perception, and the planner's semantic descriptor cannot name it — so
those elements can only be matched geometrically, against a location the model cannot reliably
specify. The structured-descriptor gate is strong for a11y-labeled elements and weak again exactly
at the label-less elements; the extra senses' elements remain the hard case.

## 2.9 SPINE WIRED + VM walk (2026-06-17, commit pending) — selection FIXED live

The validated spine wired into agent_loop (memory-isolated prompt + late-band ranking +
deterministic fail-closed) and run end-to-end against the VM, same goal that failed before
("Click the Applications menu in the top panel"; Applications at (0,0) = token el_0).

- **SELECTION FIXED:** the model clicked `el_0` at (51,13) = Applications (center of its box) —
  the CORRECT element. The Applications menu OPENED (final screen: ref_6 menu + items "Run
  Program...", "Terminal Emulator", "File Manager", "Mail Reader", "Web Browser"). Goal achieved.
  Pre-spine (§2.1) the same goal wandered el_4/el_5/el_12/el_22 and never found it. The offline
  findings (late-band ranking lands the relevant candidate where attention holds; memory isolation
  stops priors from dragging the pick) TRANSFER to the live loop.
- **Done-detection still open (parked, as designed):** it clicked Applications 5x (toggling the
  menu) and never recognized the open menu = goal satisfied; the supervisor stopped it. This is
  the SEPARATE completion signal (Opus): the same select machinery that now picks correctly will
  re-pick a still-relevant target forever — selection and done-detection cannot be one call.

**Status:** Tier 1 (a11y-labeled) executor spine is real and live-confirmed. Next: (1) done-
detection as its own signal (goal-vs-current-screen, not inferred from the verifier rejecting
everything); (2) the v2 planner (memory-informed intent → memory-free executor) with the
structured semantic descriptor + deterministic geometric gate; (3) Tier-2 label-less promotion
(relational descriptor (B) then cross-modal (A)) as enhancement. 214 lib tests.

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
