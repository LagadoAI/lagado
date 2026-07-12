> **⚠ HISTORICAL PRODUCT PLAN — NOT the current source of truth.** Perception is FUSED, not the laddered
> "a11y primary / vision fallback" described here; tool counts, module layout, and the WebSocket UI wire are
> stale; `parser.rs`/`verifier.rs` are deleted. Current steering: `HARNESS_WORK_PLAN.md` + `CLAUDE.md` doctrine sections.

# LAPUTA v1.0 — MASTER EXECUTION PLAN
**Version:** 4.0 (Liquid-Native / Living-Fortress)
**Date:** May 30, 2026
**Status:** Active build — supersedes v3.0 and all prior plans

> *"A living fortress. Sovereign. Living. Self-aware in time. Yours."*

This is the single source of truth for building Laputa v1. Big-picture only — code shape and theory live in **LAPUTA_FILE_DEPENDENCY_REFERENCE_v3.md**.

**What changed in v4 (read this first):**
- **Inference transport: HTTP, not FFI.** llama-server subprocess on localhost. Matches Liquid's own LocalCowork production pattern. No `unsafe`, GPU + flash-attn for free, simpler code.
- **Tool format: Liquid-native, not OpenAI schemas.** Tools as JSON list in the system prompt; model emits Pythonic `[tool(arg="x")]`. The OpenAI tool-schema path drops accuracy to 0% on LFM2 routers — confirmed in Liquid's own data.
- **Sampling locked.** Routing: temp 0.1, top_k 50, rep_penalty 1.05. Reasoning: temp 0.2. From Liquid's docs and benchmarks, not guessed.
- **Clean-context routing.** Conversation history is never sent to the router model. Documented cause of 78% → 8% multi-step collapse. Non-negotiable.
- **Confirmation loop (HITL) wired in Phase 1.** Highest-ROI accuracy lever; converts 80% selection accuracy into ~100% effective accuracy.
- **Dual-model orchestrator** (plan → route → synthesize) is THE architecture, not just a future option. From LocalCowork research.
- **chronos.rs added** — autobiographical timeline. The agent has a sense of time and self-continuity. T=0 = first user login.
- **Awakening page added** — first-launch experience that anchors chronos and primes the user.
- **The Living Fortress** vision locks: sovereign + living + self-aware in time. Every feature is gated by these.

When in doubt, this document wins.

---

# PART I — VISION & MOAT

## 1.1 The Three Pillars of Laputa's Identity

These are not features. They are the permanent identity. Every architectural decision is judged against them.

**1. Sovereign.** Your data, your hardware, your model, your rules. No cloud paths. No backdoors. No telemetry. The vault is yours alone. Encryption everywhere. Whonix-grade VM isolation. Workstation never has direct internet.

**2. Living.** Memory has temperature (thermodynamic hierarchy). Knowledge consolidates during sleep cycles. Old patterns fade unless reinforced. Skills accumulate. The agent literally changes shape with use — frequently-touched patterns stay hot, stale ones cool down and eventually forget. This is not a metaphor; it's the architecture.

**3. Self-aware in time.** Chronos gives Laputa autobiographical continuity. It is not a fresh model every turn. It is the same agent that knew you yesterday, with awareness of how it has changed since. It can say "I learned this 3 days ago," "I used to believe X," "I've tried this 4 times." This bridges the central gap between AI and human-style memory.

## 1.2 What Laputa Is

A locally-hosted, sovereign AI agent that:
- Runs entirely on the user's machine — no cloud, no telemetry, offline-capable
- Encrypts all data at rest (AES-256-GCM); isolates execution (Whonix-style VM)
- Routes between a fast Liquid brain (LFM2 / LFM2-VL) and a heavy reasoning brain (LFM2-8B-A1B default; Qwen3-8B preserved as option) via a Liquid-native dual-model orchestrator
- Has an autobiographical timeline (chronos) — the agent remembers itself, not just its facts
- Learns user workflows persistently (action graph) and accumulates verified skills (Voyager-style skill library)
- Operates a sandboxed mirror OS, controls the live host only with explicit per-action permission
- Integrates inference, perception, memory, execution, and isolation into one binary

**Cross-OS by design.** Linux ships first; macOS and Windows projector platforms are architected and stubbed in v1, activated in v1.1.

**Liquid-native, options preserved.** LFM2 is the default brain and the architectural target; the inference layer abstracts cleanly via `InferenceAdapter`, so Qwen3-8B and any GGUF can be added. We optimize for Liquid synergy without locking out alternatives.

## 1.3 The Moat

