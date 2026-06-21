"""
PROOF (advisor-demanded): take the currently-FAILING gimp palette task and drive it to a real
env.evaluate()==1.0 through the API/CLI layer — turning 'reachable' into 'scores 1.0'.
Accounts for the layer-revealed constraint: the evaluator exports from the RUNNING GIMP instance,
so we operate-on-file (sibling-CLI ImageMagick; gimp-headless+corrected-constant fallback) then RELOAD.
"""
import json, glob, logging
logging.basicConfig(level=logging.WARNING)
from desktop_env.desktop_env import DesktopEnv

task = json.load(open(sorted(glob.glob("evaluation_examples/examples/gimp/06ca5602*.json"))[0]))
env = DesktopEnv(provider_name="docker", action_space="pyautogui", screen_size=(1920, 1080),
                 headless=True, os_type="Ubuntu", require_a11y_tree=False)
env.reset(task_config=task)

def sh(cmd, timeout=180):
    py = ("import subprocess as _s, json as _j; r=_s.run(%r, shell=True, capture_output=True, text=True, timeout=%d); "
          "print(_j.dumps({'out': r.stdout[-400:], 'err': r.stderr[-400:], 'rc': r.returncode}))" % (cmd, timeout))
    res = env.controller.execute_python_command(py)
    raw = res.get("output", "") if isinstance(res, dict) else str(res)
    try:
        return json.loads(raw.strip().splitlines()[-1])
    except Exception:
        return {"out": raw, "err": "", "rc": -1}

F = "/home/user/Desktop/computer.png"
print("=== PROOF: gimp palette via operate-on-file + reload ===", flush=True)
print("pre  mode:", sh("python3 -c \"from PIL import Image; print(Image.open('%s').mode)\"" % F), flush=True)
print("kill gui :", sh("pkill -9 gimp; sleep 1; echo killed"), flush=True)

conv = sh("convert %s -colors 256 PNG8:%s && echo OK_IM" % (F, F))
print("convert(IM):", conv, flush=True)
if "OK_IM" not in (conv.get("out", "") + conv.get("err", "")):
    g = ("gimp -i -b '(let* ((img (car (gimp-file-load RUN-NONINTERACTIVE \"%s\" \"c\")))) "
         "(gimp-image-flatten img) "
         "(gimp-image-convert-indexed img CONVERT-DITHER-NONE CONVERT-PALETTE-GENERATE 256 FALSE FALSE \"\") "
         "(file-png-save RUN-NONINTERACTIVE img (car (gimp-image-get-active-drawable img)) \"%s\" \"c\" 0 9 1 1 1 1 1))' "
         "-b '(gimp-quit 0)'" % (F, F))
    print("convert(gimp-headless, corrected constant):", sh(g), flush=True)

print("post mode:", sh("python3 -c \"from PIL import Image; print(Image.open('%s').mode)\"" % F), flush=True)
print("reload   :", sh("DISPLAY=:0 setsid gimp %s >/dev/null 2>&1 & sleep 1; echo launched" % F), flush=True)
sh("sleep 14; echo settled")

score = env.evaluate() or 0.0
print("\n=== env.evaluate() => %s   (%s) ===" % (score, "PROVEN: FAIL->PASS" if score >= 1.0 else "still failing"), flush=True)
env.close()
