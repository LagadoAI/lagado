# PHASE 1.3 — Liquid-Native Agent Loop + HITL Gate + Governor-Spawned Server
**For:** the coder (Claude Haiku 4.5) in `/home/d/laputa`
**Prereq:** Step 0 (commit + back up the 35 changes) is DONE and the tree is clean.
**Report format:** end EVERY task with the `## TASK COMPLETE` block (see end of this doc).
**Golden rule:** do tasks in order. Run the checkpoint after each. If a checkpoint fails, STOP and
report — do not proceed to the next task.

---

## CONTEXT (read before starting)

`main.rs` today is the pre-Liquid-native version. This phase converts it to:
1. spawn the llama-server subprocess itself (governor-generated flags, CPU-safe),
2. emit **Liquid-native bracket tool calls** `[tool(arg="x")]` instead of OpenAI-style JSON,
3. route every tool through a single **`evaluate_action()` gate** with **HITL confirmation** before
   execution,
4. log a **chronos** stub entry per action,
5. fix the stale constants (model path, context size).

**Scope boundaries — do NOT in this phase:** the rename (separate atomic commit later), the full
resource governor (only the minimum spawn-config here), native perception/actuation (tine/perceive
stay as-is this phase), the selector grammar (Phase 1.10), supervisor.rs (Phase 3.5). Keep the loop
*shaped* so those drop in later, but don't build them now.

**Don't-touch files:** `forge.rs`, `parser.rs`, `verifier.rs`, `Cargo.lock` (cargo may regenerate
the lock only as a side effect of an approved dependency add).

---

## TASK 1.3.1 — Remove dead grammar include; fix stale constants

**File:** `main.rs`

1. Delete the line `const GRAMMAR: &str = include_str!("../grammar.gbnf");` (line ~50). The grammar
   file exists but is unused this phase; the unused const triggers a dead-code warning. (It returns
   in Phase 1.10.)
2. Change `MODEL_PATH` to the real LFM2.5 file:
   `const MODEL_PATH: &str = "/home/d/.laputa-secure/models/LFM2.5-8B-A1B-Q4_K_M.gguf";`
3. Change `const CONTEXT_SIZE: usize = 4096;` → `const CONTEXT_SIZE: usize = 32768;`

**Checkpoint 1.3.1:** `cargo check` → passes (no `grammar.gbnf` error, no unused-`GRAMMAR` warning).

---

## TASK 1.3.2 — Liquid-native bracket tool format (prompt + parser-side type)

Liquid models emit Pythonic bracket tool calls, not JSON. The system prompt and the `ToolCall`
parsing must match.

1. **Replace the `SYSTEM_PROMPT` constant** (lines ~23-48) with the bracket-format version below.
   Keep the perception format block; only the TOOLS/RULES/EXAMPLES change:

```
You are Lagado, a sovereign personal assistant acting on the user's Linux desktop on their behalf.
Your perception tool outputs the active window's interactive elements in this format:
[focused: Terminal - user@host:~]
[window: x=0 y=51 w=1280 h=749]
  ref_1  toggle button   "Applications"     state=has-tooltip
  ref_3  toggle button   "Xfce Terminal"    state=has-tooltip
  ref_5  entry           "Search"           state=editable

You act by emitting EXACTLY ONE tool call in Python bracket syntax. Available tools:
- click(selector="<ref_id>")
- type(selector="<ref_id>", text="<string>")
- key(key="<key>")
- wait(ms=<int>)
- done(reason="<short reason>")

RULES:
1. Use the exact ref_id from perception (e.g. "ref_3").
2. To open an app, click its toggle button.
3. To type, click the entry field FIRST, then type(...).
4. When the goal is complete, emit done(reason="...").

EXAMPLES:
Goal: Open Terminal
Screen:   ref_3  toggle button  "Xfce Terminal"  state=has-tooltip
Action: click(selector="ref_3")

Goal: Type 'hello' in the search box
Screen:   ref_5  entry  "Search"  state=editable
Action: type(selector="ref_5", text="hello")

Respond with ONLY one bracket tool call. No markdown, no explanation.
```

2. **`types.rs`** — the `ToolCall` enum keeps its variants but rename `Task` → `Done` with a `reason`
   field for clarity (`Done { reason: String }`). Update all match arms that reference
   `ToolCall::Task`. If `parser.rs` is the thing that builds `ToolCall` from model text and it is on
   the don't-touch list, then DO NOT edit it — instead STOP and report that the bracket parsing lives
   in a don't-touch file and needs a ruling. (Check first: does `parser.rs` parse the tool call, or
   does `forge.rs`/`main.rs`? Report what you find.)

