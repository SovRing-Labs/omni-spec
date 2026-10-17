#!/usr/bin/env python3
"""Black-box oracle for src/jpeg.rs: writes tests/data/jpeg/*.jpg, *.rgb and tests/data/jpeg-oracle.tsv.

The JPEG files are encoded by Pillow (libjpeg-turbo, separate process) from deterministic synthetic content, in
baseline and progressive form, grayscale, 4:4:4 and 4:2:0 chroma, odd sizes, and with restart markers. The reference
pixels are Pillow's own decode (RGB8; grayscale replicated). Row: <jpg> <w> <h> <subsampled 0|1> <gate kind>.
Gate: 4:4:4 and grayscale images must match within ±1 per sample; 4:2:0 is reported only (libjpeg's fancy chroma
upsampling is not reproduced).
Usage: PYTHONPATH=<site> python3 tools/oracle_jpeg.py
"""
import io
import math
import os
import random
from PIL import Image
import PIL

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
DIR = os.path.join(ROOT, "tests", "data", "jpeg")
TSV = os.path.join(ROOT, "tests", "data", "jpeg-oracle.tsv")


def content(w, h, seed, smooth=False):
    """Deterministic test content. smooth=True keeps chroma slowly varying, so 4:2:0 upsampling choices barely matter."""
    rng = random.Random(seed)
    img = Image.new("RGB", (w, h))
    px = img.load()
    for y in range(h):
        for x in range(w):
            if smooth:
                r = (x * 255 // max(1, w - 1)) % 256
                g = (y * 255 // max(1, h - 1)) % 256
                b = 128 + int(60 * math.sin((x + y) / 9.0))
            else:
                r = (x * 255 // max(1, w - 1) + rng.randint(-20, 20)) % 256
                g = (y * 255 // max(1, h - 1) + int(40 * math.sin(x / 3.0))) % 256
                b = ((x ^ y) * 3 + rng.randint(0, 30)) % 256
            px[x, y] = (r, g, b)
    return img


CASES = [
    # name, w, h, mode, kwargs
    ("base444-33x17", 33, 17, "RGB", dict(quality=85, subsampling=0)),
    ("base444-64x64", 64, 64, "RGB", dict(quality=90, subsampling=0)),
    ("prog444-40x23", 40, 23, "RGB", dict(quality=80, subsampling=0, progressive=True)),
    ("prog444-64x48", 64, 48, "RGB", dict(quality=70, subsampling=0, progressive=True)),
    ("base-gray-37x19", 37, 19, "L", dict(quality=85)),
    ("prog-gray-40x40", 40, 40, "L", dict(quality=75, progressive=True)),
    ("restart444-70x33", 70, 33, "RGB", dict(quality=85, subsampling=0, restart_marker_blocks=2)),
    ("restart-gray-50x25", 50, 25, "L", dict(quality=85, restart_marker_blocks=3)),
    ("base420-48x32", 48, 32, "RGB", dict(quality=85, subsampling=2)),
    ("prog420-35x21", 35, 21, "RGB", dict(quality=80, subsampling=2, progressive=True)),
]


def main():
    os.makedirs(DIR, exist_ok=True)
    rows = []
    for i, (name, w, h, mode, kw) in enumerate(CASES):
        src = content(w, h, 100 + i, smooth=kw.get("subsampling", None) == 2)
        if mode == "L":
            src = src.convert("L")
        buf = io.BytesIO()
        src.save(buf, format="JPEG", **kw)
        data = buf.getvalue()
        with open(os.path.join(DIR, name + ".jpg"), "wb") as f:
            f.write(data)
        ref = Image.open(io.BytesIO(data))
        ref.load()
        rgb = ref.convert("RGB")
        assert rgb.size == (w, h)
        with open(os.path.join(DIR, name + ".rgb"), "wb") as f:
            f.write(rgb.tobytes())
        subsampled = 1 if kw.get("subsampling", None) == 2 else 0
        gate = "report" if subsampled else "gate"
        rows.append(f"{name}.jpg\t{w}\t{h}\t{subsampled}\t{gate}")
    with open(TSV, "w", encoding="utf-8") as f:
        f.write("# jpeg oracle: Pillow %s (libjpeg-turbo); reference RGB8 in tests/data/jpeg/<name>.rgb\n" % PIL.__version__)
        f.write("# columns: jpg, width, height, subsampled 4:2:0 (1) or not (0), gate (gate = ±1 check, report = info only)\n")
        f.write("\n".join(rows) + "\n")
    print(f"wrote {len(rows)} cases to {DIR}")


if __name__ == "__main__":
    main()
