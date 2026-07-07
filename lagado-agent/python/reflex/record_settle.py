"""Record settle-monitor training episodes from the real OSWorld VM — v8, INTEGRATED.

v8 rides the existing session plane instead of reinventing perception (2026-07-06
standing rule): the UNO daemon owns LibreOffice's lifecycle — `open` loads the doc
(headless, app-truth timing), `reconcile {gui:true}` raises the visible window (the
production-real "app appears" moment), and each UNO call's synchronous return is the
teaching-oracle timestamp for labels. The in-guest recorder (guest_rec.py v8) records
multi-channel: pixels (one voter) + window-list + process counts. GIMP episodes stay
shell-launched: the no-back-door class that hindsight labels must cover.

Run (OSWorld venv — needs desktop_env):
  DOCKER_HOST=unix:///run/user/1000/podman/podman.sock \
  PYTHONPATH=/home/alucard/projects/OSWorld RAM_SIZE=4G \
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
OSW_DIR = "/home/alucard/projects/lagado/lagado-agent/python/osworld"
sys.path.insert(0, OSW_DIR)

for name in ("desktopenv", "desktopenv.env", "urllib3", "requests"):
    logging.getLogger(name).setLevel(logging.WARNING)

XLSX_POOL = sorted(glob.glob("/home/alucard/projects/OSWorld/cache/*/*.xlsx"),
                   key=os.path.getsize)[:8]   # smallest first


def gpy(env, code, retries=6):
    for _ in range(retries):
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


def start_episode(env, name, duration, stim, stim_at=2.0):
    gpy(env, "import subprocess;subprocess.Popen(['python3','/home/user/guest_rec.py',"
             "%r,%r,%r,'',%r])" % (name, str(duration), stim, str(stim_at)))


def pull_episode(env, name, duration):
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
    """(name, duration_s, stim). UNO episodes carry the teaching oracle; PYAUTO ones
    exercise in-window churn; SHELL gimp is the no-back-door class; quiet/blink are
    negatives. close_visible's bracket pattern cannot match guest_rec's own argv,
    and it kills only the isolated-profile GUI instance, never the daemon's."""
    # v9 (2026-07-06, post-brutal-suite): stim times RANDOMIZED per episode — the v1 corpus
    # fired every stimulus at exactly t=2.0s, so elapsed-time predicted the hindsight label
    # perfectly and the CfC learned a clock (caught by the churn test + replay). Tail time
    # (duration after stim) keeps each episode's original budget. Seeded per round.
    import random
    rng = random.Random(1000 + rnd)
    guest_x = "/home/user/reflex_%d.xlsx" % rnd
    SCROLLER = ('SHELL:DISPLAY=:0 nohup gnome-terminal --geometry=60x20+700+300 '
                '--title=CHURNWIN -- bash -c "while true; do date +%s.%N; sleep 0.05; '
                'done" >/dev/null 2>&1 &')
    base = [
        ("quiet", 14.0, ""),
        ("uno_open", 40.0, 'UNO:open:{"file": "%s"}' % guest_x),
        ("uno_reload", 50.0, 'UNO:reconcile:{"gui": true}'),
        ("type_in_calc", 16.0,
         "PYAUTO:pyautogui.write('reflex test 12345', interval=0.08)"),
        ("blink_idle", 14.0, ""),
        ("sidebar_toggle", 12.0, "PYAUTO:pyautogui.press('f5')"),
        ("scroll", 12.0, "PYAUTO:pyautogui.scroll(-8)"),
        ("close_visible", 14.0, "SHELL:pkill -f lagado_reconcile_gui_profil[e]"),
        ("uno_close", 14.0, 'UNO:close:{}'),
        ("launch_gimp", 120.0, "SHELL:gimp"),
        ("close_gimp", 14.0, "SHELL:pkill gimp"),
        # TIMER-KILLER (v9): sustained scroller churn — the correct behavior is to NEVER
        # declare settle inside the window; every constant timer false-settles here.
        ("scroller_churn", 30.0, SCROLLER),
        # settle only AFTER the churn source dies (bracket pattern: cannot match this
        # recorder's own argv, only the scroller's bash -c command line).
        ("scroller_stop", 20.0, "SHELL:pkill -f 'date [+]%s.%N'"),
    ]
    out = []
    for (name, tail, stim) in base:
        at = round(rng.uniform(2.0, 12.0), 1)
        out.append((name, round(at + tail, 1), stim, at))
    return out


