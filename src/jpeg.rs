//! JPEG decoder (ITU-T T.81 | ISO/IEC 10918-1): baseline sequential (SOF0/SOF1) and progressive Huffman (SOF2),
//! 8-bit precision, 1 or 3 components, restart intervals, and the JFIF / Adobe RGB colour cases.
//!
//! Spec: ITU-T T.81 — B.2 markers and segment layout; B.2.4 Huffman tables (DHT), B.2.4.1 quantization tables (DQT);
//! F.1.2 frame, F.2.2 sequential decoding (DC/AC coefficient coding, EOB, ZRL); F.2.2.3 Huffman decoding procedure;
//! G.1.2 progressive decoding (spectral selection, successive approximation: DC first/refine, AC first/refine
//! with EOBRUN); A.3.3 the inverse DCT; F.1.1.5 / JFIF for YCbCr → RGB. Written from the specification text.
//!
//! Output is RGB8. Chroma is upsampled by nearest neighbour (sampling factors need not be 1:1). Arithmetic coding,
//! lossless, 12-bit and hierarchical frames are rejected as `Unsupported`.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    BadMarker(u8),
    Truncated,
    Unsupported(&'static str),
    BadHuffman(&'static str),
    BadScan(&'static str),
    BadFrame(&'static str),
}

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    /// RGB8, row-major, `width * height * 3` bytes.
    pub rgb: Vec<u8>,
}

/// Zigzag order: index in zigzag sequence -> natural (row-major) index, Annex A.3.6 Figure A.6.
const ZZ: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21, 28,
    35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55,
    62, 63,
];

/// Huffman table, decoded per F.2.2.3 (mincode / maxcode / valptr per code length).
#[derive(Clone)]
struct Huff {
    mincode: [i32; 17],
    maxcode: [i32; 17],
    valptr: [i32; 17],
    values: Vec<u8>,
}

impl Huff {
    fn new(counts: &[u8; 16], values: Vec<u8>) -> Result<Huff> {
        let mut t = Huff { mincode: [0; 17], maxcode: [-1; 17], valptr: [0; 17], values };
        let mut code: i32 = 0;
        let mut k: i32 = 0;
        for l in 1..=16usize {
            let n = counts[l - 1] as i32;
            t.valptr[l] = k;
            t.mincode[l] = code;
            code += n;
            k += n;
            if n > 0 {
                t.maxcode[l] = code - 1;
                if code - 1 >= (1 << l) {
                    return Err(Error::BadHuffman("code space exhausted"));
                }
            }
            code <<= 1;
        }
        if k as usize != t.values.len() {
            return Err(Error::BadHuffman("value count mismatch"));
        }
        Ok(t)
    }
}

/// Bit reader over an entropy-coded segment (B.1.1.5 bit stuffing; markers end the segment).
struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
    buf: u32,
    n: u32,
    marker: bool,
}

impl<'a> Bits<'a> {
    fn fill(&mut self) {
        while self.n <= 24 {
            let mut b = 0u8;
            if !self.marker && self.pos < self.d.len() {
                let c = self.d[self.pos];
                if c == 0xFF {
                    let next = self.d.get(self.pos + 1).copied().unwrap_or(0xD9);
                    if next == 0 {
                        self.pos += 2;
                        b = 0xFF;
                    } else {
                        self.marker = true;
                    }
                } else {
                    self.pos += 1;
                    b = c;
                }
            }
            self.buf = (self.buf << 8) | b as u32;
            self.n += 8;
        }
    }

    fn bit(&mut self) -> u32 {
        if self.n == 0 {
            self.fill();
        }
        self.n -= 1;
        (self.buf >> self.n) & 1
    }

    fn bits(&mut self, k: u32) -> u32 {
        let mut v = 0;
        for _ in 0..k {
            v = (v << 1) | self.bit();
        }
        v
    }

    fn huff(&mut self, t: &Huff) -> Result<u8> {
        let mut code: i32 = 0;
        for l in 1..=16usize {
            code = (code << 1) | self.bit() as i32;
            if t.maxcode[l] >= 0 && code <= t.maxcode[l] {
                let idx = t.valptr[l] + code - t.mincode[l];
                return t.values.get(idx as usize).copied().ok_or(Error::BadHuffman("symbol index"));
            }
        }
        Err(Error::BadHuffman("no code matches"))
    }

