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
- VLM subprocess on :8082 (LFM2.5-VL-450M + mmproj, vision — **being replaced by in-process FFI**)
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

**Phase 3.3 IN PROGRESS — Visual embedding (not text description):**

The VLM should NOT describe the screen in text. It should produce embedding vectors that feed
the memory system directly. The architecture:

```
Frame (PNG) → in-process visual encoder (libmtmd.so FFI) → 1024-dim vector
                                                                ↓
                                                   MemoryTiers embedding BLOB column
                                                                ↓
                                               cosine similarity retrieval at query time
                                                                ↓
                                          top-K visually similar past episodes → agent context
```

**Key facts for implementation:**
- `libmtmd.so` is already compiled at `vendored/llama.cpp-2/build/bin/libmtmd.so`
- Header: `vendored/llama.cpp-2/tools/mtmd/mtmd.h`
- Key C API calls needed:
  ```c
  llama_model_load_from_file(path, params) → llama_model*
  mtmd_init_from_file(mmproj_path, model, params) → mtmd_context*
  mtmd_bitmap_init(nx, ny, rgb_data) → mtmd_bitmap*
  mtmd_input_chunks_init() → mtmd_input_chunks*
  mtmd_tokenize(ctx, chunks, text, bitmaps, n) → i32
  mtmd_encode_chunk(ctx, chunk) → i32   // runs vision encoder
  mtmd_get_output_embd(ctx) → *f32      // n_embd_inp × n_tokens floats
  // mean-pool over n_tokens → single 1024-dim vector
  ```
- Output size: `llama_model_n_embd_inp(model) × n_image_tokens × sizeof(f32)`
- PNG decode: use `image` crate → raw RGB bytes → `mtmd_bitmap_init`
- `build.rs` must link: `libllama.so`, `libmtmd.so`, `libggml.so` from vendored build/bin
- The VLM subprocess (port 8082) can be retired once in-process path works
- `VlmPerceptor` text description path → retire, replace with embedding store+retrieval
- MemoryTiers needs: `embedding BLOB` column (JSON-encoded f32 array), cosine similarity search

**The VLM subprocess (`perception/vlm_adapter.rs` + `ensure_vlm_server`) currently committed
is the text-description version. It compiles and ships the mmproj download. It will be
replaced in the same session by the in-process FFI path — do not remove it yet.**

### Key modules (`lagado-agent/src/`)

| Module | Status | What it does |
|---|---|---|
| `hydra.rs` | ✓ | Dual-model orchestrator, few-shot classifier on :8081, blake3 state hash |
| `agent.rs` | ✓ | Agent loop, episodic memory context, HITL gate, RecoveryManager |
| `recovery.rs` | ✓ wired | 7 failure-mode dispatcher, graph-backed + LLM recovery |
| `memory_tiers.rs` | ✓ | Hot/warm/cold tiers, push_episode, assemble_context, decay protects cold |
| `sleep_gate.rs` | ✓ | Background decay every 5min — started in main.rs |
| `chronos.rs` | ✓ | SQLite timeline, T=0 anchor |
| `retrieval.rs` | ✓ | RAG K=15, Jaccard scoring |
| `action_graph.rs` | ✓ | SQLite workflow store, shortcut path |
| `skill_library.rs` | ✓ | Voyager-style multi-step procedure store |
| `security/crypto.rs` | ✓ | AES-256-GCM, Argon2id, DEK wrapping |
| `auth/mod.rs` | ✓ | Wrapped DEK, lockout, `active_key()`, `set_session_dek()` |
| `self_model.rs` | ✓ | Accepted beliefs, distill feed |
| `distill.rs` | ✓ hooks | Replay manifest for Phase 3 QLoRA |
| `perception/mod.rs` | ✓ | PerceptionCache, VlmPerceptor (text, being replaced) |
| `perception/linux.rs` | ✓ | AT-SPI2 via perceive.py, xdotool |
| `perception/delta.rs` | ✓ | Blake3 per-cell change detection |
| `perception/vlm_adapter.rs` | ✓→replacing | Text description via HTTP — being replaced by in-process FFI |
| `perception/capture.rs` | stub | grim/scrot host mode |
| `vm/mod.rs` | ✓ | QemuDesktopBackend, QMP, DynamicActuator/Perceptor |
| `vm/qmp.rs` | ✓ | QMP socket client — screendump(format:png) |
| `vm/ssh_actuator.rs` | ✓ | SSH→xdotool, PerceptionCache coord resolution |
| `vm/ssh_perceptor.rs` | ✓ | SSH→perceive.py, populates PerceptionCache |
| `governor.rs` | ✓ | Hardware detection → capability tier |
| `config.rs` | ✓ | Paths, FRAME_PATH constant, VLM/classifier/main server config |
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

## Status (2026-06-09)

**Phase 1.4 COMPLETE. Phase 2 COMPLETE. Phase 3.1 COMPLETE. Phase 3.2 COMPLETE.**
**Phase 3.3 IN PROGRESS — visual embedding via in-process libmtmd FFI.**

### What works end-to-end
- App launches → auth gate → signup or login → chat
- Immersive opens → VM auto-boots → live QEMU desktop feed via QMP screendump
- Agent actions route through SSH → xdotool in VM guest
- RecoveryManager active: parse failures, loops, deadlocks all handled
- Cold memory encrypted with session DEK; episodes persist across sessions
- SleepGate decays hot/warm every 5min; cold tier never deleted
- 1.2B classifier on :8081 handles intent classification (few-shot, ~80% accuracy)
- VLM server on :8082 can describe screen in text (Phase 3.3 text path — being replaced)
- All 3 server child processes use KillOnDrop — no orphans on app exit

### Phase 3 remaining
- **3.3 (IN PROGRESS):** Replace VlmPerceptor text path with in-process `libmtmd.so` FFI embeddings.
  Add `embedding BLOB` to MemoryTiers + cosine retrieval. Retire VLM subprocess.
- **3.4:** llama-server health monitor + auto-restart on crash
- **3.5:** `security/sandbox.rs` — seccomp/cgroups for QEMU + agent subprocesses
- **3.6:** MCP tool discovery (34 tools)
