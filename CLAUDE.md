# CLAUDE.md

Guidance for working in this repo. Read before making changes.

## What this is

**Lagado AI** — a local-first, privacy-first desktop agent. Four pillars (from PDF master plan):
1. **P1 — Maximum Security & Sovereignty** — local-only, encrypted, no telemetry. The headline moat.
2. **P2 — Dual-Brain Hydra** — fast LFM2.5 + optional 8B heavy. Governor decides tier. "AI anywhere."
3. **P3 — Fully Integrated Stack** — one coherent system, not loose modules
4. **P4 — Persistent Learning** — action graph + skill library, survives reboots

Production targets: **Windows-first, macOS, Linux**. Development on Linux.
GitHub Actions CI (linux/macos/windows) is the cross-platform test bench.
**Single source of truth:** `LAPUTA HOW TO/LAGADO_MASTER_PLAN.md.pdf` (June 3 2026 — supersedes all other plan files).
Companion detail: `docs/plans/FILE_DEPENDENCY_REFERENCE_v3.md`.

## Architecture

### Runtime
Single Tauri binary (`lagado-ui/src-tauri/`) wraps:
- React/shadcn UI — Liquid AI inspired, deep navy + blue/purple glassmorphism
- Rust agent core (`lagado-agent/` as a library)
- Vendored `llama-server` subprocess (HTTP inference on :8080, NOT FFI) — main 8B model
- Classifier subprocess on :8081 (LFM2.5-1.2B-Instruct, intent classification, CPU-only)
- Visual encoder: in-process `libmtmd.so` FFI (LFM2-VL-450M + mmproj, vision → embedding vectors, no subprocess)
- QEMU desktop VM (agent's sandboxed working surface)

Inference: HTTP to `llama-server` → `/v1/chat/completions`.
Models in `~/.laputa-secure/models/`:
- `LFM2-8B-A1B-Q4_K_M.gguf` — main agent model (gen2, temp 0.3/min_p 0.15)
- `LFM2.5-1.2B-Instruct-Q4_K_M.gguf` — intent classifier (gen2.5, temp 0.1/top_k 50)
- `LFM2-VL-450M-F16.gguf` + `mmproj-LFM2-VL-450M-F16.gguf` — vision encoder
- `LFM2-ColBERT-350M-Q4_K_M.gguf` — Board embeddings (Phase 1)

### Agent pipeline (Hydra orchestrator)

```
User message
    ↓
isPaused? → YES → send_chat() → chat inference (no tools, no screen)
    ↓ NO
action_graph shortcut (score ≥ 0.65)? → YES → agent_loop directly
    ↓ NO
hydra::classify_intent() [CLEAN PROMPT — zero history, current message only]
    → 1.2B classifier on :8081 (few-shot prompt, ~80% accuracy)
    → fallback to 8B if classifier server down
    ↓
CHAT → chat_response() with RAG context
INTERACTIVE/REASONING → agent_loop() with HITL gate + RecoveryManager
```

**CLEAN-CONTEXT DISCIPLINE is non-negotiable.** `classify_intent()` receives ONLY the current user message.

**State hash:** `blake3(perceptor.read_screen())` — used for action_graph lookups and recovery keys.

### VM Architecture (Phase 1.4 — COMPLETE)

```
QemuDesktopBackend boots qcow2 with QMP socket + VirtIO display
         ↓
Perception: SSH into guest → perceive.py (AT-SPI2 on Xorg) → PerceptionCache (ref_id → cx,cy)
Actuator:   SSH into guest → xdotool mousemove cx cy click 1 (coords resolved from cache)
Live feed:  QMP screendump (format:png) → /dev/shm/lagado_frame.png → base64 → Immersive canvas
```

**Dev image:** `~/.laputa-secure/vm-images/Arch-Linux-x86_64-cloudimg.qcow2`
**Seed ISO:** `~/.laputa-secure/vm-images/seed.iso` (cloud-init, first-boot only)
**Guest:** user `laputa:laputa`, auto-login, XFCE4, xdotool + AT-SPI2 pre-installed
**QMP screendump** — `format:png` required (default is PPM).
**SSH readiness:** `vm_ssh_port` set asynchronously via background SSH auth probe (`ssh whoami` → exit 0 + stdout contains "laputa").
**Frame path:** `/dev/shm/lagado_frame.png` — constant `config::FRAME_PATH`.
**Auto-kill:** `Drop for VmHandle` + `KillOnDrop` wrapper kills all child processes on app exit.

### Auth (Phase 2 — COMPLETE)

**Wrapped DEK scheme** — FileVault/1Password pattern:
- Signup: random 32-byte DEK wrapped with `Argon2id(password)` + `Argon2id(recovery_phrase)`
- Both blobs in `~/.laputa-secure/config/keychain.json` — raw DEK never touches disk
- Login: Argon2id(password) → unwrap DEK → `auth::set_session_dek(dek)`
- Lockout: 3 failures → 10-min cooldown, persisted, fail-closed if tampered
- `auth::active_key()` is the only crypto entry point; falls back to `machine_passphrase()` in dev

### Memory system (COMPLETE — all phases)

**Full living memory triangle operational:**
- Hot entries → sleep_gate → LLM batch summarize → warm entries → entropy prune at 10,000
- Entropy equation: `V = T × e^(−λt) × (1 + ln(n+1))`, λ = ln(2)/30days (Ebbinghaus + log reinforcement)
- Cold (vault) never entropy-pruned; 365-day natural half-life protects it
- Skill distillation: Done/Task episodes → `distill_skill_async()` → LLM extract → `skill_library.save()`
- Visual embeddings: frame encoded at episode boundaries → cosine similarity retrieval
- `SleepGate::new(memory, adapter)` — full consolidation cycle every 5 min
- DB: `~/.laputa-secure/memory.db`, `~/.laputa-secure/skill_library.db`

**Phase 3.3 COMPLETE — Visual embedding via in-process libmtmd FFI:**

```
Frame (PNG) → vision/shim.c (lagado_encode_image) → mean-pooled n_embd vector
                                                            ↓
                                               MemoryTiers embedding BLOB column
                                                            ↓
                                           cosine similarity retrieval at query time
                                                            ↓
                                      top-K visually similar past episodes → agent context
```

**Key implementation facts:**
- C shim at `lagado-agent/src/vision/shim.c` — `lagado_encode_image()` mean-pool (unchanged) + `lagado_encode_image_patches()` per-tile/per-patch with `lfm2_find_grid()` runtime grid derivation
- Rust binding at `lagado-agent/src/vision/mod.rs` — `VisualEncoder` behind `Mutex`; `encode_png_patches()` returns `Vec<TilePatches>`; `is_overview` set by structural chunk index (NOT token count)
- `build.rs` compiles shim via `cc` crate, links `libllama.so`/`libmtmd.so`/`libggml.so`
- Image decoded to RGB (NOT RGBA) before passing to `mtmd_bitmap_init`
- `VisualEncoder` fires at episode boundaries only (Done/Task/Abort), not per tick
- VLM subprocess (port 8082) and `VlmPerceptor` text path retired
- `MemoryTiers`: `embedding BLOB` column + `store_visual_embedding()` + `find_similar_by_embedding()`
- Platform gate via `lagado_vision_ffi` cfg (set by build.rs ONLY when `vendored/llama.cpp-2/include/llama.h` exists)
- CI skips shim compilation gracefully when vendored headers absent (gitignored); no linker errors
- `encode_and_store_async()` fires in background tokio::spawn, encode in spawn_blocking outside lock
- Visual retrieval wired into agent_loop: encodes current frame once per invocation → top-3 similar episodes → prompt
- `[[bin]] test=false` in Cargo.toml (static lib linking doesn't propagate to bin test targets)
- `cargo test -p lagado-agent` requires `LD_LIBRARY_PATH=.../vendored/llama.cpp-2/build/bin` (stale rpath in vendored libllama.so)
- 110 lib tests pass (HEAD 938af52)

### Key modules (`lagado-agent/src/`)

| Module | Status | What it does |
|---|---|---|
| `hydra.rs` | ✓ | Dual-model orchestrator, few-shot classifier on :8081, blake3 state hash |
| `agent.rs` | ✓ | Agent loop, episodic memory context, HITL gate, RecoveryManager |
| `recovery.rs` | ✓ wired | 7 failure-mode dispatcher, graph-backed + LLM recovery |
| `memory_tiers.rs` | ✓ | Hot/warm/cold tiers, entropy equation, drain_cool_hot, promote_warm_summary, entropy_prune_warm |
| `sleep_gate.rs` | ✓ | Full consolidation: decay → batch summarize → warm promote → entropy prune. Takes adapter. |
| `server_guard.rs` | ✓ | Health monitor — polls /health every 10s, auto-restarts crashed llama/classifier servers, emits tauri events |
| `chronos.rs` | ✓ | SQLite timeline, T=0 anchor |
| `retrieval.rs` | ✓ | RAG K=15, Jaccard scoring |
| `action_graph.rs` | ✓ | SQLite workflow store, shortcut path |
| `skill_library.rs` | ✓ | Experiential depth layer — read + distillation write path wired |
| `security/crypto.rs` | ✓ | AES-256-GCM, Argon2id, DEK wrapping |
| `auth/mod.rs` | ✓ | Wrapped DEK, lockout, `active_key()`, `set_session_dek()` |
| `self_model.rs` | ✓ | Accepted beliefs, distill feed |
| `distill.rs` | ✓ hooks | Replay manifest for Phase 3 QLoRA |
| `perception/mod.rs` | ✓ | PerceptionCache (coords + bboxes), VlmPerceptor retired |
| `perception/linux.rs` | ✓ | AT-SPI2 via perceive.py --focused, populates both coords and bboxes |
| `perception/delta.rs` | ✓ | Pixel-space blake3 per cell — decoded RGB, remainder → last col/row |
| `perception/frame.rs` | ✓ | FrameProcessor: PNG→RGB→DeltaDetector, stateful, reset() between sessions |
| `perception/cv_proposer.rs` | ✓ | Canny + 8-connected components (imageproc), ProposalResult, extract_cell_rgb |
| `perception/vlm_adapter.rs` | kept | Text path kept for reference; not used in agent pipeline |
| `perception/arbiter.rs` | NOT BUILT | IoU-dedup arbiter (TASK 6) |
| `perception/harness.rs` | NOT BUILT | PerceptionMode switch + CSV measurement log (TASK 7) |
| `vision/mod.rs` | ✓ | encode_png() mean-pool + encode_png_patches() per-tile; PatchEmbedding/TilePatches; is_overview by structure |
| `vision/shim.c` | ✓ | lagado_encode_image() mean-pool (unchanged) + lagado_encode_image_patches() per-tile; lfm2_find_grid() |
| `perception/capture.rs` | stub | grim/scrot host mode |
| `vm/mod.rs` | ✓ | QemuDesktopBackend, QMP, DynamicActuator/Perceptor |
| `vm/qmp.rs` | ✓ | QMP socket client — screendump(format:png) |
| `vm/ssh_actuator.rs` | ✓ | SSH→xdotool, PerceptionCache coord resolution |
| `vm/ssh_perceptor.rs` | ✓ | SSH→perceive.py, populates PerceptionCache |
| `governor.rs` | ✓ | NVIDIA+AMD GPU detection, VRAM-aware n_gpu_layers, GpuInfo/GpuVendor, moe_experts_on_cpu wired to --cpu-moe |
| `security/sandbox.rs` | ✓ | cgroup v2 memory+pid limits (Linux), QEMU -sandbox flag, cleanup_stale |
| `config.rs` | ✓ | Paths, FRAME_PATH, server config; llama/classifier_memory_max_bytes() from model file size × 1.5 (model-agnostic) |
| `gate.rs` | ✓ | Read/Write/Destructive tiers; ConfirmTyped for destructive text |
| `mcp/client.rs` | ✓ | MCP stdio client — discover_tools() one-shot + parse_tools_list() pure fn |
| `tools/mod.rs` | ✓ | ToolRegistry, TrustLevel, ToolBackend, 44 builtin entries, discover_mcp_tools() async |
| `tools/executor.rs` | ✓ | Native Rust executor for 42 tools; SOCKS5-aware HTTP; DDG/SearXNG web search |
| `operator.rs` | ✓ | StepEnforcer, ToolDescriptor, RiskLevel |

### UI (`lagado-ui/src/`)

**Working:** `/` chat, `/awakening`, `/immersive` (live VM feed + VM/Host toggle + draggable),
`/vm` (boot/stop/status), `/settings` (11-tab panel), `/server`, `/terminal` (auth-gated), `/vault`, `/design`.

**Auth flow:** loading → awakening (onComplete → auth_check) → signup (3-step) or login → app.
- `Awakening` now takes `onComplete` prop — App.tsx wires it to re-invoke `auth_check` and set state.
  Previously `navigate('/')` was a no-op since `<Routes>` wasn't mounted in the awakening branch.
- `LoginPage` has `onSignup` prop → "First time? Create account" link as safety net.
- `SignupPage` adds username step (stored in `localStorage.lagado_username`) before password.

**Design system:** Full token layer in `index.css` (colors, effects, spacing, radius, typography).
CSS vars (`--bg`, `--surface`, `--grad-brand-h`, etc.) + component classes (`.lg-btn`, `.lg-card`,
`.lg-glasspanel`, `.lg-bubble--user/agent`, `.lg-pill--connected/connecting/disconnected`,
`.lg-tabs`, `.lg-tab`, `.nav-item`). Mixes with Tailwind — no conflicts (different namespaces).

**Shared layout component:** `lagado-ui/src/components/AppSidebar.tsx`
- Mark + "LAGADO" wordmark, gradient "+ New conversation" button, search field
- SURFACES nav: Chat / Immersive / Vault / Terminal / Settings with Lucide icons + active highlight
- User footer: initials avatar, username (from localStorage), "Sovereign", StatusPill
- Used in `ChatDefault` and `SettingsMain`; safe to use in any page inside `<ChatProvider>`

**Network/Privacy settings:** `SettingsNetwork.tsx` + Tauri commands `get_network_settings` /
`save_network_settings` / `test_network_connection`. Persists to `~/.laputa-secure/config/network.json`.
Proxy off by default; Tor/Whonix presets; bridge field. `executor.rs::http_client()` reads settings
file first, `LAGADO_HTTP_PROXY` env var overrides.

**Color system:** deep navy bg (`#080c14`), blue (`#3b82f6`) + purple (`#8b5cf6`) accents.
Agent avatar = `lagado-mark.png` (maze-tesseract logo). Thinking state = `HyperLoader` SVG.

## Key invariants — DO NOT BREAK

1. **Mutex guard discipline**: guards MUST be dropped before any `.await`.
2. **Clean-context routing**: `classify_intent()` MUST receive only the current user message.
3. **HITL chokepoint**: all agent actions go through `gate::evaluate_action()`. Never bypass.
4. **No wildcard `_` arms** on enums you define.
5. **No `std::process::exit(1)`** from library code.
6. **No AI attribution** in commits, code, PRs, or any artifact. Author: `Lagado Labs <lagadolabs@gmail.com>`.
7. **DEK discipline**: never persist raw DEK. `active_key()` is the only crypto entry point.
8. **SSH readiness**: never set `vm_ssh_port` before SSH auth probe (`ssh -o BatchMode=yes ... whoami`) returns exit 0 and stdout contains "laputa". Bare TCP connect is insufficient.
9. **No hardcoded model/hardware values** (2026-06-17): never hardcode a model- or hardware-specific value (context window, layer count, n_gpu_layers, ctx size, model size, parallelism, CPU/GPU placement). **DISCOVER** it (GGUF metadata via the model-reader / hardware probe) or **DEFER** it (governor/user setting), always with a *discovered* default. The model is swappable (H-1) — assuming its context/layers/size is a latent bug. The only literals allowed are principled constants unrelated to model/hardware (ports, the 30-day Ebbinghaus curve). See `docs/plans/LAGADO_MODEL_AWARE_GOVERNOR_SPEC_v1.md`.

## Build / run

```bash
# From anywhere:
Arise

# Or manually from lagado-ui/:
WEBKIT_DISABLE_DMABUF_RENDERER=1 \
LAGADO_DATA_DIR=/home/d/.laputa-secure \
LAGADO_LLAMA_SERVER=/home/d/laputa/lagado-agent/vendored/llama.cpp-2/build/bin/llama-server \
LD_LIBRARY_PATH=/home/d/laputa/lagado-agent/vendored/llama.cpp-2/build/bin \
npm run tauri dev

# Checks
cargo check --workspace && cargo test -p lagado-agent
cd lagado-ui && npx tsc --noEmit
```

## Delegation workflow

**2026-06-16: Opus does ALL work — planning AND implementation. No Haiku, no Sonnet delegation** (user directive, until they say otherwise). Use the `advisor` tool for the adversarial-review/skeptic pass before load-bearing designs and when declaring done.
Verify with `cargo check --workspace` + `npx tsc --noEmit` after changes.

**Docs policy (2026-06-16 reversal):** `docs/`, `LAPUTA HOW TO/`, and all plans/PDFs are now COMMITTED (was: local-only). Machine = single point of failure; repo is private forever, only the binary ships → disaster-recovery beats secrecy. Never make the repo public.

## Status (2026-06-11)

**FULL LIVING MEMORY SYSTEM COMPLETE. Perception fusion harness TASK 1–6 complete (TASK 7 next). VM control channel end-to-end tested + provisioning fixed.**

HEAD: `0c8c99e` (+ uncommitted harness_proof bin/docs). 156 lib tests. Ubuntu ✓ macOS ✓ Windows ✓.

**Single source of truth for the plan:** `LAPUTA HOW TO/LAGADO_MASTER_PLAN.md.pdf` (June 3, 2026).

### UI ↔ Backend wire (verified)
`useTauriAgent.ts` → direct `invoke()` calls → Tauri commands. Events back via `app.emit()` / `listen()`.
`server.rs` WebSocket (port 9090) and `useAgentSocket.ts` are **orphaned** — dev scaffold for UI design iteration only.

### Full memory system (complete)
- **Hot → warm**: sleep_gate batches cooled hot entries → LLM summarizes → warm SQLite
- **Entropy pruning**: `V = T × e^(−λt) × (1 + ln(n+1))`; warm pruned at 10,000; cold never touched
- **Skill distillation**: Done/Task episodes → `distill_skill_async()` → LLM → `skill_library.save()`
- **Visual similarity**: frame encoded at episode boundaries → cosine similarity retrieval
- **action_graph**: muscle memory (exact hash bypass at score ≥ 0.65)
- **skill_library**: advisory depth context, turns 1-3 only

### What works end-to-end
- App launches → Awakening → auth → chat
- `send_goal` → hydra → 1.2B classifier → agent_loop (episodic + visual + skill context turns 1-3, tools every turn)
- At Done/Task: skill distilled + visual embedding stored
- Every 5 min: sleep_gate consolidates hot → warm → entropy prunes if over limit
- Immersive → VM auto-boots → live QEMU desktop feed → SSH → xdotool actuation
- RecoveryManager, SleepGate (full consolidation), ServerGuard, cgroup v2 sandbox all active
- 44 bundled native Rust tools, MCP stdio client, confidence gating, HITL gate

### VM control channel — end-to-end tested 2026-06-11
Smoke-test bin `lagado-agent/src/bin/harness_proof.rs` drives the real modules:
`QemuDesktopBackend::boot → poll SSH → SshPerceptor::read_screen → QmpClient screendump →
SshActuator → FrameProcessor delta → backend.shutdown`. Run with
`LD_LIBRARY_PATH=…/vendored/llama.cpp-2/build/bin LAGADO_DATA_DIR=~/.laputa-secure ./target/debug/harness_proof`.

**Result: agent code is sound; the VM was un-provisioned for the agent's SSH control model.** The
agent's ONLY VM control channel is SSH (`ssh -o BatchMode=yes`, key auth). It was fully broken by the
guest image, now fixed in `~/.laputa-secure/vm-images/` (cloud-init.yml + rebuilt seed.iso, originals
backed up). Fixes applied: DHCP eth0, `ufw allow ssh`, install host pubkey, generated host keypair
`~/.ssh/id_ed25519`. Now: boot → SSH (~14–24s) → AT-SPI2 tree read → QMP screendump → clean shutdown all work.
**Raw actuation proven**: `xdotool mousemove/click/type` over SSH changed the screen (1076-px diff).

**Open gaps (see memory `vm-harness-control-channel`):**
- `tine` (pip tine-cli) rejects `tree --json` and its `tree` text format doesn't match perceive.py's
  `parse_text_tree` → zero elements → no `ref_id→(x,y,w,h)` → **click-by-selector blocked** (raw coord click works).
- `SshPerceptor` calls `perceive.py` without `--focused` (so emits JSON parse_ref_coords can't read).
- `QemuDesktopBackend::boot()` has no kill-stale pre-flight → orphaned VMs block fresh boots.
- VM readiness gates on bare TCP poll, not real SSH-auth success → false "ready" while sshd unreachable.

### Perception fusion harness (TASK 6 code complete, committed)
- TASK 6 ✓ — `perception/arbiter.rs` IoU-dedup fusion (commit 0c8c99e): `iou()`, `fuse(a11y,cv,patches)`,
  `Sense{A11yOnly,VisionOnly,Both}`, `FusedElement`. MATCH_THRESHOLD=0.30 (loose), ±1 patch inflate fuzz,
  overview-skip, mean-pool overlapping patch embeddings, deterministic (y,x,w,h) sort. 156 lib tests.
  Still to close: CV real-screenshot noise gate (cv_measure on content-dense frame). TASK 7 next.
- TASK 1 ✓ — full bbox retained (`parse_ref_bboxes`, `PerceptionCache.bboxes`)
- TASK 2 ✓ — pixel-space DeltaDetector (decoded RGB, remainder → last col/row) + FrameProcessor
- TASK 3 ✓ — CV box proposer (Canny + connected components, imageproc 0.27) + cv_measure binary
- TASK 4 ✓ — decoder_pos flat (LFM2 1D not 2D); 1280×800 → 3×2 grid + overview; ordering verified by marker test
- TASK 5 ✓ — `lagado_encode_image_patches()` + `encode_png_patches()`; `is_overview` by structural position (img_idx ≥ grid_cols×grid_rows); 1025×1025 probe proved token-count detection fails
- TASK 6 ⬅ NEXT — `perception/arbiter.rs`: IoU-dedup → `FusedElement` / `Sense` enum (threshold <0.5, ±1 patch fuzz)
- TASK 7 — `perception/harness.rs`: `PerceptionMode`, CSV measurement log, conditional wire

### Remaining (against 7-segment PDF plan)
- **Segment 1** — Browser extension Backend #1: DOM perception + actuation (cross-platform ON-RAMP)
- **Segment 5** — Egress proof + `security/profile.rs` (Strict/Balanced/Open tiered profile)
- **Segment 6** — Immersive watch-and-direct loop (partial)
- **Segment 7** — Native desktop perception for Mac/Win (stubs exist)
- Settings tool manager: get_tools, set_tool_trust, toggle_tool_enabled
- GGUF MoE parser (auto-set moe_experts_on_cpu)
- grammar.rs GBNF constraint (accuracy lever, currently stub)
- security/audit.rs (tamper-evident append-only log)

## Harness doctrine (2026-06-14) — direction for the rebuild

Full plan: `docs/plans/LAGADO_HARNESS_DOCTRINE_AND_PLAN_v1.md`. **Execution spec (current build):** `docs/plans/LAGADO_HARNESS_BUILD_SPEC_v1.md`. Verified LFM facts: `/home/alucard/projects/research/LFM research.txt`.

**Build progress (2026-06-16, Fedora 44 rebuild):** CUDA llama.cpp built; models on disk; GPU inference proven (8B-A1B Q4 full-offload, 188 tok/s). **The spine:** deterministic floor that always works + model upgrade when affordable, governor-arbitrated, off the hot path (recurs in router/importance-gate/conduction/perception/model-modes). Work order: **① DONE** grammar-constrained router + 8B fallback (closed the silent UNPARSED→CHAT hole; `generate_constrained` on the adapter). **② DONE** G3 baseline — Jaccard F1=0.43/R=0.75 → ColBERT-350M mean-pool cosine F1=0.52/R=0.92 (the Board relevance path; MaxSim deferred). **③ NEXT** the Board (Park scorer as a NEW fn ≠ `information_value`; relevance MUST be rank/min-max normalized before the additive sum — pooled cosines are compressed [0.96,0.99]; G3 tunes β only, α/γ by principle; Rust↔Python parity test; sequenced ③a floor / ③b G4 trust / ③c G2 model-refinement). **④** single-turn loop + `supervisor.rs` escalation ladder. 166 lib tests.

**The harness is the moat; the model is swappable** (`InferenceAdapter`). LFM2 is NOT a continuous-reflex ODE net (that's the LTC/CfC/NCP drone line) — it's a discrete edge-CPU transformer-hybrid. Use it for edge efficiency + shippable license + agentic variants + cheap fine-tune, not for "liquid" magic. The word "Liquid" must not load-bear in an architecture decision.

**Core problem (verified):** small models degrade over multi-turn history (~0.63⁵≈10%/5 turns; premature commitment; no recovery; temperature doesn't help). So:
- **Externalize state; every model step is single-turn-fresh.** Re-present a clean, fully-specified slice each step. The slice-assembler is **deterministic code, not a model call.** Mitigate the re-encode cost with llama-server `/slots` KV-prefix reuse (seam exists in `inference/mod.rs`, stubbed).
- **The "board" = a standard scored memory store (Park / Generative Agents), NOT a physics engine.** `score = α·recency + β·relevance + γ·importance`, recomputed stateless per step, top-k. **`memory_tiers.rs` already implements recency (`information_value`) + relevance (`find_similar_by_embedding`) + top-k** — extend it (add importance + one scorer + wire as slice-assembler), don't rebuild. Hot tier in `/dev/shm` (zero-copy, already the frame path). **Conduction (ACT-R spreading activation) OFF by default** — add only if a retrieval eval proves it earns the complexity.
- **Retrieval ≠ planning.** Board surfaces candidate ingredients; a separate, named, **deterministic sequencer** does ordering/dependencies.
- **`supervisor.rs` = reset-from-corrected-board + bounded-retry escalation ladder** (N retries → 8B → optional cloud → HITL). Not "think harder."
- **Born flightworthy via LEARNED pipes** (record traces → promote to action-graph), not 25–30k hand-authored entries (they rot like perceive.py's DOM assumptions). Seed thin (~50) if at all.

**Four gaps that are hard requirements:** G1 eviction/archival tier (cool-don't-delete needs a disk tier, not infinite RAM); G2 write-quality/importance gate; G3 retrieval eval set (build BEFORE tuning α/β/γ); **G4 particle trust tier** (perceived DOM/screen text → board → model context is a prompt-injection vector; tag `perceived-untrusted` vs `user-intent-trusted` — the perception-side analog of the HITL gate; critical for the browser surface).

**Steal, don't invent:** ACT-R (1983), Park Generative Agents (2023), Hearsay-II blackboard (1980). Convergence = the shape is right, not that we're first. Invention budget goes ONLY to the LFM2 edge-CPU single-turn-reset harness. **Build the boring stateless version; let the eval decide if anything fancier earns its keep.**

Open decisions (need user): QEMU vs libkrun (research gate); board embedding source; extend memory_tiers vs new organ; G2 deterministic-vs-model importance; sequencing.
