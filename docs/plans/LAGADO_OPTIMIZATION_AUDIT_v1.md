# Lagado — Optimization & Smoothness Audit v1

**Date:** 2026-06-19 · **Scope:** full-codebase read (16,688 lines, ~50 modules) by 5 parallel auditors + clippy.
**Goal (user):** "so highly optimized it runs smooth no matter what." Engine-focused (user owns the UI).
**Status of each item:** ☐ not started · ◐ partial · ✅ done. Each item: `file:line — problem — FIX — IMPACT`.

The 90+ findings collapse into **SIX themes**. Fixing the themes ≈ fixing the app's smoothness.

---

## THEME 1 — Blocking I/O on the async runtime (THE jank cause) ☐

Perception (SSH), inference (HTTP), and SQLite all run *synchronously inside async* `agent_loop`/`hydra::run`,
on Tauri's shared tokio runtime. Each blocks an executor worker for the whole round-trip → the live VM canvas,
UI events, `server_guard`, and `sleep_gate` all freeze during every tick. NB: `spawn_blocking` does NOT shorten
the per-step wall-time (the loop waits regardless) — it stops that wait from STARVING the rest of the runtime.
That starvation IS the jank. (Model calls in Forge are *already* wrapped — the pattern just wasn't applied to
perception/actuation/SQLite.)

- **[P1] agent.rs:1306-1311,1485,1546 — sync SSH `read_screen()`/`run_command()` directly in async — wrap every Perceptor/Actuator call in `spawn_blocking` (or make the traits async).**
- [P1] agent.rs:956-1023 — `observe_until_quiet` does a full PNG decode every 120ms poll (×300) + blocking SSH read, on the runtime — run decode+delta+a11y read in `spawn_blocking`; reuse one decoded buffer.
- [P1] hydra.rs:138-141,155-165 — `classify_intent` + `chat_response` call blocking HTTP `generate` in async — `spawn_blocking`.
- [P2] hydra.rs:387-388 — `ActionGraph::open` + query (blocking SQLite) directly in async `run()` — `spawn_blocking`.
- [P2] agent.rs:895-909 — `deterministic_reform` fires serial blocking `command -v` SSH probes in async — `spawn_blocking`; batch `command -v a b c`.
- [P1] sleep_gate.rs / memory_tiers.rs — decay/scan/prune/store run sync SQLite under the held `Mutex<MemoryTiers>` (see Theme 4).
- [P2] tools/executor.rs:76-200,204-285,552-616 — every `std::fs` + git/clipboard `Command` runs in async `dispatch` (only `run_command` is wrapped) — route through `spawn_blocking`/`tokio::fs`.
- [P1] hydra.rs:155 + tools/executor web tools — blocking `ureq`/`reqwest` in async paths.

## THEME 2 — Built-but-UNWIRED intelligence (the "fell back to CPU" root cause) ☐

The smart subsystems exist, are unit-tested, and are simply not connected to the live path.