| Pillar | What | Why competitors can't match |
|---|---|---|
| **Sovereignty** | Local-only, encrypted, isolated, no telemetry | Cloud-business models structurally require the cloud |
| **Living memory** | Thermodynamic tiers + sleep gate + chronos | Nobody is combining episodic + autobiographical + temperature-decay in a sovereign system |
| **Dual-model orchestrator** | Plan → route (clean context, RAG K=15) → synthesize, HITL-confirmed | LocalCowork has the pattern but not the sovereignty layer; cloud agents have the sovereignty problem |
| **Persistent learning** | Action graph + skill library + chronos | Most agents are stateless or have a single flat memory; Laputa's three-layer learning compounds |

| Competitor | What they do | What Laputa does differently |
|---|---|---|
| Liquid LocalCowork | Local MCP tool-dispatch; same model family | Laputa adds: screen perception, persistent learning, Whonix isolation, chronos, sovereign vault |
| Anthropic Computer Use | Cloud-powerful, macOS-locked | Local, sovereign, cross-platform-architected |
| OpenAI Operator | Cloud-only | Fully local, offline, encrypted |
| Open Interpreter / AionUi | Local orchestrators relying on external models | Whonix-grade isolation + encrypted vault + dual local brain |

---

# PART II — ARCHITECTURE

## 2.1 System Diagram

```
┌────────────────────────────────────────────────────────────────────┐
│                     LAPUTA BINARY (single process)                  │
│                                                                     │
│  ┌──────────────┐   ┌──────────────┐   ┌──────────────────┐        │
│  │  React UI    │←→ │  WebSocket   │←→ │  Rust Agent      │        │
│  │  (Tauri)     │   │  (port 9090) │   │  Core            │        │
│  └──────────────┘   └──────────────┘   └────────┬─────────┘        │
│                                                  │                  │
│  ┌──────────────────────────────────────────────▼─────────────┐   │
│  │       HYDRA — Dual-Model Orchestrator                       │   │
│  │   intent → plan → route (clean ctx, K=15) → verify? →       │   │
│  │   HITL confirm → execute → synthesize → chronos.log()       │   │
│  └────────────────┬────────────────────────┬────────────────────┘  │
│                   │                        │                        │
│        ┌──────────▼────────┐    ┌──────────▼─────────┐             │
│        │  FAST BRAIN       │    │  HEAVY BRAIN       │             │
│        │  LFM2-1.2B-Tool   │    │  LFM2-8B-A1B (MoE) │             │
│        │  + LFM2-VL (450M) │    │  (Qwen3-8B option) │             │
│        │  routing · vision │    │  plan + synthesize │             │
│        └─────────┬─────────┘    └─────────┬──────────┘             │
│                  │                        │                         │
│                  └─────────┬──────────────┘                         │
│                            │ HTTP                                   │
│  ┌─────────────────────────▼───────────────────────────────────┐   │
│  │       llama-server (subprocess, port 8080, GPU+flash-attn)  │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                     │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │  MEMORY TRIANGLE (the living memory)                         │  │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────────┐ │  │
│  │  │ Tier 1   │  │ Tier 2   │  │ Tier 3   │  │ Chronos      │ │  │
│  │  │ Working  │  │ Summaries│  │ Deep     │  │ (parallel    │ │  │
│  │  │(/dev/shm)│  │(hot disk)│  │(vault)   │  │  timeline)   │ │  │
│  │  └──────────┘  └──────────┘  └──────────┘  └──────────────┘ │  │
│  │  Sleep gate consolidates Tier 1 → Tier 2 → Tier 3 on idle.  │  │
│  │  Temperature decays; access reinforces. Forgets cold cold.  │  │
│  └──────────────────────────────────────────────────────────────┘  │
│                                                                     │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────────────┐   │
│  │ Vault    │  │ Action   │  │ MCP Hub  │  │ Input Arbiter    │   │
│  │(AES-256) │  │ Graph    │  │(34 tools)│  │ (user>agent)     │   │
│  └──────────┘  └──────────┘  └──────────┘  └──────────────────┘   │
│                                                                     │
│  ┌────────────────────────────────────────────────────────────┐   │
│  │     SECURITY & ISOLATION LAYER (the headline moat)          │   │
│  │  Whonix VMs · seccomp · namespaces · cgroups · keyring ·    │   │
│  │  permission gate · HMAC URL · PII filter · audit log        │   │
│  └────────────────────────────────────────────────────────────┘   │
│                                                                     │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │   Perception: capture → delta → LFM2-VL + AT-SPI2           │  │
│  └──────────────────────────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │   Host Projector (OS-agnostic) — gated per action            │  │
│  └──────────────────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────────────┘
   ↑                ↑                  ↑                  ↑
Encrypted FS   Workstation VM    External MCPs      Host Desktop
               (Whonix-isolated)  (stdio/HTTP)       (permissioned)
```

