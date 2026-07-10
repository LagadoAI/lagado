# CLAUDE.md

Guidance for working in this repo. Read before making changes.

## What this is

**Lagado AI** — a local-first, privacy-first desktop agent. Four pillars (from PDF master plan):
1. **P1 — Maximum Security & Sovereignty** — local-only, encrypted, no telemetry. The headline moat.
2. **P2 — Dual-Brain Hydra** — fast LFM2.5 + optional 8B heavy. Governor decides tier. "AI anywhere."
3. **P3 — Fully Integrated Stack** — one coherent system, not loose modules
4. **P4 — Persistent Learning** — action graph + skill library, survives reboots

Production targets: **Windows-first, macOS, Linux**. Development on Linux.
GitHub Actions CI (linux/macos/windows) is the cross-platform test bench.
**Single source of truth:** `LAPUTA HOW TO/LAGADO_MASTER_PLAN.md.pdf` (June 3 2026 — supersedes all other plan files).
Companion detail: `docs/plans/FILE_DEPENDENCY_REFERENCE_v3.md`.

**CURRENT FOCUS (2026-07-03): THE HARNESS IS THE PRODUCT; the app/UI is the eventual shipping vehicle, not the current work.** The four pillars above are the long-term product vision. The defensible thing being built NOW is the deterministic harness that lets a small local model reliably operate a real desktop — proven on the REAL OSWorld benchmark, where success = golds with **zero false-pass** (integrity is scored, not assumed). Start from **"Status (2026-07-03)"** below and the harness doctrine sections; treat app-phase sections as background.

## Architecture

