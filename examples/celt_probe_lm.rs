//! celt_probe_lm IN.bit — per frame size (LM), intra rate and first-frame intra under two readings of Table 56: transient always coded
//! vs coded only when LM > 0 (a 2.5 ms frame has no shorter MDCTs to switch to). C1 plausibility probe.
use omni_spec::opus::{parse_packet, read_vector, Mode, RangeDecoder};
fn main() {
    let bit = std::fs::read(std::env::args().nth(1).unwrap()).unwrap();
    for rule in ["always", "lm>0"] {
        let mut st = [[0usize; 3]; 4]; // per LM: frames, intra, transient
        let mut first: Vec<(usize, bool)> = vec![];
        for p in read_vector(&bit).unwrap() {
            let Ok(q) = parse_packet(p.data) else { continue };
            if q.toc.mode() != Mode::Celt { continue; }
            let lm = (q.toc.frame_samples_48k() / 120).trailing_zeros() as usize;
            for fr in &q.frames {
                if fr.is_empty() { continue; }
                let mut rd = RangeDecoder::new(fr);
                if rd.bit_logp(15) { continue; }
                if rd.bit_logp(1) { let o = rd.uint(6); rd.bits(4 + o); rd.bits(3); rd.pdf(&[2, 1, 1]); }
                let tr = if rule == "always" || lm > 0 { rd.bit_logp(3) } else { false };
                let intra = rd.bit_logp(3);
                st[lm][0] += 1; st[lm][1] += intra as usize; st[lm][2] += tr as usize;
                if first.len() < 3 { first.push((lm, intra)); }
            }
        }
        println!("rule {rule}: per LM [frames, intra, transient] {st:?}; first frames (lm, intra) {first:?}");
    }
}
