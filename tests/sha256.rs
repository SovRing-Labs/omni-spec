//! SHA-256 against the FIPS 180-4 example values (NIST CSRC "Examples with Intermediate Values": "abc", the 448-bit
//! two-block message) plus the empty string and one million "a", and incremental == one-shot on random splits.

use omni_spec::sha256::{digest, hex, Sha256};

#[test]
fn fips_examples() {
    assert_eq!(hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    assert_eq!(hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"), "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1");
    assert_eq!(hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    assert_eq!(hex(&vec![b'a'; 1_000_000]), "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
}

#[test]
fn incremental_matches_one_shot() {
    let data: Vec<u8> = (0..5000u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
    for len in [0, 1, 55, 56, 57, 63, 64, 65, 119, 120, 128, 1000, 5000] {
        let one = digest(&data[..len]);
        for step in [1, 3, 7, 64, 100] {
            let mut s = Sha256::new();
            for c in data[..len].chunks(step) {
                s.update(c);
            }
            assert_eq!(s.finish(), one, "len {len} step {step}");
        }
    }
}
