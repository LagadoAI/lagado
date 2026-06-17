# LAGADO — HARNESS BUILD SPEC v1

**Date:** 2026-06-16 · **Status:** Active execution spec. Builds on (does not replace) `LAGADO_HARNESS_DOCTRINE_AND_PLAN_v1.md` (doctrine v1.1). Captures the design converged in the 2026-06-16 session and the concrete build order. Author drives as Opus, no delegation.

---

## 0. THE SPINE (organizing principle — applies everywhere)

**A deterministic floor that always works + a model upgrade layered on when affordable, governor-arbitrated, off the hot path.** The floor is the sovereignty guarantee (the agent runs fully local on weak hardware, offline, every time); the upgrade is the excellence. This single shape recurs in:
- **Router** — grammar + deterministic parse (floor) + 8B / confidence escalation (upgrade).
- **Importance/write gate (G2)** — deterministic heuristic on the write path (floor) + model rating async in the sleep gate (upgrade).
- **Conduction** — stateless Park score (floor) + spreading-activation traversal (upgrade, eval-gated).
- **Perception** — a11y tier (floor) + CV/VLM/DOM tiers (upgrades, governed).
- **Reapproach** — C4 escalation ladder (N → 8B → cloud → HITL).
- **Model modes** — local (floor) / hybrid / cloud (upgrades).

Corollary: every "expensive model" path runs OFF the hot path (async refinement, demand-driven escalation) and is throttled by the governor against real hardware. Storage is never gated (keep all data); only *influence* is gated (by corroboration + decay).

---

## 1. WORK ORDER (this build)

### ① Grammar-constrained router + 8B fallback — ✅ DONE (commit cfde176)
The routing eval showed the 1.2B emits un-parseable tokens ("Escape", "Search") that silently default to CHAT → an action request no-ops. Fixed:
- `grammar.rs::intent_grammar()` → real GBNF `root ::= ("CHAT" | "INTERACTIVE" | "REASONING")`.
- `InferenceAdapter::generate_constrained()` (non-breaking default delegates to `generate_with_confidence`).
- `llama_cpp`: single private `request()` builder feeds generate / with_confidence / constrained → sampling params can never drift again; passes `grammar` + `logprobs`.
- `hydra::classify_intent`: grammar-constrained, falls back to the 8B if the classifier is down, logs confidence.
- **Deferred:** the C5 confidence gate ("when unsure → treat as planning") — confidence is logged but NOT yet gated; the floor will be set by the routing eval, NOT by a vibes number.
- 166 lib tests.

### ② Close G3 retrieval eval + Jaccard→ColBERT — IN PROGRESS
**Why:** the gate before any α/β/γ tuning (doctrine H-9 / G3). No `memory.db` exists yet on this box.
- Schema (confirmed in `memory_tiers.rs::open`): `memory_entries(id TEXT PK, text TEXT NOT NULL, tier TEXT, temperature REAL, created_at INT, accessed_at INT, access_count INT, embedding BLOB)`. The eval's INSERT matches.
- Steps: stand up a dedicated eval DB with that schema → `--seed` the G3 set → `--eval` for the **Jaccard floor** (Precision/Recall/F1 @K).
- Then upgrade: wire **LFM2-ColBERT-350M** and re-measure. ColBERT is *late-interaction* (token-level multi-vector, MaxSim), NOT a single pooled vector — design choice to resolve at that step: (a) MaxSim late-interaction (most faithful, heavier), vs (b) pool ColBERT token vectors to one vector to fit the existing `embedding BLOB` + cosine path (cheaper, loses late-interaction edge). Decide by what the G3 number demands.
- **Acceptance:** a recorded Jaccard F1 baseline + a ColBERT F1 that beats it meaningfully, else ColBERT's complexity isn't earning its keep.

### ③ Phase 1 — The Board (extend `memory_tiers`, don't rebuild) — sequenced ③a/③b/③c (advisor 2026-06-16)

**The Park slice-scorer is a NEW function, NOT `information_value`.** `information_value` (T×recency×reinforce, multiplicative, λ=ln2/30d) is the *entropy/pruning* scorer = "what to forget." The Park score is additive, recomputed per step = "what to surface now." Different masters, likely different time constant — keep them separate.

**MANDATORY: transform the relevance term BEFORE it enters the additive sum.** ColBERT pooled cosines span [0.96, 0.99] — a 0.03 band. Raw, relevance contributes a near-constant ~0.97β to every particle and goes inert; recency+importance (range ~[0,1]) drown it. So the slice-assembler computes raw cosine for the candidate set, then **rank-based or per-query min-max normalizes relevance across candidates**, THEN combines: `α·recency_norm + β·relevance_norm + γ·importance`. Normalization restores *range* but NOT the *ordering* errors on abstract queries ("browser earlier" pulled noise above firefox) — that ordering failure is the **conditional MaxSim trigger**: if normalized relevance still drags noise into the slice after ③ tuning, late-interaction MaxSim is justified; until then it's deferred.

