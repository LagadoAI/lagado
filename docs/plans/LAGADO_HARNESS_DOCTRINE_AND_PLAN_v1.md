# LAGADO — HARNESS DOCTRINE & EXECUTION PLAN v1.1

**Date:** June 14, 2026 · **Status:** Active doctrine. **v1.1 = v1 rewritten same-day after an adversarial review that was correct on every point.** Appends to MASTER_PLAN_v4 + RECONCILIATION_ADDENDUM v1.2/v1.3 + SUPERVISOR_HARNESS_ADDENDUM.
**Origin:** Post-recovery design session (Fedora 44). Reconciled with verified LFM2 facts (`/home/alucard/projects/research/LFM research.txt`) and an adversarial review of v1.

> **Governing reframes (read first):**
> 1. **The harness is the moat; the model is swappable.**
> 2. **Stop inventing — steal the field-tested standard.** Our "board with physics" is a re-derivation of ACT-R declarative memory (Anderson 1983), Stanford Generative Agents (Park et al., UIST 2023), and the Blackboard architecture (Hearsay-II, 1980). Convergence = the shape is right, NOT that we're first.
> 3. **We already built most of it.** `memory_tiers.rs` is ~80% of Park's scored memory. `/dev/shm` is the zero-copy hot tier. The KV-slot seam exists (stubbed). REUSE, don't rebuild.
> 4. **Build the boring version; gate complexity behind an eval with a number.**

---

## PART A — WHERE WE ARE (2026-06-14)

- Repo at `~/projects/lagado` (gh auth, LagadoAI). Plans restored from git history to `docs/plans/` + `LAPUTA HOW TO/` (gitignored, NOT pushed).
- **Milestone (pre-crash):** human-verified task completed end-to-end through the agent. The full loop closes. This is the floor.
- **Fresh Fedora 44 — nothing installed:** no Rust/Node/Tauri deps, QEMU/KVM, CUDA, models, llama.cpp build, or `~/.laputa-secure`.
- **Already built and reusable:** `memory_tiers.rs` (recency-decay `information_value`, cosine `find_similar_by_embedding`, `reinforce`, `assemble_context`); `/dev/shm` zero-copy frame path; `InferenceAdapter` KV-slot seam (`supports/save/restore/has_kv_slot`, stubbed); action_graph + skill_library.
- **Landmine:** release builds split data dir (debug-only `LAGADO_DATA_DIR` in `config.rs`; `auth/`+`vm/` read it directly). Fix before release.
- **Debt/stubs:** `perceive.py` is Python + Linux-only (violates no-Python); `grammar.rs` empty; `supervisor.rs` absent; `security/profile.rs` absent; cloud adapter absent; retrieval Jaccard (no text embeddings); TASK 7 (PerceptionMode/CSV) unbuilt; tine `tree --json` gap blocks selector-clicks.

---

## PART B — THE MAJOR ISSUES

1. **Small models can't run long-horizon loops smoothly.** VERIFIED: multi-turn compounding (~0.63⁵≈10%/5 turns), premature commitment, no recovery once wrong, temperature doesn't help. THE problem.
2. **Cold start / "baby AI"** — must be useful day one.
3. **Perception fidelity gates everything.**
4. **Linux + Python lock-in** → must be cross-platform, pure Rust.
5. **VM transport unsettled** (QEMU vs libkrun).
6. **Host control now viable** (Fedora, vision model capable) but unbuilt.
7. **Model lock-in risk** — edge-model race is live.
8. **Sovereignty promise needs architectural teeth**, especially with cloud in the loop.
9. **Consumer reach / differentiation vs Claude.**

---

## PART C — THE DOCTRINE (vetted; corrected per review)

**C1 — Spend the glass's tokens on the highest-value slice. (NOT "losslessness.")** The glass is small *because you must throw most away.* The target is the right lossy *selection*, not zero loss — "more fidelity = better" is FALSE for a context-bounded model. The lever is selection (C3), not plumbing-polish.

**C2 — Externalize state; every model step is single-turn-fresh; the slice-assembler is DETERMINISTIC.** Never run a long multi-turn conversation in the model (verified failure). Re-present a clean, fully-specified slice each step. Two priced trades, named consciously:
- (a) **KV-cache:** single-turn-reset forfeits cache reuse → mitigate via llama-server `/slots`: cache the stable prefix (system + tools), re-encode only the volatile slice. The `kv_slots` seam exists (stubbed) — wire it. Tie to `/dev/shm` zero-copy.
- (b) **The assembler is deterministic code (board top-k), NOT a model call** — a model assembler inherits the exact premature-commitment unreliability we're routing around. Push selection to deterministic.
SUPPORT: Microsoft "LLMs Get Lost in Multi-Turn"; NVIDIA SLM paper; memory-agent line.

