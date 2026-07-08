"""membrane/fb_mmap.py — RUNG 3: zero-copy framebuffer reads from QEMU guest RAM.

With `-object memory-backend-file,share=on` QEMU backs guest RAM with a host-
mmappable file. virtio-gpu allocates its scanout resource in GUEST RAM, so the
pixels the guest is displaying exist in that mapping — readable by the host with
NO copies, NO protocol, NO guest cooperation, at any rate.

Localization needs no cooperation either: take ONE QMP screendump as reference,
scan the mmap for its distinctive rows (trying pixel-format permutations), lock
the offset + stride, then verify the mapping TRACKS live changes (change the
screen, compare mmap bytes against a fresh screendump). After lock-in, every
read is `bytes(view[off : off + h*stride])` — the membrane at the pixel layer.

Usage (guest already booted with shared RAM + QMP socket):
    python fb_mmap.py locate  <ram_file> <screendump.png>
    python fb_mmap.py read    <ram_file> <offset> <W> <H> <stride> <out.png>
"""
import mmap
import sys

import numpy as np


def load_png(path):
    from PIL import Image
    img = Image.open(path).convert("RGB")
    return np.asarray(img, dtype=np.uint8)


def candidate_rows(ref):
    """Byte patterns for one distinctive reference row under common formats."""
    H, W, _ = ref.shape
    # pick the row with maximal variance (avoid flat rows that match everywhere)
    row_var = ref.astype(np.int32).var(axis=(1, 2))
    y = int(np.argmax(row_var))
    row = ref[y]
    pats = {}
    r, g, b = row[:, 0], row[:, 1], row[:, 2]
    zero = np.zeros_like(r)
    ff = np.full_like(r, 255)
    for name, chans in [("BGRX", (b, g, r, zero)), ("BGRA", (b, g, r, ff)),
                        ("RGBX", (r, g, b, zero)), ("RGBA", (r, g, b, ff))]:
        pats[name] = np.stack(chans, axis=1).astype(np.uint8).tobytes()
    return y, pats


def locate(ram_path, png_path):
    ref = load_png(png_path)
    H, W, _ = ref.shape
    y, pats = candidate_rows(ref)
    with open(ram_path, "rb") as f:
        mm = mmap.mmap(f.fileno(), 0, prot=mmap.PROT_READ)
        for name, pat in pats.items():
            # search on a coarse probe first (first 64 bytes) then confirm full row
            probe = pat[:64]
            pos = mm.find(probe)
            while pos >= 0:
                if mm[pos:pos + len(pat)] == pat:
                    stride = W * 4
                    off = pos - y * stride
                    if off >= 0:
                        print(f"LOCATED format={name} fb_offset={off} stride={stride} "
                              f"(row {y} at {pos})")
                        mm.close()
                        return off, stride, name
                pos = mm.find(probe, pos + 1)
        mm.close()
    print("NOT-FOUND: framebuffer rows not present in shared RAM "
          "(VRAM-backed display? use virtio-vga, or blob=off)")
    return None


def read(ram_path, off, W, H, stride, out_png, fmt="BGRX"):
    with open(ram_path, "rb") as f:
        mm = mmap.mmap(f.fileno(), 0, prot=mmap.PROT_READ)
        raw = np.frombuffer(mm[off:off + H * stride], dtype=np.uint8).reshape(H, stride // 4, 4)[:, :W]
        mm.close()
    if fmt.startswith("BGR"):
        rgb = raw[:, :, [2, 1, 0]]
    else:
        rgb = raw[:, :, [0, 1, 2]]
    from PIL import Image
    Image.fromarray(rgb, "RGB").save(out_png)
    print(f"read {W}x{H} from offset {off} -> {out_png}")


if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "locate":
        locate(sys.argv[2], sys.argv[3])
    elif cmd == "read":
        read(sys.argv[2], int(sys.argv[3]), int(sys.argv[4]), int(sys.argv[5]),
             int(sys.argv[6]), sys.argv[7])
