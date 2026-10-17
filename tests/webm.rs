//! WebM / EBML demux checks from the specifications' own examples (RFC 8794 Table 2, RFC 9559 Tables 36–39), plus stream behaviour
//! the specs require: any chunking gives the same events (MSE appends), unknown-sized Segment / Cluster end at the next Segment-level
//! element (RFC 8794 §6.2), BlockGroup duration + DiscardPadding (RFC 9559 §5.1.3.5). Real YouTube segments: MEDIA-PLAYER-0 P2.

use omni_spec::webm::*;

// ── a test-only writer, following RFC 8794 §4–§6 (shortest VINT for sizes, IDs written as their bytes) ──
fn size(n: u64) -> Vec<u8> {
    for len in 1..=8u32 {
        if n < (1u64 << (7 * len)) - 1 { let v = n | (1u64 << (7 * len)); return v.to_be_bytes()[8 - len as usize..].to_vec(); }
    }
    panic!("too big")
}
fn id(i: u32) -> Vec<u8> { let b = i.to_be_bytes(); let z = (i.leading_zeros() / 8) as usize; b[z..].to_vec() }
fn el(i: u32, body: &[u8]) -> Vec<u8> { let mut v = id(i); v.extend(size(body.len() as u64)); v.extend_from_slice(body); v }
fn unknown(i: u32) -> Vec<u8> { let mut v = id(i); v.push(0xFF); v }
fn u(i: u32, x: u64) -> Vec<u8> { let b = x.to_be_bytes(); let z = (x.leading_zeros() / 8).min(7) as usize; el(i, &b[z..]) }
fn f8(i: u32, x: f64) -> Vec<u8> { el(i, &x.to_bits().to_be_bytes()) }
fn cat(parts: &[Vec<u8>]) -> Vec<u8> { parts.concat() }
fn simple(track: u8, rel: i16, key: bool, data: &[u8]) -> Vec<u8> {
    let mut b = vec![0x80 | track]; b.extend(rel.to_be_bytes()); b.push(if key { 0x80 } else { 0 }); b.extend_from_slice(data); el(ID_SIMPLE_BLOCK, &b)
}

fn init() -> Vec<u8> {
    let header = el(ID_EBML, &cat(&[el(ID_DOCTYPE, b"webm")]));
    let info = el(ID_INFO, &cat(&[u(ID_TIMESTAMP_SCALE, 1_000_000), f8(ID_DURATION, 5000.0)]));
    let opus = el(ID_TRACK_ENTRY, &cat(&[u(ID_TRACK_NUMBER, 1), u(ID_TRACK_TYPE, 2), el(ID_CODEC_ID, b"A_OPUS"), el(ID_CODEC_PRIVATE, b"OpusHead\x01\x02"),
        u(ID_CODEC_DELAY, 6_500_000), u(ID_SEEK_PREROLL, 80_000_000), el(ID_AUDIO, &cat(&[f8(ID_SAMPLING_FREQUENCY, 48000.0), u(ID_CHANNELS, 2)]))]));
    let vp9 = el(ID_TRACK_ENTRY, &cat(&[u(ID_TRACK_NUMBER, 2), u(ID_TRACK_TYPE, 1), el(ID_CODEC_ID, b"V_VP9"),
        el(ID_VIDEO, &cat(&[u(ID_PIXEL_WIDTH, 1280), u(ID_PIXEL_HEIGHT, 720)]))]));
    cat(&[header, unknown(ID_SEGMENT), info, el(ID_TRACKS, &cat(&[opus, vp9]))])
}

#[test]
fn vint_rfc8794_table2() {
    // the integer 2 at lengths 1–4 (Table 2)
    for (bytes, len) in [(&[0x82u8][..], 1), (&[0x40, 0x02], 2), (&[0x20, 0x00, 0x02], 3), (&[0x10, 0x00, 0x00, 0x02], 4)] {
        assert_eq!(vint(bytes), Some((2, len, false)));
    }
    // all VINT_DATA bits one = unknown size (§6.2)
    assert_eq!(vint(&[0xFF]).map(|v| v.2), Some(true));
    assert_eq!(vint(&[0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]).map(|v| v.2), Some(true));
    // not enough bytes yet
    assert_eq!(vint(&[0x40]), None);
    assert_eq!(element_id(&[0x1A, 0x45, 0xDF, 0xA3]), Some((ID_EBML, 4)));
}

