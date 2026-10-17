# uax29 — Unicode extended grapheme clusters

- **Spec:** UAX #29 (Unicode 18.0.0), https://www.unicode.org/reports/tr29/ — §3.1 rules GB1–GB999 (extended: GB9a, GB9b, GB9c, GB11 included), Table 2 Grapheme_Cluster_Break, Indic_Conjunct_Break for GB9c, Extended_Pictographic for GB11.
- **Data:** `GraphemeBreakProperty.txt`, `DerivedCoreProperties.txt` (InCB), `emoji-data.txt` (UCD 18.0.0) → `tools/gen_uax29.py` → `src/uax29_tables.rs` (2,183 runs).
- **Code:** `src/uax29.rs`. **Conformance:** `tests/uax29.rs` vs `tests/data/GraphemeBreakTest.txt`: **853 / 853 pass**.
- **Known gaps:** none against GraphemeBreakTest. Legacy (non-extended) clusters are not offered.
- **Patent screen:** UAX #29 (Unicode Grapheme Clusters) — Unicode Standard Annex. Unicode License v3 applies (royalty-free). No known patent encumbrances on the grapheme cluster algorithm.

## API
- `props(char) -> Props { gcb, incb, ext_pict }`
- `boundaries_chars(&[char]) -> Vec<usize>` — boundary indices, always includes 0 and len.
- `boundaries(&str) -> Vec<usize>` — byte offsets.
- `graphemes(&str) -> Vec<&str>`.
