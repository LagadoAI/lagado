"""Thin host-side client for the promoted settle monitor + the tick featurizer.

SettleMonitor spawns settle_service.py under the reflex venv (torch+ncps live
THERE, not in the caller's venv) and talks JSONL over pipes. FAIL-OPEN by
doctrine: any problem — venv missing, spawn failure, service crash, timeout,
bad JSON — returns (None, None) / None, marks the monitor dead, and NEVER
raises into the caller. Callers treat (None, None) as "monitor unavailable"
and fall back to the deterministic fixed-sleep floor.

TickFeaturizer reproduces guest_rec.py's multi-channel senses host-side from a
guest screenshot + wmctrl/pgrep readings, emitting the exact 52-dim vector the
monitor was trained on:
  [0:49]  pixel changed-fractions (8x6 grid + whole frame, 480x270, PIXEL_EPS=12)
  [49]    window-list changed this tick (0/1)
  [50]    window count / 8
  [51]    app process count / 8
The first tick after reset() emits all-ones pixel dims (no previous frame =
maximally busy) — conservative, and matches the recorder's frame-0 artifact.

The client itself is stdlib-only; TickFeaturizer needs numpy+PIL in the
CALLER's venv (imported lazily so SettleMonitor works without them).
"""
import json
import os
import select
import subprocess
import time

_BASE = os.path.dirname(os.path.abspath(__file__))
SERVICE_PY = os.path.join(_BASE, ".venv", "bin", "python")
SERVICE_SCRIPT = os.path.join(_BASE, "settle_service.py")


class SettleMonitor:
    """tick(feats, dt) -> (p, settled) | (None, None) on any failure (fail-open)."""

    def __init__(self, spawn_timeout=10.0, op_timeout=2.0):
        self.dead = False
        self.proc = None
        self._buf = b""
        self.op_timeout = op_timeout
        try:
            self.proc = subprocess.Popen(
                [SERVICE_PY, SERVICE_SCRIPT],
                stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL)
            # Warmup handshake: the first reset also absorbs torch import + model
            # load (service contract: ready < 5s; allow spawn_timeout of slack).
            r = self._rpc({"op": "reset"}, timeout=spawn_timeout)
            if r is None or not r.get("ok"):
                self._kill()
        except Exception:
            self._kill()

    # -- wire ------------------------------------------------------------------

    def _kill(self):
        self.dead = True
        if self.proc is not None:
            try:
                self.proc.kill()
                self.proc.wait(timeout=1)
            except Exception:
                pass

    def _read_line(self, deadline):
        """Read one newline-terminated response from the raw fd; None on timeout/EOF."""
        fd = self.proc.stdout.fileno()
        while b"\n" not in self._buf:
            remain = deadline - time.monotonic()
            if remain <= 0:
                return None
            ready, _, _ = select.select([fd], [], [], remain)
            if not ready:
                return None
            chunk = os.read(fd, 65536)
            if not chunk:                      # EOF: service died
                return None
            self._buf += chunk
        line, self._buf = self._buf.split(b"\n", 1)
        return line

    def _rpc(self, obj, timeout=None):
        if self.dead or self.proc is None or self.proc.poll() is not None:
            self._kill()
            return None
        try:
            self.proc.stdin.write((json.dumps(obj) + "\n").encode())
            self.proc.stdin.flush()
            line = self._read_line(time.monotonic() + (timeout or self.op_timeout))
            if line is None:
                self._kill()
                return None
            return json.loads(line)
        except Exception:
            self._kill()
            return None

    # -- API -------------------------------------------------------------------

    def tick(self, feats, dt):
        """-> (p, settled). (None, None) = monitor unavailable; use the floor."""
        r = self._rpc({"op": "tick", "feats": [float(x) for x in feats],
                       "dt": float(dt)})
        if r is None or "p" not in r:
            self._kill()
            return (None, None)
        return (float(r["p"]), bool(r["settled"]))

    def reset(self):
        """-> True | None (fail-open)."""
        r = self._rpc({"op": "reset"})
        if r is None or not r.get("ok"):
            self._kill()
            return None
        return True

    def close(self):
        if self.proc is not None:
            try:
                self.proc.stdin.close()
                self.proc.wait(timeout=2)
            except Exception:
                self._kill()
        self.dead = True


class TickFeaturizer:
    """guest_rec.py's senses, computed host-side. step() -> list of 52 floats."""

    GRID_COLS, GRID_ROWS = 8, 6
    PIXEL_EPS = 12
    W, H = 480, 270
    N_PIX = GRID_COLS * GRID_ROWS + 1          # 49

    def __init__(self):
        self.prev_px = None
        self.prev_wh = None

    def reset(self):
        self.prev_px = None
        self.prev_wh = None

    def step(self, png_bytes, win_hash, win_count, proc_count):
        import io

        import numpy as np
        from PIL import Image

        img = Image.open(io.BytesIO(png_bytes)).convert("RGB").resize((self.W, self.H))
        arr = np.asarray(img, dtype=np.int16)
        if self.prev_px is None:
            # TRAIN/PROD PARITY (2026-07-06 brutal-suite finding): the training recorder emits NO
            # feature row for the first frame; the old synthetic all-1.0 row here was out-of-
            # distribution — the model answered it garbage-confidently (p=0.999 settled on a
            # maximally-busy input) and the poisoned hidden state rode forward. First frame now
            # primes prev state and returns None; callers skip the tick.
            self.prev_px = arr
            self.prev_wh = win_hash
            return None
        else:
            changed = (np.abs(arr - self.prev_px).max(axis=2) > self.PIXEL_EPS)
            h, w = changed.shape
            ch, cw = h // self.GRID_ROWS, w // self.GRID_COLS
            px = []
            for r in range(self.GRID_ROWS):
                for c in range(self.GRID_COLS):
                    y1 = (r + 1) * ch if r < self.GRID_ROWS - 1 else h
                    x1 = (c + 1) * cw if c < self.GRID_COLS - 1 else w
                    px.append(float(changed[r * ch:y1, c * cw:x1].mean()))
            px.append(float(changed.mean()))
        row = px + [
            1.0 if (self.prev_wh is not None and win_hash != self.prev_wh) else 0.0,
            win_count / 8.0,
            proc_count / 8.0,
        ]
        self.prev_px = arr
        self.prev_wh = win_hash
        return row
