# THE HARNESS — COMPLETE MAP (2026-07-06)

Compiled from a full line-level read of every file in the working system (four independent
code-reading passes, line references throughout). This is the machine as it actually is —
the system that scores 20/30 on held-out OSWorld Calc tasks with zero false passes.

---

## 0. THE MACHINE AT ONE GLANCE

```
HOST (Fedora, 15GB RAM, one GPU)
│
├── THE BRAIN  llama-server :8080  (start_brain.sh)
│     Qwen2.5-Coder-7B Q4_K_M, full GPU offload, --no-mmap, --parallel 1,
│     ctx 4096, q8_0 KV, --embeddings --pooling last
│     serves BOTH /v1/chat/completions (grammar-constrained emission)
│     and /v1/embeddings (the model's own latent space, for binding)
│
├── THE PIPELINE  battery_calc.py (2944 lines) — runs on the host
│     detect → cards → REASON → EMIT (grammar) → parse → resolve fail-closed
│     → apply (groundings/withholds) → read-back FALSIFIERS → fact-only feedback
│     → retry/resample → claim gate (corroborate) → reconcile → settle → evaluate
│
├── THE MONITOR BANK  reflex/ — tiny CfC experts (37k params, CPU)
│     settle monitor v1 at the seam (floor-clamped), v2 in gate iteration
│
├── THE SWEEP RUNNER  battery_breadth.py — per-task attribution, integrity ledger
│
└── THE ARENA  podman container (rootless, patched provider)
      └── nested QEMU VM (Ubuntu, 3GB) — the OSWorld guest
            ├── :5000 HTTP  /execute /screenshot  (OSWorld's own channel)
            ├── THE SESSION DAEMON  uno_daemon.py + uno_ops.py (deployed per task)
            │     owned soffice, UNO socket, /tmp/lagado_session.sock
            └── env.evaluate()  ← THE ORACLE (official OSWorld metrics, untouched)
```

One sentence: **a small local model is allowed to make exactly one kind of contribution —
choosing typed operations by name — and deterministic machinery owns everything else:
perception, binding, application, verification, retry, and the honesty of the final claim.**

---

## 1. THE BOX — VM + networking (provider_fedora_rootless.patch / provider_patched.py)

- OSWorld's DesktopEnv boots `happysixd/osworld-docker` via **rootless podman**
  (`DOCKER_HOST=unix:///run/user/1000/podman/podman.sock`). Inside the container runs a
  **nested QEMU VM** (Ubuntu + GNOME) — the actual desktop the tasks live on.
- Patch deltas vs upstream (each was a measured failure): container trimmed to **3GB RAM /
  2 CPU** (15GB host); `_wait_for_vm_ready` 300→**900s**; `/dev/net/tun` passed through
  (without it: usermode networking, no port-forward, guest unreachable);
  `security_opt label=disable` (SELinux blocks /dev/kvm+tun rootless); `privileged` +
  `ip_forward` sysctl (qemu-docker NAT); volume mode `ro,z` (SELinux relabel).
- **The rootless DNAT fix** (the subtle one): rootless podman's port forwarder connects
  *inside* the container netns, so traffic never hits PREROUTING and qemu-docker's DNAT to
  the nested guest (20.20.20.21) never matches. Fix: after the container's own entrypoint
  finishes flushing/creating its nat table (poll up to 180s for its rule to appear), inject
  **OUTPUT-chain DNAT** for ports 5000/9222/8080 → 20.20.20.21, + MASQUERADE on dockerbridge.
- Guest :5000 = OSWorld's server: `/screenshot` (readiness + observation), `/execute`
  (shell/python in guest). This is also the transport our host code uses (`Guest.py/sh`).

## 2. THE BRAIN — start_brain.sh

Qwen2.5-Coder-7B-Instruct **Q4_K_M** on llama-server :8080. Every flag is a scar:
- `--no-mmap`: mmap'd GGUF pinned ~7GB cold host pages → zram → the 3GB nested VM couldn't
  boot. no-mmap loads, uploads to GPU, frees the host copy (MemAvailable 4.7→11GB).
- `--parallel 1`: multi-slot batching made temp-0 same-seed outputs *vary*. One slot =
  reproducible draws, so best-of-N seed diversity is controlled diversity.
- `-c 4096`: the EMIT prompt hit 2035 tokens; ctx 2048 truncated mid-string. q8_0 KV keeps
  the cost small.
- `--embeddings --pooling last`: the SAME server serves embeddings in the reasoner's OWN
  latent space — last-token pooling is the binding lever (R1b). One model, three uses:
  reasoning, grammar-constrained emission, semantic binding.
- Model-seat defenses (all measured, all rejected): IQ4_XS (−1 net gold), Qwen3-4B
  (binding 5/10 vs 8/10 on the frozen fluency panel), compact positional grammar (first
  false pass). The kwarg names in the grammar are semantic slot-anchors.

