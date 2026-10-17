//! otshape tests: synthetic fonts built per the OpenType layouts (always run), and the HarfBuzz oracle fixture
//! (tests/data/otshape-oracle.tsv, produced by tools/oracle_otshape.py) run against the system fonts it names.

use omni_spec::otshape::{parse, Font};

/// Assemble an sfnt with the given tables (table directory, OpenType chapter 2).
fn sfnt(tables: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0u8; 6]);
    let mut offset = 12 + 16 * tables.len();
    for (tag, data) in tables {
        out.extend_from_slice(*tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        offset += data.len();
    }
    for (_, data) in tables {
        out.extend_from_slice(data);
    }
    out
}

fn be16(v: &mut Vec<u8>, x: i32) {
    v.extend_from_slice(&(x as i16 as u16).to_be_bytes());
}

/// Layout header (version 1.0) + one feature `tag` with one lookup (index 0) + lookup list with one lookup of `kind`
/// whose single subtable is `sub` (subtable bytes start right after the 8-byte lookup header).
fn layout(tag: &[u8; 4], kind: u16, sub: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    be16(&mut v, 0); // scriptList (unused)
    be16(&mut v, 10); // featureList
    be16(&mut v, 24); // lookupList
    // FeatureList at 10
    be16(&mut v, 1);
    v.extend_from_slice(tag);
    be16(&mut v, 8);
    be16(&mut v, 0); // featureParams
    be16(&mut v, 1);
    be16(&mut v, 0); // lookup index 0
    // LookupList at 24
    be16(&mut v, 1);
    be16(&mut v, 4);
    // Lookup at 28
    be16(&mut v, kind as i32);
    be16(&mut v, 0);
    be16(&mut v, 1);
    be16(&mut v, 8);
    v.extend_from_slice(sub);
    v
}

/// LigatureSubst (GSUB type 4): 'f' (102) + 'i' (105) -> 100 ; covers only glyph 102.
fn liga_subtable() -> Vec<u8> {
    let mut s = Vec::new();
    be16(&mut s, 1); // format
    be16(&mut s, 18); // coverage at +18
    be16(&mut s, 1); // ligSetCount
    be16(&mut s, 8); // LigatureSet at +8
    // LigatureSet at +8
    be16(&mut s, 1);
    be16(&mut s, 4); // Ligature at +12
    // Ligature at +12
    be16(&mut s, 100);
    be16(&mut s, 2);
    be16(&mut s, 105);
    // Coverage at +18
    be16(&mut s, 1);
    be16(&mut s, 1);
    be16(&mut s, 102);
    s
}

/// PairPos format 2 (GPOS type 2): XAdvance of class pairs. Covered lefts: 10 (class 1), 11 (class 0).
/// Right classes: 20..=21 -> class 1, everything else class 0. Matrix [c1][c2] = [0, -40, 25, 0].
fn pair2_subtable() -> Vec<u8> {
    let mut s = Vec::new();
    be16(&mut s, 2); // format
    be16(&mut s, 24); // coverage at +24
    be16(&mut s, 0x0004); // valueFormat1: XAdvance
    be16(&mut s, 0); // valueFormat2
    be16(&mut s, 32); // classDef1 at +32
    be16(&mut s, 42); // classDef2 at +42 (classDef1 is 10 bytes)
    be16(&mut s, 2); // class1Count
    be16(&mut s, 2); // class2Count
    for v in [0, -40, 25, 0] {
        be16(&mut s, v);
    }
    // Coverage at +24: glyphs 10, 11
    be16(&mut s, 1);
    be16(&mut s, 2);
    be16(&mut s, 10);
    be16(&mut s, 11);
    // ClassDef1 at +32: format 1, start 10, count 2, classes [1, 0]
    be16(&mut s, 1);
    be16(&mut s, 10);
    be16(&mut s, 2);
    be16(&mut s, 1);
    be16(&mut s, 0);
    // ClassDef2 at +42: format 2, one range 20..=21 class 1
    be16(&mut s, 2);
    be16(&mut s, 1);
    be16(&mut s, 20);
    be16(&mut s, 21);
    be16(&mut s, 1);
    s
}

/// Legacy kern table, version 0, one horizontal format 0 subtable with pairs sorted by (left, right).
fn kern_table() -> Vec<u8> {
    let pairs = [(10u16, 20u16, -50i32), (10, 30, 30), (40, 41, -7)];
    let mut t = Vec::new();
    be16(&mut t, 0); // version
    be16(&mut t, 1); // nTables
    be16(&mut t, 0); // subtable version
    be16(&mut t, (6 + 8 + 6 * pairs.len()) as i32); // length
    be16(&mut t, 0x0001); // coverage: horizontal, format 0
    be16(&mut t, pairs.len() as i32);
    be16(&mut t, 0);
    be16(&mut t, 0);
    be16(&mut t, 0);
    for (l, r, v) in pairs {
        be16(&mut t, l as i32);
        be16(&mut t, r as i32);
        be16(&mut t, v);
    }
    t
}

