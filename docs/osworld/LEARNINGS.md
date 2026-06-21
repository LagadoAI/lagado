# OSWorld — What Each Finding Tells Us To Build (the learning ledger)

Principle (user, 2026-06-20): **NOTHING is a no-attempt.** Every result — pass, fail, "too hard", "the
model can't" — is a signal that tells us what to build or how to adjust. We account for *all* of it.
Companion data: [`RESULTS.md`](RESULTS.md), [`broad_results_2026-06-20.json`](broad_results_2026-06-20.json).

## The unifying architecture the findings point to
Every finding below resolves to ONE spine: **plane-exhaustion + switch, gated by GOAL-level verification.**

```
goal → CLI plane (discover → ground → operate → reground)         ← our home plane (built)
        └─ GOAL-LEVEL verify (did the GOAL happen? not just rc=0 / readback)
             ├─ met → DONE
             ├─ unmet, path remains → keep grounding
             └─ unmet, CLI exhausted → SWITCH to GUI plane
                  └─ GUI plane: a11y → CV → pixel (fallback order)   ← NOT built (the away plane)
                       └─ GOAL-LEVEL verify
                            ├─ met → DONE
                            └─ unmet, GUI exhausted → INFEASIBLE → last action = FAIL
```

The **goal-level verify failure after a plane exhausts** is the single mechanism that drives: the
CLI→GUI switch, the running-app reload, AND infeasibility detection. Build that spine and the findings fall
out of it.

## Findings → requirements (the ledger)

### F1 — Running-app caching (os/13584542 terminal-size, chrome/030eeff7 Do-Not-Track) → CMD_WRONG
The CLI sets the config (gsettings/dconf readback PASSES, rc=0) but the **running app cached the old value**,
so the GOAL fails. **Tells us to build:**
- **R1a — GOAL-LEVEL effect-verify** (not just rc/readback): does the *goal artifact* hold? This is the
  plane-switch trigger. (Our current effect-verify only re-reads the key — insufficient; it passed here.)
- **R1b — config-apply / app-reload:** after a config write to a running app, restart it so the change
  takes effect (`gnome-terminal-server`, chrome, …). Half-built (`_running_app_to_reload` in the adapter).
- **R1c** — if reload still doesn't satisfy the goal → that's the genuine CLI→GUI switch signal (F3).

### F2 — Infeasible tasks (29 = ~8% of OSWorld; gimp 10, os 5, vs_code 5, chrome 3, …) → win = refuse
OSWorld scores an infeasible task **1.0 iff the agent's LAST action is `FAIL`** (correct refusal); a wrong
`FAIL` on a *feasible* task scores 0. **The model CANNOT judge this** — Qwen answered FEASIBLE for both gimp
traps (it doesn't know GIMP can't do CMYK; it's over-optimistic). **Tells us to build:**
- **R2a — EMPIRICAL infeasibility detection, NOT model-judgment:** conclude infeasible only when *both*
  planes EXHAUST (CLI discover-operate finds no path AND GUI actuation finds no path) → emit `FAIL`. The
  plane-exhaustion IS the signal. (This is why F2 needs the same spine as F1/F3.)
- **R2b** — never rely on the model's prior to declare infeasible (false-positives forfeit feasible tasks).

### F3 — GUI_NEEDED (gimp palette/transparency, chrome reopen-tab/browse) → no CLI path
Genuinely need on-screen actuation. **Tells us to build:**
- **R3 — the GUI actuation plane:** a11y (AT-SPI elements) → CV (detected boxes) → pixel (coords) as the
  fallback ladder. This is the one big unbuilt half of the design and the bulk of the ~369 tasks.

### F4 — os home plane = 3/4 → the CLI machinery works
discover-then-operate + effect-verify + CLI-bias + path-grounding carry file/system/config tasks (incl. a
fresh compress-by-mtime task, solved 3-command). **Tells us:** the deterministic discover→ground→operate→
verify pattern is sound — extend it onto the GUI plane (same loop, different actuation), don't replace it.

### F5 — The model is over-optimistic about its own ability (F2 corollary)
It declares tasks feasible/decomposable that it can't actually do. **Tells us:** feasibility and success must
be determined **empirically by the harness** (attempt + goal-verify + plane-exhaustion), never by the model's
self-assessment. Reinforces the whole "harness is the moat, model is swappable" thesis.

### F6 — At the wall, the agent should LOOK UP how, not just fail (user idea, 2026-06-20)
When discover-operate stalls (the terminal-size hallucination), the agent should *consult knowledge* before
declaring exhaustion. This is "give it more tools" — but the right tool, gated by sovereignty. **Tells us to
build a DISCOVERY ESCALATION LADDER (feeds the reground):**
- **R6a — system introspection** (built): `gsettings list-recursively`, `dconf dump`, `ls`, `which`.
- **R6b — LOCAL DOCS (sovereignty-safe, cheap, do this next):** `man`, `tldr`, `<cmd> --help`, `apropos`,
  `info`. The terminal can read its own manuals — this would have grounded the gnome-terminal schema without
  any network. Air-gap-clean; just more discovery probes.
- **R6c — GATED web search (security-profile-controlled):** self-hosted SearXNG or browser-via-computer-use
  (see memory `lagado-react-search-grounding`: DDG instant-answer works, general search is bot-blocked).
  **OFF in Strict/air-gapped (regulated) mode — live egress breaks the sovereignty promise; ON in Open mode.**
The escalation: introspection → local docs → (if profile allows) web. Search ≠ a special mode; it's the
top rung of the SAME discover-then-operate ladder.

## Build order (data-driven)
1. **R1a — goal-level effect-verify** (the spine's trigger; cheap; unlocks F1 + is prerequisite for F2/F3).
2. **R1b — config-apply/app-reload** (finishes the running-app class on the CLI plane).
3. **R3 — GUI actuation plane** (a11y→CV→pixel), gated by the R1a trigger (the big build).
4. **R2a — empirical infeasibility** (falls out once both planes can exhaust → FAIL) — +8% of the bench.

Update this ledger as new runs surface new finding-classes. Every measured failure is a line item here.
