"""In-guest X11 damage-event listener — membrane sensor v1 (RECTS).

Subscribes to XDamage on the root window: the X server PUSHES "this rectangle
changed" events — the change-signal itself, no polling, no capture, no compositor
performance required. v1 logs the RECTANGLES (v0 only counted them): these rects
seed `eyes.baseline.characterize_events(frames, rects=...)` — exact per-region
segmentation for free, and the push cadence tells the recorder WHEN to capture
(sampling during motion is what makes fast scrolls characterizable at all; a 4Hz
poll around a discrete jump sees two unrelated frames).

Writes /home/user/damage_log.jsonl, one line per 100ms bucket of activity:
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
MAX_RECTS_PER_BUCKET = 256   # a full-screen video repaint floods; cap, keep counts honest


def main():
    try:
        from Xlib import display
        from Xlib.ext import damage
    except Exception as e:
        open(OUT, "w").write(json.dumps({"error": "xlib-missing", "detail": str(e)}) + "\n")
        return
    d = display.Display(":0")
    if not d.has_extension("DAMAGE"):
        open(OUT, "w").write(json.dumps({"error": "no-damage-ext"}) + "\n")
        return
    root = d.screen().root
    root.damage_create(damage.DamageReportRawRectangles)
    d.flush()
    f = open(OUT, "w")
    bucket_t = time.time()
    rects = []
    dropped = 0
    while not os.path.exists(STOP):
        while d.pending_events():
            ev = d.next_event()
            try:
                a = ev.area
                if len(rects) < MAX_RECTS_PER_BUCKET:
                    rects.append([int(a.x), int(a.y), int(a.width), int(a.height)])
                else:
                    dropped += 1
            except AttributeError:
                pass
        now = time.time()
        if now - bucket_t >= BUCKET_S:
            if rects:
                line = {"t": round(bucket_t, 3), "rects": rects}
                if dropped:
                    line["dropped"] = dropped
                f.write(json.dumps(line) + "\n")
                f.flush()
            bucket_t, rects, dropped = now, [], 0
        time.sleep(0.02)
    f.close()


if __name__ == "__main__":
    main()
