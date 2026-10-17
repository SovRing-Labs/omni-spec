//! Opus audio decoder (RFC 6716, updated by RFC 8251) — built from the RFC prose and tables only (MEDIA-PLAYER-0 P3).
//! This file so far: packet framing (§3) and the range decoder (§4.1). CELT (§4.3) and SILK (§4.2) layers follow.
//! Conformance oracle (§6): the test vectors store, per packet, the reference decoder's FINAL RANGE state; a conformant decoder
//! must end every packet with the same `rng` — an exact check of every symbol decoded.

/// §3.1 Table 2: operating mode of a configuration number
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode { Silk, Hybrid, Celt }

/// §3.1 Table 2: audio bandwidth (NB 4 kHz, MB 6 kHz, WB 8 kHz, SWB 12 kHz, FB 20 kHz)
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bandwidth { Nb, Mb, Wb, Swb, Fb }

/// The TOC byte (§3.1, Figure 1: config(5) | s(1) | c(2), MSB first).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Toc { pub config: u8, pub stereo: bool, pub code: u8 }

impl Toc {
    pub fn parse(b: u8) -> Toc { Toc { config: b >> 3, stereo: b & 4 != 0, code: b & 3 } }
    pub fn mode(&self) -> Mode { match self.config { 0..=11 => Mode::Silk, 12..=15 => Mode::Hybrid, _ => Mode::Celt } }
    pub fn bandwidth(&self) -> Bandwidth {
        match self.config {
            0..=3 => Bandwidth::Nb, 4..=7 => Bandwidth::Mb, 8..=11 => Bandwidth::Wb, 12..=13 => Bandwidth::Swb, 14..=15 => Bandwidth::Fb,
            16..=19 => Bandwidth::Nb, 20..=23 => Bandwidth::Wb, 24..=27 => Bandwidth::Swb, _ => Bandwidth::Fb,
        }
    }
    /// frame size in samples at 48 kHz (Table 2: SILK 10/20/40/60 ms, Hybrid 10/20 ms, CELT 2.5/5/10/20 ms)
    pub fn frame_samples_48k(&self) -> usize {
        let c = self.config as usize;
        match self.mode() {
            Mode::Silk => [480, 960, 1920, 2880][c % 4],
            Mode::Hybrid => [480, 960][c % 2],
            Mode::Celt => [120, 240, 480, 960][c % 4],
        }
    }
}

/// A parsed packet: TOC + the compressed frames (§3.2). An empty frame (length 0) is DTX / lost (§3.2.1).
#[derive(Clone, Debug)]
pub struct Packet<'a> { pub toc: Toc, pub frames: Vec<&'a [u8]>, pub padding: usize }

impl Packet<'_> {
    pub fn samples_48k(&self) -> usize { self.toc.frame_samples_48k() * self.frames.len() }
}

/// §3.2.1 frame length: 0..=251 one byte; 252..=255 → second_byte*4 + first_byte. Returns (length, bytes used).
fn frame_len(b: &[u8]) -> Result<(usize, usize), String> {
    match b.first() {
        None => Err("missing frame length".into()),
        Some(&x) if x < 252 => Ok((x as usize, 1)),
        Some(&x) => match b.get(1) { Some(&y) => Ok((y as usize * 4 + x as usize, 2)), None => Err("truncated frame length".into()) },
    }
}

