"""In-guest settle-monitor episode recorder v8 (deployed into the VM, run detached).

MULTI-CHANNEL (single-sense recorders are banned — 2026-07-06 standing rule; the
pixel-only versions burned two days on this guest's headless-compositor traps):
  [0:49]  pixel changed-fractions, 8x6 grid + whole-frame (gnome-screenshot with
          present-forcing; context-dependently flaky here — ONE VOTER, not truth)
  [49]    window-list changed this tick (0/1, wmctrl hash vs previous)
  [50]    window count / 8
  [51]    app process count (soffice.bin + gimp) / 8
Dropped pixel frames are logged (unlink-first guard) — blind gaps, never stale reads.

STIMULI: SHELL:<cmd> | PYAUTO:<code> | UNO:<verb>:<json> — UNO verbs go through the
resident session daemon's client (uno_client.py, local socket) in a background
thread; the call's synchronous return time is recorded as t_stim_done = the
app-truth completion timestamp (the teaching oracle for label generation).

Usage: python3 guest_rec.py <name> <duration_s> <stim> [dump_dir] [stim_at_s]
(v9: stim_at_s randomizes WHEN the stimulus fires — fixed 2.0s taught the v1 model a clock.)
"""
import hashlib
import json
import os
import subprocess
import sys
import threading
import time

import numpy as np
import pyautogui
from PIL import Image

pyautogui.FAILSAFE = False

GRID_COLS, GRID_ROWS = 8, 6
PIXEL_EPS = 12
W, H = 480, 270
N_PIX = GRID_COLS * GRID_ROWS + 1
OUT_DIR = "/home/user/reflex_out"
UNO_CLIENT = "/tmp/uno_client.py"
UNO_SOCK = "/tmp/lagado_session.sock"

EVAL_OK = False   # probed once in main()


def _eval_ok():
    """Can we ask the compositor to present directly? (Eval is often locked on
    newer GNOME; probe once, fall back to synthetic-input nudge.)"""
    r = subprocess.run(["gdbus", "call", "--session", "--dest", "org.gnome.Shell",
                        "--object-path", "/org/gnome/Shell", "--method",
                        "org.gnome.Shell.Eval", "global.stage.queue_redraw()"],
                       capture_output=True, text=True, timeout=10)
    return r.returncode == 0 and "true" in r.stdout


def force_present(slot):
    """The compositor presents frames only for an audience on this headless guest:
    direct command when allowed, else a 1 px input nudge every other frame."""
    if EVAL_OK:
        subprocess.run(["gdbus", "call", "--session", "--dest", "org.gnome.Shell",
                        "--object-path", "/org/gnome/Shell", "--method",
                        "org.gnome.Shell.Eval", "global.stage.queue_redraw()"],
                       capture_output=True, timeout=10)
    elif slot % 2 == 0:
        pyautogui.moveRel(1 if slot % 4 else -1, 0)


def grab_pixels(slot):
    """One voter. unlink-first: a failed capture is a dropped frame, never stale."""
    force_present(slot)
    f = "/tmp/reflex_cap_%d.png" % (slot % 2)
    try:
        os.unlink(f)
    except OSError:
        pass
    r = subprocess.run(["gnome-screenshot", "-f", f], capture_output=True, timeout=10)
    if r.returncode != 0 or not os.path.exists(f):
        return None
    img = Image.open(f).convert("RGB").resize((W, H))
    return np.asarray(img, dtype=np.int16)


def pixel_feats(prev, arr):
    changed = (np.abs(arr - prev).max(axis=2) > PIXEL_EPS)
    h, w = changed.shape
    ch, cw = h // GRID_ROWS, w // GRID_COLS
    out = []
    for r in range(GRID_ROWS):
        for c in range(GRID_COLS):
            y1 = (r + 1) * ch if r < GRID_ROWS - 1 else h
            x1 = (c + 1) * cw if c < GRID_COLS - 1 else w
            out.append(float(changed[r * ch:y1, c * cw:x1].mean()))
    out.append(float(changed.mean()))
    return out


