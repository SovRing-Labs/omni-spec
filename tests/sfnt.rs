//! sfnt conformance against the fontTools + HarfBuzz oracle (tests/data/sfnt-oracle.tsv, tools/oracle_sfnt.py).
//! Fonts are read in place from /usr/share/fonts; a missing font is reported and skipped.

use omni_spec::sfnt::parse;
use std::collections::HashMap;

fn find_font(name: &str) -> Option<std::path::PathBuf> {
    for root in ["/usr/share/fonts/truetype", "/usr/share/fonts/opentype"] {
        for dir in std::fs::read_dir(root).ok()?.flatten() {
            let p = dir.path().join(name);
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}

#[test]
fn matches_fonttools_and_harfbuzz_oracle() {
    let fixture = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/sfnt-oracle.tsv")).expect("fixture");
    let mut fonts: HashMap<String, Vec<u8>> = HashMap::new();
    let (mut checks, mut skipped) = (0usize, 0usize);
    let mut failures = Vec::new();
    for line in fixture.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let name = f[1].to_string();
        if !fonts.contains_key(&name) {
            match find_font(&name) {
                Some(p) => {
                    fonts.insert(name.clone(), std::fs::read(p).unwrap());
                }
                None => {
                    skipped += 1;
                    continue;
                }
            }
        }
        let font = parse(&fonts[&name]).expect("parse");
        checks += 1;
        match f[0] {
            "F" => {
                let (ng, upem, nh): (u16, u16, u16) = (f[2].parse().unwrap(), f[3].parse().unwrap(), f[4].parse().unwrap());
                let got = (font.num_glyphs().unwrap(), font.units_per_em().unwrap(), font.num_h_metrics().unwrap());
                if got != (ng, upem, nh) {
                    failures.push(format!("F {name}: got {got:?} want {:?}", (ng, upem, nh)));
                }
            }
            "A" => {
                let gid: u16 = f[2].parse().unwrap();
                let (adv, lsb): (u16, i16) = (f[3].parse().unwrap(), f[4].parse().unwrap());
                match font.h_metrics(gid) {
                    Ok((a, l)) if a == adv && l == lsb => {}
                    other => failures.push(format!("A {name} gid {gid}: got {other:?} want ({adv},{lsb})")),
                }
            }
            "C" => {
                let cp: u32 = f[2].parse().unwrap();
                let want: u16 = f[3].parse().unwrap();
                let got = font.cmap(cp).unwrap().unwrap_or(0);
                if got != want {
                    failures.push(format!("C {name} U+{cp:04X}: got {got} want {want}"));
                }
            }
            "N" => {
                let want = f.get(2).copied().unwrap_or("");
                let got = font.family_name().unwrap().unwrap_or_default();
                if got != want {
                    failures.push(format!("N {name}: got {got:?} want {want:?}"));
                }
            }
            "O" => {
                let v: Vec<i64> = f[2..].iter().map(|x| x.parse().unwrap()).collect();
                let o = font.os2_metrics().unwrap();
                let got = [o.weight_class as i64, o.typo_ascender as i64, o.typo_descender as i64, o.typo_line_gap as i64, o.win_ascent as i64, o.win_descent as i64];
                if got[..] != v[..] {
                    failures.push(format!("O {name}: got {got:?} want {v:?}"));
                }
            }
            _ => {}
        }
    }
    eprintln!("sfnt oracle: {checks} rows checked, {skipped} skipped (font absent)");
    for f in failures.iter().take(20) {
        eprintln!("{f}");
    }
    assert!(failures.is_empty(), "{} mismatches", failures.len());
}
