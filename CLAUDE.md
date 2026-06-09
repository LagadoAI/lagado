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
CHAT → chat_response() with RAG context assembled from memory_tiers
INTERACTIVE/REASONING → agent_loop() with HITL gate
```

**CLEAN-CONTEXT DISCIPLINE is non-negotiable.** `classify_intent()` receives ONLY the current user message.

### VM Architecture (Phase 1.4 — ACTIVE BUILD)

The agent operates a sandboxed QEMU VM, not the host desktop.

```
QemuDesktopBackend boots qcow2 with QMP socket + VirtIO display
         ↓
Perception: SSH into guest → perceive.py (AT-SPI2 on Xorg)
Actuator:   SSH into guest → xdotool click/type/key
Live feed:  QMP screendump → /dev/shm/lagado_frame.png → base64 → Immersive canvas
```

**ISO-agnostic:** VmConfig accepts any qcow2/img path. Guest only needs SSH + xdotool.
**Dev image:** `~/.laputa-secure/vm-images/Arch-Linux-x86_64-cloudimg.qcow2`
**Seed ISO:** `~/.laputa-secure/vm-images/seed.iso` (cloud-init, first-boot only)
**Guest:** user `laputa:laputa`, auto-login, XFCE4, xdotool + AT-SPI2 pre-installed, SPICE agent

**QMP screendump is the capture method** — works Linux/Mac/Windows, no compositor needed.
**Frame transport:** self-clocked (one frame in flight), whole-frame Blake3 delta gate (skip unchanged).

### Key modules (`lagado-agent/src/`)

| Module | Status | What it does |
|---|---|---|
| `hydra.rs` | ✓ | Dual-model orchestrator, intent routing |
| `agent.rs` | ✓ | Agent loop, HITL permission gate, mutex discipline |
| `memory_tiers.rs` | ✓ | Hot/warm/cold tiers, AES-256-GCM on cold |
| `chronos.rs` | ✓ | SQLite timeline, T=0 anchor |
| `sleep_gate.rs` | ✓ stub | Background decay loop |
| `retrieval.rs` | ✓ | RAG K=15, Jaccard scoring |
| `action_graph.rs` | ✓ | SQLite workflow store, shortcut path |
| `skill_library.rs` | ✓ | Voyager-style multi-step procedure store |
| `security/crypto.rs` | ✓ tested | AES-256-GCM, Argon2id — **machine_passphrase() must be replaced before auth ships** |
| `self_model.rs` | ✓ | Accepted beliefs, distill feed |
| `distill.rs` | ✓ hooks | Replay manifest for Phase 2 QLoRA |
| `perception/mod.rs` | ✓ | AT-SPI2 via perceive.py, xdotool actuator |
| `perception/capture.rs` | ✓ stub | grim/scrot fallback — QMP path supersedes this |
| `perception/delta.rs` | ✓ | Blake3 per-cell change detection |
| `perception/vlm_adapter.rs` | stub | LFM2.5-VL bridge (Phase 2) |
| `projector/` | ✓ | Cross-platform input executor, Validator |
| `terminal/` | ✓ | PTY session manager |
| `vm/mod.rs` | 🔨 building | QemuDesktopBackend, QMP client, SSH actuator |
| `governor.rs` | ✓ | Hardware detection → capability tier |
| `config.rs` | ✓ | Cross-platform paths, model selection, env overrides |
| `gate.rs` | ✓ | Risk tiers, Authorized<ToolCall> chokepoint |
| `auth/` | stub | Phase 2: wrapped DEK scheme (see auth-vault-design memory) |
| `mcp/` | stub | MCP tool discovery (Phase 2) |
| `recovery.rs` | ✓ | 7 failure-mode dispatcher |
| `operator.rs` | ✓ | StepEnforcer, ToolDescriptor, RiskLevel, core_tools() |

### UI (`lagado-ui/src/`)

**Working:** `/` chat, `/awakening`, `/immersive` (live feed), `/settings` (models+chronos),
`/server` (real status), `/terminal` (real bash), `/vault` (real files), `/design` (component canvas).
All pages have ← Chat back navigation.

**Phase 2 (coming-soon banner):** `/code`, `/vm` (being wired now), `/mcp`.

**Color system:** deep navy bg (`#080c14`), blue (`#3b82f6`) + purple (`#8b5cf6`) accents,
red for destructive/deny only, green for success/connected only.

## Key invariants — DO NOT BREAK

1. **Mutex guard discipline**: guards MUST be dropped before any `.await`.
2. **Clean-context routing**: `classify_intent()` MUST receive only the current user message.
3. **Authorized<ToolCall> chokepoint**: only `gate` can mint it. Never bypass.
4. **No wildcard `_` arms** on enums you define.
5. **No `std::process::exit(1)`** from library code.
6. **No AI attribution** in commits, code, PRs, or any artifact. Author: `Lagado Labs <lagadolabs@gmail.com>`.

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

Opus: planning, review, architecture. Haiku: all implementation and file edits.
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

**Phase 1.3+ complete.** Core pipeline working end-to-end. App launches, routing verified.

**Phase 1.4 IN PROGRESS — VM Desktop:**
1. `QemuDesktopBackend` — boot qcow2 with QMP socket + VirtIO display (🔨 next)
2. QMP client — `screendump` → replaces grim in `capture_frame`
3. `SshActuator` — SSH into guest for xdotool commands
4. `SshPerceptor` — SSH into guest for perceive.py
5. VM Manager UI — boot/stop/status controls

**Phase 2 (after VM):**
- Auth: wrapped DEK scheme, signup/login UI, lockout (spec in `memory/auth-vault-design.md`)
- 350M intent classifier on separate server port
- VLM vision pipeline (LFM2.5-VL)
