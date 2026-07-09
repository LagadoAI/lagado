"""membrane/canvas_feed.py — THE capture path: RFB feed → shared-memory live canvas.

CANVAS CONTRACT (user-ratified: reduce conversions): one raw BGRX buffer in
/dev/shm, written ONCE by this feed, VIEWED everywhere — numpy/torch (zero-copy),
Rust cv_proposer (mmap slice), vision shim. No image codecs in the live loop.

Layout of /dev/shm/lagado_canvas:
  magic 'LGCV' | u32 w | u32 h | u32 stride | u64 seq   (24-byte header, LE)
  BGRX pixels (h * stride bytes)
`seq` bumps after each rect batch — readers detect liveness/change cheaply.

Run:  python canvas_feed.py <host> <port> [duration_s]
"""
import mmap
import os
import struct
import sys
import time

import numpy as np

CANVAS = os.environ.get("LAGADO_CANVAS", "/dev/shm/lagado_canvas")
MAGIC = b"LGCV"
HDR = 24


class ShmCanvas:
    def __init__(self, w, h):
        self.w, self.h, self.stride = w, h, w * 4
        size = HDR + h * self.stride
        f = open(CANVAS, "w+b")
        f.truncate(size)
        self.mm = mmap.mmap(f.fileno(), size)
        f.close()
        self.seq = 0
        self.mm[:HDR] = MAGIC + struct.pack("<IIIQ", w, h, self.stride, 0)
        self.px = np.frombuffer(self.mm, dtype=np.uint8, offset=HDR).reshape(h, self.stride // 4, 4)

    def patch(self, canvas_src, x, y, w, h):
        self.px[y:y + h, x:x + w] = canvas_src[y:y + h, x:x + w]
        self.seq += 1
        self.mm[16:24] = struct.pack("<Q", self.seq)   # seq LAST (readers see coherent-enough state)


def open_view(path=CANVAS):
    """Reader side: zero-copy numpy view + header. torch.from_numpy(view) shares it."""
    f = open(path, "rb")
    mm = mmap.mmap(f.fileno(), 0, prot=mmap.PROT_READ)
    f.close()
    if mm[:4] != MAGIC:
        raise ValueError("not a lagado canvas")
    w, h, stride, seq = struct.unpack("<IIIQ", mm[4:HDR])
    view = np.frombuffer(mm, dtype=np.uint8, offset=HDR).reshape(h, stride // 4, 4)
    return view, (w, h, stride), mm


def main():
    sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    from membrane.rfb_feed import RfbFeed
    host, port = sys.argv[1], int(sys.argv[2])
    duration = float(sys.argv[3]) if len(sys.argv) > 3 else 1e9
    feed = RfbFeed(host, port).connect()
    shm = ShmCanvas(feed.width, feed.height)
    shm.patch(feed.canvas, 0, 0, feed.width, feed.height)   # primed full frame
    print(f"canvas {feed.width}x{feed.height} -> {CANVAS}", flush=True)
    n = 0
    for ev in feed.updates(duration=duration):
        shm.patch(feed.canvas, ev.x, ev.y, ev.w, ev.h)
        n += 1
        if n % 200 == 0:
            print(f"{n} rects, seq={shm.seq}", flush=True)


if __name__ == "__main__":
    main()
