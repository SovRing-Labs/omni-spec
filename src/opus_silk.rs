//! Opus SILK layer (RFC 6716 §4.2) — from the RFC prose and its tables (`opus_silk_tables.rs`, generated from the RFC text).
//! Parsing (§4.2.3–§4.2.7.8) is bit-exact by construction (checked by the vectors' final range). NLSF → LPC (§4.2.7.5) follows the
//! RFC's fixed-point procedure exactly; reconstruction (§4.2.7.9, "does not need to be bit-exact") is the RFC's floating-point form.
use crate::opus::{ilog, Bandwidth, RangeDecoder};
use crate::opus_silk_tables as t;

/// samples per 5 ms subframe at the internal rate (§4.2.7.9: 40 NB, 60 MB, 80 WB)
fn subframe_len(bw: Bandwidth) -> usize { match bw { Bandwidth::Nb => 40, Bandwidth::Mb => 60, _ => 80 } }
/// internal SILK sample rate (Hz)
pub fn internal_rate(bw: Bandwidth) -> u32 { match bw { Bandwidth::Nb => 8000, Bandwidth::Mb => 12000, _ => 16000 } }
fn is_wb(bw: Bandwidth) -> bool { !matches!(bw, Bandwidth::Nb | Bandwidth::Mb) }
fn sign(x: i32) -> i32 { (x > 0) as i32 - (x < 0) as i32 }

/// Everything one SILK frame's bitstream carries (§4.2.7, Table 5).
#[derive(Clone, Debug, Default)]
pub struct FrameParams {
    /// stereo prediction weights (mid frames of stereo packets)
    pub stereo_w_q13: Option<[i32; 2]>,
    /// mid-only flag (side channel not coded for this interval)
    pub mid_only: bool,
    /// 0 inactive, 1 unvoiced, 2 voiced (Table 10)
    pub signal: usize,
    /// quantization offset type: 0 low, 1 high
    pub qoff: usize,
    pub log_gains: [i32; 4],
    pub lsf_i1: usize,
    pub lsf_i2: [i32; 16],
    /// Q2 interpolation weight (4 = none); 10 ms frames: 4
    pub interp_w: i32,
    pub lag: i32,
    pub pitch_lags: [i32; 4],
    pub periodicity: usize,
    pub ltp_filter: [usize; 4],
    pub ltp_scale_q14: i32,
    pub seed: u32,
    /// e_raw (pulses + LSBs, signed), shell blocks × 16
    pub e_raw: Vec<i32>,
    pub nsub: usize,
}

/// Per-Opus-frame parse context of one channel (decides independent gain / absolute lag coding, LTP scaling presence)
#[derive(Clone, Copy, Default)]
struct ParseCtx { prev_coded: bool, prev_voiced: bool, prev_lag: i32, prev_log_gain: Option<i32>, first: bool }

/// Table 44
fn shell_blocks(bw: Bandwidth, nsub: usize) -> usize {
    match (bw, nsub) { (Bandwidth::Nb, 2) => 5, (Bandwidth::Mb, 2) => 8, (Bandwidth::Nb, _) => 10, (Bandwidth::Mb, _) => 15, (_, 2) => 10, _ => 20 }
}

/// §4.2.7.8.3: pulses of one partition of size n (16, 8, 4, 2) holding k pulses, preorder (left half, then right half)
fn shell(rd: &mut RangeDecoder, out: &mut [i32], k: i32) {
    let n = out.len();
    if k == 0 { return; }
    if n == 1 { out[0] = k; return; }
    let tab: &[&[u16]] = match n { 16 => t::SPLIT_16, 8 => t::SPLIT_8, 4 => t::SPLIT_4, _ => t::SPLIT_2 };
    let left = rd.pdf(tab[k as usize - 1]) as i32;
    let (a, b) = out.split_at_mut(n / 2);
    shell(rd, a, left);
    shell(rd, b, k - left);
}

