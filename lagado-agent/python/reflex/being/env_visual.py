"""being/env_visual.py — visual reaching: the unification testbed.

Same dynamics as hands/env.py (momentum, dt jitter, target jumps/drift, reach+HOLD),
but the policy is NOT handed a target coordinate. It sees a rendered SCREEN — target
patch among distractors — plus proprioception (its own pointer state, which a real
system knows exactly). The visual stream carries frame latency (obs_delay applies to
FRAMES — capture lag); proprioception is current (the OS reports the pointer now).

v0 honesty note: the target is salience-defined (brightest patch; distractors are
dimmer rectangles). What this tests is the CONVERSION claim — pixels→motor with no
symbolic coordinate hop — not object recognition. Semantic targets arrive with real
damage-crop training later.
"""
import numpy as np
import torch

VIS_H, VIS_W = 40, 56    # rendered screen (same pixels the null's detector gets)
PROPRIO_DIM = 7          # p(2), v(2), prev_u(2), dt(1)
U_MAX = 3.0

CLICK_RADIUS = 0.012
HOLD_STEPS = 6


class VisualConditions:
    def __init__(self, batch, rng,
                 momentum=(0.35, 0.90),
                 frame_delay=(0, 8),          # visual latency in STEPS
                 dt_range=(0.010, 0.035),
                 noise=0.002,
                 jump_prob=(0.0, 0.025),
                 drift_speed=(0.0, 0.25),
                 n_distract=3):
        u = lambda lo, hi: torch.tensor(rng.uniform(lo, hi, size=batch), dtype=torch.float32)
        self.momentum = u(*momentum)
        self.frame_delay = torch.tensor(rng.integers(frame_delay[0], frame_delay[1] + 1, size=batch))
        self.dt_range = dt_range
        self.noise = noise
        if isinstance(jump_prob, tuple):
            self.jump_prob = rng.uniform(*jump_prob, size=batch)
        else:
            self.jump_prob = np.full(batch, jump_prob)
        drift_mag = u(*drift_speed)
        ang = u(0, 6.283185)
        self.drift = torch.stack([drift_mag * torch.cos(ang), drift_mag * torch.sin(ang)], dim=1)
        self.n_distract = n_distract
        self.rng = rng


class VisualReachEnv:
    """step(u) → (frame [B,1,H,W] DELAYED, proprio [B,7] current, dt [B])."""

    def __init__(self, batch, cond):
        self.B = batch
        self.c = cond
        yy = torch.linspace(0, 1, VIS_H).view(1, VIS_H, 1)
        xx = torch.linspace(0, 1, VIS_W).view(1, 1, VIS_W)
        self.yy, self.xx = yy, xx

    def reset(self):
        rng = self.c.rng
        r = lambda n=2: torch.tensor(rng.uniform(0.12, 0.88, size=(self.B, n)), dtype=torch.float32)
        self.p = r()
        self.g = r()
        self.v = torch.zeros(self.B, 2)
        self.prev_u = torch.zeros(self.B, 2)
        # distractors: static dim rectangles, sized/positioned per episode
        K = self.c.n_distract
        self.d_pos = torch.tensor(rng.uniform(0.08, 0.92, size=(self.B, K, 2)), dtype=torch.float32)
        self.d_size = torch.tensor(rng.uniform(0.03, 0.10, size=(self.B, K, 2)), dtype=torch.float32)
        self.d_val = torch.tensor(rng.uniform(0.45, 0.65, size=(self.B, K)), dtype=torch.float32)
        self.frames = []
        self.t = 0
        dt0 = torch.full((self.B,), 0.02)
        return self._obs(dt0)

    def _render(self):
        """[B,1,H,W]; pure data (no grad) — the scene, not the gradient path."""
        with torch.no_grad():
            rng = self.c.rng
            img = torch.full((self.B, VIS_H, VIS_W), 0.15)
            img += torch.tensor(rng.normal(0, 0.02, size=img.shape), dtype=torch.float32)
            for k in range(self.c.n_distract):
                dy = self.d_pos[:, k, 1].view(-1, 1, 1)
                dx = self.d_pos[:, k, 0].view(-1, 1, 1)
                hh = self.d_size[:, k, 1].view(-1, 1, 1)
                ww = self.d_size[:, k, 0].view(-1, 1, 1)
                m = ((self.yy - dy).abs() < hh) & ((self.xx - dx).abs() < ww)
                img = torch.where(m, self.d_val[:, k].view(-1, 1, 1).expand_as(img), img)
            ty = self.g[:, 1].view(-1, 1, 1)
            tx = self.g[:, 0].view(-1, 1, 1)
            tm = ((self.yy - ty).abs() < 0.035) & ((self.xx - tx).abs() < 0.025)
            img = torch.where(tm, torch.ones_like(img), img)
            return img.clamp(0, 1).unsqueeze(1)

    def _obs(self, dt):
        self.frames.append(self._render())
        idx = torch.clamp(torch.tensor(len(self.frames) - 1) - self.c.frame_delay, min=0)
        stacked = torch.stack(self.frames)                       # [t,B,1,H,W]
        frame = stacked[idx, torch.arange(self.B)]
        proprio = torch.cat([self.p, self.v / U_MAX, self.prev_u, dt.unsqueeze(1) * 30.0], dim=1)
        return frame, proprio, dt

    def step(self, u):
        rng = self.c.rng
        dt = torch.tensor(rng.uniform(*self.c.dt_range, size=self.B), dtype=torch.float32)
        a = self.c.momentum.unsqueeze(1)
        self.v = a * self.v + (1 - a) * (u * U_MAX)
        noise = torch.tensor(rng.normal(0, self.c.noise, size=(self.B, 2)), dtype=torch.float32)
        self.p = self.p + self.v * dt.unsqueeze(1) + noise
        with torch.no_grad():
            self.g = self.g + self.c.drift * dt.unsqueeze(1)
            jump = torch.tensor(rng.random(self.B) < self.c.jump_prob)
            if jump.any():
                nj = int(jump.sum())
                self.g[jump] = torch.tensor(rng.uniform(0.15, 0.85, size=(nj, 2)), dtype=torch.float32)
            self.g.clamp_(0.05, 0.95)
        self.prev_u = u
        self.t += 1
        return self._obs(dt)


def episode_success(dist_seq):
    T, B = dist_seq.shape
    inside = dist_seq < CLICK_RADIUS
    run = torch.zeros(B, dtype=torch.long)
    done = torch.zeros(B, dtype=torch.bool)
    t_click = torch.full((B,), T, dtype=torch.long)
    for t in range(T):
        run = torch.where(inside[t], run + 1, torch.zeros_like(run))
        just = (~done) & (run >= HOLD_STEPS)
        t_click[just] = t
        done |= just
    return done, t_click


def detect_target(frame):
    """The NULL's symbolic eyes: centroid of the brightest blob (>0.9) on the same
    delayed frame the unified net sees. Near-perfect for a salience target — the
    conversion tax it pays is DISCRETIZATION (pixel-grid coords) + staleness."""
    m = (frame[:, 0] > 0.9).float()
    B, H, W = m.shape
    ys = torch.linspace(0, 1, H).view(1, H, 1)
    xs = torch.linspace(0, 1, W).view(1, 1, W)
    n = m.sum(dim=(1, 2)).clamp(min=1e-6)
    cy = (m * ys).sum(dim=(1, 2)) / n
    cx = (m * xs).sum(dim=(1, 2)) / n
    found = m.sum(dim=(1, 2)) > 0
    out = torch.stack([cx, cy], dim=1)
    out[~found] = 0.5
    return out
