# LAGADO AI — RECONCILIATION ADDENDUM v1.2
**Date:** June 1, 2026 · **Appends to:** Reconciliation & Corrections Record v1.1
**Status:** Strategic decisions locked. These bind all downstream work.

> v1.1 still holds in full. This addendum records four decisions settled in discussion plus a
> mandatory pre-work step. Where this conflicts with anything earlier, this wins.

---

## A — LICENSING & BUSINESS MODEL **[LOCKED]**

**Lagado is proprietary, closed source.** No open-sourcing now. ("Maybe eventually" is a future
lever, never an obligation.)

- **L-1 CLEARED.** LFM Open License v1.0 is Apache-2.0-based: broad, royalty-free, perpetual rights
  to use, modify, fine-tune, and distribute the models and derivatives, **including in proprietary
  commercial products**. No copyleft — fine-tuned/modified models may stay closed. Free commercial
  use until **$10M USD total annual company revenue**, then a commercial deal with Liquid is
  required (sales@liquid.ai). Conditions to honor:
  1. **Attribution** — visible "Powered by Liquid / built on LFM2.5" notice (about screen).
  2. **Document modifications** — note quantization/fine-tuning in a changelog.
  3. **Include license text** — bundle the LFM Open License with shipped weights.
  4. **Patent peace** — patent grant terminates if Lagado initiates patent litigation. Don't.
  5. License terminates on violation → must stop use + delete copies. Stay attributed + under $10M.
  - Folds into the L-2 root LICENSE/NOTICE task.
- **Verifiability without open source:** the "zero telemetry / nothing leaves" claim (L-6) is
  discharged by (a) a **CI egress test** asserting no network except the gateway [now], and (b) a
  **third-party audit + reproducible builds + signed binaries** [before enterprise sale]. This beats
  open source for buyer trust and keeps the moat. "Open the egress/crypto sliver only" stays a
  back-pocket option.
- **Partnership with Liquid:** the $10M license conversation is the natural, warm entry point —
  arrive as a proven revenue-generating edge-AI showcase. Don't force it now; build the showcase.

---

## B — ADAPTIVE RESOURCE GOVERNOR (core subsystem) **[LOCKED]**

**Thesis: a system as adaptive as the AI it hosts — "AI anywhere."** Not CPU-first, not GPU-first;
it **senses the substrate and configures itself** to produce the best AI that machine can run.
Feasible because LFM2.5-8B-A1B is MoE (~1B active params/token), so total weights sit in RAM (~5GB
@Q4) but only a fraction compute per step → usable CPU token-rates. The dual-brain split favors CPU
(fast router stays snappy; heavy reasoning is MoE-cheap).

This **is** the "resource governor" flagged as the one missing module. It is the spine, not a
feature. `system/detect.rs` is its front half (promoted from "feeds Hydra sizing" to full detection).

**Detects (launch + runtime):** physical vs logical cores; SIMD (AVX2/AVX-512/AMX/NEON); **memory
bandwidth** (the real CPU-inference bottleneck — core count alone lies); total + available RAM; GPU
presence/VRAM/backend (CUDA/ROCm/Vulkan/Metal); OS scheduler.

**Tunes (replaces hardcoded flags):** which models load + quant level; context length + KV cache
quant (q8_0/q4_0 on thin boxes); thread count (≈ physical cores; hyperthreads often hurt); batch +
`n_parallel` (1–2 on CPU); GPU offload split (`--n-gpu-layers`) when VRAM present; interactivity
protection (nice/cgroups so the machine stays usable).

**v1 minimum:** detect → pick a sane profile → generate the llama-server launch config with a clean
**CPU-only fallback** (no hardcoded `--n-gpu-layers 99`/`--flash-attn on`). **North-star:** continuous
runtime re-balancing (throttle when user active, expand on idle/sleep) + fidelity-cost accounting.

---

## C — FIDELITY IS THE PRIMARY RESOURCE **[LOCKED — governing principle]**

Borrowed from quantum-OS design: optimize for **trustworthy correctness, not raw speed**. Every
subsystem is fidelity-aware (reports its own confidence; recalibrates strategy on drift). This
resolves the secure-vs-experience tension: the experience sold is *trust*, not benchmarks. Sits
beside the three pillars (Sovereign · Living · Self-aware-in-time) when judging features. Health/
calibration loop (parse-failure rate, router-accuracy drift, action-graph confidence decay, memory
temperature) unifies the scattered monitor/heartbeat/sleep-gate pieces and surfaces via the
brain-routing transparency badge.

*(Forward note, not now: for a vault protecting data for years, plan a hybrid-PQC path for any
**asymmetric** crypto — audit-log signing, TLS, update signing, future sync. AES-256-GCM symmetric
is already quantum-safe.)*

---

## D — MANDATORY PRE-WORK: SECURE THE TREE **[STEP 0 — before anything]**

The working tree (`/home/d/laputa`, branch `master`) has **35 uncommitted changes and no git
remote.** All unprotected.

- **D-0.1** — commit the 35 changes on a clean checkpoint **before** Phase 1.3 and **before** the
  rename. The atomic rename must start from a clean tree or rename-churn becomes impossible to
  separate from pending work.
- **D-0.2** — add a backup remote (even a private/local mirror) so `master` isn't single-point-of-loss.
- **D-0.3** — coder model is **Claude Haiku 4.5**: executes precise specs well, improvises poorly.
  → All phase specs must be detailed + checkpointed (this is why 1.3 is written the way it is).

---

## CORRECTION TO v1.1

- **D-6 attribution fix:** "Origin Pilot" in `Architectural_Analysis.txt` conflates a real Chinese
  **quantum OS** (Origin Quantum, open-sourced Feb 2026) with the AI-agent-governance narrative. The
  agent-supervisor "Origin Pilot/HQ" is **not** verifiable as a real product. **Never cite "Origin
  Pilot" as prior art for Lagado's supervisor** (it's a quantum OS — inaccurate, credibility-damaging
  to auditors/investors). Cite the real lineage: supervisory control, capability-based security,
  reachability analysis, HITL. The quantum OS is still valid *inspiration* for the Lagado-OS north
  star at the pattern level (adaptive orchestration of a heterogeneous substrate), which is where
  Principle C and Subsystem B come from.

---

## DECISION LEDGER (cumulative)

| ID | Decision | Status |
|---|---|---|
| OD-1 | Sovereign personal assistant (not security-research agent) | ✅ LOCKED (v1.1) |
| A | Proprietary / closed source; verify via audit+reproducible builds | ✅ LOCKED |
| L-1 | LFM license clears proprietary commercial use to $10M ARR | ✅ CLEARED |
| B | Adaptive resource governor = core subsystem; AI-anywhere | ✅ LOCKED |
| C | Fidelity is the primary resource | ✅ LOCKED |
| D-0 | Commit + back up the 35 changes before any work | ⏳ TODO (Step 0) |
| OD-2 | Rename timing: atomic before Phase 1.3 | ⏳ recommended, pending your go |
| OD-3 | Whonix: in-house vs ship-and-comply | ⏳ open |
| OD-6/7/8 | arise.sh / entropy_gate.py / UI component set | ⏳ open (low stakes) |

*— End addendum v1.2.*
