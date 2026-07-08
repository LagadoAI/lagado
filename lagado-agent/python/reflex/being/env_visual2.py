"""being/env_visual2.py — messy-desktop visual reaching (being v1).

Upgrades over v0.5, per user directive ("LibreOffice isn't enough — messy desktops,
multiple apps, see how it breaks"):

  REAL BACKGROUNDS — scenes composite onto real VM screenshots (desktop, LibreOffice,
    terminal) mixed with generated fake-app UIs (infinite layout variety). Bright UI
    clutter is everywhere, so brightness is no longer a valid target cue.
  PATTERN TARGET — the target is a distinctive checker glyph, not "the bright patch".
    Decoy glyphs (one cell flipped) are injected among the distractors: the being must
    DISCRIMINATE, and the breakage report counts decoy captures.
  FOVEA — two visual streams: delayed global view (context: where roughly) + a
    high-res crop of the delayed frame centered at the CURRENT pointer (precision:
    exactly where, once close). The static-precision gap of v0.5 was resolution.
  BREAKAGE TAXONOMY — failures are classified (near_miss / decoy_capture / wander).

Scenes are static per episode except the target (windows don't move; the target
does), so the delayed frame is RECONSTRUCTED = static background + target at its
delayed position — full-fidelity history without storing frames.
"""
import glob
import os
import numpy as np
import torch

HI_H, HI_W = 160, 224          # underlying scene (native-ish)
GLO_H, GLO_W = 40, 56          # global stream (downsampled context)
FOV = 33                       # fovea crop size (odd, centered)
PROPRIO_DIM = 7
U_MAX = 3.0
CLICK_RADIUS = 0.012
HOLD_STEPS = 6

REAL_GLOBS = ["/tmp/lagado_battery/frames/*.png", "/tmp/lagado_battery/settle_dump/*.png"]

# target glyph: 3x4 checker (rendered ~11x15 px hi-res); decoys flip one cell
GLYPH = torch.tensor([[1., 0., 1., 0.],
                      [0., 1., 0., 1.],
                      [1., 0., 1., 0.]])
DECOY = GLYPH.clone(); DECOY[1, 1] = 1.0
CELL = 4                       # px per glyph cell at hi-res
G_H, G_W = 3 * CELL, 4 * CELL  # 12 x 16 px


def load_backgrounds(rng, n_synth=60):
    """Real VM screenshots + generated fake-app UIs, all at hi-res grayscale."""
    from PIL import Image
    outs = []
    for p in sorted(q for g in REAL_GLOBS for q in glob.glob(g)):
        try:
            outs.append(np.asarray(Image.open(p).convert("L").resize((HI_W, HI_H)), dtype=np.float32) / 255.0)
        except Exception:
            continue
    import sys
    sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    from eyes import synth as eyes_synth
    for _ in range(n_synth):
        ui = eyes_synth.gen_ui_base(rng)           # 80x128 fake app
        img = Image.fromarray(ui).resize((HI_W, HI_H))
        outs.append(np.asarray(img, dtype=np.float32) / 255.0)
    return torch.tensor(np.stack(outs))


def _paste_glyph(scene, cx, cy, glyph, contrast=1.0):
    """Paste glyph (soft edges via bilinear placement) onto scene [B,H,W] at
    normalized centers. In-place-ish; returns scene."""
    B = scene.shape[0]
    px = cx * (HI_W - 1) - G_W / 2
    py = cy * (HI_H - 1) - G_H / 2
    gy = torch.arange(G_H).view(1, G_H, 1)
    gx = torch.arange(G_W).view(1, 1, G_W)
    big = glyph.repeat_interleave(CELL, 0).repeat_interleave(CELL, 1)  # [G_H,G_W]
    for b in range(B):                       # per-sample paste (bilinear split)
        x0f, y0f = px[b].item(), py[b].item()
        x0, y0 = int(np.floor(x0f)), int(np.floor(y0f))
        fx, fy = x0f - x0, y0f - y0
        for dy in (0, 1):
            for dx in (0, 1):
                w = (fy if dy else 1 - fy) * (fx if dx else 1 - fx)
                if w < 1e-4:
                    continue
                ys, xs = y0 + dy, x0 + dx
                ye, xe = ys + G_H, xs + G_W
                if ys < 0 or xs < 0 or ye > HI_H or xe > HI_W:
                    continue
                region = scene[b, ys:ye, xs:xe]
                scene[b, ys:ye, xs:xe] = region * (1 - w * contrast) + big * (w * contrast)
    return scene


