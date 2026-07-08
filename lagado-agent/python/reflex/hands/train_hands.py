"""hands/train_hands.py — closed-loop cursor CfC, trained through the dynamics,
gated against per-condition-TUNED classical controllers.

Seat the CfC the way the drone lineage does: small input (delayed error/velocity,
prev action, dt), persistent hidden state across the episode, continuous-time cell,
output = velocity command. Trained by BPTT THROUGH the differentiable environment —
no teacher, so nothing caps it below the dynamics' optimum.

THE GATE (timer-null discipline, control edition): per evaluation regime, P and PD
controllers are GRID-TUNED on that regime (the strongest classical opponent), then
one fixed CfC weight-set must beat the retuned classical family where dynamics are
messy without losing the clean regime. Extrapolation rows use dynamics outside the
training ranges — labeled as such.

Run: .venv/bin/python hands/train_hands.py [--quick]
"""
import json
import os
import sys
import time

import numpy as np
import torch
import torch.nn as nn
from ncps.torch import CfCCell

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from hands.env import (ReachEnv, Conditions, episode_success,
                       OBS_DIM, U_MAX, CLICK_RADIUS)

SEED = 5
OUT_DIR = os.path.dirname(os.path.abspath(__file__))
T_STEPS = 140          # ≈ 2-4s episodes at 10-35ms steps


# ── policy ──────────────────────────────────────────────────────────────────────

class HandsCfC(nn.Module):
    def __init__(self, units=64):
        super().__init__()
        self.inp = nn.Linear(OBS_DIM, 32)
        self.cell = CfCCell(32, units)
        self.out = nn.Linear(units, 2)
        self.units = units

    def scale_obs(self, obs):
        s = obs.clone()
        s[:, 2:4] = s[:, 2:4] / U_MAX     # velocity → ~[-1,1]
        s[:, 6] = s[:, 6] * 30.0          # dt → ~[0.3,1]
        return s

    def forward(self, obs, h, dt):
        x = torch.tanh(self.inp(self.scale_obs(obs)))
        _, h = self.cell(x, h, dt.unsqueeze(1))
        return torch.tanh(self.out(h)), h


def rollout_policy(model, cond, batch, grad=True):
    env = ReachEnv(batch, cond)
    obs = env.reset()
    h = torch.zeros(batch, model.units)
    dists, jerk = [], 0.0
    prev_u = torch.zeros(batch, 2)
    ctx = torch.enable_grad() if grad else torch.no_grad()
    with ctx:
        dt = torch.full((batch,), 0.02)
        for _ in range(T_STEPS):
            u, h = model(obs, h, dt)
            obs, dt = env.step(u)
            dists.append(torch.linalg.norm(env.g - env.p, dim=1))
            jerk = jerk + ((u - prev_u) ** 2).mean()
            prev_u = u
    return torch.stack(dists), jerk


# ── classical opponents (see the SAME delayed observations) ─────────────────────

def rollout_classical(kp, kd, cond, batch):
    env = ReachEnv(batch, cond)
    obs = env.reset()
    dists = []
    with torch.no_grad():
        for _ in range(T_STEPS):
            e, v = obs[:, 0:2], obs[:, 2:4]
            u = torch.clamp(kp * e - kd * v / U_MAX, -1.0, 1.0)
            obs, _ = env.step(u)
            dists.append(torch.linalg.norm(env.g - env.p, dim=1))
    return torch.stack(dists)


def tune_classical(make_cond, batch):
    """Grid-tune P and PD ON THIS REGIME (tuning seeds ≠ eval seeds). Returns the
    best (kp, kd) by success rate, ties broken by median click time."""
    best = None
    for kp in [1.5, 3, 5, 8, 12, 18]:
        for kd in [0.0, 0.5, 1.0, 2.0, 4.0, 8.0]:
            d = rollout_classical(kp, kd, make_cond(seed_shift=900), batch)
            ok, t_click = episode_success(d)
            key = (ok.float().mean().item(), -t_click.float().median().item())
            if best is None or key > best[0]:
                best = (key, kp, kd)
    return best[1], best[2]


# ── evaluation regimes ───────────────────────────────────────────────────────────

def regime(name, seed, **kw):
    def make(seed_shift=0):
        rng = np.random.default_rng(seed + seed_shift)
        return Conditions(kw.pop("batch", 512) if False else 512, rng, **kw)
    return name, make

