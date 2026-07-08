"""eyes/train_eyes.py — train + gate the change characterizer (eyes v0).

Conv encoder per step → CfC over the window with REAL dt → heads:
  kind (6-way), bbox (4, normalized), direction (5-way), magnitude (scalar).

SPLIT DISCIPLINE (the transfer claim, in miniature):
  train bases  = PIL-generated fake apps + HALF the real VM frames
  eval bases   = the OTHER half of the real VM frames (pixels never seen in training)
Both gates run on the SAME eval windows:
  gate 1 (baseline-null): the deterministic characterizer (baseline.py)
  verdict PROMOTE only if the model beats the baseline where it counts; else HOLD
  with the honest per-class numbers.

Run: .venv/bin/python eyes/train_eyes.py [--quick]
"""
import glob
import json
import os
import sys
import time

import numpy as np
import torch
import torch.nn as nn
from ncps.torch import CfCCell

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from eyes import synth, baseline
from eyes.synth import KINDS, DIRS, T, H, W

SEED = 7
REAL_GLOBS = ["/tmp/lagado_battery/frames/*.png", "/tmp/lagado_battery/settle_dump/*.png"]
OUT_DIR = os.path.dirname(os.path.abspath(__file__))


# ── Model ───────────────────────────────────────────────────────────────────────

class Eyes(nn.Module):
    def __init__(self, feat=128, units=96):
        super().__init__()
        self.enc = nn.Sequential(
            nn.Conv2d(2, 8, 5, stride=2, padding=2), nn.ReLU(),
            nn.Conv2d(8, 16, 5, stride=2, padding=2), nn.ReLU(),
            nn.Conv2d(16, 32, 3, stride=2, padding=1), nn.ReLU(),
            nn.AdaptiveAvgPool2d((4, 8)), nn.Flatten(),
            nn.Linear(32 * 4 * 8, feat), nn.ReLU(),
        )
        # CfCCell driven by our own unroll: ncps's CfC wrapper squeezes timespans to
        # [B], which cannot broadcast against [B, units] — a batch>1 library bug.
        # Passing ts as [B,1] to the cell directly is correct and batch-safe.
        self.cell = CfCCell(feat, units)
        self.units = units
        self.kind = nn.Linear(units, len(KINDS))
        self.bbox = nn.Linear(units, 4)
        self.dir = nn.Linear(units, len(DIRS))
        self.mag = nn.Linear(units, 1)

    def forward(self, frames, dts):
        # frames [B,T,H,W] float in [0,1]; dts [B,T]
        B, T_, H_, W_ = frames.shape
        prev = torch.cat([frames[:, :1], frames[:, :-1]], dim=1)
        x = torch.stack([frames, (frames - prev).abs()], dim=2)   # [B,T,2,H,W]
        f = self.enc(x.reshape(B * T_, 2, H_, W_)).reshape(B, T_, -1)
        h = torch.zeros(B, self.units, device=frames.device)
        for t in range(T_):
            _, h = self.cell(f[:, t], h, dts[:, t:t + 1])
        z = h
        return (self.kind(z), torch.sigmoid(self.bbox(z)),
                self.dir(z), torch.sigmoid(self.mag(z)).squeeze(-1))


def encode_labels(labels):
    k = torch.tensor([KINDS.index(l["kind"]) for l in labels])
    b = torch.tensor([l["bbox"] for l in labels], dtype=torch.float32)
    d = torch.tensor([DIRS.index(l["dir"]) for l in labels])
    m = torch.tensor([min(1.0, l["mag"]) for l in labels], dtype=torch.float32)
    return k, b, d, m


# ── Metrics ─────────────────────────────────────────────────────────────────────

def iou(a, b):
    ax0, ay0 = a[0] - a[2] / 2, a[1] - a[3] / 2
    ax1, ay1 = a[0] + a[2] / 2, a[1] + a[3] / 2
    bx0, by0 = b[0] - b[2] / 2, b[1] - b[3] / 2
    bx1, by1 = b[0] + b[2] / 2, b[1] + b[3] / 2
    ix = max(0.0, min(ax1, bx1) - max(ax0, bx0))
    iy = max(0.0, min(ay1, by1) - max(ay0, by0))
    inter = ix * iy
    ua = a[2] * a[3] + b[2] * b[3] - inter
    return inter / ua if ua > 0 else 0.0


def score(preds, labels):
    """preds/labels: lists of label dicts → metric dict."""
    per_kind = {k: {"n": 0, "hit": 0} for k in KINDS}
    ious, dir_hits, dir_n, mag_err = [], 0, 0, []
    for p, l in zip(preds, labels):
        per_kind[l["kind"]]["n"] += 1
        if p["kind"] == l["kind"]:
            per_kind[l["kind"]]["hit"] += 1
        if l["kind"] != "none":
            ious.append(iou(p["bbox"], l["bbox"]))
            mag_err.append(abs(min(1.0, p["mag"]) - min(1.0, l["mag"])))
        if l["kind"] == "scroll":
            dir_n += 1
            dir_hits += int(p["dir"] == l["dir"])
    acc = sum(v["hit"] for v in per_kind.values()) / max(1, len(labels))
    return {
        "kind_acc": round(acc, 4),
        "per_kind": {k: (round(v["hit"] / v["n"], 3) if v["n"] else None) for k, v in per_kind.items()},
        "bbox_iou": round(float(np.mean(ious)), 4) if ious else None,
        "dir_acc": round(dir_hits / dir_n, 4) if dir_n else None,
        "mag_mae": round(float(np.mean(mag_err)), 4) if mag_err else None,
    }


