#!/usr/bin/env python3
"""Black-box oracle for src/brotli.rs: writes tests/data/brotli/*.br and tests/data/brotli-oracle.tsv.

The reference is the Brotli encoder/decoder of the `brotli` Python package (run as a separate process; its source is
never read). Each row: <stream file> <original length> <FNV-1a 64 of original>. Inputs are deterministic:
  text    first 48 KiB of tests/data/rfc7932.txt (dictionary-rich English + hex)
  bin     seeded pseudo-random bytes with runs (4 KiB) and a small structured binary (16 KiB)
  tiny    empty, 1 byte, short repeats
Streams are produced at qualities 0, 1, 5, 9, 11 and windows 10, 16, 22 where meaningful.
Usage: PYTHONPATH=<site> python3 tools/oracle_brotli.py
"""
import os
import random
import brotli

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
DATA = os.path.join(ROOT, "tests", "data")
OUTDIR = os.path.join(DATA, "brotli")
TSV = os.path.join(DATA, "brotli-oracle.tsv")


def fnv1a64(data: bytes) -> int:
    h = 0xCBF29CE484222325
    for b in data:
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def inputs():
    text = open(os.path.join(DATA, "rfc7932.txt"), "rb").read()[:48 * 1024]
    rng = random.Random(7932)
    noise = bytes(rng.getrandbits(8) for _ in range(4096))
    runs = b"".join(bytes([rng.getrandbits(8)]) * rng.randint(1, 300) for _ in range(200))[:16 * 1024]
    structured = b"".join(i.to_bytes(4, "little") + (i * 7).to_bytes(2, "big") for i in range(3000))[:16 * 1024]
    return [
        ("text", text, [(0, 16), (1, 16), (5, 16), (9, 22), (11, 22), (11, 10)]),
        ("noise", noise, [(0, 16), (5, 16), (11, 16)]),
        ("runs", runs, [(5, 16), (11, 22)]),
        ("structured", structured, [(5, 16), (11, 16)]),
        ("mixed", text + structured + runs, [(5, 22), (11, 22)]),
        ("empty", b"", [(5, 16)]),
        ("one", b"x", [(5, 16), (11, 16)]),
        ("repeats", b"abcabcabcabc" * 50, [(5, 16), (11, 16)]),
    ]


def main():
    os.makedirs(OUTDIR, exist_ok=True)
    rows = []
    for name, data, configs in inputs():
        for q, lgwin in configs:
            stream = brotli.compress(data, quality=q, lgwin=lgwin, mode=brotli.MODE_GENERIC)
            # The reference must round-trip; if it does not, the row is not an oracle.
            assert brotli.decompress(stream) == data, (name, q)
            fname = f"{name}-q{q}-w{lgwin}.br"
            with open(os.path.join(OUTDIR, fname), "wb") as f:
                f.write(stream)
            rows.append(f"{fname}\t{len(data)}\t{fnv1a64(data):016x}")
    with open(TSV, "w", encoding="utf-8") as f:
        f.write("# brotli oracle: python brotli %s; columns: stream file (tests/data/brotli/), original length, FNV-1a 64\n"
                % brotli.__version__ if hasattr(brotli, "__version__") else "# brotli oracle\n")
        f.write("\n".join(rows) + "\n")
    print(f"wrote {len(rows)} streams to {OUTDIR}")


if __name__ == "__main__":
    main()
