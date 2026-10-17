//! Brotli conformance: (1) RFC 7932 §11.1 trivial-compressor streams (spec example); (2) black-box oracle streams
//! from the Python brotli encoder (tests/data/brotli/, index tests/data/brotli-oracle.tsv, FNV-1a 64 of the original);
//! (3) corrupted and random inputs never panic.

use omni_spec::brotli::decompress;

fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

#[test]
fn rfc7932_trivial_compressor_streams() {
    // Empty input: a single byte 6 (RFC 7932 §11.1).
    assert_eq!(decompress(&[6]).unwrap(), b"");
    // "abc": WBITS=16 (byte 12), one uncompressed meta-block of 3 bytes: (r & 31) << 3 = 16, r >> 5 = 0, 8 + (r >> 13) = 8,
    // then the bytes, then the final empty meta-block marker 3.
    assert_eq!(decompress(&[12, 16, 0, 8, b'a', b'b', b'c', 3]).unwrap(), b"abc");
}

#[test]
fn oracle_streams_match_python_brotli() {
    let index = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/brotli-oracle.tsv")).expect("index");
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/brotli");
    let mut failures = Vec::new();
    let mut checked = 0usize;
    for line in index.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let stream = std::fs::read(format!("{dir}/{}", f[0])).expect("stream file");
        let want_len: usize = f[1].parse().unwrap();
        let want_hash = u64::from_str_radix(f[2], 16).unwrap();
        checked += 1;
        match decompress(&stream) {
            Ok(out) => {
                if out.len() != want_len || fnv1a64(&out) != want_hash {
                    failures.push(format!("{}: len {} want {}", f[0], out.len(), want_len));
                }
            }
            Err(e) => failures.push(format!("{}: {:?}", f[0], e)),
        }
    }
    eprintln!("brotli oracle: {checked} streams checked");
    assert!(failures.is_empty(), "{:?}", failures);
}

#[test]
fn corrupt_and_random_inputs_never_panic() {
    let mut x: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/brotli");
    let base = std::fs::read(format!("{dir}/text-q11-w22.br")).unwrap();
    for _ in 0..1500 {
        let mut s = base[..(next() as usize % base.len()).max(1)].to_vec();
        let i = next() as usize % s.len();
        s[i] ^= 1 << (next() % 8);
        let _ = decompress(&s);
    }
    for _ in 0..20_000 {
        let n = (next() % 96) as usize;
        let v: Vec<u8> = (0..n).map(|_| next() as u8).collect();
        let _ = decompress(&v);
    }
}
