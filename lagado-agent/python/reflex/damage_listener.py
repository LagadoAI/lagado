"""In-guest X11 damage-event listener — membrane sensor v0 (VALIDATION ONLY).

Subscribes to XDamage on the root window: the X server PUSHES "this rectangle
changed" events — the change-signal itself, no polling, no capture, no compositor
performance required. This run it records alongside the episode recorder so its
events can be validated against the pixel/window channels (and against the traps
that ate 2026-07-06: does damage see the paints the captures missed?).

Writes /home/user/damage_log.jsonl: one line per second of activity:
  {"t": epoch_sec, "events": N, "area": total_damaged_px}
Runs until /home/user/damage_stop exists. Exits with a marker line if Xlib or the
damage extension is unavailable (validation deferred, never fatal).
"""
import json
import os
import time

OUT = "/home/user/damage_log.jsonl"
STOP = "/home/user/damage_stop"


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
    bucket_t = int(time.time())
    events = 0
    area = 0
    while not os.path.exists(STOP):
        while d.pending_events():
            ev = d.next_event()
            if ev.type == d.extension_event.DamageNotify[0] if hasattr(
                    d.extension_event, "DamageNotify") else False:
                pass
            try:
                a = ev.area
                events += 1
                area += a.width * a.height
            except AttributeError:
                pass
        now = int(time.time())
        if now != bucket_t:
            if events:
                f.write(json.dumps({"t": bucket_t, "events": events, "area": area}) + "\n")
                f.flush()
            bucket_t, events, area = now, 0, 0
        time.sleep(0.05)
    f.close()


if __name__ == "__main__":
    main()
