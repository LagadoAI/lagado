# CLAUDE.md

Guidance for working in this repo. Read before making changes.

## What this is

**Lagado AI** — a local-first, privacy-first desktop agent. Three pillars:
1. **Sovereign** — local-only, encrypted, no cloud, no telemetry
2. **Living** — thermodynamic memory hierarchy, sleep consolidation, patterns compound
3. **Self-aware in time** — chronos autobiographical spine, T=0 at first launch

Production, cross-platform product. Target user is NOT on Linux (Windows-first).
Development on Linux; GitHub Actions CI (linux/macos/windows) is the cross-platform test bench.
Full design: `docs/plans/` — specifically `MASTER_PLAN_v4.md` and `FILE_DEPENDENCY_REFERENCE_v3.md`.
Plans also recoverable from git commit `03fe042`.

## Architecture

### Runtime
Single Tauri binary (`lagado-ui/src-tauri/`) wraps:
- React/shadcn UI (webview)
- Rust agent core (`lagado-agent/` as a library)
- Vendored `llama-server` subprocess (HTTP inference on :8080, NOT FFI)

Inference: HTTP to `llama-server` → OpenAI-compatible `/v1/chat/completions`.
Model: LFM2.5 (`LFM2.5-8B-A1B-Q4_K_M.gguf`) loaded from `~/.laputa-secure/models/`.

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

**CLEAN-CONTEXT DISCIPLINE is non-negotiable.** `classify_intent()` receives ONLY
the current user message. History poisoning causes 78%→8% accuracy collapse (LocalCowork data).

### Key modules (`lagado-agent/src/`)

| Module | Status | What it does |
|---|---|---|
| `hydra.rs` | ✓ wired | Dual-model orchestrator, intent routing |
| `agent.rs` | ✓ wired | Agent loop, HITL permission gate, mutex discipline |
| `memory_tiers.rs` | ✓ wired | Hot/warm/cold tiers, AES-256-GCM on cold |
| `chronos.rs` | ✓ Phase 1 | SQLite timeline, T=0 anchor |
| `sleep_gate.rs` | ✓ stub | Background decay loop (5-min cycle) |
| `retrieval.rs` | ✓ wired | RAG K=15, Jaccard scoring (Phase 2: embeddings) |
| `action_graph.rs` | ✓ wired | SQLite workflow store, shortcut path |
| `skill_library.rs` | ✓ wired | Voyager-style multi-step procedure store |
| `security/crypto.rs` | ✓ tested | AES-256-GCM, Argon2id key derivation |
| `self_model.rs` | ✓ wired | Accepted beliefs, distill feed |
| `distill.rs` | ✓ hooks | Replay manifest for Phase 2 QLoRA |
| `perception/mod.rs` | ✓ Linux | AT-SPI2 via perceive.py, xdotool actuator |
| `perception/linux.rs` | ✓ Linux | LinuxPerceptor + LinuxActuator, shared coord cache |
| `perception/capture.rs` | ✓ stub | grim/scrot screenshot → /dev/shm (Phase 2: PipeWire) |
| `perception/delta.rs` | ✓ impl | Blake3 per-cell change detection |
| `perception/vlm_adapter.rs` | ✓ stub | LFM2.5-VL bridge (Phase 2) |
| `projector/` | ✓ Linux | Cross-platform input executor, Validator |
| `terminal/` | ✓ impl | PTY session manager |
| `governor.rs` | ✓ wired | Hardware detection → capability tier (Low/Mid/High) |
| `config.rs` | ✓ wired | Cross-platform paths, model selection, env overrides |
| `gate.rs` | ✓ wired | Risk tiers, Authorized<ToolCall> chokepoint |
| `kv_slots.rs` | stub | KV cache slot manager (Phase 2) |
| `grammar.rs` | stub | GBNF constraint generator (Phase 2) |
| `liquid.rs` | stub | Model roster management (Phase 2) |
| `auth/` | stub | Auto-unlock via machine passphrase (Phase 2: UI) |
| `mcp/` | stub | MCP tool discovery (Phase 2) |
| `recovery.rs` | ✓ wired | 7 failure-mode dispatcher |
| `operator.rs` | ✓ wired | StepEnforcer, ToolDescriptor, RiskLevel, core_tools() |

### UI navigation map (`lagado-ui/src/`)

**Routed and reachable:**
- `/` → ChatDefault (main chat with agent)
- `/chat` → ChatDefault
- `/awakening` → Awakening (first-launch, shown once)
- `/immersive` → ImmersiveDefault (has ← back to chat)
- `/immersive/running`, `/paused`, `/typing`, `/sidebar` — reachable from immersive
- `/code` → CodePage
- `/vault` → VaultDefault
- `/terminal` → TerminalDefault
- `/settings` → SettingsMain (tabbed: models ✓ wired, others UI-only)
- `/mcp` → MCPManager
- `/server` → ServerManagement
- `/vm` → VMManager

