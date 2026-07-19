# CLAUDE.md

Working guide for the Lagado **harness**. Read before making changes.

## What we are working on

The deterministic harness that lets a small local model reliably operate a real desktop —
benchmarked on the REAL OSWorld benchmark, where success = golds with **zero false-pass**
(integrity is scored, not assumed). **The harness is the product; the app/UI is the eventual
shipping vehicle, not the current work.** The harness, not the model, is the defensible asset
(proven: a brain swap with the harness unchanged raised the score).

**Orientation order:**
1. `docs/CURRENT_STATE.md` — verified-against-code current state: what's live by default, what's
   built-but-gated pending A/B, what the fused perception stack actually does today. Wins on any
   conflict with anything else, including this file.
2. `HARNESS_WORK_PLAN.md` — the live queue.
3. This file's doctrine + invariants sections.

**Current position (2026-07-14):** LibreOffice Calc is the one domain with a proven plane
(19/47 official OSWorld gold, integrity-audited, sound falsifiers + corroboration). Full official
split: 24/368 — the failure histogram is a BUILD-MAP (missing planes), not a comprehension
verdict. The fused perception arbiter (a11y+CV+DOM+vision) is real, tested architecture; CV is
default-on; DOM / backdoor / writer-solver / impress-solver are built and gated behind explicit
A/B-pending flags (`config.rs`).

**Current arc (ratified 2026-07-10):** benchmarks come FROM the harness, no leading ever;
ablation-per-capability before anything defaults on. Build = OP vocab finish + DOM floor + API
planes for all apps. Sensorimotor redesign (ratified 2026-07-08): eyes / hands / chronos /
fusion-feed built HOST-SIDE, multi-app — not LibreOffice-shaped. Cortex/subcortex: the LLM fires
rarely, on unsettled questions; settled questions go to deterministic dispatch / CfC reflexes
with their own promotion gate.

## Runtime facts you need daily

- Single Tauri binary; Rust core is `lagado-agent/` (library) + `bin/` research probes.
- Brain = `llama-server` on **:8080** (benchmark brain: **Qwen2.5-Coder-7B** via
  `start_brain.sh`; the LFM2 set in `~/.laputa-secure/models/` is app-shipping intent, not the
  benchmark brain). Classifier **:8081**, embedder **:8082**, all watched by `server_guard`.
- Working surface = QEMU guest VM: **Fedora 44 + Cinnamon** (GTK/AT-SPI2),
  `~/.laputa-secure/vm-images/lagado-guest-fedora.qcow2`, built by
  `vm-provision/build-guest-fedora.sh`, guest user `laputa`, SSH key auth. Cinnamon GUI a11y is
  flaky — lean on terminal for file/doc work, a11y for forms. Control-channel proof:
  `cargo run --bin harness_proof`.
- Plane ladder (richest-first, deterministic pick in `plane.rs`):
  API → backdoor → a11y → CV → pixel → CLI.
- Data dir: `~/.laputa-secure` (`LAGADO_DATA_DIR`).
- Full runtime/module/subsystem reference (Hydra pipeline, auth, memory, vision FFI, UI):
  `docs/plans/CLAUDE_MD_ARCHITECTURE_REFERENCE_2026-07-18.md` — reference material, partly
  app-era; verify against `docs/CURRENT_STATE.md` before trusting.

## Doctrine — how harness work is done

- **Propose/verify at every granularity:** amnesiac proposer, sound verifier, prefix-commit,
  resample at divergence. Fail-closed needs a CONTRACT (structure > dialogue > under-claim).
- **No leading, ever:** benchmarks FROM the harness; official evaluator only; artifact-first;
  frozen/held-out prompts. Never prompt-nudge to pass a task — a global prompt tweak is an
  unattributable knob (A/B-proven to gold one task while silently regressing another). Fix
  emission locally (per-fault detector or GBNF).
- **Integrate before invent:** SEE + INTEGRATE first; invent only after that fails. Don't let
  golds set priority.
- **Ceiling mindset:** a failure is a HOW not yet solved, never an IF-verdict. Honest data is the
  engine, not a brake. `osworld_stress` is an INTERNAL proxy — never conflate with the official
  bench.
- **Deterministic over prompt:** every reliability win so far came from removing an LLM decision,
  not improving one.

## Key invariants — DO NOT BREAK