def model_predict(model, frames, dts, bs=64):
    model.eval()
    out = []
    with torch.no_grad():
        for i in range(0, len(frames), bs):
            fb = torch.tensor(frames[i:i + bs], dtype=torch.float32) / 255.0
            db = torch.tensor(dts[i:i + bs], dtype=torch.float32)
            k, b, d, m = model(fb, db)
            for j in range(len(fb)):
                out.append({
                    "kind": KINDS[int(k[j].argmax())],
                    "bbox": tuple(float(v) for v in b[j]),
                    "dir": DIRS[int(d[j].argmax())],
                    "mag": float(m[j]),
                })
    return out


# ── Main ────────────────────────────────────────────────────────────────────────

def main():
    quick = "--quick" in sys.argv
    rng = np.random.default_rng(SEED)
    torch.manual_seed(SEED)

    # bases: synthetic apps + real frames split train/eval by FRAME (unseen pixels)
    real_paths = sorted(p for g in REAL_GLOBS for p in glob.glob(g))
    real = synth.load_real_bases(real_paths)
    rng.shuffle(real)
    real_train, real_eval = real[: len(real) // 2], real[len(real) // 2:]
    synth_bases = [synth.gen_ui_base(rng) for _ in range(40 if not quick else 10)]
    train_bases = synth_bases + real_train
    print(f"bases: {len(synth_bases)} synthetic, {len(real_train)} real-train, {len(real_eval)} real-eval")

    n_train = 3000 if not quick else 300
    n_eval = 600 if not quick else 120
    t0 = time.time()
    trF, trD, trL = synth.make_dataset(n_train, train_bases, rng)
    evF, evD, evL = synth.make_dataset(n_eval, real_eval, rng)
    print(f"dataset: {n_train} train / {n_eval} eval windows in {time.time()-t0:.1f}s")

    # ── baseline on eval (clockless deterministic null) ──
    t0 = time.time()
    base_preds = [baseline.characterize(evF[i]) for i in range(len(evF))]
    base_m = score(base_preds, evL)
    print(f"BASELINE ({time.time()-t0:.1f}s): {json.dumps(base_m)}")

    # ── train ──
    model = Eyes()
    n_params = sum(p.numel() for p in model.parameters())
    opt = torch.optim.Adam(model.parameters(), lr=2e-3)
    ce = nn.CrossEntropyLoss()
    l1 = nn.L1Loss(reduction="none")
    kk, bb, dd, mm = encode_labels(trL)
    epochs = 8 if not quick else 2
    bs = 32
    for ep in range(epochs):
        model.train()
        perm = torch.randperm(n_train)
        tot, nb = 0.0, 0
        for i in range(0, n_train, bs):
            idx = perm[i:i + bs]
            fb = torch.tensor(trF[idx.numpy()], dtype=torch.float32) / 255.0
            db = torch.tensor(trD[idx.numpy()], dtype=torch.float32)
            k, b, d, m = model(fb, db)
            not_none = (kk[idx] != KINDS.index("none")).float().unsqueeze(-1)
            loss = (ce(k, kk[idx])
                    + ce(d, dd[idx])
                    + 4.0 * (l1(b, bb[idx]) * not_none).mean()
                    + (l1(m, mm[idx]) * not_none.squeeze(-1)).mean())
            opt.zero_grad(); loss.backward(); opt.step()
            tot += loss.item(); nb += 1
        ev_preds = model_predict(model, evF, evD)
        ev_m = score(ev_preds, evL)
        print(f"epoch {ep+1}/{epochs}: loss {tot/nb:.3f}  eval kind_acc {ev_m['kind_acc']}  iou {ev_m['bbox_iou']}")

    model_m = score(model_predict(model, evF, evD), evL)

    # ── gate ──
    beats = {
        "kind_acc": model_m["kind_acc"] > base_m["kind_acc"],
        "bbox_iou": (model_m["bbox_iou"] or 0) > (base_m["bbox_iou"] or 0),
        "dir_acc": (model_m["dir_acc"] or 0) >= (base_m["dir_acc"] or 0),
        "mag_mae": (model_m["mag_mae"] or 1) < (base_m["mag_mae"] or 1),
    }
    verdict = "PROMOTE" if beats["kind_acc"] and beats["bbox_iou"] else "HOLD(baseline-null)"
    report = {
        "date": time.strftime("%Y-%m-%d %H:%M"),
        "quick": quick,
        "params": n_params,
        "train_windows": n_train, "eval_windows": n_eval,
        "eval_bases": "real VM frames unseen in training",
        "baseline": base_m, "model": model_m, "beats": beats, "verdict": verdict,
    }
    out = os.path.join(OUT_DIR, "eyes_report.json")
    json.dump(report, open(out, "w"), indent=2)
    torch.save(model.state_dict(), os.path.join(OUT_DIR, "eyes_v0.pt"))
    print(f"\nMODEL: {json.dumps(model_m)}")
    print(f"VERDICT: {verdict}  (params {n_params:,})")
    print(f"report -> {out}")


if __name__ == "__main__":
    main()
