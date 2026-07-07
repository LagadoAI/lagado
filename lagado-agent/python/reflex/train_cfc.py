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
N_PIX = 49            # feature layout: [0:49]=pixel grid, 49=win_changed, 50+=aux


def load_episodes(data_dir):
    eps = []
    for p in sorted(glob.glob(os.path.join(data_dir, "ep*.npz"))):
        z = np.load(p, allow_pickle=True)
        eps.append({"t": z["t"], "feats": z["feats"], "t_stim": float(z["t_stim"]),
                    "t_done": float(z["t_stim_done"]) if "t_stim_done" in z else -1.0,
                    "name": str(z["name"]), "rnd": int(z["rnd"]), "path": p})
    return eps


def calibrate_eps(train_eps):
    """Noise floor from stimulus-free episodes (quiet + blink), skipping frame 0."""
    vals = np.concatenate([e["feats"][1:, N_PIX - 1] for e in train_eps
                           if e["name"] in ("quiet", "blink_idle")])
    return max(float(np.quantile(vals, 0.99)) * 1.5, 1e-4)


def busy_signal(ep, eps):
    """FUSED per-frame busy flag: pixel change above noise floor OR window-list
    changed. The monitor's inputs and its truth use the same fused senses."""
    px = ep["feats"][:, N_PIX - 1] > eps
    if ep["feats"].shape[1] > N_PIX:
        return px | (ep["feats"][:, N_PIX] > 0.5)
    return px


GAP_MAX = 1.5   # capture-blind interval: cannot certify quiet through it


def label_episode(ep, eps):
    """Per-frame oracle settled labels + valid mask.

    A frame is unlabeled if its future window is missing OR contains a capture
    gap > GAP_MAX (blind interval: the world may have churned unseen)."""
    t = ep["t"]
    busy = busy_signal(ep, eps)
    n = len(t)
    settled = np.zeros(n, dtype=np.float32)
    valid = np.zeros(n, dtype=bool)
    gap_after = np.append(np.diff(t) > GAP_MAX, False)
    for i in range(n):
        fut = (t > t[i]) & (t <= t[i] + W_FUTURE)
        if t[i] + W_FUTURE > t[-1]:
            continue                      # no future window -> unlabeled
        span = (t >= t[i]) & (t <= t[i] + W_FUTURE)
        if gap_after[span].any():
            continue                      # blind gap inside window -> unlabeled
        valid[i] = True
        settled[i] = 1.0 if not busy[fut].any() else 0.0
    # TEACHING ORACLE (visible-consequence session verbs only): the daemon call's
    # [fired, returned] interval is app-truth "world busy" — overrides hindsight,
    # even where the senses saw nothing. Headless verbs (uno_open/uno_close) are
    # deliberate negatives: internally busy, visibly settled — hindsight stands.
    if ep["name"] == "uno_reload" and ep["t_done"] > 0:
        in_oracle = (t >= ep["t_stim"]) & (t <= ep["t_done"])
        settled[in_oracle] = 0.0
        valid[in_oracle] = True
    valid[0] = False                      # first frame is the all-ones artifact
    return settled, valid


def eval_start(ep):
    return ep["t_stim"] if ep["t_stim"] > 0 else 1.0


def run_baseline(ep, eps):
    """First fire of the K-consecutive-quiet rule on the FUSED senses (production
    parity: pixel delta OR window churn), from eval start."""
    t = ep["t"]
    busy = busy_signal(ep, eps)
    start = eval_start(ep)
    streak = 0
    for i in range(len(t)):
        if t[i] < start:
            continue
        streak = 0 if busy[i] else streak + 1
        if streak >= K_BASELINE:
            return t[i]
    return None


def run_timer(ep, c):
    """The TIMER NULL: fire at stimulus + a constant. This is the shortcut the 2026-07-06
    brutal suite caught the promoted v1 CfC having learned (all episodes fired their stimulus
    at the same time and settled on schedule, so elapsed-time predicted the hindsight label
    perfectly and the gate could not tell a clock from a pixel-reader). Any candidate must
    now BEAT the best constant timer — a monitor that cannot has learned nothing but time."""
    return eval_start(ep) + c


def pick_timer(train_eps, eps):
    """Train-side selection of the best constant: zero-FS configs first, then fewest
    misses, then lowest latency — the same lexicographic rule the CfC gets."""
    best = None
    for c in np.arange(0.25, 10.01, 0.25):
        fs = miss = 0
        lats = []
        for ep in train_eps:
            f, lat, m = score_fire(ep, eps, run_timer(ep, float(c)))
            fs += f
            miss += m
            if lat is not None:
                lats.append(lat)
        cand = (fs, miss, float(np.mean(lats)) if lats else 99.0, float(c))
        if best is None or cand < best:
            best = cand
    print("  timer-null selected: c=%.2fs (train FS=%d miss=%d)" % (best[3], best[0], best[1]))
    return best[3]


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
    # pixel changed-fractions span ~5 orders of magnitude -> log-scale to [0,1];
    # the non-pixel channels (win_changed, counts) are already O(1): pass through.
    f = ep["feats"].astype(np.float64)
    f[:, :N_PIX] = (np.log10(f[:, :N_PIX] + 1e-6) + 6.0) / 6.0
    x = torch.tensor(f, dtype=torch.float32)
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