/// §4.2.7.1–§4.2.7.8: one SILK frame. `vad` = active (LBRR frames are always active). `side_coded` (mid frames of stereo packets
/// only): whether the side channel's frame for this interval is known to be coded (VAD / LBRR flag) — else the mid-only flag follows.
#[allow(clippy::too_many_arguments)]
fn parse_frame(rd: &mut RangeDecoder, bw: Bandwidth, nsub: usize, vad: bool, lbrr: bool, stereo_mid: Option<bool>, ctx: &mut ParseCtx, ltp_scale_present: bool) -> FrameParams {
    let mut f = FrameParams { nsub, interp_w: 4, ltp_scale_q14: 15565, ..Default::default() };
    // §4.2.7.1 stereo prediction weights (Table 6/7)
    if let Some(side_coded) = stereo_mid {
        let n = rd.pdf(t::STEREO_W[0]) as i32;
        let i0 = rd.pdf(t::STEREO_W[1]) as i32; let i1 = rd.pdf(t::STEREO_W[2]) as i32;
        let i2 = rd.pdf(t::STEREO_W[1]) as i32; let i3 = rd.pdf(t::STEREO_W[2]) as i32;
        let (wi0, wi1) = ((i0 + 3 * (n / 5)) as usize, (i2 + 3 * (n % 5)) as usize);
        let w = &t::STEREO_WEIGHTS_Q13;
        let w1 = w[wi1] + (((w[wi1 + 1] - w[wi1]) * 6554) >> 16) * (2 * i3 + 1);
        let w0 = w[wi0] + (((w[wi0 + 1] - w[wi0]) * 6554) >> 16) * (2 * i1 + 1) - w1;
        f.stereo_w_q13 = Some([w0, w1]);
        // §4.2.7.2 mid-only flag (Table 8)
        if !side_coded { f.mid_only = rd.pdf(t::MID_ONLY) == 1; }
    }
    // §4.2.7.3 frame type (Tables 9, 10)
    let ft = rd.pdf(t::FRAME_TYPE[(vad || lbrr) as usize]);
    f.signal = ft / 2; f.qoff = ft & 1;
    // §4.2.7.4 subframe gains (Tables 11–13)
    let independent = ctx.first || !ctx.prev_coded;
    let mut prev = ctx.prev_log_gain;
    for s in 0..nsub {
        let g = if s == 0 && independent {
            let msb = rd.pdf(t::GAIN_MSB[f.signal]) as i32; let lsb = rd.pdf(t::GAIN_LSB) as i32;
            let gi = (msb << 3) | lsb;
            match prev { Some(p) => gi.max(p - 16), None => gi }
        } else {
            let d = rd.pdf(t::GAIN_DELTA) as i32;
            let p = prev.unwrap_or(0);
            (2 * d - 16).max(p + d - 4).clamp(0, 63)
        };
        f.log_gains[s] = g; prev = Some(g);
    }
    ctx.prev_log_gain = prev;
    // §4.2.7.5.1–2 normalized LSF indices (Tables 14–19)
    let wb = is_wb(bw);
    f.lsf_i1 = rd.pdf(t::LSF_S1[(wb as usize) * 2 + (f.signal == 2) as usize]);
    let d = if wb { 16 } else { 10 };
    for k in 0..d {
        let sel = if wb { t::LSF_S2_SEL_WB[f.lsf_i1][k] } else { t::LSF_S2_SEL_NB[f.lsf_i1][k] } as usize;
        let mut v = rd.pdf(if wb { t::LSF_S2_WB[sel] } else { t::LSF_S2_NB[sel] }) as i32 - 4;
        if v == -4 || v == 4 { v += sign(v) * rd.pdf(t::LSF_EXT) as i32; }
        f.lsf_i2[k] = v;
    }
    // §4.2.7.5.5 interpolation weight (20 ms frames only, Table 26)
    if nsub == 4 { f.interp_w = rd.pdf(t::LSF_INTERP) as i32; }
    // §4.2.7.6 LTP parameters (voiced only)
    if f.signal == 2 {
        let (scale, lmin, lmax) = match bw { Bandwidth::Nb => (4, 16, 144), Bandwidth::Mb => (6, 24, 216), _ => (8, 32, 288) };
        let absolute = ctx.first || !ctx.prev_coded || !ctx.prev_voiced;
        let mut lag = None;
        if !absolute {
            let dl = rd.pdf(t::LAG_DELTA) as i32;
            if dl != 0 { lag = Some(ctx.prev_lag + dl - 9); }
        }
        let lag = lag.unwrap_or_else(|| {
            let hi = rd.pdf(t::LAG_HIGH) as i32;
            let lo = rd.pdf(t::LAG_LOW[match bw { Bandwidth::Nb => 0, Bandwidth::Mb => 1, _ => 2 }]) as i32;
            hi * scale + lo + lmin
        });
        f.lag = lag; ctx.prev_lag = lag;
        // Table 32–36 pitch contour
        let ci = match (bw, nsub) { (Bandwidth::Nb, 2) => 0, (Bandwidth::Nb, _) => 1, (_, 2) => 2, _ => 3 };
        let c = rd.pdf(t::CONTOUR[ci]);
        for s in 0..nsub {
            let off = match ci { 0 => t::CONTOUR_NB_10[c][s], 1 => t::CONTOUR_NB_20[c][s], 2 => t::CONTOUR_WB_10[c][s], _ => t::CONTOUR_WB_20[c][s] };
            f.pitch_lags[s] = (lag + off).clamp(lmin, lmax);
        }
        // §4.2.7.6.2 periodicity + per-subframe filters (Tables 37, 38)
        f.periodicity = rd.pdf(t::PERIODICITY);
        for s in 0..nsub { f.ltp_filter[s] = rd.pdf(t::LTP_FILTER[f.periodicity]); }
        // §4.2.7.6.3 LTP scaling (Table 42)
        if ltp_scale_present { f.ltp_scale_q14 = [15565, 12288, 8192][rd.pdf(t::LTP_SCALE)]; }
    }
    // §4.2.7.7 LCG seed (Table 43)
    f.seed = rd.pdf(t::LCG_SEED) as u32;
    // §4.2.7.8 excitation
    let nb = shell_blocks(bw, nsub);
    let rate = rd.pdf(t::RATE_LEVEL[(f.signal == 2) as usize]);
    let mut counts = vec![0i32; nb]; let mut lsbs = vec![0u32; nb];
    for b in 0..nb {
        let mut c = rd.pdf(t::PULSE_COUNT[rate]);
        while c == 17 { lsbs[b] += 1; c = rd.pdf(t::PULSE_COUNT[if lsbs[b] == 10 { 10 } else { 9 }]); }
        counts[b] = c as i32;
    }
    let mut e = vec![0i32; nb * 16];
    for b in 0..nb { shell(rd, &mut e[b * 16..b * 16 + 16], counts[b]); }
    for b in 0..nb {
        if lsbs[b] == 0 { continue; }
        for x in &mut e[b * 16..b * 16 + 16] { for _ in 0..lsbs[b] { *x = *x * 2 + rd.pdf(t::LSB) as i32; } }
    }
    for b in 0..nb {
        let pdf = t::SIGN[f.signal * 14 + f.qoff * 7 + (counts[b] as usize).min(6)];
        for x in &mut e[b * 16..b * 16 + 16] { if *x != 0 && rd.pdf(pdf) == 0 { *x = -*x; } }
    }
    f.e_raw = e;
    ctx.prev_coded = true; ctx.prev_voiced = f.signal == 2; ctx.first = false;
    f
}

