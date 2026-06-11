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
- Platform gate via `lagado_vision_ffi` cfg (set by build.rs ONLY when `vendored/llama.cpp-2/include/llama.h` exists)
- CI skips shim compilation gracefully when vendored headers absent (gitignored); no linker errors
- `encode_and_store_async()` fires in background tokio::spawn, encode in spawn_blocking outside lock
- Visual retrieval wired into agent_loop: encodes current frame once per invocation → top-3 similar episodes → prompt
- `[[bin]] test=false` in Cargo.toml (static lib linking doesn't propagate to bin test targets)
- `cargo test -p lagado-agent` requires `LD_LIBRARY_PATH=.../vendored/llama.cpp-2/build/bin` (stale rpath in vendored libllama.so)
- 101 lib tests pass (HEAD d366c55)

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
| `skill_library.rs` | ✓ read / ✗ write | Experiential depth layer — retrieval wired, distillation not yet built |
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

## Status (2026-06-11)

**Phase 1.4 COMPLETE. Phase 2 COMPLETE. Phase 3.1–3.6 COMPLETE. UI design system COMPLETE. CI all platforms green.**

HEAD: `d366c55`. 101 lib tests. Ubuntu ✓ macOS ✓ Windows ✓.

### UI ↔ Backend wire (verified)
`useTauriAgent.ts` → direct `invoke()` calls → Tauri commands. Events back via `app.emit()` / `listen()`.
`server.rs` WebSocket (port 9090) and `useAgentSocket.ts` are **orphaned** — never called in production.

### Two-layer experiential memory (architecture settled)
- **action_graph** = muscle memory: exact blake3(screen) hash → action shortcut, bypasses inference at score ≥ 0.65
- **skill_library** = depth: situation-class → advisory NL procedures, informs inference, never replays
  - Retrieved on turns 1-3 only (tapers off; action history + live screen are fresher by turn 4+)
  - **Read path wired. Write/distillation NOT built — library is currently inert in production.**

### What works end-to-end
- App launches → Awakening → auth → chat
- `send_goal` → hydra → 1.2B classifier → agent_loop (episodic + visual + skill context turns 1-3, tools every turn)
- Immersive → VM auto-boots → live QEMU desktop feed → SSH → xdotool actuation
- RecoveryManager, SleepGate, ServerGuard, cgroup v2 sandbox all active
- 44 bundled native Rust tools, MCP stdio client, confidence gating, HITL gate
- Network proxy settings: SOCKS5/HTTP opt-in, Tor/Whonix presets
- Vault FAISS: `build_index.py` indexes vault facts + chunks via MiniLM-L6-v2

### Remaining / next session
- **Skill distillation** (immediate next): wire write path — at episode completion (Done/Task/Abort),
  call LLM to extract (name, description, approach) from trajectory → `skill_library.save()`.
  Lives in `sleep_gate.rs` and/or agent_loop Done/Abort handlers.
- Settings tool manager: view/enable/disable tools, change trust levels (get_tools, set_tool_trust, toggle_tool_enabled)
- GGUF parser for MoE detection (auto-set moe_experts_on_cpu, enable partial GPU offload)
- FAISS action graph and pre-seeding: deferred indefinitely (architecture changed to skill_library experiential model)