**Checkpoint 1.3.2:** `cargo check` → passes. Report which file does the actual bracket/JSON parsing.

> NOTE: if parsing is in a don't-touch file, this task is BLOCKED — report and stop. We will adjust
> the don't-touch list before continuing. Do not edit a don't-touch file.

---

## TASK 1.3.3 — `evaluate_action()` gate + HITL confirmation

Insert a single decision chokepoint between the model's chosen tool and execution. This is the v1
seed of the supervisor's gate — keep the signature stable so richer logic drops in later.

1. **New module `gate.rs`** (add `mod gate;` to `main.rs`):

```rust
// gate.rs — single decision chokepoint for every agent action (v1 seed of supervisor).
use crate::types::ToolCall;

#[derive(Debug, Clone, PartialEq)]
pub enum RiskTier { Read, Write, Destructive }

#[derive(Debug, Clone)]
pub enum Verdict {
    Allow,                 // safe, auto-execute
    ConfirmTap,            // user taps to approve
    ConfirmTyped,          // user types to approve (destructive)
    Block(String),         // refused, with reason
}

pub fn classify(call: &ToolCall) -> RiskTier {
    match call {
        ToolCall::Wait { .. } | ToolCall::Done { .. } => RiskTier::Read,
        ToolCall::Click { .. } | ToolCall::Key { .. } | ToolCall::Type { .. } => RiskTier::Write,
        // destructive detection refined in later phases; nothing maps here yet
    }
}

/// v1: risk-tier → verdict. Later phases add reachability + capability mask here.
pub fn evaluate_action(call: &ToolCall) -> Verdict {
    match classify(call) {
        RiskTier::Read => Verdict::Allow,
        RiskTier::Write => Verdict::ConfirmTap,
        RiskTier::Destructive => Verdict::ConfirmTyped,
    }
}
```

2. **Wire it into `agent_loop`** in `main.rs`, immediately before the existing `execute_tool` call
   (~line 162). On a verdict requiring confirmation, the loop must PAUSE and emit a confirmation
   request over the WebSocket, then wait for an approve/deny reply before proceeding. For THIS phase,
   implement the gate + the WS message emission + the wait-for-reply; the UI side can be a stub that
   auto-approves in dev, but the **approval must round-trip through the WebSocket**, not be skipped.
   Add two inbound WS messages: `confirm:approve` and `confirm:deny`. On `Block`, skip execution and
   record the reason to memory (and chronos, Task 1.3.4).

   Confirmation pattern (sketch — adapt to the existing `AgentState`):
   - add `pending_confirmation: Option<ToolCall>` and an approval channel/notify to `AgentState`,
   - on `ConfirmTap`/`ConfirmTyped`: set pending, send WS `confirm_request:<verdict>:<tool debug>`,
     await the approval signal, then proceed or skip.

**Checkpoint 1.3.3:**
- `cargo check` → passes.
- Manual: start the binary, send a `goal:`, confirm in the log that **every Write action emits a
  `confirm_request` and waits** before the tool fires. Read-tier (`wait`/`done`) auto-allows.

---

## TASK 1.3.4 — chronos stub (autobiographical log)

Every action gets one timeline entry. Full chronos is a later phase; this is the append-only stub.

1. **New module `chronos.rs`** (add `mod chronos;`):

```rust
// chronos.rs — append-only autobiographical log (v1 stub).
use std::fs::OpenOptions;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn log(event: &str) {
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let line = format!("{ts}\t{event}\n");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true)
        .open(chronos_path()) {
        let _ = f.write_all(line.as_bytes());
    }
}

fn chronos_path() -> String {
    // TODO(rename/R-2): derive from $HOME/XDG, not hardcoded. Acceptable for this phase only.
    let home = std::env::var("HOME").unwrap_or_else(|_| "/home/d".into());
    format!("{home}/.laputa-secure/chronos.log")
}
```

   (Using `$HOME` here already partially addresses the hardcoded-path debt — good.)

2. **Call `chronos::log(...)`** in `agent_loop` at: goal start (`goal_received: <goal>`), each
   executed action (`action: <tool debug> -> <output>`), each gate block (`blocked: <reason>`), and
   goal end (`goal_done: <reason>` / `goal_aborted: <error>`).

**Checkpoint 1.3.4:** run a goal; confirm `~/.laputa-secure/chronos.log` exists and contains
timestamped `goal_received`, `action`, and `goal_done` lines.

---

## TASK 1.3.5 — Governor-spawned llama-server (CPU-safe minimum)

