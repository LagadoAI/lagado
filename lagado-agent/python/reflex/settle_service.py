"""Settle-monitor serving process: the PROMOTED CfC (gate_report_PROMOTE_2026-07-06,
threshold=0.50, K_PATIENCE=3) behind a JSONL stdin/stdout protocol.

Protocol (one JSON object per line, one response line per request):
  {"op":"reset"}                          -> {"ok":true}
  {"op":"tick","feats":[52 floats],"dt":s} -> {"p":float,"settled":bool}

`settled` = threshold + patience applied ACROSS calls since the last reset (the
production fire rule: K_PATIENCE consecutive ticks with sigmoid > THRESHOLD).
Hidden state (hx) persists across ticks; reset clears hx and the streak.

Input scaling mirrors train_cfc.to_tensors exactly: the 49 pixel changed-fraction
dims are log10-scaled to [0,1]; the 3 window/proc dims pass through raw; dt is fed
to the CfC as `timespans` (the continuous-time channel — variable tick rate is fine).

Run under the reflex venv (torch+ncps):  reflex/.venv/bin/python settle_service.py
A malformed request answers {"error": ...} and the loop continues — the process
only exits on EOF. All non-protocol output goes to stderr.
"""
import json
import os
import sys

import numpy as np
import torch
from ncps.torch import CfC

# The promoted operating point (single source of truth for service + demo).
THRESHOLD = 0.50
K_PATIENCE = 3
INPUT_SIZE = 52
UNITS = 48
N_PIX = 49            # dims [0:49] are pixel changed-fractions -> log-scaled
MODEL_PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                          "settle_monitor_promoted.pt")


def load_model():
    model = CfC(input_size=INPUT_SIZE, units=UNITS, proj_size=1,
                batch_first=True, return_sequences=True)
    model.load_state_dict(torch.load(MODEL_PATH, map_location="cpu"))
    model.eval()
    return model


def scale_feats(raw):
    f = np.asarray(raw, dtype=np.float64)
    if f.shape != (INPUT_SIZE,):
        raise ValueError("feats must be %d floats, got shape %s" % (INPUT_SIZE, f.shape))
    f[:N_PIX] = (np.log10(f[:N_PIX] + 1e-6) + 6.0) / 6.0
    return torch.tensor(f, dtype=torch.float32).view(1, 1, INPUT_SIZE)


def main():
    torch.set_num_threads(1)          # 37k params; predictable single-tick latency
    model = load_model()
    hx = None
    streak = 0
    with torch.no_grad():             # warmup: first real tick answers in <10ms
        model(torch.zeros(1, 1, INPUT_SIZE), timespans=torch.full((1, 1), 0.3))
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            req = json.loads(line)
            op = req.get("op")
            if op == "reset":
                hx = None
                streak = 0
                resp = {"ok": True}
            elif op == "tick":
                x = scale_feats(req["feats"])
                dt = torch.full((1, 1), max(float(req.get("dt", 0.3)), 1e-3),
                                dtype=torch.float32)
                with torch.no_grad():
                    out, hx = model(x, hx=hx, timespans=dt)
                p = float(torch.sigmoid(out[0, -1, 0]))
                streak = streak + 1 if p > THRESHOLD else 0
                resp = {"p": round(p, 6), "settled": streak >= K_PATIENCE}
            else:
                resp = {"error": "unknown op: %r" % (op,)}
        except Exception as e:
            resp = {"error": "%s: %s" % (type(e).__name__, e)}
        sys.stdout.write(json.dumps(resp) + "\n")
        sys.stdout.flush()


if __name__ == "__main__":
    main()
