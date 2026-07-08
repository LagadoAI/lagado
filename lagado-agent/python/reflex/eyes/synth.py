"""eyes/synth.py — free-label training data for the change CHARACTERIZER (eyes v0).

The eyes must know HOW the screen changed, not just that it did (user directive
2026-07-08). Labels come free from two sources:
  (i)  synthetic transforms applied to ANY base screenshot — a scroll IS a sliding
       viewport, a popup IS an overlay composite, a text edit IS a growing local
       strip. This solves multi-app diversity without a VM: bases mix PIL-generated
       fake app UIs (infinite layout variety) with real recorded VM frames.
  (ii) real recordings labeled by episode stimulus (VM-side, later).

Each sample is a WINDOW of T grayscale frames with RANDOMIZED per-step dt (the
continuous-time argument: the same change at different tick rates must read the
same) and a label {kind, bbox, direction, magnitude}:
  kind      ∈ {none, scroll, popup, text_edit, repaint, animation}
  bbox      normalized (cx, cy, w, h) of the changed region ((0,0,0,0) for none)
  direction ∈ {na, up, down, left, right} (scroll only)
  magnitude normalized: scroll = total shift / region extent; others = changed-area
            fraction of the frame
"""
import numpy as np

H, W = 80, 128          # model-input resolution
T = 6                   # frames per window
KINDS = ["none", "scroll", "popup", "text_edit", "repaint", "animation"]
DIRS = ["na", "up", "down", "left", "right"]


# ── Base screens ──────────────────────────────────────────────────────────────────

