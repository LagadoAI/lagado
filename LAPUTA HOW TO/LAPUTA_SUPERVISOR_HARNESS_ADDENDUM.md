# LAPUTA — SUPERVISOR HARNESS ADDENDUM
## The Two-Loop Architecture: From Brute-Force to Reapproach

**Append to:** LAPUTA_v1_UNIFIED_MASTER_PLAN_v4.md (as Part XVIII) and LAPUTA_FILE_DEPENDENCY_REFERENCE_v3.md (new module).
**Date:** May 30, 2026
**Origin:** Diagnosis that forge.rs is a brute-force retry loop, not a problem-solver. Inspired by Origin Pilot's supervisory control discipline (monitor → diagnose → recalibrate → bounded retry → escalate).

---

## THE DIAGNOSIS

Forge is `generate → parse → verify → retry`. On failure it retries the same tactic with the error appended. It has no layer above the attempt, so it cannot ask "am I approaching this right?" — only "try again." This is the brute-force ceiling, and it is why LFM2's multi-step completion looks weak *in this harness*: the failure is the transmission, not the engine.

A human who fails twice does not try a third time identically. They step back, diagnose *why*, change *strategy* (not just tactic), and reapproach — or reassess whether the whole plan was wrong. Sometimes the problem isn't *what* you do, it's *how*. Forge has no "how" layer.

---

## THE FIX: TWO LOOPS, NOT ONE

### Inner loop (tactical) — forge.rs, UNCHANGED
Attempt a single step: generate → parse → verify. Stays on the don't-touch list. It is a fine executor; it is just the wrong thing to be the *whole* harness.

### Outer loop (strategic) — supervisor.rs, NEW
Sits ABOVE forge. Treats forge as its tactical executor. Owns the *how*. Four stages:

1. **Monitor** — is the current step succeeding, stalling, or looping? (Same instinct as lifecycle.rs heartbeat, applied to task execution.)
2. **Diagnose** (on failure) — *why* did it fail? Wrong tool / wrong approach / missing info / bad assumption? This is the question forge never asks. (recovery.rs's 7 failure types is the seed — already a classifier, just not yet wired to strategy.)
3. **Reform** — given the diagnosis, change the *strategy*: different tool, gather missing info first, decompose further, or ask the user. NOT a blind retry.
4. **Reassess** — is the original plan still valid? A step-3 failure can mean the step-1 plan was wrong. The outer loop can revise the plan, not just the step. (Re-invokes hydra's plan phase.)

The distinction in one line: **forge retries; the supervisor reapproaches.**

---

## WIRING (the pieces already exist)

The supervisor is mostly a *connector* of components Laputa already has:

| Stage | Existing component | What's added |
|-------|-------------------|--------------|
| Monitor | lifecycle.rs heartbeat instinct | stall/loop detection on task execution |
| Diagnose | recovery.rs (7 failure types) | feed classification to a strategy selector |
| Reform | operator.rs (tool registry) + hydra RAG K=15 | pick a *different* strategy, not retry |
| Reassess | hydra.rs plan phase | re-invoke planning when a step invalidates the plan |
| Meta-signal | chronos + self_model | "I've tried this 4 times" → escalate or change tack |
| Shortcut | action_graph + skill_library | known-good strategies skip re-derivation |

supervisor.rs is the control structure that turns these from isolated parts into a strategic loop.

---

## THE ORIGIN PILOT DISCIPLINE (bounded supervision)

What makes Origin Pilot trustworthy with fragile state is NOT model intelligence — it is a supervisory control system with monitoring, bounded retries, recalibration, and escalation. The AI optimizes *within* hard rules it cannot break. The supervisor adopts the same bounded discipline:

- **Bounded reapproaches** — max N strategy changes, then escalate. Never loop forever (the doom-loop trap).
- **Monitoring gates** — detect stall/loop *before* burning more attempts.
- **Escalation path** — when strategies are genuinely exhausted, STOP and ask the user. This is HITL used in reverse: not "approve my action" but "I'm stuck — here's what I tried, what would you suggest?" Same epistemic honesty as the dormancy boundary: the agent admits the limit rather than degrading.

These bounds are NON-NEGOTIABLE — a strategic loop without bounds is just a more sophisticated way to hang.

---

## supervisor.rs — DEPENDENCY ENTRY

**supervisor.rs** ☐ NEW (Phase 3.5 — after hydra, before host projector)
- **What it is:** The strategic outer loop. Monitors execution, diagnoses failures, reforms strategy, reassesses the plan. Wraps forge (tactical inner loop).
- **Theory:** Forge is brute-force retry; real problem-solving needs a meta-layer that changes *how*, not just repeats *what*. Mirrors Origin Pilot's supervisory control: monitor → diagnose → recalibrate → bounded retry → escalate. The AI optimizes within hard rules it cannot break.
- **Build:** Outer loop owning monitor/diagnose/reform/reassess. Calls forge as executor. Calls recovery.rs for diagnosis, hydra.rs for re-planning, chronos/self_model for meta-signals, action_graph/skill_library for known strategies. Bounded reapproaches + escalation-to-user on exhaustion.
- **Depends on:** forge.rs (executor), recovery.rs (diagnosis), hydra.rs (re-plan), operator.rs (strategy options), chronos.rs + self_model.rs (meta-signals), action_graph.rs + skill_library.rs (shortcuts). **Depended on by:** main.rs (agent loop runs through supervisor, not forge directly).

**Update to main.rs entry:** agent loop calls supervisor.rs, which internally calls forge.rs. main.rs no longer calls forge directly once Phase 3.5 lands.

**Update to LOAD-BEARING CONNECTIONS:**
13. **supervisor.rs → forge.rs** — strategic loop wraps tactical loop. The supervisor reapproaches; forge executes. This is the line between brute-force and problem-solving.

---

## SCOPE DISCIPLINE

- forge.rs stays UNCHANGED and on the don't-touch list. The supervisor wraps it; it does not replace or modify it.
- Phase 1 proves the inner loop on one model. Do NOT build the supervisor in Phase 1.
- Phase 3 builds hydra (the planning skeleton — this is the outer loop's spine).
- Phase 3.5 builds supervisor.rs (adds monitor/diagnose/reform/reassess muscle to hydra's skeleton).
- This is where Laputa stops being brute-force and starts reapproaching.

The hooks to architect NOW (Phase 1, cheap): keep recovery.rs's failure classification accessible (don't bury it); keep the agent loop in main.rs structured so a supervisor can wrap it later without a rewrite.

---

**— End of Supervisor Harness Addendum. Forge retries; the supervisor reapproaches.**