- **[P1] bootstrap.rs:252 & hydra.rs:58 — live spawn calls the CRUDE `governor::detect_and_plan` (all-or-nothing, hardcoded 1.1× VRAM rule, never reads GGUF, `--cpu-moe` always off). The GGUF-aware `governor::plan_engine` + `gguf::read_metadata` + VRAM calibration + MoE detection are FULLY BUILT and DEAD. — Wire `gguf::read_metadata(model_path) → plan_engine(...)` into `ensure_llama_server`; emit `-c/-ngl/--cpu-moe` from the `EnginePlan`. THE single highest-value change — turns CPU-fallback into partial-GPU on any card, no env vars.**
- [P1] governor.rs:128-136 — `compute_offload` is binary (99 or 0), never partial — retire it from the live path in favor of `plan_engine` (which already computes `ngl ≤ block_count` + `cpu_moe`).
- [P1] governor.rs:134 — `fits = free ≥ model×1.1` ignores KV cost + treats <1.1× as zero-GPU — use `predict_vram_mb` + feasibility w/ 0.85 headroom; lower ctx to fit instead of dropping all layers.
- [P1] governor.rs:287-356 / gguf.rs — `plan_engine`/`predict_vram_mb`/`largest_fitting_ctx`/calibration are dead — persist measured VRAM to `config/calibration.json` after first launch; feed back.
- **[P1] inference/mod.rs:46-52 + kv_slots.rs:16-33 — KV-slot prefix reuse is STUBBED (every adapter returns false). Each single-turn-fresh step re-encodes the FULL prompt — the documented core latency mitigation is absent. — Implement save/restore/has via `POST {base}/slots`; wire `KvSlotManager`. The dominant per-step inference cost.**
- [P2] config.rs:67 — `CONTEXT_SIZE=32768` hardcoded (real is 128k; inv #9) — derive from GGUF.

## THEME 3 — Dead work computed-then-discarded + dead files ☐

- **[P1] agent.rs:1229-1282 — FOUR `_`-prefixed contexts (`_episodic_context`, `_visual_context`, `_skill_context`) computed EVERY goal then discarded: a blocking embedder HTTP, a VLM FFI encode + frame read, a skill DB retrieve, each + a `MemoryTiers` lock — all on the latency-to-first-action path, feeding nothing (code self-flags TODO(v1-cleanup); inv #10 forbids them reaching the executor anyway). — DELETE all three blocks + their compute.**
- **[P1] perception/cv_proposer.rs + agent.rs:1586-1600 — the per-frame Canny+connected-components CV pass produces label-less boxes that CANNOT be selected (CV inert until Phase-2 captions) AND forces an extra QMP capture + extra PNG decode per step. Pure cost, zero current benefit. — Gate the CV pass off by default (the `cv_enabled()` switch exists) or behind changed-cells, until captions land.**
- [P1] Cargo.toml — DEAD FILES not in lib.rs: `server.rs` (orphaned WS scaffold), `verifier.rs` (superseded by EffectClass) — delete; deleting `server.rs` lets us drop `tokio-tungstenite` + `futures-util`.
- [P3] perception/vlm_adapter.rs + `VlmPerceptor`, perception/capture.rs — retired/unused (blocking ureq) — delete after confirming no callers.
- [P3] agent.rs:1485,1546 — `prev_screen = read_screen()` after a Command/Type/Key step is overwritten unused next iter (the `else` branch re-reads) — set `String::new()`; removes a wasted SSH round-trip per command step.
- [P3] memory_tiers.rs:348 — `rank_by_relevance` is eval/test-only — gate `#[cfg(test)]` or remove.

## THEME 4 — Costs that GROW WITH USE (the stutter that worsens over time) ☐

- **[P1] sleep_gate.rs:69-71 / memory_tiers.rs:491-496 — `decay_all` full-table UPDATE every 5 min, SYNC under the held tokio Mutex, and cold NEVER prunes → an ever-growing periodic FOREGROUND FREEZE. — Lazy decay (store `last_decay_ts`, compute temperature at read) OR move the UPDATE into `spawn_blocking` + drop the lock. Headline smoothness win.**
- [P1] memory_tiers.rs:285-343 — `scored_candidates` (Board per-step retrieval) full-scans + decodes every blob + DECRYPTS every cold candidate BEFORE truncating to top_k — score-then-decrypt only the top_k; cap candidates.
- [P1] memory_tiers.rs:251-280 — `entries_missing_text_embedding` has no SQL LIMIT (truncates to 32 in Rust after loading+decrypting thousands) — add `LIMIT ?BACKFILL_BATCH`.
- [P1] memory_tiers.rs:666-671 — `entropy_prune_warm` does N+1 single-row DELETEs, no transaction — one `DELETE ... WHERE id IN (...)`.
- [P1] memory_tiers.rs schema — NO index on `tier` → every `WHERE tier=...` is a full scan — `CREATE INDEX idx_tier`.
- [P2] memory_tiers.rs:533-544 — `assemble_context` (foreground) `ORDER BY temperature DESC` with NO LIMIT — add `LIMIT 200`.

## THEME 5 — Connection / handle / object churn ☐

- **[P1] vm/ssh_perceptor.rs:23 + vm/ssh_actuator.rs:22,93 — full SSH handshake (TCP+keyexch+auth+remote fork) per read/click — add `-o ControlMaster=auto -o ControlPath=/tmp/lagado-ssh-%C -o ControlPersist=60s` to all ssh args → first call opens a master, rest reuse the channel. Single biggest per-frame lever.**
- [P1] skill_library.rs:101-130 — `conn()` reopens SQLite + re-runs CREATE TABLE + 3 ALTERs on EVERY call (retrieve is on the loop) — open once, cache the Connection, migrate at construction. (Same anti-pattern: self_model.rs, distill.rs, chronos.rs:95, retrieval.rs:63/85.)
- [P2] chronos.rs:7-23 — `log()` opens+`create_dir_all`+writes+closes the file PER event (a syscall storm on a hot path) — hold a buffered append handle in a `OnceCell<Mutex<File>>`.
- [P2] tools/executor.rs:390-412 — `http_client()` rebuilds a `reqwest::Client` (TLS init) AND re-reads+parses `network.json` from disk on EVERY web call — build once behind `OnceLock`; cache proxy config.
- [P2] vm/ssh_perceptor.rs:51-55 + vm/qmp.rs — new `QmpClient` (connect + handshake) per `capture_frame`, every 120ms — persist one session `QmpClient`.
- [P2] embedding.rs:19-21 — new `ureq::Agent` per `embed` call (no keep-alive across backfill loop) — one cached Agent.
- [P2] vm/mod.rs:74-114 — `DynamicActuator/Perceptor` build a fresh `Ssh*` (String allocs) per action — memoize per port.
- [P2] hydra.rs:368 — `Hydra::from_governor` (hardware probe + nvidia-smi + file stat + health probe) runs PER user message — build Hydra ONCE at startup, reuse.
- [P2] bootstrap.rs/server_guard.rs — `check_health_sync` builds a fresh `ureq` agent on every poll (×3 servers, forever) — reuse one.
- [P2] action_graph.rs:168-180 — `get_best_action` does an `UPDATE last_used` (WAL fsync) on every cache HIT — the "bypass inference" fast path writes to disk — skip/debounce the write.

## THEME 6 — Hot-path micro-allocations & redundant compute ☐

- [P2] perception/mod.rs:104,126,149 — `parse_ref_*` recompile their `Regex` on every `read_screen` — `LazyLock<Regex>` statics.
- [P2] perception/linux.rs + agent.rs:1580-1581 — the SAME screen text is regex-parsed 2-4× per read (coords, bboxes, labels) — single-pass parse into the cache; reuse the cache, don't re-parse.
- [P2] selection.rs:176-184 — `rank_late_band` comparator calls `relevance()` (tokenize+lowercase alloc) on both sides per comparison (O(n log n) tokenization) — decorate-sort-undecorate (compute key once per candidate).
- [P2] sleep_gate.rs:114-126 — backfill embeds 32 entries with 32 sequential HTTP round-trips, breaks on first error — one batched `input:[...]` request.
- [P2] cv_proposer.rs:187-237 — `rgb_to_gray`/`bboxes_from_labels` use per-pixel `put_pixel`/`get_pixel` + `%`/`/` + HashMap over a dense label space — linear `chunks_exact`/`as_raw()` iteration.
- [P2] agent.rs:1327-1339,1777 — `blake3::hash(screen)`/`hash(prev_screen)` recomputed 3× per iteration — hash once, reuse digests.
- [P3] agent.rs:1291 — `state.lock().await` every iter just to read `running` — `Arc<AtomicBool>`.
- [P3] agent.rs:1587 — `config::cv_enabled()` env read every iter — read once before the loop.
- [P3] agent.rs:1862 — `recent_actions` Vec `remove(0)` (O(n)) — `VecDeque::pop_front` (supervisor.rs already does this).
- [P3] retrieval.rs:42-56 — rebuilds query HashSet per candidate (×250/call) — hoist out of the loop.
- [P3] tools/executor.rs:514,318,620 — per-call regex compile (read_webpage), double regex pass (find_replace), per-char String alloc (urlencoded) — `OnceLock` regexes; single-pass; `push_str`.

## CROSS-CUTTING (build & robustness) ☐

- **[P1] Cargo.toml (workspace root) — NO `[profile.release]` → ships with defaults (no LTO, codegen-units=16, no strip, full unwind tables). — Add `lto="thin"`, `codegen-units=1`, `opt-level=3`, `strip=true`, `panic="abort"`. Biggest binary-size + glue-code-speed win, 6 lines.**
- [P1] Cargo.toml — dual HTTP stacks (`reqwest` async + `ureq` blocking) — consolidate on one.
- [P3] Cargo.toml — `tokio = { features=["full"] }` in both crates — trim to used features.
- [P3] crypto.rs/auth.rs — `.try_into().unwrap()` / `path.parent().unwrap()` in library code — `?`/guards (no-panic-in-lib).
- [P3] memory_tiers.rs:157-207 — cold writes `encrypt().unwrap_or_else(plaintext)` → SILENTLY stores plaintext in the "vault" on encrypt failure — fail closed.
- Clippy: 56 warnings, 22 auto-fixable (unused imports, needless return/clone, missing `Default`); a few dead assignments from the session's supervisor refactor.

---

## ALREADY CLEAN (verified, not padded)
`gate.rs` (pure functions, no I/O), `supervisor.rs` (pure state machine, `VecDeque` window), and agent.rs pure
helpers (`decompose_goal`, `classify_subgoal`, `command_postcondition`, `decide_reapproach`, …) — exemplary, leave alone.

## SUGGESTED ORDER (impact ÷ risk)
1. **Safe quick wins NOW:** release profile; delete `server.rs`+`verifier.rs` (+drop tungstenite/futures-util); delete the 4 dead `_`-contexts; clippy autofix. (zero behavior risk, immediate.)
2. **Biggest smoothness, moderate effort:** wire `plan_engine`+`gguf` into bootstrap (Theme 2); `spawn_blocking` sweep on perception/inference/SQLite (Theme 1); `decay_all` lock fix + lazy decay (Theme 4); SSH `ControlMaster` (Theme 5).
3. **Then:** KV-slot prefix reuse; gate the CV pass; cache connections (skill_library/chronos/Hydra); add the `tier` index; single-pass perception parse + LazyLock regexes.
4. **Polish:** the P2/P3 micro-allocs.