**C3 — The Board = a standard SCORED memory store (Park), NOT a physics engine.**
- Score (Park, recomputed STATELESS per step, top-k): `score = α·recency + β·relevance + γ·importance`. recency = exp decay on last-access; relevance = cosine(now-vector, particle); importance = rated at write.
- **We already built recency + relevance + top-k in `memory_tiers.rs`.** Add the importance term, unify into one recomputed scorer, wire as THE slice-assembler. REUSE.
- **Drop the field/physics.** No coupled forces. **Conduction (= ACT-R spreading activation) is OFF by default** — Park omits it; it's the most thrash-prone, hardest-to-tune part. Add it ONLY if a retrieval eval (G3) shows the stateless score misses something real.
- **Retrieval ≠ planning — separate explicitly.** The board surfaces candidate *ingredients* (a ranked bag). A separate, NAMED, deterministic **sequencer** does ordering / dependencies / preconditions. "The hot slice IS the plan" was false; surfacing ≠ ordering.
- **"Cool, don't delete"** = Park's append-only stream (their design, not our novelty) — but tiered (see G1), not infinite RAM. Hot tier → `/dev/shm` zero-copy.

**C4 — Reapproach = reset-from-corrected-board, WITH an escalation ladder.** On a wrong turn: diagnose → write correction to board → restart clean. BUT diagnosis is itself a fallible call → **bounded retries (N) → escalate: 8B → (optional) cloud → HITL gate (already built).** Without the ladder you get reset loops. `supervisor.rs` owns this.

**C5 — Per-call slice shapes, AND guard the router that picks them.** Routing/extraction → minimal, single-turn, no-CoT. Ambiguous planning → short scratchpad, then write-to-board-and-clear. The routing-vs-planning classifier is a fallible call; misrouting a planning step into no-CoT is a SILENT failure → conservative default: when unsure, treat as planning.

**C6 — Born flightworthy via LEARNED pipes, not hand-authored ones.** Pre-seeded 25–30k static entries ROT like perceive.py's DOM assumptions (apps change UI). Make **edge-learning PRIMARY: record successful traces → promote to action-graph pipes, self-healing.** If hand-seeding at all, seed THIN (~50 highest-frequency actions), let edges fill by observation. (Revises MASTER_PLAN Phase 4's 25–30k pre-seed.)

**C7 — Blackboard (Hearsay) with an EXPLICIT merge policy.** Multi-agent coherence = shared board + reintegration (Hearsay, 1980). The hard part "reintegration" hand-waved = **concurrent contradictory writes.** Specify the merge policy NOW: last-write-wins (as we do for artifacts) / mass-weighted / reconciliation pass. Unspecified = silent loss under real parallelism.

**C8 — Why LFM, and keep the DATASET not the checkpoint.** Use LFM for edge-CPU efficiency + shippable license + agentic variants + cheap fine-tune. Caveats: "10%→96–98%" is distil-labs' surface (existence proof, NOT a forecast — re-measure on ours); fine-tuning locks a checkpoint, fighting InferenceAdapter portability → **the durable asset is the fine-tuning DATASET (regenerable on any base); the checkpoint is swappable.**

**C9 — What we steal vs what we invent.** Borrow: Park's scoring (baseline), ACT-R spreading (only if G3 demands), Hearsay's control loop (C7). **Our narrow, defensible novelty: the LFM2-specific, edge-CPU, single-turn-reset harness around a standard scored board.** Invention budget goes ONLY there.

---

## PART C2 — GAPS THE DOCTRINE MUST CLOSE (each can sink us)

- **G1 — Eviction / capacity.** "Cool don't delete" + O(N) rescore/step = unbounded growth + linear step-latency decay on the target CPU. Park is append-only because it's a research sim; we ship. Need **cold-particle archival** (disk tier, dropped from the live scored set) + a recall path. "Recoverable" = a tier, not RAM residence. (`memory_tiers` hot/warm/cold + entropy_prune already half-does this — extend it.)
- **G2 — Write quality / importance gate.** Garbage particles → garbage slice. Name who decides what becomes a particle + sets importance. Park uses an LLM rating at write (a model call). Decide: deterministic heuristic vs cheap model rating. As load-bearing as retrieval.
- **G3 — Retrieval eval.** No way to know the board surfaces the right slice. Build a labeled "given this state, correct slice = X" eval set BEFORE tuning α/β/γ. This is the gate that decides whether conduction/physics ever earns complexity. Without it = tuning by vibes.
- **G4 — Particle trust boundary (prompt injection). CRITICAL.** Perceived DOM/screen text → particles → hot slice → model context. Hostile page text can write into working memory and reach the model. Need a **trust tier on particles** (`perceived-untrusted` vs `user-intent-trusted`) so a malicious page can't promote itself into a tool-routing slice. This is the perception-side analog of the HITL gate — currently missing. Hard requirement for the browser surface (Phase 7).

