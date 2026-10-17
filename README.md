# omni-spec — published standards, implemented from the specification text only (Rust, no dependencies)

[![status: pre-alpha](https://img.shields.io/badge/status-pre--alpha%20(under%20development)-orange)](STATUS.md)
[![defensive publication](https://img.shields.io/badge/defensive%20publication-TECHNICAL--DISCLOSURE.md-blue)](TECHNICAL-DISCLOSURE.md)

Status: **under development — pre-alpha** (`STATUS.md`). Defensive publication: `TECHNICAL-DISCLOSURE.md` (dated 2026-10-16; the DOI is
minted by Zenodo on the first GitHub release).

omni-spec is a Rust library of file-format and text-processing standards (Unicode line breaking, grapheme clusters and bidirectional text;
JSON, XML, HTML tokenizing; DEFLATE, Brotli, ZIP; PNG, GIF, JPEG, WebP; OpenType, WOFF2; Office Open XML; WebM and Opus) that we wrote from
the published specifications, never from another project's code. Each module cites the specification section beside the code it governs
(and, for the modules that have one, in `docs/<module>.md`), and is checked against the standard's own conformance data or against a
second implementation run as a black box. The crate has no dependencies.

## Run it (Linux or macOS, Rust 1.85 or newer; no network needed)
```bash
cargo test --offline --test json --test sha256       # the two quickest conformance binaries (a few minutes the first time: the tests build at opt-level 3)
cargo test --offline                                 # everything; some tests need data this repository does not ship, see DATA.md and STATUS.md
bash install.sh                                      # the same checks with their explanations (opens a local page; --no-gui for the terminal)
```

## Module index (one row per module in `src/`)
"Result" is what we saw on 2026-10-16 on an Intel i5-8365U laptop (`results/2026-10-16-cargo-test-nocapture.log`); the spec column lists the
sections the module cites (taken from `docs/<module>.md`, or from the module's own header for the four Opus / WebM modules that have no doc
file yet).

| Module (`src/`) | Standard implemented | Spec, version, sections cited | Test that checks it | Result / status |
|---|---|---|---|---|
| `brotli.rs` (+ tables, `brotli_dict.bin`) | Brotli decoder | RFC 7932 (2016): §3 prefix codes, §4 distances, §5 insert-and-copy, §6 block switching, §7 context modelling, §8 dictionary and transforms, §9 format, §10 algorithm, App. A, B | `tests/brotli.rs` (3) | RFC §11.1 streams pass; 20 streams from an independent encoder decode equal. Decoder only, bit-serial |
| `inflate.rs` | DEFLATE / zlib decoder | RFC 1951 §3.1.1, §3.2.2–3.2.7; RFC 1950 §2.2, §8.2 | `tests/inflate.rs` (3) | RFC examples and fuzz pass; the 49-stream corpus check **skipped on 2026-10-16** (its PNG sources are not in this repository) |
| `deflate.rs` | DEFLATE writer | RFC 1951 §3.1.1, §3.2.2–3.2.7; RFC 1950 §2.2 | `tests/deflate.rs` (4) + 5 unit tests in the crate | 15-stream corpus round-trips and Python zlib decodes it byte-equal; needs the Unicode data files listed in `DATA.md` |
| `zip.rs` | ZIP archives (read, write) | PKWARE APPNOTE 6.3.10 (2022): §4.3.7, §4.3.9, §4.3.12, §4.3.14–4.3.16 and ZIP64 | `tests/zip.rs` (13) | 13/13 pass: fixtures from Info-ZIP as a black box, corruption and fuzz never panic |
| `sha256.rs` | SHA-256 | FIPS 180-4 (2015): §4.1.2, §4.2.2, §5.1.1, §5.3.3, §6.2 | `tests/sha256.rs` (2) | 2/2 pass (NIST examples, one million `a`, incremental splits) |
| `json.rs` | JSON parser / writer | RFC 8259: §2, §6, §7, §8.1 | `tests/json.rs` (3) | JSONTestSuite: 95 must-accept accepted, 188 must-reject rejected |
| `xml.rs` | XML 1.0 + Namespaces (non-validating) | XML 1.0 5th ed. (2008) §2.2, §2.5–2.8, §2.11, §3.1, §3.3.3, §4.1, §4.6; Namespaces in XML 1.0 3rd ed. | `tests/xml.rs` (11) | 11/11 pass |
| `html.rs` (+ `html_entities.rs`) | HTML tokenizer | WHATWG HTML Living Standard §13.2.3.5, §13.2.5 (all states), §13.2.5.72–80, §13.5. Tree construction is not implemented | `tests/html_tokenizer.rs` (1) | html5lib tokenizer tests 6,941/6,941 (56 CDATA initial states and 52 obsolete processing-instruction tests excluded by the test) |
| `uax14.rs` (+ tables) | Unicode line breaking | UAX #14, Unicode 18.0.0: §5.1 Table 1, §6.1 LB1–LB31 | `tests/uax14.rs` (1) | 19,346/19,346 of LineBreakTest.txt (file not shipped, see `DATA.md`) |
| `uax29.rs` (+ tables) | Unicode grapheme clusters (extended) | UAX #29, Unicode 18.0.0: §3.1 GB1–GB999 | `tests/uax29.rs` (1) | 853/853 of GraphemeBreakTest.txt |
| `uax9.rs` (+ tables) | Unicode bidirectional algorithm | UAX #9, Unicode 18.0.0: §3.3.1–3.3.5, §3.4 | `tests/uax9.rs` (2) | BidiTest 770,241/770,241 and BidiCharacterTest 91,707/91,707 (files not shipped, see `DATA.md`). One line per call, no mirroring |
| `png.rs` | PNG decoder | W3C PNG 3rd ed.: §5, §6, §7, §8, §11.2.3, §11.3.2.1, §13.12 | `tests/png.rs` (6) | 45 images equal Pillow RGBA8; **49 more skipped on 2026-10-16** (sources absent). No colour management |
| `gif.rs` | GIF87a / GIF89a decoder | GIF89a (1989): §17, §18, §20, §22 | `tests/gif.rs` (2) | frame 0 of 117/117 files equals Pillow RGBA8 (the files are read from the host's `/usr`); later frames are not composited |
| `jpeg.rs` | JPEG baseline + progressive decoder | ITU-T T.81: B.2, F.2.2, G.1.2, A.3.3 | `tests/jpeg.rs` (1) | 8 gated images within ±1 of libjpeg-turbo; 4:2:0 is reported only (max deviation 11 on one file) |
| `webp.rs` | WebP lossless (VP8L) | WebP Lossless Bitstream Specification (2023-03-09): §3–§7. Lossy VP8 is not implemented | `tests/webp.rs` (2) | 10/10 files equal libwebp |
| `sfnt.rs` | OpenType / TrueType font tables (read) | OpenType spec: ch. 2 table directory and TTC, `head`, `hhea`, `maxp`, `hmtx`, `cmap` 4/12, `name`, `OS/2` | `tests/sfnt.rs` (1) | 14,094 rows equal fontTools + HarfBuzz (fonts read from the host's `/usr/share/fonts`) |
| `otshape.rs` | OpenType kerning + ligatures | OpenType spec: GPOS lookup 2 (formats 1, 2), GSUB lookup 4, extension lookups, `kern` format 0, Coverage / ClassDef / Script / Feature / Lookup lists | `tests/otshape.rs` (5) | 275 kern pairs and 20 ligature strings equal HarfBuzz on two host fonts; script selection latn / DFLT only |
| `woff2.rs` | WOFF 2.0 decoder | W3C WOFF2 Recommendation (2018): §3, §4.1, §4.2, §5.1, §5.3, §5.4 | `tests/woff2.rs` (4) | 40 fonts: 520 tables equal fontTools; the glyph-by-glyph test **skipped on 2026-10-16** (fontTools not installed) |
| `ooxml/opc.rs` | Open Packaging Conventions | ECMA-376 5th ed. Part 2: §9.1, §9.3, §10.1.2, §10.2, Annex C | `tests/opc.rs` (6) | 6/6 pass |
| `ooxml/xlsx.rs` | SpreadsheetML read / write | ECMA-376 5th ed. Part 1 §18 (§18.2–18.4, §18.8, §18.17.4.1, §18.18.11) | `tests/xlsx.rs` (8) | 8/8 pass (LibreOffice workbooks cell-exact; LibreOffice opens our output). No formulas |
| `ooxml/docx.rs` | WordprocessingML read / write | ECMA-376 5th ed. Part 1 §17 (§17.3–17.5, §17.7, §17.9, §17.11, §17.13.5, §17.16), §20.4 | `tests/docx.rs` (5) | 5/5 pass. Text boxes, comments, headers, footers and equations are skipped |
| `webm.rs` | EBML + WebM / Matroska demuxer (streaming) | RFC 8794 §4–§7, §11.2; RFC 9559 §5.1, §10.1–10.3 | `tests/webm.rs` (5) | 5/5 pass (any chunking gives the same events). No `docs/webm.md` yet |
| `opus.rs` | Opus packet framing + range decoder | RFC 6716 §3 (Table 2, Figure 1), §4.1; RFC 8251 | `tests/opus.rs` (5) | framing exact on all 12 RFC 8251 vectors (package not shipped, see `DATA.md`). No `docs/opus.md` yet |
| `opus_silk.rs` (+ tables) | Opus SILK layer | RFC 6716 §4.2.3–§4.2.7.9 | `tests/opus.rs` | final range equals the reference on SILK-only packets of vectors 02, 03, 04 (1,185 / 998 / 1,265); vectors 08, 09 and 12 carry redundant CELT frames and 14 of their packets differ (`STATUS.md`) |
| `opus_celt.rs` | Opus CELT layer (partial) | RFC 6716 §4.3: Table 55 band layout, Table 56 frame header, Laplace coarse-energy coder | `tests/opus.rs` (`celt_laplace_intervals_contiguous`) | header and first-band probes only; **no CELT audio decode yet** |

## Where the doctor reads this library
Our code-repair tool ("the doctor", part of the omni-render engine, not in this repository) uses omni-spec in two ways. (1) As a Cargo path
dependency: omni-render calls `uax14` (line breaking) and `otshape` (kerning) from `../omni-spec`. (2) As a donor library for its function
search: `core --index --out <index-dir> <omni-spec-dir>` builds the code index and `core --donors --index <index-dir>` searches it. Its
"spec reader" (`core --abc spec --src <file> --licence own --out specs.jsonl`) is a different tool: it turns one *source* file into
name-free function specs for clean-room rebuilding, and it does not read the standards' text. Details and what we have not checked: `STATUS.md`.

## Files
| Path | What |
|---|---|
| `src/` | the 25 modules above (`lib.rs` lists them); generated tables are marked `@generated` with their generator in `tools/` |
| `tests/` | one integration test file per module family; `tests/data/` holds fixtures and the conformance files small enough to ship (see `DATA.md`) |
| `docs/` | one page per module: spec and version, sections, conformance, known gaps, patent note (19 pages; four Opus / WebM modules have none yet) |
| `examples/` | small programs: `opus_dec`, `silk_vocoder`, `webm_dump`, `woff2_dump`, `xlsx_dump`, `docx_dump`, three CELT probes |
| `tools/` | table generators and the black-box oracle scripts that produced the `*-oracle.tsv` files |
| `NOTICE`, `LICENSE`, `LICENSING.md` | attribution (Unicode License v3 for the Unicode data), licence, release checks |
| `suite.json`, `install.sh` | the test suite record and its runner |

## Licence
Code: **Apache-2.0** (`LICENSE`). Documents (`README.md`, `STATUS.md`, `TECHNICAL-DISCLOSURE.md`, `PROVENANCE.md`): **CC BY 4.0**
(`LICENSE-CC-BY-4.0.md`). The Unicode data files keep the Unicode License v3 (`NOTICE`). Free for the community. Details: `LICENSING.md`.

Part of [SovRing-Labs](https://github.com/SovRing-Labs/SovRing). Cite: `CITATION.cff`.
