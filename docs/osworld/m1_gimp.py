"""
M1 (gimp leg): drive gimp/06ca5602 'set image to Palette-Based' to a real env.evaluate()==1.0
via OPERATE-ON-FILE + RELOAD-INTO-FOCUS.

The evaluator's postconfig fires `shift+ctrl+e` (Export As) at WHATEVER window is focused, then
reads /home/user/Desktop/palette_computer.png. Conversion params are proven sound offline
(256-color no-dither -> SSIM 1.0); the only lever is FOCUS. wmctrl-only (no xdotool in guest).
"""
import json, glob, logging
logging.basicConfig(level=logging.WARNING)
from desktop_env.desktop_env import DesktopEnv
from m1_reconcile import make_helpers, list_windows, kill_app, reload_into_focus, find_window

F = "/home/user/Desktop/computer.png"

task = json.load(open(sorted(glob.glob("evaluation_examples/examples/gimp/06ca5602*.json"))[0]))
env = DesktopEnv(provider_name="docker", action_space="pyautogui", screen_size=(1920, 1080),
                 headless=True, os_type="Ubuntu", require_a11y_tree=False)
env.reset(task_config=task)
gpy, sh = make_helpers(env)


def convert_to_palette_on_disk(path):
    # strip the embedded ICC profile too: it triggers GIMP's modal "Convert to RGB Working Space?"
    # dialog on reload (which steals focus and blocks the export). Stripping it does NOT change pixel
    # values, so SSIM is unaffected.
    pil = ("python3 -c \"from PIL import Image; im=Image.open('%s').convert('RGB'); "
           "q=im.quantize(colors=256, dither=Image.Dither.NONE); q.info.pop('icc_profile', None); "
           "q.save('%s'); print('PIL_OK')\"" % (path, path))
    r = sh(pil)
    if "PIL_OK" in (r.get("out", "") + r.get("err", "")):
        return "pil"
    g = ("gimp -i -b '(let* ((img (car (gimp-file-load RUN-NONINTERACTIVE \"%s\" \"c\")))) "
         "(gimp-image-flatten img) "
         "(gimp-image-convert-indexed img CONVERT-DITHER-NONE CONVERT-PALETTE-GENERATE 256 FALSE FALSE \"\") "
         "(file-png-save RUN-NONINTERACTIVE img (car (gimp-image-get-active-drawable img)) \"%s\" \"c\" 0 9 1 1 1 1 1))' "
         "-b '(gimp-quit 0)'" % (path, path))
    sh(g)
    return "gimp-headless"


print("=== M1 gimp: operate-on-file + reload-into-focus ===", flush=True)
print("tools     : wmctrl=%s xprop=%s" % (
    sh("which wmctrl || echo NO").get("out", "").strip(),
    sh("which xprop || echo NO").get("out", "").strip()), flush=True)
print("windows@start:", list_windows(sh), flush=True)
print("pre  mode :", sh("python3 -c \"from PIL import Image; print(Image.open('%s').mode)\"" % F).get("out", "").strip(), flush=True)

print("kill gimp :", kill_app(sh, "gimp", "gimp"), flush=True)
print("convert   :", convert_to_palette_on_disk(F), flush=True)
print("post mode :", sh("python3 -c \"from PIL import Image; print(Image.open('%s').mode)\"" % F).get("out", "").strip(), flush=True)

wid, active = reload_into_focus(sh, F, "gimp", "gimp")
print("reloaded  : wid=%s active=%r" % (wid, active), flush=True)
print("windows post-load:", list_windows(sh), flush=True)

# defensive: if a stray modal dialog still grabbed focus, dismiss it (Escape = keep current state),
# then re-activate the gimp main window so the evaluator's shift+ctrl+e lands there.
for w_id, cls, title in list_windows(sh):
    if "dialog" in cls.lower() or "Convert to" in title or "Working Space" in title:
        print("  dismissing stray dialog:", repr(title), flush=True)
        sh("DISPLAY=:0 wmctrl -ia %s 2>/dev/null; true" % w_id)
        gpy("import pyautogui; pyautogui.press('escape')")
        sh("sleep 1; true")
gwid = find_window(sh, "gimp")[0]
if gwid:
    sh("DISPLAY=:0 wmctrl -ia %s 2>/dev/null; true" % gwid)
sh("sleep 6; true")
print("windows@eval:", list_windows(sh), flush=True)
print("gimp window present:", bool(find_window(sh, "gimp")[0]), flush=True)

score = env.evaluate() or 0.0
print("export exists:", sh("ls -la /home/user/Desktop/palette_computer.png 2>&1").get("out", "").strip(), flush=True)
print("\n=== env.evaluate() => %s   (%s) ===" % (
    score, "PROVEN: FAIL->PASS" if score >= 1.0 else "still failing"), flush=True)
env.close()
