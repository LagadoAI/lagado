"""membrane/record_feed.py — recorder v2: real-feed episodes with free labels.

Records what the being will actually live in: the RFB pixel feed (initial canvas +
every (t, rect, pixels) event at repaint cadence) while QMP drives NAMED stimuli —
the episode name IS the label (the proven settle-recorder pattern, now on the feed).

Episode file (npz): canvas0 [H,W] gray u8, rects [N,5] (t_rel,x,y,w,h) f64,
patches = concatenated gray rect pixels (offsets from rect dims), stim name, t_stim.

Usage: python record_feed.py <vnc_host> <vnc_port> <qmp_sock> <out_dir> [rounds]
"""
import json
import os
import sys
import time

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from membrane.rfb_feed import RfbFeed
from membrane.test_rungs13 import Qmp


def gray(px4):
    return (0.114 * px4[:, :, 0] + 0.587 * px4[:, :, 1] + 0.299 * px4[:, :, 2]).astype(np.uint8)


def _wheel(q, n, up=True):
    b = "wheel-up" if up else "wheel-down"
    for _ in range(n):
        for down in (True, False):
            q.cmd("input-send-event", events=[{"type": "btn", "data": {"button": b, "down": down}}])
        time.sleep(0.25)


def _drag(q, x1, y1, x2, y2, steps=8):
    q.mouse(x1, y1); time.sleep(0.15)
    q.cmd("input-send-event", events=[{"type": "btn", "data": {"button": "left", "down": True}}])
    for i in range(1, steps + 1):
        q.mouse(x1 + (x2 - x1) * i // steps, y1 + (y2 - y1) * i // steps)
        time.sleep(0.08)
    q.cmd("input-send-event", events=[{"type": "btn", "data": {"button": "left", "down": False}}])


def _type(q, text, ret=True):
    m = {" ": "spc", "-": "minus", "/": "slash", ".": "dot", "_": "shift-minus",
         "|": "shift-backslash", ":": "shift-semicolon", "~": "shift-grave_accent"}
    for ch in text:
        code = m.get(ch, ch)
        if code.startswith("shift-"):
            q.cmd("send-key", keys=[{"type": "qcode", "data": "shift"},
                                    {"type": "qcode", "data": code[6:]}])
        else:
            q.key(code)
        time.sleep(0.08)
    if ret:
        q.key("ret")


EPISODES = [
    # (name, pre_s, stim(q), post_s) — name is the free label; stim runs CONCURRENTLY
    ("quiet",        1.0, lambda q: None,                                  4.0),
    ("term_launch",  0.5, lambda q: q.click(237, 783),                     4.0),
    ("type_cmd",     0.5, lambda q: _type(q, "seq 1 400"),                 4.0),
    ("scroll_up",    0.5, lambda q: (q.mouse(640, 400), _wheel(q, 5, up=True)), 3.5),
    ("scroll_down",  0.5, lambda q: (q.mouse(640, 400), _wheel(q, 5, up=False)), 3.5),
    ("window_drag",  0.5, lambda q: _drag(q, 640, 95, 840, 260),           3.0),
    ("menu_open",    0.5, lambda q: q.click(40, 783),                      3.0),
    ("menu_close",   0.5, lambda q: q.key("esc"),                          2.5),
    ("cursor_blink", 0.5, lambda q: None,                                  4.0),
]


def record_episode(feed, q, name, pre, stim, post, out_dir, idx):
    canvas0 = gray(feed.canvas.copy())
    rects, patches = [], []
    t0 = time.time()
    t_stim = None

    def collect(duration):
        for ev in feed.updates(duration=duration):
            g = gray(feed.canvas[ev.y:ev.y + ev.h, ev.x:ev.x + ev.w].copy())
            rects.append((ev.t - t0, ev.x, ev.y, ev.w, ev.h))
            patches.append(g.reshape(-1))

    collect(pre)
    t_stim = time.time() - t0
    import threading
    th = threading.Thread(target=lambda: stim(q))
    th.start()                    # stim CONCURRENT with collection: per-frame
    collect(post)                 # updates arrive against an outstanding request
    th.join()
    path = os.path.join(out_dir, f"ep{idx:03d}_{name}.npz")
    np.savez_compressed(
        path, canvas0=canvas0,
        rects=np.array(rects, dtype=np.float64).reshape(-1, 5),
        patches=(np.concatenate(patches) if patches else np.zeros(0, dtype=np.uint8)),
        name=name, t_stim=t_stim)
    print(f"  {name}: {len(rects)} rects, t_stim={t_stim:.2f}s -> {os.path.basename(path)}", flush=True)
    return len(rects)


def main():
    host, port, qmp_sock, out_dir = sys.argv[1], int(sys.argv[2]), sys.argv[3], sys.argv[4]
    rounds = int(sys.argv[5]) if len(sys.argv) > 5 else 2
    os.makedirs(out_dir, exist_ok=True)
    q = Qmp.__new__(Qmp)
    import socket as _s
    q.s = _s.socket(_s.AF_UNIX); q.s.connect(qmp_sock); q.s.settimeout(10)
    q._recv(); q.cmd("qmp_capabilities")
    feed = RfbFeed(host, port).connect()
    for _ in feed.updates(duration=2.0):     # prime full canvas
        pass
    print(f"recording {rounds} rounds x {len(EPISODES)} episodes at {feed.width}x{feed.height}", flush=True)
    idx, total = 0, 0
    for rnd in range(rounds):
        for name, pre, stim, post in EPISODES:
            total += record_episode(feed, q, name, pre, stim, post, out_dir, idx)
            idx += 1
    json.dump({"episodes": idx, "rects": total, "size": [feed.width, feed.height]},
              open(os.path.join(out_dir, "meta.json"), "w"))
    print(f"DONE: {idx} episodes, {total} rect events -> {out_dir}", flush=True)


if __name__ == "__main__":
    main()
