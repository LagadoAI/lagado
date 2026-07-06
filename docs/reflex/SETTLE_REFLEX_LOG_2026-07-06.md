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