class MessyConditions:
    def __init__(self, batch, rng,
                 momentum=(0.35, 0.90), frame_delay=(0, 8),
                 dt_range=(0.010, 0.035), noise=0.002,
                 jump_prob=(0.0, 0.025), drift_speed=(0.0, 0.25),
                 n_decoys=2):
        u = lambda lo, hi: torch.tensor(rng.uniform(lo, hi, size=batch), dtype=torch.float32)
        self.momentum = u(*momentum)
        self.frame_delay = torch.tensor(rng.integers(frame_delay[0], frame_delay[1] + 1, size=batch))
        self.dt_range = dt_range
        self.noise = noise
        self.jump_prob = rng.uniform(*jump_prob, size=batch) if isinstance(jump_prob, tuple) else np.full(batch, jump_prob)
        drift_mag = u(*drift_speed)
        ang = u(0, 6.283185)
        self.drift = torch.stack([drift_mag * torch.cos(ang), drift_mag * torch.sin(ang)], dim=1)
        self.n_decoys = n_decoys
        self.rng = rng


class MessyReachEnv:
    """step(u) → (global [B,1,40,56] DELAYED, fovea [B,1,33,33] delayed frame at
    CURRENT pointer, proprio [B,7], dt [B])."""

    def __init__(self, batch, cond, backgrounds):
        self.B, self.c = batch, cond
        self.bgs = backgrounds
        big = GLYPH.repeat_interleave(CELL, 0).repeat_interleave(CELL, 1)
        self._glyph_pad = torch.nn.functional.pad(big, (1, 1, 1, 1)).view(1, 1, G_H + 2, G_W + 2)
        self._alpha_pad = torch.nn.functional.pad(torch.ones_like(big), (1, 1, 1, 1)).view(1, 1, G_H + 2, G_W + 2)

    def reset(self):
        rng = self.c.rng
        idx = torch.tensor(rng.integers(0, len(self.bgs), size=self.B))
        self.bg = self.bgs[idx].clone()                       # [B,H,W] static scene
        r = lambda: torch.tensor(rng.uniform(0.12, 0.88, size=(self.B, 2)), dtype=torch.float32)
        # bake DECOY glyphs into the static background (they never move)
        for _ in range(self.c.n_decoys):
            d = r()
            _paste_glyph(self.bg, d[:, 0], d[:, 1], DECOY)
        self.decoy_last = d                                    # for breakage taxonomy
        self.p, self.g = r(), r()
        self.v = torch.zeros(self.B, 2)
        self.prev_u = torch.zeros(self.B, 2)
        self.g_hist = []
        self.t = 0
        # precompute downsampled static bg for the global stream
        self.bg_lo = torch.nn.functional.avg_pool2d(self.bg.unsqueeze(1), kernel_size=4).squeeze(1)
        return self._obs(torch.full((self.B,), 0.02))

    def _delayed_g(self):
        idx = torch.clamp(torch.tensor(len(self.g_hist) - 1) - self.c.frame_delay, min=0)
        return torch.stack(self.g_hist)[idx, torch.arange(self.B)]

    def _obs(self, dt):
        import torch.nn.functional as F
        self.g_hist.append(self.g.clone())
        gd = self._delayed_g()                                 # target pos in the DELAYED frame
        # global stream: downsampled static bg + target glyph blob at delayed pos
        glo = self.bg_lo.clone()
        yy = torch.linspace(0, 1, GLO_H).view(1, GLO_H, 1)
        xx = torch.linspace(0, 1, GLO_W).view(1, 1, GLO_W)
        cy, cx = gd[:, 1].view(-1, 1, 1), gd[:, 0].view(-1, 1, 1)
        ey, ex = 1.0 / GLO_H, 1.0 / GLO_W
        cov = (((G_H / 2 / HI_H) - (yy - cy).abs()) / ey + 0.5).clamp(0, 1) \
            * (((G_W / 2 / HI_W) - (xx - cx).abs()) / ex + 0.5).clamp(0, 1)
        glo = glo * (1 - cov) + 0.85 * cov                    # glyph averages ~0.5-1.0
        # FOVEA (vectorized, sub-pixel): grid_sample crop of the static bg at the
        # CURRENT pointer + the target glyph (at its DELAYED position) composited
        # analytically into the crop. No frame storage, no python loops.
        half = FOV // 2
        lin = torch.arange(FOV, dtype=torch.float32) - half            # px offsets
        pc = self.p.detach() * torch.tensor([HI_W - 1.0, HI_H - 1.0])  # [B,2] px center
        ax = (pc[:, 0].view(-1, 1, 1) + lin.view(1, 1, FOV)).expand(-1, FOV, -1)
        ay = (pc[:, 1].view(-1, 1, 1) + lin.view(1, FOV, 1)).expand(-1, -1, FOV)
        grid = torch.stack([ax / (HI_W - 1) * 2 - 1, ay / (HI_H - 1) * 2 - 1], dim=-1)
        fov = F.grid_sample(self.bg.unsqueeze(1), grid, mode="bilinear",
                            padding_mode="border", align_corners=True)
        # glyph compositing: sample glyph bitmap+alpha at fovea pixel coords
        gpx = gd * torch.tensor([HI_W - 1.0, HI_H - 1.0])              # glyph center px
        rx = ax - (gpx[:, 0].view(-1, 1, 1) - G_W / 2)
        ry = ay - (gpx[:, 1].view(-1, 1, 1) - G_H / 2)
        ggrid = torch.stack([(rx + 1) / (G_W + 2 - 1) * 2 - 1,
                             (ry + 1) / (G_H + 2 - 1) * 2 - 1], dim=-1)
        gval = F.grid_sample(self._glyph_pad.expand(self.B, -1, -1, -1), ggrid,
                             mode="bilinear", padding_mode="zeros", align_corners=True)
        galp = F.grid_sample(self._alpha_pad.expand(self.B, -1, -1, -1), ggrid,
                             mode="bilinear", padding_mode="zeros", align_corners=True)
        fov = fov * (1 - galp) + gval * galp
        proprio = torch.cat([self.p, self.v / U_MAX, self.prev_u, dt.unsqueeze(1) * 30.0], dim=1)
        return glo.unsqueeze(1), fov, proprio, dt   # fov is already [B,1,F,F] from grid_sample

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
            self.g.clamp_(0.06, 0.94)
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


