//! celt_chain IN.bit --lm<L>=<floor|add>,<min>,<reserve>,<fs0>,<decay> … — G3 probe: for every non-silent INTER CELT frame whose LM has a
//! band-0 coder given, decode the header, then band 0 of channel 0 with that coder (exact range update), and report where the NEXT coarse
//! symbol falls (fs15): band 0 of channel 1 on stereo frames (same coder, decoded too, then the following symbol), band 1 of channel 0 on
//! mono frames. TSV: pkt frame pos lm stereo q00 fs1 q01 fs2 tell_frac — q01 / fs2 only on stereo frames; "x" = fs past the modelled table.
use omni_spec::opus::{parse_packet, read_vector, Mode, RangeDecoder};
use omni_spec::opus_celt::{decode_header, Laplace};
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let bit = std::fs::read(&args[1]).unwrap();
    let mut coder: [Option<Laplace>; 4] = [None; 4];
    let mut icoder: [Option<Laplace>; 4] = [None; 4];   // --intra<L>=… : intra-frame band-0 coder (STREAM-CORE v1, god_band0_intra)
    for a in &args[2..] {
        let intra = a.starts_with("--intra");
        let (k, v) = a.trim_start_matches("--lm").trim_start_matches("--intra").split_once('=').unwrap();
        let p: Vec<&str> = v.split(',').collect();
        let n = |i: usize| p[i].parse::<u32>().unwrap();
        let slot = if intra { &mut icoder } else { &mut coder };
        slot[k.parse::<usize>().unwrap()] = Some(Laplace { add: p[0] == "add", min: n(1), reserve: n(2), fs0: n(3), decay: n(4) });
    }
    println!("pkt\tframe\tpos\tlm\tstereo\tq00\tfs1\tq01\tfs2\ttell_frac");
    let (mut pos, mut last) = (0usize, 960usize);
    for (k, p) in read_vector(&bit).unwrap().iter().enumerate() {
        if p.data.is_empty() { pos += last; continue; }
        let q = parse_packet(p.data).unwrap();
        let n = q.toc.frame_samples_48k();
        let lm = (n / 120).trailing_zeros();
        for (fi, fr) in q.frames.iter().enumerate() {
            let here = pos;
            pos += n;
            if q.toc.mode() != Mode::Celt || fr.is_empty() { continue; }
            let mut rd = RangeDecoder::new(fr);
            let h = decode_header(&mut rd, lm);
            if h.silence { continue; }
            let Some(c) = (if h.intra { icoder } else { coder })[lm as usize] else { continue };
            let sym = |x: Option<i32>| x.map(|v| v.to_string()).unwrap_or("x".into());
            let q00 = c.decode(&mut rd);
            let mut cols = vec![k.to_string(), fi.to_string(), here.to_string(), lm.to_string(), (q.toc.stereo as u8).to_string(), sym(q00)];
            if q00.is_some() {
                let mut r2 = rd.clone();
                cols.push(r2.decode_bin(15).to_string());
                if q.toc.stereo {
                    let q01 = c.decode(&mut rd);
                    cols.push(sym(q01));
                    cols.push(if q01.is_some() { rd.clone().decode_bin(15).to_string() } else { "x".into() });
                } else { cols.extend(["", ""].map(String::from)); }
                cols.push(rd.tell_frac().to_string());
            }
            println!("{}", cols.join("\t"));
        }
        last = q.samples_48k();
    }
}
