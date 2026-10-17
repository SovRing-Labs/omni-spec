//! ZIP (APPNOTE 6.3.10) conformance: archives written by Info-ZIP zip 3.0 (tools/make-zip-fixtures.sh), our writer read
//! back by us and by Info-ZIP `unzip -t` (when installed), corruption and random inputs never panic.

use omni_spec::zip::{crc32, decode_cp437, Archive, Error, Writer};

const D: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/zip");

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{D}/{name}")).expect(name)
}
fn src(name: &str) -> Vec<u8> {
    std::fs::read(format!("{D}/src/{name}")).expect(name)
}

#[test]
fn crc32_check_value() {
    // ISO 3309 / ITU-T V.42 check value for "123456789".
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    assert_eq!(crc32(b""), 0);
}

#[test]
fn cp437_table_is_complete() {
    let all: Vec<u8> = (0x80..=0xFF).collect();
    let s = decode_cp437(&all);
    assert_eq!(s.chars().count(), 128);
    assert_eq!(decode_cp437(&[0x80, 0x81, 0xE1, 0xFF]), "Çüß\u{a0}");
}

#[test]
fn stored_archive() {
    let z = fixture("stored.zip");
    let a = Archive::parse(&z).unwrap();
    let names: Vec<_> = a.entries().iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["hello.txt", "docs/long.txt"]);
    assert!(a.entries().iter().all(|e| e.method == 0));
    assert_eq!(a.read_name("hello.txt").unwrap(), src("hello.txt"));
    assert_eq!(a.read_name("docs/long.txt").unwrap(), src("docs/long.txt"));
}

#[test]
fn deflate_archive() {
    let z = fixture("deflate.zip");
    let a = Archive::parse(&z).unwrap();
    for (name, want) in [("hello.txt", "hello.txt"), ("docs/long.txt", "docs/long.txt"), ("docs/sub/n.txt", "docs/sub/n.txt"), ("random.bin", "random.bin")] {
        let e = a.find(name).unwrap();
        assert_eq!(a.read(e).unwrap(), src(want), "{name}");
    }
    assert_eq!(a.find("docs/long.txt").unwrap().method, 8);
    let (y, ..) = a.find("hello.txt").unwrap().modified();
    assert!(y >= 2026);
}

#[test]
fn directories() {
    let z = fixture("dirs.zip");
    let a = Archive::parse(&z).unwrap();
    assert!(a.entries().iter().any(|e| e.is_dir() && e.name == "docs/"));
    assert!(a.entries().iter().any(|e| e.is_dir() && e.name == "docs/sub/"));
    assert_eq!(a.read_name("docs/sub/n.txt").unwrap(), src("docs/sub/n.txt"));
}

#[test]
fn zip64_records() {
    let z = fixture("zip64.zip");
    // the ZIP64 end of central directory record must be present (§4.3.14)
    assert!(z.windows(4).any(|w| w == 0x0606_4b50u32.to_le_bytes()));
    let a = Archive::parse(&z).unwrap();
    assert_eq!(a.entries().len(), 2);
    assert_eq!(a.read_name("docs/long.txt").unwrap(), src("docs/long.txt"));
}

#[test]
fn streamed_with_data_descriptor() {
    let z = fixture("stream.zip");
    let a = Archive::parse(&z).unwrap();
    let e = &a.entries()[0];
    assert_eq!(e.name, "-");
    assert_ne!(e.flags & 8, 0, "bit 3: sizes in a data descriptor");
    assert_eq!(a.read(e).unwrap(), src("docs/long.txt"));
}

#[test]
fn utf8_names() {
    let z = fixture("utf8.zip");
    let a = Archive::parse(&z).unwrap();
    assert_eq!(a.entries()[0].name, "café-日本.txt");
    assert_eq!(a.read(&a.entries()[0]).unwrap(), src("café-日本.txt"));
}

#[test]
fn archive_comment() {
    let z = fixture("comment.zip");
    let a = Archive::parse(&z).unwrap();
    assert_eq!(a.comment, b"archive comment here");
    assert_eq!(a.read_name("hello.txt").unwrap(), b"hello, zip\n");
}

