# omni-spec — build status (SPECLIB wave)

Order per `the build plan`. One row per module. A module is done only when `cargo test` is green.

| module | spec | conformance pass/total | known gaps | commit |
|---|---|---|---|---|
| uax14 | UAX #14 (18.0.0) | 19346/19346 (LineBreakTest.txt) | none | 1b1856c |
| otshape | OpenType GPOS 2 / GSUB 4 / kern 0 | 275/275 kern pairs + 20/20 ligature strings (HarfBuzz oracle, DejaVuSerif + LiberationSerif) + synthetic layouts | script selection latn/DFLT only; no lookupFlag/GDEF; no device deltas (docs/otshape.md) | (this commit) |
| uax29 | UAX #29 (18.0.0), extended clusters | 853/853 (GraphemeBreakTest.txt) | none (legacy clusters not offered) | (this commit) |
| inflate | RFC 1951 / RFC 1950 | 49/49 corpus PNG IDAT streams vs zlib oracle; RFC examples; fuzz no-panic | no FDICT; no gzip; bit-serial decoder (docs/inflate.md) | (this commit) |
| png | W3C PNG 3rd ed. | 94/94 images vs Pillow RGBA8 (all colour types except gray-1/2/4/8 oracle sample; synthetic tests cover them) | no gray 1/2/4/8 oracle sample; no colour management (docs/png.md) | (this commit) |
| brotli | RFC 7932 | 20/20 black-box oracle streams (Python brotli); RFC §11.1 vectors; fuzz no-panic | WOFF2 container deferred to sfnt; bit-serial, whole-output (docs/brotli.md) | (this commit) |
| sfnt | OpenType/TrueType (WOFF2 → woff2 row) | 14094/14094 rows vs fontTools + HarfBuzz (hmtx all glyphs, cmap, names, OS/2) | variable-font tables not read (docs/sfnt.md) | (this commit) |
| woff2 | W3C WOFF2 REC (container + glyf/loca + hmtx transforms) | 40 public fonts: 520 non-glyf tables byte-equal to fontTools; glyf/loca 12,553 glyphs equal by content (0 differences); sfnt output re-parses; fuzz no-panic | collections (ttcf) unsupported; glyf rebuilt equivalent, not always byte-equal (docs/woff2.md) | STACK-C C7 |
| json | RFC 8259 | JSONTestSuite test_parsing: 95/95 y_ accepted, 188/188 n_ rejected, round-trips | no big-number mode; depth 512 (docs/json.md) | (this commit) |
| gif | GIF89a / GIF87a | 117/117 frame-0 compositions vs Pillow RGBA8 (16 interlaced, 92 transparent, 19 animated) | later frames not composited (docs/gif.md) | (this commit) |
| jpeg | ITU-T T.81 baseline + progressive | gate 8/8 within ±1 (4:4:4, gray, restart, progressive); 4:2:0 report-only, mean 2.3–3.5 | 4:2:0 nearest upsampling; no arithmetic/lossless/CMYK (docs/jpeg.md) | (this commit) |
| webp | WebP container + VP8L lossless (VP8 lossy PARKED) | 10/10 lossless files exact vs libwebp (Pillow) | lossy VP8 PARKED: RFC 6386 decoder not implemented (docs/webp.md) | (this commit) |
| uax9 | UAX #9 bidi | BidiTest 770,241/770,241; BidiCharacterTest 91,707/91,707 | one line per call; no mirroring (docs/uax9.md) | (this commit) |
| html | WHATWG HTML §13.2.5 tokenizer | html5lib-tests tokenizer suite 6,941/6,941 (56 CDATA initial states and 52 obsolete processing-instruction tests excluded, reported by the test) | tree construction (§13.2.6) PARKED; no parse-error reporting (docs/html.md) | STACK-C C8 re-run 2026-10-14 |
| zip | PKWARE APPNOTE 6.3.10 | 13/13 tests: Info-ZIP fixtures (stored, deflate, dirs, ZIP64, data descriptor, UTF-8, comment); our output passes `unzip -t`; fuzz no-panic | write = stored or deflated (OFFICE-FORMATS-1); no encryption / multi-disk (docs/zip.md) | OFFICE-FORMATS-0 |
| deflate (write) | RFC 1951 / 1950 | 9 tests: 15-stream corpus byte-equal through our inflate + Python zlib; `unzip -t`; LibreOffice opens our deflated xlsx/docx; ratio UnicodeData 316,068 vs zlib-6 325,819 | no streaming; 16K-token blocks (docs/deflate.md) | STACK-B-1014 B9 |
| xml | W3C XML 1.0 5th ed. + Namespaces 3rd ed. | 11/11 tests: spec cases, 13 malformed rejected, namespaces, writer round trip, 20k fuzz no-panic | non-validating; DTD entities not expanded (docs/xml.md) | OFFICE-FORMATS-0 |
| sha256 | FIPS 180-4 | NIST examples + 1M "a" + incremental splits | — (docs/sha256.md) | OFFICE-FORMATS-0 |
| ooxml/opc | ECMA-376 Part 2 | 6/6 on LibreOffice packages + our writer | — (docs/ooxml.md) | OFFICE-FORMATS-0 |
| ooxml/xlsx | ECMA-376 Part 1 §18 | 8/8: LibreOffice workbooks cell-exact; dates 1900-03-01..2100 round trip; LibreOffice opens our output, CSV matches | no formula writing / evaluation; no .xls (docs/ooxml.md) | OFFICE-FORMATS-0 |
| ooxml/docx | ECMA-376 Part 1 §17 | 5/5: LibreOffice document read construct by construct; synthetic 15-block model round-trips exactly; LibreOffice exports our docx to PDF and re-saves it with structure intact | text boxes, comments, headers/footers, OMML skipped (docs/ooxml.md) | OFFICE-FORMATS-0 |
