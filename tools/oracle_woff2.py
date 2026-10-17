#!/usr/bin/env python3
"""Black-box oracle for src/woff2.rs: writes tests/data/woff2-oracle.tsv.

Reference: fontTools' WOFF2 reader (a separate process; its source is never read). Rows, per public .woff2 file:
  F <file> <flavor-hex> <numTables>
  T <file> <tag> <length> <sha256-of-table-bytes>      (every table of the ORIGINAL font, glyf/loca/hmtx reconstructed)
The fonts are not copied into the tree (licences are theirs); the test reads them from FONT_DIR and skips what is absent.
Usage: PYTHONPATH=<site with fontTools+brotli> python3 tools/oracle_woff2.py [FONT_DIR]
"""
import glob, hashlib, os, sys
from fontTools.ttLib.woff2 import WOFF2Reader

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
OUT = os.path.join(ROOT, "tests", "data", "woff2-oracle.tsv")
DIR = sys.argv[1] if len(sys.argv) > 1 else "/usr/lib/claude-desktop/resources/ion-dist/assets/v1"

rows = []
for path in sorted(glob.glob(os.path.join(DIR, "*.woff2"))):
    name = os.path.basename(path)
    r = WOFF2Reader(open(path, "rb"))
    rows.append(f"F\t{name}\t{int.from_bytes(open(path, 'rb').read()[4:8], 'big'):x}\t{len(r.tables)}")
    for tag in sorted(r.keys()):
        data = r[tag]
        rows.append(f"T\t{name}\t{tag}\t{len(data)}\t{hashlib.sha256(data).hexdigest()}")
open(OUT, "w").write("\n".join(rows) + "\n")
print(f"{len(rows)} rows -> {OUT}")
