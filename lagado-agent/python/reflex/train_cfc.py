"""Train + gate-evaluate the settle-reflex CfC (reflex bank expert #1).

Oracle label (retrospective, computable only offline): frame t is SETTLED iff no
frame in (t, t+W] has whole-frame changed-fraction > EPS. EPS is calibrated from
the train-split quiet/blink episodes (the production noise-floor idea, measured
not hand-picked). The last W seconds of each episode are unlabeled (no future).

Two detectors are compared on held-out episodes, same features, same EPS:
  BASELINE  the production-shaped rule: K consecutive frames under EPS.
  CFC       ncps CfC (dense, 48 units, proj 1) fed the 49-dim feature stream with
            real inter-frame dt as `timespans`; fires when sigmoid > threshold,
            threshold chosen on TRAIN for zero false-settles (fail-closed bias).

PROMOTION GATE (R1a rule): promote only if CFC false-settle rate <= baseline AND
mean detection latency < baseline on held-out. Otherwise the deterministic floor
stands and this run is recorded as a miss.

Run:  reflex/.venv/bin/python train_cfc.py <data_dir> [heldout_round]
"""
import glob
import json
import os
import sys
import time

import numpy as np
import torch
from ncps.torch import CfC

W_FUTURE = 2.0        # settle horizon (s)
K_BASELINE = 3        # production-shaped: N stable observations
SEED = 7


def load_episodes(data_dir):
    eps = []
    for p in sorted(glob.glob(os.path.join(data_dir, "ep*.npz"))):
        z = np.load(p, allow_pickle=True)
        eps.append({"t": z["t"], "feats": z["feats"], "t_stim": float(z["t_stim"]),
                    "name": str(z["name"]), "rnd": int(z["rnd"]), "path": p})
    return eps


def calibrate_eps(train_eps):
    """Noise floor from stimulus-free episodes (quiet + blink), skipping frame 0."""
    vals = np.concatenate([e["feats"][1:, -1] for e in train_eps
                           if e["name"] in ("quiet", "blink_idle")])
    return max(float(np.quantile(vals, 0.99)) * 1.5, 1e-4)


def label_episode(ep, eps):
    """Per-frame oracle settled labels + valid mask (future window exists)."""
    t, total = ep["t"], ep["feats"][:, -1]
    n = len(t)
    settled = np.zeros(n, dtype=np.float32)
    valid = np.zeros(n, dtype=bool)
    for i in range(n):
        fut = (t > t[i]) & (t <= t[i] + W_FUTURE)
        if t[i] + W_FUTURE > t[-1]:
            continue                      # no future window -> unlabeled
        valid[i] = True
        settled[i] = 1.0 if not (total[fut] > eps).any() else 0.0
    valid[0] = False                      # first frame is the all-ones artifact
    return settled, valid


def eval_start(ep):
    return ep["t_stim"] if ep["t_stim"] > 0 else 1.0


def run_baseline(ep, eps):
    """First fire time of the K-consecutive-quiet rule, from eval start."""
    t, total = ep["t"], ep["feats"][:, -1]
    start = eval_start(ep)
    streak = 0
    for i in range(len(t)):
        if t[i] < start:
            continue
        streak = streak + 1 if total[i] <= eps else 0
        if streak >= K_BASELINE:
            return t[i]
    return None


def episode_truth(ep, eps):
    """First oracle settle time at/after eval start (None if never settles)."""
    settled, valid = label_episode(ep, eps)
    start = eval_start(ep)
    idx = np.where(valid & (settled > 0.5) & (ep["t"] >= start))[0]
    return float(ep["t"][idx[0]]) if len(idx) else None


def score_fire(ep, eps, t_fire):
    """-> (false_settle, latency|None, miss)."""
    t_true = episode_truth(ep, eps)
    if t_fire is None:
        return (False, None, t_true is not None)
    if t_true is None or t_fire < t_true - 1e-9:
        return (True, None, False)
    return (False, t_fire - t_true, False)


def to_tensors(ep):
    # changed-fractions span ~5 orders of magnitude (noise floor 1e-4 .. 1.0);
    # raw values are all ~0 to a randomly-initialized net. Log-scale to [0,1].
    f = np.log10(ep["feats"] + 1e-6)          # [-6, 0]
    x = torch.tensor((f + 6.0) / 6.0, dtype=torch.float32)
    dt = np.diff(ep["t"], prepend=ep["t"][0])
    dt[0] = np.median(dt[1:]) if len(dt) > 1 else 0.2
    return x.unsqueeze(0), torch.tensor(dt, dtype=torch.float32).unsqueeze(0)


def train(train_eps, eps):
    torch.manual_seed(SEED)
    np.random.seed(SEED)
    model = CfC(input_size=train_eps[0]["feats"].shape[1], units=48, proj_size=1,
                batch_first=True, return_sequences=True)
    opt = torch.optim.Adam(model.parameters(), lr=3e-3)
    data = []
    for ep in train_eps:
        y, valid = label_episode(ep, eps)
        x, dt = to_tensors(ep)
        data.append((x, dt, torch.tensor(y).unsqueeze(0),
                     torch.tensor(valid).unsqueeze(0)))
    for epoch in range(150):
        total_loss = 0.0
        for x, dt, y, valid in data:
            opt.zero_grad()
            out = model(x, timespans=dt)[0].squeeze(-1)
            # false-settle (pred 1 on label 0) is the dangerous error: weight it 4x
            w = torch.where(y < 0.5, 4.0, 1.0) * valid
            loss = (torch.nn.functional.binary_cross_entropy_with_logits(
                out, y, reduction="none") * w).sum() / w.sum().clamp(min=1)
            loss.backward()
            opt.step()
            total_loss += float(loss)
        if epoch % 25 == 0:
            print("epoch %3d loss %.4f" % (epoch, total_loss / len(data)), flush=True)
    return model