// ───────────────────────── NLSF → LPC (§4.2.7.5.2–§4.2.7.5.8), fixed point exactly as the RFC states ─────────────────────────

/// §4.2.7.5.2–5.4: stage-1 + stage-2 indices → stabilized NLSF_Q15
pub fn nlsf_decode(wb: bool, i1: usize, i2: &[i32]) -> Vec<i32> {
    let d = if wb { 16 } else { 10 };
    let cb: Vec<i32> = if wb { t::LSF_CB1_WB[i1].to_vec() } else { t::LSF_CB1_NB[i1].to_vec() };
    let qstep = if wb { 9830 } else { 11796 };
    let mut res = vec![0i32; d];
    for k in (0..d).rev() {
        let pred = if k + 1 < d {
            let sel = if wb { t::LSF_PRED_SEL_WB[i1][k] } else { t::LSF_PRED_SEL_NB[i1][k] } as usize;
            let w = if wb { t::LSF_PRED_WB[sel][k] } else { t::LSF_PRED_NB[sel][k] };
            (res[k + 1] * w) >> 8
        } else { 0 };
        res[k] = pred + ((((i2[k] << 10) - sign(i2[k]) * 102) * qstep) >> 16);
    }
    // §4.2.7.5.3 IHMW weights from the stage-1 vector
    let mut nlsf = vec![0i32; d];
    for k in 0..d {
        let lo = if k == 0 { 0 } else { cb[k - 1] }; let hi = if k + 1 == d { 256 } else { cb[k + 1] };
        let w2 = ((1024 / (cb[k] - lo) + 1024 / (hi - cb[k])) as u32) << 16;
        let i = ilog(w2) as i32;
        let fr = ((w2 >> (i - 8)) & 127) as i32;
        let y = (if i & 1 != 0 { 32768 } else { 46214 }) >> ((32 - i) >> 1);
        let wq9 = y + ((213 * fr * y) >> 16);
        nlsf[k] = ((cb[k] << 7) + (res[k] << 14) / wq9).clamp(0, 32767);
    }
    stabilize(&mut nlsf, if wb { &t::LSF_MIN_SPACING_WB[..] } else { &t::LSF_MIN_SPACING_NB[..] });
    nlsf
}

