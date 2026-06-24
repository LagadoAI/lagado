# Future research tasks

Durable home for research-horizon threads that are NOT near-term builds — the bets we earn on the bench
later, captured so the reasoning isn't lost. Near-term work lives in `docs/osworld/INVESTIGATION_PLAN_*`.
Cross-ref: CLAUDE.md "PARALLEL RESEARCH" + "SPRINT PLAN"; memory `lagado-prompt-brittleness`,
`lagado-capability-interface-altitude`, `lagado-native-session-plane`, `lagado-lfm-facts`.

---

## R1 — The latent membrane: collapse the conversion tax between app and agent (2026-06-23)

**Thesis (user, refined over the 2026-06-23 grounding session).** The bottleneck in a computer-use agent is
**conversion**. To touch one cell today we go: app binary → text serialization → tokens → the model's latent
vectors → cognition → output tokens → op string → parse → UNO call → binary. ~6 conversions each way; each is
both a **latency** tax and a **lossy** one — serializing to `"40557"` destroys date-ness, serializing to
`"Loan Issue Date"` destroys column-identity. Everything the harness does to *ground* (bind a name back to a
column, a serial back to a date) is reconstructing what conversion destroyed. We built a machine for
un-spilling milk.

**The insight from photonic / neuromorphic / quantum substrates:** their shared miracle is that the
representation and the computation are the *same thing* — no shuttling between "where data lives" and "where
it's worked on" (the von Neumann tax dissolves). Applied to agent+app: the tax is the conversion between the
app's representation and the model's, currently bridged through the lossy *human* interface (text/pixels).

**Where it goes — the back door's final form = a shared latent membrane.** Combine the two collapses:
the back-door already deletes the *human-interface* conversions (no pixels/GUI — app speaks structured ops);
the latent thread deletes the *representation* conversion (model thinks in vectors, not strings). Together, the
app exposes its state **in the medium the model already thinks in**: a column handed over as a latent handle
that *is* the column, carrying identity + type in the vector. Nothing serialized ⇒ nothing lost ⇒ nothing to
ground. Reference-binding and type-grounding don't get solved — they stop *existing*. Perception becomes
attention over shared state; action becomes selection in a shared action space. And there is **no app-state-vs-
model-memory divergence to reconcile** because there is no second representation to drift — one substrate (the
real form of what the native-session-plane op-log approximates).

**R1a — ODE / continuous-time models as the membrane's reflex layer (the keystone).** The membrane's reflex
layer — an always-live latent mirror of app-state, synced per-op, that binds + acts in continuous space — *is*
a continuous-time net (liquid / CfC / LTC / NCP / neural-ODE). A transformer computes in discrete jumps (each a
conversion: tokenize/embed/attend/project); an ODE net holds state as a continuous vector and *flows* it by
integrating an ODE — inference is evolution, not stepping. Event-driven by construction: an op is a **boundary
condition** nudging the flow, not a full context re-chew (vs a transformer re-processing its whole context on
every change → maps onto native-session "live, per-op, no re-serialize", as a *model* not a daemon). And it is
the software photonic/neuromorphic hardware was waiting for — a liquid net on analog photonic hardware deletes
the final conversion, the digital clock itself.

**HONEST SEAMS (earn on the bench; do NOT let romance override engineering — "Liquid" must not load-bear):**
- Feeding raw latent vectors *into* a frozen model's reasoning is **training-gated** (needs an aligned
  projector, LLaVA-style). The *full* membrane wants the co-designed fat-free model at the end of the road.
- ODE nets have **earned** continuous-CONTROL (robotics/drones/time-series sensor→actuator); they are
  **unproven at discrete, compositional REASONING / tool-authoring** (the regime transformers own). Zero prior
  art for tool-calling; architectural-fit headwind.
- LFM2 is NOT an ODE net (discrete edge-CPU hybrid) — the brand doesn't earn the property (`lagado-lfm-facts`).
- ⇒ Likely shape: a SPLIT (rhymes with the reasoner/emitter split + ReAct-reflex): perception-reflex-binding
  goes ODE-native/continuous (its strength, where the membrane lives); symbolic reasoning stays discrete for
  now; they meet at the membrane. `ncps` (Apache-2.0) is the parallel-research vehicle — promote ONLY if it
  matches at lower latency/params on OUR action vocab, benchmarked vs the transformer on the same vocab.

**R1b — EMPIRICAL (2026-06-23, the first inward rung, PROBED on real models).** Tested training-free
semantic binding (resolve a fuzzy NL reference to a live column header by embedding cosine):
- **One unified Qwen2.5-Coder-7B instance serves chat + grammar + /v1/embeddings simultaneously** (verified:
  `--embeddings --pooling last`, chat+grammar unaffected). So NO separate embedder is needed — the brain IS
  the encoder, and binding happens in the brain's OWN latent space (the membrane requirement). Single model.
- **LFM2-ColBERT mean-pool = useless** (cosines 0.96–0.98 with an UNRELATED column at 0.961). Wrong family
  (≠ the Qwen brain's space → a THIRD representation, another conversion) AND wrong pooling (ColBERT is built
  for token-level MaxSim; mean-pooling discards its mechanism). User caught the family mismatch; data confirmed.
- **Qwen MEAN-pool = poor** (3/6 bound to the WRONG header, tiny margins). **Qwen LAST-TOKEN pool = the lever**:
  distinctive refs bind with STRONG margins and distractors go NEGATIVE — "the movie titles"→Garbage Movie
  titles (margin 0.19), "amount spent"→Spent ($) (margin 0.55, all else <0). A causal LM packs its summary in
  the last token; pooling is DECISIVE. Still WRONG on genuine ambiguity (three overlapping loan-DATE columns;
  terse single-word "Rank" anti-correlated) — but those are exactly where a margin threshold should FAIL-CLOSED
  and abstain (sound). Not "code model can't do language" — pooling was the confound, failures are real overlap.
- **VERDICT:** training-free latent binding in the brain's own space is VIABLE for distinctive references.
  Resolver design = LEXICAL-first (exact header) → semantic-fallback ONLY when lexical fails AND margin>θ →
  else fail-closed. Lowers the R1 wall: same-family naive embeddings carry usable signal short of full co-design.
  NOT YET wired into the harness (needs a constructed fuzzy-reference eval task to demonstrate a NEW gold;
  safe-by-construction on existing lexical golds since the semantic path never fires when exact match wins).
  ⚠️ inv-#10: this is a SEPARATE fail-closed resolver, NEVER prompt-injected context.

**The membrane is a GRADIENT, not a cliff — walk it inward one conversion at a time:** native session already
deleted GUI + re-open conversions → next rung = a persistent **latent mirror** of app-state, synced per-op by
our own embedder, that the harness binds against (references/retrieval resolve in that shared space, not by
string match) = the lexical grounding we shipped, promoted to its semantic form → … → the last rung is the
co-designed continuous model where the final conversion (tokens) disappears. **Direct, training-free near-term
applications of this thread to the CURRENT harness are tracked in the INVESTIGATION_PLAN, not here.**
