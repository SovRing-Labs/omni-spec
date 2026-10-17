---
title: woff2 — WOFF2 container decoder (W3C WOFF File Format 2.0)
updated: 2026-10-14
status: DONE (DOCNEXT-SPECREST part 1, STACK-C-1014 C7)
---
# woff2 — WOFF2 container decoder

- **Spec:** W3C Recommendation "WOFF File Format 2.0" (2018), https://www.w3.org/TR/WOFF2/ — §3 header, §4.1 UIntBase128, §4.2 255UInt16, §5.1 table directory (known-tag index 0–62, flags byte, transform version, optional `transformLength`), §5.3 the single Brotli stream (RFC 7932, our `brotli.rs`), §5.4 metadata/private blocks (ignored), §6.1 glyf/loca transform (seven sub-streams, triplet coordinate encoding §6.1.1.1, bbox bitmap, overlap bitmap), §6.2 hmtx transform. sfnt output per OpenType spec ch. 2.
- **Code:** `src/woff2.rs` — `decode(data, max_out) -> Woff2 {flavor, tables, total_sfnt_size, glyf_size_differs}`, `to_sfnt(&Woff2)` (tables sorted by tag, 4-byte aligned, table checksums and `head.checkSumAdjustment` set). `examples/woff2_dump.rs` writes the decoded font + tables for the oracle.
- **Conformance (`cargo test --test woff2`, 4 tests):** the 40 public WOFF2 fonts under `/usr/lib/claude-desktop/resources/ion-dist/assets/v1` (not copied into the tree; absent fonts are skipped). 520 tables other than glyf/loca (hmtx transform included, 1 font) are byte-equal to fontTools (`tools/oracle_woff2.py` → `tests/data/woff2-oracle.tsv`, length + SHA-256). glyf/loca are rebuilt to an equivalent encoding — fontTools pads glyphs to 2 bytes and picks its own flag repeats, ours uses 4-byte padding and run-length flags — so they are compared glyph by glyph through fontTools (`tools/check_woff2_vs_fonttools.py`: contours, end points, flags, coordinates, bytecode, components, bbox): **40 fonts, 12,553 glyphs, 0 differences**. Our sfnt output re-parses with `sfnt::parse` (12 fonts), file checksum = 0xB1B0AFBA; 400 bit-flip variants and every 97th-byte truncation return Ok or Err, never panic.
- **Size check:** rebuilt glyf equals the directory's `origLength` for 36 of the 40 fonts; 4 rebuild a few bytes shorter (4–8 bytes: the encoder's original spends bytes ours does not) — reported as `glyf_size_differs`, not refused (the spec asks for an equivalent font).
- **Known gaps:** font collections (flavor `ttcf`, §5.2) → `Error::Unsupported`; transform versions other than glyf/loca 0 and hmtx 1 are refused; no metadata block parsing.
- **Patent screen:** WOFF2 is a W3C Recommendation under the W3C royalty-free patent policy; Brotli (RFC 7932) is royalty-free. No known encumbrances.
- **Provenance:** written from the specification text; fontTools was run as a black box, its source was not read.