/// §4.2.7.5.4 (Table 25)
fn stabilize(n: &mut [i32], dmin: &[i32]) {
    let d = n.len();
    for _ in 0..20 {
        // find i minimising NLSF[i] - NLSF[i-1] - dmin[i] over 0..=d (NLSF[-1] = 0, NLSF[d] = 32768), lowest i on ties
        let (mut bi, mut bv) = (0usize, i32::MAX);
        for i in 0..=d {
            let prev = if i == 0 { 0 } else { n[i - 1] }; let cur = if i == d { 32768 } else { n[i] };
            let v = cur - prev - dmin[i];
            if v < bv { bv = v; bi = i; }
        }
        if bv >= 0 { return; }
        if bi == 0 { n[0] = dmin[0]; }
        else if bi == d { n[d - 1] = 32768 - dmin[d]; }
        else {
            let min_c = (dmin[bi] >> 1) + dmin[..bi].iter().sum::<i32>();
            let max_c = 32768 - (dmin[bi] >> 1) - dmin[bi + 1..=d].iter().sum::<i32>();
            let c = ((n[bi - 1] + n[bi] + 1) >> 1).clamp(min_c, max_c);
            n[bi - 1] = c - (dmin[bi] >> 1);
            n[bi] = n[bi - 1] + dmin[bi];
        }
    }
    // fallback
    n.sort_unstable();
    for k in 0..d { let prev = if k == 0 { 0 } else { n[k - 1] }; n[k] = n[k].max(prev + dmin[k]); }
    for k in (0..d).rev() { let next = if k + 1 == d { 32768 } else { n[k + 1] }; n[k] = n[k].min(next - dmin[k + 1]); }
}

/// §4.2.7.5.7 silk_bwexpander_32 recurrence
fn bwexpand(a: &mut [i64], sc0: i64) {
    let mut sc = sc0;
    for x in a.iter_mut() { *x = (*x * sc) >> 16; sc = ((sc0 as u64 * sc as u64 + 32768) >> 16) as i64; }
}

/// §4.2.7.5.8 stability test of the Q12 filter (DC response + fixed-point Levinson on Q24)
fn stable(a12: &[i64]) -> bool {
    let d = a12.len();
    if a12.iter().sum::<i64>() > 4096 { return false; }
    let mut a: Vec<i64> = a12.iter().map(|&x| x << 12).collect();
    let mut inv_gain: i64 = 1 << 30;
    for k in (0..d).rev() {
        if a[k].abs() > 16773022 { return false; }
        let rc = -a[k] << 7;
        let div = (1i64 << 30) - ((rc * rc) >> 32);
        inv_gain = ((inv_gain * div) >> 32) << 2;
        if inv_gain < 107374 { return false; }
        if k > 0 {
            let b1 = ilog(div as u32) as i64; let b2 = b1 - 16;
            let inv = ((1i64 << 29) - 1) / (div >> (b2 + 1));
            let err = (1i64 << 29) - (((div << (15 - b2)) * inv) >> 16);
            let gain = (inv << 16) + ((err * inv) >> 13);
            let prev = a.clone();
            for n in 0..k {
                let num = prev[n] - ((prev[k - n - 1] * rc + (1i64 << 30)) >> 31);
                a[n] = (num * gain + (1i64 << (b1 - 1))) >> b1;
            }
        }
    }
    true
}

/// §4.2.7.5.6–5.8: NLSF_Q15 → final a_Q12
pub fn nlsf_to_lpc(wb: bool, nlsf: &[i32]) -> Vec<i32> {
    let d = nlsf.len(); let d2 = d / 2;
    let order: &[usize] = if wb { &t::LSF_ORDER_WB } else { &t::LSF_ORDER_NB };
    let mut c = vec![0i64; d];
    for k in 0..d {
        let (i, f) = ((nlsf[k] >> 8) as usize, (nlsf[k] & 255) as i64);
        let (c0, c1) = (t::COS_Q12[i] as i64, t::COS_Q12[i + 1] as i64);
        c[order[k]] = (c0 * 256 + (c1 - c0) * f + 4) >> 3;
    }
    // P and Q recurrences (Q16); row k has entries 0..=k+1, symmetric extension p[k][k+2] = p[k][k]
    let poly = |off: usize| -> Vec<i64> {
        let mut p = vec![1i64 << 16, -c[off]];
        for k in 1..d2 {
            let get = |p: &Vec<i64>, j: i64| -> i64 { if j < 0 { 0 } else if (j as usize) < p.len() { p[j as usize] } else { p[(2 * (p.len() as i64 - 2) - j + 2) as usize] } };
            let mut nx = vec![0i64; k + 2];
            for j in 0..=(k + 1) as i64 {
                nx[j as usize] = get(&p, j) + get(&p, j - 2) - ((c[2 * k + off] * get(&p, j - 1) + 32768) >> 16);
            }
            p = nx;
        }
        p
    };
    let (p, q) = (poly(0), poly(1));
    let mut a = vec![0i64; d];
    for k in 0..d2 {
        a[k] = -(q[k + 1] - q[k]) - (p[k + 1] + p[k]);
        a[d - k - 1] = (q[k + 1] - q[k]) - (p[k + 1] + p[k]);
    }
    // §4.2.7.5.7 range limiting (≤ 10 rounds)
    let mut rounds = 0;
    while rounds < 10 {
        let (mut k, mut m) = (0usize, 0i64);
        for (i, &x) in a.iter().enumerate() { if x.abs() > m { m = x.abs(); k = i; } }
        let maxabs = ((m + 16) >> 5).min(163838);
        if maxabs <= 32767 { break; }
        let sc0 = 65470 - ((maxabs - 32767) << 14) / ((maxabs * (k as i64 + 1)) >> 2);
        bwexpand(&mut a, sc0);
        rounds += 1;
    }
    if rounds == 10 { for x in a.iter_mut() { *x = (((*x + 16) >> 5).clamp(-32768, 32767)) << 5; } }
    // §4.2.7.5.8 prediction gain limiting (≤ 16 rounds)
    for i in 0..16 {
        let a12: Vec<i64> = a.iter().map(|&x| (x + 16) >> 5).collect();
        if stable(&a12) { return a12.iter().map(|&x| x as i32).collect(); }
        bwexpand(&mut a, 65536 - (2i64 << i));
    }
    a.iter().map(|&x| ((x + 16) >> 5) as i32).collect()
}