## 3. THE SESSION PLANE — uno_daemon.py (462) + uno_client.py + run_session_task.py

Deployed into the guest **per task** (`deploy_daemon`: base64 the three files, launch
detached, poll for `DAEMON READY`). It is the "make the app AI-native" back door:
- **Owned soffice**: per-PID isolated `UserInstallation`, recovery/autosave pre-disabled
  via seeded xcu, headless by default (`LAGADO_VISIBLE` shows the real window). Kill by
  Popen handle only — never a global pkill (the ×8 lesson).
- **Verbs over a unix socket** (JSONL, one request per connection): `open` (identity guard,
  enables live recalculation), `apply` (dispatch to uno_ops; failed ops never enter the op
  log), `read` (typed cell matrix), `structure` (sheets, extents, headers, per-column
  date/number classification with a belt-and-suspenders format check), `health`,
  `reconcile`, `close`.
- **`reconcile` — the TURN-16 seam** (the one that was mislabeled "render parity" for
  months): calculateAll → storeToURL as real XLSX → stdlib zip patchers fix what LO's
  export drops (font theme→rgb always; freeze panes, zoom, per-cell font colors from the
  ops' recorded `_matched` cells) → kill the headless instance, clear the lock → relaunch a
  **GUI soffice under a separate isolated profile**. Why separate: with the default profile
  the evaluator's own `--convert-to` forwarded into the GUI instance blocked by the
  "keep format?" modal → CSV never written → auto-0 on every render task.
- **Host transport** (`Guest`): `py()` → OSWorld's `/execute` python channel; `sh()` wraps
  shell with JSON-encoded out/err/rc; `client()` runs uno_client.py inside the guest against
  the daemon socket. `memory_ok()` = 4500MB MemAvailable floor before any VM boot.
- **Clobber avoidance**: `env.reset` opens the task file in its own GUI soffice holding the
  lock; the run kills it before the daemon takes ownership.

## 4. THE OP VOCABULARY — uno_ops.py (868)

24 kinds in `apply_one_op`, structural ops ordered first. Notable mechanics:
- `set`: type-into-cell semantics — a leading `=` string becomes a formula (without this,
  formulas land as text and score 0). `excel_to_calc` translates `!`→`.` and `,`→`;`
  outside string literals.
- `set_formula_range`: seed + `fillAuto` (fill-handle semantics, relative refs adjust).
- `sort_range`: **in-memory sort** — UNO's SortDescriptor silently no-ops on this build
  (measured). Numbers before text, header skip, write-back.
- `format_cells` / `format_cells_where` (weekend/max/exact-text predicates): record
  `_matched` cells because LO's xlsx export **drops programmatic font colors** — reconcile
  re-imposes them by patching styles.xml per cell.
- `freeze_panes` / `set_zoom`: headless has no view (setViewData crashes pyuno) — validated
  at apply, actually **written into the saved xlsx by stdlib zip patchers** at reconcile.
- `create_chart`: idempotent by name, exact anchor from cell geometry (charts stopped
  landing on tables), diagram-type mapping with the Vertical-flag inversion documented,
  `DataRowSource` forced (auto-detect mis-oriented).
- `create_pivot`: a REAL DataPilot (the saved xlsx carries the OOXML pivot part the
  evaluator reads back); count-by-self = same field in rows+data; each pivot gets its own
  10-column band; output row 2 (a pivot at A1 clobbered a written title — measured).
- `dedup_column`, `transpose_range` (python zip transpose), `hide_rows_where` (hide never
  delete, error-32767 NA detection), `set_decimal_separator` (locale-keyed format codes,
  measured per-column precision; values untouched), `export_csv`/`export_pdf` (sheet-name
  misbind grounding on the output name).

## 5. THE PIPELINE — battery_calc.py (2944). One task's life:

**(a) Perceive.** `structure` → `detect()`: read up to 400 rows/sheet; `find_header_row`
(text row followed by numeric row); `segment_regions` — multi-table segmentation by blank
row-blocks × column-groups, per-region headers/data-span/title, small tables carried whole.
`candidate_cards` renders it for the model — single-table cards deliberately OMIT row spans
(A/B-measured: stating spans fixed one task, deterministically broke another 3/3→0/3;
range robustness is owned at apply, not in prompt wording).

**(b) Reason.** `REASON_PROMPT` — goal + cards + "think step by step", nothing else. No
solution schema; the de-leading reckoning proved a decomposition schema is cheating.

**(c) Emit.** `EMIT_PROMPT` = goal + cards + the model's own analysis + **the manual** (26
verbs with usage docs — the single unconditional teaching surface) → grammar-constrained
generation (GRAMMAR_B: strings exclude newlines; temp 0, seed 7). **Static best-of-N**: if
the draw has statically detectable defects (unbalanced parens, count shortfalls, untouched
goal-named columns), redraw at temp 0.35 then 0.7, keep the cleanest.