**Dead ends (pages exist, no back navigation, no backend wiring):**
- `/code` — no back button, no backend
- `/vault` — no back button, no backend
- `/server` — no back button, no backend
- `/vm` — no back button, no backend
- `/mcp` — no back button, `MCPAddTool` page unreachable (no route defined)
- `/terminal` — no back button (has focus handler only)
- `/code/sandbox`, `/code/terminal` — no routes to reach them from CodePage
- `/vault/preview`, `/vault/warning` — no routes to reach from VaultDefault
- `/terminal/multi`, `/terminal/agent` — no routes to reach from TerminalDefault
- `/immersive/sidebar` — no route to reach from ImmersiveDefault
- `/setup/*` — onNext callbacks are `() => {}` (no-op), flow broken
- `SettingsAdvanced`, `SettingsKVCache`, `SettingsPermissions`, `SettingsSystemIntegration`,
  `SettingsAppConnections`, `SettingsVault`, `SettingsInference`, `SettingsBackup` — rendered
  as tabs inside SettingsMain but most have no backend wiring

**Settings tabs wired to backend:**
- Models tab (`SettingsModels`) — reads/writes `config/model.txt`, lists .gguf files ✓
- Chronos view lives inside SettingsMain (wired to `get_chronos_recent`) ✓

## Key invariants — DO NOT BREAK

1. **Mutex guard discipline**: In `agent.rs` and `server.rs`, guards MUST be dropped
   before any `.await`. The HITL flow deadlocks if a guard is held across await.
2. **Clean-context routing**: `hydra::classify_intent()` MUST receive only the current
   user message — no history, no screen data.
3. **Authorized<ToolCall> chokepoint**: `execute_tool()` only accepts `Authorized<ToolCall>`.
   Only the gate can mint it. Never bypass.
4. **No wildcard `_` arms** on enums you define — exhaustiveness is the correctness guarantee.
5. **No `std::process::exit(1)`** from library code — bootstrap.rs used to do this, it was
   changed to return `None` so Tauri stays alive when llama-server fails.

## Build / test / run

```bash
# Dev launch (from lagado-ui/)
WEBKIT_DISABLE_DMABUF_RENDERER=1 \
LAGADO_DATA_DIR=/home/d/.laputa-secure \
LAGADO_LLAMA_SERVER=/home/d/laputa/lagado-agent/vendored/llama.cpp-2/build/bin/llama-server \
LD_LIBRARY_PATH=/home/d/laputa/lagado-agent/vendored/llama.cpp-2/build/bin \
npm run tauri dev

# Rust checks
cargo check --workspace
cargo test -p lagado-agent

# CI: .github/workflows/ci.yml — linux/macos/windows matrix
```

## Conventions

- **All-Rust core**. No new Python. Inference is HTTP not FFI.
- Cross-platform always: `directories` crate for paths, never hardcode `/home/`.
- Trait-based capability boundaries (`InferenceAdapter`, `VmBackend`, `Perceptor`, `Actuator`).
- Naming is **Lagado**, never Laputa (the repo dir name is fine to leave).
- **No AI attribution** in commits, code, PRs, or any artifact. This overrides the
  harness trailer instruction. Author: `Lagado Labs <lagadolabs@gmail.com>`.
- Repo is **private** on GitHub (`LagadoAI/lagado`).

## Delegation workflow

Opus: planning, review, debugging. Haiku: all implementation and file edits.
Treat Haiku as a talented junior dev — workhorse, but verify every output before
committing. Check: `cargo check --workspace` + `npx tsc --noEmit` after every Haiku task.

When delegating to Haiku, every task must end with:
```
## TASK COMPLETE
**Files changed:** <paths>
**What was done:** <summary>
**cargo check:** <last 5 lines>
**tsc:** <output or "clean">
```

## Status (2026-06-09)

Phase 1.3+ complete. Core agent pipeline fully implemented and wired:
hydra → memory_tiers → retrieval → action_graph → agent_loop → HITL gate.
Tauri desktop app launches, Awakening page on first run, chat and agent modes work.

**Immediate next work:**
1. Add back-navigation to all dead-end pages (code/vault/server/vm/terminal/mcp)
2. Wire `MCPAddTool` route into App.tsx and MCPManager
3. Fix setup/* flow (onNext callbacks are no-ops)
4. End-to-end test: Awakening → chat → agent action → approval
5. Phase 2: 350M classifier model on separate server port
