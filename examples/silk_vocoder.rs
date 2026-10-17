//! silk_vocoder — SILK WB synthesis as the TTS core's LPC vocoder (TTS-SILK-VOCODER-0). stdin JSON:
//!   {"out_rate": 16000|24000|48000, "subframes": [{"formants": [[Hz, bw], …], "prev": [[Hz, bw], …] | null, "mix": 0..1,
//!     "gain": g, "exc": [80 source samples at 16 kHz]}, …]}
//! Each subframe's filter = the LSF interpolation (1−mix)·LSF(prev) + mix·LSF(formants) (interpolating LSFs keeps the filter
//! ordered and stable), through the RFC's NLSF → LPC; stdout = f32 little-endian PCM at out_rate (16 k direct; 48 k via the
//! Table-54 resampler; 24 k = every 2nd 48 k sample — the 16 k signal has no content above 8 kHz).
use omni_spec::json::{parse, Value};
use omni_spec::opus::Resampler;
use omni_spec::opus_silk::{formants_to_poly, lsf_to_nlsf_q15, poly_to_lsf, LpcVocoder};
use std::collections::HashMap;
use std::io::{Read, Write};

fn get<'a>(v: &'a Value, k: &str) -> Option<&'a Value> { if let Value::Object(m) = v { m.iter().find(|(n, _)| n == k).map(|(_, x)| x) } else { None } }
fn num(v: Option<&Value>) -> f64 { if let Some(Value::Number(x)) = v { *x } else { 0.0 } }
fn formants(v: Option<&Value>) -> Option<Vec<(f64, f64)>> {
    let Some(Value::Array(a)) = v else { return None };
    Some(a.iter().filter_map(|p| if let Value::Array(q) = p { Some((num(q.first()), num(q.get(1)))) } else { None }).collect())
}

fn main() {
    let mut s = Vec::new(); std::io::stdin().read_to_end(&mut s).unwrap();
    let j = parse(&s).expect("json");
    let out_rate = num(get(&j, "out_rate")) as u32;
    let mut cache: HashMap<String, Vec<f64>> = HashMap::new();
    let mut lsf_of = |f: &[(f64, f64)]| -> Vec<f64> {
        let key = format!("{f:?}");
        cache.entry(key).or_insert_with(|| poly_to_lsf(&formants_to_poly(f, 16000.0))).clone()
    };
    let mut voc = LpcVocoder::new();
    let mut pcm16 = Vec::new();
    let Some(Value::Array(sf)) = get(&j, "subframes") else { panic!("no subframes") };
    for x in sf {
        let cur = lsf_of(&formants(get(x, "formants")).expect("formants"));
        let lsf = match formants(get(x, "prev")) {
            Some(p) => { let pl = lsf_of(&p); let m = num(get(x, "mix")); pl.iter().zip(&cur).map(|(a, b)| (1.0 - m) * a + m * b).collect() }
            None => cur,
        };
        let a = LpcVocoder::filter(&lsf.iter().map(|&w| lsf_to_nlsf_q15(w)).collect::<Vec<_>>());
        let exc: Vec<f32> = if let Some(Value::Array(e)) = get(x, "exc") { e.iter().map(|v| num(Some(v)) as f32).collect() } else { vec![0.0; 80] };
        // loudness follows the requested gain, not the formants: divide by the filter's impulse-response RMS (256 samples)
        let mut probe = LpcVocoder::new();
        let mut imp = vec![0f32; 256]; imp[0] = 1.0;
        let h = probe.subframe(&a, 1.0e-3, &imp);
        let hr = (h.iter().map(|v| (v * 1.0e3) as f64 * (v * 1.0e3) as f64).sum::<f64>()).sqrt().max(1e-9);
        pcm16.extend(voc.subframe(&a, (num(get(x, "gain")) / hr) as f32, &exc));
    }
    let out: Vec<f32> = match out_rate {
        16000 => pcm16,
        r => { let up = Resampler::new(16000).process(&pcm16); if r == 24000 { up.iter().step_by(2).copied().collect() } else { up } }
    };
    let mut o = std::io::stdout().lock();
    for v in out { o.write_all(&v.to_le_bytes()).unwrap(); }
}
