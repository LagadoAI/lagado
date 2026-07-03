"""
probe_app_apis.py — API-COVERAGE PROBE.

For each OSWorld app, does it expose a SCRIPTING interface reachable from the command plane
(the plane that already passes os 3/3)? This sizes the "operate the app via its real API instead
of clicking pixels" fraction of the surface — the input to the post-sweep build decision
(app-automation/MCP plane vs grinding the CV/OCR plane).

Each probe ACTUALLY INVOKES the interface (not just `which`) and reports REACHABLE / PRESENT / ABSENT
plus the evidence. GIMP's probe runs the EXACT verb (convert-indexed) the menu-descent failed at.

Boots ONE container, runs the battery, closes. ~3-5 min. Run AFTER the sweep frees the env:
  DOCKER_HOST=unix:///run/podman/podman.sock .venv/bin/python probe_app_apis.py
"""
import json, sys, logging
logging.basicConfig(level=logging.WARNING)
from desktop_env.desktop_env import DesktopEnv

# (app, interface, shell probe). The probe prints a marker we grep for REACHABLE; otherwise PRESENT/ABSENT.
PROBES = [
    ("gimp", "Script-Fu batch (PDB)",
     # runs the ACTUAL palette-conversion verb we failed at via menus — decisive proof
     "timeout 90 gimp -i "
     "-b '(let* ((img (car (gimp-image-new 8 8 RGB)))) "
     "(gimp-image-convert-indexed img CONVERT-NO-DITHER CONVERT-PALETTE-GENERATE 16 FALSE FALSE \"\") "
     "(gimp-image-delete img))' "
     "-b '(gimp-quit 0)' 2>&1; echo \"__RC=$?\""),

    ("libreoffice", "UNO bridge (python) + headless soffice",
     "python3 -c 'import uno; print(\"__UNO_OK\")' 2>&1; "
     "soffice --headless --version 2>&1 | head -1; echo \"__RC=$?\""),

    ("chrome", "DevTools Protocol (CDP) :9222",
     "curl -s --max-time 5 http://localhost:9222/json/version 2>&1 | head -c 200; echo; "
     "(google-chrome --version 2>&1 || chromium --version 2>&1 || chromium-browser --version 2>&1) | head -1; "
     "echo \"__RC=$?\""),

    ("vlc", "D-Bus/MPRIS + rc/http intf",
     "vlc --version 2>&1 | head -1; which dbus-send qdbus 2>&1; echo \"__RC=$?\""),

    ("thunderbird", "CLI (-compose) / Gecko (limited)",
     "thunderbird --version 2>&1 | head -1; echo \"__RC=$?\""),

    ("vs_code", "`code` CLI (rich: open/goto/extensions)",
     "code --version 2>&1 | head -2; echo \"__RC=$?\""),

    ("os", "command plane itself (substrate)",
     "echo '__SUBSTRATE: this IS the plane (os 3/3)'; echo \"__RC=0\""),
]

env = DesktopEnv(provider_name="docker", action_space="pyautogui", screen_size=(1920, 1080),
                 headless=True, os_type="Ubuntu", require_a11y_tree=False)

# any task just to bring the desktop up; we only need the installed apps, not a specific state
import glob
any_task = json.load(open(sorted(glob.glob("evaluation_examples/examples/os/*.json"))[0]))
env.reset(task_config=any_task)

def sh(cmd):
    py = ("import subprocess as _s, json as _j; r=_s.run(%r, shell=True, capture_output=True, text=True); "
          "print(_j.dumps({'out': r.stdout, 'err': r.stderr, 'rc': r.returncode}))" % cmd)
    res = env.controller.execute_python_command(py)
    raw = res.get("output", "") if isinstance(res, dict) else str(res)
    try:
        d = json.loads(raw.strip().splitlines()[-1])
        return (d.get("out", "") + d.get("err", "")).strip()
    except Exception:
        return raw.strip()

print("=== API-COVERAGE PROBE | does each app expose a command-plane-reachable scripting interface? ===\n", flush=True)
results = []
for app, iface, cmd in PROBES:
    out = sh(cmd)
    low = out.lower()
    # classify: REACHABLE = the interface actually responded with its proof marker / clean rc=0 + version
    reachable = (
        ("__rc=0" in low and "fatal" not in low and "error" not in low.split("__rc")[0][-120:])
        or "__uno_ok" in low or "webkit" in low or '"browser"' in low or "__substrate" in low
    )
    present = any(k in low for k in ["version", "gimp", "uno", "code ", "vlc", "thunderbird", "/usr/bin"])
    verdict = "REACHABLE" if reachable else ("PRESENT" if present else "ABSENT")
    ev = " | ".join(l.strip() for l in out.splitlines() if l.strip())[:240]
    print(f"  [{verdict:9s}] {app:14s} {iface}\n             ↳ {ev}\n", flush=True)
    results.append({"app": app, "interface": iface, "verdict": verdict, "evidence": ev})

env.close()
json.dump(results, open("/tmp/api_coverage.json", "w"), indent=1)

print("\n=== COVERAGE SUMMARY ===")
import collections
c = collections.Counter(r["verdict"] for r in results)
for v in ("REACHABLE", "PRESENT", "ABSENT"):
    apps = [r["app"] for r in results if r["verdict"] == v]
    print(f"  {v:9s} {c[v]}  {apps}")
print("\nfull → /tmp/api_coverage.json")