def cfc_fire_time(model, ep, thresh):
    x, dt = to_tensors(ep)
    with torch.no_grad():
        p = torch.sigmoid(model(x, timespans=dt)[0].squeeze(-1).squeeze(0)).numpy()
    start = eval_start(ep)
    for i in range(1, len(p)):
        if ep["t"][i] >= start and p[i] > thresh:
            return float(ep["t"][i])
    return None


def pick_threshold(model, train_eps, eps):
    """Highest-recall threshold with ZERO train false-settles (fail-closed)."""
    for thresh in np.arange(0.50, 0.96, 0.05):
        fs = sum(score_fire(ep, eps, cfc_fire_time(model, ep, thresh))[0]
                 for ep in train_eps)
        if fs == 0:
            return float(thresh)
    return 0.95


def evaluate(name, fire_fn, eval_eps, eps):
    fs = miss = 0
    lats = []
    for ep in eval_eps:
        f, lat, m = score_fire(ep, eps, fire_fn(ep))
        fs += f
        miss += m
        if lat is not None:
            lats.append(lat)
    r = {"detector": name, "episodes": len(eval_eps), "false_settle": fs,
         "miss": miss, "mean_latency_s": round(float(np.mean(lats)), 3) if lats else None,
         "fires_scored": len(lats)}
    print("%-8s  FS=%d  miss=%d  latency=%s  (n=%d)" %
          (name, fs, miss, r["mean_latency_s"], len(eval_eps)), flush=True)
    return r


def run_fold(eps_all, data_dir, heldout_rnd):
    train_eps = [e for e in eps_all if e["rnd"] != heldout_rnd]
    test_eps = [e for e in eps_all if e["rnd"] == heldout_rnd]
    print("episodes: train=%d heldout=%d (round %d held out)" %
          (len(train_eps), len(test_eps), heldout_rnd), flush=True)
    eps = calibrate_eps(train_eps)
    print("calibrated EPS (noise floor) = %.5f" % eps, flush=True)

    model = train(train_eps, eps)
    n_params = sum(p.numel() for p in model.parameters())
    thresh = pick_threshold(model, train_eps, eps)
    print("params=%d  fail-closed threshold=%.2f" % (n_params, thresh), flush=True)

    base = evaluate("BASELINE", lambda ep: run_baseline(ep, eps), test_eps, eps)
    cfc = evaluate("CFC", lambda ep: cfc_fire_time(model, ep, thresh), test_eps, eps)

    # per-tick CPU cost (stateful single-step, the production shape)
    x1 = torch.zeros(1, 1, test_eps[0]["feats"].shape[1] if test_eps else 49)
    dt1 = torch.full((1, 1), 0.2)
    with torch.no_grad():
        _, hx = model(x1, timespans=dt1)
        t0 = time.perf_counter()
        for _ in range(1000):
            _, hx = model(x1, hx=hx, timespans=dt1)
        us = (time.perf_counter() - t0) * 1e3  # ms for 1000 -> us per tick

    torch.save(model.state_dict(),
               os.path.join(data_dir, "settle_cfc_r%d.pt" % heldout_rnd))
    return {"eps": eps, "params": n_params, "threshold": thresh,
            "us_per_tick": round(us, 1), "baseline": base, "cfc": cfc,
            "heldout_round": heldout_rnd}


def main():
    data_dir = sys.argv[1] if len(sys.argv) > 1 else "/tmp/lagado_battery/reflex_data"
    mode = sys.argv[2] if len(sys.argv) > 2 else "3"
    eps_all = load_episodes(data_dir)
    rounds = sorted({e["rnd"] for e in eps_all})
    folds = rounds if mode == "cv" else [int(mode)]

    reports = []
    for rnd in folds:
        print("\n===== FOLD: hold out round %d =====" % rnd, flush=True)
        reports.append(run_fold(eps_all, data_dir, rnd))

    def tot(key, det):
        return sum(r[det][key] for r in reports)

    def lat(det):
        ls = [r[det]["mean_latency_s"] for r in reports
              if r[det]["mean_latency_s"] is not None]
        return round(float(np.mean(ls)), 3) if ls else None

    n_ep = sum(r["baseline"]["episodes"] for r in reports)
    agg = {"folds": len(reports), "episodes": n_ep,
           "baseline": {"false_settle": tot("false_settle", "baseline"),
                        "miss": tot("miss", "baseline"), "mean_latency_s": lat("baseline")},
           "cfc": {"false_settle": tot("false_settle", "cfc"),
                   "miss": tot("miss", "cfc"), "mean_latency_s": lat("cfc")},
           "params": reports[0]["params"],
           "us_per_tick": reports[0]["us_per_tick"], "reports": reports}
    b, c = agg["baseline"], agg["cfc"]
    promote = (c["false_settle"] <= b["false_settle"] and c["miss"] <= b["miss"]
               and c["mean_latency_s"] is not None and b["mean_latency_s"] is not None
               and c["mean_latency_s"] < b["mean_latency_s"])
    agg["verdict"] = "PROMOTE" if promote else "HOLD"
    out = os.path.join(data_dir, "gate_report.json")
    json.dump(agg, open(out, "w"), indent=1)
    print("\n===== AGGREGATE (%d folds, %d episodes) =====" % (agg["folds"], n_ep))
    print("BASELINE  FS=%d miss=%d latency=%s" % (b["false_settle"], b["miss"], b["mean_latency_s"]))
    print("CFC       FS=%d miss=%d latency=%s" % (c["false_settle"], c["miss"], c["mean_latency_s"]))
    print("VERDICT: %s  -> %s" % (agg["verdict"], out), flush=True)


if __name__ == "__main__":
    main()
