"""membrane/autolabel.py — the offline auto-labeler (Tesla pattern, our organ).

Replays each recorded feed episode (canvas0 + rect patches), reconstructs the frame
sequence, and runs the deterministic eyes (characterize_events, damage rects seeding
segmentation) over the stimulus window. Output: labels.jsonl — structured change
events per episode, ready to train the learned eyes/being.

Because episode names are stimulus truth, this run doubles as the FIRST
real-recording accuracy measurement of eyes v1: stimulus family vs detected kind.

Usage: python autolabel.py <corpus_dir>
"""
import glob
import json
import os
import sys

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from eyes import baseline

# stimulus family → acceptable eyes kinds (the agreement rubric)
EXPECT = {
    "quiet": {"none"},
    "cursor_blink": {"none", "animation"},
    "scroll_up": {"scroll"},
    "scroll_down": {"scroll"},
    "window_drag": {"moved", "popup"},      # union of old+new pos reads popup pre-v1 fovea
    "menu_open": {"popup"},
    "menu_close": {"popup", "repaint"},     # content restored where menu was
    "term_launch": {"popup"},
    "type_cmd": {"text_edit", "popup", "scroll"},  # typing + output flood + autoscroll
    "menu_open2": {"popup"}, "menu_close2": {"popup", "repaint"},
    "hover_items": {"popup", "animation", "text_edit"},
    "type_burst": {"none", "text_edit"},
}
MAX_FRAMES = 10


def replay(ep):
    """Reconstruct frames: canvas0, then canvas state sampled at ≤MAX_FRAMES event
    times across the episode. Returns (frames [T,H,W] u8, all_rects px)."""
    canvas = ep["canvas0"].copy()
    rects = ep["rects"]
    patches = ep["patches"]
    frames = [canvas.copy()]
    n = len(rects)
    keep = set(np.linspace(0, n - 1, min(n, MAX_FRAMES - 1)).astype(int)) if n else set()
    off = 0
    out_rects = []
    for i, (t, x, y, w, h) in enumerate(rects):
        x, y, w, h = int(x), int(y), int(w), int(h)
        patch = patches[off:off + w * h].reshape(h, w)
        off += w * h
        canvas[y:y + h, x:x + w] = patch
        out_rects.append((x, y, w, h))
        if i in keep:
            frames.append(canvas.copy())
    if len(frames) == 1:
        frames.append(canvas.copy())
    return np.stack(frames), out_rects


def main():
    corpus = sys.argv[1]
    out = open(os.path.join(corpus, "labels.jsonl"), "w")
    agree, total = 0, 0
    per_stim = {}
    for path in sorted(glob.glob(os.path.join(corpus, "ep*.npz"))):
        ep = np.load(path, allow_pickle=True)
        name = str(ep["name"])
        frames, rects = replay(ep)
        events = baseline.characterize_events(frames, rects=rects if rects else None)
        kinds = sorted({e["kind"] for e in events if e["kind"] != "none"}) or ["none"]
        ok = bool(EXPECT.get(name, set()) & set(kinds)) if name in EXPECT else None
        rec = {"ep": os.path.basename(path), "stim": name, "n_rects": len(rects),
               "kinds": kinds, "agree": ok,
               "events": [{k: (round(v, 4) if isinstance(v, float) else
                               [round(x, 4) for x in v] if isinstance(v, tuple) else v)
                           for k, v in e.items()} for e in events[:6]]}
        out.write(json.dumps(rec) + "\n")
        if ok is not None:
            total += 1
            agree += int(ok)
            per_stim.setdefault(name, []).append(ok)
        flag = {True: "✓", False: "✗", None: "?"}[ok]
        print(f"  {flag} {os.path.basename(path):28s} {name:13s} rects={len(rects):3d} → {','.join(kinds)}")
    out.close()
    print(f"\nAGREEMENT (eyes v1 vs stimulus truth, REAL feed recordings): {agree}/{total}"
          f" = {agree/max(1,total):.2f}")
    for s, oks in sorted(per_stim.items()):
        print(f"  {s:13s} {sum(oks)}/{len(oks)}")
    print(f"labels -> {corpus}/labels.jsonl")


if __name__ == "__main__":
    main()
