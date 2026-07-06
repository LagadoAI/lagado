"""Frame -> settle-reflex feature vector.

Mirrors the production DeltaDetector geometry (perception/delta.rs: 8x6 pixel-space
grid, remainder pixels folded into the last col/row) but emits per-cell CHANGED
FRACTIONS instead of blake3 hashes — the reflex net needs magnitude, not identity.

Feature vector per frame (49 dims):
  [0:48]  per-cell fraction of pixels whose max-channel abs diff > PIXEL_EPS
  [48]    whole-frame changed fraction
(dt is appended at training time from the timestamps — CfC consumes elapsed time.)
"""
import io

import numpy as np
from PIL import Image

GRID_COLS = 8
GRID_ROWS = 6
PIXEL_EPS = 12        # per-channel abs diff below this = compression/dither noise
DOWNSCALE = 4         # diff on a 1/4-res frame; settle semantics unchanged, 16x cheaper

N_FEATURES = GRID_COLS * GRID_ROWS + 1


class Featurizer:
    """Stateful: diffs each frame against the previous one."""

    def __init__(self):
        self.prev = None

    def reset(self):
        self.prev = None

    def step(self, png_bytes):
        img = Image.open(io.BytesIO(png_bytes)).convert("RGB")
        if DOWNSCALE > 1:
            img = img.resize((img.width // DOWNSCALE, img.height // DOWNSCALE),
                             Image.BILINEAR)
        arr = np.asarray(img, dtype=np.int16)
        if self.prev is None or self.prev.shape != arr.shape:
            self.prev = arr
            return np.ones(N_FEATURES, dtype=np.float32)  # first frame = all changed
        changed = (np.abs(arr - self.prev).max(axis=2) > PIXEL_EPS)
        self.prev = arr
        h, w = changed.shape
        ch, cw = h // GRID_ROWS, w // GRID_COLS
        out = np.zeros(N_FEATURES, dtype=np.float32)
        for r in range(GRID_ROWS):
            for c in range(GRID_COLS):
                y1 = (r + 1) * ch if r < GRID_ROWS - 1 else h   # remainder -> last row
                x1 = (c + 1) * cw if c < GRID_COLS - 1 else w   # remainder -> last col
                cell = changed[r * ch:y1, c * cw:x1]
                out[r * GRID_COLS + c] = cell.mean()
        out[N_FEATURES - 1] = changed.mean()
        return out
