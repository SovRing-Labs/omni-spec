//! WOFF2 (W3C REC) decoder conformance. Every table except glyf/loca, rebuilt from the container (hmtx transform reversed), equals
//! fontTools' output byte for byte (`tools/oracle_woff2.py` → tests/data/woff2-oracle.tsv: length + SHA-256 per table). glyf/loca are
//! rebuilt to an equivalent encoding (fontTools pads glyphs to 2 bytes and chooses its own flag repeats), so they are compared glyph by
//! glyph by content through fontTools itself (`tools/check_woff2_vs_fonttools.py`, run when python3 + fontTools are importable via
//! OMNI_ORACLE_SITE). The 40 public fonts are read from FONT_DIR (not copied into the tree); a missing font is skipped and reported.
//! Also: our sfnt writer's output re-parses with `sfnt::parse` and holds the same tables; corrupted inputs never panic.

use omni_spec::sha256;
use omni_spec::sfnt;
use omni_spec::woff2::{decode, to_sfnt, Error};
use std::collections::BTreeMap;

const ORACLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/woff2-oracle.tsv");
const FONT_DIR: &str = "/usr/lib/claude-desktop/resources/ion-dist/assets/v1";

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

struct Row {
    flavor: u32,
    n_tables: usize,
    tables: BTreeMap<String, (usize, String)>,
}

fn oracle() -> BTreeMap<String, Row> {
    let mut m: BTreeMap<String, Row> = BTreeMap::new();
    for line in std::fs::read_to_string(ORACLE).expect("oracle tsv").lines() {
        let f: Vec<&str> = line.split('\t').collect();
        match f[0] {
            "F" => {
                m.insert(f[1].into(), Row { flavor: u32::from_str_radix(f[2], 16).unwrap(), n_tables: f[3].parse().unwrap(), tables: BTreeMap::new() });
            }
            "T" => {
                m.get_mut(f[1]).unwrap().tables.insert(f[2].into(), (f[3].parse().unwrap(), f[4].into()));
            }
            _ => {}
        }
    }
    m
}

#[test]
fn every_table_equals_fonttools() {
    let (mut fonts, mut tables, mut skipped) = (0, 0, 0);
    for (name, row) in oracle() {
        let Ok(data) = std::fs::read(format!("{FONT_DIR}/{name}")) else {
            skipped += 1;
            continue;
        };
        let w = decode(&data, 64 << 20).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(w.flavor, row.flavor, "{name}: flavor");
        assert_eq!(w.tables.len(), row.n_tables, "{name}: table count");
        for t in &w.tables {
            let tag = String::from_utf8_lossy(&t.tag).to_string();
            let (len, sha) = row.tables.get(&tag).unwrap_or_else(|| panic!("{name}: unexpected table {tag}"));
            if tag == "glyf" || tag == "loca" {
                continue; // equivalent encoding: compared by content in glyf_matches_fonttools_glyph_by_glyph
            }
            assert_eq!(t.data.len(), *len, "{name}/{tag}: length");
            assert_eq!(&hex(&sha256::digest(&t.data)), sha, "{name}/{tag}: bytes differ from fontTools");
            tables += 1;
        }
        fonts += 1;
    }
    eprintln!("woff2: {fonts} fonts, {tables} tables byte-equal to fontTools ({skipped} fonts absent from {FONT_DIR})");
    assert!(fonts > 0 || skipped > 0);
}

#[test]
fn sfnt_output_reparses_with_the_same_tables() {
    let mut checked = 0;
    for (name, _) in oracle().into_iter().take(12) {
        let Ok(data) = std::fs::read(format!("{FONT_DIR}/{name}")) else { continue };
        let w = decode(&data, 64 << 20).unwrap();
        let s = to_sfnt(&w).unwrap();
        let f = sfnt::parse(&s).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        for t in &w.tables {
            let got = f.table(&t.tag).unwrap();
            if &t.tag == b"head" {
                // checkSumAdjustment (bytes 8..12) is recomputed by the sfnt writer
                assert_eq!((&got[..8], &got[12..]), (&t.data[..8], &t.data[12..]), "{name}/head");
            } else {
                assert_eq!(got, t.data.as_slice(), "{name}/{}", String::from_utf8_lossy(&t.tag));
            }
        }
        // OpenType `head`: the whole file sums to 0xB1B0AFBA
        let mut sum: u32 = 0;
        for c in s.chunks(4) {
            let mut b = [0u8; 4];
            b[..c.len()].copy_from_slice(c);
            sum = sum.wrapping_add(u32::from_be_bytes(b));
        }
        assert_eq!(sum, 0xB1B0_AFBA, "{name}: file checksum");
        checked += 1;
    }
    eprintln!("woff2 -> sfnt: {checked} fonts re-parsed");
}

#[test]
fn corrupted_and_truncated_inputs_never_panic() {
    let Some(name) = oracle().into_keys().next() else { return };
    let Ok(data) = std::fs::read(format!("{FONT_DIR}/{name}")) else { return };
    let mut s = 0x2545F4914F6CDD1Du64;
    let mut next = || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    };
    let (mut ok, mut err) = (0, 0);
    for _ in 0..400 {
        let mut d = data.clone();
        for _ in 0..(1 + next() % 4) {
            let i = (next() as usize) % d.len();
            d[i] ^= 1 << (next() % 8);
        }
        match decode(&d, 8 << 20) {
            Ok(_) => ok += 1,
            Err(_) => err += 1,
        }
    }
    for cut in (0..data.len()).step_by(97) {
        assert!(decode(&data[..cut], 8 << 20).is_err() || cut == data.len());
    }
    assert_eq!(decode(&data[..8], 1 << 20).unwrap_err(), Error::Truncated);
    eprintln!("woff2 fuzz: 400 bit-flip variants ({ok} decoded, {err} refused), truncations refused");
}

#[test]
fn glyf_matches_fonttools_glyph_by_glyph() {
    let Ok(site) = std::env::var("OMNI_ORACLE_SITE").or_else(|_| Ok::<_, ()>("/tmp/claude-1000/pysite".into())) else { return };
    if std::process::Command::new("python3").args(["-c", "import fontTools,brotli"]).env("PYTHONPATH", &site).output().map(|o| !o.status.success()).unwrap_or(true) {
        eprintln!("fontTools not importable (set OMNI_ORACLE_SITE): skipped");
        return;
    }
    if !std::path::Path::new(FONT_DIR).is_dir() {
        return;
    }
    let out = std::env::temp_dir().join(format!("omni-woff2-{}", std::process::id()));
    let mut n = 0;
    for (name, _) in oracle() {
        let Ok(data) = std::fs::read(format!("{FONT_DIR}/{name}")) else { continue };
        let w = decode(&data, 64 << 20).unwrap();
        let d = out.join(&name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("font.ttf"), to_sfnt(&w).unwrap()).unwrap();
        n += 1;
    }
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tools/check_woff2_vs_fonttools.py");
    let o = std::process::Command::new("python3").arg(script).arg(FONT_DIR).arg(&out).env("PYTHONPATH", &site).output().unwrap();
    let _ = std::fs::remove_dir_all(&out);
    let text = String::from_utf8_lossy(&o.stdout);
    eprintln!("{}", text.lines().last().unwrap_or(""));
    assert!(o.status.success() && text.contains("0 fonts with differences") && n > 0, "{text}\n{}", String::from_utf8_lossy(&o.stderr));
}
