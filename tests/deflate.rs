//! DEFLATE writer (RFC 1951) conformance: every input round-trips through our `inflate`, and the stream is accepted by an
//! independent decoder run as a black box (Python zlib; Info-ZIP `unzip -t` for the zip method) when installed.
//! Compression ratio is reported, never gated against another implementation's output.

use omni_spec::deflate::{deflate_raw, zlib_compress, Error};
use omni_spec::inflate::{inflate_raw, zlib_decompress};
use omni_spec::zip::{Archive, Writer};

const DATA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data");

fn xorshift(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}

fn corpus() -> Vec<(String, Vec<u8>)> {
    let mut c: Vec<(String, Vec<u8>)> = vec![("empty".into(), vec![]), ("one".into(), vec![7]), ("zeros 1M".into(), vec![0; 1 << 20])];
    let mut s = 0x9E3779B97F4A7C15u64;
    c.push(("random 300k".into(), (0..300_000).map(|_| xorshift(&mut s) as u8).collect()));
    c.push(("skewed 200k".into(), (0..200_000).map(|_| (xorshift(&mut s) % 7) as u8 * 3).collect()));
    c.push(("all lengths".into(), (1..=258usize).flat_map(|l| std::iter::repeat_n(b'a' + (l % 26) as u8, l)).collect()));
    c.push(("far repeats".into(), {
        let blk: Vec<u8> = (0..4000).map(|_| xorshift(&mut s) as u8).collect();
        let mut v = blk.clone();
        v.extend((0..30_000).map(|_| xorshift(&mut s) as u8));
        v.extend(&blk);
        v
    }));
    for name in ["UnicodeData.txt", "BidiTest.txt", "emoji-data.txt", "DerivedCoreProperties.txt", "EastAsianWidth.txt"] {
        if let Ok(b) = std::fs::read(format!("{DATA}/{name}")) {
            c.push((name.into(), b));
        }
    }
    if let Ok(rd) = std::fs::read_dir(format!("{DATA}/zip/src")) {
        for e in rd.flatten() {
            if let Ok(b) = std::fs::read(e.path()) {
                c.push((format!("zip/src/{}", e.file_name().to_string_lossy()), b));
            }
        }
    }
    c
}

#[test]
fn corpus_round_trips_through_our_inflate() {
    let mut total_in = 0usize;
    let mut total_out = 0usize;
    for (name, d) in corpus() {
        let c = deflate_raw(&d, usize::MAX).unwrap();
        let (back, used) = inflate_raw(&c, usize::MAX).unwrap();
        assert!(back == d, "{name}: round trip differs");
        assert_eq!(used, c.len(), "{name}: trailing bytes");
        let z = zlib_compress(&d, usize::MAX).unwrap();
        assert!(zlib_decompress(&z).unwrap() == d, "{name}: zlib wrapper");
        // never worse than stored by more than one 5-byte stored-block header per 16 KiB token block
        assert!(c.len() <= d.len() + 5 * (d.len() / 16384 + 1) + 5, "{name}: expanded {} -> {}", d.len(), c.len());
        eprintln!("deflate {name}: {} -> {} ({:.1}%)", d.len(), c.len(), 100.0 * c.len() as f64 / d.len().max(1) as f64);
        total_in += d.len();
        total_out += c.len();
    }
    eprintln!("deflate corpus total: {total_in} -> {total_out}");
}

#[test]
fn python_zlib_decodes_our_streams_byte_equal() {
    let dir = std::env::temp_dir().join(format!("omni-deflate-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut names = Vec::new();
    for (i, (name, d)) in corpus().into_iter().enumerate() {
        std::fs::write(dir.join(format!("{i}.raw")), &d).unwrap();
        std::fs::write(dir.join(format!("{i}.z")), zlib_compress(&d, usize::MAX).unwrap()).unwrap();
        names.push(name);
    }
    let script = "import sys,zlib\nd=sys.argv[1]\nn=int(sys.argv[2])\nbad=[i for i in range(n) if zlib.decompress(open(f'{d}/{i}.z','rb').read())!=open(f'{d}/{i}.raw','rb').read()]\nprint('MISMATCH',bad) if bad else print('OK',n)\n";
    let out = std::process::Command::new("python3").arg("-I").arg("-c").arg(script).arg(&dir).arg(names.len().to_string()).output();
    let _ = std::fs::remove_dir_all(&dir);
    match out {
        Ok(o) => {
            let s = String::from_utf8_lossy(&o.stdout);
            assert!(o.status.success() && s.starts_with("OK"), "python zlib: {s} {}", String::from_utf8_lossy(&o.stderr));
            eprintln!("python zlib decoded {} streams byte-equal", names.len());
        }
        Err(_) => eprintln!("python3 not installed: skipped"),
    }
}

#[test]
fn output_limit_is_enforced() {
    let mut s = 12345u64;
    let d: Vec<u8> = (0..50_000).map(|_| xorshift(&mut s) as u8).collect();
    assert_eq!(deflate_raw(&d, 1000), Err(Error::OutputLimit));
}

#[test]
fn zip_add_deflated_round_trip_and_unzip_t() {
    let xml: Vec<u8> = b"<row r=\"1\"><c r=\"A1\" t=\"s\"><v>0</v></c><c r=\"B1\"><v>12.5</v></c></row>".iter().cycle().take(60_000).cloned().collect();
    let mut s = 99u64;
    let noise: Vec<u8> = (0..5000).map(|_| xorshift(&mut s) as u8).collect();
    let mut w = Writer::new();
    w.add_deflated("xl/worksheets/sheet1.xml", &xml).unwrap();
    w.add_deflated("media/image1.bin", &noise).unwrap(); // incompressible: falls back to stored
    w.add_deflated("名前.txt", b"x").unwrap(); // tiny: stored
    let z = w.finish().unwrap();
    let a = Archive::parse(&z).unwrap();
    let e = a.find("xl/worksheets/sheet1.xml").unwrap();
    assert_eq!(e.method, 8);
    assert!((e.compressed_size as usize) < xml.len() / 20, "{} of {}", e.compressed_size, xml.len());
    assert_eq!(a.read(e).unwrap(), xml);
    assert_eq!(a.find("media/image1.bin").unwrap().method, 0);
    assert_eq!(a.read_name("media/image1.bin").unwrap(), noise);
    let p = std::env::temp_dir().join(format!("omni-zip-deflated-{}.zip", std::process::id()));
    std::fs::write(&p, &z).unwrap();
    if let Ok(o) = std::process::Command::new("unzip").arg("-t").arg(&p).output() {
        let out = String::from_utf8_lossy(&o.stdout).to_string();
        let _ = std::fs::remove_file(&p);
        assert!(o.status.success() && out.contains("No errors detected"), "unzip -t: {out}");
    }
}