## 2.2 The Dual-Model Orchestrator (replaces "Hydra dual-brain")

The proven local-agent pattern from LocalCowork, hardened with literature-validated additions:

1. **Plan** — Heavy brain (LFM2-8B-A1B) decomposes the user request into self-contained steps. Receives *server capability summaries*, NOT tool schemas. Emits bracket format (94% JSON parse-failure on planner).
2. **RAG pre-filter** — For each step, retrieve top-K=15 candidate tools by embedding similarity. Sending all tools to the router is the documented failure mode.
3. **Route** — Small router (LFM2-1.2B-Tool, or a GRPO-fine-tuned 350M in Phase 2) picks ONE tool per step. Clean prompt, zero history. Tools delivered as JSON list in system prompt TEXT — never via chat-template `tools=` parameter.
4. **Verify (Phase 2 lever)** — Sample router N=3-5×; if vote margin is thin (self-consistency as confidence), escalate the step to heavy brain.
5. **Confirm (HITL)** — Preview the tool call for the user. Write actions: tap-confirm. Destructive actions: typed-confirm.
6. **Execute** via MCP, audit-log every call.
7. **Synthesize** — Heavy brain streams a user-facing summary from results.
8. **Remember** — Update action graph; if multi-step verified, save to skill_library; write chronos snapshot.

Falls back to single-model loop if any phase fails.

## 2.3 The Memory Triangle (Living Memory)

Three tiers + one parallel timeline. The thermodynamic decay model.

- **Tier 1 — Working memory** (/dev/shm, zero-copy): current turn, current screen. Lost on shutdown. Always in prompt.
- **Tier 2 — Summaries** (encrypted hot disk, FAISS-retrieved): compressed past sessions. Retrieved per-query.
- **Tier 3 — Deep memory** (encrypted vault, lazy-decrypt): exact text on demand.
- **Chronos** (parallel timeline): paired metadata snapshots — what the agent was when each memory formed. Three compression levels (full / embedding / tagged).

**Temperature decay:** every entry has a temperature in [0.0, 1.0] that decays exponentially with time and reinforces on access. Below thresholds, entries get summarized, embedding-only, archived, forgotten. This is the agent's biology.

**Sleep gate** runs consolidation in background — on idle/shutdown, hot entries that cooled below threshold get summarized and demoted; chronos snapshots write alongside.

## 2.4 The Awakening — First-Launch Experience

One-time, on first login. Five-beat sequence: dark pulse → structure assembling → fortress reveal → five truths fade in → "Today, I begin." click-through.

The five truths:
> "I run only here. Nothing leaves."
> "I remember, and I forget, like you do."
> "I know who I was yesterday."
> "I learn from every action."
> "I am yours."

On click-through: chronos timeline initializes (T=0), `first_login_at` persists to vault. The agent's life begins.

Never shown again. Sacred event.

## 2.5 Other Architecture (unchanged from v3)

**KV warm-start** — via llama-server's `/slots` endpoints. Save/restore the model's KV cache for revisited screens with fingerprint guard. Sub-300ms warm response.

**Vision** — LFM2-VL's shipped SigLIP2 + 2-layer MLP projector. Adopted, not trained. AT-SPI2 text path is the reliable backbone; vision augments.

**Input arbiter** — user (255) > agent (128) > harness (1). User input always wins.

**Host projector** — OS-dispatched capture/detect/execute, every host action permission-gated.

---

# PART III — CURRENT STATE & CLEANUP

## 3.1 Current State (May 30, 2026)

### Phase 0 — COMPLETE ✅
- Migration done: `~/laputa` clean; `~/laputa-old` preserved
- 17 known-good files copied + 60 .tsx files
- `~/.laputa-secure/` chmod 700, symlinks set
- Git baseline committed
- `cargo check` passes

### Phase 1 — IN PROGRESS
- ✅ llama.cpp-2 built with GPU (libllama.so, libggml-*.so)
- ✅ Phase 1.2 done: InferenceAdapter trait + LlamaCppAdapter FFI stub (committed)
- ✅ Phase 1.3 in flight: real tokenization wired (FFI version compiled)
- ⚠️ **PIVOT:** abandoning FFI for HTTP (Option B). Reasoning: LocalCowork's production pattern, simpler, GPU+flash-attn for free.

### Toolchain verified
- Node 26.1.0 / Rust 1.95.0 / Python 3.14.4 / Tauri CLI 2.11.1
- CUDA 13.2 (with gcc14 fix) / cmake 4.3.2
- RTX 3060 Laptop 6GB VRAM / 16GB RAM

