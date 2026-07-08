# THE RUST HARNESS — COMPLETE MAP (2026-07-08)

Five independent full-read passes over all 91 files / 23,651 lines of `lagado-agent/src`
(excludes the OSWorld Python battery, mapped separately in HARNESS_COMPLETE_MAP_2026-07-06).
Compiled for the owner's line-by-line review. Two audits ran alongside the neutral read:
(1) where enumerated vocabularies constrain action, (2) what the raw motor/sense substrate is.

---

## 1. THE ACTION SPINE (agent.rs 3712, supervisor, recovery, forge, bracket_parser, grammar)

`agent_loop` owns goal execution. Three paths after `plan_goal`:
- **ReAct capability loop** (all-Command goals): verify-first → discover env (find, depth 4,
  cap 80) → capability_grammar GBNF → parse/validate/to_command → gate → execute. Max 8 steps.
- **API plane** (in-app spreadsheet): NativeSession daemon (P2c) with build_guest_apply as
  the proven floor; per-op emission, max 12 ops.
- **GUI click loop**: settle (observe_until_quiet: 120ms ticks, 3 quiet reads, frame-delta
  gated by DeltaDetector, 300-poll backstop) → a11y read → fuse (a11y ONLY — see §2) →
  build_candidates → goal_matches_any (fail-closed, lexical) → rank_late_band (cap 64,
  re-tokenize so best target = el_{n-1}; measured 0/12→12/12) → selector_grammar → model →
  divergence rail (best_match_token) → gate → click → effect_confirmed (Open/Activate
  classes) → sequencer advance.