def breakage(ok, final_p, g, decoy):
    """Failure taxonomy: near_miss (<2r), decoy_capture (at a decoy), wander."""
    fail = ~ok
    d_t = torch.linalg.norm(final_p - g, dim=1)
    d_d = torch.linalg.norm(final_p - decoy, dim=1)
    near = fail & (d_t < 2 * CLICK_RADIUS)
    decoyed = fail & ~near & (d_d < 3 * CLICK_RADIUS)
    wander = fail & ~near & ~decoyed
    n = max(1, int(fail.sum()))
    return {"fail": int(fail.sum()),
            "near_miss": round(int(near.sum()) / n, 2),
            "decoy_capture": round(int(decoyed.sum()) / n, 2),
            "wander": round(int(wander.sum()) / n, 2)}


def composite_full(env, gd):
    """Reconstruct the full delayed hi-res frame (static bg + glyph at gd),
    vectorized — the exact image a deterministic detector would receive."""
    import torch.nn.functional as F
    B = env.B
    ax = torch.arange(HI_W, dtype=torch.float32).view(1, 1, HI_W).expand(B, HI_H, -1)
    ay = torch.arange(HI_H, dtype=torch.float32).view(1, HI_H, 1).expand(B, -1, HI_W)
    gpx = gd * torch.tensor([HI_W - 1.0, HI_H - 1.0])
    rx = ax - (gpx[:, 0].view(-1, 1, 1) - G_W / 2)
    ry = ay - (gpx[:, 1].view(-1, 1, 1) - G_H / 2)
    ggrid = torch.stack([(rx + 1) / (G_W + 2 - 1) * 2 - 1,
                         (ry + 1) / (G_H + 2 - 1) * 2 - 1], dim=-1)
    gval = F.grid_sample(env._glyph_pad.expand(B, -1, -1, -1), ggrid,
                         mode="bilinear", padding_mode="zeros", align_corners=True)
    galp = F.grid_sample(env._alpha_pad.expand(B, -1, -1, -1), ggrid,
                         mode="bilinear", padding_mode="zeros", align_corners=True)
    return env.bg.unsqueeze(1) * (1 - galp) + gval * galp
