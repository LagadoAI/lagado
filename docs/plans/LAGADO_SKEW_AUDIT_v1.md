# Lagado harness skew audit — class-vs-instance (2026-06-21)

**Principle audited:** code must solve a CLASS of tasks, never a single task. The model absorbs
within-class variation; code is a thin general capability surface + rails. Any code that only works for
one task / app / evaluator / model / machine is SKEW. Method: 6 parallel agents, one region each, every
unit classified CLASS-GENERAL / INSTANCE-SKEW / BORDERLINE with file:line + severity + the class it should be.

## Verdict
The fear ("the harness is just a pile of code written to pass individual tests") is **NOT** the dominant
reality. The decision rails (supervisor, gate, grammar, parser, action_graph, loop-control), the capability
registry + tool executor, the perception/fusion math, and the governor/gguf/sysinfo discovery layer are
**genuinely class-general** — several are exemplary, with in-code prohibitions against tuning constants to
pass tests and explicit DISCOVER-or-DEFER discipline.

BUT the instinct is real and recurs at one predictable place: **wherever code touches an EXECUTION BOUNDARY
it reaches for the instance.** The skew is concentrated at three boundaries, not spread through the logic:
1. **Environment-instance** — the one dev VM (`/home/laputa`, an XFCE app map, `DISPLAY=:0`, VM literals).
2. **Model-instance** — no-hardcode-invariant (#9) violations baked for one model (ctx size, sampling, filenames, VLM patch grid).
3. **Benchmark-instance** — the fitting *habit* (GIMP exemplars + named-tool lists in planner prompts, an evaluator-shaped verb).

Good news: ~16 load-bearing items, mostly mechanical, localized to those boundaries — not a rewrite.

## Region ratings
| region | rating | load-bearing skews |
|---|---|---|
| OSWorld adapter (`lagado_agent.py`) | MED | 2 |
| M1/M2 probes (reusable: `m1_reconcile`,`uno_apply`,`m2_uno`) | LOW | 0 (instance quarantined in proof scripts) |
| Rust decision/action core (`agent.rs`…) | MED | 5 |
| Rust capability/tools/planner | MED | 4 (1 live bug) |
| Rust perception/vision/vm | LOW–MED | 4 |
| Rust memory/board/governor/inference | LOW–MED | 4 |

## The emblem (worst single item — fix first)
**`agent.rs:924` — `/home/laputa` hardcoded as a LOAD-BEARING FILTER.** `derive_expected` discards every
expected path not under `/home/laputa/`. On any real machine (`/home/<realuser>`) the completion checks come
back empty → the agent fail-closes even on success. It is simultaneously a **live product bug**, a **no-hardcode
#9 violation**, and the perfect emblem of the worry: it literally makes the agent work only in the one test
environment. The sibling `discover_environment:846` already does `$HOME` correctly — proving it's needless.
(Corroborated independently by the capability and decision-core audits.)

## Consolidated load-bearing skews (deduped, by bucket)

### Bucket 1 — environment-instance (the one dev VM)
| # | file:line | what | class it should be |
|---|---|---|---|
| 1 | `agent.rs:924,938,968,916` (+capability/recovery prompts) | `/home/laputa` hardcoded user home; line 924 is a hard output filter | `$HOME`-relative (discover) |
| 2 | `agent.rs:1211-1216` | app-synonym→binary `pgrep` map (file manager→thunar, terminal→xfce4-terminal…); XFCE-only and **stale** (guest is now Fedora/Cinnamon → nemo) | "verify an app launched" via observed focus/window change, DE-agnostic |
| 3 | `vm/mod.rs` (+ssh_actuator/perceptor) | `laputa` user, `127.0.0.1`, port `2222`, `seed-fedora.iso`, `lagado-guest-fedora.qcow2` | identity from `VmConfig` |
| 4 | `ssh_perceptor.rs:30`, `ssh_actuator.rs:177,194,203` | `DISPLAY=:0` on every guest command | discovered/configured display |
| 5 | `vm/mod.rs:201` | guest geometry `xres=1280,yres=800` frozen in QEMU args | resolution in `VmConfig` |
| 6 | `lagado_agent.py:320-324, 168-176` | GNOME-Terminal-specific probe branch; `_is_desktop_config` hardcoded `org.gnome.*` | DE-agnostic config introspection |

### Bucket 2 — model-instance (no-hardcode invariant #9)
| # | file:line | what | class it should be |
|---|---|---|---|
| 7 | `main.rs:698` + `config.rs:74` | live main adapter `CONTEXT_SIZE=32768` (used for budget + capability tiering) | discover from GGUF (`gguf.rs` already reads `context_length`) |
| 8 | `config.rs:11` → `bootstrap.rs:51` | classifier server `-c 2048` hardcoded | discover from GGUF (embedder at `bootstrap.rs:188-197` already does) |
| 9 | `config.rs:29-30` → `llama_cpp.rs:28,39` | sampling `GEN2_MIN_P=0.15`, `GEN25_TOP_K=50`, `REPEAT_PENALTY=1.05` baked | defer to governor/user (EnginePrefs) |
| 10 | `recovery.rs:51` | `const QWEN_MODEL="LFM2.5-8B-A1B-Q4_K_M.gguf"` (stale + mislabeled) | discover/defer via `InferenceAdapter` |
| 11 | `vision/mod.rs:240-241` | `PATCH_STRIDE=32`, `N_PATCH_COLS=16` tied to LFM2-VL — breaks swappable-model | derive from encoder grid (C shim `lfm2_find_grid()` already does) |
| 12 | `liquid.rs:18-35` | hardcoded model roster (param counts + filenames incl. rejected gen2.5) | discover (`select_model` is a stub → latent) |

### Bucket 3 — benchmark-instance (the fitting habit)
| # | file:line | what | class it should be |
|---|---|---|---|
| 13 | `agent.rs:1050-1061` | `extract_to_file` multiplexes value/count/list — modes mirror evaluator output shapes | 3 distinct general read primitives |
| 14 | `grammar.rs:92` → `agent.rs:1048` | `filter="larger_than_1k"` → `-size +1k` promoted to a grammar enum | general size-filter param (model supplies threshold) |
| 15 | `osworld_plan.rs:82-93, 30-41, 113-119` | GIMP menu recipes ("Image→Mode→Indexed") + `dconf/gsettings/pactl/stat` named-tool lists in planner prompts | general hints, no app/tool exemplars |
| 16 | `m2_authoring.py:31-34`, `m2_calc.py:46-49,102-120,87-90` | prompt leaks evaluator mechanism ("verified by reading cell values, write literal values, integers") + per-task postconditions + instance ReAct hints | **already removed** in `m2_uno.py` (kept here only as the before) |

## Dead / coherence-rot (cosmetic — delete or re-contract)
- `lagado_agent.py:293-307` `_running_app_to_reload` — per-app dict, never called (comment says removed). Delete.
- `recovery.rs` whole module — emits/expects `{"tool":"click"}` JSON, a format no longer used (live = bracket/Pythonic). Retire or re-contract.
- `bootstrap.rs:99-162` `ensure_vlm_server` — `-ngl 32`/`-t 4`/`VLM_CONTEXT_SIZE` hardcodes; `#[allow(dead_code)]`, vision is in-process FFI now.

## Borderline / watch (method general, content instance-leaning)
- `selection.rs:90-94` `RELEVANCE_STOPWORDS` — English + UI-chrome word list tuned to the decoy battery; method general, content instance. (The model's job eventually.)
- `lagado_agent.py:22-23` `UNGROUNDED` English stderr fragments — gates discover/reground.
- `lagado_agent.py` `_action_locus` magic offsets + flyout region clip — 1080p/GTK-tuned watch regions (graceful via retry).
- `agent.rs:845` focused-roots scope — chosen because recursing `$HOME` halved pass rate; generalizes by construction (goal-named abs paths added) but the *choice* was pass-rate-driven.
- `m1_reconcile.py:71` `pkill script-fu` in `kill_app` — cosmetic GIMP leak in otherwise-general reaping.
- `m1_gimp.py` ICC-strip + title-keyed modal dismiss — instance fix for a CLASS problem; the class-general home is a focus-stealing-modal rail in `reload_into_focus` (a **missing generalization**, isolated in the proof script, not contaminating reusable code).

## Fix order (close the skew without touching the general core)
1. **`agent.rs:924` `/home/laputa` → `$HOME`** (live bug; highest leverage). Sweep all `/home/laputa` to discovered `$HOME`.
2. **Model-instance #9 sweep:** main-adapter ctx + classifier `-c` → GGUF-discovered; sampling → EnginePrefs. (The pattern already exists in-region; copy it.)
3. **App-launch map → generic launch-confirm** (observed focus/window change), delete the stale XFCE table.
4. **VM literals → `VmConfig`** (user, host, port, geometry, display).
5. **Planner prompts → de-exemplar** (drop GIMP recipes + named-tool lists; keep the general technique).
6. **`extract_to_file` → split** into general read primitives; reconsider `larger_than_1k` enum.
7. **Delete dead code** (`_running_app_to_reload`, retire `recovery.rs`'s dead contract, dead VLM spawn).
8. **Add the missing modal rail** to `reload_into_focus` (subsumes the gimp ICC/dismiss instance fix).
9. **VLM patch grid → derive from encoder** (closes the swappable-model break; latent until patch path re-wired).

## FIX STATUS (2026-06-21)

**DONE + verified (cargo check + 287 lib tests green), commits `b7b5edc`, `30d0380`:**
- **#1 `/home/laputa` live bug** — new `guest_home(env)` derives `/home/<user>` from the observed listing;
  `derive_expected` (prompt + the load-bearing filter), `react_next_command`, `capability_prompt` all use it.
  The Rust agent now works against ANY guest user (laputa, OSWorld's `user`, …), not one.
- **#3 stale XFCE app-map** — deleted from `goal_postconditions`; launch completion is confirmed by the
  DE-agnostic perception/effect layer (`effect_confirmed`/`observe_until_quiet`). Test updated.
- **#7 (part)** — deleted dead `_running_app_to_reload` per-app dict (`lagado_agent.py`).
- **#2 (part)** — classifier ctx now CLAMPED to the GGUF-discovered max (`min(task_need, model_max)`) —
  removes the unsafe assumption without bloating the CPU classifier's KV.
- **#5 planner de-exemplar** — dropped GIMP menu recipes from `--next` and the GNOME/audio tool-list from
  `--verify`; kept the general technique (knowledge-of-the-app menu path; read-only exit-0 check).

**DEFERRED — need LIVE verification (won't change blind; a wrong value is worse than a known-stable one):**
- **#2 main adapter `CONTEXT_SIZE=32768`** — the correct value is the GOVERNOR'S PLANNED ctx (what the
  server actually runs at), not the GGUF max (using max would over-budget `assemble_context` if the server
  runs smaller). Needs the startup sequence wired + an app smoke test.
- **#2 sampling params** (`GEN2_MIN_P`/`GEN25_TOP_K`/`REPEAT_PENALTY`) — route through `EnginePrefs`
  (governor/user); plumbing + live inference check.
- **#4 VM literals → `VmConfig`** (user/host/port/geometry/`DISPLAY`) — runtime VM path; needs a guest boot.
- **#9 VLM patch grid** — derive from the encoder grid; latent (patch path unwired), needs the FFI path.

**Re-scoped after closer look (NOT a blind box-tick):**
- **#8 modal rail in `reload_into_focus`** — a *blanket* post-load dialog-dismiss is UNSAFE (wrong-button:
  a load dialog may need keep/convert/don't-show, not Escape). The class-general primitives are PREVENTION
  (produce a clean file, e.g. strip ICC) + SURFACING the signal (`reload_into_focus` already returns the
  verified active window so the caller sees a stolen focus). Instance-specific dialog handling stays in the
  proof script. So no blanket-dismiss was added — adding it would reduce safety to satisfy a checkbox.

**Remaining (lower-risk, next batch):** #6 split `extract_to_file` / reconsider `larger_than_1k`;
#7 retire/re-contract `recovery.rs`'s dead `{"tool":...}` JSON + delete the dead VLM spawn.

## Standing rule (the way of thinking, generalized)
The instance-instinct appears at execution boundaries. For every line touching the VM, the model, or the
evaluator, ask: *does this name a class, or the one instance I'm looking at?* If it can only name the
instance — discover it (`$HOME`, GGUF, probe), defer it (config/governor/user), or push the variability to
the model. New code that names an instance is rejected, not merged.