REGIMES = [
    ("clean",            dict(momentum=(0.40, 0.50), obs_delay=(0, 1), jump_prob=0.0, drift_speed=(0.0, 0.0))),
    ("inertia (extrap)", dict(momentum=(0.91, 0.96), obs_delay=(0, 1), jump_prob=0.0, drift_speed=(0.0, 0.0))),
    ("lag (in-range)",   dict(momentum=(0.60, 0.70), obs_delay=(5, 8), jump_prob=0.0, drift_speed=(0.0, 0.0))),
    ("lag (extrap)",     dict(momentum=(0.60, 0.70), obs_delay=(9, 12), jump_prob=0.0, drift_speed=(0.0, 0.0))),
    ("moving target",    dict(momentum=(0.55, 0.70), obs_delay=(1, 3), jump_prob=0.02, drift_speed=(0.15, 0.25))),
    ("messy (combined)", dict(momentum=(0.80, 0.90), obs_delay=(4, 8), jump_prob=0.01, drift_speed=(0.10, 0.20))),
]


def evaluate(model, batch=512):
    rows = []
    for i, (name, kw) in enumerate(REGIMES):
        make = lambda seed_shift=0, kw=kw, i=i: Conditions(
            batch, np.random.default_rng(7000 + i * 13 + seed_shift), **kw)
        kp, kd = tune_classical(make, batch)
        d_cls = rollout_classical(kp, kd, make(), batch)
        ok_c, t_c = episode_success(d_cls)
        d_cfc, _ = rollout_policy(model, make(), batch, grad=False)
        ok_f, t_f = episode_success(d_cfc)
        rows.append({
            "regime": name,
            "classical": {"kp": kp, "kd": kd,
                          "success": round(ok_c.float().mean().item(), 3),
                          "median_steps": int(t_c.float().median().item())},
            "cfc": {"success": round(ok_f.float().mean().item(), 3),
                    "median_steps": int(t_f.float().median().item())},
        })
    return rows


# ── training (BPTT through the dynamics) ─────────────────────────────────────────

def main():
    quick = "--quick" in sys.argv
    torch.manual_seed(SEED)
    model = HandsCfC()
    n_params = sum(p.numel() for p in model.parameters())
    opt = torch.optim.Adam(model.parameters(), lr=3e-3)
    iters = 1500 if not quick else 40
    batch = 256 if not quick else 96
    ramp = torch.linspace(0.2, 1.0, T_STEPS).unsqueeze(1)   # late distance matters most

    t0 = time.time()
    for it in range(iters):
        cond = Conditions(batch, np.random.default_rng(10_000 + it))   # fresh regime each iter
        dists, jerk = rollout_policy(model, cond, batch, grad=True)
        d_soft = torch.sqrt(dists ** 2 + 1e-6)
        loss = (ramp * d_soft).mean() + 0.02 * jerk / T_STEPS
        opt.zero_grad()
        loss.backward()
        torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0)
        opt.step()
        if it % 50 == 0 or it == iters - 1:
            with torch.no_grad():
                ok, _ = episode_success(dists)
            print(f"iter {it:4d}  loss {loss.item():.4f}  train-success {ok.float().mean():.2f}"
                  f"  ({time.time()-t0:.0f}s)", flush=True)

    print("\n── GATE: one CfC weight-set vs per-regime-TUNED P/PD ──")
    rows = evaluate(model, batch=512 if not quick else 128)
    for r in rows:
        c, f = r["classical"], r["cfc"]
        print(f"  {r['regime']:18s} classical(kp={c['kp']:>4}, kd={c['kd']:>3}): "
              f"{c['success']:.3f} @ {c['median_steps']:3d} steps   "
              f"CfC: {f['success']:.3f} @ {f['median_steps']:3d} steps")

    messy = [r for r in rows if r["regime"] != "clean"]
    clean = [r for r in rows if r["regime"] == "clean"][0]
    wins = sum(1 for r in messy if r["cfc"]["success"] > r["classical"]["success"])
    clean_ok = clean["cfc"]["success"] >= clean["classical"]["success"] - 0.03
    verdict = "PROMOTE" if wins >= 4 and clean_ok else ("PARTIAL" if wins >= 2 and clean_ok else "HOLD(classical-null)")
    report = {
        "date": time.strftime("%Y-%m-%d %H:%M"), "quick": quick, "params": n_params,
        "iters": iters, "t_steps": T_STEPS, "click_radius": CLICK_RADIUS,
        "note": "classical opponents are grid-tuned PER REGIME on separate seeds; CfC is ONE weight-set",
        "rows": rows, "messy_wins": f"{wins}/{len(messy)}", "clean_held": clean_ok,
        "verdict": verdict,
    }
    json.dump(report, open(os.path.join(OUT_DIR, "hands_report.json"), "w"), indent=2)
    torch.save(model.state_dict(), os.path.join(OUT_DIR, "hands_v0.pt"))
    print(f"\nVERDICT: {verdict}  (params {n_params:,}; messy wins {wins}/{len(messy)}; clean held: {clean_ok})")
    print(f"report -> {OUT_DIR}/hands_report.json")


if __name__ == "__main__":
    main()
