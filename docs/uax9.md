# uax9 — Unicode Bidirectional Algorithm

- **Spec:** UAX #9 (Unicode 18.0.0), https://www.unicode.org/reports/tr9/ — §3.3.1 P2–P3, §3.3.2 X1–X10, §3.3.3 W1–W7, §3.3.4 N0–N2 (BD14–BD16 brackets), §3.3.5 I1–I2, §3.4 L1–L2.
- **Data:** `DerivedBidiClass.txt`, `BidiBrackets.txt`, `UnicodeData.txt` (UCD 18.0.0) → `tools/gen_uax9.py` → `src/uax9_tables.rs`.
- **Code:** `src/uax9.rs` — `class_of`, `bracket_of`, `resolve(classes, brackets, Direction)` → `Resolved { para_level, levels, order }`, `resolve_str`.
- **Conformance:**
  - `BidiTest.txt`: **770,241 / 770,241** cases (class sequences × Auto/LTR/RTL paragraph directions): levels and visual order match.
  - `BidiCharacterTest.txt`: **91,707 / 91,707** cases (code points, brackets, canonical equivalence): paragraph level, levels and order match.
- **Process note:** the rules were implemented from the UAX #9 rule set with section references in the code; the UAX #9 text itself was not re-read in this session. The official conformance data is the verification.
- **Known gaps:** one line per call (L1 applies to the whole input); a B inside the input resets the embedding state (X8) and takes the paragraph level, as the conformance data expects; no line breaking into multiple lines; no mirroring (BidiMirroring.txt) output.
- **Patent screen:** UAX #9 (Unicode Bidirectional Algorithm) — Unicode Standard Annex. Unicode License v3 applies (royalty-free). No known patent encumbrances on the bidi algorithm.
