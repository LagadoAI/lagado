"""eyes/baseline.py — the DETERMINISTIC change characterizer the learned eyes must beat.

Same discipline as the settle monitor's timer-null gate: a learned organ is promoted
only if it beats the best dumb machine that could do its job. This baseline is built
honestly strong — diff-mask bbox, shift-correlation scroll detection, periodicity for
animation, growth-pattern for text-vs-popup — not a strawman.

Input: window frames [T,H,W] uint8 (dt is IGNORED — the baseline is clockless, which
is exactly the capability gap the CfC is supposed to fill on irregular tick rates).
Output: label dict in the same schema as synth.py.
"""
import numpy as np

DIFF_THRESH = 12       # |Δgray| above sensor noise
NONE_FRAC = 3e-4       # changed-pixel fraction below this = no change
REPAINT_FRAC = 0.55    # above this = whole-surface repaint
MAX_SHIFT = 40         # scroll search range (px at model resolution)


def _diff_mask(a, b):
    return np.abs(a.astype(np.int16) - b.astype(np.int16)) > DIFF_THRESH


def _mask_bbox(mask):
    ys, xs = np.nonzero(mask)
    if len(ys) == 0:
        return None
    x0, x1 = xs.min(), xs.max() + 1
    y0, y1 = ys.min(), ys.max() + 1
    return int(x0), int(y0), int(x1), int(y1)


def _bbox_norm(x0, y0, x1, y1, W, H):
    return ((x0 + x1) / 2 / W, (y0 + y1) / 2 / H, (x1 - x0) / W, (y1 - y0) / H)


def characterize(frames):
    """frames [T,H,W] uint8 → label dict {kind, bbox, dir, mag}."""
    T, H, W = frames.shape
    f0, fl = frames[0], frames[-1]

    # union mask over the window (catches blink-back-to-start animations too)
    union = np.zeros((H, W), dtype=bool)
    for t in range(1, T):
        union |= _diff_mask(frames[t - 1], frames[t])
    frac = union.mean()
    if frac < NONE_FRAC:
        return {"kind": "none", "bbox": (0.0, 0.0, 0.0, 0.0), "dir": "na", "mag": 0.0}

    bb = _mask_bbox(union)
    x0, y0, x1, y1 = bb
    bbox = _bbox_norm(x0, y0, x1, y1, W, H)

    # ANIMATION: the surface RETURNS to a prior state WITH change in between (true
    # periodicity — a popup that appears and then holds still must NOT match, so the
    # departure is required between the matching pair, not anywhere in the window).
    per_step = [_diff_mask(frames[t - 1], frames[t]).mean() for t in range(1, T)]
    periodic = False
    for j in range(T - 1, max(T - 3, 0), -1):
        for i in range(0, j - 1):
            came_back = _diff_mask(frames[i], frames[j]).mean() < NONE_FRAC * 4
            departed = max(per_step[i:j]) > NONE_FRAC * 4  # per_step[t] = diff(t, t+1)
            if came_back and departed:
                periodic = True
    if periodic:
        return {"kind": "animation", "bbox": bbox, "dir": "na",
                "mag": float((x1 - x0) * (y1 - y0) / (W * H))}

    # REPAINT: most of the frame changed.
    if frac > REPAINT_FRAC:
        return {"kind": "repaint", "bbox": _bbox_norm(0, 0, W, H, W, H), "dir": "na", "mag": 1.0}

    # SCROLL: verified translation inside the changed region (same fixture-pinned
    # content-frame logic as the v1 segmented path).
    r0 = f0[y0:y1, x0:x1]
    rl = fl[y0:y1, x0:x1]
    if r0.shape[0] >= 12 and r0.shape[1] >= 12:
        sdy, sdx, gain = _phase_shift(r0, rl, mask=_diff_mask(r0, rl))
        if gain > 2.0 and (abs(sdy) > 1 or abs(sdx) > 1):
            if abs(sdy) >= abs(sdx):
                return {"kind": "scroll", "bbox": bbox,
                        "dir": "down" if sdy > 0 else "up",
                        "mag": abs(sdy) / max(1, y1 - y0)}
            return {"kind": "scroll", "bbox": bbox,
                    "dir": "right" if sdx > 0 else "left",
                    "mag": abs(sdx) / max(1, x1 - x0)}

    # TEXT_EDIT vs POPUP: typing grows monotonically in small strips; a dialog
    # appears in ONE step and then holds still.
    area = (x1 - x0) * (y1 - y0) / (W * H)
    steps_active = sum(1 for p in per_step if p > NONE_FRAC)
    strip_like = (y1 - y0) <= H // 8 and area < 0.05
    if strip_like and steps_active >= 2:
        return {"kind": "text_edit", "bbox": bbox, "dir": "na", "mag": float(frac)}
    if steps_active <= 2:
        return {"kind": "popup", "bbox": bbox, "dir": "na", "mag": float(area)}
    # ambiguous residual: call it text_edit if tiny, popup otherwise
    kind = "text_edit" if area < 0.02 else "popup"
    return {"kind": kind, "bbox": bbox, "dir": "na", "mag": float(frac if kind == "text_edit" else area)}


