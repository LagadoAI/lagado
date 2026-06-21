"""
Reusable reload-reconcile machinery for OSWorld tasks whose evaluator reconciles by
exporting/saving FROM THE LIVE APP INSTANCE (gimp shift+ctrl+e; calc activate_window+ctrl+s).

Spine: kill original instance -> operate-on-file (deterministic, correct on disk) -> relaunch the
file in its app -> wait for the window -> raise+activate it. Built on wmctrl only (the guest has
wmctrl but NOT xdotool); active-window verification uses xprop when present, else degrades to a
window-exists check.

All commands run IN THE GUEST via env.controller.execute_python_command.
"""
import json, re


def make_helpers(env):
    def gpy(code):
        res = env.controller.execute_python_command(code)
        return (res.get("output", "") if isinstance(res, dict) else str(res)).strip()

    def sh(cmd, timeout=120):
        py = ("import subprocess as _s, json as _j; r=_s.run(%r, shell=True, capture_output=True, text=True, timeout=%d); "
              "print(_j.dumps({'out': r.stdout[-800:], 'err': r.stderr[-800:], 'rc': r.returncode}))" % (cmd, timeout))
        raw = gpy(py)
        try:
            return json.loads(raw.splitlines()[-1])
        except Exception:
            return {"out": raw, "err": "", "rc": -1}

    return gpy, sh


def list_windows(sh):
    """[(wid, wm_class, title)] from wmctrl -lx (class-aware)."""
    out = sh("DISPLAY=:0 wmctrl -lx 2>/dev/null").get("out", "")
    res = []
    for line in out.splitlines():
        parts = line.split(None, 4)        # wid desktop class host title
        if len(parts) >= 5:
            res.append((parts[0], parts[2], parts[4]))
        elif len(parts) == 4:
            res.append((parts[0], parts[2], ""))
    return res


def find_window(sh, hint):
    """Match a window whose CLASS or TITLE contains hint (case-insensitive)."""
    h = hint.lower()
    for wid, cls, t in list_windows(sh):
        if h in cls.lower() or h in t.lower():
            return wid, t
    return None, None


def active_window_title(sh):
    """Active window title via xprop; None if xprop absent or no active window."""
    out = sh("DISPLAY=:0 xprop -root _NET_ACTIVE_WINDOW 2>/dev/null").get("out", "")
    m = re.search(r"0x[0-9a-fA-F]+", out)
    if not m:
        return None
    n = sh("DISPLAY=:0 xprop -id %s WM_NAME 2>/dev/null" % m.group(0)).get("out", "")
    mm = re.search(r'WM_NAME[^=]*=\s*"?(.*?)"?\s*$', n)
    return mm.group(1) if mm else None


def kill_app(sh, proc_name, title_hint, max_wait=25):
    """Kill instances by PROCESS NAME (name-based pkill/pgrep do NOT match the python wrapper that
    runs them, so no self-match), poll until BOTH the window is gone AND no matching process remains.
    Keep hammering each tick so a slow-to-die instance can't survive into the relaunch (which, for
    single-instance apps like GIMP, would make the relaunch ATTACH to the stale instance)."""
    for _ in range(max_wait):
        sh("pkill -9 %s 2>/dev/null; pkill -9 script-fu 2>/dev/null; true" % proc_name)
        nproc = (sh("pgrep -c %s; true" % proc_name).get("out", "").strip().splitlines() or ["0"])[-1]
        win_gone = not find_window(sh, title_hint)[0]
        if win_gone and nproc == "0":
            return True
        sh("sleep 1; true")
    return False


def reload_into_focus(sh, path, app_cmd, title_hint, ready_timeout=90, settle=4):
    """Relaunch file in app, wait for its window, raise+activate. Returns (wid, verified_active)."""
    sh("DISPLAY=:0 setsid %s %s >/dev/null 2>&1 & echo launched" % (app_cmd, path))
    wid = None
    for _ in range(ready_timeout):
        wid = find_window(sh, title_hint)[0]
        if wid:
            break
        sh("sleep 1; true")
    if not wid:
        return None, None
    sh("sleep %d; true" % settle)            # let the document finish loading
    sh("DISPLAY=:0 wmctrl -ia %s 2>/dev/null; true" % wid)
    sh("sleep 1; true")
    act = active_window_title(sh)
    verified = (act is not None and title_hint.lower() in act.lower())
    return wid, (act if verified else (act or "<xprop-absent>"))