#[test]
fn lacing_rfc9559_examples() {
    // three frames 800 / 500 / 1000 octets (§10.3.2 Table 36, §10.3.3 Table 38); header: track 1, timestamp 0, flags
    let frames: Vec<Vec<u8>> = [800usize, 500, 1000].iter().enumerate().map(|(k, &n)| vec![k as u8 + 1; n]).collect();
    let body: Vec<u8> = frames.concat();
    let hdr = |lacing: u8| vec![0x81u8, 0x00, 0x00, lacing << 1];
    let xiph = cat(&[hdr(1), vec![0x02, 0xFF, 0xFF, 0xFF, 0x23, 0xFF, 0xF5], body.clone()]);
    assert_eq!(xiph.len(), 2311, "Table 36: the Block is 2311 octets");
    let ebml = cat(&[hdr(3), vec![0x02, 0x43, 0x20, 0x5E, 0xD3], body.clone()]);
    // Table 38 says 2308, but its own rows overlap (second size at octets 7–8, first frame from octet 8): 4 header + 1 count + 2 + 2
    // size octets + 2300 data = 2309. The layout follows the prose (§10.3.3); the total is the table's slip.
    assert_eq!(ebml.len(), 2309, "Table 38 layout (its printed total 2308 double-counts octet 8)");
    for b in [xiph, ebml] {
        let fs = block(&b, false, 0, 1_000_000).unwrap();
        assert_eq!(fs.iter().map(|f| f.data.len()).collect::<Vec<_>>(), vec![800, 500, 1000]);
        assert_eq!(fs.iter().map(|f| f.data.clone()).collect::<Vec<_>>(), frames);
    }
    assert_eq!(signed_vint(&[0x5E, 0xD3]), Some((-300, 2)), "Table 38: 500 - 800 = -300");
    // fixed-size (Table 39): three frames of 800, a 2405-octet Block
    let fixed = cat(&[hdr(2), vec![0x02], vec![7u8; 2400]]);
    assert_eq!(fixed.len(), 2405);
    assert_eq!(block(&fixed, false, 0, 1).unwrap().iter().map(|f| f.data.len()).collect::<Vec<_>>(), vec![800, 800, 800]);
    // a lace whose sizes run past the Block is an error, not a panic
    assert!(block(&cat(&[hdr(1), vec![0x01, 0xFF, 0xFF, 0x10], vec![0u8; 10]]), false, 0, 1).is_err());
}

fn stream() -> Vec<u8> {
    let c1 = cat(&[unknown(ID_CLUSTER), u(ID_TIMESTAMP, 1000), simple(1, 0, true, b"opus-a"), simple(2, 0, true, b"vp9-key"), simple(1, 20, true, b"opus-b"),
        simple(2, 33, false, b"vp9-inter")]);
    let group = el(ID_BLOCK_GROUP, &cat(&[el(ID_BLOCK, &cat(&[vec![0x81, 0x00, 0x14, 0x00], b"opus-last".to_vec()])), u(ID_BLOCK_DURATION, 20),
        el(ID_DISCARD_PADDING, &3_000_000i64.to_be_bytes()[5..])]));
    let c2 = el(ID_CLUSTER, &cat(&[u(ID_TIMESTAMP, 2000), simple(2, -5, false, b"vp9-b"), group]));
    // an unknown-sized Cluster ends at the next Cluster; a Cues element (skipped) follows
    cat(&[init(), c1, c2, el(ID_CUES, &[0xBB, 0x80])])
}

fn frames(ev: &[Event]) -> Vec<(u64, i64, bool, Vec<u8>)> {
    ev.iter().filter_map(|e| if let Event::Frame(f) = e { Some((f.track, f.pts_ns, f.key, f.data.clone())) } else { None }).collect()
}

#[test]
fn demux_whole_stream() {
    let ev = Demuxer::new().push(&stream());
    assert_eq!(ev[0], Event::Header("webm".into()));
    assert!(matches!(ev[1], Event::Info { timestamp_scale: 1_000_000, duration_ns: Some(d) } if d == 5e9));
    let Event::Tracks(t) = &ev[2] else { panic!("tracks: {:?}", ev[2]) };
    assert_eq!((t[0].codec.as_str(), t[0].kind, t[0].rate, t[0].channels, t[0].codec_delay_ns, t[0].seek_preroll_ns), ("A_OPUS", 2, 48000.0, 2, 6_500_000, 80_000_000));
    assert_eq!(t[0].private, b"OpusHead\x01\x02");
    assert_eq!((t[1].codec.as_str(), t[1].kind, t[1].width, t[1].height), ("V_VP9", 1, 1280, 720));
    let ms = 1_000_000i64;
    assert_eq!(frames(&ev), vec![
        (1, 1000 * ms, true, b"opus-a".to_vec()), (2, 1000 * ms, true, b"vp9-key".to_vec()), (1, 1020 * ms, true, b"opus-b".to_vec()),
        (2, 1033 * ms, false, b"vp9-inter".to_vec()), (2, 1995 * ms, false, b"vp9-b".to_vec()), (1, 2020 * ms, true, b"opus-last".to_vec())]);
    let last = ev.iter().rev().find_map(|e| if let Event::Frame(f) = e { Some(f.clone()) } else { None }).unwrap();
    assert_eq!((last.duration_ns, last.discard_padding_ns), (Some(20 * ms as u64), Some(3_000_000)));
    assert_eq!(ev.iter().filter(|e| matches!(e, Event::Cluster(_))).count(), 2);
    assert!(!ev.iter().any(|e| matches!(e, Event::Error(_))));
}

#[test]
fn any_chunking_same_events() {
    let s = stream();
    let whole = Demuxer::new().push(&s);
    for step in [1usize, 2, 3, 7, 64, 333] {
        let mut d = Demuxer::new();
        let mut ev = Vec::new();
        for c in s.chunks(step) { ev.extend(d.push(c)); }
        assert_eq!(ev, whole, "chunk size {step}");
    }
}

#[test]
fn garbage_is_an_error_not_a_panic() {
    let mut d = Demuxer::new();
    let ev = d.push(&[0x00, 0x01, 0x02, 0x03]);
    assert!(matches!(ev.last(), Some(Event::Error(_))));
    assert!(d.push(&stream()).is_empty(), "stopped until reset");
    d.reset();
    assert!(d.push(&stream()).iter().any(|e| matches!(e, Event::Frame(_))));
}
