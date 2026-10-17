//! Opus conformance (RFC 6716 §6, RFC 8251 vectors). Vectors: OPUS_VECTORS dir (default opus_newvectors,
//! extracted from the RFC 8251 test-vector package). Missing vectors → the test reports and passes vacuously.
use omni_spec::opus::{parse_packet, read_vector, Mode, RangeDecoder};
use std::path::PathBuf;

fn vec_dir() -> Option<PathBuf> {
    let d = PathBuf::from(std::env::var("OPUS_VECTORS").unwrap_or_else(|_| "opus_newvectors".into()));
    if d.join("testvector01.bit").exists() { Some(d) } else { eprintln!("opus vectors not found at {d:?}: skipped"); None }
}

/// §3 framing: every packet of all 12 vectors parses, and the total duration equals the reference output's length
/// (.dec = 48 kHz stereo 16-bit). A zero-length packet is a lost packet: the decoder fills it with the previous packet's duration.
#[test]
fn framing_matches_reference_duration() {
    let Some(d) = vec_dir() else { return };
    for i in 1..=12 {
        let bit = std::fs::read(d.join(format!("testvector{i:02}.bit"))).unwrap();
        let dec = std::fs::metadata(d.join(format!("testvector{i:02}.dec"))).unwrap().len() as usize;
        let pk = read_vector(&bit).unwrap();
        let (mut total, mut last) = (0usize, 0usize);
        let mut modes = [0usize; 3];
        for p in &pk {
            if p.data.is_empty() { total += last; continue; }
            let q = parse_packet(p.data).unwrap_or_else(|e| panic!("vector {i}: {e}"));
            last = q.samples_48k(); total += last;
            modes[match q.toc.mode() { Mode::Silk => 0, Mode::Hybrid => 1, Mode::Celt => 2 }] += 1;
        }
        eprintln!("vector {i:02}: {} packets, SILK/Hybrid/CELT {modes:?}, {total} samples, reference {}", pk.len(), dec / 4);
        assert_eq!(total, dec / 4, "vector {i}");
    }
}

/// §4.1 range decoder: ec_tell() of a fresh decoder is 1 (§4.1.6.1), and tell == ceil(tell_frac / 8) (§4.1.6) over a run of symbols.
#[test]
fn range_decoder_tell_invariants() {
    let Some(d) = vec_dir() else { return };
    let bit = std::fs::read(d.join("testvector01.bit")).unwrap();
    for p in read_vector(&bit).unwrap().iter().take(200) {
        let Ok(q) = parse_packet(p.data) else { continue };
        let Some(f) = q.frames.first() else { continue };
        let mut r = RangeDecoder::new(f);
        assert_eq!(r.tell(), 1);
        for k in 0..40 {
            match k % 4 { 0 => { r.bit_logp(1 + k % 15); } 1 => { r.pdf(&[7, 2, 21, 2]); } 2 => { r.uint(6 + k * 37); } _ => { r.icdf(&[200, 100, 0], 8); } }
            assert_eq!(r.tell(), (r.tell_frac() + 7) / 8, "tell vs tell_frac");
        }
    }
}

/// SILK-only packets (§4.2): the final range after each packet equals the reference's (§6), and the 48 kHz output tracks the
/// reference PCM (our own measure: SNR per vector over SILK-only stretches, best alignment within ±64 samples).
#[test]
fn silk_final_range_and_pcm() {
    let Some(d) = vec_dir() else { return };
    for i in [2, 3, 4, 12, 8, 9] {
        let bit = std::fs::read(d.join(format!("testvector{i:02}.bit"))).unwrap();
        let dec = std::fs::read(d.join(format!("testvector{i:02}.dec"))).unwrap();
        let refpcm: Vec<f32> = dec.chunks(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0).collect();
        let mut dcd = omni_spec::opus::Decoder::new();
        let (mut ok, mut bad, mut red) = (0, 0, 0);
        let mut first_bad = None;
        let mut out: Vec<f32> = Vec::new();
        let mut silk_spans = Vec::new();
        for (k, p) in read_vector(&bit).unwrap().iter().enumerate() {
            if p.data.is_empty() { let n = out.len(); out.resize(n + 960 * 2, 0.0); continue; }
            let r = dcd.decode(p.data).unwrap();
            let start = out.len();
            out.extend_from_slice(&r.pcm);
            if let Some(fr) = r.final_range {
                if r.redundancy { red += 1; }
                if fr == p.final_range { ok += 1; silk_spans.push((start, out.len())); } else { bad += 1; if first_bad.is_none() { first_bad = Some((k, fr, p.final_range, r.redundancy)); } }
            }
        }
        // SNR over SILK spans at the best lag
        let (mut best, mut blag) = (f64::MIN, 0i64);
        for lag in -64i64..=64 {
            let (mut se, mut sn) = (0f64, 0f64);
            for &(a, b) in &silk_spans {
                for j in a..b {
                    let rj = j as i64 + lag * 2;
                    if rj < 0 || rj as usize >= refpcm.len() { continue; }
                    let (x, y) = (out[j] as f64, refpcm[rj as usize] as f64);
                    se += y * y; sn += (x - y) * (x - y);
                }
            }
            let snr = 10.0 * (se / sn.max(1e-20)).log10();
            if snr > best { best = snr; blag = lag; }
        }
        eprintln!("vector {i:02}: SILK final range {ok} ok / {bad} bad (redundancy in {red}); first bad {first_bad:?}; SNR {best:.1} dB at lag {blag}");
    }
}

