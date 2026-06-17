# Invention Disclosure — Lagado AI

**Confidential.** Prepared for patent counsel. Not legal advice; claim language below is an
engineering-level draft for an attorney to formalize. Date: 2026-06-17. Assignee: Lagado Labs.

> **Counsel note:** the strongest filing is likely a **provisional** to lock the priority date,
> followed by a full utility application within 12 months. The "reduction to practice" in §7 is
> real (working code + measured experiments, committed to a private repository with dated history).
> Recommend filing the architecture as a patent and retaining the trained models, training data,
> and specific tuned constants as **trade secrets** (see §9).

---

## 1. Title

**Method and system for reliable autonomous user-interface operation by a small, fully-local
language model using deterministic control rails.**

## 2. Field

Autonomous software agents that operate a graphical user interface (clicking, typing, reading the
screen) to accomplish user goals — specifically, agents constrained to run **entirely on local
consumer hardware, offline, with no cloud inference**, using a deliberately small language model.

## 3. Background and problem

State-of-the-art GUI agents depend on large, cloud-hosted language models. A privacy- and
sovereignty-first product requires the opposite: a **small** model running locally. Small models
exhibit specific, measurable failure modes that make naive GUI control unreliable:

- **(P1) Context-content domination.** The model's choice is dominated by the *semantic content of
  the text preceding the decision*, not by the actual on-screen options. Retrieved "memory" or
  context that is merely *related* to a wrong option can override the correct choice.
- **(P2) Position bias / attention non-uniformity.** The model attends unevenly to a list of
  options; which option it favors depends on *where in the prompt* the option sits, not on its
  match to the goal.
- **(P3) Non-abstention.** The model will not reliably signal "none of these options fit"; offered
  an explicit escape token, it nonetheless emits a (wrong) action rather than declining.
- **(P4) Action amnesia (single-step blindness).** Operating each step from a fresh context (needed
  to avoid multi-turn degradation), the model has no memory of its own prior action and will
  re-derive an action it already performed (e.g., re-toggling an opened menu).
- **(P5) Planning incapacity.** The model cannot reliably decompose a multi-step goal or judge
  whole-goal completion; it emits a premature "complete" signal even when handed the remaining
  steps explicitly.

Each of P1–P5 was independently measured in the inventors' work (§7). The problem solved by this
invention is: **how to obtain reliable, autonomous, multi-step GUI operation from a small local
model exhibiting P1–P5.**

## 4. Summary of the invention

The core inventive concept is a **control architecture that confines the language model to a single
narrow task it performs reliably — selecting one target from the currently-visible options — and
relocates every other function (planning, ordering, completion-detection, abstention, memory
handling, and recovery) to deterministic (non-model) components**. Compactly: *determinism on the
rails, the model on the local choice.*

The architecture comprises the following cooperating elements, each addressing a measured failure:

- **(A) Memory-isolated selection prompt** (addresses P1): the prompt for the selection step is
  constructed to **exclude** retrieved/episodic/semantic memory, containing only fixed system
  framing, the current perceived candidates, and the goal/sub-goal. This both fixes selection
  reliability and constitutes a structural defense against prompt-injection via perceived/retrieved
  text.
- **(B) Attention-position-aware ("late-band") candidate ordering** (addresses P2): a deterministic
  ranker orders the candidate list so the candidate(s) most relevant to the goal are placed at the
  position of the model's empirically-strongest attention. The deterministic layer controls
  *ordering only*; the model still makes the selection.
- **(C) Length-pinned preamble with regression guard** (addresses P2): the fixed system preamble's
  length is held constant because it positions the candidate list within the attended region; a
  stored test ("position sweep") guards against silent drift if the preamble is later edited.
- **(D) Deterministic fail-closed selection gate** (addresses P3): because the model does not
  self-abstain, a deterministic check evaluates whether any candidate matches the goal; on no match
  the system **re-perceives/escalates** rather than executing a wrong action, biased toward
  re-perception (a false re-perception is recoverable; a false action is not).
- **(E) Grammar-constrained selection over a synthetic per-frame index** (addresses P3, security):
  the model's output is formally constrained (e.g., GBNF) to one index from a per-frame index space
  that names **every** fused candidate — including candidates with no text label. An off-list or
  off-screen target is therefore unrepresentable in the output. An actuator resolves the chosen
  index to a screen coordinate.