/// §3.2 frame packing, with the §3.4 malformed-packet rules R1–R7 enforced.
pub fn parse_packet(p: &[u8]) -> Result<Packet<'_>, String> {
    let Some(&t) = p.first() else { return Err("R1: empty packet".into()) };
    let toc = Toc::parse(t);
    let mut rest = &p[1..];
    let mut frames = Vec::new();
    let mut padding = 0;
    match toc.code {
        0 => frames.push(rest),
        1 => {
            if rest.len() % 2 != 0 { return Err("R3: code 1 payload length is odd".into()); }
            let h = rest.len() / 2;
            frames.push(&rest[..h]); frames.push(&rest[h..]);
        }
        2 => {
            let (n1, u) = frame_len(rest)?;
            rest = &rest[u..];
            if n1 > rest.len() { return Err("R4: code 2 first frame longer than the payload".into()); }
            frames.push(&rest[..n1]); frames.push(&rest[n1..]);
        }
        _ => {
            // §3.2.5 Figure 5: frame count byte v(1) p(1) M(6), MSB first
            let Some(&fc) = rest.first() else { return Err("R6: code 3 packet shorter than 2 bytes".into()) };
            rest = &rest[1..];
            let (vbr, pad, m) = (fc & 0x80 != 0, fc & 0x40 != 0, (fc & 0x3F) as usize);
            if m == 0 { return Err("R5: code 3 frame count 0".into()); }
            if m * toc.frame_samples_48k() > 5760 { return Err("R5: packet longer than 120 ms".into()); }
            if pad {
                // 0..=254 → that many bytes; 255 → 254 bytes plus the next byte's value (repeatable)
                loop {
                    let Some(&v) = rest.first() else { return Err("R6/R7: truncated padding length".into()) };
                    rest = &rest[1..];
                    if v == 255 { padding += 254; } else { padding += v as usize; break; }
                }
                if padding > rest.len() { return Err("R6/R7: padding longer than the packet".into()); }
                rest = &rest[..rest.len() - padding];
            }
            if vbr {
                let mut lens = Vec::with_capacity(m);
                for _ in 0..m - 1 { let (l, u) = frame_len(rest)?; rest = &rest[u..]; lens.push(l); }
                let sum: usize = lens.iter().sum();
                if sum > rest.len() { return Err("R7: frame lengths exceed the packet".into()); }
                lens.push(rest.len() - sum);
                for l in lens { frames.push(&rest[..l]); rest = &rest[l..]; }
            } else {
                if rest.len() % m != 0 { return Err("R6: CBR payload not a multiple of the frame count".into()); }
                let l = rest.len() / m;
                for k in 0..m { frames.push(&rest[k * l..(k + 1) * l]); }
            }
        }
    }
    if frames.iter().any(|f| f.len() > 1275) { return Err("R2: frame longer than 1275 bytes".into()); }
    Ok(Packet { toc, frames, padding })
}

/// ilog(x): bits needed to store x (ilog(0) = 0) — the RFC's ilog()
#[inline]
pub fn ilog(x: u32) -> u32 { 32 - x.leading_zeros() }

/// §4.1 range decoder over one Opus frame. State (val, rng) are 32-bit unsigned; raw bits are read from the frame's END (§4.1.4).
#[derive(Clone)]
pub struct RangeDecoder<'a> {
    buf: &'a [u8],
    /// next byte to read from the front
    offs: usize,
    /// bytes consumed from the end by raw bits
    end_offs: usize,
    end_window: u32,
    nend_bits: u32,
    /// §4.1.6 whole bits processed so far (range-coder renormalisation steps × 8 + raw bits)
    nbits_total: i32,
    pub rng: u32,
    val: u32,
    /// the last byte read: its low bit is the leftover bit of §4.1.1 / §4.1.2.1
    rem: u32,
    /// rng/ft from the last decode(), used by update()
    ext: u32,
    /// set when ec_dec_uint() saw t >= ft (§4.1.5) — the frame is corrupt
    pub error: bool,
}

impl<'a> RangeDecoder<'a> {
    /// §4.1.1: rng = 128, val = 127 - (b0 >> 1), keep b0 & 1, renormalise. nbits_total starts at 9 so that a fresh decoder's
    /// ec_tell() reports 1 bit used (§4.1.6.1: the bit reserved for the encoder's termination): 9 + 3 renormalisations × 8 = 33,
    /// and ilog(rng = 2^31) = 32.
    pub fn new(buf: &'a [u8]) -> Self {
        let b0 = buf.first().copied().unwrap_or(0) as u32;
        let mut d = RangeDecoder { buf, offs: 1, end_offs: 0, end_window: 0, nend_bits: 0, nbits_total: 9, rng: 128, val: 127 - (b0 >> 1), rem: b0, ext: 0, error: false };
        d.normalize();
        d
    }