K_PATIENCE = 1   # set per-fold by pick_threshold (train-side joint selection over
                 # threshold x patience; single-tick K=2 alone was measured WORSE:
                 # FS unchanged, misses 2->6 — the errors are sustained, not blips)


def cfc_fire_time(model, ep, thresh):
    x, dt = to_tensors(ep)
    with torch.no_grad():
        p = torch.sigmoid(model(x, timespans=dt)[0].squeeze(-1).squeeze(0)).numpy()
    start = eval_start(ep)
    streak = 0
    for i in range(1, len(p)):
        if ep["t"][i] < start:
            continue
        streak = streak + 1 if p[i] > thresh else 0
        if streak >= K_PATIENCE:
            return float(ep["t"][i])
    return None


def pick_threshold(model, train_eps, eps):
    """Joint train-side selection of (threshold, patience): among configs with ZERO
    train false-settles, take the one with fewest misses, then lowest latency."""
    global K_PATIENCE
    best = None
    for k in (1, 2, 3):
        K_PATIENCE = k
        for thresh in np.arange(0.50, 0.96, 0.05):
            fs = miss = 0
            lats = []
            for ep in train_eps:
                f, lat, m = score_fire(ep, eps, cfc_fire_time(model, ep, thresh))
                fs += f
                miss += m
                if lat is not None:
                    lats.append(lat)
            if fs == 0:
                cand = (miss, float(np.mean(lats)) if lats else 99.0, k, float(thresh))
                if best is None or cand < best:
                    best = cand
    if best is None:
        K_PATIENCE = 3
        return 0.95
    K_PATIENCE = best[2]
    print("  train-selected: K=%d thresh=%.2f (train miss=%d)" % (best[2], best[3], best[0]))
    return best[3]


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
    tc = pick_timer(train_eps, eps)
    timer = evaluate("TIMER", lambda ep: run_timer(ep, tc), test_eps, eps)
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
            "us_per_tick": round(us, 1), "baseline": base, "timer": timer,
            "timer_c": tc, "cfc": cfc, "heldout_round": heldout_rnd}


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
           "timer": {"false_settle": tot("false_settle", "timer"),
                     "miss": tot("miss", "timer"), "mean_latency_s": lat("timer")},
           "cfc": {"false_settle": tot("false_settle", "cfc"),
                   "miss": tot("miss", "cfc"), "mean_latency_s": lat("cfc")},
           "params": reports[0]["params"],
           "us_per_tick": reports[0]["us_per_tick"], "reports": reports}
    b, c, tm = agg["baseline"], agg["cfc"], agg["timer"]
    beats_floor = (c["false_settle"] <= b["false_settle"] and c["miss"] <= b["miss"]
                   and c["mean_latency_s"] is not None and b["mean_latency_s"] is not None
                   and c["mean_latency_s"] < b["mean_latency_s"])
    # TIMER NULL (mandatory since the 2026-07-06 shortcut finding): a monitor that cannot
    # beat the best constant clock IS that clock — it has read nothing from its senses.
    beats_timer = (c["false_settle"] <= tm["false_settle"] and c["miss"] <= tm["miss"]
                   and c["mean_latency_s"] is not None and tm["mean_latency_s"] is not None
                   and c["mean_latency_s"] < tm["mean_latency_s"])
    agg["verdict"] = "PROMOTE" if (beats_floor and beats_timer) else \
        ("HOLD(timer-null)" if beats_floor else "HOLD")
    out = os.path.join(data_dir, "gate_report.json")
    json.dump(agg, open(out, "w"), indent=1)
    print("\n===== AGGREGATE (%d folds, %d episodes) =====" % (agg["folds"], n_ep))
    print("BASELINE  FS=%d miss=%d latency=%s" % (b["false_settle"], b["miss"], b["mean_latency_s"]))
    print("TIMER     FS=%d miss=%d latency=%s" % (tm["false_settle"], tm["miss"], tm["mean_latency_s"]))
    print("CFC       FS=%d miss=%d latency=%s" % (c["false_settle"], c["miss"], c["mean_latency_s"]))
    print("VERDICT: %s  -> %s" % (agg["verdict"], out), flush=True)


if __name__ == "__main__":
    main()
