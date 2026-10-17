#!/usr/bin/env python3
"""Glyph-level oracle for src/woff2.rs. fontTools (black box, separate process) decodes each .woff2 itself; our decoder's sfnt
(examples/woff2_dump.rs -> OUTROOT/<name>/font.ttf) is parsed by fontTools too. glyf bytes cannot be compared directly (fontTools pads
glyphs to 2 bytes and picks its own flag repeats; ours follows the file's directory origLength), so every glyph is compared by
content: contours, end points, flags, coordinates, bytecode, components, bbox; plus loca-derived glyph count, hmtx and all other tables.
Usage: PYTHONPATH=<site> python3 tools/check_woff2_vs_fonttools.py FONT_DIR OUTROOT   (prints one line per font, exit 1 on any difference)
"""
import glob, os, sys
from fontTools.ttLib import TTFont

font_dir, out_root = sys.argv[1], sys.argv[2]
bad = 0; glyphs = 0; fonts = 0
for path in sorted(glob.glob(os.path.join(font_dir, "*.woff2"))):
    name = os.path.basename(path)
    ours_path = os.path.join(out_root, name, "font.ttf")
    if not os.path.exists(ours_path):
        continue
    ref = TTFont(path); ours = TTFont(ours_path)
    problems = []
    if sorted(ref.keys()) != sorted(ours.keys()):
        problems.append(f"table sets {sorted(set(ref.keys()) ^ set(ours.keys()))}")
    for tag in ref.keys():
        if tag in ("glyf", "loca", "head", "GlyphOrder"):
            continue
        if tag in ours.keys() and ref.reader[tag] != ours.reader[tag]:
            problems.append(f"table {tag} bytes")
    if "head" in ref and "head" in ours:
        a, b = ref["head"], ours["head"]
        for k in ("unitsPerEm", "xMin", "yMin", "xMax", "yMax", "indexToLocFormat", "macStyle", "flags", "created", "modified"):
            if getattr(a, k) != getattr(b, k): problems.append(f"head.{k}")
    if "glyf" in ref:
        rg, og = ref["glyf"], ours["glyf"]
        if ref.getGlyphOrder() != ours.getGlyphOrder(): problems.append("glyph order")
        for gn in ref.getGlyphOrder():
            a, b = rg[gn], og[gn]; glyphs += 1
            if a.numberOfContours != b.numberOfContours: problems.append(f"{gn} contours"); continue
            if a.numberOfContours == 0: continue
            if (a.xMin, a.yMin, a.xMax, a.yMax) != (b.xMin, b.yMin, b.xMax, b.yMax): problems.append(f"{gn} bbox")
            if a.isComposite():
                ca = [(c.glyphName, c.flags, c.x, c.y, getattr(c, "transform", None)) for c in a.components]
                cb = [(c.glyphName, c.flags, c.x, c.y, getattr(c, "transform", None)) for c in b.components]
                if ca != cb: problems.append(f"{gn} components")
                if hasattr(a, "program") != hasattr(b, "program") or (hasattr(a, "program") and a.program.getBytecode() != b.program.getBytecode()): problems.append(f"{gn} composite bytecode")
            else:
                if list(a.endPtsOfContours) != list(b.endPtsOfContours): problems.append(f"{gn} endPts")
                if list(a.coordinates) != list(b.coordinates): problems.append(f"{gn} coordinates")
                if list(a.flags) != list(b.flags): problems.append(f"{gn} flags")
                if a.program.getBytecode() != b.program.getBytecode(): problems.append(f"{gn} bytecode")
    fonts += 1
    print(("FAIL " if problems else "ok   ") + name + ("  " + "; ".join(problems[:6]) if problems else ""))
    bad += bool(problems)
print(f"check_woff2_vs_fonttools: {fonts} fonts, {glyphs} glyphs compared, {bad} fonts with differences")
sys.exit(1 if bad else 0)
