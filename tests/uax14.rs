//! UAX #14 conformance: official LineBreakTest.txt (Unicode 18.0.0, tests/data/LineBreakTest.txt).
//! Each line is a code point sequence with `÷` (break opportunity) and `×` (no break) between the code points.

use omni_spec::uax14::breaks_chars;

#[test]
fn line_break_test_conformance() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/LineBreakTest.txt");
    let text = std::fs::read_to_string(path).expect("LineBreakTest.txt present in tests/data");

    let mut total = 0usize;
    let mut failures: Vec<String> = Vec::new();
    for (lineno, raw) in text.lines().enumerate() {
        let data = raw.split('#').next().unwrap_or("").trim();
        if data.is_empty() {
            continue;
        }
        total += 1;

        // Walk the tokens: hex code points and break markers.
        let mut chars: Vec<char> = Vec::new();
        let mut expected: Vec<usize> = Vec::new();
        for tok in data.split_whitespace() {
            match tok {
                "÷" => expected.push(chars.len()),
                "×" => {}
                hex => chars.push(char::from_u32(u32::from_str_radix(hex, 16).expect("hex code point")).expect("valid scalar")),
            }
        }

        let got: Vec<usize> = breaks_chars(&chars).into_iter().map(|b| b.index).collect();
        // Expected output never includes gap 0 (LB2), so compare with index 0 removed.
        let got: Vec<usize> = got.into_iter().filter(|&i| i != 0).collect();
        let expected: Vec<usize> = expected.into_iter().filter(|&i| i != 0).collect();
        if got != expected && failures.len() < 10_000 {
            failures.push(format!("line {}: {}\n  expected {:?}\n  got      {:?}", lineno + 1, data, expected, got));
        }
    }

    let passed = total - failures.len();
    eprintln!("UAX14 LineBreakTest: {passed}/{total} pass");
    for f in failures.iter().take(20) {
        eprintln!("{f}");
    }
    assert!(failures.is_empty(), "{} of {} LineBreakTest lines fail", failures.len(), total);
}
