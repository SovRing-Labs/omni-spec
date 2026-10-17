//! UAX #29 conformance: official GraphemeBreakTest.txt (Unicode 18.0.0, tests/data/GraphemeBreakTest.txt).
//! Each line is a code point sequence with `÷` (boundary) and `×` (no boundary) between the code points.

use omni_spec::uax29::boundaries_chars;

#[test]
fn grapheme_break_test_conformance() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/GraphemeBreakTest.txt");
    let text = std::fs::read_to_string(path).expect("GraphemeBreakTest.txt present in tests/data");

    let mut total = 0usize;
    let mut failures: Vec<String> = Vec::new();
    for (lineno, raw) in text.lines().enumerate() {
        let data = raw.split('#').next().unwrap_or("").trim();
        if data.is_empty() {
            continue;
        }
        total += 1;
        let mut chars: Vec<char> = Vec::new();
        let mut expected: Vec<usize> = Vec::new();
        for tok in data.split_whitespace() {
            match tok {
                "÷" => expected.push(chars.len()),
                "×" => {}
                hex => chars.push(char::from_u32(u32::from_str_radix(hex, 16).expect("hex code point")).expect("valid scalar")),
            }
        }
        let got = boundaries_chars(&chars);
        if got != expected {
            failures.push(format!("line {}: {}\n  expected {:?}\n  got      {:?}", lineno + 1, data, expected, got));
        }
    }

    let passed = total - failures.len();
    eprintln!("UAX29 GraphemeBreakTest: {passed}/{total} pass");
    for f in failures.iter().take(20) {
        eprintln!("{f}");
    }
    assert!(failures.is_empty(), "{} of {} GraphemeBreakTest lines fail", failures.len(), total);
}
