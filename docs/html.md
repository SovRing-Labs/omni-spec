# html — WHATWG HTML tokenizer (tree construction PARKED)

- **Spec:** WHATWG HTML Living Standard, https://html.spec.whatwg.org/multipage/parsing.html — §13.2.3.5 input preprocessing (CR/CRLF → LF), §13.2.5 tokenization (all states), §13.2.5.72–80 character references, §13.5 named character references (`tests/data/html-entities.json`, generated table `src/html_entities.rs` by `tools/gen_html_entities.py`).
- **Code:** `src/html.rs` — `Tokenizer::new / with_state / next_token / all_tokens`, `Token`, `State`.
- **Conformance:** html5lib-tests tokenizer suite (`tests/data/html5lib-tokenizer/`, MIT; LICENSE there), run from every listed initial state: **6,941 / 6,941** cases pass. Includes namedEntities (4,210 tests as states), numericEntities, entities, unicodeChars, test1–4, contentModelFlags, escapeFlag, domjs, doubleEscaped cases.
- **Excluded, reported by the test:** 56 initial states "CDATA section state" (not a tokenizer state in this module); 52 tests that expect obsolete `ProcessingInstruction` tokens / `eof-in-processing-instruction` errors (a superseded draft; current WHATWG makes `<?` a bogus comment).
- **Known gaps / PARKED:**
  - **Tree construction (§13.2.6) is not implemented** — no DOM, no insertion modes, no adoption agency, no foster parenting. Reason: a full tree builder (23 insertion modes plus the foreign-content rules) is a separate module; the tokenizer stands alone. Next step: `html::tree` with the html5lib `tree-construction` tests as the conformance set.
  - CDATA sections in foreign content: `<![CDATA[` is always a bogus comment (no adjusted current node without a tree builder).
  - Parse errors are not reported.
- **Patent screen:** WHATWG HTML Living Standard — W3C Community Group / WHATWG royalty-free patent policy applies. No known patent encumbrances on the tokenizer algorithm.
