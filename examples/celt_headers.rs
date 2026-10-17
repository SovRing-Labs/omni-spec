//! celt_headers IN.bit… — decode the prose-exact CELT frame header (RFC 6716 Table 56: silence, post-filter, transient, intra) of every
//! CELT-only frame and print statistics (MEDIA-PLAYER-0 C1 plausibility; the exact check needs the full frame → final range).
use omni_spec::opus::{parse_packet, read_vector, Mode, RangeDecoder};
use omni_spec::opus_celt::decode_header;
fn main() {
    for f in std::env::args().skip(1) {
        let bit = std::fs::read(&f).unwrap();
        let (mut n, mut sil, mut pf, mut tr, mut intra) = (0, 0, 0, 0, 0);
        let mut first_intra = Vec::new(); let mut periods = (u32::MAX, 0u32); let mut taps = [0; 3]; let mut lm_hist = [0; 4];
        let mut tell_hdr = 0i64;
        for p in read_vector(&bit).unwrap() {
            let Ok(q) = parse_packet(p.data) else { continue };
            if q.toc.mode() != Mode::Celt { continue; }
            for fr in &q.frames {
                if fr.is_empty() { continue; }
                let lm = (q.toc.frame_samples_48k() / 120).trailing_zeros() as usize; lm_hist[lm] += 1;
                let mut rd = RangeDecoder::new(fr);
                let h = decode_header(&mut rd, lm as u32);
                n += 1; tell_hdr += rd.tell() as i64;
                if h.silence { sil += 1; continue; }
                if let Some((per, _g, t)) = h.postfilter { pf += 1; periods.0 = periods.0.min(per); periods.1 = periods.1.max(per); taps[t] += 1; }
                if h.transient { tr += 1; } if h.intra { intra += 1; if first_intra.len() < 5 { first_intra.push(n - 1); } }
            }
        }
        println!("{f}: {n} CELT frames (LM hist {lm_hist:?}); silence {sil}; postfilter {pf} (period {}..{}, tapsets {taps:?}); transient {tr}; intra {intra} (first at frames {first_intra:?}); mean header bits {:.2}",
            periods.0, periods.1, tell_hdr as f64 / n.max(1) as f64);
    }
}
