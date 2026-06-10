# CLAUDE.md

Guidance for working in this repo. Read before making changes.

## What this is

**Lagado AI** — a local-first, privacy-first desktop agent. Three pillars:
1. **Sovereign** — local-only, encrypted, no cloud, no telemetry
2. **Living** — thermodynamic memory hierarchy, sleep consolidation, patterns compound
3. **Self-aware in time** — chronos autobiographical spine, T=0 at first launch

Production targets: **Windows-first, macOS, Linux**. Development on Linux.
GitHub Actions CI (linux/macos/windows) is the cross-platform test bench.
Full design: `docs/plans/MASTER_PLAN_v4.md` and `FILE_DEPENDENCY_REFERENCE_v3.md`.

## Architecture

### Runtime
Single Tauri binary (`lagado-ui/src-tauri/`) wraps:
- React/shadcn UI — Liquid AI inspired, deep navy + blue/purple glassmorphism
- Rust agent core (`lagado-agent/` as a library)
- Vendored `llama-server` subprocess (HTTP inference on :8080, NOT FFI) — main 8B model
- Classifier subprocess on :8081 (LFM2.5-1.2B-Instruct, intent classification, CPU-only)
- Visual encoder: in-process `libmtmd.so` FFI (LFM2.5-VL-450M + mmproj, vision → embedding vectors, no subprocess)
- QEMU desktop VM (agent's sandboxed working surface)

Inference: HTTP to `llama-server` → `/v1/chat/completions`.
Models in `~/.laputa-secure/models/`:
- `LFM2.5-8B-A1B-Q4_K_M.gguf` — main agent model
- `LFM2.5-1.2B-Instruct-Q4_K_M.gguf` — intent classifier
- `LFM2.5-VL-450M-F16.gguf` + `mmproj-LFM2.5-VL-450m-F16.gguf` — vision encoder

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
**SSH readiness:** `vm_ssh_port` set asynchronously via background TCP poll.
**Frame path:** `/dev/shm/lagado_frame.png` — constant `config::FRAME_PATH`.
**Auto-kill:** `Drop for VmHandle` + `KillOnDrop` wrapper kills all child processes on app exit.

### Auth (Phase 2 — COMPLETE)

**Wrapped DEK scheme** — FileVault/1Password pattern:
- Signup: random 32-byte DEK wrapped with `Argon2id(password)` + `Argon2id(recovery_phrase)`
- Both blobs in `~/.laputa-secure/config/keychain.json` — raw DEK never touches disk
- Login: Argon2id(password) → unwrap DEK → `auth::set_session_dek(dek)`
- Lockout: 3 failures → 10-min cooldown, persisted, fail-closed if tampered
- `auth::active_key()` is the only crypto entry point; falls back to `machine_passphrase()` in dev

### Memory system (Phase 3.1 — COMPLETE)

**MemoryTiers** wired into agent loop + SleepGate running:
- `push_episode(text)` stores goal completions/aborts to cold tier (encrypted, temp=1.0)
- `assemble_context(budget)` feeds episodic context into agent prompt as "Past sessions:" section
- `decay_all()` decays hot/warm only — cold tier (vault) is never deleted by decay
- SleepGate runs in background every 5min via `tauri::async_runtime::spawn`
- DB: `~/.laputa-secure/memory.db`

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
- C shim at `lagado-agent/src/vision/shim.c` handles all struct-by-value C ABI
- Rust binding at `lagado-agent/src/vision/mod.rs` — `VisualEncoder` behind `Mutex`
- `build.rs` compiles shim via `cc` crate, links `libllama.so`/`libmtmd.so`/`libggml.so`
- Image decoded to RGB (NOT RGBA) before passing to `mtmd_bitmap_init`
- `VisualEncoder` fires at episode boundaries only (Done/Task/Abort), not per tick
- VLM subprocess (port 8082) and `VlmPerceptor` text path retired
- `MemoryTiers`: `embedding BLOB` column + `store_visual_embedding()` + `find_similar_by_embedding()`
- Platform gate ONLY inside `vision/mod.rs` — public API compiles everywhere, `load()` returns Err on non-Linux
- `encode_and_store_async()` fires in background tokio::spawn, encode in spawn_blocking outside lock
- Visual retrieval wired into agent_loop: encodes current frame once per invocation → top-3 similar episodes → prompt
- `[[bin]] test=false` in Cargo.toml (static lib linking doesn't propagate to bin test targets)
- `cargo test -p lagado-agent` requires `LD_LIBRARY_PATH=.../vendored/llama.cpp-2/build/bin` (stale rpath in vendored libllama.so)
- 57 lib tests pass; FFI smoke-tested via `load_returns_err_on_bad_path` (calls real `lagado_encoder_init`)

### Key modules (`lagado-agent/src/`)

| Module | Status | What it does |
|---|---|---|
| `hydra.rs` | ✓ | Dual-model orchestrator, few-shot classifier on :8081, blake3 state hash |
| `agent.rs` | ✓ | Agent loop, episodic memory context, HITL gate, RecoveryManager |
| `recovery.rs` | ✓ wired | 7 failure-mode dispatcher, graph-backed + LLM recovery |
| `memory_tiers.rs` | ✓ | Hot/warm/cold tiers, push_episode, assemble_context, decay protects cold |
| `sleep_gate.rs` | ✓ | Background decay every 5min — started in main.rs |
| `server_guard.rs` | ✓ | Health monitor — polls /health every 10s, auto-restarts crashed llama/classifier servers, emits tauri events |
| `chronos.rs` | ✓ | SQLite timeline, T=0 anchor |
| `retrieval.rs` | ✓ | RAG K=15, Jaccard scoring |
| `action_graph.rs` | ✓ | SQLite workflow store, shortcut path |
| `skill_library.rs` | ✓ | Voyager-style multi-step procedure store |
| `security/crypto.rs` | ✓ | AES-256-GCM, Argon2id, DEK wrapping |
| `auth/mod.rs` | ✓ | Wrapped DEK, lockout, `active_key()`, `set_session_dek()` |
| `self_model.rs` | ✓ | Accepted beliefs, distill feed |
| `distill.rs` | ✓ hooks | Replay manifest for Phase 3 QLoRA |
| `perception/mod.rs` | ✓ | PerceptionCache, VlmPerceptor retired |
| `perception/linux.rs` | ✓ | AT-SPI2 via perceive.py, xdotool |
| `perception/delta.rs` | ✓ | Blake3 per-cell change detection |
| `perception/vlm_adapter.rs` | kept | Text path kept for reference; not used in agent pipeline |
| `vision/mod.rs` | ✓ | VisualEncoder FFI wrapper, cosine_similarity, Linux-only |
| `vision/shim.c` | ✓ | C shim over libmtmd — lagado_encoder_init/encode_image/free |
| `perception/capture.rs` | stub | grim/scrot host mode |
| `vm/mod.rs` | ✓ | QemuDesktopBackend, QMP, DynamicActuator/Perceptor |
| `vm/qmp.rs` | ✓ | QMP socket client — screendump(format:png) |
| `vm/ssh_actuator.rs` | ✓ | SSH→xdotool, PerceptionCache coord resolution |
| `vm/ssh_perceptor.rs` | ✓ | SSH→perceive.py, populates PerceptionCache |
| `governor.rs` | ✓ | NVIDIA+AMD GPU detection, VRAM-aware n_gpu_layers, GpuInfo/GpuVendor, moe_experts_on_cpu wired to --cpu-moe |
| `security/sandbox.rs` | ✓ | cgroup v2 memory+pid limits (Linux), QEMU -sandbox flag, cleanup_stale |
| `config.rs` | ✓ | Paths, FRAME_PATH, server config; llama/classifier_memory_max_bytes() from model file size × 1.5 (model-agnostic) |
| `gate.rs` | ✓ | Read/Write/Destructive tiers; ConfirmTyped for destructive text |
| `mcp/` | stub | MCP tool discovery (Phase 3.6) |
| `operator.rs` | ✓ | StepEnforcer, ToolDescriptor, RiskLevel |

### UI (`lagado-ui/src/`)

**Working:** `/` chat, `/awakening`, `/immersive` (live VM feed + VM/Host toggle + draggable),
`/vm` (boot/stop/status), `/settings`, `/server`, `/terminal` (auth-gated), `/vault`, `/design`.

**Auth flow:** loading → awakening → signup or login → app.

**Color system:** deep navy bg (`#080c14`), blue (`#3b82f6`) + purple (`#8b5cf6`) accents.

## Key invariants — DO NOT BREAK

1. **Mutex guard discipline**: guards MUST be dropped before any `.await`.
2. **Clean-context routing**: `classify_intent()` MUST receive only the current user message.
3. **HITL chokepoint**: all agent actions go through `gate::evaluate_action()`. Never bypass.
4. **No wildcard `_` arms** on enums you define.
5. **No `std::process::exit(1)`** from library code.
6. **No AI attribution** in commits, code, PRs, or any artifact. Author: `Lagado Labs <lagadolabs@gmail.com>`.
7. **DEK discipline**: never persist raw DEK. `active_key()` is the only crypto entry point.
8. **SSH readiness**: never set `vm_ssh_port` before TCP poll confirms sshd is accepting.

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

Opus/Sonnet: planning, review, architecture. Haiku: all implementation and file edits.
Verify with `cargo check --workspace` + `npx tsc --noEmit` after every Haiku task.

## Status (2026-06-10)

**Phase 1.4 COMPLETE. Phase 2 COMPLETE. Phase 3.1 COMPLETE. Phase 3.2 COMPLETE. Phase 3.3 COMPLETE. Phase 3.4 COMPLETE. Phase 3.5 COMPLETE.**

### What works end-to-end
- App launches → auth gate → signup or login → chat
- Immersive opens → VM auto-boots → live QEMU desktop feed via QMP screendump
- Agent actions route through SSH → xdotool in VM guest
- RecoveryManager active: parse failures, loops, deadlocks all handled
- Cold memory encrypted with session DEK; episodes persist across sessions
- SleepGate decays hot/warm every 5min; cold tier never deleted
- 1.2B classifier on :8081 handles intent classification (few-shot, ~80% accuracy)
- Visual encoder runs in-process via libmtmd FFI; embeddings stored in MemoryTiers; cosine retrieval active
- 2 server child processes (main 8B + 1.2B classifier) use KillOnDrop (defined in `bootstrap.rs`) — no orphans on app exit
- ServerGuard polls /health every 10s; declares crash after 3 consecutive failures; restarts and retries indefinitely; emits `server_crashed`/`server_restarted`/`server_restart_failed` tauri events
- VLM subprocess retired; vision now in-process FFI only
- GPU detection: NVIDIA (nvidia-smi) + AMD (DRM sysfs); conservative binary fit (vram_free ≥ model×1.1 → ngl=99, else CPU); moe_experts_on_cpu wired to --cpu-moe for MoE models; vram_fit_fraction() on ServerConfig ready for Phase 3.x GGUF parser
- cgroup v2 sandbox: apply_limits on llama/classifier/qemu; QEMU -sandbox seccomp flag; cleanup_stale at startup; memory caps from model file size × 1.5 (model-agnostic, env overrideable)

### Phase 3 remaining
- **3.6:** MCP tool discovery — stub at `mcp/mod.rs`. Design open: transport (stdio subprocess preferred over new localhost port per security model), ToolCall::Mcp enum extension deferred until execution surface is decided (both VM-caged and host-side HITL modes needed). "34 tools" spec was in deleted LAPUTA_v1_UNIFIED_MASTER_PLAN_v4.md — needs user input before catalog is defined.
