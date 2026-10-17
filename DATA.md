# DATA — files left out of this repository

Five official Unicode 18.0.0 data files and one html5lib test file are larger than the 1 MiB limit of this export and are not shipped,
and the RFC 7932 text is left out because it carries the RFC authors' e-mail addresses. Tests that read them are marked external in the
suite record. Licence of the Unicode files: Unicode License v3 (see `NOTICE`).

| File (`tests/data/`) | Bytes | SHA-256 (first 16 hex) | Used by | Where to get it |
|---|---|---|---|---|
| `BidiTest.txt` | 7,959,988 | 9af2f882a4ab5091 | `tests/uax9.rs`, `tests/deflate.rs` | https://www.unicode.org/Public/18.0.0/ucd/BidiTest.txt |
| `BidiCharacterTest.txt` | 6,880,771 | 045b24d2c8ab0669 | `tests/uax9.rs` | https://www.unicode.org/Public/18.0.0/ucd/BidiCharacterTest.txt |
| `LineBreakTest.txt` | 3,178,750 | fd9ff9eca4110800 | `tests/uax14.rs` | https://www.unicode.org/Public/18.0.0/ucd/auxiliary/LineBreakTest.txt |
| `UnicodeData.txt` | 2,243,593 | 0736451de439ae7b | `tests/deflate.rs`; table generators | https://www.unicode.org/Public/18.0.0/ucd/UnicodeData.txt |
| `DerivedCoreProperties.txt` | 1,159,889 | 09c928886a178fca | `tests/deflate.rs`; UAX #29 table generator | https://www.unicode.org/Public/18.0.0/ucd/DerivedCoreProperties.txt |
| `html5lib-tokenizer/namedEntities.test` | 1,128,317 | a7f0e59ff7653820 | `tests/html_tokenizer.rs` (the harness reads every `.test` file in the folder; without this one it runs fewer tests) | the html5lib-tests repository, `tokenizer/namedEntities.test` (MIT) |
| `rfc7932.txt` | 389,979 | 394abd1879e016f4 | `tools/gen_brotli_tables.py`, `tools/oracle_brotli.py` (not the tests) | https://www.rfc-editor.org/rfc/rfc7932.txt |

Put a downloaded file in `tests/data/` under that name and compare the hash: `sha256sum tests/data/<file> | cut -c1-16`.

Other inputs the tests use and this repository does not contain:
- The RFC 8251 Opus test vectors (`testvector01.bit` … `testvector12.bit` and their `.dec` files): set `OPUS_VECTORS` to their folder
  (default `opus_newvectors`); without them the Opus conformance tests return early and pass without checking.
- Host files read in place: fonts under `/usr/share/fonts` and `/usr/lib` (SFNT, OpenType, WOFF2 oracles), image files under `/usr`
  (GIF, PNG, inflate oracles). The `*-oracle.tsv` files in `tests/data/` list the paths and the expected values; a missing file is
  skipped and reported.
- Optional programs: `soffice` (LibreOffice), `unzip`, `python3` with `zlib`, Pillow, `brotli`, `fontTools` (oracle regeneration only).
