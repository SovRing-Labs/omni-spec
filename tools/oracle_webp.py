#!/usr/bin/env python3
"""Black-box oracle for src/webp.rs (lossless VP8L): writes tests/data/webp/*.webp and tests/data/webp-oracle.tsv.

Pillow's libwebp encodes deterministic synthetic images losslessly (separate process; sources never read) at several
methods and with/without alpha; the reference is Pillow's own decode to RGBA8. Lossless must match exactly.
Row: <file> <width> <height> <FNV-1a 64 of RGBA8>.
Usage: PYTHONPATH=<site> python3 tools/oracle_webp.py
"""
import io
import math
import os
import random
import sys
from PIL import Image
import PIL
from PIL import features

sys.path.insert(0, os.path.dirname(__file__))
from oracle_inflate import fnv1a64

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
DIR = os.path.join(ROOT, "tests", "data", "webp")
TSV = os.path.join(ROOT, "tests", "data", "webp-oracle.tsv")


def content(w, h, seed, alpha, colors=12, noise=0.05):
    rng = random.Random(seed)
    img = Image.new("RGBA", (w, h))
    px = img.load()
    palette = [(rng.randint(0, 255), rng.randint(0, 255), rng.randint(0, 255), 255) for _ in range(colors)]
    for y in range(h):
        for x in range(w):
            k = (x // 5 + y // 3) % colors
            r, g, b, _ = palette[k]
            a = (255 if not alpha else (0 if (x + y) % 7 == 0 else 128 + int(100 * math.sin(x / 4.0))))
            if rng.random() < noise:
                r, g, b = rng.randint(0, 255), rng.randint(0, 255), rng.randint(0, 255)
            px[x, y] = (r, g, b, max(0, min(255, a)))
    return img


CASES = [
    ("lossless-1x1", 1, 1, False, 4),
    ("lossless-7x3", 7, 3, False, 4),
    ("lossless-33x17-m0", 33, 17, False, 0),
    ("lossless-33x17-m6", 33, 17, False, 6),
    ("lossless-64x48-alpha-m4", 64, 48, True, 4),
    ("lossless-40x40-alpha-m6", 40, 40, True, 6),
    ("lossless-100x30-m2", 100, 30, False, 2),
    ("lossless-17x65-alpha-m1", 17, 65, True, 1),
    ("palette16-64x20-m4", 64, 20, False, 4),
    ("palette2-37x9-m6", 37, 9, False, 6),
]
SPECIAL = {"palette16-64x20-m4": dict(colors=16, noise=0.0), "palette2-37x9-m6": dict(colors=2, noise=0.0)}


def main():
    os.makedirs(DIR, exist_ok=True)
    if not features.check("webp"):
        raise SystemExit("Pillow has no WebP support")
    rows = []
    for i, (name, w, h, alpha, method) in enumerate(CASES):
        src = content(w, h, 200 + i, alpha, **SPECIAL.get(name, {}))
        buf = io.BytesIO()
        src.save(buf, format="WEBP", lossless=True, method=method, exact=True)
        data = buf.getvalue()
        with open(os.path.join(DIR, name + ".webp"), "wb") as f:
            f.write(data)
        ref = Image.open(io.BytesIO(data))
        ref.load()
        rgba = ref.convert("RGBA")
        assert rgba.size == (w, h)
        rows.append(f"{name}.webp\t{w}\t{h}\t{fnv1a64(rgba.tobytes()):016x}")
    with open(TSV, "w", encoding="utf-8") as f:
        f.write("# webp lossless oracle: Pillow %s (libwebp); decoded RGBA8 FNV-1a 64\n" % PIL.__version__)
        f.write("\n".join(rows) + "\n")
    print(f"wrote {len(rows)} lossless files")


if __name__ == "__main__":
    main()