    /// RECEIVE + EXTEND (F.2.2.1): read `s` bits and sign-extend.
    fn receive_extend(&mut self, s: u32) -> i32 {
        if s == 0 {
            return 0;
        }
        let v = self.bits(s) as i32;
        if v < (1 << (s - 1)) {
            v - (1 << s) + 1
        } else {
            v
        }
    }

    /// Restart: discard buffered bits and skip the RSTn marker (B.2.1, E.1.4).
    fn restart(&mut self) {
        self.n = 0;
        self.buf = 0;
        self.marker = false;
        while self.pos + 1 < self.d.len() {
            if self.d[self.pos] == 0xFF && (0xD0..=0xD7).contains(&self.d[self.pos + 1]) {
                self.pos += 2;
                return;
            }
            if self.d[self.pos] == 0xFF && self.d[self.pos + 1] != 0 {
                return; // a different marker; leave it for the marker parser
            }
            self.pos += 1;
        }
    }
}

#[derive(Clone)]
struct Component {
    id: u8,
    h: usize,
    v: usize,
    tq: usize,
    /// Blocks per line / column in the padded (MCU-aligned) storage.
    bpl: usize,
    bpc: usize,
    /// Coefficients per block, natural order, `bpl * bpc * 64` entries.
    coef: Vec<i32>,
    dc_table: usize,
    ac_table: usize,
    pred: i32,
}

struct Frame {
    width: usize,
    height: usize,
    progressive: bool,
    hmax: usize,
    vmax: usize,
    mcus_x: usize,
    mcus_y: usize,
    comps: Vec<Component>,
}

