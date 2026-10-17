#!/usr/bin/env python3
"""Black-box oracle for src/png.rs: writes tests/data/png-oracle.tsv.

Sources: every PNG under projects/omni-render/corpus (49, all 8-bit RGB), plus up to 6 system PNGs per
(colour type, bit depth, interlace) class found under /usr/share (covers palette, gray+alpha, RGBA, 16-bit, Adam7).
Each image is decoded by Pillow (a separate process; never read from source) and converted to RGBA8.
Row: <path> <width> <height> <FNV-1a 64 of RGBA8> <colour type> <bit depth> <interlace>.
Paths under /usr/share are absolute; the Rust test skips rows whose file is absent.

Usage: python3 tools/oracle_png.py
"""
import glob
import os
import struct
import sys
from collections import defaultdict

from PIL import Image
import PIL

sys.path.insert(0, os.path.dirname(__file__))
from oracle_inflate import fnv1a64  # same FNV-1a 64 as the inflate oracle

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
OUT = os.path.join(ROOT, "tests", "data", "png-oracle.tsv")
CORPUS = os.path.abspath(os.path.join(ROOT, "..", "omni-render", "corpus"))
PER_CLASS = 6


def ihdr(path):
    with open(path, "rb") as f:
        d = f.read(29)
    if d[:8] != b"\x89PNG\r\n\x1a\n":
        return None
    _, _, bd, ct, _, _, il = struct.unpack(">IIBBBBB", d[16:29])
    return ct, bd, il


def system_sample():
    classes = defaultdict(list)
    n = 0
    for dirpath, _, names in os.walk("/usr/share"):
        for name in names:
            if not name.lower().endswith(".png"):
                continue
            p = os.path.join(dirpath, name)
            try:
                k = ihdr(p)
            except OSError:
                continue
            if k and len(classes[k]) < PER_CLASS and os.path.getsize(p) < 2_000_000:
                classes[k].append(p)
            n += 1
            if n > 20000:
                return classes
    return classes


def main():
    paths = sorted(glob.glob(os.path.join(CORPUS, "**", "*.png"), recursive=True))
    paths = [(os.path.relpath(p, CORPUS), p) for p in paths]
    for k, ps in sorted(system_sample().items()):
        paths += [(p, p) for p in ps]
    rows, failed = [], []
    for name, path in paths:
        try:
            k = ihdr(path)
            with Image.open(path) as im:
                im.load()
                rgba = im.convert("RGBA")
                w, h = rgba.size
                rows.append(f"{name}\t{w}\t{h}\t{fnv1a64(rgba.tobytes()):016x}\t{k[0]}\t{k[1]}\t{k[2]}")
        except Exception as e:
            failed.append(f"{name}: {e}")
    with open(OUT, "w", encoding="utf-8") as f:
        f.write("# png oracle: Pillow %s; columns: path, width, height, fnv1a64(RGBA8), colour type, bit depth, interlace\n" % PIL.__version__)
        f.write("\n".join(rows) + "\n")
    print(f"wrote {OUT}: {len(rows)} images; {len(failed)} not decodable by oracle")
    for x in failed[:10]:
        print("  ", x)


if __name__ == "__main__":
    main()
