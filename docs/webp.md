# webp — WebP container and lossless (VP8L) decoder

- **Spec:** "WebP Lossless Bitstream Specification" (Alakuijala, Google, 2023-03-09) — §3 header, §4 transforms (predictor, colour, subtract-green, colour indexing with pixel bundling), §5 LZ77 with the 120-entry distance map and colour cache, §6 prefix codes (simple and normal code-length codes, max_symbol), §7 structure. RIFF/WEBP container as in the WebP container specification.
- **Code:** `src/webp.rs` — `decode(&[u8]) -> Result<Image>` (RGBA8; RIFF 'WEBP', 'VP8L' chunk, VP8X tolerated); `decode_vp8l` for a bare bitstream.
- **Conformance:** Pillow / libwebp oracle (`tools/oracle_webp.py` → `tests/data/webp/*.webp`, index `tests/data/webp-oracle.tsv`): **10 / 10** lossless files decode to the exact reference RGBA8 (FNV-1a 64): 1×1, 7×3, 33×17 and 100×30 (methods 0–6), with alpha (64×48, 40×40, 17×65), and palette images with 16 and 2 colours (colour indexing with pixel bundling).
- **Known gaps:**
  - **Lossy VP8 (`'VP8 '` chunk) is PARKED:** returns `Unsupported`. Reason: the RFC 6386 boolean decoder, intra prediction, IDCT/WHT, and loop filter are not implemented. Lossy files are not decoded.
  - ALPH (alpha for lossy VP8) is not read.
  - Predictor modes 14 and 15 (undefined in the spec) are treated as mode 0.
  - Bit reading is per bit, not buffered (correct, not optimised).
- **Patent screen:** WebP lossless (VP8L) — no known patent issues. WebP lossy (VP8) — covered by Google's royalty-free patent grant for VP8/WebP (https://www.webmproject.org/license/). The RFC 6386 (VP8) reference code carries a permissive licence with a patent grant; we did not read it.
