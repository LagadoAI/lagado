# Lagado AI

**A sovereign desktop agent that remembers. Runs entirely on your machine — no cloud, no telemetry, no egress.**

Lagado is a local-first AI agent built on one thesis: a small model inside the right support
structure beats a large model with none. An 8B-parameter "frontal cortex" is surrounded by the
faculties that make human work possible — persistent memory, procedural habit, recognition,
recovery, and a human in the loop — all running on consumer hardware, with every byte of the
user's data staying on the user's disk.

---

## The four pillars

1. **Maximum security & sovereignty** — local-only inference, AES-256-GCM encrypted storage,
   Argon2id-wrapped keys, zero telemetry. Nothing leaves the machine. This is not a feature
   flag; the architecture has no cloud path to disable.
2. **Dual-brain orchestration** — a fast 1.2B classifier routes every request on a clean,
   history-free prompt; the 8B mixture-of-experts brain is reserved for reasoning and
   synthesis. Small models stay in the single-pass regime they're good at.
3. **One integrated stack** — perception, actuation, memory, and security are one coherent
   system in a single binary, not a chain of loosely-coupled services.
4. **Persistent learning** — the agent's experience survives reboots: workflows become
   muscle memory, completed tasks distill into skills, and screens it has seen become
   recognition memory.

## Architecture at a glance

```
┌─────────────────────────────────────────────────────────────┐
│  Tauri desktop app (React UI + Rust agent core)             │
│                                                             │
│  1.2B intent classifier ──► routes on clean context         │
│  8B agent brain         ──► reasons, plans, acts            │
│  450M vision encoder    ──► visual memory (in-process FFI)  │
│                                                             │
│  Living memory triangle                                     │
│    hot (working) → warm (summarized) → cold (encrypted)     │
│    entropy-based forgetting: V = T·e^(−λt)·(1+ln(n+1))      │
│    + action graph (habit) + skill library (technique)       │
│    + visual embeddings (recognition)                        │
│                                                             │
│  QEMU desktop VM — the agent's sandboxed working surface    │
│    perceive: accessibility tree → elements with coordinates │
│    act:      mouse/keyboard, every action HITL-gated        │
│    verify:   frame deltas — did the action do what was      │
│              expected?                                      │
└─────────────────────────────────────────────────────────────┘
```

**The agent works inside a virtual machine, not on your desktop.** It perceives the VM's
screen through the accessibility tree fused with classical CV and vision-model patches,
acts through synthetic input, and every action passes a human-in-the-loop gate — routine
actions confirm with a tap, destructive ones require typed confirmation.

**Memory is modeled thermodynamically.** Every trace carries a temperature that decays with
time and reheats on access. A background consolidation cycle ("sleep gate") summarizes the
day's residue, promotes what matters, and lets the rest fade on an Ebbinghaus curve.
Completed tasks are distilled into a skill library; successful workflows become an action
graph the agent can replay without re-reasoning. The result is effective context bounded by
disk, not by a context window.

**Learning from demonstration (in design).** The most reliable teacher is the human. The
Lens — a separate recorder, deliberately not part of the agent binary — captures workflows
as the user demonstrates them, producing inspectable "workflow capsules" that import into
the agent's action graph. The trust model is strict: *the thing that watches cannot touch,
and the thing that touches cannot watch.*

## What works today

- Full desktop app: first-run ceremony, encrypted auth (wrapped-DEK scheme), chat with
  retrieval over the agent's own history
- Dual-model routing with clean-context classification
- Complete memory system: hot/warm/cold tiers, sleep-gate consolidation, entropy pruning,
  skill distillation, visual-similarity recall
- VM control loop, end-to-end and independently verified: boot → perceive elements with
  screen coordinates → resolve a target → click → observe the change
- Perception fusion: accessibility tree + CV box proposer + per-patch vision embeddings,
  deduplicated by an IoU arbiter
- 44 built-in native tools + MCP client; tiered trust with a human gate on every action
- 156 library tests; CI green on Linux, macOS, and Windows

## Honest status

This is a working foundation, not a finished product. The control loop is proven
mechanically; the current frontier is letting the 8B drive it on real multi-step tasks and
wiring the verification loop (act → observe delta → check expectation) into the agent's
own reasoning. Windows-native perception, the browser backend, and the Lens recorder are
designed but not yet built. We publish what works and what doesn't.

## Design rationale

Every major decision here was an argument we had on purpose. [DESIGN.md](DESIGN.md) records
the twelve that matter — each one as *the decision, why, and what it cost us*.

## Stack

Rust (agent core) · Tauri + React (app) · llama.cpp (vendored, local inference) ·
LFM2.5 model family (8B MoE / 1.2B / 450M vision) · QEMU/KVM (sandboxed working surface) ·
SQLite (memory, encrypted at rest)

---

*Lagado Labs — built local-first because the alternative is someone else's computer.*
