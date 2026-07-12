# Lagado — Architecture Brief (for outside prior-art survey)

**Date:** 2026-06-17 · **Scope:** the Lagado *agent app* only (excludes the custom-OS / "Laputa" work and the demonstration-capture "Lens", which is designed-not-built). **Source of truth:** verified against the codebase at HEAD, file-cited. Where the code contradicts prior verbal description, the **code wins** and the discrepancy is flagged ⚠️. Status labels: **BUILT&VERIFIED** (wired + exercised end-to-end or unit-tested) / **BUILT-UNVERIFIED** (code exists, not exercised in the live path) / **IN-PROGRESS** / **PLANNED** / **STUBBED** (dead code).

Crate root: `lagado-agent/` (Rust, ~34 modules); host: `lagado-ui/src-tauri/` (Tauri). Paths below are relative to `lagado-agent/src/` unless noted.

---

## 1. Product thesis

A **fully-local, offline computer-use agent**: a small model drives a real GUI (perceives the screen, clicks, types) to accomplish user goals, with **no cloud path of any kind** — inference, memory, keys, and the agent's working surface all live on the user's disk. Privacy is guaranteed *by architecture* (there is no egress to disable), not by policy. The model is deliberately small (8B MoE, ~1B active); the engineering bet is that a **deterministic control harness** — memory, recognition, recovery, a human gate, and rails that constrain the model to one reliable decision per step — closes the gap a raw small model can't. The intended market is consumer/edge (runs on hardware people already own), which is a *business* assumption, not a settled one (see §11).

## 2. Hard constraints (non-negotiables, enforced as invariants)

