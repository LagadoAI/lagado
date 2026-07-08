"""hands/env.py — differentiable cursor-reaching environment (hands v0).

The CfC's home turf, built honestly: the policy must drive a pointer to a target and
HOLD it there (a click needs stability), under the dynamics that make real desktops
messy for fixed-gain controllers:

  momentum      — the pointer has inertia (actuator lag): v ← α·v + (1−α)·u
  OBSERVATION latency — the policy sees the world L steps late (frame-capture delay;
                  this is where prediction pays and where fixed gains oscillate)
  dt jitter     — steps arrive at irregular intervals (the continuous-time argument)
  target motion — the target can JUMP mid-flight (UI relayout) or DRIFT (smooth scroll)
  noise         — actuation noise

Everything is torch and differentiable, so a policy can be trained by BACKPROP
THROUGH THE DYNAMICS (no RL machinery, no teacher to cap it). Positions are in a
normalized [0,1]² screen. One env instance runs a BATCH of episodes with per-episode
conditions.
"""
import numpy as np
import torch

OBS_DIM = 7   # err(2) obs-delayed, vel(2) obs-delayed, prev action(2), dt(1)
ACT_DIM = 2   # commanded velocity, tanh-bounded, scaled by U_MAX
U_MAX = 3.0   # max commanded speed (screens/second)


class Conditions:
    """Per-episode dynamics, sampled once per episode. Ranges define a regime;
    the gate sweeps regimes the training never saw (extrapolation rows)."""
    def __init__(self, batch, rng,
                 momentum=(0.35, 0.90),      # α: 0 = ideal, →1 = heavy inertia
                 obs_delay=(0, 8),           # observation latency in STEPS
                 dt_range=(0.010, 0.035),    # seconds per step (jittered every step)
                 noise=0.002,                # actuation noise (screens)
                 jump_prob=(0.0, 0.025),     # per-step target jump probability (per-episode)
                 drift_speed=(0.0, 0.25)):   # target drift (screens/second)
        u = lambda lo, hi: torch.tensor(rng.uniform(lo, hi, size=batch), dtype=torch.float32)
        self.momentum = u(*momentum)
        self.obs_delay = torch.tensor(rng.integers(obs_delay[0], obs_delay[1] + 1, size=batch))
        self.dt_range = dt_range
        self.noise = noise
        if isinstance(jump_prob, tuple):
            self.jump_prob = rng.uniform(*jump_prob, size=batch)   # per-episode, like momentum
        else:
            self.jump_prob = np.full(batch, jump_prob)
        drift_mag = u(*drift_speed)
        ang = u(0, 6.283185)
        self.drift = torch.stack([drift_mag * torch.cos(ang), drift_mag * torch.sin(ang)], dim=1)
        self.rng = rng


class ReachEnv:
    """Vectorized differentiable rollout. Call reset(), then step(u) T times.
    Histories are kept so each sample reads its own delayed observation."""

    def __init__(self, batch, cond):
        self.B = batch
        self.c = cond
        self.t = 0

    def reset(self):
        rng = self.c.rng
        r = lambda: torch.tensor(rng.uniform(0.1, 0.9, size=(self.B, 2)), dtype=torch.float32)
        self.p = r()                    # pointer
        self.g = r()                    # target
        self.v = torch.zeros(self.B, 2)
        self.prev_u = torch.zeros(self.B, 2)
        self.hist_err = []              # for delayed observation
        self.hist_vel = []
        self.t = 0
        return self._obs(torch.full((self.B,), 0.02))

    def _delayed(self, hist, fallback):
        idx = torch.clamp(torch.tensor(len(hist) - 1) - self.c.obs_delay, min=0)
        if not hist:
            return fallback
        stacked = torch.stack(hist)                       # [t, B, 2]
        return stacked[idx, torch.arange(self.B)]         # per-sample delayed row

    def _obs(self, dt):
        err = self.g - self.p
        self.hist_err.append(err)
        self.hist_vel.append(self.v)
        e = self._delayed(self.hist_err, err)
        v = self._delayed(self.hist_vel, self.v)
        return torch.cat([e, v, self.prev_u, dt.unsqueeze(1)], dim=1)

    def step(self, u):
        """u ∈ [-1,1]²; returns (obs, dt). Differentiable through p/v/err chains."""
        rng = self.c.rng
        dt = torch.tensor(rng.uniform(*self.c.dt_range, size=self.B), dtype=torch.float32)
        a = self.c.momentum.unsqueeze(1)
        self.v = a * self.v + (1 - a) * (u * U_MAX)
        noise = torch.tensor(rng.normal(0, self.c.noise, size=(self.B, 2)), dtype=torch.float32)
        self.p = self.p + self.v * dt.unsqueeze(1) + noise
        # target motion (non-differentiable events; constant between events)
        with torch.no_grad():
            self.g = self.g + self.c.drift * dt.unsqueeze(1)
            jump = torch.tensor(rng.random(self.B) < self.c.jump_prob)
            if jump.any():
                nj = int(jump.sum())
                self.g[jump] = torch.tensor(rng.uniform(0.15, 0.85, size=(nj, 2)), dtype=torch.float32)
            self.g.clamp_(0.02, 0.98)
        self.prev_u = u
        self.t += 1
        return self._obs(dt), dt


# ── success metric (shared by CfC and classical nulls) ─────────────────────────────

CLICK_RADIUS = 0.012    # ~23px at 1920 wide
HOLD_STEPS = 6          # must stay inside for this many consecutive steps

def episode_success(dist_seq):
    """dist_seq [T, B] → (success [B] bool, time_idx [B] step of click or T).
    Click = first run of HOLD_STEPS consecutive steps inside CLICK_RADIUS."""
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