/// §4.2.7.4 silk_log2lin + gain dequantization: log_gain (0..63) → gain_Q16
pub fn gain_q16(log_gain: i32) -> i32 {
    let x = ((0x1D1C71i64 * log_gain as i64) >> 16) as i32 + 2090;
    let (i, f) = (x >> 7, x & 127);
    (1 << i) + (((-174 * f * (128 - f)) >> 16) + f) * ((1 << i) >> 7)
}

// ───────────────────────── reconstruction (§4.2.7.8.6, §4.2.7.9), per channel ─────────────────────────

const HIST: usize = 320;

#[derive(Clone)]
struct Channel {
    /// first frame after a reset / uncoded side frame: no LSF interpolation, zero histories
    fresh: bool,
    prev_nlsf: Vec<i32>,
    prev_log_gain: Option<i32>,
    /// last HIST clamped outputs (out[]) and unclamped LPC outputs (lpc[])
    out: Vec<f32>,
    lpc: Vec<f32>,
}

impl Channel {
    fn new() -> Self { Channel { fresh: true, prev_nlsf: vec![], prev_log_gain: None, out: vec![0.0; HIST], lpc: vec![0.0; HIST] } }
    fn clear(&mut self) { self.fresh = true; self.out.iter_mut().for_each(|x| *x = 0.0); self.lpc.iter_mut().for_each(|x| *x = 0.0); }

    /// one coded frame → nsub·n samples (nominal range ±1)
    fn synth(&mut self, bw: Bandwidth, f: &FrameParams) -> Vec<f32> {
        let wb = is_wb(bw); let d = if wb { 16 } else { 10 };
        let n = subframe_len(bw); let len = f.nsub * n;
        // LPC filters: n2 from this frame; n1 interpolated for the first half of a 20 ms frame when w < 4
        let n2 = nlsf_decode(wb, f.lsf_i1, &f.lsf_i2[..d]);
        let w = if self.fresh || self.prev_nlsf.len() != d { 4 } else { f.interp_w };
        let a2 = nlsf_to_lpc(wb, &n2);
        let a1 = if f.nsub == 4 && w < 4 {
            let n1: Vec<i32> = (0..d).map(|k| self.prev_nlsf[k] + ((w * (n2[k] - self.prev_nlsf[k])) >> 2)).collect();
            Some(nlsf_to_lpc(wb, &n1))
        } else { None };
        // excitation (§4.2.7.8.6)
        let off = [[25, 60], [25, 60], [8, 25]][f.signal][f.qoff];
        let mut seed = f.seed;
        let e: Vec<f32> = f.e_raw[..len].iter().map(|&r| {
            let mut v = (r << 8) - sign(r) * 20 + off;
            seed = seed.wrapping_mul(196314165).wrapping_add(907633515);
            if seed & 0x8000_0000 != 0 { v = -v; }
            seed = seed.wrapping_add(r as u32);
            v as f32 / 8388608.0
        }).collect();
        // working buffers: history (HIST) then this frame
        let mut out = self.out.clone(); out.resize(HIST + len, 0.0);
        let mut lpc = self.lpc.clone(); lpc.resize(HIST + len, 0.0);
        let mut res = vec![0f32; HIST + len];
        let gains: Vec<f32> = (0..f.nsub).map(|s| gain_q16(f.log_gains[s]) as f32).collect();
        for s in 0..f.nsub {
            let a: Vec<f32> = match (&a1, s < 2) { (Some(a1), true) => a1, _ => &a2 }.iter().map(|&x| x as f32 / 4096.0).collect();
            let j = HIST + s * n;
            let g = gains[s];
            if f.signal == 2 {
                let lag = f.pitch_lags[s] as usize;
                let (out_end, scale) = if f.nsub == 4 && s >= 2 && a1.is_some() { (j - (s - 2) * n, 16384.0) } else { (j - s * n, f.ltp_scale_q14 as f32) };
                for i in (j - lag - 2)..out_end {
                    let mut acc = out[i];
                    for k in 0..d { acc -= out[i - k - 1] * a[k]; }
                    res[i] = 4.0 * scale / g * acc.clamp(-1.0, 1.0);
                }
                for i in out_end..j {
                    let mut acc = lpc[i];
                    for k in 0..d { acc -= lpc[i - k - 1] * a[k]; }
                    res[i] = 65536.0 / g * acc;
                }
                let b: [i32; 5] = match f.periodicity { 0 => t::LTP_TAPS_0[f.ltp_filter[s]], 1 => t::LTP_TAPS_1[f.ltp_filter[s]], _ => t::LTP_TAPS_2[f.ltp_filter[s]] };
                for i in j..j + n {
                    let mut v = e[i - HIST];
                    for k in 0..5 { v += res[i + 2 - lag - k] * b[k] as f32 / 128.0; }
                    res[i] = v;
                }
            } else {
                for i in j..j + n { res[i] = e[i - HIST]; }
            }
            for i in j..j + n {
                let mut v = g / 65536.0 * res[i];
                for k in 0..d { v += lpc[i - k - 1] * a[k]; }
                lpc[i] = v;
                out[i] = v.clamp(-1.0, 1.0);
            }
        }
        self.out = out[len..].to_vec();
        self.lpc = lpc[len..].to_vec();
        self.prev_nlsf = n2;
        self.fresh = false;
        out[HIST..].to_vec()
    }
}