- **No cloud path.** Inference is local `llama-server` subprocesses; hybrid/cloud model tiers are STUBBED. (`DESIGN.md §1`, `CLAUDE.md` runtime.)
- **CPU-deployable *target*.** The LFM2 family is chosen because its architecture was NAS-searched for embedded-SoC CPUs. ⚠️ *Current dev/verification ran on a 6 GB GPU (RTX 3060) at ~188 tok/s; pure-CPU latency for the 8B-A1B is unverified.* The "CPU floor" is a stated target, not a measured baseline.
- **Single-turn-fresh.** The intent router and the executor are re-prompted clean each step; conversation history is treated as a contaminant (invariant #2: `classify_intent()` gets only the current message). Rationale is measured small-model multi-turn degradation (~0.63⁵≈10% over 5 turns).
- **Determinism on the rails, not the strategy.** The model makes one local choice per step; planning, ordering, completion, escape, and memory-handling are deterministic harness code (`supervisor.rs`, the sequencer in `agent.rs`). The doctrine is explicit in `supervisor.rs:1-20`.
- **HITL chokepoint.** Every action passes `gate::evaluate_action()`; never bypassed (invariant #3).
- **No Board/retrieved memory in the action-selection prompt** (invariant #10) — see §7, measured rationale §4.
- **No hardcoded model/hardware values** (invariant #9) — discover from GGUF or defer to the governor.
- **No AI attribution** in any artifact (commits authored `Lagado Labs`).

## 3. Model layer

All inference is **HTTP to vendored `llama.cpp` server subprocesses** (crash-isolation, hot-swap), except the vision encoder which is **in-process FFI**. (`DESIGN.md §11`.) Files: `config.rs`, `bootstrap.rs`, `server_guard.rs`, `governor.rs`, `inference/`, `vision/`.

| Model | File (GGUF) | Role | Transport / flags | Status |
|---|---|---|---|---|
| **LFM2-8B-A1B** (1B active MoE) | `LFM2-8B-A1B-Q4_K_M.gguf` (`config.rs:9`) | Main brain: action selection, tool loop | HTTP `:8080`; ctx **32768** (`config.rs:50`, governor may reduce on <12 GB CPU); GPU layers **binary** (99 full-offload if fits +10% headroom, else 0/CPU — `governor.rs:61-124`); `min_p 0.15`, `repeat_penalty 1.05`, no top_k | **BUILT&VERIFIED** |
| **LFM2.5-1.2B-Instruct** | `LFM2.5-1.2B-Instruct-Q4_K_M.gguf` (`config.rs:10`) | Intent classifier (CHAT/INTERACTIVE/REASONING) on a clean prompt | HTTP `:8081`; CPU-only (`-ngl 0`, 2 threads, `bootstrap.rs:52-54`); ctx **2048**; `top_k 50`; GBNF-constrained (`hydra.rs:129`); 8B fallback if down | **BUILT&VERIFIED** |
| **LFM2-VL-450M** (+ mmproj) | `LFM2-VL-450M-F16.gguf`, `mmproj-…F16.gguf` (`config.rs:14-15`) | Visual **embedding** (mean-pooled + per-patch), NOT description | **In-process `libmtmd.so` FFI** (`vision/mod.rs`); fires at **episode boundaries only** (Done/Task/Abort); n_embd discovered at runtime | **BUILT&VERIFIED** for visual *memory*; ⚠️ **NOT wired into action-selection** (see §5) |
| **LFM2-ColBERT-350M** | `LFM2-ColBERT-350M-Q4_K_M.gguf` (`config.rs:21`) | Board relevance embeddings | HTTP `:8082`; CPU-only; `--embeddings --pooling mean`; ctx **discovered from GGUF metadata** (`bootstrap.rs:188-196`), fallback 512 | **BUILT&VERIFIED** |
| VLM subprocess (old text-VLM path) | — | retired | `#[allow(dead_code)]` `ensure_vlm_server()` | **STUBBED** |

- **`InferenceAdapter`** trait (`inference/mod.rs`): `generate`, `generate_with_confidence` (geometric-mean logprob → confidence; 1.0 = "no info"), `generate_constrained(…, grammar)` (GBNF piped to `/v1/chat/completions` via `inference/llama_cpp.rs:91`). This is the **model-swap seam**. **BUILT&VERIFIED.**
- **Governor** (`governor.rs`): NVIDIA/AMD VRAM probe → GPU-layer plan, flash-attn, parallelism; `--cpu-moe` flag wired but MoE-expert detection deferred (currently always false). **BUILT&VERIFIED** (basic), MoE-placement **PLANNED**.
- ⚠️ **Discrepancy:** `recovery.rs:51` defines `QWEN_MODEL = "LFM2.5-8B-A1B-Q4_K_M.gguf"` — a filename that doesn't match any shipped model (main brain is `LFM2-8B-A1B`, no `2.5`/`8B-A1B` combo exists). Appears to be a stale/misnamed constant; the recovery path uses the `:8080` adapter regardless. Flag for cleanup.

## 4. Empirical model behavior (the measured facts we design around)

All measured this session via direct `/completion` probes (N=12/condition, temp 0.2 + grammar ≈ deterministic) on the live 8B; full record + reproducible scripts in `docs/plans/LAGADO_ACTION_SELECTION_OPEN_QUESTION_v1.md` §2.1-2.19 and `docs/plans/experiments/*.py`. **These have published names (mapped post-hoc):**

1. **Positional attention bias ("lost in the middle", Liu et al. 2023).** The model under-attends the early-middle of a candidate list; it favors the edges. **Model-specific structure on top:** with grammar-constrained `el_N` selection it attends the **highest token number / last item**, not the last *rendered* row. Measured: same target wins 12/12 at list-end, 0/12 in the early-middle (§2.2, §2.19).
2. **Comparison ≫ isolation.** The model reads labels and picks correctly *among a visible list* (12/12 in-band), but an *isolated* binary "does this one match? act/skip" is dominated by prepended-context length, not the candidate (acquiescence table, §2.4). → verify-mode was built and **rejected by measurement**.
3. **Prepended-content override.** Semantically-related text placed before the decision **overrides the candidate labels**: decoy-priming memory → wrong pick 12/12 (§2.5); a verbose sub-goal that contains a category word ("…**menu**") pulls a lexically-similar decoy ("Directory **Menu**") 10/12 vs the discriminating token ("Applications") 12/12 (§2.18).
4. **Weak completion detection.** The model emits the escape token 0/12 on a no-match screen (forces a wrong click instead of declining, §2.7), and declares "done" on a wrong-but-changed screen (false-completion, §2.11/§2.16). → completion is taken out of the model's hands (divergence rail built; effect-signature PLANNED, §11).
5. **Natural-intent gap.** Mapping intent→app: token-overlap 0/5, ColBERT-cosine ~1/5 (short-label compression), **model world-knowledge as a single clean classification 4/5** (§2.18). This is the basis for the planned intent→capability router.
6. **Multi-turn degradation** (independent literature + LFM vendor data): ~0.63⁵≈10% over 5 turns untuned, 96-98% after task-specific fine-tuning — the basis for single-turn-fresh and for the fine-tuning bet (§11).

## 5. Perception → action pipeline

**Senses:** (a) **AT-SPI2 accessibility tree** via `perceive.py --focused` over SSH → rows `ref_N role "label" (x,y,w,h)` (`vm/ssh_perceptor.rs`, `perceive.py`); (b) **classical-CV box proposer** (Canny + connected components, `perception/cv_proposer.rs`); (c) **per-patch vision embeddings** (`vision/mod.rs`). Parsing/caching in `perception/mod.rs` (`parse_ref_coords`/`parse_ref_bboxes` → `PerceptionCache{coords,bboxes}`).

**Fusion / arbiter** (`perception/arbiter.rs`): `fuse(a11y, cv, patches) → Vec<FusedElement>`. `FusedElement{ ref_id: Option<String> (None for CV/vision-only), bbox, sense: A11yOnly|VisionOnly|Both, patch_embd: Option<Vec<f32>> }`. IoU dedup, `MATCH_THRESHOLD=0.30`, deterministic `(y,x,w,h)` sort. **BUILT&VERIFIED (16 unit tests).**

⚠️ **Discrepancy (code wins):** `DESIGN.md §10` describes the live pipeline fusing **all three senses**. In the actual loop, `agent.rs:582` calls `fuse(&bboxes, &[], &[])` — **a11y-only**; CV and vision patches are passed empty. The CV proposer, the patch encoder, and `patch_embd` attachment are **BUILT-UNVERIFIED** (exist + tested, not fed in the live action path). Vision currently serves only episode-boundary visual *memory*.

**Synthetic index + grammar selection** (`perception/selection.rs`, `grammar.rs`): every `FusedElement` gets an `el_N` token (`build_candidates`), even label-less ones — so the grammar can't collapse fusion back to a11y. `selector_grammar(&[FusedElement])` (`grammar.rs:24`) emits a GBNF over `click(selector="el_N")` (+ `type/key/wait/done`) constrained to the valid `el_N` + a `none` escape; piped via `generate_constrained`. Actuator resolves `el_N → bbox-center → coord click` (`candidate_coords` + `SshActuator::set_targets`). **BUILT&VERIFIED, live.**

**The validated selection fixes (all live in `agent.rs`, all measured — §4):**
1. **Memory isolation** — the executor prompt is `SYS + ranked candidates + goal` only; no Board/episodic/visual/skill/trajectory (`agent.rs:562-660`). *Why:* prepended memory overrides labels (§4.3).
2. **`discriminating_phrase`** (`selection.rs`; `agent.rs:657`) — strips action verbs + colliding category nouns from the goal-slot ("Applications", not "Open the Applications menu"). *Why:* the verbose sub-goal leaks a decoy token (§4.3).
3. **`rank_late_band`** (`selection.rs`; `agent.rs:631`) — relevance-orders candidates so the best lands last, **then RE-TOKENS by render position** so the target carries `el_{n-1}`. ⚠️ **Marked do-not-delete-as-redundant** (code comment in `rank_late_band` + `CLAUDE.md`): *the model attends the highest token number, not the last-rendered row; if ranking reorders the display but keeps spatial tokens, the target's token is no longer the max and the model picks the max-token element instead — measured 0/12 without re-token, 12/12 with.* A refactor that "cleans up" the re-tokenization silently re-grows the bug.
4. **Deterministic fail-closed** — `goal_matches_any` (`selection.rs`; `agent.rs:594`): no candidate label shares a content token with the goal → re-perceive (the model won't self-escape, §4.4).
5. **Selection-intent divergence rail** — `best_match_token` (`selection.rs`) + gate at `agent.rs:721`: if a *unique* best-match candidate exists and the model clicks a *different* element, fail-closed before acting. *Why:* converts a wrong pick (and the false-completion it would cause) into a clean handback; also makes the completion signal honest. **BUILT&VERIFIED** (terminal/file-manager pass; browser now selects the matched element).

**Actuation** (`vm/ssh_actuator.rs`): SSH → `xdotool mousemove --sync {cx} {cy} click 1` / `type` / `key`. **BUILT&VERIFIED.**

## 6. Control & safety spine

- **Supervisor escalation ladder** (`supervisor.rs`): a pure, unit-tested state machine. The **governor supplies an ordered `Vec<EscalationTier>`** (e.g. weak box `[local, human]`, hybrid `[local-1.2b, heavy-8b, cloud, human]`); the supervisor walks it on deterministic signals — `Failed`×N → ResetFromBoard then Escalate; `NoChange`×3 → Escalate; oscillation (state-hash revisit) → Escalate; ladder exhausted → Abort. Loop reports `StepOutcome` + state-hash, obeys `Directive`. **BUILT&VERIFIED** (wired at `agent.rs:501`; the loop currently acts on Escalate→Human and Abort, defers other directives to inner machinery).
- **Deterministic sequencer** (`agent.rs`): `decompose_goal` (`agent.rs:241`) splits on explicit sequential markers only ("then"/"and then"/";"); a `current_sub` pointer advances when `structural_change` (a11y element-set or focus changed, `agent.rs:283`) is detected; plan exhausted → deterministic completion. Un-parseable/compound goals stay one sub-goal → executor + handback. **BUILT&VERIFIED** (2-step VM tasks pass).
- **HITL / permission gate** (`gate.rs`): `RiskTier{Read,Write,Destructive}` → `Verdict{Allow,ConfirmTap,ConfirmTyped,Block}`. Destructive *content* in args (`rm -rf`, `dd if=`, `drop table`) hard-overrides to ConfirmTyped regardless of tool trust. Per-tool `TrustLevel{Auto,Tap,Typed,Disabled}`. `confidence_escalate`: <0.30 → ConfirmTyped, 0.30-0.60 → +1 tier, ≥0.60 pass; logprob-sentinel 1.0 never gated; Block never lifted. **BUILT&VERIFIED.**
- **Fail-closed** is the through-line: no-match (§5.4), divergence (§5.5), and deviation (repeated no-match → handback) all escalate rather than guess.
- **Recovery manager** (`recovery.rs`): 7 `FailureType`s (ParseFailure/ToolError/LoopDetected/DeadLock/HallucinatedAction/…), action-graph lookup → LLM recovery → HITL → reset; thresholds (loop 5, deadlock 10). **BUILT&VERIFIED.**
- **Envelope protocol** (`envelope.rs`): `{v:1, kind, payload}`; kinds `permission|approval|goal|command|action_log|status`. JSON, round-trip tested. **BUILT&VERIFIED.**
- **Step bound** (`operator.rs`): `StepEnforcer` MAX_STEPS=50, urgency nudge at 10.
- **Keys/auth** (`auth/`, `security/crypto.rs`): wrapped-DEK (Argon2id × password + recovery phrase), raw DEK never on disk, lockout fail-closed. **BUILT&VERIFIED.**

## 7. Memory (the Board)

- **Tiers** (`memory_tiers.rs`): hot (in-RAM `Vec`) / warm (SQLite, LLM-summarized) / cold (SQLite, AES-GCM-encrypted exact text, never auto-pruned). DBs at `~/.laputa-secure/memory.db`, `skill_library.db`.
- **Entropy/decay** (`memory_tiers.rs:684`): `V = T · e^(−λ·age) · (1 + ln(n+1))`, λ = ln2/30 days; `sleep_gate.rs` runs a 5-min cycle: decay 5% → drain cooled hot → LLM-summarize → promote warm → entropy-prune warm at ≥10 000 → backfill text embeddings (32/cycle). **BUILT&VERIFIED.**
- **Board scorer** (`board.rs:99`): `score = α·rec_n + β·rel_n + γ·importance`, defaults α=β=γ=1.0; **recency and relevance min-max normalized across the candidate set before the sum** (`board.rs:82`). **BUILT&VERIFIED** (unit-tested).
- **Relevance** (`embedding.rs`, `memory_tiers.rs:321`): LFM2-ColBERT-350M **mean-pooled** vector cosine (`vision::cosine_similarity`); G3 eval F1 0.43 (Jaccard) → **0.52** (ColBERT). **Known problem, in code comments:** pooled cosines compress into **~[0.96, 0.99]** ("short-label cosine compression") → used raw they go inert, hence the min-max normalization; if normalization still drags noise, the noted next step is late-interaction MaxSim (deferred). This same compression is why intent→label embedding matching fails (§4.5).
- **Three experience stores** (`DESIGN.md §5`): episodic (tiered), procedural (`action_graph.rs`, exact-state-hash replay ≥0.65), technique (`skill_library.rs`, distilled advisory), recognition (visual embeddings). **BUILT.**
- ⚠️ **Critical for prior-art mapping:** the Board is **deliberately ISOLATED from the action-selection prompt** (invariant #10). `agent.rs` computes the slice/visual/skill context into `_episodic_context`/`_visual_context`/`_skill_context` (underscore = unused) and the prompt excludes them (`agent.rs:562` comment + `:658` format). The Board infra is **production-ready but reserved for a v2 upstream planner**, not the v1 click decision. So: *memory is central to the product story and absent from the action loop by design.*

## 8. Sandbox / execution (the VM)

`vm/mod.rs`, `vm/qmp.rs`, `vm/ssh_*.rs`, `security/sandbox.rs`. **BUILT&VERIFIED** end-to-end (`bin/harness_proof.rs`: 8 stages boot→ssh→X→perceive→screendump→click→delta→shutdown, 24.9 s).

- **QEMU/KVM**, `-enable-kvm -cpu host -m 4096 -smp 4`, `virtio-vga xres=1280 yres=800`, **headless** (`-display none`), `-boot order=c`, optional cloud-init seed ISO. Guest = Ubuntu 24.04 + XFCE (provisioned via `vm-provision/build-guest.sh`).
- **Control channel:** QMP unix socket (`screendump format=png` → `/dev/shm/lagado_frame.png`); SSH hostfwd `tcp::PORT-:22`, **key-auth only** (`BatchMode=yes`); readiness gated on `whoami → "laputa"` before `vm_ssh_port` is set; kill-stale pre-flight (`pgrep -f hostfwd`).
- **Host sandbox** (`security/sandbox.rs`): cgroup-v2 `memory.max = mem×1.5`, `pids.max 512` (best-effort, logged-not-fatal); QEMU `-sandbox on,obsolete=deny,…` (seccomp syscall filter — ⚠️ does **not** isolate fs/net; container-grade isolation e.g. libkrun/Firecracker is a research gate, not built).
- **DynamicPerceptor/Actuator** route to SSH (VM) or host impls by `vm_ssh_port`. Host-desktop actuation (`perception/linux.rs`) exists but the product runs the agent **inside the VM** for blast-radius containment (`DESIGN.md §6`).

## 9. Browser path

⚠️ **DOES NOT EXIST IN THE REPO.** Whole-repo search (excluding `target/`, `node_modules/`) for `manifest`/`content_script`/`chrome.runtime`/DOM-adapter code → **zero matches**. There is no `BrowserPerceptor`, no extension, no hashed-ref DOM scheme in code. It is **PLANNED** (CLAUDE.md "Segment 1 — Browser extension: DOM perception + actuation"; and the staged build order lists DOM as the *2nd mastered surface* after the a11y floor). Current perception is **AT-SPI2 (Linux) + SSH-xdotool (VM guest) only**. *(This corrects any prior statement that a browser/DOM extension was built.)* The associated **G4 trust tier** (`perceived-untrusted` vs `user-intent-trusted`, a prompt-injection defense for DOM/screen text) is also **PLANNED** — `Candidate.trusted` carries a flag (`selection.rs:45`) but nothing gates on it yet.

## 10. Stack

- **Rust** agent crate (HTTP inference; no FFI binary-inference linking — only `libmtmd.so`/`libllama.so` linked for the in-process vision encoder via `vision/shim.c` + `build.rs`). **Tauri** host (`lagado-ui/src-tauri/`, ~8 commands, wraps the agent as a library). **React + shadcn + Tailwind** UI (`lagado-ui/src/`). Inference target: vendored `llama.cpp` (`lagado-agent/vendored/llama.cpp-2/`).
- **Key deps** (`Cargo.toml`): `tokio`, `reqwest 0.12` (json+socks), `ureq 2`, `rusqlite 0.31` (bundled), `blake3` (screen/state hashing), `aes-gcm 0.10` + `argon2 0.5`, `image 0.25` + `imageproc 0.27` (CV), `nix 0.27` (cgroups), `tokio-tungstenite` (a WebSocket `server.rs` exists but is **orphaned** dev-scaffold).
- **`InferenceAdapter`** (§3) is the deliberate model-swap seam — the architecture's bet that "the harness is the moat, the model is swappable." MCP tool client (`mcp/`, stdio) + 44 native tools (`tools/`). 226 lib tests; CI matrix linux/macOS/windows (now `paths-ignore` + manual-dispatch to conserve minutes).

## 11. Open technical decisions (where outside input is most valuable)

1. **Hardware floor — the unmade *business* decision wearing a technical costume.** CPU-for-everyone vs 6 GB-GPU-minimum vs buyer-supplied hardware. It cascades into everything below and is currently *assumed* (CPU-for-all), not decided. ⚠️ Flag: dev ran on GPU; pure-CPU 8B-A1B latency is unverified.
2. **Grounding model: fine-tune LFM2-VL-450M for Set-of-Marks grounding on CPU?** The headline bet. UI-TARS proves vision+Set-of-Marks grounding works (94% pixel-grounding) but needs a GPU and is ByteDance-origin. The question: can a **450M** vision model, **fine-tuned** (the LFM vendor pattern: 34-63% untuned → 96-98% tuned), do good-enough Set-of-Marks grounding **on CPU** — replacing the brittle a11y-text-list path with vision that works on any app? **Unmeasured.** The probe (LFM2-VL + Set-of-Marks vs the a11y floor) is the gating experiment.
3. **Distill vs adopt vs build, per component.** Candidates to *adopt/distill rather than reinvent*: **OmniParser** (screenshot→indexed elements — overlaps the FusedElement+`el_N` spine; could fill the CV/vision arbiter slot that's currently empty); **Set-of-Marks** prompting (retires the §5.3 token-attention class of bug by making selection visual); **UI-TARS / OS-Atlas / ShowUI** as *teachers/training-data* for a distilled CPU grounding model, not as the shipped (GPU) model. License note: LFM2 is **LFM Open License v1.0** (free commercial <$10M rev, fine-tunes may stay proprietary — the trade-secret asset).
4. **Completion grounding — the effect-signature (next build).** Completion-detection is the last terminal-state authority still partly inferred; the effect-signature observes a *world-change* (window/top-level node appeared, title changed, region delta) and replaces the "did the screen change" proxy. **Spec hole to resolve deliberately:** "observe a world-change" hides "decide *which* change counts" = a goal/action→signature **mapping**; lean deterministic action-type→signature, fail-closed on unmapped, and it **must not** be the model asserting its own completion. The settle delay folds in as *poll-until-fire-or-timeout*, not a fixed per-app sleep. Must distinguish "already-satisfied on entry" from "nothing happened" (compare goal-vs-current-state).
5. **The intent→capability router (the natural-intent layer).** Users say "check my email," not "launch the Terminal Emulator." Measured viable path: model-world-knowledge intent→capability classification (4/5, §4.5) constrained to a **curated capability vocabulary** with deterministic fail-closed against the set — *not* model-confidence-gated. This is the same constrained-vocab+fail-closed primitive as the executor, one altitude up. **PLANNED.**
6. **Multi-sense fusion order.** CV proposer + vision patches are built but unwired (§5); the arbiter's `patch_embd` cross-modal use risks the §7 short-vector compression in a new modality. Decision deferred behind a **visual-embedding discrimination probe** (does it separate elements where labels didn't?) — efficiency (the adiabatic grid + blake3 delta cache, all built) is not the question; discrimination is.
7. **The Board's role.** Built, isolated from the action loop by measurement (§7). Open: does it return via a memory-informed *planner* (above the memory-free executor), and how is *that* layer kept from re-introducing the prepended-content override (§4.3)?

---

*Prepared from the codebase at HEAD. Where this brief says BUILT&VERIFIED it means exercised; BUILT-UNVERIFIED means present-but-not-in-the-live-path; PLANNED means absent from the repo. The two largest code-vs-narrative corrections: the browser/DOM path does not exist (§9), and the live perception fuses a11y only despite the 3-sense design (§5).*
