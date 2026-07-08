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


def _best_shift(f0, f1, axis):
    """Best integer shift of f1 (within the changed region) matching f0 along axis.
    Returns (shift, gain): gain = mse(no shift) / mse(best shift). A real scroll has a
    shift whose re-alignment slashes the error; noise / local edits don't."""
    base_err = np.mean((f0.astype(np.float32) - f1.astype(np.float32)) ** 2) + 1e-6
    best_s, best_err = 0, base_err
    size = f0.shape[0] if axis == 0 else f0.shape[1]
    for s in range(1, min(MAX_SHIFT, size // 2)):
        for sign in (1, -1):
            shifted = np.roll(f1, sign * s, axis=axis)
            # exclude the wrapped band from the comparison
            if axis == 0:
                sl = (slice(s, None) if sign > 0 else slice(None, -s)), slice(None)
            else:
                sl = slice(None), (slice(s, None) if sign > 0 else slice(None, -s))
            err = np.mean((f0[sl].astype(np.float32) - shifted[sl].astype(np.float32)) ** 2) + 1e-6
            if err < best_err:
                best_s, best_err = sign * s, err
    return best_s, base_err / best_err


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

    # SCROLL: re-alignment inside the changed region slashes the mismatch.
    r0 = f0[y0:y1, x0:x1]
    rl = fl[y0:y1, x0:x1]
    if r0.shape[0] >= 8 and r0.shape[1] >= 8:
        sv, gv = _best_shift(r0, rl, axis=0)
        sh_, gh = _best_shift(r0, rl, axis=1)
        if max(gv, gh) > 2.0:
            if gv >= gh:
                return {"kind": "scroll", "bbox": bbox,
                        "dir": "down" if sv > 0 else "up",
                        "mag": abs(sv) / max(1, y1 - y0)}
            return {"kind": "scroll", "bbox": bbox,
                    "dir": "left" if sh_ > 0 else "right",
                    "mag": abs(sh_) / max(1, x1 - x0)}

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