---

## PART D — PHASED PLAN (vetted)

### Phase 0 — Resurrection & Ground Truth [unblocks all]
Toolchain (dnf): rust, node, Tauri deps, qemu-kvm+libvirt, CUDA+gcc/cmake, python3 (interim). Rebuild `~/.laputa-secure` + templates; re-download LFM2.5 GGUFs; build vendored llama.cpp **with CUDA**; regen host SSH key; fix data_dir landmine. Green checks/tests; re-verify the human task on Fedora.
**Evals (the report's ask):** (1) single-vs-multi-turn tool-routing on LFM2.5-1.2B with our real schema on the 3060 → our compounding number. (2) Begin the **G3 retrieval eval set.**
**Vet:** mechanical except the evals. vm-images DEFER to Phase 5. Risk: low.

### Phase 1 — The Board = Park score on top of `memory_tiers` [reuse, don't rebuild]
Add importance term + a single recomputed `α·recency+β·relevance+γ·importance` scorer over `memory_tiers`; deterministic top-k slice-assembler; hot tier in `/dev/shm`; wire `kv_slots` prefix reuse (C2a). Add the **particle trust tier (G4)** and **importance/write gate (G2)** from the start. **Separate sequencer** (deterministic) for ordering. **Conduction OFF.** Tune α/β/γ ONLY against the G3 eval.
**Vet:** needs text embeddings (decide source: LFM2 embed/ColBERT variant vs small dedicated embed model) — folds in the Jaccard→embedding upgrade. Reconcile with chronos/action_graph/skill_library (extend, don't duplicate). Eviction = G1 (extend memory_tiers tiers). Risk: medium. **Gate:** embedding source; G3 eval must exist before tuning.

### Phase 2 — Single-turn reflex loop + supervisor + grammar [highest-ROI smoothness]
Rework `agent_loop` to refill from the board slice each step (kill the growing prompt). Build `supervisor.rs` = reset-from-corrected-board **+ escalation ladder (C4)**. `grammar.rs`: real GBNF (valid tools + on-screen ref_ids). Per-call slice shapes + **guarded router (C5)**. Pin bracket parser to real runtime; LFM2.5 sampling (temp 0.1/top_k 50/rep 1.05; never share with LFM2-gen).
**Vet:** all harness, buildable. Risk: medium (supervisor). **Gate:** Phase 1.

### Phase 3 — Perception: pure Rust + cross-platform + finish fusion [the floor]
Port `perceive.py` → Rust (`atspi`) INTO a per-OS a11y trait (Linux now; mac AX / Win UIAutomation later). Close tine gap. Finish TASK 7 (PerceptionMode + CSV) → prove fusion beats a11y-alone. CV+VLM senses port free.
**Vet:** big; gates computer-use; no-Python non-negotiable. Risk: medium-high (verify `atspi` maturity early). Parallelizable with 1/2.

### Phase 4 — Host control with a sanctioned safety-guard [competitiveness]
Host projector rides the OS's own sanctioned agent/host-access (Linux portals/a11y first; Win UIAutomation/MS agent host; Mac AX/ScreenCaptureKit/TCC) + our perception on top.
**Vet:** Linux host first. Risk: medium — Win/Mac sanctioned-API research needed before those targets. **Gate:** that research before non-Linux.

### Phase 5 — VM settled by evidence + Boxes-grade UX [moat + reach]
Adversarial QEMU vs libkrun/krunvm comparison, fact-verified (criteria: perceivable desktop, GPU passthrough, boot/overhead, isolation, mac/Win HVF portability, maturity). Evaluate **libvirt** as substrate (intersects transport + the Boxes premise). Study Boxes (GPL — don't copy). Build dead-simple create/manage + bring-your-own-gateway-ISO flow. Optional Whonix-STYLE gateway (dead-man's-door), available not forced.
**Vet:** VM stays (moat). Risk: medium. **Gate:** QEMU-vs-libkrun research is a hard gate before transport build.

### Phase 6 — Model-agnostic seam + the sovereignty wall [the promise]
Confirm `InferenceAdapter` generic; add cloud (Claude) adapter behind the wall. Wall = egress control + `security/profile.rs` (Strict/Balanced/Open) + audit log → architecturally impossible for cloud-mode to leak the vault. User picks the mode. Keep the fine-tune DATASET as the durable asset (C8).
**Vet:** promise gets teeth; depends on egress/profile/audit (segment 5, partly unbuilt). Risk: high (security-critical; Phase 16 audit applies). **Gate:** security review mandatory.

### Phase 7 — Reach: voice + browser [consumer differentiation]
Voice: whisper.cpp (LOCAL, MIT, GGML) input + Piper (local) output → hands-free; local-only, explicit on/off, audio never persists. Browser extension: DOM perception+actuation (strong consumer on-ramp, OS-agnostic) — MUST enforce the G4 particle trust tier (hostile DOM is the canonical injection vector) and sit inside the wall.
**Vet:** high consumer value; whisper.cpp fits the GGML stack. Risk: low-medium. **Gate:** G4 + Phase 6 (or co-designed).

---

## PART E — CROSS-CUTTING
- **Progressive disclosure** — simple-by-default, depth-on-demand. Folds all user types in without diluting; reconciles "build for all" with v1 surface discipline (4–5 things at premium depth). Architect for all; surface progressively; ship depth in slices.
- **Boxes-grade simplicity** = UX north-star.
- **The moat is sovereignty + ease + experience — never "we have an agent."**

---

## PART F — OPEN DECISIONS (gates needing the user)
1. **QEMU vs libkrun** — research gate before Phase 5.
2. **"Model agnostic" scope** — confirm BOTH OS- and inference-model-agnostic.
3. **Board embedding source** — LFM2 embed/ColBERT vs separate small embed model.
4. **Board: extend `memory_tiers` vs new organ above it** (lean: extend).
5. **Importance/write gate (G2)** — deterministic heuristic vs cheap model rating.
6. **Sequencing** — proposed 0 → (1,2,3 parallel-ish) → 4 → 5 → 6 → 7.
7. **vm-images now or deferred** to Phase 5.

---

## DECISION LEDGER

| ID | Decision | Status |
|---|---|---|
| H-1 | Harness is the moat; model swappable (InferenceAdapter) | ✅ LOCKED |
| H-2 | "Continuous-reflex liquid" retired; LFM for edge efficiency/license/variants/fine-tune | ✅ VERIFIED |
| H-3 | Board = Park-scored store over `memory_tiers`; NO physics engine; conduction OFF by default | ✅ CORRECTED-LOCKED |
| H-3a | Retrieval ≠ planning; separate deterministic sequencer | ✅ LOCKED |
| H-3b | Slice-assembler is deterministic, not a model call | ✅ LOCKED |
| H-4 | Reapproach = reset-from-corrected-board + bounded-retry escalation ladder | ✅ LOCKED |
| H-5 | Externalize state; every model step single-turn-fresh; KV-slot prefix reuse | ✅ LOCKED |
| H-6 | Born flightworthy via LEARNED pipes (edge-learning primary); seed thin if at all | ✅ CORRECTED-LOCKED |
| H-7 | Blackboard needs an explicit merge policy for concurrent writes | ⏳ POLICY TBD |
| H-8 | Keep the fine-tune DATASET (durable), not the checkpoint | ✅ LOCKED |
| H-9 | Build the boring stateless version; gate physics behind the G3 eval | ✅ LOCKED |
| G1 | Eviction/archival tier (extend memory_tiers) | ⏳ REQUIRED |
| G2 | Write-quality / importance gate | ⏳ REQUIRED (decision F5) |
| G3 | Retrieval eval set before tuning | ⏳ REQUIRED |
| G4 | Particle trust tier (prompt-injection boundary) | ⏳ REQUIRED (critical for browser) |
| H-10 | QEMU vs libkrun | ⏳ RESEARCH GATE |
| H-11 | Two isolation surfaces; three model modes walled; sovereignty guaranteed | ✅ LOCKED |
| H-12 | Pure Rust + cross-platform; perceive.py→Rust into per-OS a11y trait | ✅ LOCKED |
| H-13 | Voice = whisper.cpp + Piper, LOCAL only | ✅ LOCKED |
| H-14 | Progressive disclosure serves all user types | ✅ LOCKED |

*— End Harness Doctrine & Plan v1.1. Steal the standard; spend invention on the LFM2 edge-CPU single-turn harness. Build the boring board; let the eval decide the rest.*