def window_sense():
    r = subprocess.run("DISPLAY=:0 wmctrl -l", shell=True, capture_output=True,
                       text=True, timeout=10)
    lines = [ln for ln in r.stdout.splitlines() if ln.strip()]
    return hashlib.md5(r.stdout.encode()).hexdigest(), len(lines)


def proc_sense():
    r = subprocess.run("pgrep -c soffice.bin; pgrep -c gimp", shell=True,
                       capture_output=True, text=True, timeout=10)
    vals = [int(x) if x.strip().isdigit() else 0 for x in r.stdout.splitlines()[:2]]
    return sum(vals)


class Stim:
    """Fires the stimulus; UNO verbs run threaded so capture never blocks, and
    their synchronous return = app-truth completion (t_done)."""

    def __init__(self, spec):
        self.spec = spec
        self.t_fired = -1.0
        self.t_done = -1.0
        self.ok = None

    def fire(self, t0):
        self.t_fired = time.time() - t0
        if self.spec.startswith("UNO:"):
            _, verb, payload = self.spec.split(":", 2)

            def run():
                r = subprocess.run(["python3", UNO_CLIENT, verb, payload,
                                    "--sock=%s" % UNO_SOCK],
                                   capture_output=True, text=True, timeout=180)
                self.t_done = time.time() - t0
                self.ok = '"ok": true' in r.stdout or '"ok":true' in r.stdout

            threading.Thread(target=run, daemon=True).start()
        elif self.spec.startswith("PYAUTO:"):
            exec(self.spec[len("PYAUTO:"):], {"pyautogui": pyautogui, "time": time})
        else:
            cmd = self.spec[len("SHELL:"):] if self.spec.startswith("SHELL:") else self.spec
            subprocess.Popen(cmd, shell=True)


def main():
    global EVAL_OK
    name, duration, spec = sys.argv[1], float(sys.argv[2]), sys.argv[3]
    dump_dir = sys.argv[4] if len(sys.argv) > 4 else ""
    stim_at = float(sys.argv[5]) if len(sys.argv) > 5 else 2.0
    if dump_dir:
        os.makedirs(dump_dir, exist_ok=True)
    os.makedirs(OUT_DIR, exist_ok=True)
    try:
        EVAL_OK = _eval_ok()
    except Exception:
        EVAL_OK = False
    stim = Stim(spec) if spec else None
    ts, feats, dropped = [], [], []
    prev_px = None
    prev_wh = None
    n = 0
    t0 = time.time()
    while time.time() - t0 < duration:
        if stim and stim.t_fired < 0 and (time.time() - t0) >= stim_at:
            stim.fire(t0)
        arr = grab_pixels(n)
        wh, wc = window_sense()
        pc = proc_sense()
        if arr is None:
            dropped.append(round(time.time() - t0, 2))
            n += 1
            time.sleep(0.2)
            prev_wh = wh
            continue
        if dump_dir and n % 10 == 0:
            Image.fromarray(arr.astype(np.uint8)).save(
                os.path.join(dump_dir, "f%03d_t%04.1f.png" % (n, time.time() - t0)))
        n += 1
        if prev_px is not None:
            row = pixel_feats(prev_px, arr)
            row.append(1.0 if (prev_wh is not None and wh != prev_wh) else 0.0)
            row.append(wc / 8.0)
            row.append(pc / 8.0)
            feats.append(row)
            ts.append(time.time() - t0)
        prev_px = arr
        prev_wh = wh
    out = {"t": ts, "feats": feats, "t0_epoch": t0,
       "t_stim": stim.t_fired if stim else -1.0,
           "t_stim_done": stim.t_done if stim else -1.0,
           "stim_ok": stim.ok if stim else None, "dropped": dropped, "name": name}
    path = os.path.join(OUT_DIR, name + ".json")
    json.dump(out, open(path, "w"))
    open(path + ".done", "w").write("ok")


if __name__ == "__main__":
    main()
