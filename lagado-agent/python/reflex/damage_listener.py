"""In-guest X11 damage-event listener — membrane sensor v2 (LIVE-VALIDATED 2026-07-08).

Subscribes to XDamage: the X server PUSHES "this rectangle changed" events — the
change-signal itself, no polling, no capture. The rects seed
`eyes.baseline.characterize_events(frames, rects=...)` (exact segmentation) and the
event *times* are the capture trigger (sampling during motion is what makes fast
scrolls characterizable; a fixed-Hz poll around a discrete jump sees two unrelated
frames). Live validation: a blinking cursor = one 1082x21 line-strip rect per blink;
each wheel burst = one full-viewport rect at the repaint instant.

Three field-facts this file encodes (each cost a debugging round):
  1. XDamage REQUIRES DamageQueryVersion before any other request — without it
     damage_create fails with an extension error python-Xlib can't even parse.
  2. Under a compositing WM, damage must attach PER top-level WINDOW (viewable,
     InputOutput). Root-window damage sees almost nothing (apps are redirected).
  3. Damage rects are WINDOW-relative — translate by the window's screen origin.

Writes /home/user/damage_log.jsonl, one line per 100ms bucket of activity, rects in
SCREEN coordinates:
  {"t": epoch_float, "rects": [[x,y,w,h], ...]}
Runs until /home/user/damage_stop exists. Exits with a marker line if Xlib or the
damage extension is unavailable (validation deferred, never fatal).
"""
import json
import os
import time

OUT = "/home/user/damage_log.jsonl"
STOP = "/home/user/damage_stop"
BUCKET_S = 0.1
RESCAN_S = 2.0               # pick up newly-mapped windows
MAX_RECTS_PER_BUCKET = 256   # a full-screen video floods; cap, keep counts honest


def main():
    try:
        # python-Xlib can CRASH parsing extension errors it doesn't model
        # (measured: BadRRCrtcError lacking .sequence_number inside pending_events).
        # A foreign error must never kill the sensor — patch the parser to drop them.
        from Xlib.protocol import display as proto_display
        _orig_parse = proto_display.Display.parse_error_response

        def _safe_parse(self, request):
            try:
                return _orig_parse(self, request)
            except AttributeError:
                return 0
        proto_display.Display.parse_error_response = _safe_parse

        from Xlib import display, X
        from Xlib.ext import damage
    except Exception as e:
        open(OUT, "w").write(json.dumps({"error": "xlib-missing", "detail": str(e)}) + "\n")
        return
    d = display.Display(":0")
    d.set_error_handler(lambda *a: 0)
    if not d.has_extension("DAMAGE"):
        open(OUT, "w").write(json.dumps({"error": "no-damage-ext"}) + "\n")
        return
    d.damage_query_version()          # REQUIRED handshake (field-fact 1)
    root = d.screen().root

    tracked = {}   # window id → (x, y) screen origin

    def attach_new():
        for w in list(root.query_tree().children):
            if w.id in tracked:
                continue
            try:
                at = w.get_attributes()
                if at.win_class != X.InputOutput or at.map_state != X.IsViewable:
                    continue
                g = w.get_geometry()
                org = w.translate_coords(root, 0, 0)   # window origin in root coords
                w.damage_create(damage.DamageReportRawRectangles)
                tracked[w.id] = (-org.x, -org.y, g.width, g.height)
            except Exception:
                continue
        d.flush()

    attach_new()
    time.sleep(0.3)
    try:                                # drain the initial full-drawable reports
        while d.pending_events():
            d.next_event()
    except Exception:
        pass

    f = open(OUT, "w")
    bucket_t = time.time()
    last_scan = bucket_t
    rects = []
    dropped = 0
    while not os.path.exists(STOP):
        try:
            n = d.pending_events()
        except Exception:
            n = 0
        for _ in range(n):
            try:
                ev = d.next_event()
                a = ev.area
                # field-fact 3: window-relative → screen coords via the damaged
                # drawable's origin (ev.drawable is the window the damage is on)
                ox, oy = 0, 0
                t = tracked.get(getattr(ev, "drawable", None) and ev.drawable.id)
                if t:
                    ox, oy = t[0], t[1]
                if len(rects) < MAX_RECTS_PER_BUCKET:
                    rects.append([int(a.x) + ox, int(a.y) + oy, int(a.width), int(a.height)])
                else:
                    dropped += 1
            except AttributeError:
                pass
            except Exception:
                break
        now = time.time()
        if now - bucket_t >= BUCKET_S:
            if rects:
                line = {"t": round(bucket_t, 3), "rects": rects}
                if dropped:
                    line["dropped"] = dropped
                f.write(json.dumps(line) + "\n")
                f.flush()
            bucket_t, rects, dropped = now, [], 0
        if now - last_scan >= RESCAN_S:
            attach_new()
            last_scan = now
        time.sleep(0.02)
    f.close()


if __name__ == "__main__":
    main()
