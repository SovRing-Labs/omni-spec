# LICENSING — omni-spec

Free for the community, permissive: open code, open documents. Decision of the author, 2026-10-06: stock Apache-2.0.

| Files | Licence |
|---|---|
| source code: `src/`, `tests/*.rs`, `examples/`, `tools/`, `Cargo.toml`, `install.sh`, `docs/` | **Apache-2.0** (`LICENSE`) |
| documents: `README.md`, `STATUS.md`, `TECHNICAL-DISCLOSURE.md`, `PROVENANCE.md`, `NOTES.md`, `DATA.md`, `results/` | **CC BY 4.0** (`LICENSE-CC-BY-4.0.md`) |
| Unicode data files and tables derived from them (`tests/data/*.txt` from the Unicode Character Database, `src/uax*_tables.rs`) | **Unicode License v3**, attribution in `NOTICE` |
| third-party test data kept in `tests/data/` | the licences below |

Attribution for the documents: "Juan Carlos De Santiago, with the Sovereign Raptor fleet (AI-assisted)".
Apache-2.0 includes an express patent licence from each contributor. The disclosure is also published as dated prior art: the author
claims no patent on what it describes and asks that none be granted on it.

## Third-party test data (release gate 5)
| Data | Licence | Kept in the repository |
|---|---|---|
| JSONTestSuite (Nicolas Seriot, 2016) | MIT; licence file in `tests/data/json-suite/` | yes (319 small files) |
| html5lib-tests tokenizer tests (James Graham, Geoffrey Sneddon and contributors) | MIT; `tests/data/html5lib-tokenizer/LICENSE` | yes |
| Unicode Character Database and UAX conformance files | Unicode License v3 (`NOTICE`) | the files under 1 MiB; the five larger ones are left out (`DATA.md`) |
| RFC 7932 text (`tests/data/rfc7932.txt`) | IETF Trust Legal Provisions: unmodified RFC text may be copied | not shipped (the text carries the authors' e-mail addresses; `DATA.md`) |
| WHATWG named character references (`tests/data/html-entities.json`) | WHATWG, CC BY 4.0 | yes, attribution here |
| RFC 8251 Opus test vectors | IETF package | **not** in the repository |
| PngSuite | not present | no |
| fixtures made by us (`zip/`, `ooxml/`, `webp/`, `jpeg/`, `brotli/`, `*-oracle.tsv`) | Apache-2.0; synthetic content, produced by running other programs as black boxes (their output carries no third-party licence) | yes |

## Release checks recorded for the author (not for users)
Clean-room provenance, patent notes in `docs/`, attribution and trademarks (no "OpenType", "Unicode", "POSIX" in the crate name) are done;
the clone check against open implementations has **not** been run (`STATUS.md`).
