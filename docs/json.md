# json — RFC 8259 parser and serializer

- **Spec:** RFC 8259 (The JSON Data Interchange Format), https://www.rfc-editor.org/rfc/rfc8259.txt — §2 grammar and whitespace, §6 numbers, §7 strings and escapes (surrogate pairs), §8.1 UTF-8 input.
- **Code:** `src/json.rs` — `parse(&[u8]) -> Result<Value>`, `to_string(&Value) -> String`; `Value` keeps object members in source order; `MAX_DEPTH` = 512.
- **Conformance:** JSONTestSuite `test_parsing` (Nicolas Seriot, MIT; copy in `tests/data/json-suite/`, licence alongside): **95 / 95 y_ accepted**, **188 / 188 n_ rejected**, every accepted y_ document round-trips through `to_string` (object order and values preserved). i_ (implementation-defined): 11 accepted, 24 rejected, reported only.
- **Behaviour choices:** lone surrogate escapes are rejected (LoneSurrogate); invalid UTF-8 is rejected; numbers parse to binary64 (`1e400` → infinity, not an error); non-finite numbers serialize as `null`; duplicate object names are kept.
- **Known gaps:** numbers lose precision beyond binary64 (no big-number or raw-text mode); no streaming parser; recursive descent limited to depth 512.
- **Patent screen:** JSON (RFC 8259) — IETF RFC with no known patent claims. The JSON format itself is not patentable; no encumbrances.