/// The SILK layer of one Opus decoder: two channels (mid, side), stereo unmixing state.
pub struct Silk {
    ch: [Channel; 2],
    prev_w_q13: [i32; 2],
    /// mid[i-2], mid[i-1] and side[i-1] carried across frames (§4.2.8); mono uses mid[i-1] (the same one-sample delay)
    mid_hist: [f32; 2],
    side_hist: f32,
    prev_stereo: bool,
    pub bw: Option<Bandwidth>,
}

impl Default for Silk { fn default() -> Self { Self::new() } }

impl Silk {
    pub fn new() -> Self { Silk { ch: [Channel::new(), Channel::new()], prev_w_q13: [0, 0], mid_hist: [0.0; 2], side_hist: 0.0, prev_stereo: false, bw: None } }
    /// §4.5.2 decoder reset
    pub fn reset(&mut self) { *self = Silk::new(); }

    /// Decode the SILK layer of one Opus frame (§4.2.2–§4.2.8). Returns the output channels at the internal rate
    /// (1 for a mono packet, 2 = left/right for a stereo packet).
    pub fn decode(&mut self, rd: &mut RangeDecoder, bw: Bandwidth, frame_ms: u32, stereo: bool) -> Vec<Vec<f32>> {
        if self.bw != Some(bw) && self.bw.is_some() { let keep = self.prev_stereo; self.reset(); self.prev_stereo = keep; }
        self.bw = Some(bw);
        if stereo && !self.prev_stereo { self.prev_w_q13 = [0, 0]; self.ch[1].clear(); }
        let nfr = if frame_ms <= 20 { 1 } else { (frame_ms / 20) as usize };
        let nsub = if frame_ms == 10 { 2 } else { 4 };
        let nch = if stereo { 2 } else { 1 };
        // §4.2.3 header bits: per channel, VAD flags (one per frame) then the LBRR flag
        let mut vad = [[false; 3]; 2]; let mut lbrr_any = [false; 2];
        for c in 0..nch {
            for v in vad[c].iter_mut().take(nfr) { *v = rd.bit_logp(1); }
            lbrr_any[c] = rd.bit_logp(1);
        }
        // §4.2.4 per-frame LBRR flags
        let mut lbrr = [[false; 3]; 2];
        for c in 0..nch {
            if !lbrr_any[c] { continue; }
            if nfr == 1 { lbrr[c][0] = true; } else {
                let v = rd.pdf(t::LBRR_FLAGS[nfr - 2]);
                for (i, l) in lbrr[c].iter_mut().enumerate().take(nfr) { *l = v >> i & 1 != 0; }
            }
        }
        // §4.2.5 LBRR frames: parsed (they redo the previous packet's audio; used only on loss)
        let mut lctx = [ParseCtx { first: true, ..Default::default() }; 2];
        for i in 0..nfr {
            for c in 0..nch {
                if !lbrr[c][i] {
                    lctx[c].prev_coded = false;
                    continue;
                }
                let mid = if stereo && c == 0 { Some(lbrr[1][i]) } else { None };
                let scale = i == 0 || !lbrr[c][i - 1];
                let fp = parse_frame(rd, bw, nsub, true, true, mid, &mut lctx[c], scale);
                if c == 0 && stereo && fp.mid_only { /* side LBRR absent */ }
            }
        }
        // §4.2.6 regular frames, mid/side interleaved per 20 ms
        let mut outs: [Vec<f32>; 2] = [vec![], vec![]];
        let mut rctx = [ParseCtx { first: true, ..Default::default() }; 2];
        for (c, ctx) in rctx.iter_mut().enumerate() { ctx.prev_log_gain = self.ch[c].prev_log_gain; }
        let mut weights = [self.prev_w_q13; 3];
        let mut side_coded_fr = [false; 3];
        for i in 0..nfr {
            let mut side_skip = false;
            for c in 0..nch {
                if c == 1 && side_skip {
                    // §4.2.7.2: no side frame; zeros go to unmixing; the side state is "not coded"
                    rctx[1].prev_coded = false; rctx[1].prev_log_gain = None;
                    self.ch[1].clear(); self.ch[1].prev_log_gain = None;
                    outs[1].extend(std::iter::repeat(0.0).take(nsub * subframe_len(bw)));
                    continue;
                }
                let mid = if stereo && c == 0 { Some(vad[1][i]) } else { None };
                let fp = parse_frame(rd, bw, nsub, vad[c][i], false, mid, &mut rctx[c], i == 0);
                if let Some(w) = fp.stereo_w_q13 { weights[i] = w; }
                if c == 0 && stereo { side_skip = fp.mid_only; }
                if c == 1 { side_coded_fr[i] = true; }
                let pcm = self.ch[c].synth(bw, &fp);
                self.ch[c].prev_log_gain = rctx[c].prev_log_gain;
                outs[c].extend(pcm);
            }
        }
        let _ = side_coded_fr;
        // §4.2.8 stereo unmixing (and the mono one-sample delay)
        let n_fr = nsub * subframe_len(bw);
        let n1 = match bw { Bandwidth::Nb => 64, Bandwidth::Mb => 96, _ => 128 };
        let total = outs[0].len();
        if !stereo {
            let mut o = Vec::with_capacity(total);
            for &m in &outs[0] { o.push(self.mid_hist[1]); self.mid_hist = [self.mid_hist[1], m]; }
            self.prev_stereo = false;
            return vec![o];
        }
        let (mut l, mut r) = (Vec::with_capacity(total), Vec::with_capacity(total));
        for fi in 0..nfr {
            let [w0n, w1n] = weights[fi];
            let [w0p, w1p] = self.prev_w_q13;
            for k in 0..n_fr {
                let i = fi * n_fr + k;
                let ph = k.min(n1) as f32;
                let w0 = w0p as f32 / 8192.0 + ph * (w0n - w0p) as f32 / (8192.0 * n1 as f32);
                let w1 = w1p as f32 / 8192.0 + ph * (w1n - w1p) as f32 / (8192.0 * n1 as f32);
                let m = outs[0][i]; let s = outs[1][i];
                let (m2, m1) = (self.mid_hist[0], self.mid_hist[1]);
                let p0 = (m2 + 2.0 * m1 + m) / 4.0;
                l.push(((1.0 + w1) * m1 + self.side_hist + w0 * p0).clamp(-1.0, 1.0));
                r.push(((1.0 - w1) * m1 - self.side_hist - w0 * p0).clamp(-1.0, 1.0));
                self.mid_hist = [m1, m]; self.side_hist = s;
            }
            self.prev_w_q13 = [w0n, w1n];
        }
        self.prev_stereo = true;
        vec![l, r]
    }
}