**G3 tunes β-quality ONLY — it is NOT a 3-weight oracle.** The fixture sets `created_at=accessed_at=now`, `temperature=1.0`, `access_count=1` for every entry and has no importance labels → recency is uniform, importance unlabeled → **α and γ have nothing to tune against.** Set α/γ by principle (sane defaults) for now; do NOT grid-search 3 weights on 6 queries (overfit). Enrich the fixture later (real temporal spread + importance labels + queries whose right answer depends on recency/importance) only if ③ shows α/γ matter.

**Parity test before trusting any Board number:** the F1=0.52 was proven in Python (urllib→server→f64 cosine). The Board uses `memory_tiers::find_similar_by_embedding` over f32 BLOBs — embedding normalization, BLOB round-trip, f32-vs-f64 can silently diverge. **③a includes a test asserting Rust retrieval order matches the Python eval on the same fixture.**

- **③a (floor):** deterministic importance heuristic + the new additive Park scorer (with the relevance transform) + the deterministic top-k slice-assembler + the Rust↔Python parity test + unit tests.
- **③b:** G4 particle trust tier — tag `user-intent-trusted` vs `perceived-untrusted`; untrusted perceived text can't promote into a tool-routing slice without the gate.
- **③c:** G2 dual-path importance — model rating as async refinement folded into the **sleep gate** (already runs every 5 min), governor-arbitrated; the deterministic ③a heuristic stays the always-on floor.
- **Conduction OFF:** build & store every edge from day one (the richness, no data discarded) but gate the spreading TRAVERSAL until G3 proves the stateless score misses something; even then governor-throttle depth.
- **Separate deterministic sequencer** for ordering (retrieval ≠ planning).
- **Acceptance:** β-relevance (against G3) ≥ ColBERT mean-pool F1 0.52 *through the Rust path* (parity-verified); slice-assembler is pure deterministic code; relevance transform present; trust tier + deterministic write gate from the start.

### ④ Phase 2 — Single-turn reflex loop + supervisor
- Rework `agent_loop` to **refill from the board slice each step** (kill the growing prompt) — every model step single-turn-fresh.
- Build `supervisor.rs` = **reset-from-corrected-board + escalation ladder** (N retries → 8B → optional cloud → HITL). Diagnosis is itself a fallible call, so the ladder is mandatory or you get reset loops.
- KV-slot prefix reuse (C2a): cache the stable prefix (system+tools), re-encode only the volatile slice (the `kv_slots` seam is stubbed in `inference/mod.rs`).
- **Acceptance:** measurable — the single-turn loop beats the growing-context loop on a multi-step task on this hardware (the core bet).