#[test]
fn synthetic_ligature_from_gsub() {
    let data = sfnt(&[(b"GSUB", layout(b"liga", 4, &liga_subtable()))]);
    let font: Font = parse(&data).expect("parse");
    assert_eq!(font.ligate(&[102, 105]).unwrap(), vec![100]);
    assert_eq!(font.ligate(&[102, 106]).unwrap(), vec![102, 106]);
    assert_eq!(font.ligate(&[102, 105, 102, 105]).unwrap(), vec![100, 100]);
    assert_eq!(font.ligate(&[105, 102]).unwrap(), vec![105, 102]);
}

#[test]
fn synthetic_class_pair_kerning_from_gpos() {
    let data = sfnt(&[(b"GPOS", layout(b"kern", 2, &pair2_subtable()))]);
    let font = parse(&data).expect("parse");
    assert_eq!(font.pair_advance(11, 20).unwrap(), -40);
    assert_eq!(font.pair_advance(10, 30).unwrap(), 25);
    assert_eq!(font.pair_advance(10, 20).unwrap(), 0);
    assert_eq!(font.pair_advance(11, 30).unwrap(), 0);
    // Not covered by the subtable: no GPOS kern value.
    assert_eq!(font.pair_advance(12, 20).unwrap(), 0);
}

#[test]
fn synthetic_legacy_kern_table_fallback() {
    let data = sfnt(&[(b"kern", kern_table())]);
    let font = parse(&data).expect("parse");
    assert_eq!(font.pair_advance(10, 20).unwrap(), -50);
    assert_eq!(font.pair_advance(10, 30).unwrap(), 30);
    assert_eq!(font.pair_advance(40, 41).unwrap(), -7);
    assert_eq!(font.pair_advance(41, 40).unwrap(), 0);
}

#[test]
fn malformed_input_is_an_error_not_a_panic() {
    assert!(parse(&[]).is_err());
    assert!(parse(b"ttcf\0\0\0\0").is_err());
    let mut data = sfnt(&[(b"GSUB", layout(b"liga", 4, &liga_subtable()))]);
    data.truncate(data.len() - 20);
    if let Ok(font) = parse(&data) {
        let _ = font.ligate(&[102, 105]);
    }
}

// ---- HarfBuzz oracle fixture -------------------------------------------------------------------------

fn find_font(name: &str) -> Option<std::path::PathBuf> {
    let roots = ["/usr/share/fonts/truetype", "/usr/share/fonts/opentype"];
    for root in roots {
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
fn harfbuzz_oracle_fixture() {
    let fixture = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/otshape-oracle.tsv")).expect("fixture");
    let (mut pairs, mut ligs, mut skipped) = (0usize, 0usize, 0usize);
    let mut failures = Vec::new();
    let mut cache: std::collections::HashMap<String, Vec<u8>> = Default::default();
    for line in fixture.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let name = f[1].to_string();
        let data = match cache.get(&name) {
            Some(d) => d.clone(),
            None => match find_font(&name) {
                Some(p) => {
                    let d = std::fs::read(p).expect("read font");
                    cache.insert(name.clone(), d.clone());
                    d
                }
                None => {
                    skipped += 1;
                    continue;
                }
            },
        };
        let font = parse(&data).expect("parse system font");
        match f[0] {
            "P" => {
                pairs += 1;
                let (l, r): (u16, u16) = (f[2].parse().unwrap(), f[3].parse().unwrap());
                let want: i16 = f[4].parse().unwrap();
                let got = font.pair_advance(l, r).expect("pair");
                if got != want {
                    failures.push(format!("P {name} {l} {r}: want {want} got {got}"));
                }
            }
            "L" => {
                ligs += 1;
                let ins: Vec<u16> = f[2].split(',').map(|x| x.parse().unwrap()).collect();
                let want: Vec<u16> = f[3].split(',').map(|x| x.parse().unwrap()).collect();
                let got = font.ligate(&ins).expect("ligate");
                if got != want {
                    failures.push(format!("L {name} {ins:?}: want {want:?} got {got:?}"));
                }
            }
            _ => {}
        }
    }
    eprintln!("otshape oracle: {} kern pairs, {} ligature strings checked, {} skipped (font absent)", pairs, ligs, skipped);
    for f in failures.iter().take(20) {
        eprintln!("{f}");
    }
    assert!(failures.is_empty(), "{} oracle mismatches", failures.len());
}
