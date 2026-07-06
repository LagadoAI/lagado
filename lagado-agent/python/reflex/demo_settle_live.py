"""End-to-end proof of the settle-monitor SERVING path (client -> service -> CfC).

Replays every recorded episode in a data dir through SettleMonitor exactly as a
live caller would (reset, then per-frame tick(feats, dt)), and scores the
resulting fire times with train_cfc's own oracle (label_episode / score_fire —
the same machinery that produced the PROMOTE gate report). Also cross-checks the
service's stepwise-hx probabilities against an offline full-sequence forward pass
(they must match to float32 noise), so a gate-parity result here certifies the
service, not just the weights.

Fire rule = the promoted operating point (settle_service.THRESHOLD=0.50,
K_PATIENCE=3), streak counted from the episode's eval start (t_stim, or 1.0 s on
stimulus-free episodes) — cfc_fire_time's convention. NOTE: episodes here include
the promoted model's training rounds, so this is a SERVING-PARITY claim, not a
fresh held-out claim (that is the gate report's job).

Run:  reflex/.venv/bin/python demo_settle_live.py [data_dir]
      (default data_dir = /tmp/lagado_battery/reflex_combined)
"""
import os
import sys
import time

import numpy as np
import torch

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import settle_service
import train_cfc
from settle_client import SettleMonitor

THRESH = settle_service.THRESHOLD
K = settle_service.K_PATIENCE


def episode_dts(ep):
    dt = np.diff(ep["t"], prepend=ep["t"][0])
    dt[0] = np.median(dt[1:]) if len(dt) > 1 else 0.2
    return dt


def service_fire_time(mon, ep):
    """Replay one episode through the service; return (fire_t, p_stream, tick_ms)."""
    if mon.reset() is None:
        raise RuntimeError("monitor dead")
    dts = episode_dts(ep)
    start = train_cfc.eval_start(ep)
    ps, streak, fire, lat = [], 0, None, []
    for i in range(len(ep["t"])):
        t0 = time.monotonic()
        p, _settled = mon.tick(ep["feats"][i], dts[i])
        lat.append((time.monotonic() - t0) * 1000)
        if p is None:
            raise RuntimeError("monitor died mid-episode (fail-open)")
        ps.append(p)
        # cfc_fire_time's convention: skip frame 0, count the streak from eval start
        if i == 0 or ep["t"][i] < start:
            continue
        streak = streak + 1 if p > THRESH else 0
        if streak >= K and fire is None:
            fire = float(ep["t"][i])
    return fire, np.array(ps), float(np.mean(lat))


def offline_probs(model, ep):
    x, dt = train_cfc.to_tensors(ep)
    with torch.no_grad():
        return torch.sigmoid(model(x, timespans=dt)[0].squeeze(-1).squeeze(0)).numpy()


def main():
    data_dir = sys.argv[1] if len(sys.argv) > 1 else "/tmp/lagado_battery/reflex_combined"
    eps_all = train_cfc.load_episodes(data_dir)
    if not eps_all:
        raise SystemExit("no episodes in %s" % data_dir)
    eps_noise = train_cfc.calibrate_eps(eps_all)   # noise floor over all rounds
    model = settle_service.load_model()            # offline twin for the parity check

    mon = SettleMonitor()
    if mon.dead:
        raise SystemExit("could not start settle service (fail-open would engage in prod)")

    print("episodes=%d  data=%s  noise_eps=%.5f  op-point: thresh=%.2f K=%d"
          % (len(eps_all), data_dir, eps_noise, THRESH, K))
    print("%-16s %-3s %7s %7s %9s %7s  %s" %
          ("name", "rnd", "t_stim", "fire", "truth", "lat_s", "verdict"))
    fs = miss = 0
    lats, parity, tick_ms = [], [], []
    for ep in eps_all:
        fire, ps, ms = service_fire_time(mon, ep)
        tick_ms.append(ms)
        parity.append(float(np.max(np.abs(ps - offline_probs(model, ep)))))
        false_settle, lat, missed = train_cfc.score_fire(ep, eps_noise, fire)
        truth = train_cfc.episode_truth(ep, eps_noise)
        fs += false_settle
        miss += missed
        if lat is not None:
            lats.append(lat)
        verdict = ("FALSE-SETTLE" if false_settle else
                   "MISS" if missed else "ok(+%.2fs)" % lat)
        print("%-16s %-3d %7.2f %7s %9s %7s  %s" %
              (ep["name"], ep["rnd"], ep["t_stim"],
               "-" if fire is None else "%.2f" % fire,
               "-" if truth is None else "%.2f" % truth,
               "-" if lat is None else "%.2f" % lat, verdict))
    mon.close()

    print("\nSERVING-PATH REPLAY: episodes=%d  false_settle=%d  miss=%d  "
          "mean_latency=%.3fs" % (len(eps_all), fs, miss,
                                  float(np.mean(lats)) if lats else float("nan")))
    print("service-vs-offline parity: max|dp|=%.2e over %d probs   "
          "mean tick=%.3fms" % (max(parity), sum(len(e["t"]) for e in eps_all),
                                float(np.mean(tick_ms))))
    print("gate report (held-out CV) for reference: CFC FS=0 miss=0 latency=1.978s")
    return 0 if (fs == 0 and miss == 0) else 1


if __name__ == "__main__":
    sys.exit(main())
