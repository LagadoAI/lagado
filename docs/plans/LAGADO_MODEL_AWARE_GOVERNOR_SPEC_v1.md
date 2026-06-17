# LAGADO — MODEL-AWARE GOVERNOR & THE NO-HARDCODE INVARIANT (v1)

**Date:** 2026-06-17 · **Status:** Active, foundational. **Trigger:** a systemic issue — the
codebase hardcodes model/hardware assumptions instead of discovering or deferring them. This
violates the project's own H-1 (the model is swappable). **LOCAL must work perfectly before
hybrid/cloud** (both stubbed for now). This is a *complete-job, no-loose-ends* effort with a
real user-visible UI, not a backend stub.

---

## 0. THE INVARIANT (new — enforced going forward)

> **No model- or hardware-specific value may be hardcoded. DISCOVER it (GGUF metadata /
> hardware probe) or DEFER it (governor/user setting) — always with a DISCOVERED default.
> The model is swappable (H-1); assuming its context / layers / size is a latent bug that
> detonates on the first swap.**

Every value falls in one bucket:
- **DISCOVER** — read from the model (GGUF) or probe the machine. (e.g. real context window, layer count, free VRAM, cores, RAM)
- **DERIVE** — the governor computes it from `model × hardware`. (e.g. `ctx`, `n_gpu_layers`, KV-cache size)
- **DEFER** — a user/governor setting, seeded with a *discovered* default. (behavioral params)
- **LEGIT** — a principled constant unrelated to model/hardware (ports, the 30-day Ebbinghaus curve, the chars/token ×4 estimate). The only kind allowed to be a literal.

The failure mode to kill: reaching for a plausible-looking constant when hitting an unknown, instead of reading the real value or handing the choice up.

---

## 1. THE AUDIT (every assumed site)

**① DISCOVER FROM THE MODEL (GGUF metadata — the missing foundation):**
- `config.rs:43` `CONTEXT_SIZE = 32768` labeled "model context window" → the model's real `context_length` (**128k** for LFM2-8B-A1B; `n_ctx_train=128000` was printed in the server log and ignored). The headline lie.
- `governor.rs compute_offload → (99, _)` — `-ngl 99` is a "more than any model has" hack → real `block_count` → *actual* partial offload.
- `moe_experts_on_cpu` hardwired `false` → `expert_count` from metadata (LFM2-8B-**A1B** IS MoE — `--cpu-moe` never fires).
- KV-cache sizing does not exist; needs `block_count, head_count, head_count_kv, embedding_length`.
- `config.rs` `CLASSIFIER_CONTEXT_SIZE = 2048`, `VLM_CONTEXT_SIZE = 2048` → each model's real max.

**② DISCOVER FROM HARDWARE (probe — partly done, extend):**
- VRAM ✓, cores ✓ (`governor.rs` already discovers these — keep). Add: total/free RAM, which GPU drives the display (iGPU frees the dGPU), bandwidth.

