# Architecture reference (moved out of CLAUDE.md, 2026-07-18)

This is the detailed architecture material that used to live in CLAUDE.md — runtime wiring, the
Hydra pipeline, VM/auth/memory subsystem detail, the module table, and the UI design system. It
was moved here so CLAUDE.md stays a working guide for the harness arc, not a reference manual.

**Staleness warning:** parts of this predate the harness arc. `docs/CURRENT_STATE.md` is the
verified-against-code source of truth and WINS on any conflict. Known issues in the module table
below: it omits the harness-era load-bearing modules (`plane.rs`, `native_session.rs`,
`api_plane.rs`, `back_door.rs`, `board.rs`, `envelope.rs`, `forge.rs`, `gguf.rs`); it
green-checks `distill.rs` and `self_model.rs` whose wiring is unconfirmed (a 2026-07-18 review
found zero lib references); `operator.rs` is legacy, superseded by `tools/mod.rs` + `gate.rs`.

## Runtime

Single Tauri binary (`lagado-ui/src-tauri/`) wraps:
- React/shadcn UI — Liquid AI inspired, deep navy + blue/purple glassmorphism
- Rust agent core (`lagado-agent/` as a library)
- Vendored `llama-server` subprocess (HTTP inference on :8080, NOT FFI) — the brain (model-swappable)
- Classifier subprocess on :8081 (LFM2.5-1.2B-Instruct, intent classification, CPU-only)
- Embedder subprocess on :8082 (LFM2-ColBERT-350M, `--embeddings --pooling mean`, CPU-only) — the Board's relevance signal; spawned in `main.rs`, watched by `server_guard`, fed by `sleep_gate` backfill, consumed by `agent_loop` via `assemble_slice` (recency floor when down)
- Visual encoder: in-process `libmtmd.so` FFI (LFM2-VL-450M + mmproj, vision → embedding vectors, no subprocess)
- QEMU desktop VM (agent's sandboxed working surface)

Inference: HTTP to `llama-server` → `/v1/chat/completions`. **The brain is model-swappable — the
harness is the moat, not the model** (proven: swapping to a different same-size model raised the
score, harness unchanged). The OSWorld harness work benchmarks on **Qwen2.5-Coder-7B**
(`start_brain.sh`); the LFM2 family below is the eventual *shipping* intent for the app, not the
current benchmark brain.
Models in `~/.laputa-secure/models/` (the LFM2 app-shipping set — NOT the benchmark brain):
- `LFM2-8B-A1B-Q4_K_M.gguf` — app-shipping agent model (gen2, temp 0.3/min_p 0.15)
- `LFM2.5-1.2B-Instruct-Q4_K_M.gguf` — intent classifier (gen2.5, temp 0.1/top_k 50)
- `LFM2-VL-450M-F16.gguf` + `mmproj-LFM2-VL-450M-F16.gguf` — vision encoder
- `LFM2-ColBERT-350M-Q4_K_M.gguf` — Board embeddings (Phase 1)

## Agent pipeline (Hydra orchestrator)

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

## VM Architecture (Phase 1.4 — COMPLETE)

```
QemuDesktopBackend boots qcow2 with QMP socket + VirtIO display
         ↓
Perception: SSH into guest → perceive.py (AT-SPI2 on Xorg) → PerceptionCache (ref_id → cx,cy)
Actuator:   SSH into guest → xdotool mousemove cx cy click 1 (coords resolved from cache)
Live feed:  QMP screendump (format:png) → /dev/shm/lagado_frame.png → base64 → Immersive canvas
```

**Guest image:** `~/.laputa-secure/vm-images/lagado-guest-fedora.qcow2` — **Fedora 44 + Cinnamon**
(GTK/AT-SPI2; KDE/Qt deliberately avoided), built by `vm-provision/build-guest-fedora.sh`
**Seed ISO:** `~/.laputa-secure/vm-images/seed-fedora.iso` (cloud-init NoCloud, first boot only —
dnf-installs the desktop, takes several minutes)
**Guest:** user `laputa`, auto-login (lightdm), SSH key auth (host `~/.ssh/id_ed25519`, BatchMode),
xdotool + AT-SPI2 provisioned. Cinnamon GUI a11y is flaky — lean on terminal for file/doc work,
a11y for forms. Control-channel proof after first boot: `cargo run --bin harness_proof`
**QMP screendump** — `format:png` required (default is PPM).
**SSH readiness:** `vm_ssh_port` set asynchronously via background SSH auth probe (`ssh whoami` → exit 0 + stdout contains "laputa").
**Frame path:** `/dev/shm/lagado_frame.png` — constant `config::FRAME_PATH`.
**Auto-kill:** `Drop for VmHandle` + `KillOnDrop` wrapper kills all child processes on app exit.

## Auth (Phase 2 — COMPLETE)

**Wrapped DEK scheme** — FileVault/1Password pattern:
- Signup: random 32-byte DEK wrapped with `Argon2id(password)` + `Argon2id(recovery_phrase)`
- Both blobs in `~/.laputa-secure/config/keychain.json` — raw DEK never touches disk
- Login: Argon2id(password) → unwrap DEK → `auth::set_session_dek(dek)`
- Lockout: 3 failures → 10-min cooldown, persisted, fail-closed if tampered
- `auth::active_key()` is the only crypto entry point; falls back to `machine_passphrase()` in dev

## Memory system

*(2026-07-18 accuracy note: episodic memory is deliberately EXCLUDED from the action path —
invariant #10; entropy pruning exists but is not called from anywhere; MemoryReset unimplemented.
"Operational" below refers to the consolidation cycle, not the action loop.)*

- Hot entries → sleep_gate → LLM batch summarize → warm entries → entropy prune at 10,000
- Entropy equation: `V = T × e^(−λt) × (1 + ln(n+1))`, λ = ln(2)/30days (Ebbinghaus + log reinforcement)
- Cold (vault) never entropy-pruned; 365-day natural half-life protects it
- Skill distillation: Done/Task episodes → `distill_skill_async()` → LLM extract → `skill_library.save()`
- Visual embeddings: frame encoded at episode boundaries → cosine similarity retrieval
- `SleepGate::new(memory, adapter)` — full consolidation cycle every 5 min
- DB: `~/.laputa-secure/memory.db`, `~/.laputa-secure/skill_library.db`

**Phase 3.3 — Visual embedding via in-process libmtmd FFI:**

```
Frame (PNG) → vision/shim.c (lagado_encode_image) → mean-pooled n_embd vector
                                                            ↓
                                               MemoryTiers embedding BLOB column
                                                            ↓
                                           cosine similarity retrieval at query time
                                                            ↓
                                      top-K visually similar past episodes → agent context
```

Key implementation facts:
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

## Key modules (`lagado-agent/src/`) — see staleness warning at top

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
| `self_model.rs` | wiring unconfirmed | Accepted beliefs, distill feed |
| `distill.rs` | wiring unconfirmed (0 refs found) | Replay manifest for Phase 3 QLoRA |
| `perception/mod.rs` | ✓ | PerceptionCache (coords + bboxes), VlmPerceptor retired |
| `perception/linux.rs` | ✓ | AT-SPI2 via perceive.py --focused, populates both coords and bboxes |
| `perception/delta.rs` | ✓ | Pixel-space blake3 per cell — decoded RGB, remainder → last col/row |
| `perception/frame.rs` | ✓ | FrameProcessor: PNG→RGB→DeltaDetector, stateful, reset() between sessions |
| `perception/cv_proposer.rs` | ✓ | Canny + 8-connected components (imageproc), ProposalResult, extract_cell_rgb |
| `perception/vlm_adapter.rs` | kept | Text path kept for reference; not used in agent pipeline |
| `perception/arbiter.rs` | ✓ | IoU-dedup fusion → `FusedElement` (a11y+CV+DOM+vision), per-frame index space. a11y always live; CV default-on since the 2026-07-08 redesign (`config::cv_enabled()`, kill-switch `LAGADO_CV_DISABLE=1`); DOM gated behind `LAGADO_DOM` pending A/B. See `docs/CURRENT_STATE.md` §1 for the per-sense default table. |
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
| `operator.rs` | legacy | StepEnforcer, ToolDescriptor, RiskLevel — superseded by `tools/mod.rs` + `gate.rs` |

*(Missing from this table, added post-hoc as a pointer: `plane.rs` — deterministic plane picker +
dispatch table; `native_session.rs` — resident UNO session plane (Calc); `api_plane.rs` — stateless
UNO floor; `back_door.rs` — config/D-Bus plane, gated; `board.rs`, `envelope.rs`, `forge.rs`,
`bracket_parser.rs`, `gguf.rs`, `kv_slots.rs`, `liquid.rs` (stub). See `docs/CURRENT_STATE.md`.)*

## UI (`lagado-ui/src/`)

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
