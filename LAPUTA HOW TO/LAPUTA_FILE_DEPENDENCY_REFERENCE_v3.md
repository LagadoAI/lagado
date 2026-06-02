# LAPUTA — FILE DEPENDENCY REFERENCE
**Companion to the Master Plan.** For each file: **what it is** · **theory (why this shape)** · **how we build it** · **connections (depends on / depended on by)**.

**Version 3 · May 30, 2026 · Liquid-Native / Living-Fortress**
**Status:** ☑ working · ◐ needs wiring · ☐ not started.

> **What changed in v3:**
> - **Inference transport switched FFI → HTTP** (Option B). llama-server subprocess + reqwest, matching Liquid's own LocalCowork production pattern.
> - **chronos.rs added** — autobiographical timeline; gives agent a sense of time and self-continuity. T=0 anchored to first user login.
> - **Awakening page added** — first-launch experience that primes the user and starts chronos.
> - **Tool format corrected** — Liquid-native (system-prompt JSON list + Pythonic emission), NOT OpenAI schemas.
> - **Sampling locked** — temp 0.1, top_k 50, rep_penalty 1.05 for tool routing; temp 0.2 for reasoning brain.
> - **Clean-context discipline** — no conversation history sent to router; history poisons small models (LocalCowork 78% → 8% data).
> - **Confirmation loop (HITL)** wired in Phase 1, not deferred.
> - **Dual-model orchestrator** (plan/route/synthesize) is the architecture, not just a future option.
> - **skill_library.rs added** — Voyager-style verified-action store for Phase 3+.
> - **Living Fortress vision** governs every decision: sovereign + living + self-aware in time.

---

## UI CORE

**main.tsx** ☑
- **What it is:** React entry point; mounts the app.
- **Theory:** Standard Vite/React bootstrap. No business logic.
- **Build:** Done.
- **Depends on:** App.tsx, index.css. **Depended on by:** index.html.

**App.tsx** ◐
- **What it is:** Root component + router; routes between pages including the new Awakening page.
- **Theory:** Single router source. First-launch detection determines whether to show Awakening or Login.
- **Build:** Add Awakening route; conditional render based on `first_login_complete` flag from backend.
- **Depends on:** all routed pages, react-router, useAgentSocket. **Depended on by:** main.tsx.

**index.css** ☑ / **index.html** ☑ — global styles, Vite entry. Done.

## UI COMPONENTS (all ☑ unless noted)
Reusable presentational pieces. Depend on utils. Depended on by pages.

**Button/Input/Card/Header/Tabs/Dialog/FormGroup/Checkbox/Radio/Select/Slider/Alert/ProgressBar/Badge/CodeBlock/Spinner/FileTree/MetadataList/Layout** — leaves of the UI tree. Depend on colors.ts, format.ts.

**ConfirmationDialog** ☐ NEW
- **What it is:** Modal asking the user to approve, edit, or reject a proposed tool call before execution.
- **Theory:** LocalCowork's research shows this single feature converts 80% selection accuracy into ~100% effective accuracy. It is the highest-ROI reliability lever. Two confirmation levels: tap-confirm for normal write actions, typed-confirm for destructive actions.
- **Build:** Receives `{tool_name, args, risk_level}` over WebSocket; presents a clear preview; returns approval/rejection. Risk level inferred from tool registry — read = auto, write = confirm, destructive = typed-confirm.
- **Depends on:** Dialog, Button, useAgentSocket. **Depended on by:** ChatDefault, ImmersiveDefault.

**PermissionBrowserDialog/SessionRestoreDialog/URLHandlerConfirmationDialog/ClipboardAccessDialog/ExitImmersiveDialog** — depend on Dialog + useAgentSocket; flow-specific.

**HostModeGate** ☐ — approve/deny/edit gate for host actions. Pairs with validator.rs, input_arbiter.rs.

## UI HOOKS