// ───────────────────────── SILK synthesis as a vocoder (TTS-SILK-VOCODER-0, J 2026-10-15) ─────────────────────────
// The TTS core's source (glottal pulse + noise) drives SILK's WB LPC synthesis. Its filter comes from formant targets via the
// encoder side of §4.2.7.5.6: formants → A(z) → LSF angles (roots of P and Q) → NLSF_Q15 through the inverse of the decoder's
// piecewise-linear cosine → the RFC's own stabilisation + NLSF → LPC (range and prediction-gain limiting) → LPC synthesis §4.2.7.9.2.

/// All-pole A(z) = Π (1 − 2 r cos θ z⁻¹ + r² z⁻²) for formants (Hz, bandwidth Hz) at sample rate fs. Returns [1, c1, …, c_d]
/// (A(z) = Σ c_k z⁻ᵏ; the RFC's a[k] = −c_k).
pub fn formants_to_poly(formants: &[(f64, f64)], fs: f64) -> Vec<f64> {
    let mut p = vec![1.0f64];
    for &(f, b) in formants {
        let r = (-std::f64::consts::PI * b / fs).exp();
        let th = 2.0 * std::f64::consts::PI * f / fs;
        let s = [1.0, -2.0 * r * th.cos(), r * r];
        let mut q = vec![0.0; p.len() + 2];
        for (i, &x) in p.iter().enumerate() { for (j, &y) in s.iter().enumerate() { q[i + j] += x * y; } }
        p = q;
    }
    p
}