    fn read_byte(&mut self) -> u32 {
        if self.offs < self.buf.len() { let b = self.buf[self.offs] as u32; self.offs += 1; b } else { 0 }
    }

    /// §4.1.2.1: while rng <= 2^23: rng <<= 8; sym = leftover bit (high) | top 7 bits of the next byte; val = ((val << 8) + 255 - sym) & 0x7FFFFFFF
    fn normalize(&mut self) {
        while self.rng <= 1 << 23 {
            self.nbits_total += 8;
            self.rng <<= 8;
            let b = self.read_byte();
            let sym = ((self.rem << 8 | b) >> 1) & 0xFF;
            self.rem = b;
            self.val = ((self.val << 8).wrapping_add(255 - sym)) & 0x7FFF_FFFF;
        }
    }

    /// §4.1.2 ec_decode(ft): fs = ft - min(val / (rng / ft) + 1, ft)
    pub fn decode(&mut self, ft: u32) -> u32 {
        self.ext = self.rng / ft;
        ft - (self.val / self.ext + 1).min(ft)
    }

    /// §4.1.3.1 ec_decode_bin(ftb) = ec_decode(1 << ftb)
    pub fn decode_bin(&mut self, ftb: u32) -> u32 {
        self.ext = self.rng >> ftb;
        (1u32 << ftb) - (self.val / self.ext + 1).min(1 << ftb)
    }

    /// §4.1.2 ec_dec_update(fl, fh, ft): val -= (rng/ft)·(ft - fh); rng = (rng/ft)·(fh - fl) if fl > 0, else rng - (rng/ft)·(ft - fh)
    pub fn update(&mut self, fl: u32, fh: u32, ft: u32) {
        let s = self.ext * (ft - fh);
        self.val -= s;
        self.rng = if fl > 0 { self.ext * (fh - fl) } else { self.rng - s };
        self.normalize();
    }

    /// §4.1.3.2 ec_dec_bit_logp(logp): P("1") = 2^-logp. Equivalent to decode(1<<logp) + update with (0, ft-1, ft) for a 0
    /// and (ft-1, ft, ft) for a 1; a 1 is decoded exactly when val < rng >> logp.
    pub fn bit_logp(&mut self, logp: u32) -> bool {
        let s = self.rng >> logp;
        let one = self.val < s;
        if one { self.rng = s; } else { self.val -= s; self.rng -= s; }
        self.normalize();
        one
    }

    /// §4.1.3.3 ec_dec_icdf(icdf, ftb): icdf[k] = (1<<ftb) - fh[k], table ends with 0. Returns k.
    pub fn icdf(&mut self, icdf: &[u8], ftb: u32) -> usize {
        let ft = 1u32 << ftb;
        let fs = self.decode_bin(ftb);
        let mut k = 0;
        while fs >= ft - icdf[k] as u32 { k += 1; }
        let fl = if k > 0 { ft - icdf[k - 1] as u32 } else { 0 };
        self.update(fl, ft - icdf[k] as u32, ft);
        k
    }

    /// Decode a symbol from a PDF given as frequency counts (the RFC's "{f0, f1, …}/ft" notation). Returns k.
    pub fn pdf(&mut self, f: &[u16]) -> usize {
        let ft: u32 = f.iter().map(|&x| x as u32).sum();
        let fs = self.decode(ft);
        let (mut k, mut fl) = (0usize, 0u32);
        while fl + f[k] as u32 <= fs { fl += f[k] as u32; k += 1; }
        self.update(fl, fl + f[k] as u32, ft);
        k
    }

