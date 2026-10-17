# AGENTS.md — omni-spec

For coding assistants (and people) setting this repository up. Plain steps; every command runs from the repo root.

**Status: pre-alpha, under development.** Interfaces, numbers and file layout can change without notice. Read `STATUS.md` first: what works today (each line names its test and evidence file), what is not yet tested, the known failures and the corrections.

## What it is
Stage: about 30 % (under development, pre-alpha).
25 standards implemented from the specification text only (Unicode line breaking, graphemes and bidi; JSON; XML; HTML tokenizer; DEFLATE; Brotli; ZIP; PNG; GIF; JPEG; WebP; OpenType; WOFF2; Office Open XML; WebM; parts of Opus) in dependency-free Rust.

## Install and check (3 commands)
```bash
git clone --depth 1 https://github.com/SovRing-Labs/omni-spec && cd omni-spec
# nothing to install beyond: cargo (Rust 1.85 or newer)
cargo test --offline --test json --test sha256   # expect: test result: ok
```
Needs: cargo (Rust 1.85 or newer).
Whole suite instead: `bash install.sh --no-gui` (python check → venv → every runnable `suite.json` test, headless; without `--no-gui` it then opens the local results page on 127.0.0.1).

GUI: `bash install.sh   # suite page on http://127.0.0.1 (localhost only)`

## What the tests mean
`suite.json` lists every claim this repository makes with its verify command, the expected output, what passing means, how the number was obtained and how to use it in your own work (8 tests, in `suite.json` at the repository root). `STATUS.md` says what works today, what is not yet tested and the known failures; the umbrella's `STATUS.md` has one row per repository.

## Where the evidence is
- `README.md`
- `TECHNICAL-DISCLOSURE.md` — the method, as dated prior art
- `CITATION.cff`

## Honest limits (from STATUS.md)
Some checks pass without checking.** On 2026-10-16 `tests/inflate.rs` reported "0 streams match, 49 skipped (absent)" and `tests/png.rs` "45 images match, 49 skipped (absent)": those corpora are read from files on the host that were not there, and the tests still pass. `tests/woff2.rs` skipped its glyph-by-glyph test because `fontTools` is not installed. `tests/opus.rs` returns early and passes if the RFC 8251 vectors are not found (they were found on 2026-10-16). Older documents (`docs/STATUS.md`) quote 49/49 and 94/94; those numbers were not reproduced today. UAX #14 and UAX #9 conformance …

## Licence
Apache-2.0 (code) + CC BY 4.0 (documents).

## Guided setup through MCP
The SovRing umbrella ships an MCP server that explains, checks readiness, clones and runs these tests: https://github.com/SovRing-Labs/SovRing (README → "Paste this to your assistant"). Zero-install docs endpoint: https://gitmcp.io/SovRing-Labs/omni-spec
