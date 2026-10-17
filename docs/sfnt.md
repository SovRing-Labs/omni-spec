# sfnt — OpenType/TrueType font reading

- **Spec:** Microsoft OpenType specification — chapter 2 (Table Directory, TTC header), `head`, `hhea`, `maxp`, `hmtx`, `cmap` (formats 4, 12), `name`, `OS/2`. https://learn.microsoft.com/en-us/typography/opentype/spec/
- **Code:** `src/sfnt.rs` — `parse`, `parse_collection` (ttcf), `Font::{table, units_per_em, index_to_loc_format, num_glyphs, num_h_metrics, h_metrics, advance, cmap, family_name, os2_metrics}`. Bounds-checked; malformed input → `Err`.
- **cmap subtable preference:** (3,10) → (0,4) → (3,1) → (0,3) → (0,0..2). Formats 4 and 12 are read; other formats return `BadFormat`.
- **Conformance:** `tools/oracle_sfnt.py` → `tests/data/sfnt-oracle.tsv` (fontTools for head/hhea/maxp/hmtx/name/OS-2; HarfBuzz via uharfbuzz for cmap). Fonts: DejaVuSerif.ttf, LiberationSerif-Regular.ttf, C059-Roman.otf (read in place). **14,094 / 14,094 rows match**: 3 font headers, 6,985 hmtx entries (every glyph), 7,100 cmap lookups (every cmap code point + 200 unmapped probes), 3 family names, 3 OS/2 metric sets.
- **WOFF2:** done in `src/woff2.rs` (docs/woff2.md): the container decoder rebuilds the sfnt tables (glyf/loca/hmtx transforms) and `woff2::to_sfnt` hands the font to this module.
- **Known gaps:** no TTC-specific table sharing checks; no variable-font tables (fvar/gvar/HVAR not read); `cmap` formats other than 4/12 are not read; `family_name` reads name ID 1 only.
- **Patent screen:** OpenType/TrueType (Microsoft OpenType spec) — no known patent encumbrances for the table formats read here (head, hhea, maxp, hmtx, cmap 4/12, name, OS/2). WOFF2 (W3C REC) — W3C royalty-free policy applies.
- **Note:** `otshape` carries its own minimal table-directory reader; it can switch to `sfnt::parse` in a later refactor.