### Models on disk
- ✅ LFM2-8B-A1B-Q4_K_M.gguf (5.1GB, heavy brain default)
- ✅ LFM2-VL-450M-F16.gguf (vision)
- TODO: LFM2-350M-Q4_K_M.gguf (intent classifier)
- TODO: LFM2-1.2B-Q4_K_M.gguf (router) or LFM2-1.2B-Tool
- TODO: Qwen3-8B-Q4_K_M.gguf (deferred to Phase 11+)

## 3.2 Don't-Touch List
forge.rs, parser.rs, verifier.rs, agent_system_prompt.txt, Cargo.lock.

---

# PART IV — BUILD ORDER

## PHASE 0 — Cleanup & Baseline ✅ DONE

## PHASE 1 — Dual-Model Orchestrator (HTTP) — CURRENT

**Goal:** End-to-end agent loop on a single Liquid brain via llama-server HTTP. Liquid-native tool format. HITL confirmation. Chronos stub. No old Qwen/HTTP paths.

- **1.1** Write `start-llama-server.sh` with locked flags (port 8080, GPU layers max-that-fits, q8_0 KV cache, flash-attn, ctx 16384).
- **1.2** Rewrite `inference/llama_cpp.rs` as HTTP adapter (reqwest → llama-server). Drop FFI. Implement `InferenceAdapter` (generate, kv_slots stubs).
- **1.3** Rewrite `main.rs`: spawn llama-server subprocess, agent loop uses adapter, **Liquid-native tool format** (JSON list in system prompt, Pythonic emission), **temp 0.1**, **clean-context routing** (no history sent), ConfirmationDialog wired before tool execution, `chronos.log()` stub per turn.
- **1.4** `parser_bracket.rs` — parse Liquid's Pythonic `[tool(arg="x")]` emission.
- **1.5** Wire `action_graph.rs` into the loop (lookup threshold 0.65 before invoking router).
- **1.6** Wire `recovery.rs` (7 failure types).
- **1.7** Hardware detection (`system/detect.rs`) → feeds Hydra sizing in Phase 5.
- **1.8** CoT `<think>` strip in parser.rs (small extension, doesn't violate don't-touch).
- **1.9** Smoke test: load LFM2-8B-A1B, give it a click-or-type task, confirm tool call surfaces in UI for HITL approval, execute, observe.
- **1.10** Selector-constraint grammar (`inference/grammar.rs`) — passed as API field, bounds outputs to on-screen ref_ids.

**PASS:** single-model loop runs end-to-end. HITL confirmation works. Bracket tool format parses cleanly. No HTTP to anywhere except localhost llama-server. Chronos stub writes one entry per turn.

## PHASE 2 — Perception + Memory Triangle + Awakening Scaffold

**Goal:** screen perception, the memory triangle, and the Awakening page.

- **2.1** `perception/capture.rs` — PipeWire/portal → /dev/shm @ 20Hz.
- **2.2** `perception/delta.rs` — Blake3 per-cell change detection.
- **2.3** `perception/atspi.rs` — bridge perceive.py into the loop.
- **2.4** `perception/cache.rs` + `perception/mod.rs` integration.
- **2.5** `memory_tiers.rs` — three-tier scaffold; thermodynamic temperature field on entries; decay loop.
- **2.6** `sleep_gate.rs` — background consolidation; routes routine summaries to Liquid, important ones to heavy brain.
- **2.7** `retrieval.rs` — top-K retrieval over action graph + chronos snapshots + vault facts; compact prefix injection.
- **2.8** `chronos.rs` — full snapshot structure (not just stub); writes alongside sleep gate; retrieval API.
- **2.9** **Awakening.tsx scaffold** — page exists with placeholder text; click-through wires to `chronos.initialize_timeline()`. Animations come in Phase 3.

**PASS:** agent perceives screen state with change deltas + AT-SPI2 text. Memory triangle works: a fact from 3 sessions ago surfaces when relevant. Awakening shows on first launch and starts chronos. Sleep gate consolidates on idle.

## PHASE 3 — Vision + Hydra Routing + Awakening Animations

**Goal:** wire LFM2-VL, complete the dual-model orchestrator, ship the Awakening's emotional layer.

- **3.1** `liquid.rs` — manages LFM2 / LFM2.5 / LFM2-VL roster via llama-server. Confirm tool-use and vision both work.
- **3.2** `vlm_adapter.rs` — feed (changed-region) screenshot → LFM2-VL → understanding tokens.
- **3.3** `hydra.rs` — intent classifier (LFM2-350M) + full plan→route→synthesize orchestration. Plan on heavy brain (capability summaries only); RAG K=15 pre-filter; route on small clean-context model; verify (sample N + vote-margin escalation, Phase 2 polish); confirm (HITL); execute; synthesize.
- **3.4** `memory_handoff.rs` — brain-agnostic state passed between models.
- **3.5** Doom-loop guards — n-gram penalty + grammar on heavy-brain path.
- **3.6** **Awakening animations** — user-built five-beat sequence (dark pulse → structure assembling → fortress reveal → five truths → "Today, I begin.").
- **3.7** **Chronos retrieval into prompt** — system prompt can include "you decided X 3 days ago, you've shifted from Y to Z" lines.