`main.rs` must start the llama-server subprocess itself with flags chosen from detected hardware —
NOT assume a GPU. This is the v1-minimum of the resource governor.

1. **New module `governor.rs`** (add `mod governor;`). Minimum viable detection + config:

```rust
// governor.rs — v1 minimum: detect substrate, emit a CPU-safe llama-server config.
pub struct ServerConfig {
    pub ctx: usize,
    pub n_gpu_layers: u32,   // 0 on CPU-only
    pub flash_attn: bool,    // false on CPU-only
    pub threads: usize,
    pub n_parallel: usize,
}

pub fn detect_and_plan(default_ctx: usize) -> ServerConfig {
    let has_gpu = detect_gpu();           // see below
    let phys = num_physical_cores();      // physical, not logical
    if has_gpu {
        ServerConfig { ctx: default_ctx, n_gpu_layers: 99, flash_attn: true,
                       threads: phys, n_parallel: 4 }
    } else {
        // CPU-only: no offload, no flash-attn, fewer parallel slots, scale ctx down if low RAM
        let ctx = if available_ram_gb() < 12 { 8192 } else { default_ctx.min(16384) };
        ServerConfig { ctx, n_gpu_layers: 0, flash_attn: false,
                       threads: phys, n_parallel: 2 }
    }
}
```

   Implement the helpers simply and safely (no new heavy deps if avoidable):
   - `detect_gpu()`: try running `nvidia-smi -L` (success + non-empty stdout ⇒ true); else check for
     `/dev/dri/renderD*` for non-NVIDIA. If detection errors, **default to false (CPU-safe).**
   - `num_physical_cores()`: `std::thread::available_parallelism()` is acceptable as a v1 proxy;
     note in a comment it counts logical cores (refine later).
   - `available_ram_gb()`: parse `/proc/meminfo` `MemAvailable`. On failure, return a conservative
     small number so we pick the safe profile.
   - If you need a crate for any of this, prefer **none**; if unavoidable, propose it in the report
     and do NOT add it without approval.

2. **Spawn the server in `main()` before binding the adapter.** Replace the assumption that an
   external `start-llama-server.sh` is already running. Build the command from `ServerConfig`:
   `llama-server -m <MODEL_PATH> -c <ctx> -ngl <n_gpu_layers> [-fa] -t <threads> --parallel <n_parallel> --host 127.0.0.1 --port 8080`
   (omit `-fa` when `flash_attn` is false). Spawn it as a child process, then **poll
   `http://127.0.0.1:8080/health` until ready (timeout ~60s)** before constructing `LlamaCppAdapter`.
   Keep the child handle so it isn't dropped/killed. Log the chosen config via `chronos::log` and
   stdout.

3. Log the selected profile to chronos: `server_config: gpu=<bool> ctx=<n> ngl=<n> threads=<n>`.

**Checkpoint 1.3.5:**
- `cargo check` → passes.
- Manual on this (GPU) box: binary spawns llama-server with `-ngl 99 -fa`, `/health` goes ready,
  agent runs. Report the exact spawned command line.
- Reason about the CPU path: with GPU absent, confirm by reading the code that `n_gpu_layers=0` and
  `-fa` is omitted. (No CPU box needed now — just verify the branch is correct.)

---

## TASK 1.3.6 — Full smoke test

1. Build release: `cargo build` → passes.
2. Run the binary. Send `goal: open the terminal` over the WebSocket (use the existing UI or a
   `websocat` one-liner — report which).
3. Confirm the full loop: server spawned → perception read → bracket tool emitted → parsed →
   `evaluate_action` verdict → confirm round-trip → tool executed → chronos logged → loop continues →
   `done(...)` ends cleanly.

**Checkpoint 1.3.6 (Phase 1.3 PASS criteria):**
- Server is spawned by the binary (not by hand), CPU-safe branch verified by inspection.
- Model emits **bracket** tool calls that parse.
- **No Write action executes without a WS confirm round-trip.**
- `chronos.log` shows the full action timeline.
- A simple goal completes end to end.

Report the full `## TASK COMPLETE` block for 1.3.6 with the smoke-test transcript.

---

## REQUIRED REPORT FORMAT (after each task)

```
## TASK COMPLETE: [e.g. 1.3.3]
**Files changed:** [paths]
**What was done:** [2-3 lines]
**Verification run:** [command + result, e.g. "cargo check → passes"]
**Blockers/deviations:** [anything off-spec, or "none"]
**Ready for:** [next task per spec]
```

If any checkpoint fails or a don't-touch file is in the way, set **Blockers** and STOP. Do not
continue to the next task.
