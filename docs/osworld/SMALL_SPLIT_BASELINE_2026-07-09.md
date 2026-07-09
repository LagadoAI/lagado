# OSWorld official small split — whole-harness baseline (2026-07-09)

**2/39 (5.1%)** — official test_small.json, all 10 domains, WHOLE Rust harness
(osworld_run: router → planner → plane governor → CLI/GUI planes → supervisor),
official env.evaluate, fresh VM per task, 2 parallel lanes, brain = Qwen2.5-Coder-7B.

Golds: chrome/7b6c7e24, multi_apps/716a6079.

## The reading (build-map, not verdict)
- The general harness floor is 5%; our ENGINEERED calc territory is 77% (23/30
  heldout via battery_calc's API-plane pipeline). The gap is INTEGRATION: the
  general runner does NOT route calc tasks through the proven battery pipeline —
  small-split calc tasks (357ef137, 42e0a640, abed40dc) all 0 here despite the
  calc stack existing. Lever #1: dispatch calc/scriptable-app tasks to the
  battle-tested pipeline from the general runner (the sense-market dispatch
  table's api-plane rule, made real).
- Chrome/multi_apps/GIMP/Impress/VLC/Thunderbird/VS Code have no app planes yet —
  expected zeros (op-vocab/plane coverage, the known fracture line, now measured
  on the official general suite).
- Infra held: 39 fresh-VM cycles, 2 lanes, zero infra aborts; Chrome CDP setup
  flake self-heals on retry (attempt 2 always connected).
