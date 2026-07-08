"""Live validation driver for rung 1 (RFB feed) + rung 3 (RAM mmap) against a QEMU
guest booted with `-vnc :7 -object memory-backend-file,share=on,mem-path=/dev/shm/lg_ram`.

Stimulus is QMP input-send-event mouse motion (server-side cursor + any hover
repaints); reference truth is QMP screendump.
"""
import json
import os
import socket
import sys
import time

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from membrane.rfb_feed import RfbFeed
from membrane import fb_mmap

QMP_SOCK = "/tmp/lg_qmp.sock"
RAM = "/dev/shm/lg_ram"
OUT = "/tmp/lagado_membrane"
os.makedirs(OUT, exist_ok=True)


class Qmp:
    def __init__(self, path):
        self.s = socket.socket(socket.AF_UNIX)
        self.s.connect(path)
        self.s.settimeout(10)
        self._recv()                                  # greeting
        self.cmd("qmp_capabilities")

    def _recv(self):
        buf = b""
        while not buf.endswith(b"\n"):
            buf += self.s.recv(65536)
        return [json.loads(l) for l in buf.decode().strip().split("\n")]

    def cmd(self, name, **args):
        self.s.sendall((json.dumps({"execute": name, "arguments": args}) + "\n").encode())
        while True:
            for m in self._recv():
                if "return" in m or "error" in m:
                    return m

    def mouse(self, x, y):
        self.cmd("input-send-event", events=[
            {"type": "abs", "data": {"axis": "x", "value": int(x * 32767 / 1280)}},
            {"type": "abs", "data": {"axis": "y", "value": int(y * 32767 / 800)}}])

    def click(self, x, y):
        self.mouse(x, y)
        for down in (True, False):
            self.cmd("input-send-event", events=[
                {"type": "btn", "data": {"button": "left", "down": down}}])
            time.sleep(0.05)

    def key(self, qcode):
        self.cmd("send-key", keys=[{"type": "qcode", "data": qcode}])

    def screendump(self, path):
        return self.cmd("screendump", filename=path)


def main():
    q = Qmp(QMP_SOCK)

    # ── RUNG 1: RFB feed ──
    print("── RUNG 1: RFB raw-pixel feed ──")
    feed = RfbFeed("127.0.0.1", 5907).connect()
    print(f"  connected: {feed.width}x{feed.height} '{feed.name}'")
    # drain the initial full update
    n0 = 0
    t0 = time.time()
    for _ in feed.updates(duration=2.0):
        n0 += 1
    print(f"  initial/full: {n0} rects in {time.time()-t0:.1f}s; canvas mean={feed.gray().mean():.1f}")
    # stimulate: mouse sweep via QMP while collecting incremental rects
    events = []
    t0 = time.time()
    import threading
    def stim():
        time.sleep(0.3)
        q.click(40, 783)          # open the Cinnamon menu (big repaint)
        time.sleep(1.5)
        q.key("esc")              # close it (second repaint)
        time.sleep(0.8)
        q.click(40, 783)          # open again
    
    th = threading.Thread(target=stim)
    th.start()
    for ev in feed.updates(duration=4.0):
        events.append(ev)
    th.join()
    print(f"  stimulated: {len(events)} rect events in 4.0s")
    if events:
        dts = np.diff([e.t for e in events])
        areas = [e.w * e.h for e in events]
        print(f"  cadence: median dt={np.median(dts)*1000:.0f}ms  rect areas min/med/max="
              f"{min(areas)}/{int(np.median(areas))}/{max(areas)} px")
    from PIL import Image
    Image.fromarray(feed.gray()).save(f"{OUT}/rung1_canvas.png")
    print(f"  live canvas -> {OUT}/rung1_canvas.png")
    feed.close()

    # ── RUNG 3: zero-copy RAM read (all buffer generations, pick the live one) ──
    print("\n── RUNG 3: guest-RAM mmap framebuffer ──")
    q.screendump(f"{OUT}/ref1.ppm")
    from PIL import Image
    Image.open(f"{OUT}/ref1.ppm").save(f"{OUT}/ref1.png")
    hits = fb_mmap.locate(RAM, f"{OUT}/ref1.png")
    if not hits:
        print("  RUNG3: NOT-FOUND")
        return
    ref = np.asarray(Image.open(f"{OUT}/ref1.png").convert("RGB"))
    H, W, _ = ref.shape
    # change the screen, then find which candidate TRACKS the fresh state
    q.key("esc"); time.sleep(1.0)
    q.click(40, 783)              # menu open = large fresh change
    time.sleep(1.2)
    q.screendump(f"{OUT}/ref2.ppm")
    Image.open(f"{OUT}/ref2.ppm").save(f"{OUT}/ref2.png")
    ref2 = np.asarray(Image.open(f"{OUT}/ref2.png").convert("RGB"), dtype=np.int16)
    best = None
    for off, stride, fmt in hits:
        raw = fb_mmap.read_raw(RAM, off, W, H, stride)
        rgb = raw[:, :, [2, 1, 0]] if fmt.startswith("BGR") else raw[:, :, [0, 1, 2]]
        mad2 = np.abs(rgb.astype(np.int16) - ref2).mean()
        mad1 = np.abs(rgb.astype(np.int16) - ref.astype(np.int16)).mean()
        print(f"  candidate off={off}: MAD fresh={mad2:.2f} old={mad1:.2f}")
        if best is None or mad2 < best[0]:
            best = (mad2, off, stride, fmt, rgb)
    mad2, off, stride, fmt, rgb = best
    Image.fromarray(rgb.astype(np.uint8), "RGB").save(f"{OUT}/rung3_live.png")
    print(f"  BEST off={off} fmt={fmt} MAD-vs-fresh={mad2:.2f} -> {OUT}/rung3_live.png")
    print("  RUNG3: " + ("TRACKS-LIVE ✓" if mad2 < 3.0 else "STALE/MISMATCH"))


if __name__ == "__main__":
    main()
