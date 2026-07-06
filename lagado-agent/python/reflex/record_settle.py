"""Record settle-reflex training episodes from the real OSWorld VM.

v2: capture runs IN-GUEST (guest_rec.py, deployed once, launched detached per
episode). The guest recorder samples ~6 Hz, computes features inline, and fires
its own stimulus at t=2 s — zero HTTP during the episode, so heavy app-launch
churn can no longer break capture (v1 lost every cold-launch episode to guest
screenshot-server dropouts). Host side only starts episodes, polls for .done
with retries, and pulls gzipped JSON results.

Run (OSWorld venv — needs desktop_env):
  DOCKER_HOST=unix:///run/user/1000/podman/podman.sock \
  PYTHONPATH=/home/alucard/projects/OSWorld \
  /home/alucard/projects/OSWorld/.venv/bin/python record_settle.py <out_dir> <rounds>
"""
import base64
import glob
import gzip
import json
import logging
import os
import sys
import time

import numpy as np

REFLEX_DIR = os.path.dirname(os.path.abspath(__file__))

for name in ("desktopenv", "desktopenv.env", "urllib3", "requests"):
    logging.getLogger(name).setLevel(logging.WARNING)

XLSX_POOL = sorted(glob.glob("/home/alucard/projects/OSWorld/cache/*/*.xlsx"))[:8]


def gpy(env, code, retries=6):
    for i in range(retries):
        try:
            r = env.controller.execute_python_command(code)
            if isinstance(r, dict) and r.get("status") == "success":
                return r.get("output", "")
            if isinstance(r, dict):
                return r.get("output", "") or ""
        except Exception:
            pass
        time.sleep(2.0)
    return ""


def push_file(env, src, dst):
    b64 = base64.b64encode(open(src, "rb").read()).decode()
    gpy(env, "import base64;open(%r,'wb').write(base64.b64decode(%r))" % (dst, b64))


def start_episode(env, name, duration, stim):
    gpy(env, "import subprocess;subprocess.Popen(['python3','/home/user/guest_rec.py',"
             "%r,%r,%r])" % (name, str(duration), stim))


def pull_episode(env, name, duration):
    """Wait for the guest .done marker, then pull the gzipped JSON."""
    path = "/home/user/reflex_out/%s.json" % name
    deadline = time.time() + duration + 40
    while time.time() < deadline:
        out = gpy(env, "import os;print(os.path.exists(%r + '.done'))" % path, retries=1)
        if "True" in out:
            break
        time.sleep(2.0)
    else:
        return None
    out = gpy(env, "import gzip,base64;"
                   "print(base64.b64encode(gzip.compress(open(%r,'rb').read())).decode())"
              % path, retries=12)
    for ln in reversed(out.splitlines()):
        ln = ln.strip()
        if ln and all(c.isalnum() or c in "+/=" for c in ln):
            try:
                return json.loads(gzip.decompress(base64.b64decode(ln)))
            except Exception:
                continue
    return None


def episodes_for_round(rnd):
    """(name, duration_s, stim) — stim '' = none, PYAUTO: = in-process pyautogui."""
    guest_x = "/home/user/reflex_%d.xlsx" % rnd
    eps = [("quiet", 14.0, "")]
    if XLSX_POOL:
        eps += [
            ("launch_calc", 75.0, "soffice --norestore --calc " + guest_x),
            ("type_in_calc", 16.0,
             "PYAUTO:pyautogui.write('reflex test 12345', interval=0.08)"),
            ("blink_idle", 14.0, ""),
            ("sidebar_toggle", 12.0, "PYAUTO:pyautogui.press('f5')"),
            ("scroll", 12.0, "PYAUTO:pyautogui.scroll(-8)"),
            # bracket patterns: the stim string sits in guest_rec's OWN argv, so a
            # plain `pkill -f soffice` matches the recorder and kills it mid-episode
            ("close_calc", 14.0, "pkill soffice"),
        ]
    eps += [("launch_gimp", 75.0, "gimp"), ("close_gimp", 14.0, "pkill gimp")]
    return eps


def main():
    out_dir = sys.argv[1] if len(sys.argv) > 1 else "/tmp/lagado_battery/reflex_data2"
    rounds = int(sys.argv[2]) if len(sys.argv) > 2 else 4
    os.makedirs(out_dir, exist_ok=True)

    from desktop_env.desktop_env import DesktopEnv
    env = DesktopEnv(provider_name="docker", action_space="pyautogui",
                     screen_size=(1920, 1080), headless=True, os_type="Ubuntu",
                     require_a11y_tree=False)
    n = 0
    try:
        env.reset()
        push_file(env, os.path.join(REFLEX_DIR, "guest_rec.py"), "/home/user/guest_rec.py")
        probe = gpy(env, "import numpy,pyautogui;print('DEPS-OK')")
        if "DEPS-OK" not in probe:
            raise RuntimeError("guest deps missing: %r" % probe[:200])
        gpy(env, 'import subprocess;subprocess.run("pkill soffice;pkill gimp",shell=True)')
        time.sleep(4)
        for rnd in range(rounds):
            if XLSX_POOL:
                push_file(env, XLSX_POOL[rnd % len(XLSX_POOL)],
                          "/home/user/reflex_%d.xlsx" % rnd)
            for name, dur, stim in episodes_for_round(rnd):
                ep_id = "ep%03d_%s" % (n, name)
                start_episode(env, ep_id, dur, stim)
                time.sleep(dur + 2)
                data = pull_episode(env, ep_id, 0)
                if not data or len(data.get("t", [])) < 8:
                    print("SKIP %s (no data)" % ep_id, flush=True)
                    continue
                feats = np.array(data["feats"], dtype=np.float32)
                np.savez_compressed(
                    os.path.join(out_dir, ep_id + ".npz"),
                    t=np.array(data["t"]), feats=feats,
                    t_stim=float(data["t_stim"]), name=name, rnd=rnd)
                hz = len(data["t"]) / max(data["t"][-1], 1e-6)
                peak = feats[1:, -1].max() if len(feats) > 1 else 0.0
                flag = ""
                if name.startswith(("launch_", "close_")) and peak < 0.05:
                    flag = "  ** FLAT-STIMULUS WARNING (peak %.4f) **" % peak
                print("%s  frames=%3d  %.1f Hz  t_stim=%.1f  peak=%.3f%s" %
                      (ep_id.ljust(24), len(data["t"]), hz, data["t_stim"], peak, flag),
                      flush=True)
                n += 1
            gpy(env, 'import subprocess;subprocess.run("pkill soffice;pkill gimp",shell=True)')
            time.sleep(4)
    finally:
        try:
            env.close()
        except Exception:
            pass
    print("RECORDED %d episodes -> %s" % (n, out_dir), flush=True)
    json.dump({"episodes": n}, open(os.path.join(out_dir, "meta.json"), "w"))


if __name__ == "__main__":
    main()