/// LSF angles (0 < ω < π, ascending, d of them) of A(z) = [1, c1..c_d]: zeros of P(z) = A(z) + z^-(d+1) A(1/z) and
/// Q(z) = A(z) − z^-(d+1) A(1/z) on the unit circle, found as sign changes of the real-valued e^{jω(d+1)/2}·P / ·Q, refined by bisection.
pub fn poly_to_lsf(c: &[f64]) -> Vec<f64> {
    let d = c.len() - 1;
    let mut pc = vec![0.0; d + 2]; let mut qc = vec![0.0; d + 2];
    for k in 0..=d { pc[k] += c[k]; qc[k] += c[k]; pc[d + 1 - k] += c[k]; qc[d + 1 - k] -= c[k]; }
    // symmetric P: e^{jω(d+1)/2} P(e^{jω}) = Σ pc_k cos(((d+1)/2 − k) ω) (real); antisymmetric Q: Σ qc_k sin(((d+1)/2 − k) ω)
    let h = (d as f64 + 1.0) / 2.0;
    let fp = |w: f64| -> f64 { pc.iter().enumerate().map(|(k, &x)| x * ((h - k as f64) * w).cos()).sum() };
    let fq = |w: f64| -> f64 { qc.iter().enumerate().map(|(k, &x)| x * ((h - k as f64) * w).sin()).sum() };
    let mut roots = Vec::with_capacity(d);
    let steps = 8192;
    for f in [&fp as &dyn Fn(f64) -> f64, &fq] {
        let mut prev_w = 1e-9; let mut prev = f(prev_w);
        for s in 1..steps {
            let w = std::f64::consts::PI * s as f64 / steps as f64;
            let v = f(w);
            if (v > 0.0) != (prev > 0.0) {
                let (mut lo, mut hi, mut flo) = (prev_w, w, prev);
                for _ in 0..60 { let m = 0.5 * (lo + hi); let fm = f(m); if (fm > 0.0) == (flo > 0.0) { lo = m; flo = fm; } else { hi = m; } }
                roots.push(0.5 * (lo + hi));
            }
            prev_w = w; prev = v;
        }
    }
    roots.sort_by(|a, b| a.partial_cmp(b).unwrap());
    roots
}

/// §4.2.7.5.6 encoder side: the inverse of the decoder's piecewise-linear cosine (Table 28) — LSF angle ω → NLSF_Q15, so that
/// the decoder's c_Q17 reproduces cos(ω).
pub fn lsf_to_nlsf_q15(w: f64) -> i32 {
    let c = w.cos() * 4096.0;
    let mut i = 0usize;
    while i < 127 && (t::COS_Q12[i + 1] as f64) >= c { i += 1; }
    let (c0, c1) = (t::COS_Q12[i] as f64, t::COS_Q12[i + 1] as f64);
    let f = if c0 == c1 { 0.0 } else { ((c0 - c) / (c0 - c1) * 256.0).clamp(0.0, 255.0) };
    (((i as f64) * 256.0 + f).round() as i32).clamp(0, 32767)
}

/// One-channel SILK WB LPC synthesis driven by an external source (16 kHz, 80-sample subframes).
pub struct LpcVocoder { hist: Vec<f32> }

impl Default for LpcVocoder { fn default() -> Self { Self::new() } }

impl LpcVocoder {
    pub fn new() -> Self { LpcVocoder { hist: vec![0.0; 16] } }
    /// NLSF_Q15 (16, any order/spacing — stabilised here as §4.2.7.5.4 does) → the decoder's a_Q12
    pub fn filter(nlsf_q15: &[i32]) -> Vec<i32> {
        let mut n = nlsf_q15.to_vec();
        stabilize(&mut n, &t::LSF_MIN_SPACING_WB);
        nlsf_to_lpc(true, &n)
    }
    /// §4.2.7.9.2: lpc[i] = gain·exc[i] + Σ lpc[i−k−1]·a_Q12[k]/4096; output clamped to ±1 (history keeps the unclamped values)
    pub fn subframe(&mut self, a_q12: &[i32], gain: f32, exc: &[f32]) -> Vec<f32> {
        let a: Vec<f32> = a_q12.iter().map(|&x| x as f32 / 4096.0).collect();
        let d = a.len();
        let mut buf = self.hist.clone();
        for &e in exc {
            let n = buf.len();
            let mut v = gain * e;
            for k in 0..d { v += buf[n - k - 1] * a[k]; }
            buf.push(v);
        }
        let out: Vec<f32> = buf[self.hist.len()..].iter().map(|v| v.clamp(-1.0, 1.0)).collect();
        self.hist = buf[buf.len() - 16..].to_vec();
        out
    }
}