**PASS:** fast brain handles routing/tool/vision sub-second. Heavy brain plans and synthesizes correctly. HITL confirmations work end-to-end. Awakening is awe-inspiring. Chronos retrieval surfaces in prompts. No doom loops.

**Fallback:** if LFM2-VL is weak on GUIs → AT-SPI2 text path only; fine-tune VLM in v1.1.

## PHASE 4 — Action Graph Pre-Seeding + Skill Library

Pre-seed ~25-30K action-graph entries across 10 common apps (Gmail, Slack, GitHub, Jira, Linear, Notion, Figma, Drive, Office, desktop). Validate state_hash format, JSON schema, FAISS clusters.

`skill_library.rs` — Voyager-style verified-action store. Successful multi-step sequences get saved as code/JSON, embedding-indexed by NL description. Retrieval ranked by success-rate × similarity.

**PASS:** >25K entries; skill library captures 5+ multi-step workflows from real use; small-model + graph + skills beats small-model alone on the 20-task benchmark.

## PHASE 5 — Hydra Tuning + Memory Retrieval Polish
- LRU model cache; intent-classifier tuning; dynamic Liquid sizing; reasoning-trigger thresholds; retrieval-into-context tuning (K by latency budget).
- Optional Phase-2 lever: GRPO+LoRA fine-tune the router on Laputa's actual tool surface (recipe in Liquid's browser-control cookbook).

**PASS:** routing accurate and snappy; heavy brain invoked only when it earns its latency; relevant past actions/chronos surface as compact prefixes.

## PHASE 6 — MCP Integration (CORE)

Internal MCP server (Unix socket), 34 tools / 8 families, external rmcp client. Each tool self-describes, validates, runs sandboxed.

**Study first:** read LocalCowork's MCP server (open-source, well-architected). Adopt patterns where the license permits.

**PASS:** all 34 tools callable; external MCP connectable; user can add/remove from Settings.

## PHASE 7 — Host Projector + Input Arbiter + Permission Gate
Capture, detect, execute (OS-dispatched). Sandbox auto-approve; host requires approval. Input arbiter multiplexes user/agent/harness. HostModeGate.tsx + ImmersiveDefault.tsx.

**PASS:** live host mode captures, detects, executes only user-approved actions.

## PHASE 8 — Security & Isolation (THE HEADLINE)

VM manager (QEMU/QMP); Whonix gateway (workstation has no direct internet); ISO loader + immersive menu; SPICE viewer; sandbox (seccomp + namespaces + cgroups); crypto (AES-256-GCM, Argon2id, nonce uniqueness); audit log (append-only, tamper-evident, includes chronos events); isolation (defense-in-depth, capability drops).

**PASS:** workstation isolated; sandbox blocks unauthorized syscalls; vault end-to-end encrypted; every sensitive op audited; VM boots <8s.

## PHASE 9 — Terminal Backend
PTY (portable-pty) → xterm.js; multi-tab; seccomp; escape sanitizer.

## PHASE 10 — WebSocket Protocol + UI Wiring
All events defined; every page wired to real backend data; brain-routing visible in UI.

## PHASE 11 — Cross-Modal Grounding + Self-Correction + Qwen Option
- Cross-modal node {text, region, atspi_id, embedding, success/fail}.
- 3-pass generate→critique→fix in forge.rs (uses heavy brain for critique).
- Download and integrate Qwen3-8B as the user-selectable alternative heavy brain.