- **(F) Cross-sense fusion into the index space**: multiple perception sources (accessibility tree,
  classical computer-vision proposals, visual-encoder regions) are fused (e.g., by spatial overlap
  deduplication) into one indexed candidate set, so that visually-distinct but label-less elements
  remain selectable.
- **(G) Deterministic action-effect detection** (addresses P4): the system computes a structural
  diff of the perceived screen state before and after an action. If the model re-derives an action
  that *already produced its effect*, the harness halts the re-derivation (recognizing
  accomplishment). The complementary case — the same action with *no* effect — signals an impasse.
- **(H) Deterministic sequencer over single-step model selection** (addresses P5): a compound goal
  is decomposed **deterministically** into ordered sub-goals; the model performs only single-step
  selection per sub-goal; the progress pointer is deterministic harness state. Each sub-goal carries
  a deterministic **expected-effect signature** keyed to an action class, used as both a
  **precondition** (if the sub-goal's end-state already holds, advance without acting) and a
  **postcondition** (advance only when the action-class-specific structural effect is detected — not
  on a bare screen change). If the screen matches neither the precondition nor the post-effect of
  the current sub-goal (the world has diverged from the plan), the system **escalates to the human**
  rather than executing a stale plan.

A key sub-concept spanning G and H: **deterministic harness "trajectory state"** (the action-effect
fact and the step pointer) is supplied to the system as ground-truth state-change information,
distinct from retrieved semantic memory (which is excluded per A). This distinction — *deterministic
run-state is admissible where semantic memory is barred* — is itself inventive in this context.

## 5. Detailed description (preferred embodiment)

A single binary runs locally: a small quantized language model served locally, a perception layer,
an actuator, and the deterministic control components. Per step:

1. **Perceive.** Read the current screen via one or more senses; **fuse** (F) into a candidate set,
   each candidate assigned a stable per-frame index, a bounding box, and (where available) a label.
2. **Sequence (H).** Determine the current sub-goal from the deterministic plan and pointer. Evaluate
   the sub-goal's **precondition** signature against the candidate set; if already satisfied, advance
   the pointer and repeat. If the screen matches neither precondition nor expected post-effect,
   **escalate**.
3. **Gate (D).** Deterministically test whether any candidate matches the current sub-goal; if none,
   re-perceive/escalate.
4. **Rank (B).** Order the candidates so the most goal-relevant is in the attended (late) band.
5. **Select.** Construct the **memory-isolated** (A), **length-pinned** (C) prompt containing system
   framing + ranked candidates + sub-goal; obtain a **grammar-constrained** (E) index choice from the
   model; resolve index → coordinate; actuate.
6. **Detect effect (G).** Diff pre/post screen state. Classify outcome; if the model attempts to
   re-derive an already-accomplished action, halt. Advance the sequencer pointer on the expected
   action-class effect (H).
7. **Repeat** until the deterministic plan is exhausted (→ done) or divergence/impasse → escalate.

The model is **never** asked to plan, decompose, judge whole-goal completion, abstain, order
options, or consume retrieved memory — each of which it was measured to do unreliably.

## 6. Independent and dependent claims (engineering draft — for counsel)

**Independent claim 1 (system).** A system for autonomous operation of a graphical user interface,
comprising a local language model and a deterministic controller, wherein the controller: (a)
constructs a selection prompt that contains a set of perceived interface candidates and a goal and
that **excludes retrieved semantic memory**; (b) **deterministically orders** the candidates within
the prompt according to relevance to the goal and a model-attention-position profile; (c) constrains
the model's output to a single identifier from a per-frame index that names each candidate; (d)
**deterministically determines abstention** when no candidate matches the goal; and (e) actuates the
interface at a location resolved from the model-selected identifier.

**Independent claim 2 (method — multi-step).** A method comprising: deterministically decomposing a
goal into ordered sub-goals; for each sub-goal, (i) deterministically testing a precondition
signature and advancing without acting if already satisfied; (ii) obtaining a single-step target
selection from a local language model over a memory-isolated, relevance-ordered candidate set; (iii)
deterministically detecting whether an action-class-specific expected effect occurred and advancing
on detection; and (iv) escalating to a human when the interface state matches neither the
precondition nor the expected effect.

**Independent claim 3 (method — action-effect / anti-re-derivation).** A method comprising
computing a structural difference of interface state before and after an action and, upon the model
re-deriving an action whose prior instance already produced its structural effect, suppressing
re-execution and treating the action as accomplished.

**Representative dependent claims.** ...wherein the ordering places the most-relevant candidate at
the end of the candidate list (late band). ...wherein a stored regression test verifies the ordering
behavior against changes in preamble length. ...wherein the candidate set is a fusion of an
accessibility source, a computer-vision source, and a visual-encoder source, and the index names
candidates lacking a text label. ...wherein abstention biases toward re-perception over action.
...wherein deterministic run-state (a prior-action-effect record and a progress pointer) is supplied
to the controller while retrieved semantic memory is excluded. ...wherein the model output is
constrained by a formal grammar. ...wherein the local model runs fully offline with no network
inference. ...wherein the expected-effect signature for a "menu-open" action class is the appearance
of a spatial cluster of new candidates near the actuation point.

## 7. Reduction to practice (measured; private dated repository)

Working implementation exists; the following were measured (N=12 trials/condition unless noted,
live local model, deterministic decoding):

- **P1 measured (→A):** injecting goal-related memory that named a competing option flipped the
  model's selection to the wrong option 12/12; isolating memory restored correct selection.
- **P2 measured (→B,C):** the same option was selected correctly 12/12 when placed in the late band
  and 0/12 when placed first; stripping the preamble inverted the effect — establishing ordering and
  preamble length as control variables.
- **P3 measured (→D,E):** offered an explicit "none" escape token on a no-match screen, the model
  emitted it 0/12, forcing a wrong action — establishing that abstention must be deterministic.
- **P4 measured (→G):** the agent re-clicked an opened menu repeatedly until stopped; the
  action-effect detector reduced this to a single action followed by a clean stop, verified on a live
  desktop virtual machine (the menu opened; the agent halted).
- **P5 measured (→H):** the model emitted premature "complete" 11–12/12 on a compound goal, and did
  so even when handed an explicit list of remaining steps — establishing that decomposition and
  completion must be deterministic.
- **End-to-end:** on a live desktop VM, the assembled selection architecture (A–G) selected the
  correct element and accomplished a single-step goal where a naive baseline wandered.

The experiment record and code history are retained in a private, timestamped repository.

## 8. Novelty and non-obviousness

- The prior art teaches *larger/cloud models* and *more context/memory* as the path to agent
  reliability. This invention teaches the **opposite**: a *smaller* model, *less* context in the
  decision (memory deliberately excluded), and reliability obtained by **deterministic rails**. That
  is counter-intuitive relative to the field's direction.
- The specific mechanisms — exploiting a measured *attention-position* profile via deterministic
  ordering (B); *deterministic* abstention because the model will not self-abstain (D); the
  *precondition+postcondition expected-effect signature* advance criterion distinct from a bare
  screen-change (H); and the admissibility distinction between *deterministic run-state* and
  *retrieved semantic memory* (A vs G/H) — are non-obvious and are each justified by a measured
  failure that the naive approach does not anticipate.
- The architecture is **enabled by, and arises from, the sovereignty constraint** (no large/cloud
  model permitted), which competitors building on cloud models have no reason to solve.

## 9. Patent vs. trade-secret strategy (for the assignee)

The shipped product is a local binary; a determined competitor could reverse-engineer the
*architecture* from it, so secrecy will not hold for the architecture — **patent it** (A–H).
Conversely, the **trained/fine-tuned models, the training/replay datasets, and the specific tuned
constants/thresholds** are not readily recoverable and confer ongoing advantage — **retain as trade
secrets**, do not disclose in the filing beyond what enablement requires.

## 10. Alternative embodiments / extensions (for breadth)

- Additional senses fused into the index space (audio, DOM); the index space generalizes.
- The attended band may be characterized per-model/per-deployment by an automated calibration sweep;
  ordering then targets the measured band rather than a fixed assumption.
- Label-less candidate selection via a relational descriptor (named anchor + spatial relation) or a
  cross-modal match, with the deterministic gate clearing on geometry/type when no label exists
  (a planned extension; selection of label-less elements currently escalates).
- The escalation transition may route to a larger/remote model tier where policy permits, with the
  local deterministic floor unchanged (a governed upgrade, off the reliability-critical path).
