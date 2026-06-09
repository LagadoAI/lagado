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
- Vendored `llama-server` subprocess (HTTP inference on :8080, NOT FFI)
- QEMU desktop VM (agent's sandboxed working surface)

Inference: HTTP to `llama-server` → `/v1/chat/completions`.
Model: LFM2.5 (`LFM2.5-8B-A1B-Q4_K_M.gguf`) from `~/.laputa-secure/models/`.

### Agent pipeline (Hydra orchestrator)

```
User message
    ↓
isPaused? → YES → send_chat() → chat inference (no tools, no screen)
    ↓ NO
action_graph shortcut (score ≥ 0.65)? → YES → agent_loop directly
    ↓ NO
hydra::classify_intent() [CLEAN PROMPT — zero history, current message only]
    ↓
CHAT → chat_response() with RAG context
INTERACTIVE/REASONING → agent_loop() with HITL gate + RecoveryManager
```

**CLEAN-CONTEXT DISCIPLINE is non-negotiable.** `classify_intent()` receives ONLY the current user message.

**State hash:** `blake3(perceptor.read_screen())` — used for action_graph lookups and recovery keys.

### VM Architecture (Phase 1.4 — COMPLETE)

The agent operates a sandboxed QEMU VM, not the host desktop.

```
QemuDesktopBackend boots qcow2 with QMP socket + VirtIO display
         ↓
Perception: SSH into guest → perceive.py (AT-SPI2 on Xorg) → PerceptionCache (ref_id → cx,cy)
Actuator:   SSH into guest → xdotool mousemove cx cy click 1 (coords resolved from cache)
Live feed:  QMP screendump (format:png) → /dev/shm/lagado_frame.png → base64 → Immersive canvas
```

**ISO-agnostic:** VmConfig accepts any qcow2/img path. Guest only needs SSH + xdotool.
**Dev image:** `~/.laputa-secure/vm-images/Arch-Linux-x86_64-cloudimg.qcow2`
**Seed ISO:** `~/.laputa-secure/vm-images/seed.iso` (cloud-init, first-boot only)
**Guest:** user `laputa:laputa`, auto-login, XFCE4, xdotool + AT-SPI2 pre-installed

**QMP screendump** — works Linux/Mac/Windows, no compositor needed. `format:png` required (default is PPM).
**SSH readiness:** `vm_ssh_port` set asynchronously via background TCP poll — agent never attempts SSH before sshd is up.
**Frame transport:** self-clocked (one frame in flight), Blake3 whole-frame delta gate (skip unchanged).
**Auto-kill:** `Drop for VmHandle` kills QEMU when the app closes.
**Source toggle:** Immersive page supports VM (QMP) or Host (grim/scrot) capture, persisted to localStorage.

### Auth (Phase 2 — COMPLETE)

**Wrapped DEK scheme** — FileVault/1Password pattern:
- Signup: random 32-byte DEK wrapped with `Argon2id(password, salt1)` + `Argon2id(recovery_phrase, salt2)`
- Both blobs persisted to `~/.laputa-secure/config/keychain.json` — raw DEK never touches disk
- Login: Argon2id(password, salt) → unwrap DEK → `auth::set_session_dek(dek)`
- Recovery: recovery_phrase → unwrap DEK → re-wrap with new password
- Lockout: 3 failures → 10-min cooldown, persisted to `lockout.json`, fail-closed if tampered
- `auth::active_key()` returns session DEK if set, falls back to `machine_passphrase()` (dev only)
- All cold-tier memory encryption routes through `active_key()`

### Key modules (`lagado-agent/src/`)

| Module | Status | What it does |
|---|---|---|
| `hydra.rs` | ✓ | Dual-model orchestrator, intent routing, blake3 state hash |
| `agent.rs` | ✓ | Agent loop, HITL gate, RecoveryManager wired, loop/deadlock detection |
| `recovery.rs` | ✓ wired | 7 failure-mode dispatcher, graph-backed + LLM recovery, connected to agent loop |
| `memory_tiers.rs` | ✓ | Hot/warm/cold tiers, AES-256-GCM on cold via active_key() |
| `sleep_gate.rs` | stub | Background decay loop — built, not started in main.rs yet |
| `chronos.rs` | ✓ | SQLite timeline, T=0 anchor |
| `retrieval.rs` | ✓ | RAG K=15, Jaccard scoring |
| `action_graph.rs` | ✓ | SQLite workflow store, shortcut path, record_outcome wired |
| `skill_library.rs` | ✓ | Voyager-style multi-step procedure store |
| `security/crypto.rs` | ✓ | AES-256-GCM, Argon2id, `encrypt_with_key`/`decrypt_with_key` for DEK wrapping |
| `auth/mod.rs` | ✓ | Wrapped DEK, lockout, `active_key()`, `set_session_dek()` |
| `self_model.rs` | ✓ | Accepted beliefs, distill feed |
| `distill.rs` | ✓ hooks | Replay manifest for Phase 3 QLoRA |
| `perception/mod.rs` | ✓ | `PerceptionCache` (shared ref_id→coords), `parse_ref_coords`, MockPerceptor/Actuator |
| `perception/linux.rs` | ✓ | AT-SPI2 via perceive.py, xdotool, shared PerceptionCache |
| `perception/capture.rs` | stub | grim/scrot — superseded by QMP for VM, still used for host mode |
| `perception/delta.rs` | ✓ | Blake3 per-cell change detection |
| `perception/vlm_adapter.rs` | stub | LFM2.5-VL bridge (Phase 3) |
| `projector/` | ✓ | Cross-platform input executor, Validator |
| `terminal/` | ✓ | PTY session manager |
| `vm/mod.rs` | ✓ | QemuDesktopBackend, QMP client, DynamicActuator/Perceptor, VmSshPort |
| `vm/qmp.rs` | ✓ | QMP Unix socket client — screendump(format:png), capability handshake |
| `vm/ssh_actuator.rs` | ✓ | SSH→xdotool, shared PerceptionCache, ref_id→coords lookup |
| `vm/ssh_perceptor.rs` | ✓ | SSH→perceive.py, populates shared PerceptionCache |
| `governor.rs` | ✓ | Hardware detection → capability tier |
| `config.rs` | ✓ | Cross-platform paths, model selection, env overrides (debug-only) |
| `gate.rs` | ✓ | Read/Write/Destructive tiers; Type with destructive text → ConfirmTyped |
| `mcp/` | stub | MCP tool discovery (Phase 3) |
| `operator.rs` | ✓ | StepEnforcer, ToolDescriptor, RiskLevel, core_tools() |

### UI (`lagado-ui/src/`)

**Working:** `/` chat, `/awakening`, `/immersive` (live VM feed + VM/Host toggle + draggable controls),
`/vm` (boot/stop/status), `/settings`, `/server`, `/terminal` (auth-gated), `/vault`, `/design`.

**Auth flow:** loading → awakening → signup (`/setup/account`) or login → app.
Login: password unlock with lockout countdown. Recovery: inline recovery phrase flow.
Signup: two-step (password + recovery phrase with vault warning).

**Phase 3 (coming-soon):** `/code`, `/mcp`.

**Color system:** deep navy bg (`#080c14`), blue (`#3b82f6`) + purple (`#8b5cf6`) accents,
red for destructive/deny only, green for success/connected only.

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

Haiku completion format:
```
## TASK COMPLETE
**Files changed:** <paths>
**What was done:** <summary>
**cargo check:** <last 5 lines>
**tsc:** <output or "clean">
```

## Status (2026-06-09)

**Phase 1.4 COMPLETE — VM Desktop fully operational.**
**Phase 2 Auth COMPLETE — wrapped DEK, lockout, signup/login/recovery UI.**

### What works end-to-end
- App launches → auth gate → signup (first launch) or login → chat
- Immersive opens → VM auto-boots → live QEMU desktop feed via QMP screendump
- Agent actions route through SSH → xdotool in VM guest (ref_id resolved to coords)
- RecoveryManager active: parse failures, loops, deadlocks all handled before abort
- Cold memory encrypted with session DEK (user's password, not machine ID)
- Destructive text inputs (rm -rf, DROP TABLE, etc.) require typed confirmation

### Known remaining items (Phase 3)
- `sleep_gate.rs` not started in main.rs — memory never decays between sessions
- `MemoryTiers` not instantiated in agent loop — still using legacy `memory.rs`
- `action_graph::record_outcome()` wired but state_hash quality depends on screen read timing
- `useAgentSocket.ts` is dead code — WebSocket hook superseded by Tauri IPC
- Dual-model routing (350M classifier) deferred — 8B handles classification
- VLM pipeline (LFM2.5-VL) deferred
- `security/sandbox.rs` — seccomp/cgroups/namespace isolation not built
- llama-server crash recovery — no auto-restart on mid-session server death
- `projector/` is a parallel unused implementation alongside `perception/linux.rs`
- No `InputArbiter` — user > agent > harness input priority not enforced

### Phase 3 build order
1. Wire `MemoryTiers` into agent loop, start `SleepGate` in main.rs
2. 350M intent classifier on separate llama-server port
3. VLM vision pipeline (LFM2.5-VL) using VM frames
4. `security/sandbox.rs` — seccomp profile for QEMU + agent subprocesses
5. llama-server health monitor + auto-restart
6. MCP tool discovery (34 tools)