### Runtime
Single Tauri binary (`lagado-ui/src-tauri/`) wraps:
- React/shadcn UI — Liquid AI inspired, deep navy + blue/purple glassmorphism
- Rust agent core (`lagado-agent/` as a library)
- Vendored `llama-server` subprocess (HTTP inference on :8080, NOT FFI) — main 8B model
- Classifier subprocess on :8081 (LFM2.5-1.2B-Instruct, intent classification, CPU-only)
- Embedder subprocess on :8082 (LFM2-ColBERT-350M, `--embeddings --pooling mean`, CPU-only) — the Board's relevance signal; spawned in `main.rs`, watched by `server_guard`, fed by `sleep_gate` backfill, consumed by `agent_loop` via `assemble_slice` (recency floor when down)
- Visual encoder: in-process `libmtmd.so` FFI (LFM2-VL-450M + mmproj, vision → embedding vectors, no subprocess)
- QEMU desktop VM (agent's sandboxed working surface)

Inference: HTTP to `llama-server` → `/v1/chat/completions`.
Models in `~/.laputa-secure/models/`:
- `LFM2-8B-A1B-Q4_K_M.gguf` — main agent model (gen2, temp 0.3/min_p 0.15)
- `LFM2.5-1.2B-Instruct-Q4_K_M.gguf` — intent classifier (gen2.5, temp 0.1/top_k 50)
- `LFM2-VL-450M-F16.gguf` + `mmproj-LFM2-VL-450M-F16.gguf` — vision encoder
- `LFM2-ColBERT-350M-Q4_K_M.gguf` — Board embeddings (Phase 1)

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

**STATE-AWARE ROUTING (2026-06-18 — `hydra::deterministic_route`, the hard levers in front of the LLM router).** Routing = f(message-shape, system-state), NOT f(message) alone — system state is GROUND TRUTH, the LLM classify is a guess. Deterministic levers decide FIRST; the 1.2B fires only on the ambiguous/question residual (latency + reliability win; verified the 1.2B misroutes 6/7 natural task goals — create/make/delete/show/rename → CHAT/REASONING). NOT a clean-context violation: state ≠ conversation history (inv #2 safe, same carve-out as inv #10's trajectory state). Levers:
- **`SurfaceState { vm_active, immersive_active, host_control_active }`** — what the agent can act on NOW. `vm_active` = ground truth from `vm_ssh_port`; immersive = frontend flag (TODO plumb); host = Segment-7 slot (stubbed false). `any()==false` ⇒ no surface ⇒ action request → **Offer** to start one, else CHAT.
- **`RouteMode { Auto, ChatLock, ActLock }`** — explicit user mode; the REAL replacement for the weak/never-wired `is_paused`. ChatLock→always CHAT ("just chat"); ActLock→actionable acts, clear questions still chat ("you have control"); Auto→state+shape+residual-LLM.
- **`is_action_shaped`** (message-shape): command phrase | GUI verb | STRONG task verb (install/run/kill/git…) | SOFT task verb (create/make/delete/show…) + a COMPUTER-OBJECT (path / file-ext / system noun — the object separates "create a FILE" from "create a POEM"). Surface-active + action-shaped → Interactive, no LLM. `is_clear_question` keeps questions as chat under ActLock.
- Caller assembles `RouteContext` (e.g. `send_goal` reads `vm_ssh_port`); approval for autonomous plans = preview-whole-plan/approve-once + destructive-always-typed-confirm, inside the tiered earned-autonomy model (Strict/Balanced/Open + action_graph muscle-memory). See memory `lagado-autonomous-planning`.

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
**SSH readiness:** `vm_ssh_port` set asynchronously via background SSH auth probe (`ssh whoami` → exit 0 + stdout contains "laputa").
**Frame path:** `/dev/shm/lagado_frame.png` — constant `config::FRAME_PATH`.
**Auto-kill:** `Drop for VmHandle` + `KillOnDrop` wrapper kills all child processes on app exit.

### Auth (Phase 2 — COMPLETE)

**Wrapped DEK scheme** — FileVault/1Password pattern:
- Signup: random 32-byte DEK wrapped with `Argon2id(password)` + `Argon2id(recovery_phrase)`
- Both blobs in `~/.laputa-secure/config/keychain.json` — raw DEK never touches disk
- Login: Argon2id(password) → unwrap DEK → `auth::set_session_dek(dek)`
- Lockout: 3 failures → 10-min cooldown, persisted, fail-closed if tampered
- `auth::active_key()` is the only crypto entry point; falls back to `machine_passphrase()` in dev

### Memory system (COMPLETE — all phases)

**Full living memory triangle operational:**
- Hot entries → sleep_gate → LLM batch summarize → warm entries → entropy prune at 10,000
- Entropy equation: `V = T × e^(−λt) × (1 + ln(n+1))`, λ = ln(2)/30days (Ebbinghaus + log reinforcement)
- Cold (vault) never entropy-pruned; 365-day natural half-life protects it
- Skill distillation: Done/Task episodes → `distill_skill_async()` → LLM extract → `skill_library.save()`
- Visual embeddings: frame encoded at episode boundaries → cosine similarity retrieval
- `SleepGate::new(memory, adapter)` — full consolidation cycle every 5 min
- DB: `~/.laputa-secure/memory.db`, `~/.laputa-secure/skill_library.db`

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
- C shim at `lagado-agent/src/vision/shim.c` — `lagado_encode_image()` mean-pool (unchanged) + `lagado_encode_image_patches()` per-tile/per-patch with `lfm2_find_grid()` runtime grid derivation
- Rust binding at `lagado-agent/src/vision/mod.rs` — `VisualEncoder` behind `Mutex`; `encode_png_patches()` returns `Vec<TilePatches>`; `is_overview` set by structural chunk index (NOT token count)
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
- 110 lib tests pass (HEAD 938af52)

### Key modules (`lagado-agent/src/`)

| Module | Status | What it does |
|---|---|---|
| `hydra.rs` | ✓ | Dual-model orchestrator, few-shot classifier on :8081, blake3 state hash |
| `agent.rs` | ✓ | Agent loop, episodic memory context, HITL gate, RecoveryManager |
| `recovery.rs` | ✓ wired | 7 failure-mode dispatcher, graph-backed + LLM recovery |
| `memory_tiers.rs` | ✓ | Hot/warm/cold tiers, entropy equation, drain_cool_hot, promote_warm_summary, entropy_prune_warm |
| `sleep_gate.rs` | ✓ | Full consolidation: decay → batch summarize → warm promote → entropy prune. Takes adapter. |
| `server_guard.rs` | ✓ | Health monitor — polls /health every 10s, auto-restarts crashed llama/classifier servers, emits tauri events |
| `chronos.rs` | ✓ | SQLite timeline, T=0 anchor |
| `retrieval.rs` | ✓ | RAG K=15, Jaccard scoring |
| `action_graph.rs` | ✓ | SQLite workflow store, shortcut path |
| `skill_library.rs` | ✓ | Experiential depth layer — read + distillation write path wired |
| `security/crypto.rs` | ✓ | AES-256-GCM, Argon2id, DEK wrapping |
| `auth/mod.rs` | ✓ | Wrapped DEK, lockout, `active_key()`, `set_session_dek()` |
| `self_model.rs` | ✓ | Accepted beliefs, distill feed |
| `distill.rs` | ✓ hooks | Replay manifest for Phase 3 QLoRA |
| `perception/mod.rs` | ✓ | PerceptionCache (coords + bboxes), VlmPerceptor retired |
| `perception/linux.rs` | ✓ | AT-SPI2 via perceive.py --focused, populates both coords and bboxes |
| `perception/delta.rs` | ✓ | Pixel-space blake3 per cell — decoded RGB, remainder → last col/row |
| `perception/frame.rs` | ✓ | FrameProcessor: PNG→RGB→DeltaDetector, stateful, reset() between sessions |
| `perception/cv_proposer.rs` | ✓ | Canny + 8-connected components (imageproc), ProposalResult, extract_cell_rgb |
| `perception/vlm_adapter.rs` | kept | Text path kept for reference; not used in agent pipeline |
| `perception/arbiter.rs` | ✓ | IoU-dedup fusion → `FusedElement` (a11y+CV+vision), per-frame index space; wired live (a11y) in `agent_loop` |
| `perception/selection.rs` | ✓ | Candidate set from fused elements: `build_candidates`/`candidate_coords`/`render_candidates`/`rank_late_band` (late-band)/`goal_matches_any` (fail-closed)/`index_token`/`ESCAPE_TOKEN` |
| `grammar.rs` | ✓ | `intent_grammar` + `selector_grammar(&[FusedElement])` real GBNF (el_N + escape), piped via `generate_constrained` |
| `perception/harness.rs` | NOT BUILT | PerceptionMode switch + CSV measurement log (TASK 7) |
| `vision/mod.rs` | ✓ | encode_png() mean-pool + encode_png_patches() per-tile; PatchEmbedding/TilePatches; is_overview by structure |
| `vision/shim.c` | ✓ | lagado_encode_image() mean-pool (unchanged) + lagado_encode_image_patches() per-tile; lfm2_find_grid() |
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
8. **SSH readiness**: never set `vm_ssh_port` before SSH auth probe (`ssh -o BatchMode=yes ... whoami`) returns exit 0 and stdout contains "laputa". Bare TCP connect is insufficient.
9. **No hardcoded model/hardware values** (2026-06-17): never hardcode a model- or hardware-specific value (context window, layer count, n_gpu_layers, ctx size, model size, parallelism, CPU/GPU placement). **DISCOVER** it (GGUF metadata via the model-reader / hardware probe) or **DEFER** it (governor/user setting), always with a *discovered* default. The model is swappable (H-1) — assuming its context/layers/size is a latent bug. The only literals allowed are principled constants unrelated to model/hardware (ports, the 30-day Ebbinghaus curve). See `docs/plans/LAGADO_MODEL_AWARE_GOVERNOR_SPEC_v1.md`.
10. **No Board/retrieved memory in the action path** (2026-06-17): the action-selection prompt (`agent_loop`'s executor) sees ONLY the pinned SYS framing + the candidate list + the goal/sub-goal. NEVER inject episodic/visual/skill/Board memory into it — verified that semantically-related prepended memory *overrides* the candidate labels and flips the pick (decoy-priming → 12/12 wrong; see `docs/plans/LAGADO_ACTION_SELECTION_OPEN_QUESTION_v1.md` §2.5). The Board is chat-RAG + skill-advisory ONLY. Deterministic harness *trajectory* state (Q1 action-outcome fact, step pointer) is a DIFFERENT, safe category (not retrieved prose) and is allowed. Corollary: the executor prompt's **SYS preamble length is load-bearing** (it lands the candidate list in the model's late-attention band) — do not trim it without re-running the position sweep (`docs/plans/experiments/lean_gate.py`).

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

**The top model does ALL work — planning AND implementation. No Haiku/Sonnet delegation** (user directive 2026-06-16; main loop moved Opus → Fable 5 on 2026-07-03). Use the `advisor` tool for the adversarial-review/skeptic pass before load-bearing designs and when declaring done.
Verify with `cargo check --workspace` + `npx tsc --noEmit` after changes.

**Docs policy (2026-06-16 reversal):** `docs/`, `LAPUTA HOW TO/`, and all plans/PDFs are now COMMITTED (was: local-only). Machine = single point of failure; repo is private forever, only the binary ships → disaster-recovery beats secrecy. Never make the repo public.

## Status (2026-07-03) — harness / OSWorld phase (CURRENT)

**Branch `Harness`** (continuation of `deskew/class-not-instance`; `main` is pre-OSWorld). Current work = the REAL OSWorld benchmark as the harness's proving ground; `env.evaluate()` is the incorruptible judge.

- **Where we are:** LibreOffice Calc battery, held-out stress (30 never-opened tasks) = **7/30 gold, 0 false-pass** (2026-06-29). OP-VOCAB coverage note NOW STALE: as of 2026-07-10 the calc vocab is 22 op kinds in `uno_ops.py` (pivots/freeze/csv/transpose/reorder/conditional(format_cells_where)/locale(set_decimal_separator)/dedup/sort/zoom/pdf/charts ALL built) — only sparklines + row/col-resize genuinely absent. This full vocab is now REACHABLE FROM THE GENERAL LOOP via the calc-solver rung (`calc_solve.py`, flag `LAGADO_CALC_SOLVER`, A/B 2/3 vs 0/3 baseline 2026-07-10). Attribution model+harness vs model-alone awaits the per-grounding ablation matrix (see `docs/osworld/PROMPT_GROUNDING_AUDIT_2026-06-29.md`).
- **Architecture in play:** native session plane (resident guest UNO daemon over a host-owned op log; the proven stateless one-shot kept as the floor) + interface-altitude loop (candidates → reason → emit-in-names → notation-robust resolve, fail-closed → read-back/corroborate → retry). Grounding applied at seven seams, bind-or-abstain integrity held under stress.
- **Authoritative resume:** `docs/osworld/INVESTIGATION_PLAN_2026-06-23.md` (the "POST-CLEAR RESUME PLAN" block at top). Companions: `docs/osworld/PREDICTIONS.md`, `docs/osworld/BATTERY_FINDINGS_2026-06-22.md`, `docs/INTEGRATION_SURVEY_2026-06-29.md`.
- **Operating agreements (binding, see memory):** integrate-before-invent; verifiable-evals integrity (official evaluator only, frozen/held-out prompts, never lead-to-gold); deterministic-over-prompt (never prompt-nudge to pass a task); never replace what works; honest numbers — never soften a failure or inflate a result.
- **Known structural debt:** executable harness tooling (`uno_daemon.py`, `uno_ops.py`, `battery_calc.py`, `lagado_agent.py`, batteries/probes) lives under `docs/osworld/` — relocation pending; ⚠️ `import uno` ONLY at `uno_ops.py` module top.

## App-build status (2026-06-11 — HISTORICAL, pre-OSWorld phase)

**FULL LIVING MEMORY SYSTEM COMPLETE. Perception fusion harness TASK 1–6 complete (TASK 7 next). VM control channel end-to-end tested + provisioning fixed.**

HEAD: `0c8c99e` (+ uncommitted harness_proof bin/docs). 156 lib tests. Ubuntu ✓ macOS ✓ Windows ✓.

**Single source of truth for the plan:** `LAPUTA HOW TO/LAGADO_MASTER_PLAN.md.pdf` (June 3, 2026).

### UI ↔ Backend wire (verified)
`useTauriAgent.ts` → direct `invoke()` calls → Tauri commands. Events back via `app.emit()` / `listen()`.
`server.rs` WebSocket (port 9090) and `useAgentSocket.ts` are **orphaned** — dev scaffold for UI design iteration only.

### Full memory system (complete)
- **Hot → warm**: sleep_gate batches cooled hot entries → LLM summarizes → warm SQLite
- **Entropy pruning**: `V = T × e^(−λt) × (1 + ln(n+1))`; warm pruned at 10,000; cold never touched
- **Skill distillation**: Done/Task episodes → `distill_skill_async()` → LLM → `skill_library.save()`
- **Visual similarity**: frame encoded at episode boundaries → cosine similarity retrieval
- **action_graph**: muscle memory (exact hash bypass at score ≥ 0.65)
- **skill_library**: advisory depth context, turns 1-3 only

### What works end-to-end
- App launches → Awakening → auth → chat
- `send_goal` → hydra → 1.2B classifier → agent_loop (episodic + visual + skill context turns 1-3, tools every turn)
- At Done/Task: skill distilled + visual embedding stored
- Every 5 min: sleep_gate consolidates hot → warm → entropy prunes if over limit
- Immersive → VM auto-boots → live QEMU desktop feed → SSH → xdotool actuation
- RecoveryManager, SleepGate (full consolidation), ServerGuard, cgroup v2 sandbox all active
- 44 bundled native Rust tools, MCP stdio client, confidence gating, HITL gate

### VM control channel — end-to-end tested 2026-06-11
Smoke-test bin `lagado-agent/src/bin/harness_proof.rs` drives the real modules:
`QemuDesktopBackend::boot → poll SSH → SshPerceptor::read_screen → QmpClient screendump →
SshActuator → FrameProcessor delta → backend.shutdown`. Run with
`LD_LIBRARY_PATH=…/vendored/llama.cpp-2/build/bin LAGADO_DATA_DIR=~/.laputa-secure ./target/debug/harness_proof`.

**Result: agent code is sound; the VM was un-provisioned for the agent's SSH control model.** The
agent's ONLY VM control channel is SSH (`ssh -o BatchMode=yes`, key auth). It was fully broken by the
guest image, now fixed in `~/.laputa-secure/vm-images/` (cloud-init.yml + rebuilt seed.iso, originals
backed up). Fixes applied: DHCP eth0, `ufw allow ssh`, install host pubkey, generated host keypair
`~/.ssh/id_ed25519`. Now: boot → SSH (~14–24s) → AT-SPI2 tree read → QMP screendump → clean shutdown all work.
**Raw actuation proven**: `xdotool mousemove/click/type` over SSH changed the screen (1076-px diff).

**VM control channel — FULLY PROVEN end-to-end 2026-06-17 (Fedora 44 rebuild).** `harness_proof`
green on all 8 stages through the real modules in 24.9s: boot → sshd (11.8s) → X(1280×800) →
SshPerceptor **8 coords/8 bboxes** → QMP screendump → SshActuator **`click('ref_4')→Clicked at (1140,13)`**
(click-by-selector via coord cache) → FrameProcessor delta (6 cells) → clean shutdown. Guest = Ubuntu
24.04 cloud image + XFCE/autologin/SSH/AT-SPI2/xdotool/tine, provisioned via cloud-init. Reproducible:
`vm-provision/build-guest.sh` (downloads base, builds `seed.iso` with the host pubkey, makes the 20G
working disk). Images live in `~/.laputa-secure/vm-images/` (outside the repo); `vm-provision/` is committed.

**Prior open gaps — ALL RESOLVED 2026-06-17:**
- ✓ `tine` zero-elements: root cause was **pipx isolation**, not a format mismatch — perceive.py runs
  `python3 -m tine.cli` (system python), which can't import a pipx venv. Fix: `pip install
  --break-system-packages tine-cli` (in cloud-init). The pip ref-cache format DOES match perceive.py.
- ✓ click-by-selector: works (`Clicked ref_4 at (1140,13)` through the coord cache).
- ✓ `SshPerceptor` `--focused`: emits parseable `(x,y,w,h)` now that tine is reachable.
- ✓ `QemuDesktopBackend::boot()` kill-stale pre-flight: present and exercised.
- ✓ VM readiness: `harness_proof` gates on real BatchMode `whoami` SSH-auth, not bare TCP.

### Perception fusion harness (TASK 6 code complete, committed)
- TASK 6 ✓ — `perception/arbiter.rs` IoU-dedup fusion (commit 0c8c99e): `iou()`, `fuse(a11y,cv,patches)`,
  `Sense{A11yOnly,VisionOnly,Both}`, `FusedElement`. MATCH_THRESHOLD=0.30 (loose), ±1 patch inflate fuzz,
  overview-skip, mean-pool overlapping patch embeddings, deterministic (y,x,w,h) sort. 156 lib tests.
  Still to close: CV real-screenshot noise gate (cv_measure on content-dense frame). TASK 7 next.
- TASK 1 ✓ — full bbox retained (`parse_ref_bboxes`, `PerceptionCache.bboxes`)
- TASK 2 ✓ — pixel-space DeltaDetector (decoded RGB, remainder → last col/row) + FrameProcessor
- TASK 3 ✓ — CV box proposer (Canny + connected components, imageproc 0.27) + cv_measure binary
- TASK 4 ✓ — decoder_pos flat (LFM2 1D not 2D); 1280×800 → 3×2 grid + overview; ordering verified by marker test
- TASK 5 ✓ — `lagado_encode_image_patches()` + `encode_png_patches()`; `is_overview` by structural position (img_idx ≥ grid_cols×grid_rows); 1025×1025 probe proved token-count detection fails
- TASK 6 ⬅ NEXT — `perception/arbiter.rs`: IoU-dedup → `FusedElement` / `Sense` enum (threshold <0.5, ±1 patch fuzz)
- TASK 7 — `perception/harness.rs`: `PerceptionMode`, CSV measurement log, conditional wire

### Remaining (against 7-segment PDF plan)
- **Segment 1** — Browser extension Backend #1: DOM perception + actuation (cross-platform ON-RAMP)
- **Segment 5** — Egress proof + `security/profile.rs` (Strict/Balanced/Open tiered profile)
- **Segment 6** — Immersive watch-and-direct loop (partial)
- **Segment 7** — Native desktop perception for Mac/Win (stubs exist)
- Settings tool manager: get_tools, set_tool_trust, toggle_tool_enabled
- GGUF MoE parser (auto-set moe_experts_on_cpu)
- grammar.rs GBNF constraint (accuracy lever, currently stub)
- security/audit.rs (tamper-evident append-only log)

## Harness doctrine (2026-06-14) — direction for the rebuild

Full plan: `docs/plans/LAGADO_HARNESS_DOCTRINE_AND_PLAN_v1.md`. **Execution spec (current build):** `docs/plans/LAGADO_HARNESS_BUILD_SPEC_v1.md`. Verified LFM facts: `/home/alucard/projects/research/LFM research.txt`.

**Build progress (2026-06-16, Fedora 44 rebuild):** CUDA llama.cpp built; models on disk; GPU inference proven (8B-A1B Q4 full-offload, 188 tok/s). **The spine:** deterministic floor that always works + model upgrade when affordable, governor-arbitrated, off the hot path (recurs in router/importance-gate/conduction/perception/model-modes). Work order: **① DONE** grammar-constrained router + 8B fallback (closed the silent UNPARSED→CHAT hole; `generate_constrained` on the adapter). **② DONE** G3 baseline — Jaccard F1=0.43/R=0.75 → ColBERT-350M mean-pool cosine F1=0.52/R=0.92 (the Board relevance path; MaxSim deferred). **③ DONE** the Board (Park scorer, relevance normalized, ColBERT embedder live) — but NOTE the action-selection findings below now keep it OUT of the action path. **④ DONE** single-turn loop + `supervisor.rs` escalation ladder. 214 lib tests.

**ACTION-SELECTION ARCHITECTURE (2026-06-17, closed-loop after 14 experiments + adversarial review).** Full record + every experiment: `docs/plans/LAGADO_ACTION_SELECTION_OPEN_QUESTION_v1.md` (§2.1–2.15) + `docs/plans/experiments/`. The 8B is dominated by prepended-text content, so the design uses the LLM ONLY for single-step target selection and makes everything it fails at deterministic:
- **Executor (built, live):** memory-isolated prompt (SYS + candidates + goal) + deterministic **late-band ranking** (`selection::rank_late_band` — the relevant candidate goes LAST, where label-reading holds) + grammar rail (`selector_grammar` over per-frame `el_N` index + escape) + **deterministic fail-closed** (`selection::goal_matches_any` — the model emits the escape token 0/12, so the harness gates: no label-match → re-perceive). Live walk: clicks the right element, menu opens.
- **Q1 action-effect (built, live):** same-action + screen-changed = accomplished → stop re-deriving (complement of `should_cutoff`). Fixed the re-click. Also the (to-wire) pointer-advance signal for multi-step.
- **Completion / multi-step (CORE BUILT + LIVE, commit 37587c2; refinements pending):** the LLM CANNOT plan/decompose (spurious `complete` even handed explicit progress, §2.14) → **deterministic sequencer**. BUILT: `decompose_goal` (split on explicit sequential markers; semantic-compound stays one sub-goal — no mangled plan) + `sub_goals`/`current_sub` pointer + pointer ADVANCES on action-effect (structural a11y screen change) + deterministic completion when the plan is exhausted. **2-step VM walk passes** (open menu → launch terminal, 2 clicks). NO Board memory in the loop (inv #10). UPDATE 2026-06-18 (§2.20): now there IS a Board-informed LLM **planner** (`plan_goal`, upstream of the memory-isolated executor — influence not in-loop) for implicit goals; the §2.15 **effect-signature POSTCONDITION + observe-until-quiet settle + deviation→escalate are BUILT**; the **precondition skip is the one banked piece** (Option 2). See §2.20 below.
- **Parked (v2):** Tier-2 label-less CV/vision-only element selection (relational / cross-modal descriptor). Correctly non-blocking — those elements escalate.

**STRESS TEST + REFRAME (2026-06-17, §2.18 — the current direction).** Stress (8 OSWorld-style tasks × 6, execution-verified): terminal 2-step 12/12; file-manager/browser 2-step 0/12 (step-1 clicked "Directory **Menu**" for "open the Applications **menu**" — lexical decoy pull); implicit goals → clean handback. Natural-intent gap proven: token-overlap 0/5, embedding ~1/5, **model world-knowledge intent→app classification 4/5 deterministic**. **THE REFRAME (Opus):** the floor selection miss and the router miss are ONE failure (salience/lexical pull, not semantics) at two altitudes — solve ONCE as **constrained-vocabulary selection + deterministic fail-closed**. "Deterministic 8/8" = consistent, NOT reliable. Root cause of 0/12 = the sequencer leaks a distractor token in the sub-goal string ("…menu" ↔ "Directory Menu"); **fix at sub-goal PHRASING (lead with the discriminating token, strip the colliding category noun), deterministic, at the sequencer** — not a new ranker, not verification (both retired). **BUILD ORDER:** (1) sub-goal-phrasing probe [next, ~free, the root cause]; (2) router = intent→curated-capability + fail-closed against the set (NOT confidence-gate) → unlocks natural intent; (3) ship a11y-floor + router: **reach-and-operate, judging handed to the human** (sovereignty line: "reaches, doesn't judge"); (4) THEN DOM as 2nd mastered surface, switch edges CAPABILITY-DRIVEN/deterministic (the launch procedure declares its surface; Brandon's browser-extension DOM mode); (5) THEN vision Tier-2/3 last-resort, delta-cache-gated, ONLY after a visual-embedding DISCRIMINATION probe (efficiency≠the question; whether patches separate is). Dynamic sense-switching is the ENDPOINT, one measured surface at a time. Cloud tier (judging-half only, opt-in) is v2+, NEVER the perception/action path. Vision-blob pieces (arbiter `patch_embd`, `encode_png_patches`, blake3 delta detector) all BUILT but unwired (`agent_loop` fuses a11y only).

**FLOOR-MASTERING (2026-06-17, §2.19 — DONE: selection reliable).** Three measured fixes, all verified fresh (terminal+filemanager PASS, browser clicks the matched target): (1) `selection::discriminating_phrase` — strip the verb + colliding category noun from the sub-goal goal-slot (kills the step-1 decoy leak); (2) `selection::best_match_token` + the SELECTION-INTENT DIVERGENCE rail in `agent_loop` — if the model clicks a different element than the unique best-match, fail closed before acting (kills false-completion at the source; makes completion the honest instrument); (3) **`rank_late_band` RE-TOKENS by render position** — ⚠️ **the model attends to the HIGHEST token number / last item, NOT the last-RENDERED row; ranking reorders the display but if tokens stay spatial the target's token isn't the max and the model picks the max-token item instead (0/12). Re-tokenization makes the most-relevant target carry el_{n-1} (12/12). DO NOT delete it as redundant — the late-band fix silently dies if you do.** **NEXT BUILD = the effect-signature (the two open items — Firefox-slow-paint and "right element clicked, effect didn't follow" — are ONE: act≠effect).** It defines what "done" OBSERVES (window/top-level node appeared, title changed, region delta), CONTAINS the settle as poll-until-fire-or-timeout→escalate (NOT a fixed per-app sleep), must distinguish "goal already satisfied on entry" from "nothing happened" (compare goal-vs-CURRENT-state), and takes completion-detection OUT of the model's hands (the §7c answer — done-detection grounded in observed world-change). **SPEC HOLE to build deliberately: "observes a world-change" hides "decides WHICH change counts" — a goal/action→expected-signature MAPPING. Lean deterministic action-type→signature (click-launcher→window, type→field-delta, click-menu→region/tree-delta), fail-closed on unmapped; it MUST NOT be the model asserting its own completion condition (the authority the divergence rail removed). The mapping + the satisfied-on-entry check are the hard parts; polling/timeout are easy.** Then harden the harness (periodic VM reboot — the per-run reset degrades over many runs), then stress for pass-rates that mean GOALS ACCOMPLISHED not clicks landed.

**EXECUTION-HARDENING ARC (2026-06-18, §2.20 — DONE: the act≠effect line closed; suite at the real 9/10 ceiling, only the structurally-unwinnable no-mail-app task fails).** Full record: `docs/plans/LAGADO_EXECUTION_HARDENING_v1.md`. Built and verified, in this order:
- **Multi-sense perception (Phase 1):** arbiter owns label provenance (`LabelSource` a11y>caption>OCR>None); live CV wired into `fuse()` via `cv_proposer::propose_frame` (fail-open); **`selection::LATE_BAND_CAP=64`** label-aware cap (sheds inert label-less CV first, never drops a goal-matching labeled target). Gate PASSED: a11y+CV == a11y-only, zero selection regression (CV is inert until Phase-2 captions; ships as free coverage).
- **Board-informed planner (Wall 1):** `agent::plan_goal` — upstream LLM step, informed by learned skills (`skill_library.retrieve`), expands an IMPLICIT goal's preconditions (e.g. "Launch Terminal" → [open menu, click Terminal]); explicit "X then Y" keeps the deterministic split. **store-vs-INFLUENCE:** memory shapes the PLAN, never the executor's click (inv #10). Fixed implicit-task fail-closed stalls.
- **Action-aware executor (Wall 2):** sub-goals carry a `SubAction` class (`classify_subgoal`): Click → selection loop; **Type/Key → deterministic one-shot through the safety gate, fire-and-advance** (no selection/fail-closed; type targets `selector="focused"`). The loop was click-only before. + intent-classifier deterministic fast-path (`hydra::opens_with_action_verb`) — an action-verb-leading message routes Interactive without asking the weak 1.2B (which misrouted long action chains to Chat → silent no-op).
- **act≠effect (the spine):** **`effect_confirmed` / `EffectClass` (§2.15 POSTCONDITION)** — advance on the action-class structural signature, not any delta. `Open` (reveal a menu) confirms only when elements APPEAR (direction-aware → toggling an already-open menu shut no longer false-advances); `Activate` = any-change catch-all. THEN **`observe_until_quiet`** replaced the fixed settle ceiling: terminate the settle on an OBSERVED signal (world goes quiet = N stable observations) not a clock — `settling_active` = a11y churn OR frame-delta pixels>noise (reuses `DeltaDetector`); the only clock left is the far-outer backstop. Fixed the term-type cold-start race (6/6 reliable, was 2/2-then-fail).
- **CV production frame-sync:** `Perceptor::capture_frame()` (default no-op; `SshPerceptor` does a QMP screendump) — the loop captures the frame at the perception instant on the settled state, so CV reads an in-sync image not a stale UI-polled one. Harness QMP feeds removed (capture_frame replaces them). **CV is now production-ready, shippable enabled.**
- Plus strategist directives: lexical-union ranker (`tokens_match` substring/prefix ∪ exact, max — premise that the ranker was embedding-based was FALSE, it's already lexical; ColBERT stays out of the action path); memory-isolation guard test (`build_executor_prompt` is inv-#10-isolated by construction).
- **BANKED / OPEN:** Option 2 = the precondition already-satisfied SKIP (now SAFE to build — the postcondition is its net; bake in the uniqueness-on-settled guard: skip only on a UNIQUE `best_match_token` in a settled set). The §2.15 `Launch` class mostly DISSOLVED into observe-until-quiet (advance fires on `focus==target`); a focus-to-new-window gate is a small follow-up only if a real blank-gap failure appears. NOT exercised: an extreme injected-slow-action test + a live hung-app escalation. Suite runs slow (observe-until-quiet polling + the Option-2-able menu oscillation).

**CAPABILITY-LAYER + GUEST + MODEL ARC (2026-06-20, commit ffd9ce9 — full record `docs/STRESS_RUN_2026-06-20.md`; memory `lagado-capability-layer`, `lagado-fedora-cinnamon-guest`, `lagado-react-reflex-architecture`).**
- **Guest VM migrated Ubuntu/XFCE → Fedora 44 + Cinnamon** ("closest to Windows" but keeps GTK/AT-SPI; Windows-first product). `vm-provision/build-guest-fedora.sh` + `user-data-fedora`; `vm/mod.rs` default disk = `lagado-guest-fedora.qcow2`. Fedora fixes: ssh unit is `sshd` not `ssh`; SELinux→permissive + firewalld off (disposable sandbox); evdev/tine needs `gcc python3-devel kernel-headers` (Py3.14 no prebuilt wheel). KEY GAP: GUI element-level a11y is FLAKY on Cinnamon (nemo buttons get no refs) → confirms LEAN ON THE TERMINAL/CLI for file-ops, a11y/GUI only for forms.
- **CAPABILITY LAYER (App-Intents equivalent) — wired into production.** The model SELECTS a typed verb via a PYTHONIC GBNF grammar (`grammar::capability_grammar`, LFM2-native `[move(source_dir="…", …)]`, SOURCE+DEST paths bound to the observe listing); the harness does resolve→exec→verify (`agent::capability_prompt/parse_capability_call/capability_to_command`). 6 file-ops verbs (move/copy/rename/make_folder/delete/extract_to_file) + write_file. Validated ~2× free-form authoring (capped at ~2/8). **GBNF GOTCHA:** llama.cpp SILENTLY DROPS a grammar whose top alternation is BARE RULE REFS (`call ::= c_move | …`) → must be TERMINAL-LEADING inline. This made every "grammar-enforced" run secretly unconstrained until fixed.
- **MODEL: stays gen2 `LFM2-8B-A1B`.** gen2.5 `LFM2.5-8B-A1B` evaluated and REJECTED for our path — it has THINKING baked into the WEIGHTS (emits `<think>` even with no-think template / `--reasoning-budget 0`), which is INCOMPATIBLE with direct grammar-constrained tool emission (output must start with `[`). `LFM2-1.2B-Tool` = grammar-compatible (no-CoT) but weak comprehension cold.
- **FORWARD DIRECTION (the user's synthesis):** REASONER+EMITTER split — gen2.5 (thinking) reasons → a no-CoT emitter (1.2B-Tool → 350M → eventually DETERMINISTIC code; grammar+harness compensate) emits the bound call. `reason_emit_probe` bin BUILT, not yet run; it doubles as the RLVR data factory. ENDGAME = a FAT-FREE harness-native model (strip the general "fat") + an AI-NATIVE OS (declared capabilities, kernel-level structured access). **PAUSED awaiting a deep-research pass (handed to the user) on whether these already exist — DO NOT build further until it lands.**
- **New benchmark bins:** `user_stress`/`hard_stress`/`osworld_real` (real OSWorld task instructions, world-state-verified) + `capability_probe`/`react_loop_probe`/`reason_emit_probe`/`discover_probe`. The batteries ARE the verifiable-reward harness for any future fine-tune.

**SPRINT PLAN (2026-06-20 — WEEKS-scale, ASAP. Adopt EVERY proven component the research cited; reinvent nothing. Full landscape: `research/The Sovereign On-Device Computer-Use Agent…` PDF + memory `lagado-sovereign-landscape`.)** The research VALIDATES the thesis: a co-designed sovereign + OFFLINE + REGULATED agent does NOT exist — it's white space. The reason weeks is plausible: we ASSEMBLE proven parts (MCP, small action models, documented techniques) + build the ONE novel thing (a single-harness narrow-vocab fat-free model). **Moat = compliance + harness RELIABILITY, not model capability. THE hard problem = MULTI-STEP autonomy (everyone fails — LocalCowork 26% cross-server) → the harness + narrow vocab is the bet.** Phases by leverage:
- **0. BASELINE (2026-06-20)** — gen2 `LFM2-8B-A1B` + the now-correct grammar (terminal-leading + source/dest path-binding) = **capability_probe 7/16** (vs free-form ~2/8 user + 1/8 osworld). The line we improve from.
- **1. ADOPT MCP (stdio)** as the capability wire format — reuse `mcp/client.rs`; our Pythonic verbs → MCP tools (JSON Schema); KEEP the grammar/resolve/verify harness. Standard, offline-native, interops with Windows App Actions. DON'T invent a format.
- **2. EMITTER — LICENSE REALITY (VERIFIED 2026-06-20, the research was WRONG):** EVERY off-the-shelf tiny action model is NON-COMMERCIAL — Hammer2.1-0.5b, xLAM-1b-fc-r, Octopus-v2 are ALL `cc-by-nc-4.0`; LFM is custom `lfm1.0`. NONE is shippable in a regulated product. They are FINE for RESEARCH/dev (prototype the reason→emit + data-gen with the 1.2B-Tool/Hammer). For the SHIPPED emitter there is NO shortcut → FINE-TUNE OUR OWN on a permissive Apache-2.0 base: **Qwen2.5-0.5B/-1.5B or SmolLM2-360M/1.7B** (VERIFIED apache-2.0). This COLLAPSES Phase 2 into Phase 6 — the emitter must be trained on our action vocab; off-the-shelf only serves research + the gen2 baseline. Threshold ≥85% single-step tool-select <500ms on OUR action set.
- **3. REASON→EMIT** — gen2.5 (thinking, the reasoner — its `<think>` is baked in the weights, INCOMPATIBLE with direct grammar emission, so use it ONLY to reason) → small grammar-constrained emitter. `reason_emit_probe` = the bridge AND the data factory.
- **4. MULTI-STEP RELIABILITY (the hard problem = our edge)** — route conversational goals INTO the capability loop (the osworld 0/8 routing gap); tighten `goal_completion_checks` (kill false-successes); no-progress/oscillation rails; cross-step state (persistent shell, built). Reliability comes from the HARNESS, not the model.
- **5. COMPLIANCE MOAT** — zero network egress (`security/profile.rs` Strict tier), AES-256 at rest (have `security/crypto`), tamper-evident action audit log (`security/audit.rs` — TODO), deterministic guardrails SEPARATING intent-understanding from action-execution (the gate + the reason/emit split already do this). These are the regulated-buyer criteria + the moat vs cloud incumbents.
- **6. THE SPECIALIZATION (defensible moat; technique now, TRAINING is GPU-gated)** — fine-tune/distill a ≤0.5B model EXCLUSIVELY on OUR action vocabulary: Octopus-style FUNCTIONAL TOKENS (one token/declared action) + Hammer FUNCTION MASKING + no-CoT + grammar-constrained decoder. Reward data = the batteries + reason→emit trajectories (already generating). The 6 GB laptop CANNOT train an 8B/even-1B well → cloud rent / partnership; set up DATA + TECHNIQUE now so training is turnkey when compute lands. NO public model is trained on a single sovereign harness's vocab — this is the white space.
- **PARALLEL RESEARCH (not the core):** ODE-liquid-net "hands" — `ncps` (Apache-2.0), CfC + linear/softmax over a fixed action set; benchmark vs the transformer on the SAME vocab; promote ONLY if it matches at lower latency/params. Zero prior art for tool-calling (genuine novelty, architectural-fit headwind). LFM2 ≠ ODE net.

**The harness is the moat; the model is swappable** (`InferenceAdapter`). LFM2 is NOT a continuous-reflex ODE net (that's the LTC/CfC/NCP drone line) — it's a discrete edge-CPU transformer-hybrid. Use it for edge efficiency + shippable license + agentic variants + cheap fine-tune, not for "liquid" magic. The word "Liquid" must not load-bear in an architecture decision.

**Core problem (verified):** small models degrade over multi-turn history (~0.63⁵≈10%/5 turns; premature commitment; no recovery; temperature doesn't help). So:
- **Externalize state; every model step is single-turn-fresh.** Re-present a clean, fully-specified slice each step. The slice-assembler is **deterministic code, not a model call.** Mitigate the re-encode cost with llama-server `/slots` KV-prefix reuse (seam exists in `inference/mod.rs`, stubbed).
- **The "board" = a standard scored memory store (Park / Generative Agents), NOT a physics engine.** `score = α·recency + β·relevance + γ·importance`, recomputed stateless per step, top-k. **`memory_tiers.rs` already implements recency (`information_value`) + relevance (`find_similar_by_embedding`) + top-k** — extend it (add importance + one scorer + wire as slice-assembler), don't rebuild. Hot tier in `/dev/shm` (zero-copy, already the frame path). **Conduction (ACT-R spreading activation) OFF by default** — add only if a retrieval eval proves it earns the complexity.
- **Retrieval ≠ planning.** Board surfaces candidate ingredients; a separate, named, **deterministic sequencer** does ordering/dependencies.
- **`supervisor.rs` = reset-from-corrected-board + bounded-retry escalation ladder** (N retries → 8B → optional cloud → HITL). Not "think harder."
- **Born flightworthy via LEARNED pipes** (record traces → promote to action-graph), not 25–30k hand-authored entries (they rot like perceive.py's DOM assumptions). Seed thin (~50) if at all.

**Four gaps that are hard requirements:** G1 eviction/archival tier (cool-don't-delete needs a disk tier, not infinite RAM); G2 write-quality/importance gate; G3 retrieval eval set (build BEFORE tuning α/β/γ); **G4 particle trust tier** (perceived DOM/screen text → board → model context is a prompt-injection vector; tag `perceived-untrusted` vs `user-intent-trusted` — the perception-side analog of the HITL gate; critical for the browser surface).

**Steal, don't invent:** ACT-R (1983), Park Generative Agents (2023), Hearsay-II blackboard (1980). Convergence = the shape is right, not that we're first. Invention budget goes ONLY to the LFM2 edge-CPU single-turn-reset harness. **Build the boring stateless version; let the eval decide if anything fancier earns its keep.**

Open decisions (need user): QEMU vs libkrun (research gate); board embedding source; extend memory_tiers vs new organ; G2 deterministic-vs-model importance; sequencing.