/// Decode a JPEG file to RGB8.
pub fn decode(data: &[u8]) -> Result<Image> {
    let mut pos = 0usize;
    if data.get(0..2) != Some(&[0xFF, 0xD8]) {
        return Err(Error::BadMarker(0xD8));
    }
    pos += 2;
    let mut qt: [[u16; 64]; 4] = [[0; 64]; 4];
    let mut dc: Vec<Option<Huff>> = vec![None; 4];
    let mut ac: Vec<Option<Huff>> = vec![None; 4];
    let mut frame: Option<Frame> = None;
    let mut restart_interval = 0usize;
    let mut adobe_transform: Option<u8> = None;

    loop {
        // Find the next marker.
        while pos < data.len() && data[pos] != 0xFF {
            pos += 1;
        }
        while pos < data.len() && data[pos] == 0xFF {
            pos += 1; // fill bytes
        }
        let m = *data.get(pos).ok_or(Error::Truncated)?;
        pos += 1;
        match m {
            0xD9 => break, // EOI
            0xD0..=0xD7 | 0x01 => continue,
            0xD8 => continue,
            _ => {}
        }
        let len = u16::from_be_bytes([*data.get(pos).ok_or(Error::Truncated)?, *data.get(pos + 1).ok_or(Error::Truncated)?]) as usize;
        if len < 2 {
            return Err(Error::BadMarker(m));
        }
        let seg = data.get(pos + 2..pos + len).ok_or(Error::Truncated)?;
        match m {
            0xDB => {
                // DQT (B.2.4.1): Pq/Tq byte then 64 values in zigzag order.
                let mut p = 0;
                while p < seg.len() {
                    let pq = seg[p] >> 4;
                    let tq = (seg[p] & 3) as usize;
                    p += 1;
                    for k in 0..64 {
                        let v = if pq == 0 {
                            let v = *seg.get(p).ok_or(Error::Truncated)? as u16;
                            p += 1;
                            v
                        } else {
                            let v = u16::from_be_bytes([*seg.get(p).ok_or(Error::Truncated)?, *seg.get(p + 1).ok_or(Error::Truncated)?]);
                            p += 2;
                            v
                        };
                        qt[tq][ZZ[k]] = v;
                    }
                }
            }
            0xC4 => {
                // DHT (B.2.4.2).
                let mut p = 0;
                while p < seg.len() {
                    let tc = seg[p] >> 4;
                    let th = (seg[p] & 3) as usize;
                    p += 1;
                    let mut counts = [0u8; 16];
                    counts.copy_from_slice(seg.get(p..p + 16).ok_or(Error::Truncated)?);
                    p += 16;
                    let total: usize = counts.iter().map(|&c| c as usize).sum();
                    let values = seg.get(p..p + total).ok_or(Error::Truncated)?.to_vec();
                    p += total;
                    let t = Huff::new(&counts, values)?;
                    if tc == 0 {
                        dc[th] = Some(t);
                    } else {
                        ac[th] = Some(t);
                    }
                }
            }
            0xC0 | 0xC1 | 0xC2 => {
                // SOF (B.2.2): baseline, extended sequential (Huffman), progressive (Huffman).
                if seg.len() < 6 || seg[0] != 8 {
                    return Err(Error::Unsupported("sample precision other than 8"));
                }
                let height = u16::from_be_bytes([seg[1], seg[2]]) as usize;
                let width = u16::from_be_bytes([seg[3], seg[4]]) as usize;
                let nf = seg[5] as usize;
                if width == 0 || height == 0 {
                    return Err(Error::BadFrame("zero dimension (DNL not supported)"));
                }
                if nf != 1 && nf != 3 {
                    return Err(Error::Unsupported("component count other than 1 or 3"));
                }
                let mut comps = Vec::with_capacity(nf);
                for i in 0..nf {
                    let b = seg.get(6 + 3 * i..9 + 3 * i).ok_or(Error::Truncated)?;
                    let h = (b[1] >> 4) as usize;
                    let v = (b[1] & 15) as usize;
                    if !(1..=4).contains(&h) || !(1..=4).contains(&v) {
                        return Err(Error::BadFrame("sampling factor"));
                    }
                    comps.push(Component {
                        id: b[0],
                        h,
                        v,
                        tq: (b[2] & 3) as usize,
                        bpl: 0,
                        bpc: 0,
                        coef: Vec::new(),
                        dc_table: 0,
                        ac_table: 0,
                        pred: 0,
                    });
                }
                let hmax = comps.iter().map(|c| c.h).max().unwrap();
                let vmax = comps.iter().map(|c| c.v).max().unwrap();
                let mcus_x = width.div_ceil(8 * hmax);
                let mcus_y = height.div_ceil(8 * vmax);
                for c in comps.iter_mut() {
                    c.bpl = mcus_x * c.h;
                    c.bpc = mcus_y * c.v;
                    c.coef = vec![0; c.bpl * c.bpc * 64];
                }
                frame = Some(Frame { width, height, progressive: m == 0xC2, hmax, vmax, mcus_x, mcus_y, comps });
            }
            0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => {
                return Err(Error::Unsupported("arithmetic, lossless, or hierarchical frame"));
            }
            0xDD => {
                // DRI (B.2.4.4).
                restart_interval = u16::from_be_bytes([*seg.first().ok_or(Error::Truncated)?, *seg.get(1).ok_or(Error::Truncated)?]) as usize;
            }
            0xEE => {
                // APP14 Adobe: transform flag at byte 11.
                if seg.len() >= 12 && &seg[0..5] == b"Adobe" {
                    adobe_transform = Some(seg[11]);
                }
            }
            0xDA => {
                // SOS (B.2.3) then entropy-coded data.
                let f = frame.as_mut().ok_or(Error::BadScan("SOS before SOF"))?;
                let ns = seg[0] as usize;
                let mut scan_comps: Vec<usize> = Vec::with_capacity(ns);
                for i in 0..ns {
                    let cs = seg[1 + 2 * i];
                    let t = seg[2 + 2 * i];
                    let ci = f.comps.iter().position(|c| c.id == cs).ok_or(Error::BadScan("unknown component"))?;
                    f.comps[ci].dc_table = (t >> 4) as usize & 3;
                    f.comps[ci].ac_table = (t & 15) as usize & 3;
                    scan_comps.push(ci);
                }
                let p = 1 + 2 * ns;
                let ss = seg[p] as usize;
                let se = seg[p + 1] as usize;
                let ah = (seg[p + 2] >> 4) as u32;
                let al = (seg[p + 2] & 15) as u32;
                let start = pos + len;
                let mut bits = Bits { d: data, pos: start, buf: 0, n: 0, marker: false };
                decode_scan(f, &mut bits, &scan_comps, ss, se, ah, al, restart_interval, &dc, &ac)?;
                // Continue marker parsing where the entropy-coded data ended.
                pos = bits.pos;
                continue;
            }
            _ => {} // APPn, COM, DNL, etc.
        }
        pos += len;
    }

    let f = frame.ok_or(Error::BadFrame("no frame"))?;
    if f.comps.len() == 4 {
        return Err(Error::Unsupported("CMYK"));
    }
    Ok(output(&f, &qt, adobe_transform))
}