**(d) Parse, don't trust.** `scan_calls` (paren-depth walker respecting quotes) →
`parse_kv` → nameops with `{Header}` names UNRESOLVED — binding happens at apply time
against live re-detected structure.

**(e) Apply = the grounded gauntlet** (`apply_B`). Per op, in order: sheet grounding
(exact → case/space-insensitive unique → single-book rebind); syntax ownership
(`_balance_trailing_parens`, quote normalization); the **withholds** (duplicate-header,
goal-echo, multi-table overwrite — each rejects with a fact and a `rejected_key` so it's
never re-proposed); the **fill-shape groundings** (a first-data-row formula over an empty
column = the fill-down gesture; the row form = fill-right; aggregate-extent alignment);
`ground_bare_refs` + `resolve_col` (notation-robust: header/letter/index, unique-or-fail-
closed, live-read disambiguation among duplicate headers); `compute_row`'s deterministic
host-side column shifting (never fillAuto); chart grounding (type-from-instruction-phrase,
range rebuild, orientation from geometry, span unification, trailing trims, EMPTY-range
fail-closed); pivot name→index resolution (all-or-nothing). Everything the model *names*
is bound by the harness or refused.

**(f) Falsify — never confirm.** Sound detectors over what was actually written:
`error_values` (#faces, formula writes only), `text_formula_numeric` (the silent '_'→0
collapse), `extent_shortfall`, plus goal-contract falsifiers: `named_target_empty` /
`column_fill_incomplete` / `structural_target_holes` (contract-compiler v1),
`style_contract` (goal-verbatim hex + property), `pivot_orientation` ("as the column
headers" vs the built layout), `text_decimals` (goal-stated decimal count vs the written
text). Result-preview attaches the model's own output beside its own input. An empty
falsifier list means "no detected fault", never "correct".

**(g) Feedback = FACTS ONLY** (user ruling 2026-07-06: suggesting IS leading). Observed
state + goal-contract mismatches. All teaching lives in the manual. A gold produced with a
prescription in the loop is not claimable.

**(h) Retry machinery.** Two-attempt loop with op-merge by `_op_key` (new corrects same-key,
different-key adds; viz ops re-ordered last); **divergence resample** (DSpark shape):
prefix stays committed, each localized fault gets ONE targeted single-op resample, counted-
artifact collapses re-emit per section of the model's own reasoning; then the **iterative
floor**: one op per step against live re-presented state, `done()` escape, one FORCED step
when the model rubber-stamps done over open problems, dependency re-apply after.

**(i) The claim gate.** `no_fault` (wrote something, nothing fired, nothing unresolved) AND
`corroborate` (an independent re-derivation at temp 0.6 must agree on the *column-level
dependency sets* of every written formula) → only then does the harness report done.
`false_pass = reported_done AND oracle<1` — the number that must stay zero.

