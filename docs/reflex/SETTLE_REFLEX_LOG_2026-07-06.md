# Settle-reflex (CfC expert #1) — build + investigation log, 2026-07-05/06

**Context.** First expert of the reflex bank (cortex/subcortex directive, ratified 2026-07-05):
a ~37k-param CfC (`ncps`, Apache-2.0) that learns "is the screen done reacting?" from the live
change-stream, gated against the production-shaped deterministic rule (K=3 consecutive quiet
frames) on false-settles / misses / latency, 4-fold CV over held-out rounds. Promote only if it
wins all three; the deterministic floor is never removed. Training: CPU, minutes, data minted
from the OSWorld VM by scripted stimuli (launch/type/scroll/toggle/close + quiet/blink negatives),
labels retrospective ("settled at t" = nothing above the calibrated noise floor in the next 2 s).

**Code.** `lagado-agent/python/reflex/`: `features.py` (8×6 DeltaDetector-geometry changed
fractions), `guest_rec.py` (in-guest recorder), `record_settle.py` (host orchestrator),
`train_cfc.py` (trainer + CV promotion gate). Venv: `reflex/.venv` (py3.12, torch-cpu, ncps).

## Gate runs so far — ALL VOID (arena bugs, not model verdicts)
- v1 (host-polled capture): guest server 500s under launch load → every cold-launch episode lost.
- v2 (in-guest pyautogui loop): all launches flat. First CV run also had a real trainer bug:
  raw changed-fractions span 5 orders of magnitude → net input-blind; fixed with log-scaling.
- v3 (in-guest ffmpeg x11grab): launches still flat.
- v4/v5 (gnome-screenshot loop, validated live in gs_probe): launches STILL flat, while close
  episodes catch 94.5% teardowns → the window paints but the capture never sees the paint.

## Root causes found (photo-proven where noted)
1. **Root-buffer captures are not the live screen** on this guest (Xorg + gnome-shell on qemu
   dummy display): x11grab frames showed a Calc window while wmctrl proved none existed
   (frame-probe PNGs). ffmpeg/xwd/XGetImage all read a stale or half-live buffer. Live truth =
   compositor-path only (gnome-screenshot, pyautogui single-shots, OSWorld /screenshot endpoint).
2. **Working theory under test (jiggle probe): gnome-shell only re-presents its stage on input
   events/animations** on this headless display — so even compositor-path captures freeze during
   input-free episodes; every input-driven episode recorded fine, every launch (no input) flat,
   and v4's close (94.5%) proves the paint existed. Fix candidate: 1 px mouse-jiggle per capture
   (~1 Hz dose — 3.3 Hz forced-repaint starved the llvmpipe guest into server 500s).
3. **pkill self-match, FIVE recurrences** (stim string in recorder argv; episode NAME in argv;
   my own host wrappers 3×). Rule: never `pkill -f` a pattern that can appear in any launcher's
   argv; use comm-match (`pkill soffice`) or pid-file kills.
4. **Ops:** podman volume leak (15 volumes) + idle brain (~5 GB) exhausted the 15 GB host →
   escalating guest 500s all day. Fixed: prune volumes between runs; stop the brain during
   recording (restart via start_brain.sh).

## State at commit
Tuned jiggle probe in flight. If it shows the launch paint: patch recorder (jiggle dose +
unlink-before-capture freshness guard + blind-gap masking in oracle labels) → v6 record →
CV gate → verdict. If not: STOP and put the fork to the user — keep excavating this guest's
compositor vs. move recording to the Lagado QEMU guest (QMP screendump capture, already trusted
by the Rust harness). Verdict/fork report will be appended below.

---

## OVERNIGHT STOP REPORT (contract executed; machine suspended after this commit)

**v6 (jiggle-fixed) tripwire FIRED → hard stop per contract.** Round-0 evidence:
quiet 0.019 / launch_calc **0.000 across 211 frames (75 s)** / typing 0.002 / sidebar 0.071 /
scroll 0.010 / **close_calc 0.945**. The close proves Calc launched, painted, and tore down
visibly — the paint again landed outside the launch episode's watch, even with the jiggle that
the probe validated at 0.9452 the same evening.

**Sharpest remaining hypothesis (NOT yet tested — for the morning):** every context that ever
saw a launch paint used **no file or the smallest xlsx** (gs_probe, jiggle probe: paint ≤2 s
post-fire). The recorder loads a **full OSWorld task xlsx into a cold LibreOffice**; v4's
timeline brackets its paint at 40–90 s post-fire; rounds using different pool files behaved
differently (0.039 vs 0.000). Plain reading: cold-LO + heavy-file load exceeds every window
tried (32 s, 75 s), and the paint keeps landing in inter-episode gaps where process-per-episode
recording discards it as the first-frame artifact.

**THE FORK (user decision, morning):**
- **A. One cheap test on this guest** (~15 min): launch episode with (i) pre-warmed LO
  (launch+close once off-camera before round 0), or (ii) 150 s window, or (iii) smallest-file
  pool. If the tripwire passes, v7 = the real gate run here. Also worth folding in: one
  guest_rec process per ROUND (episodes as segments) so no inter-episode blindness exists at all.
- **B. Move recording to the Lagado QEMU guest** (Fedora/Cinnamon, QMP screendump capture —
  the Rust harness's own trusted path). More wiring (~half a day: stimuli + episode driver on
  that guest), but escapes this compositor's traps entirely, runs on the production-adjacent
  surface, and doubles as the first cross-environment data point for the reflex-transfer story.

**Recommendation:** A(i)+(iii) first — cheapest, and the close-episode evidence makes it likely;
fall back to B if the tripwire fires again. B is worth doing eventually regardless (transfer story).

**Score for the night:** 0 valid gate runs; 6 real bugs found+fixed (input scaling, HTTP-capture
fragility, pkill self-match ×5-with-rule, stale root-buffer capture [photo-proven], input-gated
stage present [probe-proven], host RAM/volume exhaustion); recorder+trainer hardened
(freshness guard, blind-gap label masking, flat-stimulus tripwire, comm pkills, volume prune,
4G guest). The CfC itself remains untested on honest data — by design, not accident: every void
verdict was caught by the integrity rails before it could masquerade as a model result.

---

## FIRST HONEST GATE (v8, 2026-07-06 evening): HOLD — fast but not yet trustworthy

v8 recorder integrated with the session plane (UNO daemon owns LibreOffice lifecycle; its call
returns = teaching-oracle timestamps), multi-channel senses (pixels as one voter + window-list +
process counts), fused labels. 32 clean episodes over 3 rounds (round-2 tail + round 3 lost to a
guest-server death; per-round session redeploy fix required — uno_close kills the resident soffice).

3-fold CV, held-out rounds: BASELINE FS=1 miss=0 latency=2.252s | CFC (37,745 params)
FS=2 miss=2 latency=0.038s → **HOLD** by the pre-registered rule. The signal is now REAL: loss
converges every fold (vs unlearnable on single-sense data), and correct fires are ~59× faster
than the deterministic rule. Failure mode = the single fixed fail-closed threshold (0.85 → 2 FS;
0.95 → 2 misses), i.e. the decision layer, not the net. Next levers: 2-consecutive-tick patience
read-out (+~0.4s, likely kills FS) and more data (32 episodes → threshold instability across folds).
IQ4_XS quant A/B same day: FAILED regression (net −1 on flips, 17/30 vs 20/30) — Q4_K_M keeps the
reasoner seat; dialect-sensitivity applies to quants, not just models.