def gen_ui_base(rng):
    """Draw a fake app screen with PIL: title bar, menu row, side panel, text lines,
    buttons, list rows — randomized theme/layout so no two 'apps' look alike."""
    from PIL import Image, ImageDraw
    up = 4  # draw at 4x then downsample (anti-aliased, text-like strokes)
    w, h = W * up, H * up
    bg = int(rng.integers(30, 230))
    img = Image.new("L", (w, h), bg)
    d = ImageDraw.Draw(img)
    fg = (bg + 128) % 256
    # title bar + menu row
    tb = int(rng.integers(20, 48))
    d.rectangle([0, 0, w, tb], fill=(bg + 40) % 256)
    for i in range(int(rng.integers(3, 7))):
        x = 20 + i * int(rng.integers(60, 110))
        d.rectangle([x, tb + 8, x + int(rng.integers(30, 70)), tb + 24], fill=fg)
    # optional side panel
    if rng.random() < 0.5:
        pw = int(rng.integers(60, 140))
        d.rectangle([0, tb, pw, h], fill=(bg + 20) % 256)
        for r in range(6, h // 40):
            d.rectangle([8, r * 40, pw - 8, r * 40 + 14], fill=fg if rng.random() < 0.7 else (bg + 60) % 256)
    # text lines / list rows in the content area
    y = tb + 40
    while y < h - 20:
        if rng.random() < 0.85:
            x0 = int(rng.integers(80, 200))
            d.rectangle([x0, y, x0 + int(rng.integers(120, w - x0 - 40)), y + int(rng.integers(8, 16))], fill=fg)
        y += int(rng.integers(24, 44))
    # a few buttons
    for _ in range(int(rng.integers(1, 4))):
        bx, by = int(rng.integers(100, w - 120)), int(rng.integers(tb + 20, h - 40))
        d.rectangle([bx, by, bx + 90, by + 28], outline=fg, width=3)
    img = img.resize((W, H), Image.BILINEAR)
    return np.asarray(img, dtype=np.uint8)


def load_real_bases(paths):
    """Load real screenshots as grayscale HxW bases."""
    from PIL import Image
    out = []
    for p in paths:
        try:
            img = Image.open(p).convert("L").resize((W, H), Image.BILINEAR)
            out.append(np.asarray(img, dtype=np.uint8))
        except Exception:
            continue
    return out


# ── Window builders (one per change kind) ──────────────────────────────────────────

def _noise(frames, rng, sigma=1.5):
    n = rng.normal(0.0, sigma, size=frames.shape)
    return np.clip(frames.astype(np.float32) + n, 0, 255).astype(np.uint8)


def _region(rng, min_frac=0.25):
    """A scrollable-pane-like subrect (sometimes the whole content area)."""
    if rng.random() < 0.4:
        return 8, 8, W - 8, H - 8
    rw = int(rng.integers(int(W * min_frac), W - 16))
    rh = int(rng.integers(int(H * min_frac), H - 16))
    x0 = int(rng.integers(0, W - rw))
    y0 = int(rng.integers(0, H - rh))
    return x0, y0, x0 + rw, y0 + rh


def _bbox_norm(x0, y0, x1, y1):
    return ((x0 + x1) / 2 / W, (y0 + y1) / 2 / H, (x1 - x0) / W, (y1 - y0) / H)


def make_window(kind, bases, rng):
    """Build one labeled window. Returns (frames [T,H,W] uint8, dts [T] float32, label)."""
    base = bases[int(rng.integers(len(bases)))].copy()
    dts = rng.uniform(0.08, 1.4, size=T).astype(np.float32)
    frames = np.stack([base] * T)
    label = {"kind": kind, "bbox": (0.0, 0.0, 0.0, 0.0), "dir": "na", "mag": 0.0}

    if kind == "none":
        pass

    elif kind == "scroll":
        x0, y0, x1, y1 = _region(rng)
        rh, rw = y1 - y0, x1 - x0
        horiz = rng.random() < 0.25
        # virtual page = region content + a second screen's content: the viewport
        # slides, REVEALING new pixels (np.roll would wrap — trivially detectable).
        other = bases[int(rng.integers(len(bases)))]
        total = int(rng.integers(4, (rw if horiz else rh) // 2))
        sign = 1 if rng.random() < 0.5 else -1
        if horiz:
            page = np.hstack([base[y0:y1, x0:x1], other[y0:y1, x0:x1]])
            offs = np.linspace(0, total, T).astype(int)
            for t in range(T):
                o = offs[t] if sign > 0 else page.shape[1] - rw - offs[t]
                frames[t] = base.copy()
                frames[t, y0:y1, x0:x1] = page[:, o:o + rw]
            label["dir"] = "left" if sign > 0 else "right"
            label["mag"] = total / rw
        else:
            page = np.vstack([base[y0:y1, x0:x1], other[y0:y1, x0:x1]])
            offs = np.linspace(0, total, T).astype(int)
            for t in range(T):
                o = offs[t] if sign > 0 else page.shape[0] - rh - offs[t]
                frames[t] = base.copy()
                frames[t, y0:y1, x0:x1] = page[o:o + rh, :]
            # CONVENTION (fixture-pinned, test_events.py): direction = CONTENT-frame
            # motion. Viewport sliding down the page = content moving UP on screen.
            label["dir"] = "up" if sign > 0 else "down"
            label["mag"] = total / rh
        label["bbox"] = _bbox_norm(x0, y0, x1, y1)

    elif kind == "popup":
        pw = int(rng.integers(W // 5, W // 2))
        ph = int(rng.integers(H // 5, H // 2))
        x0 = int(rng.integers(4, W - pw - 4))
        y0 = int(rng.integers(4, H - ph - 4))
        tone = int(rng.integers(0, 256))
        t0 = int(rng.integers(1, T - 1))
        pop = np.full((ph, pw), tone, dtype=np.uint8)
        pop[0, :] = pop[-1, :] = (tone + 128) % 256
        pop[:, 0] = pop[:, -1] = (tone + 128) % 256
        for r in range(6, ph - 4, 10):  # dialog "text"
            pop[r:r + 3, 6:6 + int(rng.integers(pw // 3, pw - 12))] = (tone + 100) % 256
        for t in range(t0, T):
            frames[t] = frames[t].copy()
            frames[t, y0:y0 + ph, x0:x0 + pw] = pop
        label["bbox"] = _bbox_norm(x0, y0, x0 + pw, y0 + ph)
        label["mag"] = (pw * ph) / (W * H)

    elif kind == "text_edit":
        sh = int(rng.integers(3, 9))               # a text-line-height strip
        full_w = int(rng.integers(10, W // 3))
        x0 = int(rng.integers(4, W - full_w - 4))
        y0 = int(rng.integers(4, H - sh - 4))
        tone = int(base[y0:y0 + sh, x0:x0 + full_w].mean())
        ink = (tone + 120) % 256
        t0 = int(rng.integers(1, T - 2))
        for t in range(t0, T):                     # typing: the strip GROWS
            grow = int(full_w * (t - t0 + 1) / (T - t0))
            frames[t] = frames[t - 1].copy()
            seg = frames[t, y0:y0 + sh, x0:x0 + grow]
            seg[:] = ink
            seg[:, ::3] = tone                     # glyph-ish gaps
        label["bbox"] = _bbox_norm(x0, y0, x0 + full_w, y0 + sh)
        label["mag"] = (full_w * sh) / (W * H)

    elif kind == "repaint":
        # correlated bases (frames of the same desktop) or mid-gray inversion make a
        # "repaint" that barely changes pixels — a mislabel. Require a real difference;
        # the tone-rotate fallback changes EVERY pixel by 128.
        candidates = [bases[int(rng.integers(len(bases)))], 255 - base,
                      ((base.astype(np.int16) + 128) % 256).astype(np.uint8)]
        if rng.random() < 0.3:
            candidates = candidates[1:]
        other = next(c for c in candidates
                     if np.abs(c.astype(np.int16) - base.astype(np.int16)).mean() >= 20)
        t0 = int(rng.integers(1, T - 1))
        for t in range(t0, T):
            frames[t] = other
        label["bbox"] = _bbox_norm(0, 0, W, H)
        label["mag"] = 1.0

    elif kind == "animation":
        aw = int(rng.integers(3, 16))
        ah = int(rng.integers(3, 16))
        x0 = int(rng.integers(4, W - aw - 4))
        y0 = int(rng.integers(4, H - ah - 4))
        tone = int(base[y0:y0 + ah, x0:x0 + aw].mean())
        alt = (tone + int(rng.integers(80, 170))) % 256
        for t in range(T):                         # cursor-blink / spinner toggle
            frames[t] = frames[t].copy()
            if t % 2 == 1:
                frames[t, y0:y0 + ah, x0:x0 + aw] = alt
        label["bbox"] = _bbox_norm(x0, y0, x0 + aw, y0 + ah)
        label["mag"] = (aw * ah) / (W * H)

    else:
        raise ValueError(kind)

    return _noise(frames, rng), dts, label


def make_dataset(n, bases, rng, kinds=KINDS):
    """n windows, kinds balanced. Returns (frames [n,T,H,W], dts [n,T], labels list)."""
    Fs, Ds, Ls = [], [], []
    for i in range(n):
        kind = kinds[i % len(kinds)]
        f, d, l = make_window(kind, bases, rng)
        Fs.append(f); Ds.append(d); Ls.append(l)
    return np.stack(Fs), np.stack(Ds), Ls