#[test]
fn writer_round_trip() {
    let mut w = Writer::new();
    w.set_time(2026, 10, 6, 9, 30, 12);
    w.add_dir("word").unwrap();
    w.add_stored("[Content_Types].xml", b"<Types/>").unwrap();
    w.add_stored("word/document.xml", "<w:document>é</w:document>".as_bytes()).unwrap();
    w.add_stored("名前.txt", b"x").unwrap();
    let z = w.finish().unwrap();
    let a = Archive::parse(&z).unwrap();
    assert_eq!(a.entries().len(), 4);
    assert_eq!(a.read_name("word/document.xml").unwrap(), "<w:document>é</w:document>".as_bytes());
    assert_eq!(a.read_name("名前.txt").unwrap(), b"x");
    assert_eq!(a.find("word/document.xml").unwrap().modified(), (2026, 10, 6, 9, 30, 12));
    assert!(a.find_ci("WORD/Document.XML").is_some());
    // Info-ZIP's own integrity test agrees (black-box), when unzip is installed.
    let p = std::env::temp_dir().join(format!("omni-spec-zip-{}.zip", std::process::id()));
    std::fs::write(&p, &z).unwrap();
    if let Ok(o) = std::process::Command::new("unzip").arg("-t").arg(&p).output() {
        let s = String::from_utf8_lossy(&o.stdout);
        assert!(o.status.success() && s.contains("No errors detected"), "{s}");
    }
    let _ = std::fs::remove_file(&p);
}

#[test]
fn empty_archive() {
    let z = Writer::new().finish().unwrap();
    assert_eq!(z.len(), 22);
    assert!(Archive::parse(&z).unwrap().entries().is_empty());
}

#[test]
fn errors() {
    assert_eq!(Archive::parse(b"not a zip at all, no records here").err(), Some(Error::NotZip));
    let z = fixture("deflate.zip");
    // flip one byte of hello.txt's compressed data (after its local header) → CRC or inflate error, never Ok
    let a = Archive::parse(&z).unwrap();
    let e = a.find("docs/long.txt").unwrap().clone();
    let raw_start = {
        let p = e.local_offset as usize;
        p + 30 + u16::from_le_bytes([z[p + 26], z[p + 27]]) as usize + u16::from_le_bytes([z[p + 28], z[p + 29]]) as usize
    };
    let mut bad = z.clone();
    bad[raw_start + 40] ^= 0x55;
    let b = Archive::parse(&bad).unwrap();
    assert!(b.read(&e).is_err());
    // size limit
    assert!(matches!(a.read_limited(&e, 10), Err(Error::TooLarge(_))));
    assert!(matches!(a.read_name("missing"), Err(Error::NotFound(_))));
    // unsafe names are reported, not rejected
    let mut w = Writer::new();
    w.add_stored("../evil.txt", b"x").unwrap();
    let z = w.finish().unwrap();
    assert!(!Archive::parse(&z).unwrap().entries()[0].is_safe_name());
}

#[test]
fn corrupted_and_random_inputs_never_panic() {
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut rnd = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for name in ["stored.zip", "deflate.zip", "zip64.zip", "stream.zip", "dirs.zip"] {
        let z = fixture(name);
        for _ in 0..500 {
            let mut m = z.clone();
            for _ in 0..1 + rnd() % 4 {
                let i = (rnd() as usize) % m.len();
                m[i] ^= 1 << (rnd() % 8);
            }
            if rnd() % 5 == 0 {
                let n = (rnd() as usize) % m.len();
                m.truncate(n);
            }
            if let Ok(a) = Archive::parse(&m) {
                for e in a.entries().to_vec() {
                    let _ = a.read_limited(&e, 1 << 20);
                }
            }
        }
    }
    for _ in 0..5000 {
        let n = (rnd() % 200) as usize;
        let mut v: Vec<u8> = (0..n).map(|_| rnd() as u8).collect();
        if n > 22 && rnd() % 2 == 0 {
            let p = n - 22;
            v[p..p + 4].copy_from_slice(&0x0605_4b50u32.to_le_bytes());
        }
        if let Ok(a) = Archive::parse(&v) {
            for e in a.entries().to_vec() {
                let _ = a.read_limited(&e, 1 << 16);
            }
        }
    }
}