- Deterministic sequencer: decompose_goal splits ONLY explicit markers; planner (8B,
  skill-advised — the single allowed memory→action path, inv #10 guarded by CI test) for
  implicit goals; SubAction classes Click/Type/Key/Command; Type/Key = fire-and-advance.
- Supervisor: Progressed/NoChange/Failed/PerceptionBlind → escalate ladder (Model→Sense→
  Human); oscillation detection; **ResetFromBoard returned but silently ignored by the
  loop** (catch-all `_ => {}`).
- Recovery: 7 failure modes; **MemoryReset unimplemented (logs and continues);
  pre_execution_check never called; hardcoded model name (violates inv #9); recovery LLM
  calls bypass grammar rails.**

## 2. PERCEPTION (mod, linux, delta, frame, cv_proposer, arbiter, selection, capture, vlm, vision)

- **PerceptionCache**: three fields (screen_text, coords, bboxes) replaced atomically per
  read. NO TTL, NO enrichment, NO persistence — a write-once-per-frame slot.
- **DeltaDetector**: 8×6 blake3 grid over decoded RGB, remainder to last row/col, keyed
  c{row}_{col}. Frame-to-frame change detection ONLY. **The "adiabatic cache" was never
  built**: no per-cell enrichment store, no history, no invalidation-driven re-perception.
  Delta and CV are UNCONNECTED — propose_frame re-scans all 48 cells every call.
- Grid failure modes (audited): elements straddling cells split into partial boxes (no
  cross-cell merge); scroll/navigation invalidates all 48 cells with no scroll-vs-content
  distinction; resolution change silently invalidates everything.
- **cv_proposer**: Canny(15/45) + 8-connected components, per-cell, area/aspect filters.
  Built, tested — and **`cv_enabled()` defaults FALSE**; in the live loop `_cv_boxes` is
  computed then DISCARDED; `fuse()` is called with empty CV/patch arrays. a11y-only in
  production.
- **arbiter**: IoU 0.30 fusion, Sense{A11yOnly,VisionOnly,Both}, LabelSource priority
  a11y>caption>ocr (caption/OCR NOT BUILT), patch-embedding mean-pool with ±1-patch fuzz,
  deterministic (y,x,w,h) ordering. Genuinely good organ, starved of inputs.
- **vision FFI**: libmtmd in-process; mean-pool + per-tile patches; grid derivation
  replicates LFM2 preprocessing; overview detected by chunk index (token-count detection
  proven wrong at 1025×1025). Built; patches not fed to the live loop.
- vlm_adapter retired; capture.rs host stub (VM path uses QMP screendump).

## 3. THE MOTOR SYSTEM (ssh_actuator, osworld actuator, projector)

- SSH path: `xdotool mousemove --sync X Y click 1` / `type --clearmodifiers` /
  `key --clearmodifiers`; persistent bash with sentinel framing (180s silence kill).
- OSWorld path: pyautogui click/typewrite/press over guest HTTP /execute.
- projector/: platform-dispatched raw primitives — MouseClick{x,y,button}, MouseMove,
  TypeText, KeyPress (Linux=xdotool; mac/win stubs) + risk validator. **Built, nothing
  drives it dynamically.**
- **Missing motor verbs: scroll, drag, wheel, right-click, double-click, hover.** The live
  paths hardcode `click 1`. The model can NEVER emit a coordinate — every click resolves a
  selector (ref_N/el_N) from the a11y-populated cache. A raw Pixel action plane does not
  exist (the ladder's Pixel rung is a stub).

## 4. THE VOCABULARY AUDIT (every enumerated constraint)

| Surface | Constraint |
|---|---|
| selector_grammar | click/type/key/wait/done over el_0..el_{n-1} + "none" |
| capability_grammar + validate | 7 verbs: move/copy/rename/make_folder/write_file/delete/extract_to_file |
| scan_op_calls (API plane) | 9 spreadsheet ops |
| ToolRegistry | 44 named tools, trust tiers |
| intent_grammar | CHAT/INTERACTIVE/REASONING |
| gate read-only allowlist | 37 bins |
| classify_subgoal | command-lead phrases, press/hit/type starters |
| capability_expressible | 21-word blocklist routes to raw shell |

Escape hatches that ARE unconstrained: SubAction::Command (any shell, gated), UNO heredoc
(anything inside LibreOffice), Invoke over the 44 tools, and — unintentionally — the
recovery LLM paths (no GBNF).

## 5. ROUTING + MEMORY + LEARNING (hydra, memory_tiers, sleep_gate, board, action_graph, skills…)

- hydra: deterministic levers first (SurfaceState/RouteMode/action-shape), 1.2B classifier
  on the residual (clean-context, grammar-constrained), action_graph shortcut at 0.65.
- memory_tiers: hot→warm→cold with entropy pruning (30-day half-life), encrypted cold,
  dual embedding columns (visual + ColBERT text), plaintext-minimized scoring; sleep_gate
  5-min cycle (decay → LLM summarize → embed backfill 32/cycle → prune).
- Board: Park scorer (α/β/γ=1), 1-day recency half-life — **computed for chat-RAG only;
  the agent_loop priors were removed ("computed-and-discarded" audit)**.
- **Learning loops that are dead**: action_graph pruning never called; replay_manifest has
  no consumer (QLoRA Phase 2); self_model→distill unwired; skill record_success/failure
  callers not found; kv_slots fully stubbed; ChronosDb snapshots written, never read back.
- **liquid.rs is a model-roster stub (always returns the 8B). No CfC, no temporal state,
  no continuous-time anything exists in the Rust harness.** The reflex/CfC work lives only
  in Python, unconnected.

## 6. PLANES / VM / INFRA (plane, back_door, api_plane, native_session, vm/*, tools, gate, governor…)

- plane.rs richest-first ladder Api>BackDoor>A11y>Cv>Pixel + Cli launch-pad; only
  PerceptionBlind triggers a switch. Cli REAL, BackDoor REAL, Api REAL (OSWorld-proven),
  A11y REAL, **Cv inert-by-default, Pixel STUB**.
- vm/: QEMU boot (KVM, virtio-vga 1280×800, QMP socket, user-net hostfwd, seccomp sandbox,
  cgroup v2 caps, KillOnDrop), QMP screendump (png required), SSH readiness by auth probe.
- gate.rs: destructive-text override → read-only allowlist → trust tiers; plan-approval
  collapses Tap; confidence escalation (geometric-mean logprob; 1.0 sentinel = no data).
- governor: NVIDIA/AMD detect, GGUF-aware offload planning (full/‑‑cpu-moe/CPU floor),
  VRAM linear-fit prediction, env overrides.
- bootstrap/server_guard: three servers (8080/8081/8082) spawned with GGUF-discovered
  params, health-guarded restart, cgroup limits. auth: wrapped-DEK, lockout. crypto:
  AES-256-GCM + Argon2id. terminal/pty: Phase-1 stub (not a real PTY).

## 7. THE BINS (25 binaries — the experimental record)

KEEPERS (regression instruments): user_stress, hard_stress, stress_test, reform_stress,
routing_probe, multistep_probe, osworld_stress/real/heldout/plan/run, harness_proof,
cli_demo, cv_measure, first_walk.
HISTORICAL (proved a finding, now inert): capability_probe (7/16 vs 2/8 free-form),
planner_probe (plan-ahead hallucinates), react_probe/react_loop_probe (ReAct wins),
discover_probe (path-binding), axblind_probe (CV can't measure semantic blindness),
session_drive, vm_proof, reason_emit_probe (*re-runnable for a future emitter).
**No compile rot found against the current module surface.**

## 8. THE HEADLINE (for the redesign conversation)

1. The adiabatic cache never existed; the grid is frame-diff only, with the audited
   failure modes. The compositor membrane (damage events — push, native-precision rects,
   no polling, no grid) answers the same question the grid approximated, exactly.
2. The agent's whole motor life is click/type/key/wait through a11y-resolved selectors.
   Raw coordinate primitives exist (projector, xdotool) but nothing drives them; scroll/
   drag/wheel/right-click/double-click/hover don't exist at all.
3. The fusion organ exists and is good; it runs starving (a11y only, CV discarded,
   captions/OCR unbuilt, patches unwired).
4. Nothing in the Rust harness is temporal or adaptive. The CfC vision has zero presence
   here.
5. The vocabularies were built because free-form failed (measured 2/8); they became the
   ceiling instead of the rail. The missing tier between LLM intent and motor output —
   trained sensorimotor competence — is where the hands/eyes design goes.
