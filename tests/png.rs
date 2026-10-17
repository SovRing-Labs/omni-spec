//! PNG conformance: (1) Pillow oracle over the omni-render corpus and system PNGs (tests/data/png-oracle.tsv,
//! produced by tools/oracle_png.py); (2) synthetic images for colour types/depths the oracle set lacks.

use omni_spec::inflate::adler32;
use omni_spec::png::{crc32, decode, Error, Image};

fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

#[test]
fn pillow_oracle_rgba_matches() {
    let fixture = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/png-oracle.tsv")).expect("fixture");
    let corpus = concat!(env!("CARGO_MANIFEST_DIR"), "/../omni-render/corpus");
    let (mut checked, mut skipped) = (0usize, 0usize);
    let mut failures = Vec::new();
    for line in fixture.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        let path = if f[0].starts_with('/') { f[0].to_string() } else { format!("{corpus}/{}", f[0]) };
        let Ok(data) = std::fs::read(&path) else {
            skipped += 1;
            continue;
        };
        let (w, h): (u32, u32) = (f[1].parse().unwrap(), f[2].parse().unwrap());
        let want = u64::from_str_radix(f[3], 16).unwrap();
        match decode(&data) {
            Ok(Image { width, height, rgba }) => {
                checked += 1;
                if (width, height) != (w, h) || fnv1a64(&rgba) != want {
                    failures.push(format!("{} (ct {} bd {} il {}): size {}x{} hash mismatch", f[0], f[4], f[5], f[6], width, height));
                }
            }
            Err(e) => failures.push(format!("{} (ct {} bd {} il {}): {:?}", f[0], f[4], f[5], f[6], e)),
        }
    }
    eprintln!("png oracle: {checked} images match Pillow RGBA8, {skipped} skipped (absent)");
    assert!(failures.is_empty(), "{:?}", &failures[..failures.len().min(10)]);
}

/// Build a PNG: IHDR + one IDAT (zlib with stored deflate blocks) + IEND. `raw` already includes filter bytes.
fn make_png(w: u32, h: u32, depth: u8, color: u8, trns: Option<&[u8]>, plte: Option<&[u8]>, raw: &[u8]) -> Vec<u8> {
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let chunk = |out: &mut Vec<u8>, ty: &[u8; 4], body: &[u8]| {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(ty);
        out.extend_from_slice(body);
        out.extend_from_slice(&crc32(&[ty, body]).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[depth, color, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    if let Some(p) = plte {
        chunk(&mut out, b"PLTE", p);
    }
    if let Some(t) = trns {
        chunk(&mut out, b"tRNS", t);
    }
    let mut z = vec![0x78, 0x01];
    z.push(0x01);
    let len = raw.len() as u16;
    z.extend_from_slice(&len.to_le_bytes());
    z.extend_from_slice(&(!len).to_le_bytes());
    z.extend_from_slice(raw);
    z.extend_from_slice(&adler32(raw).to_be_bytes());
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

#[test]
fn gray_1_2_4_16_bit_scaling_and_trns() {
    // 1-bit gray, 10 px wide: 1010101010 -> 255,0,255,...
    let p = make_png(10, 1, 1, 0, None, None, &[0, 0xAA, 0x80]);
    let img = decode(&p).unwrap();
    let g: Vec<u8> = img.rgba.chunks(4).map(|c| c[0]).collect();
    assert_eq!(g, vec![255, 0, 255, 0, 255, 0, 255, 0, 255, 0]);
    assert!(img.rgba.chunks(4).all(|c| c[3] == 255));

    // 2-bit gray with tRNS key 1: sample 1 becomes transparent. Samples 0,1,2,3 -> 0,85,170,255.
    let p = make_png(4, 1, 2, 0, Some(&[0, 1]), None, &[0, 0b00_01_10_11]);
    let img = decode(&p).unwrap();
    let px: Vec<(u8, u8)> = img.rgba.chunks(4).map(|c| (c[0], c[3])).collect();
    assert_eq!(px, vec![(0, 255), (85, 0), (170, 255), (255, 255)]);

    // 4-bit gray: 15 -> 255, 8 -> 136.
    let p = make_png(2, 1, 4, 0, None, None, &[0, 0xF8]);
    let img = decode(&p).unwrap();
    assert_eq!(img.rgba[0], 255);
    assert_eq!(img.rgba[4], 136);

    // 16-bit gray keeps the high byte.
    let p = make_png(1, 1, 16, 0, None, None, &[0, 0x80, 0x12]);
    assert_eq!(decode(&p).unwrap().rgba[0], 0x80);
}

#[test]
fn palette_2_bit_with_alpha() {
    // Palette: red, green, blue, white; alpha 0 for index 2.
    let plte = [255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255];
    let trns = [255, 255, 0, 255];
    let p = make_png(4, 1, 2, 3, Some(&trns), Some(&plte), &[0, 0b00_01_10_11]);
    let img = decode(&p).unwrap();
    assert_eq!(&img.rgba[0..4], &[255, 0, 0, 255]);
    assert_eq!(&img.rgba[4..8], &[0, 255, 0, 255]);
    assert_eq!(&img.rgba[8..12], &[0, 0, 255, 0]);
    assert_eq!(&img.rgba[12..16], &[255, 255, 255, 255]);
}

#[test]
fn bad_crc_and_bad_signature_are_errors() {
    let mut p = make_png(1, 1, 8, 0, None, None, &[0, 7]);
    let n = p.len();
    p[n - 1] ^= 1; // corrupt IEND CRC
    assert!(matches!(decode(&p), Err(Error::BadCrc(_))));
    assert_eq!(decode(b"not a png"), Err(Error::BadSignature));
}

#[test]
fn palette_index_out_of_range_is_an_error() {
    let plte = [1, 2, 3];
    let p = make_png(1, 1, 8, 3, None, Some(&plte), &[0, 5]);
    assert_eq!(decode(&p), Err(Error::BadPalette));
}

#[test]
fn invalid_colour_depth_combination_is_rejected() {
    // Colour type 2 (RGB) does not allow bit depth 4.
    let p = make_png(1, 1, 4, 2, None, None, &[0, 0, 0, 0]);
    assert!(matches!(decode(&p), Err(Error::BadHeader(_))));
}