1. **Mutex guard discipline**: guards MUST be dropped before any `.await`.
2. **Clean-context routing**: `classify_intent()` MUST receive only the current user message.
3. **HITL chokepoint**: all agent actions go through `gate::evaluate_action()`. Never bypass.
4. **No wildcard `_` arms** on enums you define.
5. **No `std::process::exit(1)`** from library code.
6. **No AI attribution** in commits, code, PRs, or any artifact. Author: `Lagado Labs <lagadolabs@gmail.com>`.
7. **DEK discipline**: never persist raw DEK. `active_key()` is the only crypto entry point.
8. **SSH readiness**: never set `vm_ssh_port` before SSH auth probe (`ssh -o BatchMode=yes ... whoami`) returns exit 0 and stdout contains "laputa". Bare TCP connect is insufficient.
9. **No hardcoded model/hardware values** (2026-06-17): never hardcode a model- or hardware-specific value (context window, layer count, n_gpu_layers, ctx size, model size, parallelism, CPU/GPU placement). **DISCOVER** it (GGUF metadata via the model-reader / hardware probe) or **DEFER** it (governor/user setting), always with a *discovered* default. The model is swappable (H-1) — assuming its context/layers/size is a latent bug. The only literals allowed are principled constants unrelated to model/hardware (ports, the 30-day Ebbinghaus curve). See `docs/plans/LAGADO_MODEL_AWARE_GOVERNOR_SPEC_v1.md`.
10. **No Board/retrieved memory in the action path** (2026-06-17): the action-selection prompt (`agent_loop`'s executor) sees ONLY the pinned SYS framing + the candidate list + the goal/sub-goal. NEVER inject episodic/visual/skill/Board memory into it — verified that semantically-related prepended memory *overrides* the candidate labels and flips the pick (decoy-priming → 12/12 wrong; see `docs/plans/LAGADO_ACTION_SELECTION_OPEN_QUESTION_v1.md` §2.5). The Board is chat-RAG + skill-advisory ONLY. Deterministic harness *trajectory* state (Q1 action-outcome fact, step pointer) is a DIFFERENT, safe category (not retrieved prose) and is allowed. Corollary: the executor prompt's **SYS preamble length is load-bearing** (it lands the candidate list in the model's late-attention band) — do not trim it without re-running the position sweep (`docs/plans/experiments/lean_gate.py`).

## Build / run

```bash
# From the repo root — sets all env (data dir, llama-server path, VRAM-saving webview
# flags) with repo-relative paths, then runs `npm run tauri dev`:
./launch.sh

# Or manually from lagado-ui/ ($REPO = repo root):
WEBKIT_DISABLE_DMABUF_RENDERER=1 \
LAGADO_DATA_DIR=$HOME/.laputa-secure \
LAGADO_LLAMA_SERVER=$REPO/lagado-agent/vendored/llama.cpp-2/build/bin/llama-server \
LD_LIBRARY_PATH=$REPO/lagado-agent/vendored/llama.cpp-2/build/bin \
npm run tauri dev

# Checks
cargo check --workspace && cargo test -p lagado-agent
cd lagado-ui && npx tsc --noEmit
```

`cargo test -p lagado-agent` needs `LD_LIBRARY_PATH=$REPO/lagado-agent/vendored/llama.cpp-2/build/bin`
(stale rpath in vendored libllama.so). GPU temp via `nvidia-smi`; ceiling < 85°C during runs.

## Model policy

**(User directive 2026-07-10 — supersedes the 2026-06-16 no-delegation rule.)** The top model
(Fable 5) does all MAIN-LOOP work — planning and load-bearing implementation. Subagents ARE used:
**Sonnet by default, Opus for complex tasks, Fable only for load-bearing work.** Use the `advisor`
tool for the adversarial-review/skeptic pass before load-bearing designs and when declaring done.
Verify with `cargo check --workspace` + `npx tsc --noEmit` after changes.

## Repo policy

**Docs policy (2026-06-16 reversal):** `docs/`, `LAPUTA HOW TO/`, and all plans/PDFs are
COMMITTED (was: local-only). Machine = single point of failure; disaster-recovery beats secrecy.

**Public/private status: OPEN, not settled (as of 2026-07-14).** The old "repo is private
forever" note was a decision made under different constraints (product-differentiation bet) than
the ones active now (money/visibility bet, open-source under active consideration). Do not treat
"never public" as a standing invariant; whoever resolves it should update this line explicitly.

## History

The session-by-session build history (2026-06-11 → 2026-07-11 — every arc, every measured fix,
every superseded number) is preserved verbatim in
`docs/plans/CLAUDE_MD_STATUS_ARCHIVE_THROUGH_2026-07-11.md`. It is the record of *why* things are
shaped the way they are — it is just not where you look to answer "what is true right now"
(that's `docs/CURRENT_STATE.md`).
