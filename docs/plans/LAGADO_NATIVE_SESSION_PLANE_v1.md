# LAGADO — Native Session Plane (persistent app-automation session) — Spec v1

**Status:** DESIGN (no code yet). 2026-06-22.
**Purpose:** Make a human-designed app *AI-friendly* by holding its **native automation interface open as a live session**, so the agent drives it ONE op at a time WITH per-op observation — instead of the current blind, monolithic, kill→apply→reload one-shot. This is the "app adaptation" layer (user's term), done as the richest rung of the plane-governor's ladder, **additive over the proven stateless path, never replacing it.**

Companion context: `LAGADO_ACTION_SELECTION_OPEN_QUESTION_v1.md` (§2.14 sequencer primitive), `LAGADO_PLANE_GOVERNOR_v1.md` (richest-first ladder), memory `lagado-perception-latency-bug`, `lagado-layered-capability-directive`, `lagado-capability-layer`.

---

## 0. The problem this solves (and the one it doesn't)

Real OSWorld libreoffice_calc, whole harness:
- **Monolithic authoring** (one 1400-token gen of the whole op list) → temp-0 **variance** (6 ops one run, 5 the next) + **dropped ops** (incompleteness).
- **Incremental authoring, batch apply** (built 2026-06-22) → fixed variance (01b269ae 3/3) but **regressed completeness** (035f41ba: 1 op) — because an authoring-ONLY loop has no ground-truth for "what's still missing," so it falls back to the model's "done," which fires prematurely (the §2.14 trap).

**Root cause:** completeness requires **observing what's been done vs. what the goal needs** — i.e. applying an op and re-reading the doc. That requires a *live* session.

**This spec does NOT fix** raw formula-semantic correctness (e.g. `'0.00'` vs integer, wrong column mapping). That is model comprehension, a separate residual. The session fixes **completeness + ordering + per-op self-correction + variance**; a still-wrong formula stays wrong.

---

## 1. The central design move — the daemon is a NON-AUTHORITATIVE, replayable cache

The single idea that designs out the entire stateful bug class:

> **The host owns the authoritative, append-only OP LOG. The guest daemon (live soffice + UNO doc) is a disposable CACHE that is always reconstructable as `apply(op_log)` to a fresh load of the ORIGINAL file.**

Consequences (each kills a bug mode by construction):
- **Crash mid-session** → relaunch daemon, **replay the host's op log** → state rebuilt. No lost work.
- **Memory-vs-disk divergence** → there is no "truth" to diverge: the **op log is the single source of truth**; the live model and the on-disk file are both *derivations* of it.
- **The fallback floor, the final reconcile, the replay-on-restart are THE SAME primitive** — "apply the op log from scratch to a clean load" — which is **exactly today's proven `api_plane::build_guest_apply`.** So the thing we already trust (01b269ae 3/3) is reused verbatim as: (a) the fallback when the session wedges, (b) the final GUI reconcile, (c) the rebuild on daemon restart.

So the daemon adds **exactly one capability: cheap per-op reads of a live model.** Nothing it holds is unrecoverable; if it never existed, we'd still apply the log at the end (today's behavior). It is a pure **observation optimization** over a host-owned log. That is why it is safe.

---

## 2. Placement in the plane ladder (additive, never destructive)

Richest-first, per the plane-governor:

```
NATIVE SESSION (live UNO/CDP, per-op observe)   ← NEW: primary rung, this spec
   │  on ANY wedge/health-fail/op-error → fall to ↓
STATELESS ONE-SHOT (build_guest_apply: apply log → reconcile)  ← PROVEN, kept as FLOOR + final reconcile
   │  no API-addressable doc → ↓
GUI plane (a11y → CV → pixel) → CLI
```

The stateless one-shot is **demoted to the floor, not deleted.** Worst case for the whole session plane = "lose per-op observation this run, fall back to the path that already works 3/3." You cannot do worse than today.

---

## 3. Architecture

```
HOST (Rust agent)                         GUEST (OSWorld container)
─────────────────                         ─────────────────────────
op_log  : Vec<OpJson>   (AUTHORITATIVE)    uno_daemon.py  (resident; OUR process)
                                             owns: headless soffice  (--accept="socket,...;urp;")
drive loop:                                        + open UNO doc Component
  author 1 op  ───/execute  uno_client ──▶   apply(op)   → mutate live doc, append to local cache log
  read range   ───/execute  uno_client ──▶   read(range) → live cell values   (the effect-sensor)
  re-prompt with observed state                structure() → sheets/headers/extents (the observe)
  …repeat…                                     health()   → {alive, soffice_alive, doc_open, file}
  reconcile    ───/execute  uno_client ──▶   reconcile() → storeToURL(xlsx) → kill headless (release
                                                            lock) → launch GUI soffice --calc (m1)
```