**useAgentSocket** ◐
- **What it is:** THE single UI ↔ backend bridge (WebSocket :9090).
- **Theory:** Chokepoint — keeps the front/back boundary clean and testable. All state flows through one wire.
- **Build:** Already partially built; needs the new event types for chronos, awakening, confirmation, model_progress.
- **Depends on:** main.rs (backend side). **Depended on by:** every live page.

**useAsync** ☑ / **useLocalStorage** ☑ — async state / browser persistence.

## UI UTILS
**colors.ts** ☑ / **format.ts** ☑ — palette + formatting. Leaves.

## UI PAGES (◐ unless noted)

**Awakening.tsx** ☐ NEW (Phase 2 scaffold → Phase 3 animations → Phase 4 live wired)
- **What it is:** Single-use first-launch experience. Establishes Laputa's identity and starts the chronos timeline.
- **Theory:** The user must leave understanding Laputa is a *living structure*, not a chatbot. Emotional anchoring before functional use creates investment. The page also defines chronos T=0 — the agent's "birth" is when the user meets it. Never shown twice.
- **Build:** Five-beat sequence (dark pulse → structure assembling → fortress reveal → five truths fade in → "Today, I begin." click-through). User builds the animations. On click-through: WS call `chronos.initialize_timeline(user_id, now)`; persist `first_login_at` to vault; route to Login or main view.
- **Depends on:** useAgentSocket, chronos.rs (backend). **Depended on by:** App.tsx (conditional route).

**LoginPage** ☑ — auth UI. Pairs with auth/login.rs.

**ChatDefault** ◐
- **What it is:** Chat surface; shows which brain answered, includes confirmation prompts inline.
- **Theory:** Chat is the entry point most users start with. Surface brain routing transparently (small "Liquid" / "Reasoning" tag per message) and surface tool-call previews inline (not as separate modals when context allows).
- **Build:** Add ConfirmationDialog hooks. Add brain-indicator badge to message headers. Stream tokens.
- **Depends on:** useAgentSocket, Button, Input, ConfirmationDialog. **Depended on by:** App.tsx.

