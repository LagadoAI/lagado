# Lagado — Design Rationale

*Why it's built this way. The code shows what; this document is the why, and what each
decision cost us. Read alongside the [README](README.md).*

---

## 1. Local-only is not a feature — it's the absence of a cloud path

There is no telemetry endpoint to disable, no "privacy mode" to enable, no subprocessor
list to audit. The architecture simply has no road off the machine. Inference, memory,
keys, and the agent's working surface all live on the user's disk.

**Why:** privacy promises made in policy are revocable; privacy guaranteed by architecture
is not. The incumbents structurally cannot offer this — their business models *are* the
egress. That asymmetry is the moat.

**What it cost:** we give up frontier-model quality, server-side fleet learning, and easy
installs. Every capability has to fit in consumer hardware. We accepted that trade on day one.

## 2. A small brain in a strong body

The main model is an 8B mixture-of-experts. Alone, an 8B agent is weak — we don't dispute
that, we design for it. The model is a frontal cortex inside a support structure: persistent
memory, procedural habit, recognition, recovery, and a human in the loop. When a person works
long hours they forget, they slip, they re-check, they lean on notes. We don't fix that by
making the person bigger; we give them tools. Same answer here.

**Why:** the empirical record is consistent — scaffolded small models close most of the gap
on procedural work, and what breaks them is being asked to hold everything in their heads.
We never ask that.

**What it cost:** the support structure is most of the engineering. The model was the easy part.

## 3. The router never sees history

Every incoming message is classified by a fast 1.2B model on a clean prompt — zero
conversation history, current message only. This is a hard invariant, not a default.

**Why:** small-model accuracy collapses under accumulated context. The field cargo-cults
"give the model everything"; we treat history as a contaminant. The router stays in the
single-pass regime it was trained for; the heavy brain is reserved for what's worth its
latency.

**What it cost:** an occasional misroute that context would have caught (~80% router
accuracy). We accept it, because the human gate makes a misroute an inconvenience, not
an incident.

## 4. Memory has a physics, not a policy

Every memory trace carries a temperature: it decays exponentially with time
(`V = T · e^(−λt) · (1 + ln(n+1))` — an Ebbinghaus curve with logarithmic reinforcement on
access) and reheats when touched. Hot working memory, warm summaries, cold encrypted exact
text. A background sleep gate consolidates the day's residue; warm memory is pruned by
entropy value; the cold vault is never auto-forgotten.

**Why:** "should this be forgotten?" is an unanswerable policy question with a thousand edge
cases. "Is it still warm?" answers itself. Giving memory a physics makes every retention
decision fall out of the equation instead of a rule book.

**What it cost:** forgetting is lossy by design, and λ is a judgment call. We tuned for a
30-day half-life on working memory and a 365-day half-life on the vault, and we will be
wrong for some users until this is user-tunable.

## 5. Three memories, because one is not how competence works

Experience is stored three ways: **episodic** (what happened, in the tiered store),
**procedural** (the action graph — workflows that succeeded become replayable habit, firing
only on an exact state match at high confidence), and **technique** (the skill library —
completed tasks distilled by the model into advisory know-how). Screens the agent has seen
become **recognition** memory via visual embeddings.

**Why:** a pianist doesn't recall sheet music as anecdotes. Recall, habit, and technique are
different faculties with different retrieval rules, and collapsing them into one vector store
loses exactly the distinctions that make experience usable.

**What it cost:** three systems to keep coherent, and a consolidation pipeline (the sleep
gate) to maintain them. Worth it: this is the pillar everything else serves.

## 6. The agent works in a VM, not on your desktop

The agent's hands operate inside a QEMU virtual machine — its own sandboxed desktop, with
perception through the accessibility tree and actuation through synthetic input.

**Why:** blast radius. An agent that can click *your* desktop can click your bank. The VM
gives the agent a real working surface — real browser, real file manager — where the worst
case is a broken sandbox image, which is a file you delete. Sovereignty cuts both ways: the
user is sovereign over the agent too.

