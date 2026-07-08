"""being/train_being2.py — being v1: messy multi-app desktops, fovea, breakage report.

Two visual streams (delayed global context + delayed-frame fovea at the current
pointer) + proprioception → one CfC latent → motor + target readout. Backgrounds
are real VM screenshots and generated app UIs; the target is a checker glyph among
DECOY glyphs — discrimination, not salience. Null = matched-filter template
detector (knows the exact glyph) on the same delayed frames → (x,y) → tuned PD /
hands_v0. Breakage taxonomy (near_miss / decoy_capture / wander) per regime.

Run: .venv/bin/python being/train_being2.py [--quick]
"""
import json
import os
import sys
import time

import numpy as np
import torch
import torch.nn as nn
import torch.nn.functional as F
from ncps.torch import CfCCell

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from being.env_visual2 import (MessyReachEnv, MessyConditions, episode_success, breakage,
                               composite_full, load_backgrounds, GLYPH, CELL,
                               HI_H, HI_W, FOV, PROPRIO_DIM, U_MAX, G_H, G_W)
from hands.train_hands import HandsCfC

SEED = 9
OUT_DIR = os.path.dirname(os.path.abspath(__file__))
T_STEPS = 120


class Being2(nn.Module):
    def __init__(self, units=80):
        super().__init__()
        self.enc_glo = nn.Sequential(
            nn.Conv2d(1, 8, 5, stride=2, padding=2), nn.ReLU(),
            nn.Conv2d(8, 16, 3, stride=2, padding=1), nn.ReLU(),
            nn.Conv2d(16, 16, 3, stride=2, padding=1), nn.ReLU(),
            nn.Flatten(), nn.Linear(16 * 5 * 7, 48), nn.Tanh(),
        )
        self.enc_fov = nn.Sequential(
            nn.Conv2d(1, 8, 5, stride=2, padding=2), nn.ReLU(),
            nn.Conv2d(8, 16, 3, stride=2, padding=1), nn.ReLU(),
            nn.Conv2d(16, 16, 3, stride=2, padding=1), nn.ReLU(),
            nn.Flatten(), nn.Linear(16 * 5 * 5, 48), nn.Tanh(),
        )
        self.fuse = nn.Linear(48 + 48 + PROPRIO_DIM, 64)
        self.cell = CfCCell(64, units)
        self.motor = nn.Linear(units, 2)
        self.target_ro = nn.Linear(units, 2)
        self.units = units

    def forward(self, glo, fov, proprio, h, dt):
        z = torch.cat([self.enc_glo(glo), self.enc_fov(fov), proprio], dim=1)
        x = torch.tanh(self.fuse(z))
        _, h = self.cell(x, h, dt.unsqueeze(1))
        return torch.tanh(self.motor(h)), torch.sigmoid(self.target_ro(h)), h


def rollout_being(model, cond, batch, bgs, grad=True):
    env = MessyReachEnv(batch, cond, bgs)
    glo, fov, proprio, dt = env.reset()
    h = torch.zeros(batch, model.units)
    dists, aux, jerk = [], 0.0, 0.0
    prev_u = torch.zeros(batch, 2)
    ctx = torch.enable_grad() if grad else torch.no_grad()
    with ctx:
        for _ in range(T_STEPS):
            u, tpred, h = model(glo, fov, proprio, h, dt)
            glo, fov, proprio, dt = env.step(u)
            dists.append(torch.linalg.norm(env.g - env.p, dim=1))
            aux = aux + (tpred - env.g).abs().mean()
            jerk = jerk + ((u - prev_u) ** 2).mean()
            prev_u = u
    return torch.stack(dists), aux / T_STEPS, jerk, env


# ── null: matched-filter eyes → (x,y) → controller ─────────────────────────────

_big = GLYPH.repeat_interleave(CELL, 0).repeat_interleave(CELL, 1)
KER = ((_big - _big.mean()) / (_big.std() + 1e-6)).view(1, 1, G_H, G_W)


