"""being/train_being.py — 'one being' v0: pixels → shared latent → motor, ONE gradient.

The unification test. The being: conv encoder over the (delayed) screen + current
proprioception → ONE continuous-time latent (CfCCell) → motor head (control path)
+ target-position readout head (audit path, aux loss — the instrument panel, not
the drivetrain). Trained end-to-end by BPTT through the differentiable dynamics:
the eyes learn to encode what the hands need.

THE NULL = OUR OWN TWO-STAGE PIPELINE (the architecture this build would replace):
deterministic symbolic eyes (blob detector → (x,y) on the same delayed frame) hand
coordinates to (a) hands_v0's trained CfC and (b) a per-regime grid-tuned PD.
Unification is promoted only if the shared-latent being beats the message-passing
pipeline on messy regimes without losing clean.

Run: .venv/bin/python being/train_being.py [--quick]
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
from being.env_visual import (VisualReachEnv, VisualConditions, episode_success,
                              detect_target, VIS_H, VIS_W, PROPRIO_DIM, U_MAX)
from hands.train_hands import HandsCfC

SEED = 3
OUT_DIR = os.path.dirname(os.path.abspath(__file__))
T_STEPS = 120


# ── the being ────────────────────────────────────────────────────────────────────

class Being(nn.Module):
    def __init__(self, units=64):
        super().__init__()
        self.enc = nn.Sequential(
            nn.Conv2d(1, 8, 5, stride=2, padding=2), nn.ReLU(),
            nn.Conv2d(8, 16, 3, stride=2, padding=1), nn.ReLU(),
            nn.Conv2d(16, 16, 3, stride=2, padding=1), nn.ReLU(),
            nn.Flatten(), nn.Linear(16 * 5 * 7, 64), nn.Tanh(),
        )
        self.fuse = nn.Linear(64 + PROPRIO_DIM, 48)
        self.cell = CfCCell(48, units)
        self.motor = nn.Linear(units, 2)         # control path
        self.target_ro = nn.Linear(units, 2)     # audit readout (aux)
        self.units = units

    def forward(self, frame, proprio, h, dt):
        z = self.enc(frame)
        x = torch.tanh(self.fuse(torch.cat([z, proprio], dim=1)))
        _, h = self.cell(x, h, dt.unsqueeze(1))
        return torch.tanh(self.motor(h)), torch.sigmoid(self.target_ro(h)), h


def rollout_being(model, cond, batch, grad=True):
    env = VisualReachEnv(batch, cond)
    frame, proprio, dt = env.reset()
    h = torch.zeros(batch, model.units)
    dists, aux, jerk = [], 0.0, 0.0
    prev_u = torch.zeros(batch, 2)
    ctx = torch.enable_grad() if grad else torch.no_grad()
    with ctx:
        for _ in range(T_STEPS):
            u, tpred, h = model(frame, proprio, h, dt)
            frame, proprio, dt = env.step(u)
            dists.append(torch.linalg.norm(env.g - env.p, dim=1))
            aux = aux + (tpred - env.g).abs().mean()
            jerk = jerk + ((u - prev_u) ** 2).mean()
            prev_u = u
    return torch.stack(dists), aux / T_STEPS, jerk


# ── the null: symbolic eyes → coordinates → hands (the pipeline being replaced) ──

def rollout_pipeline(cond, batch, hands=None, kp=None, kd=None):
    """Two-stage: detect_target on the SAME delayed frame → hand (x,y) to either the
    trained hands_v0 CfC or a PD. This is the conversion the being eliminates."""
    env = VisualReachEnv(batch, cond)
    frame, proprio, dt = env.reset()
    h = torch.zeros(batch, hands.units) if hands is not None else None
    dists = []
    prev_u = torch.zeros(batch, 2)
    with torch.no_grad():
        for _ in range(T_STEPS):
            g_det = detect_target(frame)
            p = proprio[:, 0:2]
            v = proprio[:, 2:4] * U_MAX
            err = g_det - p
            if hands is not None:
                obs = torch.cat([err, v, prev_u, (proprio[:, 6:7] / 30.0)], dim=1)
                u, h = hands(obs, h, dt)
            else:
                u = torch.clamp(kp * err - kd * v / U_MAX, -1.0, 1.0)
            frame, proprio, dt = env.step(u)
            dists.append(torch.linalg.norm(env.g - env.p, dim=1))
            prev_u = u
    return torch.stack(dists)


def tune_pd(make_cond, batch):
    best = None
    for kp in [1.5, 3, 5, 8, 12, 18]:
        for kd in [0.0, 0.5, 1.0, 2.0, 4.0, 8.0]:
            d = rollout_pipeline(make_cond(seed_shift=900), batch, kp=kp, kd=kd)
            ok, t_click = episode_success(d)
            key = (ok.float().mean().item(), -t_click.float().median().item())
            if best is None or key > best[0]:
                best = (key, kp, kd)
    return best[1], best[2]


# ── gate regimes (same families as hands v0) ─────────────────────────────────────

REGIMES = [
    ("clean",            dict(momentum=(0.40, 0.50), frame_delay=(0, 1), jump_prob=0.0, drift_speed=(0.0, 0.0))),
    ("lag (in-range)",   dict(momentum=(0.60, 0.70), frame_delay=(5, 8), jump_prob=0.0, drift_speed=(0.0, 0.0))),
    ("lag (extrap)",     dict(momentum=(0.60, 0.70), frame_delay=(9, 12), jump_prob=0.0, drift_speed=(0.0, 0.0))),
    ("moving target",    dict(momentum=(0.55, 0.70), frame_delay=(1, 3), jump_prob=0.02, drift_speed=(0.15, 0.25))),
    ("messy (combined)", dict(momentum=(0.80, 0.90), frame_delay=(4, 8), jump_prob=0.01, drift_speed=(0.10, 0.20))),
]


def evaluate(model, hands, batch=384):
    rows = []
    for i, (name, kw) in enumerate(REGIMES):
        make = lambda seed_shift=0, kw=kw, i=i: VisualConditions(
            batch, np.random.default_rng(8100 + i * 17 + seed_shift), **kw)
        kp, kd = tune_pd(make, batch)
        d_pd = rollout_pipeline(make(), batch, kp=kp, kd=kd)
        ok_pd, t_pd = episode_success(d_pd)
        d_hp = rollout_pipeline(make(), batch, hands=hands)
        ok_hp, t_hp = episode_success(d_hp)
        d_be, _, _ = rollout_being(model, make(), batch, grad=False)
        ok_be, t_be = episode_success(d_be)
        rows.append({
            "regime": name,
            "pipeline_pd": {"kp": kp, "kd": kd, "success": round(ok_pd.float().mean().item(), 3),
                            "median_steps": int(t_pd.float().median().item())},
            "pipeline_hands": {"success": round(ok_hp.float().mean().item(), 3),
                               "median_steps": int(t_hp.float().median().item())},
            "being": {"success": round(ok_be.float().mean().item(), 3),
                      "median_steps": int(t_be.float().median().item())},
        })
    return rows


def main():
    quick = "--quick" in sys.argv
    torch.manual_seed(SEED)
    model = Being()
    n_params = sum(p.numel() for p in model.parameters())
    hands = HandsCfC()
    hands.load_state_dict(torch.load(os.path.join(OUT_DIR, "..", "hands", "hands_v0.pt")))
    hands.eval()
    opt = torch.optim.Adam(model.parameters(), lr=2e-3)
    iters = 700 if not quick else 25
    batch = 96 if not quick else 48
    ramp = torch.linspace(0.2, 1.0, T_STEPS).unsqueeze(1)

    t0 = time.time()
    for it in range(iters):
        cond = VisualConditions(batch, np.random.default_rng(20_000 + it))
        dists, aux, jerk = rollout_being(model, cond, batch, grad=True)
        d_soft = torch.sqrt(dists ** 2 + 1e-6)
        loss = (ramp * d_soft).mean() + 0.02 * jerk / T_STEPS + 0.3 * aux
        opt.zero_grad()
        loss.backward()
        torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0)
        opt.step()
        if it % 50 == 0 or it == iters - 1:
            with torch.no_grad():
                ok, _ = episode_success(dists)
            print(f"iter {it:4d}  loss {loss.item():.4f}  aux {aux.item():.4f}"
                  f"  train-success {ok.float().mean():.2f}  ({time.time()-t0:.0f}s)", flush=True)

    print("\n── GATE: shared-latent being vs the two-stage pipeline (symbolic handoff) ──")
    rows = evaluate(model, hands, batch=384 if not quick else 96)
    for r in rows:
        pd, hp, be = r["pipeline_pd"], r["pipeline_hands"], r["being"]
        print(f"  {r['regime']:18s} eyes→PD(kp={pd['kp']:>4},kd={pd['kd']:>3}): {pd['success']:.3f}@{pd['median_steps']:3d}"
              f"   eyes→handsCfC: {hp['success']:.3f}@{hp['median_steps']:3d}"
              f"   BEING: {be['success']:.3f}@{be['median_steps']:3d}")

    messy = [r for r in rows if r["regime"] != "clean"]
    clean = [r for r in rows if r["regime"] == "clean"][0]
    best_null = lambda r: max(r["pipeline_pd"]["success"], r["pipeline_hands"]["success"])
    wins = sum(1 for r in messy if r["being"]["success"] > best_null(r))
    clean_ok = clean["being"]["success"] >= best_null(clean) - 0.03
    verdict = "PROMOTE" if wins >= 3 and clean_ok else ("PARTIAL" if wins >= 2 and clean_ok else "HOLD(pipeline-null)")
    report = {
        "date": time.strftime("%Y-%m-%d %H:%M"), "quick": quick, "params": n_params,
        "iters": iters, "t_steps": T_STEPS, "vis": [VIS_H, VIS_W],
        "note": "null = deterministic blob-eyes handing (x,y) to hands_v0 CfC / per-regime-tuned PD, same delayed frames",
        "rows": rows, "messy_wins": f"{wins}/{len(messy)}", "clean_held": clean_ok,
        "verdict": verdict,
    }
    json.dump(report, open(os.path.join(OUT_DIR, "being_report.json"), "w"), indent=2)
    torch.save(model.state_dict(), os.path.join(OUT_DIR, "being_v0.pt"))
    print(f"\nVERDICT: {verdict}  (params {n_params:,}; messy wins {wins}/{len(messy)}; clean held: {clean_ok})")
    print(f"report -> {OUT_DIR}/being_report.json")


if __name__ == "__main__":
    main()