- **Transport:** reuse the OSWorld guest HTTP `/execute` (no new transport, no modifying the OSWorld server). Each agent step runs a tiny `uno_client.py <verb> <json>` via `/execute`; the client opens a localhost socket to the daemon, sends one JSON request line, reads one JSON response line, prints it to stdout → the agent parses the `/execute` stdout.
- **Daemon ⇄ soffice:** `soffice --headless --norestore --accept="socket,host=localhost,port=2002;urp;"`, connected via the pyuno bridge; the daemon keeps the loaded `Component` open.
- **Op vocabulary = the existing one** (`api_plane::ApiOp` → op-JSON: `set_cell / fill / set_formula_range / add_sheet / rename_sheet`). The daemon's `apply()` runs the SAME UNO logic now inside `build_guest_apply`'s apply loop (incl. `resolve_sheet`, structural-first ordering, `fillAuto`). Factor that apply body so the daemon and the one-shot share it — one implementation, two callers.

---

## 4. Protocol (line-delimited JSON over a localhost socket)

| verb | request | response | notes |
|---|---|---|---|
| `open` | `{file}` | `{ok, structure}` | idempotent; starts soffice if down, loads doc. If already open for a DIFFERENT file → close+reopen (identity guard). |
| `apply` | `{op}` | `{ok, error?}` | one op to the live doc; daemon appends to its cache log. NOT authoritative — host holds the master log. |
| `read` | `{sheet, range}` | `{ok, cells}` | live values — the effect-sensor. |
| `structure` | `{}` | `{ok, sheets, headers, extents}` | the per-step observe. |
| `health` | `{}` | `{ok, soffice_alive, doc_open, file}` | checked before each op. |
| `reconcile` | `{}` | `{ok}` | storeToURL(xlsx filter) → kill headless (release lock) → launch GUI `soffice --calc <file>` (m1 reload for the evaluator). |
| `close` | `{}` | `{ok}` | clean teardown: kill soffice, remove `.~lock`, daemon exits. |

Error semantics: any non-`ok`, any transport failure, or `health.soffice_alive=false` → the host treats the session as **wedged** and takes the recovery path (§5). The host never trusts a silent success.

---

## 5. The stateful bug class — designed out, mode by mode

| Failure mode (impossible in a one-shot) | Defense |
|---|---|
| **Crash mid-session** (UNO bridge/soffice dies) | `health()` before every op; on dead → relaunch daemon, `open(original)`, **replay host op_log**, continue. Work never lost (log is host-owned). |
| **Stale lock file** (`.~lock.<file>#` after unclean death) | daemon `open`/reconcile/close always lock-cleans first (reuse the proven `kill_soffice` + rm-lock from `build_guest_apply`). Kill-stale pre-flight on daemon start. |
| **Mem-vs-disk divergence** | no divergence possible: **op_log is the only truth**; live model + disk are derivations. Any reader of disk gets the truth after `reconcile`. |
| **Process / RAM leak** (4 GB guest → OOM) | **one daemon per (task,file)**; teardown on goal-end + VM-reset (mirror `VmHandle` `KillOnDrop` / kill-stale). Daemon refuses to spawn a second soffice. |
| **Identity / concurrency** (wrong file, retry collision) | daemon keyed to `file`; a request for a different file → close+reopen; one in-flight request at a time (line protocol is serial). |
| **ANY of the above, unforeseen** | universal backstop: wedge → **fall to the stateless one-shot** = `build_guest_apply(op_log)` + reconcile. The proven path is the floor under every mode. |

**Net:** because the daemon is a replayable cache (§1) with a proven fallback (§2), the worst realized outcome of any bug above is degradation to today's behavior — not a new failure surface for the agent.

---

## 6. The drive loop (sequencer-routed, observation-grounded)

```
op_log = []                                   # host-authoritative
open(file)                                     # via daemon; fallback: defer to one-shot at end
for step in 0..BUDGET:
    state = structure() + read(target_ranges)  # OBSERVE the live doc (the missing signal)
    if observed_complete(goal, state): break    # completion judged against OBSERVED state, not authored list
    op = author_one_op(goal, state, op_log)      # ONE short grammar-constrained gen (variance win)
    if op is None or op == op_log.last: break    # no-progress stop (NOT model-"done" as primary)
    if not health(): restart_and_replay(op_log)  # crash defense
    r = apply(op)
    if not r.ok: op_log.pop(); continue/escalate  # bad op: don't commit it to the log
    op_log.push(op)
reconcile()                                     # = stateless apply(op_log) + GUI reload (proven)
# on ANY wedge above → build_guest_apply(op_log) + reconcile  (the floor)
```