**ImmersiveDefault (+Typing/AgentRunning/AgentPaused/WithSidebar)** — live agent view, VM/host, SPICE, gate. Pairs with projector/*, vm/manager.rs, input_arbiter.rs.

**CodePage** — editor + run. Pairs with terminal/pty_manager.rs.

**VaultDefault** — encrypted file manager. Pairs with security/crypto.rs, memory.rs.

**TerminalDefault** — PTY UI. Pairs with terminal/*.

**SettingsMain (+Backup/Models/Inference/KVCache/Permissions/Vault/SystemIntegration/AppConnections/Advanced/Chronos)** — settings. New Chronos sub-page shows the timeline visually (high-level — "you've been together 47 days, last major change: X"). Pairs with config, hydra.rs, security/*, chronos.rs.

**MCPManager/MCPAddTool** — MCP management. Pairs with mcp/server.rs.

**ServerManagement** — model/brain control. Pairs with inference/*, hydra.rs.

**VMManager** — QEMU controls. Pairs with vm/manager.rs.

## UI CONFIG (all ☑)
**package.json / tailwind.config.js / vite.config.ts / tsconfig.json / postcss.config.js** — toolchain.

---

## AGENT CORE (Rust)

**main.rs** ◐
- **What it is:** Orchestrator + WebSocket server. The thin top-of-stack that wires everything together.
- **Theory:** Keep it thin. Logic belongs in the modules; main.rs only sequences and connects. The agent loop here is the dual-model orchestrator: plan → route (with HITL confirmation) → execute → synthesize → log to chronos. Stateless inference — each model call gets a clean prompt; persistent state lives in memory_tiers + chronos.
- **Build:** Phase 1 rewrite replaces the old HTTP/Qwen path with the LlamaCppAdapter (now HTTP to llama-server). Spawns and manages llama-server subprocess. Wires ConfirmationDialog. Inserts chronos_log() per turn.
- **Depends on:** forge, action_graph, recovery, parser, verifier, operator, types, memory, memory_tiers, chronos, input_arbiter, inference/mod, perception/mod, projector/mod, security/mod, mcp/server, pty_manager, vm/manager, tray, url/server, auth/login, gate.
- **Depended on by:** useAgentSocket, Cargo.toml, perceive.py.

**forge.rs** ☑ — DO NOT MODIFY. Harness wraps a model call with parse + verify + retry. Depended on by main.rs.

**action_graph.rs** ◐
- **What it is:** Learned-workflow store. Records (state_hash → action → outcome → probability) and short-circuits the model when a known workflow scores high.
- **Theory:** Frequently-used patterns get a fast path that bypasses inference entirely. Pre-seeded with ~25-30K common-app workflows day one (Phase 4) so the small model isn't doing first-principles routing on Day 1. Cache temperature applies here too — stale workflows decay; hits reinforce.
- **Build:** Already drafted; needs wiring into the agent loop (lookup with threshold 0.65 before invoking router).
- **Depends on:** memory, action_graph.db. **Depended on by:** main.rs, hydra.rs (shortcut), retrieval.rs.

**recovery.rs** ◐
- **What it is:** Failure-type classifier and recovery dispatcher (7 failure modes).
- **Theory:** Most agent failures fall into recurring shapes (model_error, parse_error, tool_error, verify_error, max_retries, max_steps, user_interrupt). Each gets a tailored response, not a generic retry.
- **Build:** Already drafted; wire into the agent loop's error arm.
- **Depended on by:** main.rs.

**parser.rs** ☑ — DO NOT MODIFY. JSON parse + rescue. **NOTE:** Phase 1 adds a sibling for the Pythonic bracket format (`[tool(arg="x")]`) since LFM2's native emission isn't JSON. Implemented as `parser_bracket.rs` — keeps parser.rs untouched.

**parser_bracket.rs** ☐ NEW
- **What it is:** Parses Liquid's native Pythonic tool emission (`[get_weather(location="Paris")]`) wrapped in `<|tool_call_start|>` / `<|tool_call_end|>`.
- **Theory:** LFM2 was trained on this format; forcing JSON has 94% parse-failure rate on the planner. We meet the model where it lives.
- **Build:** Strip the tool-call sentinels, parse the bracketed Python-style call into a ToolCall struct. Lightweight regex + AST walk; no eval.
- **Depends on:** types.rs. **Depended on by:** forge.rs (via dispatch), main.rs.

**verifier.rs** ☑ — DO NOT MODIFY. Post-action sanity check.

**operator.rs** ◐
- **What it is:** Tool registry + execution dispatch.
- **Theory:** Each tool is self-describing (name, schema, risk_level). The registry presents the active subset to the router; risk_level drives ConfirmationDialog gating.
- **Build:** Extend with risk_level field; add the RAG pre-filter (K=15) — `select_candidate_tools(query) -> Vec<Tool>` using embedding similarity to the user's intent.
- **Depends on:** mcp/server. **Depended on by:** main.rs.

**types.rs** ◐
- **What it is:** Shared types across modules (Step, ToolCall, PipelineError, etc.).
- **Theory:** Single source of truth for cross-module shapes. Avoid duplication.
- **Build:** Extend with ChronosSnapshot, ConfirmationRequest, RouterDecision.
- **Depended on by:** most core files.

**memory.rs** ◐
- **What it is:** Legacy FAISS + vault interface. Will be wrapped by memory_tiers.rs (Tier 3 cold storage).
- **Theory:** Treat as the cold layer. Direct access for exact-text recall only; warm/hot paths go through memory_tiers.
- **Build:** Keep API; wire into memory_tiers.rs as the Tier-3 backing store.
- **Depends on:** FAISS index, security/crypto.rs. **Depended on by:** memory_tiers.rs, retrieval.rs.

**input_arbiter.rs** ☐ — input multiplexer: user > agent > harness. Depends on executor.rs, vm/manager.rs. Depended on by main.rs.

**chronos.rs** ☐ NEW (Phase 1 stub, Phase 4.5 full)
- **What it is:** The autobiographical spine. Parallel timeline of agent state snapshots that gives Laputa a sense of time and self-continuity.
- **Theory:** Current AI is stateless per inference. It can't say "I used to believe X, now I believe Y" or "I've tried this 4 times." Chronos fills that gap with paired metadata snapshots alongside every memory entry. Three compression tiers (full ~200 tokens / embedding 256-d / tagged 50 bytes), retrievable alongside the memory itself. T=0 anchors at first user login.
- **Build:** Phase 1: stub `chronos_log()` writes timestamp + active_goal + last_action per turn to `chronos.db`. Phase 4.5: full implementation — snapshot struct, decay/reinforcement, delta detection, retrieval API. Sleep gate writes chronos + memory entries together.
- **Snapshot shape:**
  ```rust
  struct ChronosSnapshot {
    timestamp:        i64,
    active_goal:      String,
    beliefs_summary:  [f32; 256],   // embedding of current worldview
    recent_actions:   Vec<String>,
    confidence_state: f32,
    tone:             String,
    delta_from_prev:  String,        // 1-line "shifted from X to Y"
  }
  ```
- **Depends on:** memory_tiers.rs, sleep_gate.rs, chronos.db. **Depended on by:** main.rs, retrieval.rs (paired retrieval), Awakening.tsx (T=0 init), prompt construction.

---

## INFERENCE — DUAL-MODEL LIQUID-NATIVE ENGINE

**inference/mod.rs** ◐
- **What it is:** Defines the `InferenceAdapter` trait — the boundary between the rest of the code and whatever backend is in use.
- **Theory:** Backend-agnostic interface. Today llama-server over HTTP; tomorrow could be a LEAP plugin, vLLM, or anything else. The trait stays generic so swapping backends doesn't touch the rest of the system.
- **Build:** Trait already drafted (generate, supports_kv_slots, save/restore/has_kv_slot, model_name, context_size). No changes needed.
- **Depends on:** llama_cpp, hydra, liquid, kv_slots, grammar, memory_handoff. **Depended on by:** main.rs.

**llama_cpp.rs** ☐ (now HTTP, not FFI)
- **What it is:** Implements `InferenceAdapter` against a llama-server subprocess running locally on port 8080.
- **Theory:** Liquid themselves use llama-server in LocalCowork — it's the production-validated pattern. Avoids FFI's unsafe pointers, gets GPU + flash-attn for free, gives clean API access to grammar, slots, and tool routing. The "subprocess" cost is one Tokio task to monitor health and restart on crash; far simpler than maintaining FFI bindings.
- **Build:** `LlamaCppAdapter::new(model_path)` spawns llama-server with our flags (see start-llama-server.sh), waits for readiness on /health, returns the adapter. `generate()` POSTs to /v1/chat/completions with the prompt + sampling params. `save/restore_kv_slot()` uses llama-server's `/slots` endpoints. Drop kills the subprocess.
- **Sampling defaults (locked from Liquid docs):**
  - Tool routing: `temp=0.1, top_k=50, repetition_penalty=1.05, max_tokens=512`
  - Reasoning brain: `temp=0.2, top_k=80, repetition_penalty=1.05`
- **Tool format (locked from LFM2 training):** tools as JSON list in system prompt text (NOT chat-template `tools=` param — drops accuracy to 0%); model emits Pythonic `[tool(arg="x")]` wrapped in `<|tool_call_start|>...<|tool_call_end|>`.
- **Depends on:** reqwest, llama-server subprocess, kv_slots, grammar, start-llama-server.sh. **Depended on by:** inference/mod, liquid, hydra.

**hydra.rs** ☐
- **What it is:** The dual-model router. Classifies intent and routes to the right brain.
- **Theory:** LocalCowork's three-step orchestrator (plan → route → synthesize) is the proven local-agent pattern. Plan and synthesize run on the heavy MoE brain (LFM2-8B-A1B) with reasoning context; routing runs on a tiny dedicated model (LFM2-1.2B-Tool or fine-tuned LFM2-350M) on a **clean prompt with zero conversation history**. Clean context is non-negotiable — history poisoning is the documented cause of 78% → 8% collapse.
- **Build:**
  1. Intent classifier (LFM2-350M): ~50ms decision — vision / interactive / reasoning.
  2. Plan phase: heavy brain breaks user request into self-contained steps; emits bracket format.
  3. RAG pre-filter (K=15): per step, retrieve candidate tools by embedding similarity (via operator.rs).
  4. Route phase: small router model picks one tool per step from K=15 on a clean prompt.
  5. Optional verifier (Phase 2): self-consistency sample N=3-5; if vote margin is thin, escalate that step to the heavy brain for re-decision.
  6. Synthesize phase: heavy brain streams a user-facing summary from accumulated results.
  7. Fall back to single-model loop if any phase fails.
- **Depends on:** liquid.rs, llama_cpp.rs, action_graph.rs (shortcut), retrieval.rs (pre-context), operator.rs (RAG pre-filter). **Depended on by:** inference/mod.

**liquid.rs** ☐
- **What it is:** Manages the Liquid model roster (load, size-select, vision pipeline).
- **Theory:** Liquid ships several model sizes; we want to pick the right one for the job and keep small ones resident. LFM2-VL has the vision pipeline as a separate model with a shipped projector — we don't train, we adopt.
- **Build:** `load(model_id) -> Result<ModelHandle>`; `size_select(intent, latency_budget) -> ModelId`; `vision_pipeline(image) -> Tokens` via vlm_adapter.rs.
- **Depends on:** llama_cpp.rs, vlm_adapter.rs. **Depended on by:** hydra.rs, inference/mod.

**memory_handoff.rs** ☐
- **What it is:** Brain-agnostic state object passed between models during routing handoffs.
- **Theory:** When hydra routes from small to heavy brain (e.g., verifier escalation), the heavy model needs context: what the small model saw, what action_graph hit, what the current screen/state is, what intent was classified. Single struct prevents ad-hoc parameter passing.
- **Build:** Struct + serialize/deserialize. Carries (summarized_context, action_graph_hits, perception_state, intent_label) outbound; receives (decision, reasoning_trace, confidence) back. Persists to action_graph so future similar problems short-circuit.
- **Depends on:** action_graph.rs, types.rs. **Depended on by:** hydra.rs, inference/mod.

**retrieval.rs** ☐
- **What it is:** Top-k retrieval over action graph + vault facts + chronos snapshots; injects a compact prefix before model inference.
- **Theory:** Compensates for Liquid's short effective context. Even with a 32K window, smaller models degrade on long inputs — the answer is *better* context, not *more* context. Chronos retrieval gives the model autobiographical grounding ("you decided X 3 days ago, since then Y").
- **Build:** Embedding-similarity search over FAISS; for each query, retrieve top-K action graph entries + relevant chronos snapshots + vault facts; format as a compact bullet list prefix. Tunable K by latency budget.
- **Depends on:** FAISS index, action_graph.rs, memory.rs, memory_tiers.rs, chronos.rs. **Depended on by:** hydra.rs.

**memory_tiers.rs** ☐
- **What it is:** Three-tier memory hierarchy with thermodynamic temperature decay.
- **Theory:** Mimics human memory. Hot working memory in RAM (current turn); warm summarized memory on hot disk (recent sessions, FAISS-retrieved); cold deep memory in encrypted vault (exact text on demand). Each entry has a temperature (0.0–1.0) that decays exponentially with time and reinforces on access. State transitions: T<0.7 → summarize; T<0.4 → drop summary, keep embedding; T<0.1 → archive to cold; T=0 after N days → forget. This gives effectively unlimited context bounded only by SSD, while keeping the model's input clean and short.
- **Build:**
  - Tier 1 — /dev/shm working memory, zero-copy, current-turn only.
  - Tier 2 — encrypted hot disk, summarized, FAISS-retrievable.
  - Tier 3 — encrypted vault, exact text, lazy-decrypt on retrieval.
  - Temperature field on every entry; background decay loop in sleep_gate.rs.
- **Depends on:** /dev/shm, security/crypto.rs, FAISS index, memory.rs. **Depended on by:** retrieval.rs, hydra.rs, sleep_gate.rs, chronos.rs.

**sleep_gate.rs** ☐
- **What it is:** Background consolidation process. Runs on idle/shutdown, compresses recent sessions into Tier-2 summaries, updates chronos snapshots, decays temperatures.
- **Theory:** Mimics sleep-stage memory consolidation. Routine summaries use a small Liquid model; important sessions use the heavy brain. Without consolidation, hot memory bloats and context drifts. With it, memory compounds productively over time.
- **Build:** Tokio background task with idle detection. On trigger: select Tier-1 entries cooled below threshold → invoke summarization → write Tier-2 entry + chronos snapshot together → update temperatures → repeat. Configurable cadence.
- **Depends on:** memory_tiers.rs, chronos.rs, hydra.rs. **Depended on by:** main.rs.

**skill_library.rs** ☐ NEW (Phase 3+)
- **What it is:** Voyager-style verified-action store. Successful multi-step sequences saved as executable code/JSON, indexed by NL description.
- **Theory:** Voyager (NeurIPS 2023) found that storing successful behaviors as code (not natural language) unlocks compositional reuse — agents that ablated this plateaued after ~80 iterations. For Laputa, this is action_graph's bigger sibling: action_graph stores single state→action pairs; skill_library stores verified multi-step procedures ("file my receipts" = 7 ordered tool calls).
- **Build:** Defer to Phase 3. Schema: `Skill { name, nl_description, embedding, steps: Vec<ToolCall>, success_count, last_success }`. Retrieval: embedding-similarity top-K with success-rate weighting.
- **Depends on:** action_graph.rs, memory_tiers.rs. **Depended on by:** hydra.rs (alternate shortcut path).

**kv_slots.rs** ☐
- **What it is:** KV warm-start via llama-server's `/slots` endpoints.
- **Theory:** Same screen, same model → same KV cache. Reuse it for sub-300ms warm response on revisits. Fingerprint guard prevents stale-cache corruption.
- **Build:** Call llama-server's slot save/restore. Fingerprint = SHA256(model_id + ctx_params + state_hash); validate on restore.
- **Depends on:** llama-server's API, /dev/shm (fallback cache). **Depended on by:** llama_cpp.rs.

**grammar.rs** ☐
- **What it is:** GBNF constraint emitter for selector-bounded outputs.
- **Theory:** When the model picks an on-screen element, the output must be one of the ref_ids actually present. Grammar constraint at sampling time makes invalid outputs impossible, not merely retry-able. llama-server accepts grammar as an API field.
- **Build:** Generate GBNF dynamically from current perception; pass in API request body.
- **Depends on:** parser.rs, perception/mod.rs. **Depended on by:** llama_cpp.rs.

---

## PERCEPTION

**perception/mod.rs** ☐ — perception loop coordinator. Depends on capture, delta, vlm_adapter, atspi, cache. Depended on by main.rs.

**capture.rs** ☐ — PipeWire/portal screen capture → /dev/shm at 20Hz. Depended on by perception/mod.

**delta.rs** ☐ — Blake3 per-cell change detection; re-vision only on changed regions; 1000 iters <100ms target.

**vlm_adapter.rs** ☐
- **What it is:** Bridge to LFM2-VL. Feeds a (changed-region) screenshot, receives visual tokens / understanding text.
- **Theory:** Use Liquid's shipped SigLIP2 + 2-layer MLP projector — no training. Caveat: SigLIP2 trains on natural images, GUI screens may need fine-tuning later.
- **Build:** Adapter sends image + prompt to llama-server with LFM2-VL loaded; receives understanding tokens.
- **Depends on:** liquid.rs. **Depended on by:** perception/mod.

**atspi.rs** ☐ — bridges perceive.py's AT-SPI2 accessibility tree into the loop as element text. The reliable backbone (text-path perception works even if vision is weak).

**cache.rs** ☐ — perception cache in /dev/shm.

---

## PROJECTOR (Host control)

**projector/mod.rs** ☐ — coordinator.
**capturer.rs / detector.rs / executor.rs / validator.rs** ☐ — OS-dispatched capture, detect, input, permission.
**platform/{linux,macos,windows}.rs** ☐ — OS impls. Linux first.

---

## SECURITY (THE headline layer — all ☐)

**security/mod.rs** — coordinator; wires sandbox/crypto/audit/isolation/profile.
**security/sandbox.rs** — seccomp + namespaces + cgroups; fail-closed.
**security/crypto.rs** — AES-256-GCM, Argon2id, lazy decrypt, nonce uniqueness.
**security/audit.rs** — append-only tamper-evident log of every sensitive op. **Now includes chronos events** (timeline init, snapshot writes) so the timeline is itself audited.
**security/isolation.rs** — defense-in-depth, capability drops, least privilege.
**security/profile.rs** — tiered Strict / Balanced / Open. Sensitive ops always gated regardless.

---

## VM (Whonix isolation — all ☐)
**vm/manager.rs** — QEMU supervisor (QMP).
**vm/gateway.rs** — hardened Whonix Gateway VM; owns ALL networking.
**vm/network.rs** — isolated networking; workstation has no direct internet.
**iso_loader.rs** — ISO select + boot.

## TERMINAL (all ☐)
**pty_manager.rs / tabs.rs / sandbox.rs / sanitizer.rs** — multi-tab seccomp PTY with escape sanitizer.

## SYSTEM (all ☐)
**tray.rs / clipboard.rs / pii_detector.rs / detect.rs** — tray, PII-filtered clipboard, hardware detection (CPU/RAM/GPU → feeds hydra sizing).

## URL HANDLER (all ☐)
**url/server.rs / whitelist.rs / url/security.rs / hmac.rs** — laputa:// handler with HMAC validation + rate limit.

## AUTH / PERMISSIONS (all ☐)
**auth/login.rs** — Argon2id auth → vault key. **NOTE:** On first successful login, calls `chronos.initialize_timeline()` — T=0 anchor.
**gate.rs / perm/storage.rs** — permission gate + persistent grants.

## CONNECTORS (all ☐)
**registry.rs / imap.rs / caldav.rs / credentials.rs** — connector dispatch with keyring-stored credentials and TLS 1.3+.

---

## DATA / VAULT

**/dev/shm** ☐ — shared-memory bus; zero-copy substrate. Chokepoint. Used by kv_slots, cache, capture, memory_tiers (Tier 1).
**vault (AES)** ◐ — encrypted store. Depended on by memory, auth, mcp/tools, chronos (Tier 3 snapshots).
**FAISS index** ◐ — vector index. Now indexes action graph, vault facts, AND chronos beliefs_summary embeddings.
**action_graph.db** ◐ — SQLite workflow store.
**chronos.db** ☐ NEW — SQLite timeline store. Schema: `(id, timestamp, active_goal, beliefs_blob, recent_actions_json, confidence, tone, delta_str)`. Indexed on timestamp + active_goal.