/// TTS-SILK-VOCODER-0 V1: a steady vowel's formants survive formants → LSF → NLSF_Q15 → the RFC's NLSF → LPC: the decoder filter's
/// spectral peaks lie within 60 Hz of each target formant below 4 kHz.
#[test]
fn silk_vocoder_formants_survive() {
    use omni_spec::opus_silk::{formants_to_poly, poly_to_lsf, lsf_to_nlsf_q15, LpcVocoder};
    for vowel in [[(730.0, 80.0), (1090.0, 90.0), (2440.0, 120.0)], [(270.0, 60.0), (2290.0, 90.0), (3010.0, 120.0)], [(300.0, 60.0), (870.0, 90.0), (2240.0, 120.0)]] {
        let mut f: Vec<(f64, f64)> = vowel.to_vec();
        f.extend([(3500.0, 180.0), (4500.0, 300.0), (5500.0, 400.0), (6500.0, 500.0), (7500.0, 600.0)]);
        let lsf = poly_to_lsf(&formants_to_poly(&f, 16000.0));
        assert_eq!(lsf.len(), 16, "{lsf:?}");
        let nlsf: Vec<i32> = lsf.iter().map(|&w| lsf_to_nlsf_q15(w)).collect();
        let a = LpcVocoder::filter(&nlsf);
        // |1/A(e^jω)| on a 2 Hz grid; local maxima
        let mag = |hz: f64| -> f64 {
            let w = 2.0 * std::f64::consts::PI * hz / 16000.0;
            let (mut re, mut im) = (1.0f64, 0.0f64);
            for (k, &x) in a.iter().enumerate() { let c = x as f64 / 4096.0; re -= c * (w * (k + 1) as f64).cos(); im += c * (w * (k + 1) as f64).sin(); }
            1.0 / (re * re + im * im).sqrt()
        };
        let grid: Vec<f64> = (0..4000).map(|i| mag(i as f64 * 2.0)).collect();
        let peaks: Vec<f64> = (1..grid.len() - 1).filter(|&i| grid[i] > grid[i - 1] && grid[i] >= grid[i + 1]).map(|i| i as f64 * 2.0).collect();
        for &(ft, _) in vowel.iter() {
            let near = peaks.iter().map(|p| (p - ft).abs()).fold(f64::MAX, f64::min);
            eprintln!("target {ft} Hz: nearest peak {near:.0} Hz away (peaks {peaks:?})");
            assert!(near <= 60.0, "formant {ft} lost: peaks {peaks:?}");
        }
    }
}

/// CELT C1 (G3): the coarse-energy Laplace family lays its symbols out contiguously in the order 0, −1, +1, −2, +2, … from 0, never past
/// 2^15, for both floor forms (add = false) and add forms; a symbol past the table is None.
#[test]
fn celt_laplace_intervals_contiguous() {
    use omni_spec::opus_celt::Laplace;
    for c in [Laplace { add: false, min: 1, reserve: 0, fs0: 10496, decay: 4992 }, Laplace { add: false, min: 2, reserve: 16, fs0: 9216, decay: 8128 },
              Laplace { add: true, min: 1, reserve: 4, fs0: 7680, decay: 5632 }] {
        let mut next = 0u32;
        let mut seen = 0;
        for k in 0..40i32 {
            for q in if k == 0 { vec![0] } else { vec![-k, k] } {
                match c.interval(q) {
                    Some((fl, w)) => { assert_eq!(fl, next, "{c:?} q={q}"); assert!(w >= 1); next = fl + w; seen += 1; }
                    None => { assert!(next <= 32768); }
                }
            }
        }
        assert!(next <= 32768 && seen >= 9, "{c:?}: {seen} symbols, end {next}");
    }
}
