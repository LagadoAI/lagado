"""Fixture tests for the segmented characterizer (eyes v1).

THE DIRECTION FIXTURES ARE THE CONVENTION: direction = content-frame motion
(pixels visibly sliding toward the bottom of the screen = "down"). Any sign
disagreement is a bug in the code, never in these fixtures.

Run: .venv/bin/python eyes/test_events.py
"""
import sys, os
import numpy as np

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from eyes import baseline

H, W, T = 80, 128, 6
rng = np.random.default_rng(11)


def textured(h, w):
    """Structured, non-periodic texture (rows/cols distinguishable — like UI)."""
    base = rng.integers(40, 200, size=(h, w)).astype(np.uint8)
    return (base // 32 * 32).astype(np.uint8)


def window_of(f0, fl, hold_at=3):
    return np.stack([f0] * hold_at + [fl] * (T - hold_at))


FAILS = []
def check(name, cond, detail=""):
    print(("PASS " if cond else "FAIL ") + name + ("  " + detail if detail else ""))
    if not cond:
        FAILS.append(name)


# ── direction fixtures (the convention) ──────────────────────────────────────────
def fixture_scroll(dy=0, dx=0):
    page = textured(H * 2, W * 2)
    f0 = page[40:40 + H, 40:40 + W].copy()
    fl = page[40 - dy:40 - dy + H, 40 - dx:40 - dx + W].copy()  # content moves +dy/+dx on screen
    return window_of(f0, fl)

for name, (dy, dx), want in [("content-down", (10, 0), "down"), ("content-up", (-10, 0), "up"),
                             ("content-right", (0, 12), "right"), ("content-left", (0, -12), "left")]:
    ev = baseline.characterize_events(fixture_scroll(dy, dx))
    check(f"dir {name} → {want}", len(ev) == 1 and ev[0]["kind"] == "scroll" and ev[0]["dir"] == want,
          f"got {[(e['kind'], e['dir']) for e in ev]}")

# ── moved: a window translated diagonally, wallpaper revealed ─────────────────────
wall = textured(H, W)
win = textured(30, 44)
f0 = wall.copy(); f0[10:40, 8:52] = win
fl = wall.copy(); fl[30:60, 40:84] = win
ev = baseline.characterize_events(window_of(f0, fl))
kinds = sorted(e["kind"] for e in ev)
check("moved: diagonal window drag", "moved" in kinds, f"got {kinds}")

# ── segmentation: two simultaneous, disjoint changes → two events ─────────────────
# (the live right-click failure: menu appears far from a window that dims)
f0 = textured(H, W)
fl = f0.copy()
fl[8:34, 6:50] = np.clip(f0[8:34, 6:50].astype(int) - 40, 0, 255).astype(np.uint8)  # window dims
fl[50:76, 84:120] = 230                                                              # menu appears
ev = baseline.characterize_events(window_of(f0, fl))
check("segmentation: dim + menu = 2 events", len(ev) == 2, f"got {len(ev)}: {[e['kind'] for e in ev]}")
if len(ev) == 2:
    menu = [e for e in ev if e["bbox"][0] > 0.5]
    check("segmentation: menu region is a popup", bool(menu) and menu[0]["kind"] == "popup",
          f"got {[(e['kind'], round(e['bbox'][0],2)) for e in ev]}")

# ── damage rects seed the mask exactly (membrane path) ────────────────────────────
ev = baseline.characterize_events(window_of(f0, fl), rects=[(6, 8, 44, 26), (84, 50, 36, 26)])
check("damage rects: same 2 events", len(ev) == 2, f"got {len(ev)}")

# ── none stays none ───────────────────────────────────────────────────────────────
f = textured(H, W)
ev = baseline.characterize_events(np.stack([f] * T))
check("none: static window", len(ev) == 1 and ev[0]["kind"] == "none", f"got {[e['kind'] for e in ev]}")

print("\n%d/%d fixtures pass" % (9 - len(FAILS), 9))
sys.exit(1 if FAILS else 0)