def main():
    out_dir = sys.argv[1] if len(sys.argv) > 1 else "/tmp/lagado_battery/reflex_data8"
    rounds = int(sys.argv[2]) if len(sys.argv) > 2 else 4
    os.makedirs(out_dir, exist_ok=True)

    from desktop_env.desktop_env import DesktopEnv
    from run_session_task import Guest, deploy_daemon, pick_uno_python
    env = DesktopEnv(provider_name="docker", action_space="pyautogui",
                     screen_size=(1920, 1080), headless=True, os_type="Ubuntu",
                     require_a11y_tree=False)
    n = 0
    try:
        env.reset()
        g = Guest(env)
        unopy = pick_uno_python(g)
        # membrane sensor v0 rides along for VALIDATION (never a monitor input yet)
        push_file(env, os.path.join(REFLEX_DIR, "damage_listener.py"),
                  "/home/user/damage_listener.py")
        gpy(env, "import subprocess;subprocess.Popen(['python3','/home/user/damage_listener.py'])")
        push_file(env, os.path.join(REFLEX_DIR, "guest_rec.py"), "/home/user/guest_rec.py")
        probe = gpy(env, "import numpy,pyautogui;print('DEPS-OK')")
        if "DEPS-OK" not in probe:
            raise RuntimeError("guest deps missing: %r" % probe[:200])
        gpy(env, 'import subprocess;subprocess.run("pkill gimp",shell=True)')
        time.sleep(3)
        for rnd in range(rounds):
            # fresh session per round: uno_close ends the resident soffice's last
            # doc and the daemon dies with it (found the hard way in v8full round 1)
            deploy_daemon(g, unopy)
            push_file(env, XLSX_POOL[rnd % len(XLSX_POOL)],
                      "/home/user/reflex_%d.xlsx" % rnd)
            for name, dur, stim, stim_at in episodes_for_round(rnd):
                ep_id = "ep%03d_%s" % (n, name)
                start_episode(env, ep_id, dur, stim, stim_at)
                time.sleep(dur + 2)
                data = pull_episode(env, ep_id, 0)
                if not data or len(data.get("t", [])) < 8:
                    print("SKIP %s (no data)" % ep_id, flush=True)
                    continue
                feats = np.array(data["feats"], dtype=np.float32)
                np.savez_compressed(
                    os.path.join(out_dir, ep_id + ".npz"),
                    t=np.array(data["t"]), feats=feats,
                    t_stim=float(data["t_stim"]),
                    t_stim_done=float(data.get("t_stim_done", -1.0)),
                    stim_ok=json.dumps(data.get("stim_ok")),
                    dropped=np.array(data.get("dropped", []), dtype=np.float32),
                    name=name, rnd=rnd)
                hz = len(data["t"]) / max(data["t"][-1], 1e-6)
                px_peak = feats[1:, 48].max() if len(feats) > 1 else 0.0
                win_events = int(feats[:, 49].sum()) if feats.shape[1] > 49 else -1
                oracle = ""
                if float(data.get("t_stim_done", -1)) > 0:
                    oracle = "  oracle=[%.1f..%.1f]s ok=%s" % (
                        data["t_stim"], data["t_stim_done"], data.get("stim_ok"))
                print("%s  frames=%3d %.1fHz  px_peak=%.3f win_ev=%d%s" %
                      (ep_id.ljust(26), len(data["t"]), hz, px_peak, win_events, oracle),
                      flush=True)
                n += 1
            gpy(env, 'import subprocess;subprocess.run("pkill gimp",shell=True)')
            time.sleep(3)
    finally:
        try:
            env.close()
        except Exception:
            pass
    gpy(env, "open('/home/user/damage_stop','w').write('1')")
    dmg = gpy(env, "print(open('/home/user/damage_log.jsonl').read()[:20000])", retries=3)
    if dmg.strip():
        open(os.path.join(out_dir, "damage_log.jsonl"), "w").write(dmg)
        print("damage-log pulled (%d bytes)" % len(dmg), flush=True)
    print("RECORDED %d episodes -> %s" % (n, out_dir), flush=True)
    json.dump({"episodes": n}, open(os.path.join(out_dir, "meta.json"), "w"))


if __name__ == "__main__":
    main()
