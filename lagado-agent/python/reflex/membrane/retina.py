"""membrane/retina.py — RUNG 2: in-guest retina daemon (XDamage + per-rect reads → shm ring).

One copy, no protocol: subscribes to XDamage (the validated v2 recipe: version
handshake first, per-window attach, parser hardened against foreign X errors),
and for each damage rect reads JUST THAT RECT's pixels (XGetImage on the root —
local unix socket) and appends a record to a ring file in /dev/shm.

The host reads the same bytes with zero transport: for a container guest the
daemon's /dev/shm is host-visible at /proc/<pid>/root/dev/shm (container
processes ARE host processes); for a QEMU guest the same ring rides an ivshmem
device instead.

Record format (little-endian), append-only, truncates at RING_MAX:
  magic 'LGRT' | t f64 | x u16 | y u16 | w u16 | h u16 | payload w*h*4 BGRX
Runs until /dev/shm/lagado_retina_stop exists.
"""
import os
import struct
import time

RING = "/dev/shm/lagado_retina.ring"
STOP = "/dev/shm/lagado_retina_stop"
RING_MAX = 64 * 1024 * 1024
MAX_RECT_PX = 1920 * 1080     # sanity cap per record


def main():
    try:
        from Xlib.protocol import display as proto_display
        _orig = proto_display.Display.parse_error_response

        def _safe(self, request):
            try:
                return _orig(self, request)
            except AttributeError:
                return 0
        proto_display.Display.parse_error_response = _safe

        from Xlib import display, X
        from Xlib.ext import damage
    except Exception as e:
        open(RING, "wb").write(b"LGER" + str(e).encode()[:120])
        return
    d = display.Display(":0")
    d.set_error_handler(lambda *a: 0)
    d.damage_query_version()                     # REQUIRED handshake (field-fact)
    root = d.screen().root

    tracked = {}

    def attach_new():
        for w in list(root.query_tree().children):
            if w.id in tracked:
                continue
            try:
                at = w.get_attributes()
                if at.win_class != X.InputOutput or at.map_state != X.IsViewable:
                    continue
                org = w.translate_coords(root, 0, 0)
                w.damage_create(damage.DamageReportRawRectangles)
                tracked[w.id] = (-org.x, -org.y)
            except Exception:
                continue
        d.flush()

    attach_new()
    time.sleep(0.3)
    try:
        while d.pending_events():
            d.next_event()
    except Exception:
        pass

    f = open(RING, "wb")
    last_scan = time.time()
    while not os.path.exists(STOP):
        try:
            n = d.pending_events()
        except Exception:
            n = 0
        for _ in range(n):
            try:
                ev = d.next_event()
                a = ev.area
                ox, oy = tracked.get(getattr(ev, "drawable", None) and ev.drawable.id, (0, 0))
                x, y = int(a.x) + ox, int(a.y) + oy
                w, h = int(a.width), int(a.height)
                if w * h == 0 or w * h > MAX_RECT_PX:
                    continue
                # read JUST this rect from the root (local socket, no full frame)
                img = root.get_image(x, y, w, h, X.ZPixmap, 0xFFFFFFFF)
                payload = img.data if isinstance(img.data, bytes) else bytes(img.data, "latin-1")
                rec = b"LGRT" + struct.pack("<dHHHH", time.time(), x, y, w, h) + payload
                if f.tell() + len(rec) > RING_MAX:
                    f.close()
                    f = open(RING, "wb")          # wrap: reader re-syncs on magic
                f.write(rec)
                f.flush()
            except AttributeError:
                pass
            except Exception:
                break
        now = time.time()
        if now - last_scan >= 2.0:
            attach_new()
            last_scan = now
        time.sleep(0.01)
    f.close()


def read_ring(path, offset=0):
    """Host-side reader: yields (t, x, y, w, h, payload) records from `offset`;
    returns the new offset. Re-syncs on the magic if mid-record."""
    out = []
    with open(path, "rb") as f:
        f.seek(offset)
        buf = f.read()
    i = 0
    while True:
        j = buf.find(b"LGRT", i)
        if j < 0:
            break
        if len(buf) - j < 20:
            break
        t, x, y, w, h = struct.unpack("<dHHHH", buf[j + 4:j + 20])
        need = w * h * 4
        if len(buf) - j - 20 < need:
            break
        out.append((t, x, y, w, h, buf[j + 20:j + 20 + need]))
        i = j + 20 + need
    return out, offset + i


if __name__ == "__main__":
    main()