def detect(frame):
    """Template match with the EXACT glyph kernel (the strongest symbolic eyes)."""
    corr = F.conv2d(frame - frame.mean(dim=(2, 3), keepdim=True), KER)
    B = corr.shape[0]
    flat = corr.view(B, -1)
    idx = flat.argmax(dim=1)
    cy = (idx // corr.shape[3]).float() + G_H / 2
    cx = (idx % corr.shape[3]).float() + G_W / 2
    return torch.stack([cx / (HI_W - 1), cy / (HI_H - 1)], dim=1)


def rollout_pipeline(cond, batch, bgs, hands=None, kp=None, kd=None):
    env = MessyReachEnv(batch, cond, bgs)
    glo, fov, proprio, dt = env.reset()
    h = torch.zeros(batch, hands.units) if hands is not None else None
    dists = []
    prev_u = torch.zeros(batch, 2)
    with torch.no_grad():
        for _ in range(T_STEPS):
            g_det = detect(composite_full(env, env._delayed_g()))
            p = proprio[:, 0:2]
            v = proprio[:, 2:4] * U_MAX
            err = g_det - p
            if hands is not None:
                obs = torch.cat([err, v, prev_u, (proprio[:, 6:7] / 30.0)], dim=1)
                u, h = hands(obs, h, dt)
            else:
                u = torch.clamp(kp * err - kd * v / U_MAX, -1.0, 1.0)
            glo, fov, proprio, dt = env.step(u)
            dists.append(torch.linalg.norm(env.g - env.p, dim=1))
            prev_u = u
    return torch.stack(dists), env


def tune_pd(make_cond, batch, bgs):
    best = None
    for kp in [3, 5, 8, 12, 18]:
        for kd in [0.0, 0.5, 1.0, 2.0, 4.0]:
            d, _ = rollout_pipeline(make_cond(seed_shift=900), batch, bgs, kp=kp, kd=kd)
            ok, t_click = episode_success(d)
            key = (ok.float().mean().item(), -t_click.float().median().item())
            if best is None or key > best[0]:
                best = (key, kp, kd)
    return best[1], best[2]


REGIMES = [
    ("clean",            dict(momentum=(0.40, 0.50), frame_delay=(0, 1), jump_prob=0.0, drift_speed=(0.0, 0.0))),
    ("lag (in-range)",   dict(momentum=(0.60, 0.70), frame_delay=(5, 8), jump_prob=0.0, drift_speed=(0.0, 0.0))),
    ("lag (extrap)",     dict(momentum=(0.60, 0.70), frame_delay=(9, 12), jump_prob=0.0, drift_speed=(0.0, 0.0))),
    ("moving target",    dict(momentum=(0.55, 0.70), frame_delay=(1, 3), jump_prob=0.02, drift_speed=(0.15, 0.25))),
    ("messy (combined)", dict(momentum=(0.80, 0.90), frame_delay=(4, 8), jump_prob=0.01, drift_speed=(0.10, 0.20))),
]


def evaluate(model, hands, bgs_eval, batch=256):
    rows = []
    for i, (name, kw) in enumerate(REGIMES):
        make = lambda seed_shift=0, kw=kw, i=i: MessyConditions(
            batch, np.random.default_rng(9200 + i * 19 + seed_shift), **kw)
        kp, kd = tune_pd(make, batch, bgs_eval)
        d_pd, env_pd = rollout_pipeline(make(), batch, bgs_eval, kp=kp, kd=kd)
        ok_pd, t_pd = episode_success(d_pd)
        d_hp, env_hp = rollout_pipeline(make(), batch, bgs_eval, hands=hands)
        ok_hp, t_hp = episode_success(d_hp)
        d_be, _, _, env_be = rollout_being(model, make(), batch, bgs_eval, grad=False)
        ok_be, t_be = episode_success(d_be)
        rows.append({
            "regime": name,
            "pipeline_pd": {"kp": kp, "kd": kd, "success": round(ok_pd.float().mean().item(), 3),
                            "median_steps": int(t_pd.float().median().item()),
                            "breakage": breakage(ok_pd, env_pd.p, env_pd.g, env_pd.decoy_last)},
            "pipeline_hands": {"success": round(ok_hp.float().mean().item(), 3),
                               "median_steps": int(t_hp.float().median().item()),
                               "breakage": breakage(ok_hp, env_hp.p, env_hp.g, env_hp.decoy_last)},
            "being": {"success": round(ok_be.float().mean().item(), 3),
                      "median_steps": int(t_be.float().median().item()),
                      "breakage": breakage(ok_be, env_be.p, env_be.g, env_be.decoy_last)},
        })
    return rows


def main():
    quick = "--quick" in sys.argv
    torch.manual_seed(SEED)
    rng = np.random.default_rng(SEED)
    bgs = load_backgrounds(rng, n_synth=80 if not quick else 15)
    # background split: eval uses UNSEEN backgrounds (the multi-app transfer claim)
    perm = torch.randperm(len(bgs))
    bgs_train, bgs_eval = bgs[perm[:int(len(bgs) * 0.75)]], bgs[perm[int(len(bgs) * 0.75):]]
    print(f"backgrounds: {len(bgs_train)} train / {len(bgs_eval)} eval (unseen)")

    model = Being2()
    n_params = sum(p.numel() for p in model.parameters())
    hands = HandsCfC()
    hands.load_state_dict(torch.load(os.path.join(OUT_DIR, "..", "hands", "hands_v0.pt")))
    hands.eval()
    opt = torch.optim.Adam(model.parameters(), lr=2e-3)
    iters = 4000 if not quick else 25
    sched = torch.optim.lr_scheduler.CosineAnnealingLR(opt, T_max=iters, eta_min=1e-4)
    batch = 64 if not quick else 32
    ramp = torch.linspace(0.2, 1.0, T_STEPS).unsqueeze(1)

    t0 = time.time()
    for it in range(iters):
        cond = MessyConditions(batch, np.random.default_rng(30_000 + it))
        dists, aux, jerk, _ = rollout_being(model, cond, batch, bgs_train, grad=True)
        d_soft = torch.sqrt(dists ** 2 + 1e-6)
        loss = (ramp * d_soft).mean() + 0.02 * jerk / T_STEPS + 0.3 * aux
        opt.zero_grad()
        loss.backward()
        torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0)
        opt.step()
        sched.step()
        if it % 100 == 0 or it == iters - 1:
            with torch.no_grad():
                ok, _ = episode_success(dists)
            print(f"iter {it:4d}  loss {loss.item():.4f}  aux {aux.item():.4f}"
                  f"  train-success {ok.float().mean():.2f}  ({time.time()-t0:.0f}s)", flush=True)
            torch.save(model.state_dict(), os.path.join(OUT_DIR, "being_v1.pt"))

    print("\n── GATE: fovea being vs matched-filter pipeline, UNSEEN multi-app backgrounds ──")
    rows = evaluate(model, hands, bgs_eval, batch=256 if not quick else 64)
    for r in rows:
        pd_, hp, be = r["pipeline_pd"], r["pipeline_hands"], r["being"]
        print(f"  {r['regime']:18s} eyes→PD: {pd_['success']:.3f}@{pd_['median_steps']:3d}"
              f"   eyes→hands: {hp['success']:.3f}@{hp['median_steps']:3d}"
              f"   BEING: {be['success']:.3f}@{be['median_steps']:3d}")
        print(f"     breakage being: {be['breakage']}   pipeline_hands: {hp['breakage']}")

    messy = [r for r in rows if r["regime"] != "clean"]
    clean = [r for r in rows if r["regime"] == "clean"][0]
    best_null = lambda r: max(r["pipeline_pd"]["success"], r["pipeline_hands"]["success"])
    wins = sum(1 for r in messy if r["being"]["success"] > best_null(r))
    clean_ok = clean["being"]["success"] >= best_null(clean) - 0.03
    verdict = "PROMOTE" if wins >= 3 and clean_ok else ("PARTIAL" if wins >= 2 and clean_ok else "HOLD(pipeline-null)")
    report = {
        "date": time.strftime("%Y-%m-%d %H:%M"), "quick": quick, "params": n_params,
        "iters": iters, "t_steps": T_STEPS,
        "note": "unseen backgrounds; glyph target among decoys; null = exact-template matched filter",
        "rows": rows, "messy_wins": f"{wins}/{len(messy)}", "clean_held": clean_ok,
        "verdict": verdict,
    }
    json.dump(report, open(os.path.join(OUT_DIR, "being_v1_report.json"), "w"), indent=2)
    torch.save(model.state_dict(), os.path.join(OUT_DIR, "being_v1.pt"))
    print(f"\nVERDICT: {verdict}  (params {n_params:,}; messy wins {wins}/{len(messy)}; clean held: {clean_ok})")


if __name__ == "__main__":
    main()