    /// §4.1.4 ec_dec_bits(n): raw bits from the end of the frame, LSB of the last byte first. n <= 25.
    pub fn bits(&mut self, n: u32) -> u32 {
        if n == 0 { return 0; }
        while self.nend_bits < n {
            let b = if self.end_offs < self.buf.len() { self.end_offs += 1; self.buf[self.buf.len() - self.end_offs] as u32 } else { 0 };
            self.end_window |= b << self.nend_bits;
            self.nend_bits += 8;
        }
        let v = self.end_window & ((1u32 << n) - 1);
        self.end_window >>= n;
        self.nend_bits -= n;
        self.nbits_total += n as i32;
        v
    }

    /// §4.1.5 ec_dec_uint(ft): one of ft equiprobable values; > 8 bits → top 8 range-coded, the rest raw.
    pub fn uint(&mut self, ft: u32) -> u32 {
        let ftb = ilog(ft - 1);
        if ftb <= 8 {
            let t = self.decode(ft);
            self.update(t, t + 1, ft);
            t
        } else {
            let top = ((ft - 1) >> (ftb - 8)) + 1;
            let t = self.decode(top);
            self.update(t, t + 1, top);
            let t = t << (ftb - 8) | self.bits(ftb - 8);
            if t >= ft { self.error = true; ft - 1 } else { t }
        }
    }

    /// §4.1.6.1 ec_tell(): nbits_total - ilog(rng)
    pub fn tell(&self) -> i32 { self.nbits_total - ilog(self.rng) as i32 }

    /// §4.1.6.2 ec_tell_frac(): bits used in 1/8 bit, by squaring the Q15 fraction of rng three times
    pub fn tell_frac(&self) -> i32 {
        let mut lg = ilog(self.rng);
        let mut r = self.rng >> (lg - 16);
        for _ in 0..3 {
            r = (r * r) >> 15;
            let b = r >> 16;
            lg = 2 * lg + b;
            if b != 0 { r >>= 1; }
        }
        self.nbits_total * 8 - lg as i32
    }

    /// bytes in this frame
    pub fn storage(&self) -> usize { self.buf.len() }
}

/// One packet of a conformance-vector `.bit` file: 4-byte big-endian length, 4-byte big-endian final range, then the packet.
pub struct VectorPacket<'a> { pub data: &'a [u8], pub final_range: u32 }

pub fn read_vector(b: &[u8]) -> Result<Vec<VectorPacket<'_>>, String> {
    let mut out = Vec::new();
    let mut p = 0;
    while p < b.len() {
        let h = b.get(p..p + 8).ok_or("truncated vector header")?;
        let len = u32::from_be_bytes([h[0], h[1], h[2], h[3]]) as usize;
        let rng = u32::from_be_bytes([h[4], h[5], h[6], h[7]]);
        let data = b.get(p + 8..p + 8 + len).ok_or("truncated vector packet")?;
        out.push(VectorPacket { data, final_range: rng });
        p += 8 + len;
    }
    Ok(out)
}

/// Resampler from a SILK internal rate to 48 kHz (§4.2.9: the method is non-normative, the DELAY is normative — Table 54:
/// NB 0.538 ms, MB 0.692 ms, WB 0.706 ms). Windowed-sinc interpolation centred on exactly that delay D (48 kHz samples):
/// y[m] = Σ_n x[n] · sinc(t/R) · hann(t/D), t = m − D − nR, R = 48000 / rate. The window spans |t| < D, so only past and
/// present input is used (causal at delay D).
pub struct Resampler { ratio: usize, delay: f64, hist: Vec<f32>, phase: Vec<Vec<f32>> }