**④b SURGERY PLAN (oriented 2026-06-17 — the highest-risk change in the codebase; do it advisor-backed + fresh, not at the tail of a long session):**
- **④a DONE:** `supervisor.rs` (governor-injected ladder, pure state machine, commit cef238e) + `governor::escalation_ladder()` (local→human; builds what the supervisor consumes).
- **CORRECTION (advisor 2026-06-17) — the trajectory is NOT the board slice. Resolve THIS on paper before any code (④b task #1):**
  - The loop is ALREADY mostly single-turn-fresh: `agent.rs:382` rebuilds the prompt from scratch with `format!()` every iteration — it is NOT a growing chat thread (which is what the LFM multi-turn-degradation research was about). **The ONLY thing that grows is `memory.context_string()` (line 353).** Verify that's the only unbounded thing in the prompt.
  - `assemble_slice` (③a) returns relevance-**top-k of persisted, text-embedded rows.** It canNOT carry the current-turn trajectory (in-flight steps aren't embedded — embedding-on-write is ③c — nor persisted), and top-k would DROP steps the model needs to avoid repeating itself. Trajectory wants **full, recency-ordered, bounded**; the board wants **scored top-k**. Different presentation modes — folding them into one `assemble_slice` is a bug.
  - So single-turn-fresh = "re-present a clean, **bounded, deterministic** slice = goal + **full structured trajectory** + **top-k relevant persistent priors (this is where `assemble_slice` goes — replacing the episodic/visual/skill priors source)** + live screen." We kill *uncontrolled accumulation*, not the trajectory.
  - **Likely scope: NOT a core rewrite.** (a) bound/structure the trajectory string (`memory.context_string()` → capped, recency-ordered working set; port `should_cutoff`'s 3rd-identical-action intent into it or the supervisor); (b) swap the priors source (line 360-374) to `assemble_slice`; (c) wrap the existing step handling with `supervisor.observe(outcome, state_hash) → Directive`. Decide bound-vs-rewrite as task #1 — advisor bets bound.
- **DO NOT bulldoze the existing machinery — integrate.** The loop already has: `should_cutoff`/impasse (line 417), screen-hash loop detection (416), `StepEnforcer` (step cap), `RecoveryManager` (inner tactical recovery). The supervisor is the OUTER ladder; map the loop's signals → `StepOutcome` (executed? screen_unchanged? Done/Task? error?) → `supervisor.observe(outcome, state_hash)` → obey `Directive` (Continue / ResetFromBoard / Escalate(tier) / Done / Abort). Reconcile the supervisor's stall/loop detection with the existing impasse/cutoff logic (pick one owner; likely the supervisor subsumes `should_cutoff`).
- **Escalate(tier):** local-only ladder = `[model("local"), human()]`, so Escalate(Model) currently == retry-on-8B (already the main adapter) and Escalate(Human) == HITL handoff. Keep it minimal until hybrid/cloud land.
- **KV-slot reuse (C2a):** cache the stable prefix (system+tools), re-encode only the volatile slice — the `kv_slots` seam is stubbed in `inference/mod.rs`. Optional first cut; correctness before this optimization.
- **Build it floor-first + tested:** (0) **task #1: define the working-set/trajectory representation as DISTINCT from the board slice; decide bound-vs-rewrite — on paper first**; (1) a pure `classify_step_outcome(...)` mapping fn (testable); (2) bound/structure the trajectory + swap priors to `assemble_slice` (the single-turn-fresh core — NO writing trajectory to the board); (3) `supervisor.observe` wiring + Directive handling; (4) port `should_cutoff` intent into the supervisor (don't just delete — it has tuned 3rd-identical-action behavior). Test each before the next.
- **Acceptance is VM-gated:** "single-turn beats growing-context on a multi-step task" needs the VM running real tasks (not verified). So ④b "done" = builds + runs single-turn-fresh + unit-tested; the *proof of the bet* waits on VM-time. Don't let the unmeasurable masquerade as done.

- **④b TASK #1 RESOLVED (2026-06-17, read the loop):** bet confirmed — **bound, not rewrite, and the bound already exists.** Ground truth:
  - **Trajectory is already bounded AND already distinct from the board.** `memory.rs::Memory` compacts after 10 steps → 5 LLM-summarized + 5 full-detail (`context_string()` = summaries + recent). `agent.rs:382` already rebuilds the whole prompt fresh each step; only `context` (the trajectory) grows, and it's self-capping. So (a) "bound the trajectory" needs **no change**. Trajectory stays as the working set; board stays separate. No trajectory is ever written to the board.
  - **④b DONE (2026-06-17).** Change B (supervisor outer bound) committed `5eceb5d`. Change A (Board live) committed next: ColBERT embedder stood up (`bootstrap::ensure_embedder_server`, CPU-only, ctx DISCOVERED from GGUF), watched by `server_guard`, backfilled by `sleep_gate` (Step 5; `entries_missing_text_embedding` now DECRYPTS cold ciphertext → never embeds garbage), consumed by `agent_loop` via `assemble_slice` + recency floor. Smoke-verified live: embedder spawns on the exact bootstrap flags, `/v1/embeddings` returns 128-dim L2-normed vectors, cosine ranks a related memory above an unrelated one (0.980 vs 0.970 — compressed range is why the board min-max-normalizes before the Park sum). Visible payoff still VM-gated (needs accumulated memory + multi-step tasks). The two changes were:
  - **(A) Priors source → board.** Replace the pre-loop `assemble_context(2048)` (static, recency, lines 289-292) with `assemble_slice(embed(goal), BOARD_TOP_K=8, ParkWeights::default())`. Spine floor: ColBERT down **or** empty board (fresh machine has no `text_embedding` rows yet) → fall back to `assemble_context`. Format each `MemoryEntry` as `- {e.text}` (matches existing priors shape). **(B) Supervisor = outer escalation bound.** `Supervisor::new(governor::escalation_ladder())` before the loop; a pure `classify_step_outcome(is_terminal, action_executed, screen_changed) -> StepOutcome`; after the step resolves (post line 550), `observe(outcome, u64(blake3(screen)[..8]))` → act. **observe() runs EVERY step** (keeps stall/loop/retry state live), but the loop **only acts on `Escalate(Human)` → clean HITL break and `Abort` → clean abort**; `Continue`/`Done`/`ResetFromBoard`/`Escalate(Model)` fall through to the existing tested inner machinery (recovery_manager, `should_cutoff`, structural-failure). Rationale: inner tactical recovery WORKS + is tested; the supervisor's unique add right now is the graceful outer "ladder exhausted → hand to human" bound (ladder = `[local, human]`), plus it catches the **different-actions-all-stall** case `should_cutoff` (identical-action-only) misses. Genuinely additive, minimal blast radius. Later (cloud/hybrid unstub) migrates inner breaks under supervisor authority and makes ResetFromBoard/Escalate(Model) act.

---

## 2. SELECTOR GRAMMAR over the FUSED set (Phase 3 forward — design locked)

①'s grammar was trivial (fixed 3-label vocab). The **action-selection** grammar is the hard one because perception is 3–4 layers fused.

- **Trap:** `FusedElement.ref_id` is `Option<String>` — `None` for CV-only/DOM-only/vision-only elements. A grammar over `ref_id` can only name a11y-backed elements → silently collapses fusion back to a11y-only (loses exactly the elements fusion exists to recover).
- **Fix:** constrain over a **synthetic per-frame index** the arbiter assigns to every `FusedElement` (the existing deterministic `(y,x,w,h)` sort is the stable id space). `selector_grammar(&[String])` → `selector_grammar(&[FusedElement])` emitting index productions. Actuator resolves index → bbox-center → coord click.
- **Free win:** index→bbox-center→coord click sidesteps the `tine tree --json` selector gap entirely (raw coord clicks already work); `ref_id` demotes to semantic label + trust provenance.
- **Vision/VLM patches = enrichment** (they attach to a11y/CV boxes), NOT a selection vocabulary. Grammar enumerates {a11y ∪ CV ∪ DOM}; vision rides along as embedding/validation.
- **Mandatory escape production:** `none-of-these → re-perceive` — the grammar is a hard rail over a fallible candidate set; without the escape, a fusion miss becomes a forced wrong click. This escape is also the perception-escalation trigger (§3).
- **G4:** constraining to perceived candidates blocks hallucinated off-screen targets (security win), but perception-injection flows into the grammar → indices stay trust-tagged; untrusted ones gate through HITL.

---

## 3. TIERED PERCEPTION GOVERNOR (Phase 3 forward — design locked)

Perception = a governed tiered sense-fusion. **Two control loops, kept separate or it thrashes:**
- **Tier ladder (cheapest first):** a11y (AT-SPI2) = always-on FLOOR → +CV proposer (CPU) → +VLM patch embeddings (450M/libmtmd, GPU, expensive) → +DOM (cheap, browser only).
- **Main governor = the ENVELOPE (strategic, slow):** owns hardware truth + local/hybrid/cloud mode; sets the ceiling, e.g. "a11y always, CV allowed, VLM episode-boundary-only @1280×800, never per-frame."
- **Perception governor = real-time per-frame adaptation WITHIN the envelope (tactical):** runs the cheapest sense-mix that yields a confident-enough fused candidate set; escalates a tier only ON DEMAND (sparse/low-confidence fused set, dense region, action just failed).
- **Loop closure:** the selector grammar's `none-of-these` escape IS the escalation trigger → climb a tier → richer grammar → retry; bounded by the envelope (hardware ceiling) AND the C4 retry ladder (no infinite loops).
- **Seeds in code:** "VLM fires at episode boundaries only" = already tier-frequency control → generalize to a governed knob (off / episode-only / per-frame). **TASK 7 (PerceptionMode + CSV) = the perception governor's cost model** (latency-vs-fidelity numbers — same role as the main governor calibrating on real runs).
- **Degradation = the sovereignty floor:** never "turn vision off"; always best-feasible tier-mix.

---

## 4. THOUGHT / TRUST PANEL (separable; build "if easy" as the live-audit instrument)

User-facing transparency surface, NOT the Board. Structured typed event stream (reasoning / tool_call / search-with-sources / decision), each a card. **Never render `<>`** (buffer partial delimiters, or enable llama.cpp reasoning-split — the 8B emits no `reasoning_content`/`<think>` by default, needs `--reasoning-format`). Two-tier + self-folding (alive collapsed status line; long chunks collapse to a summary). Unified morphing send→stop→pause icon (stop = Abort path; pause = stop→edit + cycle branches). Surfaces G4 provenance. Anthropic/Mac-grade clean. Branching = two stores (linear canonical chronos + a separate conversation graph); chronos = multi-resolution pyramid via the sleep-gate rollup; rejected branches routed to preference signal, influence gated by corroboration+decay. (Full detail in memory `lagado-feature-designs`.)

---

*Sequencing: ② → ③ → ④ on the critical path; §2/§3 are Phase 3 (consume the governor envelope); §4 separable/parallel. Build the boring floor; let the eval decide the rest.*