---

## PYTHON (existing scripts)
**perceive.py** ☑ — AT-SPI2 parser (Tier-1 perception).
**entropy_gate.py** ◐ — vault pruning (zstd fix).
**thalamus.py** ◐ — legacy prompt routing (deprecate after hydra.rs lands).
**build_index.py** ◐ — builds FAISS.
**run_cortex.sh** ◐ — legacy llama-server launcher. **Replaced by:** start-llama-server.sh in Phase 1.
**arise.sh** ◐ — boot launcher; recreates /dev/shm.

## SCRIPTS (new)
**start-llama-server.sh** ☐ NEW
- **What it is:** Launches llama-server with the locked Liquid-tuned flags for our hardware.
- **Build:** `llama-server --port 8080 --ctx-size 16384 --cache-type-k q8_0 --cache-type-v q8_0 --n-gpu-layers 99 --flash-attn on --jinja --temp 0.1 --top-k 50 --repeat-penalty 1.05 --model /home/d/.laputa-secure/models/LFM2-8B-A1B-Q4_K_M.gguf`.
- **Depended on by:** main.rs (spawn at startup), llama_cpp.rs.

## BUILD
**Cargo.toml** ◐ — Rust deps. v3 deps: reqwest (HTTP), tokio, tokio-tungstenite, futures-util, serde, serde_json, aes-gcm, argon2, keyring, seccompiler/libseccomp, caps, nix. **Removed:** raw FFI dependencies (no longer needed).
**build.rs** ☐ — Now nearly empty (just env vars). No more FFI linking.

