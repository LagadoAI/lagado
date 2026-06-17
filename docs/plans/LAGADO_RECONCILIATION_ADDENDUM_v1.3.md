# LAGADO AI — RECONCILIATION ADDENDUM v1.3
**Date:** June 2, 2026 · **Appends to:** Record v1.1 + Addendum v1.2
**Status:** Captures the multi-agent permission design (north-star, contract-stable) and the
obsolete-file quarantine policy. Records live build progress through Block 3.

---

## A — BUILD PROGRESS (verified against live code)

**Phase 1.3 COMPLETE** (1.3.1–1.3.6 all green). Pipeline proven end-to-end: governor-spawned
server, bracket parsing, HITL gate with wait-before-execute, chronos timeline. Carried item:
goal-completion blocked on `perceive.py` staleness → resolved by D-6 native perception.

**Envelope + UI permission gate COMPLETE** (Blocks 1–3):
- **Block 1 (Rust):** versioned envelope `{v,kind,payload}` on both directions; `uuid` v4 added
  (approved exception — security-critical id matching); approvals id-matched, non-matching ignored.
- **Block 2 (TS):** `useAgentSocket` parses/emits envelopes; `ChatProvider` mounts socket once,
  holds `pendingPermission`, exposes `approve(id)`/`deny(id)`/`sendGoal`/`connState`.
- **Block 3 (TS):** `ChatDefault` migrated onto provider (single source of truth); shared
  `PermissionCard` (collapsed action+approve/deny / expanded reason+origin+switch button);
  rendered at ONE point in `ChatBox` → appears on every surface. Tap = direct approve;
  Typed = requires non-empty input to enable approve.

**FROZEN CONTRACT (do not churn):**
```
agent→UI:  {"v":1,"kind":"permission","payload":{id,type:"tap"|"typed",tool,action,reason,origin_surface,origin_agent}}
           {"v":1,"kind":"action_log","payload":{text}}
           {"v":1,"kind":"status","payload":{state,detail}}
UI→agent:  {"v":1,"kind":"goal","payload":{text}}
           {"v":1,"kind":"approval","payload":{id,approved}}
           {"v":1,"kind":"command","payload":{cmd:"pause"|"resume"|"stop"}}
```

---

## B — MULTI-AGENT PERMISSION SCALING **[NORTH-STAR / v2 — contract-stable]**

Today: single-agent. `pendingPermission` is one value; Rust `AgentState` has one `approval_tx` +
`pending_id`. Two concurrent agents would clobber each other. The frozen contract already carries
`id`, `origin_agent`, `origin_surface` — so multi-agent is a pure implementation swap, ZERO
protocol change. Build when v2 multi-agent (Immersive + IDE-triggered test) becomes real.

**Rust (`main.rs`):** replace shared `approval_tx`/`pending_id` with a session map:
```rust
sessions: HashMap<String, AgentSession>   // key = agent_id (origin_agent)
struct AgentSession { approval_tx: mpsc::Sender<bool>, pending_id: Option<String> }
```
Each `agent_loop` registers under its `origin_agent`. Approval routing: `id → owning session →
its approval_tx`. Today = one entry keyed `"main"`.

**TS provider (`use-chat-context`):** `pendingPermission: PermissionRequest | null` →
`pendingPermissions: Map<agentId, PermissionRequest>`. `approve(id)`/`deny(id)` unchanged (already
key by request id).

**TS render (`ChatBox`):** single render point becomes a surface selector:
```ts
const req = [...pendingPermissions.values()].find(p => p.origin_surface === currentSurface)
```
`PermissionCard` is UNCHANGED — takes one `req` at any scale. Only selection above it changes.

**Cross-surface attention (from the IDE scenario):** when an agent in Immersive needs permission
while user is in IDE, the card appears on the IDE surface (global), shows reason + origin, and the
[Switch to <surface>] button jumps focus. Pause (Immersive side-pane) is a per-agent HOLD, not
global. A separate GLOBAL stop halts all agents. Two scopes, one system.

---

## C — OBSOLETE-FILE QUARANTINE POLICY **[STANDING]**

Six versions of churn + the current rewrites leave dead files (the vestige register, plus
newly-bypassed code). Policy: **quarantine, never delete.** Obsolete files are MOVED into a
top-level `obsolete/` directory (preserving relative path under it) for user review and eventual
removal. Nothing is deleted by the coder.

Known obsolete candidates so far (coder must confirm each is truly unreferenced before moving):
- `parser.rs` — superseded by `bracket_parser.rs` (JSON path dead). **Verify no live `use`.**
- `thalamus.py` — legacy prompt routing (deprecate after hydra).
- `run_cortex.sh` — superseded by governor-spawned server + `start-llama-server.sh`.
- `start-llama-server.sh` — now superseded by the governor spawn in `main()`. **Verify.**
- `Architectural_Analysis.txt` reliance — keep file, but never cite "Origin Pilot" as supervisor
  prior art (it's a quantum OS; see Addendum v1.2 correction).
- `forge.rs` — NOT obsolete (restored as operating basis). Do NOT quarantine.

RULE: a file is moved to `obsolete/` ONLY if grep confirms zero live references. If referenced,
it stays and is reported, not moved.

---

## DECISION LEDGER (additions)

| ID | Decision | Status |
|---|---|---|
| Envelope contract | Versioned `{v,kind,payload}`, frozen | ✅ LIVE |
| uuid v4 | Approved exception for id matching | ✅ DONE |
| Multi-agent permissions | Session map / Map / surface selector | ⏳ v2 north-star (contract-stable) |
| Obsolete quarantine | Move to `obsolete/`, never delete, grep-gated | ✅ STANDING POLICY |

*— End addendum v1.3.*