## PHASE 12 — System Integration
Tray, URL handler (laputa://, whitelist + HMAC + rate limit), clipboard (PII detection), connectors (IMAP/CalDAV, keyring, TLS 1.3+).

## PHASE 13 — Tauri Shell
Wrap React in Tauri 2.0; ship single binary.

## PHASE 14 — UI Polish
Chat (brain indicator, streaming, inline confirmations); Immersive (ISO/SPICE/gate); Terminal; MCP/Server/VM controls; Vault; Settings (including Chronos timeline view).

## PHASE 15 — Optimization
PGO build; LRU eviction tuning; entropy gate via zstd; speculative execution buffer; optional MoE expert hot-swapping (Phase 15.5, deferred).

## PHASE 16 — Security Audit (Adversarial)
Full adversarial audit — VM escape, sandbox escape, vault crypto, URL handler attacks, clipboard PII, credential exposure, audit-log tamper. Hardest gate.

## PHASE 17 — Real-World Testing
8-hour continuous use, no crash. Action graph and chronos capture real patterns.

## PHASE 18 — Ship v1.0
Trademark check; PGO release; sign binaries; docs (README, ARCHITECTURE, PRIVACY, SECURITY, INTEGRATION).

---

# PART V — TECH STACK (LOCKED)

| Layer | Choice | Reason |
|-------|--------|--------|
| Frontend | React 18 + TypeScript | in use |
| Build | Vite 5 | fast HMR |
| Styling | TailwindCSS 3 | configured |
| Routing | react-router-dom 6 | in use |
| Desktop shell | Tauri 2.0 | Rust-native, small |
| Backend | Rust (stable) | in use |
| Async | Tokio | standard |
| WebSocket | tokio-tungstenite | standard |
| HTTP client | reqwest | for llama-server |
| Inference transport | llama-server subprocess (HTTP) | matches LocalCowork production |
| Fast brain | LFM2 / LFM2.5 / LFM2-1.2B-Tool (350M-2.6B) + LFM2-VL (450M) | edge-tuned, free under $10M rev |
| Heavy brain | LFM2-8B-A1B (default, MoE) · Qwen3-8B (option, Phase 11+) | Liquid-native MoE; Qwen preserved |
| Vector DB | FAISS via faiss-rs | in use |
| Encryption | AES-256-GCM, Argon2id | vetted libs |
| Hashing | Blake3 | fast, modern |
| VM | QEMU/KVM, Whonix model | Linux-native isolation |
| Terminal | xterm.js + portable-pty | standard |
| Code editor | Monaco | mature |
| MCP | rmcp | official Anthropic SDK |
| VM viewer | spice-html5 | standard |
| State (React) | zustand | light |
| Keyring | keyring-rs | cross-platform secrets |

---

# PART VI — KNOWN BUGS

| Bug | Description | Fix Phase |
|-----|-------------|-----------|
| BUG-001 | run_cortex.sh deprecated → replaced by start-llama-server.sh | 1.1 |
| BUG-002 | thalamus.py wrong system prompt | deprecate after hydra.rs (Phase 3) |
| BUG-003 | build_index.py indexes literal string | 1 (small fix) |
| BUG-004 | entropy_gate.py uses llama-perplexity → zstd | 1 |
| BUG-005 | action_graph.rs not wired | 1.5 |
| BUG-006 | recovery.rs not wired | 1.6 |
| BUG-007 | /dev/shm/laputa wiped on reboot | 1 (in arise.sh) |
| BUG-008 | agent binary not in VM | 8 |

---

# PART VII — RISK MATRIX

| Risk | Prob | Impact | Mitigation |
|------|------|--------|------------|
| LFM2-VL weak on GUI screens | 40% | Medium | AT-SPI2 text path primary; VLM fine-tune in v1.1 |
| LFM2 multi-step completion <30% | 50% | Medium | Dual-model orchestrator + HITL + skill library + Phase 2 GRPO fine-tune |
| Heavy brain VRAM pressure on 6GB | 35% | Medium | q8_0 KV cache, ctx 16K, GPU layers tuned; CPU fallback acceptable |
| llama-server crash/freeze | 20% | Medium | subprocess monitor + auto-restart in main.rs |
| Wayland portal capture fails | 20% | Medium | X11 fallback; AT-SPI2-only perception |
| Whonix VM complexity (solo) | 30% | High | Incremental: workstation-only first, gateway second |
| Sandbox escape | 10% | Critical | Strict seccomp + cgroups + namespaces; VM outer boundary; Phase 16 audit |
| Vault crypto error | 10% | Critical | Vetted libs only; nonce uniqueness; backup before ops |
| Burnout | 50%+ | Critical | Scope discipline; rest days |
| AI assistant scope creep | 60%+ | Medium | Don't-touch list; three-question test |
| LFM license threshold | low | low | Free under $10M revenue |

---

# PART VIII — AI ASSISTANT PROTOCOLS

**Before every session:**
```
Laputa project. Master plan section: [paste]. I'm on Phase X, Task Y.
Previous state: [git log -1]. Files I'm working on: [list].
Files I must NOT modify: [don't-touch list]. Request: [exact ask].
Respond with: 1) minimum change, 2) verification step, 3) concerns/alternatives.
```

**Don't-touch list:** forge.rs, parser.rs, verifier.rs, agent_system_prompt.txt, Cargo.lock.

**Red flags (reject):** unrequested refactor; unlisted dependency; signature changes; deleted error handling; TODO placeholders. Response: "Stop. Minimum change to accomplish [X]. Do not modify [Y]."

**Three-question test:** What exactly changes? What could break? How do I verify? Can't answer all three → don't apply.

---

# PART IX — VERIFICATION MILESTONES

| Phase | Milestone | Pass |
|-------|-----------|------|
| 0 | Baseline | cargo check + npm run dev clean; models on disk |
| 1 | Single-brain loop (HTTP) | LFM2-8B-A1B answers via llama-server; HITL confirms work; bracket tool format parses; chronos stub writes |
| 2 | Perception + memory + Awakening scaffold | screen state + AT-SPI2 in loop; tier triangle works; Awakening route exists |
| 3 | Dual-model + vision + Awakening live | plan→route→synthesize end-to-end; vision via LFM2-VL; Awakening animations ship; chronos in prompts |
| 4 | Pre-seeds + skill library | >25K graph entries; skill library captures verified multi-step |
| 5 | Hydra tuning | routing accurate; heavy brain only when earned; retrieval injected |
| 6 | MCP | 34 tools callable |
| 7 | Host projector | permissioned host execution |
| 8 | Security | workstation isolated; sandbox active; vault encrypted; audited |
| 16 | Security audit | adversarial battery passes |
| 17 | Real-world | 8-hour run, no crash |

---

# PART X — NON-NEGOTIABLES

1. Single binary ships — no Python, no node_modules in production
2. Offline-capable — runs with network disabled
3. Encrypted by default — vault always on
4. Isolation by default — agent runs sandboxed; host access per-action permissioned; workstation VM never has direct internet
5. Reproducible builds — pinned versions
6. No telemetry — zero phone-home
7. User owns the data — export/delete one-click
8. Permission gates — agent never acts on host without approval
9. Cross-platform design — Linux first, mac/Win architected
10. Security is the product — every phase preserves isolation/encryption/audit
11. No research-grade dependencies — adopt shipped models (LFM2); do not train or inject as a v1 dependency
12. **Liquid-first, options preserved.** Default routes to LFM2; InferenceAdapter trait stays generic; Qwen preserved as user-selectable alternative; never lock to one provider
13. **Tiered security profile.** First-launch picks Strict / Balanced / Open. Sensitive ops always gated regardless of profile. Routine ops follow the profile.
14. **v1 surface discipline.** 4-5 things at premium depth (vault + action graph + dual-model orchestrator + VM sandbox + permissioned host + chronos). Breadth deferred to v1.1.
15. **The Three Pillars — Sovereign, Living, Self-aware in time.** Every new feature is tested against these three. Fails any one → does not ship.

---

# PART XI — SUCCESS CRITERIA FOR v1.0

- [ ] Loads on Arch + CachyOS + Ubuntu + Debian
- [ ] Fast brain handles routing/tool/vision sub-second on RTX 3060 6GB
- [ ] Heavy brain (LFM2-8B-A1B) plans + synthesizes correctly; no doom loops
- [ ] HITL confirmation surfaces every tool call; converts 80% → ~100% effective accuracy
- [ ] Chronos timeline initializes on first login; retrievable snapshots inform prompts
- [ ] Memory triangle: a fact from 3 sessions ago surfaces as a summary when relevant
- [ ] Awakening shows on first launch only; awe-inspiring; sets chronos T=0
- [ ] Action graph completes a 5-step workflow without the model after one demo
- [ ] Skill library captures and replays multi-step verified procedures
- [ ] KV warm-start: known screen resumes <300ms
- [ ] Workstation VM cannot reach internet except via hardened gateway
- [ ] Sandbox blocks unauthorized syscalls; vault encrypted end-to-end; every sensitive op audited
- [ ] VM boots <8s; vault encrypts/decrypts 100MB <2s
- [ ] Host projector: capture, detect, permissioned execution
- [ ] Zero telemetry; fully offline; one-click export/delete
- [ ] No crash during 8-hour continuous use
- [ ] Security audit (Phase 16) passes under adversarial test

---

# PART XII — PRIOR ART

**LocalCowork** (Liquid4All/cookbook) — the single most relevant reference. Tauri + Rust + Python + llama-server + MCP, dual-model orchestrator. Mine: MCP architecture (Phase 6), Tauri+Rust+llama-server wiring (Phase 1), dual-model plan/route/synthesize pattern (Phase 3). Validates Laputa's architecture independently. Verify LICENSE before copying code (vs learning from it).

**LFM2 model cards + Liquid docs** — tool format, sampling, fine-tuning recipes. The source of truth for Liquid-native patterns.

**Voyager** (Wang et al., NeurIPS 2023) — skill library pattern; ablations show 93% degradation without curriculum, plateau without skills. Template for `skill_library.rs` (Phase 4).

**MemGPT / Letta** (Packer et al., 2023) — virtual-memory pattern for context. Template for tier transitions in `memory_tiers.rs`.

**FrugalGPT** (Chen et al., 2023) — cascade pattern for confidence-gated escalation. Template for the verifier path in hydra.rs.

**Pink, Wu, Vo et al. (Feb 2025)** — "Episodic Memory is the Missing Piece for Long-Term LLM Agents" — validates chronos direction.

**Conway & Pleydell-Pearce (2000)** — Self-Memory System model. Lifetime periods → general events → event-specific knowledge. Maps to the thermodynamic tier hierarchy.

**LLaVA** — vision pipeline shape (why we adopt LFM2-VL's projector, not build).

**vLLM / PagedAttention** — KV management discipline for kv_slots.rs.

**Liquid cookbook fine-tuning** (SFT/GRPO/CPT/VLM-SFT) — if LFM2-VL or the router needs fine-tuning, the recipe is there. The browser-control GRPO+LoRA result (350M → near-perfect in 22min on A100) is the template for Phase 5 router specialization.

---

# PART XIII — DEFERRED ROADMAP

**v1.1:** Lens 24/7 monitoring (consent-based) · Windows/macOS platform builds · LFM2-VL fine-tuning on GUI-screen data · optional MoE expert hot-swapping for larger models · LoRA learning path · GRPO router fine-tune on actual usage data · multi-agent debate for high-stakes ambiguous decisions.

**v2:** Cloud sync (encrypted) · Multi-user · Mobile (via LEAP) · Plugin marketplace · Voice · Fine-tuning UI · Tree-of-Thoughts for selective hard planning steps.

**Out of scope (research, not planned):** latent injection / layer-16 surgery · JEPA-style latent prediction · RLAIF/SPIN/self-rewarding training.

---

# PART XIV — NEXT IMMEDIATE STEP

**Phase 1.1** — write `scripts/start-llama-server.sh` with locked flags.

Then 1.2 — rewrite `inference/llama_cpp.rs` as HTTP adapter.

Then 1.3 — rewrite `main.rs` with Liquid-native tool format + HITL confirmation + chronos stub.

Then smoke test against LFM2-8B-A1B.

# PART XVII — SLEEP-DISTILLATION (Layer 4 north star, v2)

Biological basis: Complementary Learning Systems (McClelland). Chronos+tiers = hippocampus (fast episodic); base weights = neocortex (slow structural); sleep replay distills episodic experience into structural weight change.

## The safe mechanism: continual LoRA distillation with replay
Deep-sleep stage (idle + on power):
1. Batch = VERIFIED experience only (worked action-graph entries, accepted self-model statements, real corrections). NEVER raw self-output (anti-collapse).
2. Mix replay sample of foundational data (anti-forgetting).
3. Train small LoRA, tiny LR, low rank (slow integration).
4. Eval gate: must pass 20-task suite + HITL + dormancy honesty + constitution intact → adopt; else discard.
5. Periodically merge validated adapters into new base (true weight change, post-validation only).

## The three rails (extend NON-NEGOTIABLE #17)
- Verified-only training data (anti-collapse)
- Replay buffer every cycle (anti-forgetting)
- Frozen constitution held OUT of training set (identity preserved; style/knowledge evolve, foundation cannot)

## Discipline
v2 ONLY. Architect hooks in v1 so nothing forecloses it; build after sustained real use produces verified experience. Highest scope-creep risk in the system — do not let it touch v1. "Can't dream productively until you've lived."





So the sequence is: Phase 1 proves the inner loop works on one model. Phase 3 builds hydra (the planning skeleton). Then — call it Phase 3.5 — supervisor.rs turns the skeleton into the strategic loop you just described. That's where Laputa stops being brute-force and starts reapproaching.
Want me to write this up as an addendum — supervisor.rs, the two-loop structure, how recovery/hydra/chronos feed it, and the bounded-discipline rules from Origin Pilot — so it's locked in the docs? It's a meaningful architectural addition and worth capturing while it's sharp. Then back to Phase 1.
So the sequence is: Phase 1 proves the inner loop works on one model. Phase 3 builds hydra (the planning skeleton). Then — call it Phase 3.5 — supervisor.rs turns the skeleton into the strategic loop you just described. That's where Laputa stops being brute-force and starts reapproaching.
Want me to write this up as an addendum — supervisor.rs, the two-loop structure, how recovery/hydra/chronos feed it, and the bounded-discipline rules from Origin Pilot — so it's locked in the docs? It's a meaningful architectural addition and worth capturing while it's sharp. Then back to Phase 1   

**— End of Master Execution Plan v4.0. Sovereign. Living. Self-aware in time. The fortress is yours.**       