**(j) Endgame.** `reconcile{gui}` → `settle_wait` (monitor, floor-clamped ≥4s) →
`env.evaluate()` — the official oracle: postconfig (activate window by exact title, ctrl+s,
maybe --convert-to), fetch result from VM + gold from HuggingFace, run the real metric
(`sheet_data` = whole-sheet df.equals at 4 decimals — the rule that makes helper columns
fatal; `sheet_print` = exact CSV lines; pivot_table = the pivot OBJECT's field layout).
Infeasible-marked tasks: a sole `infeasible()` emission mirrors the official FAIL scoring —
right answer 1.0, wrong declaration 0 *and* sets false_pass.

## 6. THE MONITOR BANK — reflex/ (the CfC subcortex, expert #1)

- **Senses (52 dims)**: 8×6 pixel-grid changed-fractions + whole-frame (480×270, eps 12) +
  window-list-changed + window count/8 + process count/8. Log10-scaled pixels at the CfC
  input only; real inter-tick dt as CfC `timespans`.
- **Recorder** (guest_rec v9, in-guest): compositor present-forcing (gdbus Eval or input
  nudge — the headless guest presents only for an audience), unlink-first capture (dropped
  frames are blind gaps, never stale), multi-channel mandatory, UNO stimuli threaded with
  the synchronous return as the **teaching oracle** (t_done). Stimulus times RANDOMIZED
  2–12s (v1's fixed 2.0s was the clock-shortcut trap). Episodes ×13 incl. scroller-churn
  timer-killers.
- **Labels for free**: hindsight — settled at t iff nothing above the calibrated noise floor
  (from quiet/blink episodes) for the next 2.0s; window fused in; uno_reload gets the oracle
  override; frames over capture gaps >1.5s unlabeled; frame 0 masked.
- **Model**: ncps CfC, 48 units, ~37k params, trains 150 epochs on CPU in minutes; false-
  settle weighted 4× in the loss.
- **The gate** (the important part): held-out-round CV; operating point (threshold ×
  patience) selected train-side for zero false-settles; PROMOTE only if the CfC beats **the
  deterministic floor** (K-consecutive-quiet on the same senses) AND **the timer null** (the
  best constant clock, grid-searched) on false-settles AND misses AND latency. The timer
  null is the standing rule: a monitor that can't beat a clock has read nothing.
- **Serving**: settle_service.py under the reflex venv, JSONL over pipes, stateful hidden
  state, 0.2ms/tick; SettleMonitor client is FAIL-OPEN everywhere (any problem → floor).
  TickFeaturizer reproduces the recorder's senses host-side; first frame primes and returns
  None (train/prod parity — the synthetic all-1.0 row was OOD and poisoned the state).
- **Status**: v1 at the seam, **floor-clamped** (may only extend past 4s — early release is
  the one failure fail-open can't catch); v2 HOLD with a review-ordered fix list (per-cell
  busy labels + genuinely strong churn + truth=None-under-churn invariant; dt normalization;
  time-based patience; round-0 label audit). Mechanism truth from the adversarial review:
  v1 senses in-distribution (tracks randomized settles a clock can't) but is blind to
  small-area churn and biased by fast ticks.
- **damage_listener.py**: XDamage push-events sensor v0, validation-only, rides recordings.

## 7. THE INSTRUMENTS

- **battery_breadth.py**: sweep runner; HELDOUT = 30 never-referenced tasks (the clean
  transfer set); per-task: volume prune → memory floor → env.reset → 420s alarm →
  run_condition → **attribution** (GOLD / FAIL-CLOSED / FALSIFIER(name) / ABSTAIN /
  WRONG / SILENT-WRONG=false-pass / SETUP-FAIL / TIMEOUT / EXC); ledger to breadth.json +
  breadth_logs.jsonl; the sweep-level integrity line: false passes must be 0.
- **fluency_panel.py**: the frozen (v1.1) model-seat instrument — 10 binding cases in the
  candidate's OWN embedding space (θ=0.08 fail-closed margin; 4 correct-abstain traps) +
  emission smoke. Baselines: 7B Q4_K_M = 8/10; Qwen3-4B = 5/10 (why the seat didn't change).
- **settle_audit.py**: per-run settle forensics — fail-opens, sub-1s suspects, miss-vs-gold
  settle-tempo (the early-release signature), time saved vs the 4s floor.
- **brutal_settle.sh**: the four-phase adversarial suite (service-kill fail-open; render N=3;
  ambient churn — verified-on-screen or it doesn't count; forced 2s cap seam probe).

## 8. THE INTEGRITY SPINE (what makes any number here worth anything)

1. **Official evaluator only** — env.evaluate on unpatched OSWorld metrics; never
   re-implemented, never approximated for a claim.
2. **Zero false-pass is scored, not assumed** — the claim gate + attribution make a false
   claim a first-class measured failure.
3. **Fail-closed everywhere a name binds** — unresolved ⇒ refused, never guessed.
4. **Falsifiers detect wrongness only** — passing them never asserts correctness.
5. **Fact-only feedback; the manual is the only teaching surface** (suggesting IS leading).
6. **Held-out discipline** — the 30-task set never informed any prompt, threshold, or fix.
7. **Deterministic-over-prompt** — every robustness fix lands at apply/grammar/harness,
   never as prompt nudging (the global-knob brittleness is measured and documented).
8. **Honest numbers** — variance reported, misses classified, refuted claims corrected in
   the record (2026-07-06: two headline claims withdrawn by adversarial measurement).

## 9. FLAGS & RUNBOOK

| Var | Effect |
|---|---|
| LAGADO_SETTLE_MONITOR | "0" disables the monitor (default on; floor-clamped) |
| LAGADO_SETTLE_MAX / LAGADO_SETTLE_DUMP | cap (15s) / tick-photo dir |
| LAGADO_COMPACT_EMIT | rejected compact dialect arm (off; kept as negative result) |
| LAGADO_VISIBLE / _HOLD / LAGADO_WATCH_PAUSE | watch modes |
| LAGADO_BRAIN_MODEL / _PORT / _CTX / LAGADO_LLAMA_SERVER | brain seat overrides |
| LAGADO_RECONCILE_GUI | force GUI reload path |

Ops: always `start_brain.sh` (never bare llama-server); `DOCKER_HOST` to the user podman
socket; volume prune per task (automated in breadth); 4500MB boot floor; pid-file kills,
never `pkill -f` with a filename the command itself carries.