## OPTIONAL ADVANCED (Phase 15+)
**expert_cache.rs** ☐ OPTIONAL — persistent VRAM expert slot cache for sparse-MoE. Deferred; LFM2-A1B handles routing internally.

## LENS (v1.1)
**lens/mod.rs / lens/encoder.rs** ☐ — consent-based 24/7 monitor. Depends on cache, action_graph. Depended on by main.rs.

---

## LOAD-BEARING CONNECTIONS (memorize)

1. **useAgentSocket → main.rs** — the only UI↔backend wire. Build early, test hardest.
2. **main.rs → llama-server (subprocess) + everything else** — orchestration hub. Thin.
3. **hydra.rs → {liquid.rs, llama_cpp.rs, action_graph.rs, retrieval.rs}** — the dual-model orchestrator; the engine's intelligence.
4. **chronos.rs ⇄ memory_tiers.rs ⇄ sleep_gate.rs** — the living memory triangle. Every snapshot/summary pairs across all three.
5. **security/mod.rs → {sandbox, crypto, audit, isolation, profile}** — the headline moat; every sensitive path goes through it.
6. **vm/manager.rs → vm/gateway.rs** — Whonix isolation; workstation never has direct internet.
7. **/dev/shm** — shared-memory bus under Tier 1 memory, perception, KV slots, cache.
8. **security/crypto.rs ← {memory, auth, vault, mcp/tools, chronos (vault snapshots)}** — encryption substrate.
9. **Awakening.tsx → auth/login.rs → chronos.rs** — T=0 anchor chain. The user's first click initializes the timeline.

