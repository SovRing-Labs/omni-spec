# png — PNG decoder (W3C PNG Third Edition)

- **Spec:** https://www.w3.org/TR/png-3/ — §5 chunk layout and CRC (§5.4), §6 colour types, §7 filters (incl. §7.2 bytes-per-pixel rule), §8 Adam7 interlace, §11.2.3 PLTE, §11.3.2.1 tRNS, §11.2 Table 11.1 valid depth/colour combinations, §13.12 gray scaling.
- **Code:** `src/png.rs` — `decode(&[u8]) -> Result<Image>` → RGBA8; `crc32`. Uses `crate::inflate` (RFC 1950/1951).
- **Output rules:** sub-8-bit gray and palette-free samples scaled to 8 bits by `v*255/(2^d-1)`; 16-bit keeps the high byte; tRNS matches the raw sample value; palette alpha from tRNS (default 255).
- **Conformance:**
  - Pillow oracle (`tools/oracle_png.py` → `tests/data/png-oracle.tsv`, black-box process): **94 / 94 images** match Pillow's RGBA8 (FNV-1a 64 + size). Set = 49 corpus PNGs (8-bit RGB) + up to 6 system PNGs per (colour type, depth, interlace) class: palette 4/8-bit, gray+alpha 8-bit, RGBA 8-bit (incl. 5 Adam7-interlaced), RGBA 16-bit, RGB 16/gray 16, palette 2-bit.
  - Synthetic (`tests/png.rs`): gray 1/2/4/16-bit scaling and tRNS key, palette 2-bit with alpha, CRC corruption, bad signature, palette index out of range, invalid depth/colour combination.
- **Known gaps:** colour type 0 gray at depths 1/2/4/8 and 1-bit palette have only the synthetic tests (no oracle sample found on this machine); no gAMA/iCCP/sRGB colour management (pixels are returned as stored); no ancillary metadata beyond PLTE/tRNS; IDAT must be contiguous; no APNG; `MAX_PIXELS` = 2^28.
- **Patent screen:** PNG (W3C Recommendation, PNG Third Edition) — W3C royalty-free patent policy applies. No known patent encumbrances for the baseline PNG format.
