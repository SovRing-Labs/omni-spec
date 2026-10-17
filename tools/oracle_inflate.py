#!/usr/bin/env python3
"""Black-box oracle for src/inflate.rs: writes tests/data/inflate-oracle.tsv.

For every PNG under projects/omni-render/corpus, concatenates the IDAT chunks (the zlib stream) and records what the
reference zlib (python3 zlib, run as a separate process) produces: <png path> <output length> <FNV-1a 64 of output>.
FNV-1a is used so the fixture stays small while still pinning every output byte.

Usage: python3 tools/oracle_inflate.py
"""
import glob
import os
import struct
import zlib

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
OUT = os.path.join(ROOT, "tests", "data", "inflate-oracle.tsv")
CORPUS = os.path.abspath(os.path.join(ROOT, "..", "omni-render", "corpus"))


def fnv1a64(data: bytes) -> int:
    h = 0xCBF29CE484222325
    for b in data:
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def idat(png: bytes) -> bytes:
    """Concatenate IDAT chunk payloads of a PNG (PNG chunk layout: length, type, data, CRC)."""
    pos, out = 8, []
    while pos + 8 <= len(png):
        (n,) = struct.unpack(">I", png[pos:pos + 4])
        typ = png[pos + 4:pos + 8]
        if typ == b"IDAT":
            out.append(png[pos + 8:pos + 8 + n])
        pos += 12 + n
    return b"".join(out)


def main():
    rows = []
    for path in sorted(glob.glob(os.path.join(CORPUS, "**", "*.png"), recursive=True)):
        with open(path, "rb") as f:
            png = f.read()
        stream = idat(png)
        if not stream:
            continue
        try:
            out = zlib.decompress(stream)
        except zlib.error:
            continue
        rel = os.path.relpath(path, CORPUS)
        rows.append(f"{rel}\t{len(stream)}\t{len(out)}\t{fnv1a64(out):016x}")
    with open(OUT, "w", encoding="utf-8") as f:
        f.write("# inflate oracle: python3 zlib %s; columns: png (relative to corpus), idat_len, out_len, fnv1a64(out)\n" % zlib.ZLIB_RUNTIME_VERSION)
        f.write("\n".join(rows) + "\n")
    print(f"wrote {OUT}: {len(rows)} streams")


if __name__ == "__main__":
    main()
