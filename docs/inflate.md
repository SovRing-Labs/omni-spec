# inflate — DEFLATE (RFC 1951) and zlib (RFC 1950)

- **Spec:** RFC 1951 §3.2.2–3.2.7 (canonical codes, stored/fixed/dynamic blocks, length/distance tables), §3.1.1 bit order; RFC 1950 §2.2 (header), §8.2 (Adler-32). https://www.rfc-editor.org/rfc/rfc1951.txt , …/rfc1950.txt
- **Code:** `src/inflate.rs` — `inflate`, `inflate_bounded`, `inflate_raw` (returns bytes consumed), `zlib_decompress(_bounded)`, `adler32`. Output cap `MAX_OUTPUT` = 256 MiB by default.
- **Conformance:**
  - Corpus: 49 PNG IDAT zlib streams under `projects/omni-render/corpus` (IDAT concatenated) decompress to the same length and FNV-1a 64 as python3 zlib (black-box, `tools/oracle_inflate.py` → `tests/data/inflate-oracle.tsv`): **49 / 49 match**.
  - RFC stored and fixed examples; unit tests for Adler-32, stored LEN/NLEN, reserved BTYPE, zlib header checks.
  - Robustness: 2,000 bit-flipped/truncated real streams and 20,000 random short inputs run with no panic (each returns Ok or Err).
- **Known gaps:** no preset dictionary (FDICT → `PresetDictionary` error); no gzip wrapper (RFC 1952 not in scope); incomplete Huffman code sets are accepted (zlib accepts the same case); the decoder is bit-serial (correct, not optimised); the test profile is built at opt-level 3 so the corpus test runs in ~10 s.

- **Patent screen:** DEFLATE (RFC 1951) / zlib (RFC 1950) — IETF RFCs with no known patent claims. The zlib reference implementation is zlib-licensed with no patent restrictions.
## Errors
`Truncated`, `BadBlockType`, `BadStoredLength`, `BadCode(&str)`, `BadDistance`, `BadHeader`, `PresetDictionary`, `BadChecksum`, `OutputLimit`.
