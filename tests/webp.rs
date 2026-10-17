//! WebP lossless (VP8L) conformance against Pillow / libwebp (tests/data/webp/, index tests/data/webp-oracle.tsv,
//! tools/oracle_webp.py). Lossless output must equal the reference RGBA8 exactly.

use omni_spec::webp::decode;

fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

#[test]
fn vp8l_matches_libwebp_exactly() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/webp");
    let index = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/webp-oracle.tsv")).expect("index");
    let mut failures = Vec::new();
    let mut checked = 0usize;
    for line in index.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let data = std::fs::read(format!("{dir}/{}", f[0])).unwrap();
        let (w, h): (usize, usize) = (f[1].parse().unwrap(), f[2].parse().unwrap());
        let want = u64::from_str_radix(f[3], 16).unwrap();
        checked += 1;
        match decode(&data) {
            Ok(img) => {
                if (img.width, img.height) != (w, h) || fnv1a64(&img.rgba) != want {
                    failures.push(format!("{}: pixel mismatch", f[0]));
                }
            }
            Err(e) => failures.push(format!("{}: {:?}", f[0], e)),
        }
    }
    eprintln!("webp lossless oracle: {checked} files checked");
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn lossy_vp8_is_reported_unsupported() {
    // RIFF 'WEBP' with a 'VP8 ' chunk of 0 bytes: a clean error, not a panic.
    let f = b"RIFF\x0e\x00\x00\x00WEBPVP8 \x00\x00\x00\x00".to_vec();
    assert!(decode(&f).is_err());
}
