//! webm_dump FILE... — push each file (one appended MSE chunk each, in order) through the streaming demuxer and print
//! a summary: header, tracks, clusters, frame counts / key frames / pts range per track, errors. MEDIA-PLAYER-0 P2.
use omni_spec::webm::{Demuxer, Event};
use std::collections::BTreeMap;

fn main() {
    let mut d = Demuxer::new();
    let (mut clusters, mut errs) = (0, Vec::new());
    let mut per: BTreeMap<u64, (usize, usize, i64, i64, usize)> = BTreeMap::new(); // frames, keys, pts min, pts max, bytes
    let mut first: BTreeMap<(u64, u8), usize> = BTreeMap::new(); // (track, first byte >> 3): Opus TOC config (RFC 6716 §3.1) / codec header bits
    for f in std::env::args().skip(1) {
        let b = std::fs::read(&f).expect("read");
        for e in d.push(&b) {
            match e {
                Event::Header(t) => println!("header {t}"),
                Event::Info { timestamp_scale, duration_ns } => println!("info scale {timestamp_scale} duration_ns {duration_ns:?}"),
                Event::Tracks(ts) => for t in ts { println!("track {} {} {}x{} rate {} ch {} encoded {} private {} B", t.number, t.codec, t.width, t.height, t.rate, t.channels, t.encoded, t.private.len()) },
                Event::Cluster(_) => clusters += 1,
                Event::Frame(fr) => { let e = per.entry(fr.track).or_insert((0, 0, i64::MAX, i64::MIN, 0)); e.0 += 1; e.1 += fr.key as usize; e.2 = e.2.min(fr.pts_ns); e.3 = e.3.max(fr.pts_ns); e.4 += fr.data.len(); if let Some(&b) = fr.data.first() { *first.entry((fr.track, b >> 3)).or_default() += 1; } }
                Event::Error(m) => errs.push(m),
            }
        }
    }
    println!("clusters {clusters}");
    for (t, (n, k, a, b, by)) in per { println!("track {t}: frames {n} keys {k} pts {:.3}..{:.3} s bytes {by}", a as f64 / 1e9, b as f64 / 1e9); }
    println!("first byte >> 3 per track: {first:?}");
    println!("errors {}: {:?}", errs.len(), errs);
}
