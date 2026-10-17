//! inflate conformance and robustness.
//! - PNG IDAT streams of tests/data corpus: outputs must match the python zlib oracle (length + FNV-1a 64).
//! - Corrupted and random inputs must return Err or Ok, never panic.

use omni_spec::inflate::{inflate, zlib_decompress};

fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// Concatenate IDAT payloads of a PNG (chunk layout: length, type, data, CRC).
fn idat(png: &[u8]) -> Vec<u8> {
    let mut pos = 8;
    let mut out = Vec::new();
    while pos + 8 <= png.len() {
        let n = u32::from_be_bytes(png[pos..pos + 4].try_into().unwrap()) as usize;
        if &png[pos + 4..pos + 8] == b"IDAT" && pos + 8 + n <= png.len() {
            out.extend_from_slice(&png[pos + 8..pos + 8 + n]);
        }
        pos += 12 + n;
    }
    out
}

#[test]
fn corpus_png_idat_matches_zlib_oracle() {
    let fixture = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/inflate-oracle.tsv")).expect("fixture");
    let corpus = concat!(env!("CARGO_MANIFEST_DIR"), "/../omni-render/corpus");
    let (mut checked, mut skipped) = (0usize, 0usize);
    let mut failures = Vec::new();
    for line in fixture.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let path = format!("{corpus}/{}", f[0]);
        let Ok(png) = std::fs::read(&path) else {
            skipped += 1;
            continue;
        };
        let stream = idat(&png);
        let want_len: usize = f[2].parse().unwrap();
        let want_hash = u64::from_str_radix(f[3], 16).unwrap();
        match zlib_decompress(&stream) {
            Ok(out) => {
                checked += 1;
                if out.len() != want_len || fnv1a64(&out) != want_hash {
                    failures.push(format!("{}: len {} want {}", f[0], out.len(), want_len));
                }
            }
            Err(e) => failures.push(format!("{}: error {:?}", f[0], e)),
        }
    }
    eprintln!("inflate corpus: {checked} streams match, {skipped} skipped (absent)");
    assert!(failures.is_empty(), "{:?}", &failures[..failures.len().min(10)]);
}

#[test]
fn rfc_stored_and_fixed_examples() {
    assert_eq!(inflate(&[0x01, 0x03, 0x00, 0xFC, 0xFF, b'a', b'b', b'c']).unwrap(), b"abc");
    assert_eq!(inflate(&[0x4B, 0x4C, 0x4C, 0x4C, 0x04, 0x00]).unwrap(), b"aaaa");
}

#[test]
fn corrupt_and_random_inputs_never_panic() {
    // Deterministic xorshift so failures reproduce.
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    // Bit flips and truncations of a real stream.
    let base = zlib_decompress(&[0x78, 0x9C, 0x4B, 0x4C, 0x4C, 0x4C, 0x04, 0x00, 0x03, 0x02, 0x04, 0x6A]);
    let _ = base;
    let stream = {
        let png = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../omni-render/corpus/amazon-com/chrome.png")).unwrap_or_default();
        idat(&png)
    };
    for _ in 0..2000 {
        if stream.is_empty() {
            break;
        }
        let mut s = stream[..(next() as usize % stream.len().min(4096)).max(1)].to_vec();
        let i = next() as usize % s.len();
        s[i] ^= 1 << (next() % 8);
        let _ = zlib_decompress(&s);
        let _ = inflate(&s);
    }
    // Pure random bytes.
    for _ in 0..20_000 {
        let n = (next() % 64) as usize;
        let v: Vec<u8> = (0..n).map(|_| next() as u8).collect();
        let _ = zlib_decompress(&v);
        let _ = inflate(&v);
    }
}