**What it cost:** VM orchestration, guest provisioning, and perception plumbing that a
"runs on your desktop" agent skips. We paid it for the safety story, and because the
debugging honesty it forces (you cannot fake a screendump) made the whole perception stack
better.

## 7. Every action passes a human gate

All agent actions flow through one chokepoint. Reads pass; writes require a tap; destructive
actions require typed confirmation — and destructive *content* in arguments (an `rm -rf` in
a command string) hard-overrides whatever trust level the tool had.

**Why:** the arithmetic of reliability. An 80%-accurate router with a human confirming writes
is a near-perfectly-reliable *system*, because the human closes the last gap at exactly the
moments that matter. The gate is not an apology for model weakness; it is what converts
capability into trustworthiness.

**What it cost:** friction, and an agent that can't run fully unattended by default. We think
products that skipped this gate are one incident away from rediscovering it.

## 8. The thing that watches can't touch; the thing that touches can't watch

The best teacher an agent can have is the human demonstrating the work. But a host-screen
recorder living inside an agent binary is a standing back door — gates protect *actions*,
and watching is passive. So demonstration capture (the Lens) is a separate artifact:
explicitly launched, visibly recording, able to see and never to act. It produces
inspectable workflow capsules that the user reviews and imports into the agent's action
graph. The agent, in turn, acts in its VM and can never see the host.

**Why:** the two most dangerous capabilities — observing the user and acting autonomously —
must never share a process. Separation by architecture, not by promise.

**What it cost:** the Lens is designed, not yet built, and the learning loop is slower than
a baked-in recorder would be. We accept slower for sovereign.

## 9. Keys: the raw key never touches disk

A random data-encryption key is wrapped twice — once by an Argon2id-derived key from the
password, once from a recovery phrase. Login unwraps it into memory; lockout after repeated
failures persists across restarts and fails closed if tampered with.

**Why:** the FileVault/1Password pattern exists because it's correct: password changes don't
re-encrypt the world, a forgotten password has exactly one recovery path, and disk forensics
yield nothing.

**What it cost:** lose the password *and* the recovery phrase and the data is gone. That is
not a bug. A vendor who can recover your data is a vendor who can read it.

## 10. Verification before autonomy

The perception stack fuses three senses — the accessibility tree, a classical-CV box
proposer, and per-patch vision-model embeddings — deduplicated by an IoU arbiter. Frames are
diffed cell-by-cell, so after every action there is a ground-truth answer to *"did the screen
actually change the way I expected?"*

**Why:** an agent that cannot check its own work compounds errors silently. We built the eyes
for verification before scaling the autonomy, because the reverse order is how demos are made
and products fail.

**What it cost:** weeks of unglamorous work on tiling geometry, coordinate spaces, and edge
cases — capability that produces no flashy demo by itself. It is the foundation the flashy
demo stands on.

## 11. Inference over HTTP, not FFI

Models run in a vendored `llama.cpp` server subprocess; the agent talks to it over local
HTTP. (One exception: the small vision encoder is in-process FFI, because embedding vectors
per frame don't justify a server round-trip.)

**Why:** crash isolation — a model crash is a restart, not an app crash; a health monitor
auto-restarts it. Plus model-swap without relink, and a clean seam for the governor to make
hardware-aware decisions (GPU layers, MoE expert placement) per machine.

**What it cost:** milliseconds of localhost overhead per call. Cheap insurance.

## 12. We publish what doesn't work

The test suite is real (156 and counting, CI on three platforms), the commit history is
honest, and the status sections of our docs say what is unproven as plainly as what is
proven. Internally we run an adversarial review on every component — the rule that found
our favorite bug: a vision overview-tile detected by token count instead of structure,
correct at every resolution we tested and wrong at the one we hadn't.

**Why:** a system asking to be trusted with someone's entire digital life has to earn it in
how it's built, not just what it claims. Candor in engineering is a security property.

**What it cost:** nothing. It's the cheapest discipline on this list.

---

*Lagado Labs*
