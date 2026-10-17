//! UAX #9 conformance: official BidiTest.txt (class sequences, all paragraph directions) and BidiCharacterTest.txt
//! (code points with bracket pairs, explicit paragraph direction), Unicode 18.0.0, tests/data/.

use omni_spec::uax9::{bracket_of, class_of, resolve, Class, Direction};

fn class_from_name(s: &str) -> Class {
    match s {
        "L" => Class::L,
        "R" => Class::R,
        "AL" => Class::AL,
        "EN" => Class::EN,
        "ES" => Class::ES,
        "ET" => Class::ET,
        "AN" => Class::AN,
        "CS" => Class::CS,
        "NSM" => Class::NSM,
        "BN" => Class::BN,
        "B" => Class::B,
        "S" => Class::S,
        "WS" => Class::WS,
        "ON" => Class::ON,
        "LRE" => Class::LRE,
        "LRO" => Class::LRO,
        "RLE" => Class::RLE,
        "RLO" => Class::RLO,
        "PDF" => Class::PDF,
        "LRI" => Class::LRI,
        "RLI" => Class::RLI,
        "FSI" => Class::FSI,
        "PDI" => Class::PDI,
        other => panic!("unknown class {other}"),
    }
}

#[test]
fn bidi_test_txt() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/BidiTest.txt")).expect("BidiTest.txt");
    let (mut cases, mut failures) = (0usize, Vec::new());
    let mut levels: Vec<Option<u8>> = Vec::new();
    let mut reorder: Vec<usize> = Vec::new();
    for raw in text.lines() {
        if raw.is_empty() || raw.starts_with('#') {
            continue;
        }
        if let Some(rest) = raw.strip_prefix("@Levels:") {
            levels = rest
                .split_whitespace()
                .map(|t| if t == "x" { None } else { Some(t.parse::<u8>().unwrap()) })
                .collect();
            continue;
        }
        if let Some(rest) = raw.strip_prefix("@Reorder:") {
            reorder = rest.split_whitespace().map(|t| t.parse().unwrap()).collect();
            continue;
        }
        if raw.starts_with('@') {
            continue;
        }
        let (classes_s, bits_s) = raw.split_once(';').expect("class line");
        let classes: Vec<Class> = classes_s.split_whitespace().map(class_from_name).collect();
        let brackets = vec![None; classes.len()];
        let bits: u32 = bits_s.trim().parse().unwrap();
        for (bit, dir) in [(1u32, Direction::Auto), (2, Direction::Ltr), (4, Direction::Rtl)] {
            if bits & bit == 0 {
                continue;
            }
            cases += 1;
            let r = resolve(&classes, &brackets, dir);
            if r.levels != levels || r.order != reorder {
                failures.push(format!("{classes_s} [{dir:?}]: levels {:?} want {:?}; order {:?} want {:?}", r.levels, levels, r.order, reorder));
            }
        }
    }
    eprintln!("BidiTest.txt: {cases} cases, {} failures", failures.len());
    for f in failures.iter().take(15) {
        eprintln!("{f}");
    }
    assert!(failures.is_empty(), "{} failures", failures.len());
}

#[test]
fn bidi_character_test_txt() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/BidiCharacterTest.txt")).expect("BidiCharacterTest.txt");
    let (mut cases, mut failures) = (0usize, Vec::new());
    for raw in text.lines() {
        if raw.is_empty() || raw.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = raw.split(';').collect();
        let chars: Vec<char> = f[0].split_whitespace().map(|h| char::from_u32(u32::from_str_radix(h, 16).unwrap()).unwrap()).collect();
        let dir = match f[1] {
            "0" => Direction::Ltr,
            "1" => Direction::Rtl,
            _ => Direction::Auto,
        };
        let want_levels: Vec<Option<u8>> = f[3].split_whitespace().map(|t| if t == "x" { None } else { Some(t.parse().unwrap()) }).collect();
        let want_order: Vec<usize> = f[4].split_whitespace().map(|t| t.parse().unwrap()).collect();
        let want_para: u8 = f[2].parse().unwrap();
        let classes: Vec<Class> = chars.iter().map(|&c| class_of(c)).collect();
        let brackets: Vec<Option<(u32, bool)>> = chars.iter().map(|&c| bracket_of(c)).collect();
        let r = resolve(&classes, &brackets, dir);
        cases += 1;
        if r.levels != want_levels || r.order != want_order || r.para_level != want_para {
            failures.push(format!("{} dir {}: levels {:?} want {:?}; order {:?} want {:?}", f[0], f[1], r.levels, want_levels, r.order, want_order));
        }
    }
    eprintln!("BidiCharacterTest.txt: {cases} cases, {} failures", failures.len());
    for f in failures.iter().take(15) {
        eprintln!("{f}");
    }
    assert!(failures.is_empty(), "{} failures", failures.len());
}
