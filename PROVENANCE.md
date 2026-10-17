# PROVENANCE — omni-spec 0.1.0

- **Origin.** Written in the SovRing-Labs development tree from 2026-10-06 to 2026-10-16, one module per task, so that our renderer, document
  tools and media player depend on code whose origin is the published standard and nothing else. The Opus, CELT, SILK and WebM modules
  were added from 2026-10-15.
- **Sources of ideas.** The published standards named in `README.md` and in each `docs/<module>.md`: Unicode UAX #9, #14, #29 (18.0.0),
  RFC 1950, 1951, 6716, 7932, 8251, 8259, 8794, 9559, FIPS 180-4, ITU-T T.81, W3C PNG 3rd ed., W3C WOFF2, W3C XML 1.0 5th ed.,
  WHATWG HTML, ECMA-376 5th ed., PKWARE APPNOTE 6.3.10, GIF89a, the WebP Lossless Bitstream Specification and the OpenType specification.
  Package-merge length-limited Huffman coding follows Larmore and Hirschberg (1990).
- **Third-party code.** None. No source of another implementation was read or copied. Other programs were run as separate processes
  (black boxes) to produce fixtures and to compare output: Pillow, libjpeg-turbo, libwebp, the Python `brotli` and `zlib` modules, fontTools,
  HarfBuzz, Info-ZIP, LibreOffice. Their output is compared; their code is not part of this repository.
- **Third-party data.** Listed with licences in `LICENSING.md`; the Unicode data in `NOTICE`.
- **Generated code.** `src/*_tables.rs`, `src/html_entities.rs`, `src/brotli_dict.bin`: produced by the scripts in `tools/` from the
  standard's own text or data files, and checked against the values printed in the standard before writing.
- **AI assistance.** Code and text were written with AI coding assistants (Claude) directed and reviewed by the author; every number in
  this repository comes from a test in `tests/` with its log in `results/`.
- **Export.** This repository is a fresh single-history export of the development tree (history squashed because the development
  history carries private paths), cut by a manifest-driven release tool that rejects private paths, secrets and databases.