**③ DERIVE (the governor's real job — `model × hardware` → config):**
- `ctx` = `min(model.context_length, what free VRAM affords via KV-cache math, user setting)` — not a flat 32768.
- `n_gpu_layers` = computed from `block_count` + per-layer bytes + free VRAM (real partial offload, not 99/0).
- `n_parallel` (`4`/`2`), `flash_attn`, and **`bootstrap.rs:52` `-ngl 0 -t 2` classifier launch** (hardcoded "CPU-only" — only this machine needs that) → all governor-derived.
- `cpu_config` `8192`/`12`/`16384` literals → derived from model max ∩ RAM.

**④ DEFER (user/governor setting, DISCOVERED default):**
- `board.rs` half-life (`86400`), importance weights (`0.6/0.4/0.3`, `0.20` caps), `ParkWeights 1/1/1`, `memory_tiers MAX_WARM_ENTRIES` (10k cap should scale with hardware), `sleep_gate` 5-min interval.

**⑤ LEGITIMATE (principled, leave as constants):**
- ports/hosts (env-overridable), the 30-day Ebbinghaus entropy half-life (doctrine), chars/token ×4 estimate, classify `max_tokens=10`.

---

## 2. THE MODEL-READER (foundation — everything hangs on it)

```
pub struct ModelInfo {
    arch: String,            // general.architecture
    context_length: u32,     // {arch}.context_length      — replaces CONTEXT_SIZE guess
    block_count: u32,        // {arch}.block_count          — replaces -ngl 99 hack
    embedding_length: u32,   // {arch}.embedding_length
    head_count: u32,         // {arch}.attention.head_count
    head_count_kv: u32,      // {arch}.attention.head_count_kv (GQA → KV-cache sizing)
    expert_count: u32,       // {arch}.expert_count         — >1 ⇒ MoE ⇒ --cpu-moe real
    param_count: Option<u64>,// general.parameter_count
    file_bytes: u64,         // weights footprint
}
read_gguf(path: &Path) -> Result<ModelInfo, String>
```
- **Parse the GGUF metadata KV header ONLY** — no tensor load, cheap, runs **pre-launch** (the governor must know the model before it can set `-c`/`-ngl`).
- **From scratch, zero deps** — sovereignty / supply-chain: GGUF header is a simple documented format (magic `GGUF` u32, version u32, tensor_count u64, kv_count u64, then typed KV pairs). No crate.
- Tested against the real on-disk GGUFs (8B, 1.2B, ColBERT, VL) — assert context_length=128000 etc.

---

## 3. THE GOVERNOR DERIVATION (replace assumptions with math)

- `kv_bytes_per_token = 2 (K+V) × block_count × head_count_kv × (embedding_length / head_count) × 2 (f16)`
- partial offload: `n_gpu_layers` = layers fitting in `free_vram − weights_on_gpu − kv_for_ctx − overhead`
- `ctx = min(model.context_length, vram_afforded_ctx, user_setting)`
- `expert_count > 1` → offer `--cpu-moe` when VRAM is tight
- classifier/VLM placement (ngl/threads) → governor decides, `bootstrap` consumes (stop hardcoding `-ngl 0`)
- `detect_and_plan` takes `&ModelInfo` (not a `default_ctx` literal + file-size proxy)

This is the first-principles cost model from the deferred intelligent-governor design, now grounded in REAL metadata. Calibrate predictions against real runs (first datapoint: 8B-A1B Q4 → 5074 MiB / 188 tok/s).

---

## 4. THE UI FEATURE (real, user-visible, testable — NOT a stub)

An **Engine / Performance** settings page so the user SEES the discovery + derivation and can tweak:
- **Discovered model** — arch, real context window (128k), layers, params, MoE yes/no. Proof the system KNOWS the model.
- **Probed hardware** — GPU, VRAM free/total, cores, RAM.
- **Derived config** — ctx, n_gpu_layers (e.g. "28/32 layers on GPU"), offload %, parallelism — *with the reasoning shown*.
- **Adjustable DEFER items** — context-size slider capped at the model's real max; memory cap — each with governor feasibility (green/amber/red, generated from the cost model) + one-click "Reset to recommended."
- Tauri commands: `get_model_info`, `get_hardware`, `get_derived_config`, `set_engine_overrides`. React page under settings.

---

## 5. BUILD SEQUENCE (each step: build + test + commit; no step leaves a hardcoded model/hw value)

1. **model-reader** — `read_gguf` + `ModelInfo` + tests on the real GGUFs.
2. **governor derivation** — consume `ModelInfo`; real offload/ctx/KV math + unit tests.
3. **fix consumers** — `config::CONTEXT_SIZE` → derived; hydra capability → derived (or removed); `bootstrap` classifier launch → governor-decided.
4. **UI feature** — Tauri commands + the Engine settings page (the thing the user tests).
5. **DEFER items** → settings, governor-defaulted.

---

## 6. DOCS / MEMORY UPDATED (so the mistake can't recur)

- CLAUDE.md: new **Invariant #9 — no hardcoded model/hardware values** (discover or defer).
- This spec.
- Memory: `lagado-no-hardcode-invariant` (feedback). Governor reclassified from "deferred" to ACTIVE (local-must-work-perfect requires it); hybrid/cloud remain stubbed.

---

## 7. WHERE THIS SITS vs THE HARNESS BUILD

Pauses ④b (single-turn loop wiring). Done so far on the harness: ① grammar router, ② G3/ColBERT, ③a Board scorer+wiring, ④a supervisor (governor-injected ladder). Servers up: 8080/8081/8082. ~181 lib tests. After the model-aware governor + UI land, resume ④b.
