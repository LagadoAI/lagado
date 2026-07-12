# Lagado

**A deterministic harness that lets a small local model reliably operate a real desktop — entirely on your own machine. No cloud, no telemetry, no egress.**

The bet: **a small model inside the right support structure beats a large model with none.** A modest
local model (≤8B) is wrapped in deterministic machinery — structured perception, typed actions, and
read-back verification — that does the reliability the model can't. The harness is engineered so
carefully that the model becomes a commodity: swapping the brain for a different model of the same
size *raised* the completion rate with the harness unchanged. **The harness is the moat, not the model.**

---

## What we're building

A harness that drives a real desktop the way a competent person does — reaching for the most reliable
channel available and only falling back when it must:

- **Perception is fused, always — one world model, not a fallback chain.** Every sense reads the
  screen at once — the accessibility tree, the browser's own DOM, computer vision, and the raw pixel
  feed — and they're fused into a single deduplicated, provenance-labelled model of what's on screen.
  Each sense is blind exactly where another sees (the browser DOM surfaces dozens of real controls on
  pages where the accessibility tree returns nothing), so we don't pick one — we fuse all of them into
  one representation, the way a self-driving stack fuses every sensor into a single world model rather
  than trusting any one.
- **Action reaches for the richest channel available.** Perception is unified; *acting* is where the
  richest-first ladder lives. The model picks a typed operation and the harness drives it through the
  most reliable channel the app exposes — its own scripting API where one exists, a command-line /
  back-door route otherwise, and GUI actuation (aimed by the fused perception above) as the general
  floor. Most reliable first, falling back only when it must.
- **Read-back verification, and honest handbacks.** The harness reads the world back after acting and
  will hand a task back to you rather than claim a success it cannot prove. *"I can't verify this"* is
  a first-class outcome, never a silent success.

## Where it stands (honestly)

Measured on a real 369-task desktop-automation benchmark, scored by the benchmark's **own official
evaluator** — never by the harness grading itself.

- **Spreadsheets are built out end-to-end** and reach a meaningful fraction of that domain's tasks on
  the official grader — the proof that the structured-plane approach works.
- **Other surfaces are in progress.** Word-processor and presentation planes are built but not yet
  validated; browser actuation, media, and mail are next. Every surface without a built-out plane is
  an honest zero, not a mystery — that's the build map, not a verdict.
- **Integrity is measured, not assumed.** The target for *false passes* — claiming a task is done when
  it isn't — is zero, and we hunt them adversarially. A recent internal audit (an independent model
  told to *refute* our own results) overturned some of our conclusions, found false passes we'd
  missed, and exposed a verification path that could claim success without checking anything. All
  now fixed. Hardening the measurement comes before building anything new on top of it.

This is a working research harness, not a finished product. We report the ugly numbers with the good.

## Stack

Rust (agent core) · llama.cpp (vendored, local inference) · model-swappable brain (≤8B; benchmarked
on Qwen2.5-Coder-7B) · QEMU/KVM (sandboxed working surface) · SQLite (encrypted at rest). 362 library
tests; CI on Linux/macOS/Windows.

## Where to read

- **[CLAUDE.md](CLAUDE.md)** — the working map: architecture, current status, and the harness doctrine.
- **[docs/osworld/](docs/osworld/)** — the benchmark investigation, results, and per-run audits.
- **[docs/plans/](docs/plans/)** — the design decisions and open questions, as they were argued.
- **[docs/vision/](docs/vision/)** — the longer product arc (sovereign local agent, persistent memory,
  the app shell). The eventual shipping vehicle; not the current work.

---

*Lagado Labs — sovereign, local, honest about what works and what doesn't.*