/// One scan's entropy-coded data into the coefficient arrays (F.2.2, G.1.2).
#[allow(clippy::too_many_arguments)]
fn decode_scan(
    f: &mut Frame,
    bits: &mut Bits,
    scan: &[usize],
    ss: usize,
    se: usize,
    ah: u32,
    al: u32,
    restart_interval: usize,
    dc: &[Option<Huff>],
    ac: &[Option<Huff>],
) -> Result<()> {
    let progressive = f.progressive;
    if progressive {
        if ss > se || se > 63 || (ss == 0 && se != 0) {
            return Err(Error::BadScan("spectral selection"));
        }
    } else if ss != 0 || se != 63 || ah != 0 || al != 0 {
        return Err(Error::BadScan("sequential scan parameters"));
    }
    for c in f.comps.iter_mut() {
        c.pred = 0;
    }
    let mut eobrun: u32 = 0;
    // MCU geometry (A.2.3): a single-component scan is not interleaved and covers only the component's own blocks.
    let (units_x, units_y, single) = if scan.len() == 1 {
        let c = &f.comps[scan[0]];
        let cw = (f.width * c.h).div_ceil(f.hmax);
        let ch = (f.height * c.v).div_ceil(f.vmax);
        (cw.div_ceil(8), ch.div_ceil(8), true)
    } else {
        (f.mcus_x, f.mcus_y, false)
    };
    let total = units_x * units_y;
    for n in 0..total {
        if restart_interval > 0 && n > 0 && n % restart_interval == 0 {
            bits.restart();
            for c in f.comps.iter_mut() {
                c.pred = 0;
            }
            eobrun = 0;
        }
        let (ux, uy) = (n % units_x, n / units_x);
        if single {
            let ci = scan[0];
            let block = uy * f.comps[ci].bpl + ux;
            decode_block(&mut f.comps[ci], bits, block, progressive, ss, se, ah, al, dc, ac, &mut eobrun)?;
        } else {
            for &ci in scan {
                let (h, v) = (f.comps[ci].h, f.comps[ci].v);
                for by in 0..v {
                    for bx in 0..h {
                        let row = uy * v + by;
                        let col = ux * h + bx;
                        let bpl = f.comps[ci].bpl;
                        decode_block(&mut f.comps[ci], bits, row * bpl + col, progressive, ss, se, ah, al, dc, ac, &mut eobrun)?;
                    }
                }
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn decode_block(
    c: &mut Component,
    bits: &mut Bits,
    block: usize,
    progressive: bool,
    ss: usize,
    se: usize,
    ah: u32,
    al: u32,
    dc: &[Option<Huff>],
    ac: &[Option<Huff>],
    eobrun: &mut u32,
) -> Result<()> {
    let out = &mut c.coef[block * 64..block * 64 + 64];
    let dct = dc[c.dc_table].as_ref();
    let act = ac[c.ac_table].as_ref();
    if !progressive {
        // Baseline: DC then AC 1..63 within the block, with an EOB per block (F.2.2).
        let t = dct.ok_or(Error::BadHuffman("missing DC table"))?;
        let s = bits.huff(t)? as u32;
        let diff = bits.receive_extend(s);
        c.pred += diff;
        out[0] = c.pred;
        let at = act.ok_or(Error::BadHuffman("missing AC table"))?;
        let mut k = 1usize;
        while k < 64 {
            let rs = bits.huff(at)?;
            let (r, s) = ((rs >> 4) as usize, (rs & 15) as u32);
            if s == 0 {
                if r == 15 {
                    k += 16;
                    continue;
                }
                break;
            }
            k += r;
            if k > 63 {
                return Err(Error::BadScan("AC run past block end"));
            }
            out[ZZ[k]] = bits.receive_extend(s);
            k += 1;
        }
        return Ok(());
    }
    if ss == 0 {
        // DC scans (G.1.2.1).
        if ah == 0 {
            let t = dct.ok_or(Error::BadHuffman("missing DC table"))?;
            let s = bits.huff(t)? as u32;
            let diff = bits.receive_extend(s);
            c.pred += diff;
            out[0] = c.pred * (1 << al);
        } else if bits.bit() == 1 {
            out[0] |= 1 << al;
        }
        return Ok(());
    }
    let at = act.ok_or(Error::BadHuffman("missing AC table"))?;
    if ah == 0 {
        // AC first (G.1.2.2).
        if *eobrun > 0 {
            *eobrun -= 1;
            return Ok(());
        }
        let mut k = ss;
        while k <= se {
            let rs = bits.huff(at)?;
            let (r, s) = ((rs >> 4) as usize, (rs & 15) as u32);
            if s == 0 {
                if r < 15 {
                    *eobrun = (1u32 << r) + bits.bits(r as u32);
                    *eobrun -= 1;
                    break;
                }
                k += 16;
                continue;
            }
            k += r;
            if k > 63 {
                return Err(Error::BadScan("AC run past block end"));
            }
            out[ZZ[k]] = bits.receive_extend(s) * (1 << al);
            k += 1;
        }
        return Ok(());
    }
    // AC refinement (G.1.2.3).
    let p1: i32 = 1 << al;
    let m1: i32 = -1 << al;
    let mut k = ss;
    if *eobrun == 0 {
        while k <= se {
            let rs = bits.huff(at)?;
            let mut r = (rs >> 4) as i32;
            let s0 = (rs & 15) as u32;
            let mut s: i32 = 0;
            if s0 != 0 {
                s = if bits.bit() == 1 { p1 } else { m1 };
            } else if r != 15 {
                *eobrun = 1u32 << r;
                if r > 0 {
                    *eobrun += bits.bits(r as u32);
                }
                break;
            }
            while k <= se {
                let idx = ZZ[k];
                if out[idx] != 0 {
                    if bits.bit() == 1 && (out[idx] & p1) == 0 {
                        out[idx] += if out[idx] >= 0 { p1 } else { m1 };
                    }
                } else {
                    r -= 1;
                    if r < 0 {
                        break;
                    }
                }
                k += 1;
            }
            if s != 0 && k <= 63 {
                out[ZZ[k]] = s;
            }
            k += 1;
        }
    }
    if *eobrun > 0 {
        while k <= se {
            let idx = ZZ[k];
            if out[idx] != 0 && bits.bit() == 1 && (out[idx] & p1) == 0 {
                out[idx] += if out[idx] >= 0 { p1 } else { m1 };
            }
            k += 1;
        }
        *eobrun -= 1;
    }
    Ok(())
}

/// Integer inverse DCT, the "islow" accurate method (T.81 A.3.3 realised with 13-bit fixed-point constants,
/// Loeffler-Ligtenberg-Moschytz factorisation). Dequantizes, transforms columns then rows, level-shifts and clamps.
/// Constants are FIX(x) = round(x · 2^13) for the cosine values used by the factorisation.
fn idct_block(coef: &[i32], q: &[u16; 64], out: &mut [u8; 64]) {
    const CONST_BITS: i32 = 13;
    const PASS1_BITS: i32 = 2;
    let fix = |x: f64| (x * (1 << CONST_BITS) as f64).round() as i64;
    let f0_298 = fix(0.298631336);
    let f0_390 = fix(0.390180644);
    let f0_541 = fix(0.541196100);
    let f0_765 = fix(0.765366865);
    let f0_899 = fix(0.899976223);
    let f1_175 = fix(1.175875602);
    let f1_501 = fix(1.501321110);
    let f1_847 = fix(1.847759065);
    let f1_961 = fix(1.961570560);
    let f2_053 = fix(2.053119869);
    let f2_562 = fix(2.562915447);
    let f3_072 = fix(3.072711026);
    let descale = |x: i64, n: i32| -> i64 { (x + (1i64 << (n - 1))) >> n };
    let mut ws = [0i64; 64];
    // Pass 1: columns.
    for col in 0..8 {
        let c = |r: usize| coef[r * 8 + col] as i64 * q[r * 8 + col] as i64;
        if (1..8).all(|r| c(r) == 0) {
            let dc = c(0) << PASS1_BITS;
            for r in 0..8 {
                ws[r * 8 + col] = dc;
            }
            continue;
        }
        let z2 = c(2);
        let z3 = c(6);
        let z1 = (z2 + z3) * f0_541;
        let tmp2 = z1 + z3 * -f1_847;
        let tmp3 = z1 + z2 * f0_765;
        let tmp0 = (c(0) + c(4)) << CONST_BITS;
        let tmp1 = (c(0) - c(4)) << CONST_BITS;
        let tmp10 = tmp0 + tmp3;
        let tmp13 = tmp0 - tmp3;
        let tmp11 = tmp1 + tmp2;
        let tmp12 = tmp1 - tmp2;
        let (mut t0, mut t1, mut t2, mut t3) = (c(7), c(5), c(3), c(1));
        let (mut z1, mut z2, mut z3, mut z4) = (t0 + t3, t1 + t2, t0 + t2, t1 + t3);
        let z5 = (z3 + z4) * f1_175;
        t0 *= f0_298;
        t1 *= f2_053;
        t2 *= f3_072;
        t3 *= f1_501;
        z1 *= -f0_899;
        z2 *= -f2_562;
        z3 *= -f1_961;
        z4 *= -f0_390;
        z3 += z5;
        z4 += z5;
        t0 += z1 + z3;
        t1 += z2 + z4;
        t2 += z2 + z3;
        t3 += z1 + z4;
        let n = CONST_BITS - PASS1_BITS;
        ws[col] = descale(tmp10 + t3, n);
        ws[7 * 8 + col] = descale(tmp10 - t3, n);
        ws[8 + col] = descale(tmp11 + t2, n);
        ws[6 * 8 + col] = descale(tmp11 - t2, n);
        ws[2 * 8 + col] = descale(tmp12 + t1, n);
        ws[5 * 8 + col] = descale(tmp12 - t1, n);
        ws[3 * 8 + col] = descale(tmp13 + t0, n);
        ws[4 * 8 + col] = descale(tmp13 - t0, n);
    }
    // Pass 2: rows, with the final descale and level shift.
    let n = CONST_BITS + PASS1_BITS + 3;
    for row in 0..8 {
        let w = |k: usize| ws[row * 8 + k];
        let shift = |v: i64| (descale(v, n) + 128).clamp(0, 255) as u8;
        if (1..8).all(|k| w(k) == 0) {
            let v = (descale(w(0), PASS1_BITS + 3) + 128).clamp(0, 255) as u8;
            out[row * 8..row * 8 + 8].fill(v);
            continue;
        }
        let z2 = w(2);
        let z3 = w(6);
        let z1 = (z2 + z3) * f0_541;
        let tmp2 = z1 + z3 * -f1_847;
        let tmp3 = z1 + z2 * f0_765;
        let tmp0 = (w(0) + w(4)) << CONST_BITS;
        let tmp1 = (w(0) - w(4)) << CONST_BITS;
        let tmp10 = tmp0 + tmp3;
        let tmp13 = tmp0 - tmp3;
        let tmp11 = tmp1 + tmp2;
        let tmp12 = tmp1 - tmp2;
        let (mut t0, mut t1, mut t2, mut t3) = (w(7), w(5), w(3), w(1));
        let (mut z1, mut z2, mut z3, mut z4) = (t0 + t3, t1 + t2, t0 + t2, t1 + t3);
        let z5 = (z3 + z4) * f1_175;
        t0 *= f0_298;
        t1 *= f2_053;
        t2 *= f3_072;
        t3 *= f1_501;
        z1 *= -f0_899;
        z2 *= -f2_562;
        z3 *= -f1_961;
        z4 *= -f0_390;
        z3 += z5;
        z4 += z5;
        t0 += z1 + z3;
        t1 += z2 + z4;
        t2 += z2 + z3;
        t3 += z1 + z4;
        out[row * 8] = shift(tmp10 + t3);
        out[row * 8 + 7] = shift(tmp10 - t3);
        out[row * 8 + 1] = shift(tmp11 + t2);
        out[row * 8 + 6] = shift(tmp11 - t2);
        out[row * 8 + 2] = shift(tmp12 + t1);
        out[row * 8 + 5] = shift(tmp12 - t1);
        out[row * 8 + 3] = shift(tmp13 + t0);
        out[row * 8 + 4] = shift(tmp13 - t0);
    }
}

/// Fixed-point colour tables for JFIF YCbCr → RGB, indexed by the 8-bit Cb / Cr sample (scale 2^16).
struct ColorTables {
    cr_r: [i32; 256],
    cb_b: [i32; 256],
    cr_g: [i32; 256],
    cb_g: [i32; 256],
}

fn color_tables() -> ColorTables {
    const ONE_HALF: i64 = 1 << 15;
    let fix = |x: f64| (x * 65536.0 + 0.5) as i64;
    let mut t = ColorTables { cr_r: [0; 256], cb_b: [0; 256], cr_g: [0; 256], cb_g: [0; 256] };
    for i in 0..256usize {
        let x = i as i64 - 128;
        t.cr_r[i] = ((fix(1.40200) * x + ONE_HALF) >> 16) as i32;
        t.cb_b[i] = ((fix(1.77200) * x + ONE_HALF) >> 16) as i32;
        t.cr_g[i] = (-fix(0.71414) * x) as i32;
        t.cb_g[i] = (-fix(0.34414) * x + ONE_HALF) as i32;
    }
    t
}

fn output(f: &Frame, qt: &[[u16; 64]; 4], adobe: Option<u8>) -> Image {
    let ct = color_tables();
    let (cr_r, cb_b, cr_g, cb_g) = (&ct.cr_r, &ct.cb_b, &ct.cr_g, &ct.cb_g);
    // Component planes at full padded resolution scaled by sampling factor.
    let mut planes: Vec<(Vec<u8>, usize, usize)> = Vec::with_capacity(f.comps.len());
    for c in &f.comps {
        let pw = c.bpl * 8;
        let ph = c.bpc * 8;
        let mut plane = vec![0u8; pw * ph];
        let q = &qt[c.tq];
        let mut blk = [0u8; 64];
        for by in 0..c.bpc {
            for bx in 0..c.bpl {
                let b = (by * c.bpl + bx) * 64;
                idct_block(&c.coef[b..b + 64], q, &mut blk);
                for y in 0..8 {
                    let dst = (by * 8 + y) * pw + bx * 8;
                    plane[dst..dst + 8].copy_from_slice(&blk[y * 8..y * 8 + 8]);
                }
            }
        }
        planes.push((plane, pw, ph));
    }
    let (w, h) = (f.width, f.height);
    let mut rgb = vec![0u8; w * h * 3];
    let sample = |ci: usize, x: usize, y: usize| -> f64 {
        let c = &f.comps[ci];
        let (plane, pw, _) = &planes[ci];
        let sx = x * c.h / f.hmax;
        let sy = y * c.v / f.vmax;
        plane[sy * pw + sx] as f64
    };
    let rgb_direct = f.comps.len() == 3 && (adobe == Some(0) || (f.comps[0].id == b'R' && f.comps[1].id == b'G' && f.comps[2].id == b'B'));
    for y in 0..h {
        for x in 0..w {
            let o = (y * w + x) * 3;
            if f.comps.len() == 1 {
                let g = sample(0, x, y) as u8;
                rgb[o..o + 3].copy_from_slice(&[g, g, g]);
            } else if rgb_direct {
                rgb[o] = sample(0, x, y) as u8;
                rgb[o + 1] = sample(1, x, y) as u8;
                rgb[o + 2] = sample(2, x, y) as u8;
            } else {
                // JFIF YCbCr -> RGB in 16-bit fixed point (constants 1.40200, 0.34414, 0.71414, 1.77200; rounded).
                let yy = sample(0, x, y) as i32;
                let cb = sample(1, x, y) as usize;
                let cr = sample(2, x, y) as usize;
                let r = yy + cr_r[cr];
                let g = yy + ((cb_g[cb] + cr_g[cr]) >> 16);
                let bl = yy + cb_b[cb];
                rgb[o] = r.clamp(0, 255) as u8;
                rgb[o + 1] = g.clamp(0, 255) as u8;
                rgb[o + 2] = bl.clamp(0, 255) as u8;
            }
        }
    }
    Image { width: w, height: h, rgb }
}