**Termination, honestly (the genuine open problem):**
- PRIMARY stops = **no-progress** (no new op) + **budget**. Model-"done" is a **weak hint only** (§2.14 disproved trusting it).
- `observed_complete()` is now far stronger than authoring-blind because it reads the *live doc* — but a *fully deterministic* spreadsheet completion check remains **open** and task-class-specific (e.g. "no blank in B1:E30", "column I non-empty rows 2..N", "sheet 'Sheet2' exists with col A populated"). Build these as **deterministic predicates derived from the goal** where expressible; otherwise bound by budget + no-progress and hand back honestly. This predicate library is the real remaining research, and the loop *bounds* it rather than *solving* it.

---

## 7. What's general vs calc-specific (the cross-app payoff)

GENERAL (the **native-session plane** interface — reuse for every app):
`open / apply / read / structure / health / reconcile / close` + host-owned op_log + replay + fallback + teardown.

ADAPTER (per app, fills the interface):
- **LibreOffice (calc/writer/impress):** soffice UNO socket; ops = `ApiOp`; reconcile = xlsx/odt store + GUI reload.
- **Chrome (future):** CDP session; ops = DOM/JS actions; read = DOM query; reconcile = n/a (live tab is the artifact).
- **OS/Files (future):** the CLI plane already IS a stateless session of this shape (`react_capability_loop` = observe→one cmd→verify); it needs no daemon (the filesystem is the live model).

So this plane is the **third instance** of the one reflex loop (CLI and GUI sequencer are the other two), with a live session where the app has a real automation API.

---

## 8. Reuse vs new (additive accounting)

REUSE (unchanged, now also the fallback/reconcile/replay primitive):
- `api_plane::build_guest_apply` — apply op_log from scratch + reconcile. **Keep verbatim.**
- `api_plane::ApiOp` / `from_call` / `op_to_json` / `resolve_sheet` / structural-first ordering / `fillAuto`.
- VM teardown discipline (`KillOnDrop`, kill-stale) — daemon inherits it.
- The incremental authoring loop already built in `agent.rs` (the authoring half).

NEW:
- `uno_daemon.py` (guest resident: socket server + UNO session + apply/read/structure/reconcile/health/close). **Factor its `apply()` to share `build_guest_apply`'s apply body** — one apply implementation.
- `uno_client.py` (tiny guest RPC client, invoked via `/execute`).
- Host-side `NativeSession` driver in the API plane: open → drive loop (§6) → reconcile, with health/replay/fallback.
- `observed_complete()` predicate hooks (per-goal deterministic checks where expressible).

---

## 9. Build phases (each independently testable; 01b269ae 3/3 is the standing regression gate)

- **P1 — daemon + client in isolation.** Start daemon on a known file, drive a hand-written op_log via `apply`/`read`, assert reads reflect applies; `reconcile` produces the same file the one-shot does. Gate: a hand-driven session reproduces 01b269ae's filled file. (No model in the loop yet.)
- **P2 — wire the API plane to the session driver** (author→apply→read→re-prompt→reconcile), with **fallback to `build_guest_apply` on any wedge.** Gate: 01b269ae 3/3 (via session OR fallback); 035f41ba authors the COMPLETE op set (Sheet2 included) — score may stay 0 on formula correctness, but completeness must be restored (the regression we're fixing).
- **P3 — bug-class hardening:** health-check + restart+replay (kill soffice mid-run, assert recovery); teardown on goal-end + VM-reset (assert no leaked soffice); identity guard. Gate: inject each failure, confirm graceful fallback, confirm no leaked processes.
- **P4 — `observed_complete()` predicates** for the calc task-classes; widen the calc sweep; measure completeness-bound pass-rate lift.

---

## 10. Open questions / risks (named, not hidden)

1. **Deterministic completion check** — the genuine residual (§6). The session makes it *possible* (live observation) but doesn't *solve* it. Predicate library is per-task-class work.
2. **4 GB guest memory** — one persistent soffice is fine; the discipline is no leaks (P3). Watch headless+GUI overlap at reconcile (kill headless before GUI, as today).
3. **Formula-semantic correctness** — explicitly out of scope; needs better authoring (reason→emit / prompt) not the session.
4. **Cross-app generalization** — designed for (§7) but only the calc adapter is built; CDP/others are future and may stress the interface.
5. **Latency** — N host↔guest round trips per task; each fast, but pathological long tasks could add up. Mitigate with batched reads where the loop allows.

---

## 11. One-line summary

Hold the app's native automation interface open as a **live session that is a disposable, replayable cache over a host-owned op log** — gaining per-op observation (completeness + self-correction) — with the **proven stateless one-shot kept as the floor, the reconcile, and the replay primitive**, so the entire stateful bug class degrades to "today's behavior" instead of becoming a new failure surface. This is the AI-friendly app-adaptation layer, built additively.
