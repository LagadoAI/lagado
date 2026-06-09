# CLAUDE.md

Guidance for working in this repo. Read before making changes.

## What this is

**Lagado AI** — a local-first, privacy-first desktop agent that operates the
user's computer on their behalf, with every write action gated by a human.
This is a **production, cross-platform product**. The target user is **not on
Linux** (Windows-first); do not assume Linux. Development happens on Linux, so
**GitHub Actions CI (linux/macos/windows matrix) is the cross-platform test
bench** — there is no Windows hardware here.

This is the *second* attempt at the build. The first attempt's Python/scaffolding
is being destroyed and replaced with an all-Rust core. Treat anything Python or
clearly first-attempt as legacy slated for removal, not as reference.

## Architecture (what is actually wired)

The real program is the Rust crate at **`lagado-agent/`**. Inference runs over
**HTTP, not FFI**: the agent spawns a vendored `llama-server` and talks to its
OpenAI-compatible `/v1/chat/completions`. The model is LFM2.5
(`LFM2.5-8B-A1B-Q4_K_M.gguf`).

Modules compiled into the binary (declared in `src/main.rs`, in dependency order):

- `main.rs` — thin wiring only: init tracing, `bootstrap::ensure_llama_server()`,
  build adapter + perceptor + actuator + `AgentState`, spawn the WS server, keep-alive loop.
- `bootstrap.rs` — detect/health-check/spawn `llama-server`.
- `config.rs` — cross-platform paths via the `directories` crate. Env overrides
  (`LAGADO_DATA_DIR`, `MODEL_PATH`, `LLAMA_SERVER`, `CHRONOS_LOG`, `SYSTEM_PROMPT`)
  are **debug-only** via `dev_override`; release reads nothing from env for those.
  Handles `.exe` on Windows. `CONTEXT_SIZE = 32768`.
- `inference/` — `InferenceAdapter` trait + `LlamaCppAdapter` (HTTP client).
- `perception/mod.rs` — `Perceptor` (read_screen) + `Actuator` (click/type_text/key)
  traits, plus `MockPerceptor`/`MockActuator`. Mock actuator never echoes typed text.
- `agent.rs` — `AgentState`, `agent_loop`, `execute_tool`, `request_and_await_approval`.
- `server.rs` — WebSocket server on `127.0.0.1:9090`; envelope routing.
- `envelope.rs` — versioned protocol (v1). Inbound: `goal`/`command`/`approval`.
  Outbound: `permission`/`action_log`/`status`.
- `gate.rs` — risk classification + `describe`/`describe_redacted` (typed text is
  redacted from everything persisted; full text lives only in the live `permission` envelope).
- `bracket_parser.rs` — parses the model's bracket tool-call format.
- `chronos.rs` — append-only action log (SQLite via `rusqlite`).
- `vm/` — `QemuMicrovmBackend` behind a `VmBackend` trait. **Exists but is NOT in
  the action path.** Exercised only by `src/bin/vm_proof.rs`.
- Others wired but minimal: `action_graph`, `forge`, `governor`, `operator`,
  `memory`, `types`.

**Orphaned — present on disk but NOT compiled** (not declared in `main.rs`/`lib.rs`):
`src/auth/`, `src/connectors/`, `src/lens/`, `src/mcp/`, `src/permissions/`,
`src/projector/`, `src/security/`, `src/system/`, `src/terminal/`, `src/url_handler/`,
`src/recovery.rs`, `src/verifier.rs`. These are first-attempt scaffolding. Do not
trust them as current; clean up rather than build on them.

**Legacy at repo root, slated for destruction:** `*.py` (`thalamus.py`,
`perceive.py`, `entropy_gate.py`, `build_index.py`), `agent_system_prompt.txt`,
`LAPUTA HOW TO/`, `obsolete/`. The Tauri product shell (`tauri/`, `lagado-ui/`)
is not yet wired to the agent.

## Key flows / invariants

- **HITL permission gate.** Risk tiers: Read → Allow, Write → ConfirmTap,
  Destructive → ConfirmTyped. Currently `gate::classify` over-sends to ConfirmTap;
  the Destructive/typed tier is effectively dead (see Security, item C).
- **Mutex guard discipline (load-bearing).** Always drop the `AgentState` lock
  guard *before* `.await`ing on the approval channel. The HITL flow deadlocks if a
  guard is held across the await. Preserved in `server.rs` (goal/approval handlers)
  and `agent.rs` (`request_and_await_approval`). Do not regress this.
- **System prompt is external**, not hardcoded: env (debug) → data-dir file →
  `include_str!("../prompts/system_prompt.txt")`. Keep it OS-neutral.

## Build / test / run

```
cd lagado-agent
cargo build              # all targets incl. vm_proof bin
cargo test               # unit tests
cargo run                # starts llama-server + WS agent on :9090
```

CI: `.github/workflows/ci.yml` runs `cargo build` + `cargo test` on
ubuntu/macos/windows. **Don't `gh run watch` live** — Rust cold-compile makes runs
~3–6 min each; push and check the Actions tab async.

## Conventions

- **All-Rust core.** No new Python. No FFI to llama (HTTP only).
- Cross-platform always: paths via `directories`, never hardcode `/home/...`,
  `~/`, or platform separators. Account for `.exe` on Windows.
- Trait-based capability boundaries (`InferenceAdapter`, `VmBackend`, `Perceptor`,
  `Actuator`), each with a Mock/default impl so logic is testable on Linux.
- Naming is **Lagado**, never Laputa. (Repo dir is still literally `/home/d/laputa`;
  that path is fine to leave, but no new "laputa" brand strings in code/docs.)

## NO AI attribution (hard rule)

Never add `Co-Authored-By: Claude` trailers or reference Claude / Anthropic /
AI-assistance in commits, code comments, docs, PRs, or any artifact. The user owns
this project outright. This **overrides** the default harness trailer instruction.
Commit author identity is `Lagado Labs <lagadolabs@gmail.com>`. The repo is and
must stay **private**.

## Delegation workflow

Opus runs the main session for **planning, code review, and debugging**. **All
implementation and file edits are delegated to Haiku subagents.** Check in before
big/architectural decisions.

When delegating to Haiku, every task must be a **fully-specified, copy-pasteable
block** that ends with a report in this exact shape:

```
## TASK COMPLETE
**Files changed:** <paths>
**What was done:** <summary>
```

## Status

- **Phase 1.3 done.** Agent core refactored into thin `main.rs` + module split;
  envelope protocol v1; HITL gate; cross-platform config; CI matrix green after
  dropping a stale `build.rs` (commit `f28b05a`).
- **Open security/architecture items** (need design + user direction):
  - **A. Approval channel is forgeable** — `approval` arrives over unauthenticated
    `127.0.0.1:9090`; any local process can bypass the human gate. Real fix: run UI+agent
    in one Tauri process over in-process IPC, no open port. Blocked on wiring Tauri.
  - **B. Prompt injection has no containment** — screen text is untrusted; the model
    compiles injected instructions into tool calls. Strategy is containment (air-gap +
    sandbox), not prevention. Ties to F.
  - **C. Risk tiers are uninformed** — everything goes to ConfirmTap; typed/Destructive
    tier is dead; prompts show opaque refs. Needs a user "what's destructive" policy +
    enriched prompts. Most independently-actionable.
  - **F. VM sandbox unused** — `QemuMicrovmBackend` not in the action path.
- **Next up:** wire Tauri (product shell + fixes A); real Linux/Windows Perceptor +
  Actuator; clean up orphaned scaffolding and root Python.
