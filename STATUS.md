> **Status: under development — pre-alpha.** Interfaces, numbers and file layout can change without notice.

# STATUS — omni-spec 0.1.0 (2026-10-16)
Stage: about 30 % (J, 2026-10-16: the cores and libraries outside doctor / math / embed are at about 30 %).
Source state: branch `claude/work` at commit `29f01bc` (25 modules). All results below were produced on 2026-10-16 on an Intel i5-8365U
laptop, Rust 1.98.0, by `cargo test --offline -j 2` in a copy of the tree with `target/` removed.

## Works today (each line = a suite test that passed on 2026-10-16, with its evidence)
- **O1** JSON, RFC 8259: 3/3 tests; JSONTestSuite 95 must-accept accepted, 188 must-reject rejected — `results/2026-10-16-cargo-test-nocapture.log`
- **O2** SHA-256 (FIPS 180-4), XML 1.0, ZIP: 2 + 11 + 13 tests pass — `results/2026-10-16-cargo-test.log`
- **O3** HTML tokenizer (WHATWG §13.2.5): html5lib-tests 6,941/6,941 (56 CDATA initial states and 52 obsolete processing-instruction tests excluded by the test itself) — `results/2026-10-16-cargo-test-nocapture.log`
- **O4** Brotli, WebP lossless, JPEG: 3 + 2 + 1 tests pass; 20 Brotli oracle streams, 10 WebP files exact, 8 JPEG images within ±1 — same log
- **O5** Office Open XML (OPC, XLSX, DOCX): 6 + 8 + 5 tests pass — same log
- **O6** WebM / Matroska demuxer and UAX #29 grapheme clusters: 5 tests and 853/853 GraphemeBreakTest lines — same log
- **O7** (external, needs data not in this repository) full `cargo test`: 113 tests in 24 test binaries, 0 failed — `results/2026-10-16-cargo-test.log`; UAX #14 19,346/19,346, UAX #9 770,241 + 91,707, deflate corpus, GIF 117/117, SFNT 14,094 rows, Opus framing on 12 vectors
- **O8** the skipped and partial checks are recorded as their own row (see below)

## Not yet tested
- **Clone check (release gate 2) has not been run:** the doctor's collage of each module against the best-known open implementation of the
  same standard. Until it is run, "written from the specification only" rests on the commit history and the citations, not on a measurement.
- Brotli, JPEG, WebP, GIF, PNG, SFNT and OpenType results compare against Python packages (Pillow, brotli, fontTools, HarfBuzz) run as black
  boxes on the author's machine; the reference programs are not part of this repository.
- Opus CELT audio decoding (only headers, band layout and the Laplace coder exist), WebP lossy (VP8), HTML tree construction (§13.2.6),
  PNG colour management, XLSX formulas, DOCX text boxes / comments / headers: not implemented.
- Opus PCM quality against the reference decoder beyond phase-free SNR of 18.5–27.5 dB on SILK vectors.
- Non-x86 machines and Rust versions other than 1.98.0.

## Known failures and honest negatives
- **Some checks pass without checking.** On 2026-10-16 `tests/inflate.rs` reported "0 streams match, 49 skipped (absent)" and `tests/png.rs`
  "45 images match, 49 skipped (absent)": those corpora are read from files on the host that were not there, and the tests still pass.
  `tests/woff2.rs` skipped its glyph-by-glyph test because `fontTools` is not installed. `tests/opus.rs` returns early and passes if the
  RFC 8251 vectors are not found (they were found on 2026-10-16). Older documents (`docs/STATUS.md`) quote 49/49 and 94/94; those
  numbers were not reproduced today.
- **UAX #14 and UAX #9 conformance cannot run from this repository**: `LineBreakTest.txt`, `BidiTest.txt` and `BidiCharacterTest.txt` are larger
  than 1 MiB and are not shipped (`DATA.md`). Those tests panic on the missing file until you download the files.
- Opus SILK: the final range equals the reference on every SILK-only packet without redundancy, but 14 packets that carry redundant CELT
  frames differ (vector 12: 12 of 1,056, vectors 08 and 09: 1 of 4 each). The test asserts only the first group.
- JPEG 4:2:0 uses nearest-neighbour chroma upsampling (max sample deviation 11 against libjpeg-turbo on one file) and is reported, not gated.
- Opus, CELT, SILK and WebM modules have no `docs/<module>.md` page; release gate 1 (a doc per module naming spec and version) is met by 19 pages covering 21 of the 25 modules (`ooxml` has one page for its three files). Their citations are in the source headers.
- The `docs/STATUS.md` module table predates the Opus and WebM modules.
- Where the doctor reads this library (checked 2026-10-16): omni-render depends on it by path (`omni-spec = { path = "../omni-spec" }`) and uses
  `uax14` and `otshape`. The doctor's documented index (`core --index`, D9) was last built on 2026-10-06; its folder holds 17 projects and
  **omni-spec is not one of them**, so "indexed by the doctor" in older notes was not true of that index. The doctor's `spec` command is a
  source-to-function-spec reader (clean-room Chinese wall), not a reader of the standards in this library.

## Corrections
None recorded against this repository (first release).

## How to re-check
`python3 sovring-suite.pyz --run omni-spec` or `bash install.sh`, or `cargo test --offline`. For UAX #9 / #14: put the Unicode 18.0.0 files
named in `DATA.md` into `tests/data/`, then `cargo test --offline --test uax9 --test uax14`.

## Defensive publication
`TECHNICAL-DISCLOSURE.md` (the method: spec-only implementation with a conformance oracle per module, generated tables checked against the
standard's own values, final-range check for Opus), dated 2026-10-16. DOI: minted by Zenodo on the first GitHub release; `CITATION.cff` carries it after that.