**Build bottom-up:** /dev/shm + llama-server first → dual-model orchestrator (hydra + liquid) → memory triangle (tiers + sleep + chronos) → the UI bridge → security maxed → the rest. Awakening lands when chat and chronos are both live.

v## SLEEP-DISTILLATION (v3.3 addendum — v2 north star, hooks only in v1)

**distill.rs** ☐ NEW (v2 — architect hooks now, build later)
- **What it is:** Deep-sleep continual-learning process. Distills verified experience into LoRA adapters that slowly integrate into the model (eventually merged to base).
- **Theory:** CLS theory — sleep replay consolidates episodic memory into structural weight change. The capstone of the identity stack: episodes (chronos) → narrative (self_model) → structural weights (distill). Survivable via verified-only data (anti-collapse) + replay (anti-forgetting) + frozen constitution.
- **Build (v2):** assemble verified-experience batch + replay sample → QLoRA train (6GB-feasible, tiny LR/rank) → eval gate → adopt or discard → periodic adapter→base merge. Liquid GRPO/SFT cookbook is the baseline recipe.
- **v1 hooks (build now, cheap):** tag action-graph entries with verified-success flag; tag accepted self_model statements; keep a replay-data manifest. These make the eventual training set assemblable without retrofitting.
- **Depends on:** action_graph.rs (verified entries), self_model.rs (accepted statements), chronos.rs (high-value reflections), sleep_gate.rs (deep-sleep trigger), eval harness. **Depended on by:** liquid.rs (adapter load).

**Updates:**
- action_graph.rs — add verified_success flag (v1 hook).
- self_model.rs — accepted statements feed the distill set (v1 hook: just the flag).
- sleep_gate.rs — deepest stage triggers distill.rs (v2).
- LOAD-BEARING #12: chronos → self_model → distill. Episodes → narrative → weights. Frozen constitution is the floor across all three.






---

**— End of File Dependency Reference v3.**