# ── v1: SEGMENTED, multi-event characterization (the membrane-shaped API) ─────────
#
# The live-guest run (2026-07-08) proved the union-event flaw: a right-click makes
# the menu APPEAR and the unfocused window DIM simultaneously; merged into one union
# region, the scroll correlator found a spurious shift and the event was garbage.
# One event PER CHANGED REGION fixes it. Regions come from compositor damage rects
# when available (the membrane: push, exact, free) or from connected components of
# the diff mask as the fallback (host-side, no guest support needed).
#
# DIRECTION CONVENTION (pinned by test fixtures, not by sign-chain reasoning):
# event.direction is the CONTENT-frame motion — the way the pixels visibly moved.
# Content sliding toward the bottom of the screen = "down". A wheel-up therefore
# produces direction "down" (earlier content slides down into view).

MERGE_PAD = 3          # dilation radius before segmentation (joins fragmented damage)
MIN_REGION_PX = 12     # ignore change specks below this many pixels


def segment_regions(mask, pad=None, min_px=None):
    """Connected components (with a merge distance `pad`) of the change mask →
    list of (x0,y0,x1,y1). RUN-BASED union-find: operates on horizontal runs, not
    pixels, so it holds NATIVE resolution (1920×1080 in single-digit ms for UI-shaped
    masks — the design runs at full fidelity, never a downsampled compromise)."""
    H, W = mask.shape
    # resolution-scaled merge distance: ~1% of the short edge (3px at model res,
    # ~10px at native 1080p — text lines in one pane merge, distinct UI islands don't)
    pad = max(MERGE_PAD, min(H, W) // 100) if pad is None else pad
    min_px = MIN_REGION_PX if min_px is None else min_px

    # extract row runs (vectorized per row)
    runs = []  # (y, x0, x1) with x1 exclusive
    active_rows = np.nonzero(mask.any(axis=1))[0]
    for y in active_rows:
        row = mask[y]
        d = np.diff(row.astype(np.int8))
        starts = np.nonzero(d == 1)[0] + 1
        ends = np.nonzero(d == -1)[0] + 1
        if row[0]:
            starts = np.concatenate(([0], starts))
        if row[-1]:
            ends = np.concatenate((ends, [W]))
        for s, e in zip(starts, ends):
            runs.append((int(y), int(s), int(e)))
    if not runs:
        return []

    # union-find over runs: two runs join if their rows are within pad+1 and their
    # pad-expanded x-intervals overlap
    parent = list(range(len(runs)))

    def find(i):
        while parent[i] != i:
            parent[i] = parent[parent[i]]
            i = parent[i]
        return i

    def union(i, j):
        ri, rj = find(i), find(j)
        if ri != rj:
            parent[rj] = ri

    by_row = {}
    for idx, (y, s, e) in enumerate(runs):
        by_row.setdefault(y, []).append(idx)   # in x order (rows scan left→right)
    for y, idxs in by_row.items():
        # same-row: consecutive runs chain when the gap is within 2*pad
        for a, b in zip(idxs, idxs[1:]):
            if runs[b][1] - runs[a][2] <= 2 * pad:
                union(a, b)
        # cross-row: two-pointer sweep over x-sorted runs (a run overlapping several
        # runs in the other row unions each in turn before advancing)
        for dy in range(1, pad + 2):
            other = by_row.get(y + dy)
            if not other:
                continue
            i = j = 0
            while i < len(idxs) and j < len(other):
                _, s1, e1 = runs[idxs[i]]
                _, s2, e2 = runs[other[j]]
                if s1 - pad < e2 + pad and s2 - pad < e1 + pad:
                    union(idxs[i], other[j])
                if e1 < e2:
                    i += 1
                else:
                    j += 1

    groups = {}
    for idx in range(len(runs)):
        groups.setdefault(find(idx), []).append(idx)
    boxes = []
    for members in groups.values():
        npx = sum(runs[i][2] - runs[i][1] for i in members)
        if npx < min_px:
            continue
        x0 = min(runs[i][1] for i in members)
        x1 = max(runs[i][2] for i in members)
        y0 = min(runs[i][0] for i in members)
        y1 = max(runs[i][0] for i in members) + 1
        boxes.append((int(x0), int(y0), int(x1), int(y1)))
    boxes.sort(key=lambda b: (b[1], b[0]))
    return boxes


def _phase_shift(a, b, mask=None):
    """Translation (dy, dx) such that b ≈ a moved by (dy, dx), via FFT phase
    correlation, VERIFIED by re-alignment MSE (the peak alone can lie on periodic
    content). Returns (dy, dx, gain) — gain = mse(no shift)/mse(realigned).
    `mask` restricts correlation to CHANGED pixels: inside a window-move's union
    region the static background otherwise dominates and zero-shift wins."""
    a = a.astype(np.float32)
    b = b.astype(np.float32)
    if mask is not None and mask.any():
        a = np.where(mask, a - a[mask].mean(), 0.0)
        b = np.where(mask, b - b[mask].mean(), 0.0)
    else:
        a = a - a.mean()
        b = b - b.mean()
    Fa = np.fft.rfft2(a)
    Fb = np.fft.rfft2(b)
    R = Fa * np.conj(Fb)
    R /= (np.abs(R) + 1e-9)
    corr = np.fft.irfft2(R, s=a.shape)
    dy, dx = np.unravel_index(np.argmax(corr), corr.shape)
    if dy > a.shape[0] // 2: dy -= a.shape[0]
    if dx > a.shape[1] // 2: dx -= a.shape[1]
    dy, dx = -int(dy), -int(dx)   # sign: fixture-verified (content-frame motion a→b)
    base = np.mean((a - b) ** 2) + 1e-6
    re = np.roll(b, (-dy, -dx), axis=(0, 1))
    sl_y = slice(max(0, -dy), a.shape[0] - max(0, dy)) if dy else slice(None)
    sl_x = slice(max(0, -dx), a.shape[1] - max(0, dx)) if dx else slice(None)
    err = np.mean((a[sl_y, sl_x] - re[sl_y, sl_x]) ** 2) + 1e-6
    return dy, dx, base / err


def _characterize_region(frames, box):
    """Single-region kind logic. frames [T,H,W]; box (x0,y0,x1,y1) in frame px."""
    x0, y0, x1, y1 = box
    T = frames.shape[0]
    H, W = frames.shape[1], frames.shape[2]
    r = frames[:, y0:y1, x0:x1]
    f0, fl = r[0], r[-1]
    bbox = _bbox_norm(x0, y0, x1, y1, W, H)
    area = (x1 - x0) * (y1 - y0) / (W * H)

    # animation: returns to a prior state WITH departure in between (as v0)
    per_step = [_diff_mask(r[t - 1], r[t]).mean() for t in range(1, T)]
    for j in range(T - 1, max(T - 3, 0), -1):
        for i in range(0, j - 1):
            if (_diff_mask(r[i], r[j]).mean() < NONE_FRAC * 4
                    and max(per_step[i:j]) > NONE_FRAC * 4):
                return {"kind": "animation", "bbox": bbox, "dir": "na", "mag": float(area)}

    # translation: verified phase correlation. Diagonal → moved (window relocation);
    # axis-aligned → scroll (pane content slide). Direction = CONTENT frame.
    if r.shape[1] >= 12 and r.shape[2] >= 12:
        dy, dx, gain = _phase_shift(f0, fl, mask=_diff_mask(f0, fl))
        if gain > 2.0 and (abs(dy) > 1 or abs(dx) > 1):
            if abs(dy) > 2 and abs(dx) > 2:
                mag = float(np.hypot(dy, dx) / max(y1 - y0, x1 - x0))
                d = ("down" if dy > 0 else "up") if abs(dy) >= abs(dx) else ("right" if dx > 0 else "left")
                return {"kind": "moved", "bbox": bbox, "dir": d, "mag": mag}
            if abs(dy) >= abs(dx):
                return {"kind": "scroll", "bbox": bbox,
                        "dir": "down" if dy > 0 else "up", "mag": abs(dy) / max(1, y1 - y0)}
            return {"kind": "scroll", "bbox": bbox,
                    "dir": "right" if dx > 0 else "left", "mag": abs(dx) / max(1, x1 - x0)}

    # repaint / text_edit / popup within the region (v0 rules, region-scoped)
    changed = _diff_mask(f0, fl).mean()
    if changed > REPAINT_FRAC and area > 0.5:
        return {"kind": "repaint", "bbox": _bbox_norm(0, 0, W, H, W, H), "dir": "na", "mag": 1.0}
    steps_active = sum(1 for p in per_step if p > NONE_FRAC)
    strip_like = (y1 - y0) <= H // 8 and area < 0.05
    if strip_like and steps_active >= 2:
        return {"kind": "text_edit", "bbox": bbox, "dir": "na", "mag": float(area)}
    return {"kind": "popup", "bbox": bbox, "dir": "na", "mag": float(area)}


def characterize_events(frames, rects=None):
    """frames [T,H,W] uint8 (+ optional damage rects in frame px) → LIST of events,
    one per changed region. rects, when given (the membrane), seed the mask exactly;
    otherwise the mask is the observed frame-to-frame diff union."""
    T, H, W = frames.shape
    if rects:
        mask = np.zeros((H, W), dtype=bool)
        for (rx, ry, rw, rh) in rects:
            mask[max(0, ry):min(H, ry + rh), max(0, rx):min(W, rx + rw)] = True
        # damage says WHERE the server repainted; keep only what visibly changed
        union = np.zeros((H, W), dtype=bool)
        for t in range(1, T):
            union |= _diff_mask(frames[t - 1], frames[t])
        mask &= union
    else:
        mask = np.zeros((H, W), dtype=bool)
        for t in range(1, T):
            mask |= _diff_mask(frames[t - 1], frames[t])
    if mask.mean() < NONE_FRAC:
        return [{"kind": "none", "bbox": (0.0, 0.0, 0.0, 0.0), "dir": "na", "mag": 0.0}]
    return [_characterize_region(frames, box) for box in segment_regions(mask)]
