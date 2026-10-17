#!/usr/bin/env python3
"""Black-box oracle for src/gif.rs: writes tests/data/gif-oracle.tsv.

Sources: GIF files under /usr/share and /usr/lib (absolute paths; the Rust test skips absent files). Pillow (a
separate process) decodes frame 0 and converts to RGBA8. Pixels with alpha 0 are zeroed in the hash on both sides
(RGB under full transparency is undefined). Row: <path> <width> <height> <FNV-1a 64 of normalised RGBA8> <frames>.
Usage: python3 tools/oracle_gif.py
"""
import os
import sys
from PIL import Image
import PIL

sys.path.insert(0, os.path.dirname(__file__))
from oracle_inflate import fnv1a64

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
OUT = os.path.join(ROOT, "tests", "data", "gif-oracle.tsv")
LIMIT = 200


def normalise(rgba_bytes: bytes) -> bytes:
    b = bytearray(rgba_bytes)
    for i in range(0, len(b), 4):
        if b[i + 3] == 0:
            b[i] = b[i + 1] = b[i + 2] = 0
    return bytes(b)


def main():
    rows, n = [], 0
    for root in ("/usr/share", "/usr/lib"):
        for dirpath, _, names in os.walk(root):
            for name in names:
                if not name.lower().endswith(".gif"):
                    continue
                p = os.path.join(dirpath, name)
                try:
                    with open(p, "rb") as f:
                        head = f.read(6)
                    if head not in (b"GIF87a", b"GIF89a") or os.path.getsize(p) > 3_000_000:
                        continue
                    with Image.open(p) as im:
                        frames = getattr(im, "n_frames", 1)
                        im.seek(0)
                        rgba = im.convert("RGBA")
                        w, h = rgba.size
                        rows.append(f"{p}\t{w}\t{h}\t{fnv1a64(normalise(rgba.tobytes())):016x}\t{frames}")
                except Exception as e:
                    print("skip", p, e, file=sys.stderr)
                    continue
                n += 1
                if n >= LIMIT:
                    break
    with open(OUT, "w", encoding="utf-8") as f:
        f.write("# gif oracle: Pillow %s; frame 0 as RGBA8, alpha-0 RGB zeroed; columns: path, w, h, fnv1a64, frames\n" % PIL.__version__)
        f.write("\n".join(rows) + "\n")
    print(f"wrote {OUT}: {len(rows)} files")


if __name__ == "__main__":
    main()
