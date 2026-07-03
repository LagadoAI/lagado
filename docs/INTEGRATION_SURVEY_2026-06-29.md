# Integrate-before-invent survey: what others built that we can build off of

**Date:** 2026-06-29
**Why:** per the operating agreement ([[lagado-integrate-before-invent]]) — SEE what exists and INTEGRATE
first; invent ONLY for what nothing external covers. This is external prior art, not our own code.
**Method:** deep-research fan-out (6 angles, 22 primary sources, 103 claims extracted → 25 adversarially
verified, 25/25 confirmed, 0 refuted). Sources cited per row; verify them yourself, don't take my word.

**Headline:** **3 of the 4 layers have permissive, offline, edge-ready building blocks we can ADOPT/ADAPT.
The 4th — the fusion dispatcher + selection loop — is genuine white space.** That is exactly where the
project already located its moat, so the agreement is satisfied: integrate the three, reserve invention
for the fourth (which is the defensible novel thing anyway).

---

## Layer 1 — Deterministic execution harness

| Tool | Built by | License | Offline/edge | Verdict |
|---|---|---|---|---|
| **llama.cpp GBNF** | ggml-org | MIT | ✓ CPU | **ADOPT** — already our constraint layer; nothing to change |
| **XGrammar** | mlc-ai | Apache-2.0 | ✓ CPU, C++ core | **ADOPT/eval** — faster CFG-constrained decoding; default backend for vLLM/SGLang; integratable lightweight C++ core |
| **Bytebot** | bytebot-ai | Apache-2.0 | ✓ runs on local Ollama/LiteLLM | **ADOPT/eval** — self-hosted desktop REST actuator; an off-the-shelf actuation surface |
| **Agent S (S1/S2/S3)** | Simular | Apache-2.0 | ✗ cloud-default | **ADAPT** — GUI automation + UI-TARS grounding + memory, but needs a local-model swap + local grounding endpoint |

## Layer 2 — Probabilistic / belief-state control

| Tool | Built by | License | Offline/edge | Verdict |
|---|---|---|---|---|
| **pymdp** | infer-actively | MIT | ✓ pure-Python CPU | **ADOPT/ADAPT** — active inference / free-energy; robotics-oriented |
| **pomdp_py** | H2R (Brown) | MIT | ✓ CPU | **ADOPT/ADAPT** — POMDP belief-state planning, ships POMCP/POUCT solvers |
| **POMDPPlanners** | (open) | MIT | ✓ | **ADAPT** — reusable belief-state planners |
| **RxInfer.jl** | ReactiveBayes | MIT | ✓ but **Julia runtime** | **ADAPT** — variational message passing / active inference; adds a Julia dependency |

⚠️ All four are **robotics-oriented** — mapping belief-state onto a GUI/desktop is non-trivial; this is
real adaptation work, not plug-in.

## Layer 3 — Confidence / honesty / abstention (the "tested not trusted" layer)

| Tool | Built by | License | Offline/edge | Verdict |
|---|---|---|---|---|
| **UQLM** | CVS Health | Apache-2.0 | ✓ works with local Ollama/Llama3/Mistral | **ADOPT** — UQ-based hallucination detection, 0–1 scores; **white-box scorers fit llama.cpp logprobs** |
| **lm-polygraph** | IINemo | (verify) | ✓ | **EVAL** — second UQ library; confirm license before adopting |

**This is the standout practical find:** the integrity/abstention layer we needed *this morning* (calibrated
confidence so the harness can't quietly over-claim) has an off-the-shelf, Apache-2.0, offline, logprob-based
library. **Adopt, don't invent.**

## Layer 4 — The FUSION (dispatcher + selection loop + liquid-net tool-calling)

| Need | External option | Verdict |
|---|---|---|
| Deterministic↔probabilistic **dispatcher** ("foreman") | none surviving | **WHITE SPACE → invent** |
| Reusable permissive **RLVR / eval-gated loop** over agent trajectories | NVIDIA-NeMo/Gym (eval) — not a drop-in | **WHITE SPACE → invent** (evaluate NeMo/Gym first) |
| **Liquid-net (CfC/ncps) tool-calling** | ncps, CfC exist for *continuous control*, not tool-calling | **CAN'T-USE as-is → invent if pursued** |

---

## Honest caveats (carry these)

- **White space = absence-of-evidence in a *bounded* survey, NOT proof none exists** (confidence: medium).
  Don't claim "nobody has done it." Do a targeted second pass before committing to invent.
- Bytebot / Agent S are **cloud-default** — local-model swap required.
- UQLM white-box scorers **need logprob access** — we have it via llama.cpp.
- POMDP/active-inference libs are **robotics-tuned** — GUI belief-state mapping is the hard adaptation.
- RxInfer adds a **Julia runtime**; weigh against pymdp/pomdp_py (pure Python).

## Open questions the survey couldn't close (the targeted-second-pass list)
1. Any permissive offline router between deterministic rules and probabilistic control?
2. Any reusable permissive RLVR/eval-gated loop over agent trajectories (is NeMo/Gym usable offline)?
3. Permissive CfC/liquid-net specifically for tool-calling (vs continuous control)?
4. Which UQLM white-box scorers actually work with llama.cpp logprobs, and at what CPU cost?

**Sources:** llama.cpp, XGrammar (mlc.ai), Bytebot, Agent-S, pymdp (+JOSS), pomdp_py, RxInfer.jl, UQLM,
lm-polygraph, ncps, CfC, NVIDIA-NeMo/Gym, plus arXiv 2404.10960 / 2405.13845 / 2410.11689 / 2505.24760 /
2512.22245 / 2602.20810. Full list + per-angle claim counts in the run output.
