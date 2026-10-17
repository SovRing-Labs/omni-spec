---
title: deflate — DEFLATE writer (RFC 1951) + zlib wrapper (RFC 1950)
updated: 2026-10-14
status: DONE (OFFICE-FORMATS-1 step 1, STACK-B-1014 B9)
---
# deflate — DEFLATE writer (RFC 1951)

- **Spec:** RFC 1951 §3.1.1 (bit packing, LSB-first; Huffman codes MSB-first), §3.2.3 (BFINAL/BTYPE), §3.2.4 stored blocks (LEN/NLEN), §3.2.5 length + distance tables + extra bits, §3.2.6 fixed Huffman, §3.2.7 dynamic Huffman (HLIT/HDIST/HCLEN, CLEN_ORDER, repeat codes 16/17/18), §3.2.2 canonical codes. RFC 1950 §2.2 zlib header + Adler-32. Package-merge (Larmore & Hirschberg, "A fast algorithm for optimal length-limited Huffman codes", JACM 1990) for the 15-bit / 7-bit length limits.
- **Code:** `src/deflate.rs` — `deflate_raw(data, max_out)`, `zlib_compress(data, max_out)`; `zip::Writer::add_deflated`; OOXML packages deflate every non-image part (`src/ooxml/opc.rs`).
- **Encoder:** LZ77, 32 KiB window, 3-byte hash chains (chain 128), one-step lazy matching (below length 32); 16,384-token blocks; each block is stored / fixed / dynamic by exact bit count. Deterministic.
- **Conformance:** `cargo test --test deflate` (4) + `cargo test --lib deflate` (5): 15-stream corpus (empty, 1 byte, 1 MiB zeros, random, skewed, every match length, far repeats, UnicodeData/BidiTest/emoji/DerivedCore/EastAsianWidth, zip fixtures) round-trips byte-equal through our `inflate`; Python `zlib.decompress` (black box) decodes all 15 zlib streams byte-equal; Info-ZIP `unzip -t` accepts a deflated archive; `cargo test` whole crate green; LibreOffice headless opens our deflated .xlsx and .docx (`tests/xlsx.rs`, `tests/docx.rs`).
- **Ratio (reported, not a gate):** UnicodeData.txt 2,243,593 → 316,068 (python zlib level 6: 325,819; level 9: 311,158); BidiTest 7,959,988 → 1,291,310; 1 MiB zeros → 1,032; random data expands by one 5-byte stored header per 16 KiB block.
- **Limits / next:** no streaming API (whole buffer in, whole stream out); blocks are fixed at 16K tokens (adjacent stored blocks are not merged); matches never reach beyond the 32 KiB window by design; no ZIP64 on write.
- **Patent / provenance:** RFC 1951 is an IETF RFC with no known patent claims; no zlib/miniz source was read; Python zlib / Info-ZIP were run as black boxes only.
