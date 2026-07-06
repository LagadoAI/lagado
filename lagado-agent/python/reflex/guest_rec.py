"""In-guest settle-episode recorder (deployed into the VM, run detached).

Captures via a single continuous ffmpeg x11grab stream (6 fps, guest-side scaled
to 480x270 rawvideo over a pipe), computes the 8x6 per-cell changed-fraction
features inline (same grid geometry as host features.py), fires its own stimulus
at t=2 s (shell command or PYAUTO: pyautogui code), and writes one JSON per
episode.

Capture is gnome-screenshot (compositor path), ~3.3 Hz. HARD-WON LESSON
(2026-07-06, frame-probe photographic proof): on this guest (Xorg + gnome-shell
on a qemu dummy display), ALL root-buffer captures — ffmpeg x11grab, xwd,
Pillow XGetImage — read a stale/half-live framebuffer, NOT the live screen
(x11grab frames showed a Calc window while wmctrl proved none existed). The
only captures that always matched live truth are compositor-path ones:
gnome-screenshot, pyautogui single-shots via the execute channel, and the
OSWorld /screenshot endpoint. gs_probe validated this loop: 3.33 Hz sustained,
launch paint = 0.25 diff 0.5 s after fire, wmctrl-corroborated, zero errors.

Usage: python3 guest_rec.py <name> <duration_s> <stim> [dump_dir]
  (stim '' = none; dump_dir set -> also save every 10th frame as PNG there)
"""
import json
import os
import subprocess
import sys
import time

import numpy as np
import pyautogui
from PIL import Image

pyautogui.FAILSAFE = False

GRID_COLS, GRID_ROWS = 8, 6
PIXEL_EPS = 12
W, H = 480, 270
OUT_DIR = "/home/user/reflex_out"


def grab(slot):
    """Live-truth capture via the compositor path; ~300 ms per call.

    JIGGLE (jiggle-probe, 2026-07-06): gnome-shell on this headless display only
    re-presents its stage on input events — without the 1 px nudge every other
    frame, window paints are invisible to ANY capture (five void runs). ~1 Hz
    dose; 3.3 Hz forced repaints starved the llvmpipe guest. unlink-first makes
    a failed capture a dropped frame, never a stale read."""
    if slot % 2 == 0:
        pyautogui.moveRel(1 if slot % 4 else -1, 0)
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


def feats_from(prev, arr):
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


def fire(stim):
    if stim.startswith("PYAUTO:"):
        exec(stim[len("PYAUTO:"):], {"pyautogui": pyautogui, "time": time})
    else:
        subprocess.Popen(stim, shell=True)


def main():
    name, duration, stim = sys.argv[1], float(sys.argv[2]), sys.argv[3]
    dump_dir = sys.argv[4] if len(sys.argv) > 4 else ""
    if dump_dir:
        os.makedirs(dump_dir, exist_ok=True)
    os.makedirs(OUT_DIR, exist_ok=True)
    ts, feats = [], []
    t_stim = -1.0
    prev = None
    n = 0
    t0 = time.time()
    while time.time() - t0 < duration:
        if stim and t_stim < 0 and (time.time() - t0) >= 2.0:
            fire(stim)
            t_stim = time.time() - t0
        arr = grab(n)
        if arr is None:
            n += 1
            time.sleep(0.2)
            continue
        if dump_dir and n % 10 == 0:
            Image.fromarray(arr.astype(np.uint8)).save(
                os.path.join(dump_dir, "f%03d_t%04.1f.png" % (n, time.time() - t0)))
        n += 1
        if prev is not None:
            feats.append(feats_from(prev, arr))
            ts.append(time.time() - t0)
        prev = arr
    path = os.path.join(OUT_DIR, name + ".json")
    json.dump({"t": ts, "feats": feats, "t_stim": t_stim, "name": name}, open(path, "w"))
    open(path + ".done", "w").write("ok")


if __name__ == "__main__":
    main()
