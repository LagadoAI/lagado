"""membrane/rfb_feed.py — RUNG 1: the raw pixel feed. No screenshots exist here.

RFB (the VNC protocol) IS the membrane design, 30 years old: the client keeps an
incremental FramebufferUpdateRequest open and the server pushes back ONLY the
rectangles that changed, with raw pixels, at repaint cadence. This module speaks
minimal RFB 3.x (security none, raw encoding) and maintains a persistent LIVE
CANVAS patched rect-by-rect. Downstream there is no "frame" — only:
  canvas        — always-current pixels (numpy [H,W] gray + [H,W,3] rgb)
  events        — (t_arrival, x, y, w, h) per damaged rect
The eyes characterize per rect; the fovea reads the canvas; dt comes from real
rect-arrival intervals (the CfC's home regime).

Usage:
    feed = RfbFeed("127.0.0.1", 5900)
    feed.connect()
    for ev in feed.updates(duration=10.0):   # yields RectEvent as they arrive
        ...
"""
import socket
import struct
import time

import numpy as np


class RectEvent:
    __slots__ = ("t", "x", "y", "w", "h")

    def __init__(self, t, x, y, w, h):
        self.t, self.x, self.y, self.w, self.h = t, x, y, w, h

    def __repr__(self):
        return f"rect(t={self.t:.3f} {self.x},{self.y} {self.w}x{self.h})"


class RfbFeed:
    def __init__(self, host, port):
        self.host, self.port = host, port
        self.sock = None
        self.width = self.height = 0
        self.canvas = None            # [H,W,4] uint8 BGRX as served
        self.events = []

    # ── wire helpers ──────────────────────────────────────────────────────────
    def _recv(self, n):
        buf = b""
        while len(buf) < n:
            chunk = self.sock.recv(n - len(buf))
            if not chunk:
                raise ConnectionError("RFB socket closed")
            buf += chunk
        return buf

    def connect(self):
        self.sock = socket.create_connection((self.host, self.port), timeout=10)
        self.sock.settimeout(30)
        # version handshake
        server_ver = self._recv(12)
        self.sock.sendall(b"RFB 003.008\n")
        # security: expect None (1) among offered types
        n_sec = self._recv(1)[0]
        if n_sec == 0:
            raise ConnectionError("server refused: " + self._recv(4).decode(errors="replace"))
        types = self._recv(n_sec)
        if 1 not in types:
            raise ConnectionError(f"no 'None' security among {list(types)}")
        self.sock.sendall(bytes([1]))
        if server_ver >= b"RFB 003.008":
            result = struct.unpack(">I", self._recv(4))[0]
            if result != 0:
                raise ConnectionError("security handshake failed")
        # ClientInit: shared=1
        self.sock.sendall(bytes([1]))
        # ServerInit
        si = self._recv(24)
        self.width, self.height = struct.unpack(">HH", si[:4])
        name_len = struct.unpack(">I", si[20:24])[0]
        self.name = self._recv(name_len).decode(errors="replace")
        # force our pixel format: 32bpp BGRX little-endian true-colour
        pf = struct.pack(">BBBBHHHBBBxxx", 32, 24, 0, 1, 255, 255, 255, 16, 8, 0)
        self.sock.sendall(struct.pack(">BxxxB", 0, 0)[:1] + b"\x00\x00\x00" + pf)
        # SetEncodings: raw only (0)
        self.sock.sendall(struct.pack(">BxHi", 2, 1, 0))
        self.canvas = np.zeros((self.height, self.width, 4), dtype=np.uint8)
        # prime with one FULL (non-incremental) update, then incremental forever
        self._request(incremental=0)
        return self

    def _request(self, incremental=1):
        self.sock.sendall(struct.pack(">BBHHHH", 3, incremental, 0, 0, self.width, self.height))

    # ── the feed ──────────────────────────────────────────────────────────────
    def _read_update(self):
        """Read one FramebufferUpdate message → list of RectEvents (canvas patched)."""
        while True:
            mtype = self._recv(1)[0]
            if mtype == 0:                       # FramebufferUpdate
                break
            if mtype == 2:                       # Bell
                continue
            if mtype == 3:                       # ServerCutText
                ln = struct.unpack(">I", self._recv(7)[3:])[0]
                self._recv(ln)
                continue
            if mtype == 1:                       # SetColourMapEntries
                hdr = self._recv(5)
                n = struct.unpack(">H", hdr[3:5])[0]
                self._recv(6 * n)
                continue
            raise ConnectionError(f"unexpected RFB message type {mtype}")
        n_rects = struct.unpack(">H", self._recv(3)[1:])[0]
        out = []
        now = time.time()
        for _ in range(n_rects):
            x, y, w, h, enc = struct.unpack(">HHHHi", self._recv(12))
            if enc != 0:
                raise ConnectionError(f"non-raw encoding {enc}")
            data = self._recv(w * h * 4)
            patch = np.frombuffer(data, dtype=np.uint8).reshape(h, w, 4)
            self.canvas[y:y + h, x:x + w] = patch
            out.append(RectEvent(now, x, y, w, h))
        return out

    def updates(self, duration=10.0):
        """Yield RectEvents for `duration` seconds; keeps the incremental request loop open."""
        t_end = time.time() + duration
        while time.time() < t_end:
            evs = self._read_update()
            self._request(incremental=1)
            for e in evs:
                self.events.append(e)
                yield e

    # ── views the being consumes (no frames, only the canvas) ─────────────────
    def gray(self):
        c = self.canvas
        return (0.114 * c[:, :, 0] + 0.587 * c[:, :, 1] + 0.299 * c[:, :, 2]).astype(np.uint8)

    def fovea(self, cx, cy, size=33):
        """High-res crop of the LIVE canvas at (cx, cy) pixels."""
        half = size // 2
        x0 = int(np.clip(cx - half, 0, self.width - size))
        y0 = int(np.clip(cy - half, 0, self.height - size))
        return self.gray()[y0:y0 + size, x0:x0 + size]

    def close(self):
        try:
            self.sock.close()
        except Exception:
            pass