impl Resampler {
    pub fn new(rate: u32) -> Resampler {
        let ratio = (48000 / rate) as usize;
        let delay = match rate { 8000 => 0.538, 12000 => 0.692, _ => 0.706 } * 48.0;
        let ntaps = (2.0 * delay / ratio as f64).ceil() as usize + 1;
        // output m = q·R + r: tap k uses input n = q − k, t = r − D + k·R
        let phase = (0..ratio).map(|r| (0..ntaps).map(|k| {
            let tt = r as f64 - delay + (k * ratio) as f64;
            let u = tt / ratio as f64;
            let sinc = if u.abs() < 1e-12 { 1.0 } else { (std::f64::consts::PI * u).sin() / (std::f64::consts::PI * u) };
            let wv = tt / delay;
            let win = if wv.abs() >= 1.0 { 0.0 } else { 0.5 + 0.5 * (std::f64::consts::PI * wv).cos() };
            (sinc * win) as f32
        }).collect()).collect();
        Resampler { ratio, delay, hist: vec![0.0; ntaps], phase }
    }
    pub fn ratio(&self) -> usize { self.ratio }
    pub fn delay_48k(&self) -> f64 { self.delay }
    /// push input samples, get ratio× as many output samples
    pub fn process(&mut self, x: &[f32]) -> Vec<f32> {
        let base = self.hist.len();
        let mut buf = self.hist.clone(); buf.extend_from_slice(x);
        let mut out = Vec::with_capacity(x.len() * self.ratio);
        for q in base..buf.len() {
            for taps in &self.phase {
                let mut acc = 0f32;
                for (k, &c) in taps.iter().enumerate() { acc += buf[q - k] * c; }
                out.push(acc);
            }
        }
        self.hist = buf[buf.len() - base..].to_vec();
        out
    }
}

/// An Opus decoder (§4): 48 kHz stereo output. SILK-only packets are decoded; Hybrid / CELT frames are not yet (they produce
/// silence of the right length and no final range).
pub struct Decoder {
    silk: crate::opus_silk::Silk,
    resamp: Vec<Resampler>,
    resamp_rate: u32,
    prev_mode: Option<Mode>,
}

/// One decoded packet: interleaved stereo f32 at 48 kHz, and the range coder's final state when every frame was decoded.
pub struct Decoded { pub pcm: Vec<f32>, pub final_range: Option<u32>, pub redundancy: bool }

impl Default for Decoder { fn default() -> Self { Self::new() } }

impl Decoder {
    pub fn new() -> Self { Decoder { silk: crate::opus_silk::Silk::new(), resamp: vec![], resamp_rate: 0, prev_mode: None } }

    pub fn decode(&mut self, packet: &[u8]) -> Result<Decoded, String> {
        let p = parse_packet(packet)?;
        let fs = p.toc.frame_samples_48k();
        let mut pcm = Vec::with_capacity(p.samples_48k() * 2);
        let mut fr = None; let mut red = false;
        for f in &p.frames {
            let mode = p.toc.mode();
            match mode {
                Mode::Silk if !f.is_empty() => {
                    // §4.5.2: SILK state resets before a SILK frame that follows a CELT-only frame
                    if self.prev_mode == Some(Mode::Celt) { self.silk.reset(); }
                    let bw = p.toc.bandwidth();
                    let mut rd = RangeDecoder::new(f);
                    let ch = self.silk.decode(&mut rd, bw, (fs / 48) as u32, p.toc.stereo);
                    // §4.5.1.1: SILK-only redundancy is implicit — at least 17 bits left; then the position flag (Table 65)
                    if (f.len() as i32) * 8 - rd.tell() >= 17 { red = true; let _pos = rd.bit_logp(1); }
                    fr = Some(rd.rng);
                    let rate = crate::opus_silk::internal_rate(bw);
                    if self.resamp_rate != rate { self.resamp = vec![Resampler::new(rate), Resampler::new(rate)]; self.resamp_rate = rate; }
                    let l = self.resamp[0].process(&ch[0]);
                    let r = if ch.len() > 1 { self.resamp[1].process(&ch[1]) } else { l.clone() };
                    for k in 0..l.len() { pcm.push(l[k]); pcm.push(r[k]); }
                }
                _ => { pcm.extend(std::iter::repeat(0.0).take(fs * 2)); fr = None; }
            }
            self.prev_mode = Some(mode);
        }
        Ok(Decoded { pcm, final_range: fr, redundancy: red })
    }
}
