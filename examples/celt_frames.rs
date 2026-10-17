//! celt_frames IN.bit — one TSV line per frame of every packet (all modes): out_pos (48 kHz sample where the frame's output starts in the
//! decoded stream), samples, mode, LM, stereo, end band, and for CELT frames the prose-exact header (silence, post-filter period/gain/
//! tapset, transient, intra). C1 probe input for the reference-energy recovery.
use omni_spec::opus::{parse_packet, read_vector, Mode, RangeDecoder};
use omni_spec::opus_celt::{decode_header, end_band};
fn main() {
    let bit = std::fs::read(std::env::args().nth(1).unwrap()).unwrap();
    println!("pkt\tframe\tpos\tn\tmode\tlm\tstereo\tend\tsilence\tpf_period\tpf_gain\tpf_tap\ttransient\tintra\tfs15\ttell_frac\tfs15_lm0_notr\tintra_lm0_notr\tbytes");
    let (mut pos, mut last) = (0usize, 960usize);
    for (k, p) in read_vector(&bit).unwrap().iter().enumerate() {
        if p.data.is_empty() { pos += last; continue; }
        let q = parse_packet(p.data).unwrap();
        let n = q.toc.frame_samples_48k();
        for (fi, fr) in q.frames.iter().enumerate() {
            let lm = (n / 120).trailing_zeros();
            let mode = match q.toc.mode() { Mode::Silk => "silk", Mode::Hybrid => "hybrid", Mode::Celt => "celt" };
            let mut cols = vec![k.to_string(), fi.to_string(), pos.to_string(), n.to_string(), mode.into(), lm.to_string(),
                (q.toc.stereo as u8).to_string(), end_band(q.toc.bandwidth()).to_string()];
            if q.toc.mode() == Mode::Celt && !fr.is_empty() {
                let mut rd = RangeDecoder::new(fr);
                let h = decode_header(&mut rd, lm);
                // C1 probe: where the first coarse-energy symbol falls on a 15-bit scale (ec_decode_bin(15), no update) + bits used so far
                let tf = rd.tell_frac();
                let fs = if h.silence { -1 } else { rd.decode_bin(15) as i64 };
                // alternative reading for LM = 0: the transient flag is not coded (a 2.5 ms frame has no shorter MDCTs)
                let (fs_alt, intra_alt) = {
                    let mut r2 = RangeDecoder::new(fr);
                    if r2.bit_logp(15) { (-1, 0) } else {
                        if r2.bit_logp(1) { let o = r2.uint(6); r2.bits(4 + o); r2.bits(3); r2.pdf(&[2, 1, 1]); }
                        if lm > 0 { r2.bit_logp(3); }
                        let ia = r2.bit_logp(3) as u8;
                        (r2.decode_bin(15) as i64, ia)
                    }
                };
                let (pp, pg, pt) = h.postfilter.map(|(a, b, c)| (a as i64, b as i64, c as i64)).unwrap_or((0, -1, -1));
                cols.extend([(h.silence as u8).to_string(), pp.to_string(), pg.to_string(), pt.to_string(), (h.transient as u8).to_string(), (h.intra as u8).to_string(), fs.to_string(), tf.to_string(), fs_alt.to_string(), intra_alt.to_string()]);
            } else { cols.extend(["", "", "", "", "", "", "", "", "", ""].map(String::from)); }
            cols.push(fr.len().to_string());
            println!("{}", cols.join("\t"));
            pos += n;
        }
        last = q.samples_48k();
    }
}
