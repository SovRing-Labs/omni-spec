//! Opus CELT layer (RFC 6716 §4.3) — the parts the RFC prose specifies exactly. MEDIA-PLAYER-0 P3, step C1.
//! J 2026-10-15: CELT's pieces that the prose leaves to the reference code are DERIVED as math (Armilla / math core) or FITTED against
//! the conformance vectors' per-packet final range — never read from the RFC's appendix code. This file grows step by step.
use crate::opus::RangeDecoder;

/// Table 55: MDCT bins per band for 2.5 ms frames (×2^LM for 5 / 10 / 20 ms); 21 bands, 0–20 kHz.
pub const BAND_BINS_LM0: [usize; 21] = [1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 4, 4, 4, 6, 6, 8, 12, 18, 22];

/// band edges in bins for LM (0..=3): edge[b]..edge[b+1]
pub fn band_edges(lm: u32) -> [usize; 22] {
    let mut e = [0usize; 22];
    for b in 0..21 { e[b + 1] = e[b] + (BAND_BINS_LM0[b] << lm); }
    e
}

/// Number of coded bands for an audio bandwidth (end band): NB 4 kHz → 13, WB 8 kHz → 17, SWB 12 kHz → 19, FB 20 kHz → 21 (Table 55 stop
/// frequencies: band 12 ends at 4000 Hz, 16 at 8000, 18 at 12000, 20 at 20000).
pub fn end_band(bw: crate::opus::Bandwidth) -> usize {
    use crate::opus::Bandwidth::*;
    match bw { Nb => 13, Mb | Wb => 17, Swb => 19, Fb => 21 }
}

/// The frame-header symbols of Table 56 up to (not including) the coarse energy.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Header {
    pub silence: bool,
    /// §4.3.7.1: (pitch period 15..=1022, gain index 0..7 → G = 3(g+1)/32, tapset 0..2)
    pub postfilter: Option<(u32, u32, usize)>,
    pub transient: bool,
    pub intra: bool,
}

/// Table 56 order: silence {32767,1}/32768; post-filter {1,1}/2 then octave uniform(6), period raw 4+octave, gain raw 3, tapset {2,1,1}/4;
/// transient {7,1}/8 — only when LM > 0 (a 2.5 ms frame has no shorter MDCTs; settled on the vectors 2026-10-15: with the flag read at
/// LM 0, intra band-0 energies vs their range position scatter (9 order violations in 36 frames); without it they form the coarse 6 dB
/// staircase (1 in 30) — `projects/media-player/results/celt-c1/`); intra {7,1}/8.
pub fn decode_header(rd: &mut RangeDecoder, lm: u32) -> Header {
    let mut h = Header { silence: rd.bit_logp(15), ..Default::default() };
    if h.silence { return h; }
    if rd.bit_logp(1) {
        let octave = rd.uint(6);
        let fine = rd.bits(4 + octave);
        let period = (16u32 << octave) + fine - 1;
        let gain = rd.bits(3);
        let tapset = rd.pdf(&[2, 1, 1]);
        h.postfilter = Some((period, gain, tapset));
    }
    h.transient = lm > 0 && rd.bit_logp(3);
    h.intra = rd.bit_logp(3);
    h
}

/// A coarse-energy Laplace coder (§4.3.2.1: "a Laplace distribution with separate parameters for each frame size in intra- and inter-frame
/// modes"; the prose leaves the coder's integer layout to the reference code). Our family, designed for the G2 God search
/// (`projects/media-player/results/celt-c1/god_band0.py`) and fitted against the vectors' exact range positions: on a 15-bit scale,
/// symbol 0 = [0, fs0); then for k = 1, 2, …: −k then +k, each of width w_k = max(g_k, min) ('floor') or g_k + min ('add'), with
/// g_1 = ((32768 − 2·min·reserve − fs0)·(16384 − decay)) >> 15 and g_{k+1} = (g_k·decay) >> 14. Symbol order 0, −1, +1, −2, … (probe 3).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Laplace { pub add: bool, pub min: u32, pub reserve: u32, pub fs0: u32, pub decay: u32 }

impl Laplace {
    /// the interval [fl, fl + w) of symbol q on the 15-bit scale (None past the table)
    pub fn interval(&self, q: i32) -> Option<(u32, u32)> {
        if q == 0 { return Some((0, self.fs0)); }
        let mut g = (32768u32.saturating_sub(2 * self.min * self.reserve).saturating_sub(self.fs0) * (16384 - self.decay.min(16384))) >> 15;
        let mut cum = self.fs0;
        for k in 1..=q.unsigned_abs() {
            let w = if self.add { g + self.min } else { g.max(self.min) }.max(1);
            if k == q.unsigned_abs() { let fl = if q < 0 { cum } else { cum + w }; return (fl + w <= 32768).then_some((fl, w)); }
            cum += 2 * w;
            if cum > 32768 { return None; }
            g = (g * self.decay) >> 14;
        }
        None
    }

    /// decode one symbol: fs = decode_bin(15), walk the intervals, update. None = fs past the modelled table (rd untouched after decode_bin).
    pub fn decode(&self, rd: &mut RangeDecoder) -> Option<i32> {
        let fs = rd.decode_bin(15);
        for k in 0..64i32 {
            for q in if k == 0 { vec![0] } else { vec![-k, k] } {
                let (fl, w) = self.interval(q)?;
                if fs >= fl && fs < fl + w { rd.update(fl, fl + w, 32768); return Some(q); }
            }
        }
        None
    }
}
