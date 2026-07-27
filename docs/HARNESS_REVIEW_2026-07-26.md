# Harness review — last committed state (`4ceb8a1`, branch `Harness`)

Reviewed 2026-07-26 against the code, not the docs. Every claim below is either **VERIFIED**
(I read the call site / ran the command) or labelled as an **ESTIMATE**. Where this file
contradicts `docs/CURRENT_STATE.md` or `HARNESS_WORK_PLAN.md`, the code wins and I say so.

---

## 0. Verdict in four lines

- The **core is healthier than the docs imply**: 362/362 lib tests pass, `cargo check --workspace`
  is clean, CV→fusion is genuinely wired (the doc's own UNVERIFIED row resolves TRUE).
- The **measurement layer is worse than the docs imply**: Phase 1 instrumentation is 0% started,
  the atlas's false-pass detector is still structurally blind, and **the full-run raw data is gone**.
- The **24/368 headline was produced by a binary that predates both integrity fixes** and has never
  been rebuilt. It is stale in a direction nobody has measured.
- **72% is arithmetically unreachable without chrome + multi_apps** (147 of 368 tasks). The one
  domain with everything built sits at 40%. That gap — not the missing planes — is the real work.

---

## 1. Build state — VERIFIED

| Check | Result |
|---|---|
| `cargo check --workspace` | clean (warnings only) |
| `cargo test -p lagado-agent` | **362 passed, 0 failed, 7 ignored** |
| Rust source | 25,101 lines, 369 test fns across 40 files |
| Python (excl. venv) | 16,990 lines |
| TODO / FIXME / `todo!()` in Rust | **zero** |
| Lib warnings | 31 |
| Bin warnings | **610** — `src/main.rs` is a 54-line dev stub that `mod`-declares every module; that alone generates them |
| Uncommitted | `python/reflex/being/being_v1.pt` + report (untracked) |

`target/debug/osworld_run` is dated **2026-07-10 15:01**. `src/agent.rs` and `src/config.rs` are
newer. **The bench binary on disk is older than the integrity fixes.**

---

## 2. Findings — where the code contradicts the docs

### F1 · CV is default-ON and selection-inert — NOT stated anywhere · **HIGH**

`agent.rs:2819–2849` genuinely feeds CV boxes into `arbiter::fuse()`, and `fused` genuinely
reaches `selection::build_candidates`. That resolves `CURRENT_STATE.md` §1's UNVERIFIED row: the
wiring is **real**.

But `selection.rs:253`:

```rust
pub fn goal_matches_any(goal: &str, candidates: &[Candidate]) -> bool {
    ...
    candidates.iter().any(|c| relevance(&g, &c.label).0 > 0)
}
```

CV boxes carry **no label**. `relevance` against an empty label is 0. So a CV box can never
satisfy the gate. On an a11y-blind screen the candidate list is non-empty, `goal_matches_any` is
false, `subgoal_stuck` climbs, and the loop exits via `PerceptionBlind` → handback (`agent.rs:2893–2923`).

**CV cannot rescue a single blind screen today.** The docs call this "selection safety is
mechanism-guaranteed" — correct, but the flipside is unstated: without a captioner, the CV sense
contributes nothing to action. It only inflates `world.note_a11y_read` coverage.

This is your own AX-blind finding coming due: *captioning is a required sense, not an optional one.*
`LabelSource::Caption`/`::Ocr` exist and are unit-tested; nothing populates them. `patch_embd`
attaches and has zero consumers (`agent.rs:2816` says so explicitly).

### F2 · Phase 1 instrumentation is 0% started · **HIGH**

`git diff cc3c941..HEAD --stat`: 21 files, and **the only code file is `agent.rs` (+78)** — the two
integrity fixes. Everything else is documentation. `failure_atlas.py` is untouched since `9429ad0`.

The work plan says "instrumentation first, do NOT reorder." Nothing has been reordered — nothing
has been *started*. The last five commits are all docs.

### F3 · The false-pass detector is still structurally blind · **HIGH**

`failure_atlas.py:79` greps the chronos trace for `"self_report_done: True"`, `'"self_report_done": true'`,
or `"goal accomplished"`.

Grepped every Rust file: **no `chronos::log` call emits any of those three strings.** The only
producers of the `self_report_done` key are `calc_solve.py` / `writer_solve.py` / `impress_solve.py`,
which emit it as JSON **to stdout** — which the atlas never reads.

Audit finding #4 stands, unfixed. The detector cannot fire. The "1 false pass" in the run output
was a *default*, not a measurement.

### F4 · The trace join still bleeds across tasks · **MEDIUM**

`split_tasks` keys segments on the first 70 chars of the goal (`:54`), then **concatenates** every
segment sharing a prefix (`:62`), then matches by bidirectional substring (`:114`). Retries and
distinct-but-similar instructions merge into one blob. Every per-task categorisation downstream
inherits that contamination. Unfixed.

### F5 · The headline number predates its own fixes · **HIGH**

```
cc3c941 2026-07-10  Full OSWorld run: 24/368 gold        ← the number
d870f24 2026-07-10  INTEGRITY FIX: sub-plane may not declare whole-agent FAIL
7c09f75 2026-07-11  INTEGRITY: fail-closed on unverifiable completion
```

Both fixes land **after** the run. The 24/368 was measured with the vacuous-`complete_goal`
false-pass generator live *and* the sub-plane false-FAIL bug live — errors in opposite directions,
neither quantified. The number is not a floor or a ceiling; it is unanchored.

### F6 · The full-run raw data no longer exists · **CRITICAL for planning**

`/tmp/lagado_battery/` is gone. `full_single.jsonl` and every `solve_*.json` — the per-task scores,
flags, and authoring dumps — are unrecoverable. No copy anywhere in the repo (`find` over the whole
tree returns nothing).

What survives is the per-domain table in `docs/osworld/FULL_369_RESULTS_2026-07-10.md`. Per-task
score, per-task flag config, and every solve dump are lost.

`~/.local/share/lagado/chronos.log` still exists but is a **single rolling append file, 4.7 MB, last
written 2026-07-26** — the run's trace is interleaved with every session since.

**Consequence: Phase 2 (the re-audit) cannot be run on existing data. It requires a full re-run of
all 369 tasks.** The work plan reads as though the re-audit is a desk exercise on captured data. It
is not.

Systemic cause, will repeat: run artifacts are written to `/tmp` and chronos is one unrotated global
file. Fix both before the next run, not after.

### F7 · The config-mixture problem is already half-solved and nobody noticed · **quick win**

`osworld_run.py:128` writes `{"domain","id","score","flags"}` per task — **the flag set is recorded
on every row.** But `failure_atlas.py:107` reads only `domain`, `id`, `score` and ignores `flags`
entirely.

Audit finding #6 ("the run mixed 3 flag configs") is therefore a **one-line group-by** in the atlas,
not a methodology crisis — for the *next* run. It can't retroactively rescue this one (F6).
Do it before re-running so the next run is sliceable by config from the start.

### F8 · Invariant #9 is violated on the live bench path · **MEDIUM**

`config.rs:117` — `pub const CONTEXT_SIZE: usize = 32768;`

`osworld_run.rs:46` passes it straight into `LlamaCppAdapter::with_url(...)` with no GGUF discovery.
The benchmark brain is Qwen2.5-Coder-7B; the constant was written for LFM2-8B. This is exactly the
"latent bug when the model is swapped" the invariant exists to prevent — and the model *has already
been swapped*.

The discipline exists elsewhere and works: `bootstrap.rs:56` does
`model_max.map_or(CLASSIFIER_CONTEXT_SIZE, |max| CLASSIFIER_CONTEXT_SIZE.min(max))` for the classifier.
The main brain never got the same treatment.

Also: `liquid.rs:27–33` hardcodes four LFM2.5 GGUF filenames. Harmless (orphaned — see §3) but it
is dead code carrying a hardcode.

### F9 · Chrome CDP is read-only · **HIGH (biggest single lever)**

`perception/dom.rs` reads the DOM via CDP `Runtime.evaluate`. Grepped all of `src/` for CDP
actuation — click, type, navigate: **none exists.** The audit named chrome as the suspected top
lever and it remains entirely unbuilt on the action side. 46 chrome tasks + ~25 of 101 multi_apps.

### F10 · `CURRENT_STATE.md` covers roughly 40% of the codebase · **MEDIUM**

Its six sections are all benchmark-shaped. It says nothing about `memory_tiers.rs` (882),
`recovery.rs` (876), `governor.rs` (676), `hydra.rs` (646), `skill_library.rs` (467), `board.rs`,
`forge.rs`, `distill.rs`, `self_model.rs`, `sleep_gate.rs`, `auth/`, `security/`, `mcp/`,
`projector/`, `terminal/`. That is several thousand lines with no current-state coverage. §3 below
fills it in.

---

## 3. Module inventory — all 45 Rust modules, classified by reachability

Classification is mechanical: call sites outside the module's own file, then whether the caller is
on the bench path (`bin/osworld_run.rs` → `agent::agent_loop`), the app path
(`lagado-ui/src-tauri` → `hydra::run` → `agent::agent_loop`), a probe, or nothing.

**Both paths share the same core.** `hydra.rs:429,484` calls `agent::agent_loop`. The app is not a
separate agent — it is a different front door onto the same loop. That is a better position than
the docs suggest.

### WIRED — on the default bench path (`agent.rs` calls them directly)

| Module | LoC | Role | Note |
|---|---|---|---|
| `agent.rs` | 4061 | the sequencer | most-hardened file; 56 tests |
| `perception/` (mod, arbiter, selection, cv_proposer, delta, world, canvas, frame, dom, vlm_adapter) | ~2900 | fused senses | see F1 |
| `plane.rs` | 530 | richest-first dispatch | `dispatch_verdict` logged, control does NOT flip yet (measure-first) |
| `api_plane.rs` | 527 | UNO surface | calc proven; writer/impress gated |
| `supervisor.rs` | 357 | stall/escalation tiers | |
| `governor.rs` | 676 | hw-aware server config | |
| `recovery.rs` | 876 | action recovery | 18 tests |
| `action_graph.rs` | 479 | outcome graph | |
| `gate.rs` | 308 | **HITL chokepoint** (invariant #3) | auto-approves in bench |
| `tools/` (mod, executor) | 991 | tool execution | |
| `memory.rs`, `memory_tiers.rs` | ~1000 | episodic/semantic tiers | app-era; on bench path but low-value there |
| `chronos.rs` | 395 | trace log | **single unrotated global file — see F6** |
| `back_door.rs` | 285 | settings executor | GATED `LAGADO_BACKDOOR` |
| `native_session.rs` | 168 | persistent session cache | |
| `operator.rs`, `envelope.rs`, `grammar.rs`, `types.rs`, `config.rs` | ~1100 | plumbing | |
| `vision/mod.rs` | 434 | libmtmd FFI patch encoder | wired, **output consumed by nothing** (F1) |
| `forge.rs`, `skill_library.rs` | ~700 | skill distillation / advisory | Board-advisory only (invariant #10) |

### APP-PATH ONLY — reachable from Tauri, not from the bench

`hydra.rs` (646, the router/pipeline), `sleep_gate.rs` (290), `server_guard.rs` (329),
`sysinfo.rs`, `bootstrap.rs` (358), `auth/` (214+crypto), `security/` (sandbox, linux),
`mcp/` (client), `board.rs` (205), `retrieval.rs` (243), `embedding.rs`, `gguf.rs` (300).

These are alive and tested, just not exercised by OSWorld. They are the shipping vehicle.

### PROBE-ONLY

`perception/frame.rs` (first_walk, harness_proof) and the 24 binaries in `src/bin/` — of which
**exactly one (`osworld_run`) is the official-bench entry point.** The other 23 are research probes.
They compile on every build and are the main source of the 610 bin warnings.

### ORPHANED — declared in `lib.rs`/`main.rs`, zero callers anywhere

| Module | LoC | Verdict |
|---|---|---|
| `liquid.rs` | ~50 | **stub** — `select_model()` ignores its args and always returns `Lfm25_8B`; `vision_available()` returns `false`. Hardcodes 4 GGUF filenames. Confirms `CURRENT_STATE.md` §5: **no CfC exists in Rust.** |
| `projector/` (mod, executor, validator, platform_linux) | ~400 | app-era screen projection; not called |
| `distill.rs` | — | not called |
| `self_model.rs` | 188 | not called |
| `kv_slots.rs` | — | not called |
| `terminal/` (mod, pty) | — | not called |

Each is a decision at finish time: complete it or delete it. `liquid.rs` in particular is a
placeholder whose name promises the CfC integration that §5 of `CURRENT_STATE.md` says doesn't exist.

### Python

- `python/osworld/` (~9,000 lines) — the real second half of the harness. `battery_calc.py` (2965)
  is the proven plane; `uno_ops.py` (868) is the 22-kind op vocab; `writer_ops.py`/`impress_ops.py`
  built 2026-07-10, **never run against a live guest**, gated off.
- `python/reflex/` — settle CfC, eyes, hands, being, membrane. Genuinely disciplined ML work
  (timer-null baselines, held-out CV, fail-closed promotion gate). **Connected to Rust by exactly one
  wire**: `perception/canvas.rs` reads the shared-memory BGRX buffer that `reflex/membrane/canvas_feed.py`
  maintains. No CfC inference crosses into Rust. Two systems, one shm handoff.
- `python/guest/cdp_dom.py` (177) — the DOM read. Read-only (F9).
- Root `perceive.py` (22 KB, 2026-06-19) — the legacy AT-SPI walker, superseded by the Rust
  perception stack. Not a duplicate of `python/guest/`; it is a leftover. Delete or archive.

---

## 4. What "finish" means — two separate bars

### Bar A · The benchmark bar (your stated success criteria)

> 72% on real OSWorld with a ≤7B model, zero false-pass, single logged flag config,
> model-vs-harness attribution measured.

**The arithmetic nobody has written down.** 72% of 368 = **265 golds.**

| Domain | Tasks | % of bench | Best measured | Needed for 265 |
|---|---|---|---|---|
| multi_apps | 101 | 27.4% | 1 | ~60 |
| libreoffice_calc | 47 | 12.8% | **19 (40%)** | ~38 |
| libreoffice_impress | 47 | 12.8% | 0 | ~34 |
| chrome | 46 | 12.5% | 1 | ~33 |
| gimp | 26 | 7.1% | 0 | ~18 |
| os | 24 | 6.5% | 3 | ~17 |
| libreoffice_writer | 23 | 6.3% | 0 | ~17 |
| vs_code | 22 | 6.0% | 0 | ~16 |
| vlc | 17 | 4.6% | 0 | ~12 |
| thunderbird | 15 | 4.1% | 0 | ~11 |

*(Per-domain figures from `FULL_369_RESULTS_2026-07-10.md`, carrying its caveats: mixed flag
configs, pre-integrity-fix binary. They are the only surviving per-domain data — see F6.)*

Two structural facts fall out:

1. **chrome + multi_apps = 147 tasks = 40% of the bench.** Even at **100%** on all eight other
   domains (221 tasks) you land at 221/368 = 60%. **72% is unreachable without them.** The audit's
   instinct that chrome is the bigger lever than Impress is not a preference — it is forced.
2. **The one domain where everything is built sits at 40%.** 72% overall requires roughly
   *calc-quality × 1.8, everywhere.* Building nine more planes to calc's standard gets you ~40%
   overall, not 72%.

So "finish" is **two problems, not one**: breadth (nine missing planes) *and* depth (the 40%→72%
per-domain ceiling). Only the breadth problem is currently in the work plan.

### Bar B · The ship bar

`lagado-ui` is 91 TS/TSX files, 7,551 lines, plus a Tauri shell that reaches `agent_loop` through
`hydra::run`. The core is shared, so shipping is not a rewrite — it is re-exercising a path that has
had no attention since the harness phase began (2026-06). Unknown-condition, not unbuilt.

---

## 5. The critical path to Bar A

Order is forced by dependency, not preference. Everything in Stage 0 must precede everything else,
because right now **you cannot measure whether any change helped.**

### Stage 0 · Make measurement trustworthy — *nothing else can start*

| # | Work | Why |
|---|---|---|
| 0.1 | Move run artifacts out of `/tmp` into a run-stamped dir under the repo or `~/.laputa-secure` | F6 — this is why the last run's data is gone |
| 0.2 | Per-run chronos file, not one global append log | F6 — traces are currently interleaved with every session since |
| 0.3 | Emit a positive completion assertion to chronos (`self_report_done: true`) at every `complete_goal` success site | F3 — the detector greps for a string nothing writes |
| 0.4 | Atlas reads per-task stdout/stderr, not just chronos | F3 |
| 0.5 | Replace the 70-char prefix join with the task id (already in the jsonl) | F4 |
| 0.6 | Atlas groups by `flags` (one line — the field is already recorded) | F7 |
| 0.7 | Separate infra/setup failures and blank-output no-engage tasks out of the denominator | audit #5 |
| 0.8 | `cargo build --release --bin osworld_run` — the on-disk binary predates both integrity fixes | F5 |

**ESTIMATE: 2–4 working sessions.** All of it is small, local, and testable without a VM.

### Stage 1 · Re-run and re-audit — the number that anchors everything

Full 369 tasks, single logged flag config, fresh VM per task, on the rebuilt binary. This is the
first honest number the project will have had. It also produces the first evidence-based fix-class
histogram — which is the input to every decision after this point.

**ESTIMATE: 1 session to launch + wall-clock for the run (the 2026-07-10 run was single-lane;
budget accordingly), then 1–2 sessions to audit.**

Do **not** skip to Stage 2 on the strength of the old histogram. F6 means it cannot be reconstructed;
F3/F4 mean it was never evidence-based.

### Stage 2 · The two forced levers (in this order)

| Lever | Tasks touched | Why it's first |
|---|---|---|
| **Chrome CDP actuation** | 46 chrome + ~25 multi_apps ≈ 71 | Sight already exists (`dom.rs`); only action is missing. Highest tasks-per-unit-work in the bench. |
| **Captioner / OCR into `LabelSource::Caption`** | all GUI domains | F1 — unlocks the CV sense the harness already computes and currently discards at the selection gate. Without it, every a11y-blind screen is a handback, in every domain. |

These two are the *depth* fix, and they are why calc's 40% is 40%.

**ESTIMATE: 3–6 sessions each.**

### Stage 3 · Validate the built-but-unvalidated planes

`writer_ops.py` and `impress_ops.py` exist and are rigorous, but have never touched a live guest.
Validate via the engineering loop: run real tasks → read the failure trace → refine → re-run.
Named first checks already flagged in the work plan: the Impress colour-name→RGB table (false-pass
risk) and Writer subscript/highlight serialisation.

**ESTIMATE: 2–4 sessions each.** Cheaper than building, but this is where the false-pass risk lives.

### Stage 4 · The remaining planes

gimp (26), vlc (17), thunderbird (15), vs_code (22). No plane exists for any.

**ESTIMATE: the calc plane took roughly three weeks of sessions from M1 (2026-06-21) to its 40% run
(2026-07-10). Apply that as the unit and adjust down for the ones with a scriptable surface
(vs_code has a CLI; vlc has an HTTP interface; thunderbird has profile files) and up for gimp
(script-fu, but a hostile GUI).**

### Stage 5 · multi_apps composability

101 tasks, 27% of the bench, and the largest single block. It is not a plane — it is the *chaining*
of planes plus browsing. It cannot start until several planes exist, and it is where the sub-plane
false-FAIL bug (`7c09f75`) originally surfaced. Treat it as its own arc, not as a side effect of
building planes.

### Stage 6 · Hygiene, before you call it done

- Resolve the six orphans: complete or delete `liquid.rs`, `projector/`, `distill.rs`,
  `self_model.rs`, `kv_slots.rs`, `terminal/`. `liquid.rs` should either become the CfC bridge its
  name promises or go.
- Fix F8: discover `CONTEXT_SIZE` from GGUF metadata on the brain path, as `bootstrap.rs:56`
  already does for the classifier.
- Trim `src/main.rs`'s blanket `mod` declarations (kills ~600 warnings) or move the probes behind a
  feature flag.
- Delete or archive root `perceive.py`.
- Commit or gitignore `being_v1.pt`.
- Update `CURRENT_STATE.md` to cover the app-path modules (F10).

---

## 6. Honest risk register

| Risk | Assessment |
|---|---|
| **72% may require more than planes** | The 40% ceiling on a fully-built domain is the strongest evidence in the repo about what the harness can do, and it says the per-domain ceiling — not the plane count — is the binding constraint. Stage 2 exists to test that. If chrome actuation + captioning don't move calc-class domains past 40%, the ceiling is somewhere you haven't looked yet. |
| **Re-running costs real wall-clock and heat** | Fresh VM per task × 369, single lane, GPU under 85°C. This is a days-long commitment, not an afternoon. Budget it explicitly rather than discovering it. |
| **False-pass count is genuinely unknown** | ≥6 was a floor with 117/368 tasks having no trace coverage. After the `complete_goal` fix it should be lower — but "should be" is exactly the kind of claim the 2026-07-10 audit demolished. Stage 1 measures it; nothing before Stage 1 should assert it. |
| **Solo-maintainer surface** | 25k lines of Rust + 17k of Python + a 7.5k-line UI, one person. The orphan list and the 23 unused probe binaries are the visible edge of that. Deleting is as much "finishing" as building. |
| **Artifacts in `/tmp` will bite again** | It already cost the entire raw record of the most important run the project has done. Stage 0.1 is not housekeeping. |

---

## 7. What's genuinely strong — for calibration, not comfort

These are the parts a reviewer would find hard to argue with:

- **Fail-closed by construction.** `complete_goal` refuses to claim success without a verifiable
  postcondition (`agent.rs:278`). The harness under-claims: 40 of 43 solver invocations exited 2
  (operated, could not corroborate, handed back) — and the official evaluator golded many anyway.
  The calibration error sits entirely on the safe side.
- **Sound-only falsifiers + independent re-derivation corroboration** in the calc plane.
  `battery_p3.py`'s adversarial test (deliberately wrong-but-plausible formula) demonstrates the
  corroboration catching what falsifiers alone miss.
- **Ablation discipline is real**: five capabilities behind explicit flags, each with a written
  contract in `config.rs` stating it joins the default path only after its A/B delta is measured.
  That is rarer than it sounds.
- **The perception arbiter is properly engineered** — IoU dedup, label-provenance priority,
  documented edge-fuzz/mean-pool contract, 28 tests including a pinned test that label-less boxes
  cannot change selection.
- **You commissioned an adversarial audit and then published its refutation of your own headline
  claim, in the file carrying that claim.** `FULL_369_RESULTS_2026-07-10.md` opens with a correction
  banner against itself. That is the single most credible artifact in the repo.

---

## 8. The one-paragraph answer

Finishing Lagado is not nine more planes. It is: **(a)** fix the measurement layer so a number means
something — 2–4 sessions, and nothing else can honestly start first; **(b)** re-run the full bench on
a rebuilt binary to get the first anchored number the project has ever had; **(c)** build the two
forced levers, chrome actuation and captioning, because 40% of the bench is unreachable without the
first and every blind screen is a handback without the second; **(d)** then grind the remaining
planes, at roughly the calc plane's cost each; **(e)** then multi_apps, which is its own arc.
Bar B — shipping — is smaller than it looks, because the Tauri app and the bench share one
`agent_loop`. The number to keep in front of you is not 24/368. It is **40% on the one domain where
everything is built**, against a 72% target: that ratio is the whole remaining problem, and it is a
depth problem that the current work plan doesn't yet name.
