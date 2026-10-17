//! GIF conformance against Pillow (tests/data/gif-oracle.tsv, tools/oracle_gif.py): frame 0 composed to RGBA8,
//! alpha-0 RGB zeroed on both sides. Files are read in place from /usr/share and /usr/lib.

use omni_spec::gif::{decode, lzw_decode};

fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

fn normalise(mut rgba: Vec<u8>) -> Vec<u8> {
    for px in rgba.chunks_mut(4) {
        if px[3] == 0 {
            px[0] = 0;
            px[1] = 0;
            px[2] = 0;
        }
    }
    rgba
}

#[test]
fn invalid_lzw_code_is_an_error() {
    // min code size 2: clear=4, end=5, first code 6 is beyond the dictionary (next = 6) before any entry exists
    // only when it is not a literal: a first code >= clear right after clear must be rejected.
    let bytes = [0b0000_0110u8, 0];
    assert!(lzw_decode(&bytes, 2, 1).is_err());
    assert!(lzw_decode(&[0xFF], 0, 1).is_err());
}

#[test]
fn corpus_frame0_matches_pillow() {
    let fixture = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/gif-oracle.tsv")).expect("fixture");
    let (mut checked, mut skipped) = (0usize, 0usize);
    let mut failures = Vec::new();
    for line in fixture.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let Ok(data) = std::fs::read(f[0]) else {
            skipped += 1;
            continue;
        };
        let (w, h): (u16, u16) = (f[1].parse().unwrap(), f[2].parse().unwrap());
        let want = u64::from_str_radix(f[3], 16).unwrap();
        match decode(&data) {
            Ok(g) => {
                checked += 1;
                let rgba = normalise(g.first_frame_rgba());
                if (g.width, g.height) != (w, h) || fnv1a64(&rgba) != want {
                    failures.push(format!("{} ({:?}, {} frames): hash/size mismatch", f[0], g.version, g.frames.len()));
                }
            }
            Err(e) => failures.push(format!("{}: {:?}", f[0], e)),
        }
    }
    eprintln!("gif oracle: {checked} files checked, {skipped} skipped (absent)");
    for f in failures.iter().take(20) {
        eprintln!("{f}");
    }
    assert!(failures.is_empty(), "{} mismatches", failures.len());
}
