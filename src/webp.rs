//! WebP container and lossless (VP8L) decoder.
//!
//! Spec: "WebP Lossless Bitstream Specification" (Alakuijala, Google, 2023-03-09;
//! https://developers.google.com/speed/webp/docs/webp_lossless_bitstream_specification) — §3 RIFF header and the
//! VP8L signature 0x2f and 14-bit size fields, §4 transforms (4.1 predictor, 4.2 colour, 4.3 subtract green,
//! 4.4 colour indexing with pixel bundling), §5 image data roles and LZ77 with the 120-entry distance map and the
//! colour cache, §6 prefix codes (simple and normal code length codes, max_symbol), §7 structure. The RIFF container
//! layout follows the WebP container specification (RIFF 'WEBP', chunks padded to even length, optional VP8X).
//!
//! Lossy VP8 ('VP8 ' chunk) is NOT implemented and returns `Unsupported` (see docs/webp.md).
//! Output is RGBA8.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    BadRiff,
    NoImage,
    Truncated,
    BadSignature,
    BadVersion,
    BadHuffman(&'static str),
    BadSymbol(&'static str),
    BadLz77(&'static str),
    BadTransform(&'static str),
    Unsupported(&'static str),
    TooLarge,
}

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    /// RGBA8, row-major.
    pub rgba: Vec<u8>,
}

/// Upper bound on pixels (2^28), as for the PNG module.
pub const MAX_PIXELS: usize = 1 << 28;

/// Distance map for distance codes 1..=120 (§5.2.2): (x, y) offsets of neighbouring pixels.
const DISTANCE_MAP: [(i8, i8); 120] = [
    (0, 1), (1, 0), (1, 1), (-1, 1), (0, 2), (2, 0), (1, 2), (-1, 2), (2, 1), (-2, 1), (2, 2), (-2, 2), (0, 3), (3, 0),
    (1, 3), (-1, 3), (3, 1), (-3, 1), (2, 3), (-2, 3), (3, 2), (-3, 2), (0, 4), (4, 0), (1, 4), (-1, 4), (4, 1), (-4, 1),
    (3, 3), (-3, 3), (2, 4), (-2, 4), (4, 2), (-4, 2), (0, 5), (3, 4), (-3, 4), (4, 3), (-4, 3), (5, 0), (1, 5), (-1, 5),
    (5, 1), (-5, 1), (2, 5), (-2, 5), (5, 2), (-5, 2), (4, 4), (-4, 4), (3, 5), (-3, 5), (5, 3), (-5, 3), (0, 6), (6, 0),
    (1, 6), (-1, 6), (6, 1), (-6, 1), (2, 6), (-2, 6), (6, 2), (-6, 2), (4, 5), (-4, 5), (5, 4), (-5, 4), (3, 6), (-3, 6),
    (6, 3), (-6, 3), (0, 7), (7, 0), (1, 7), (-1, 7), (5, 5), (-5, 5), (7, 1), (-7, 1), (4, 6), (-4, 6), (6, 4), (-6, 4),
    (2, 7), (-2, 7), (7, 2), (-7, 2), (3, 7), (-3, 7), (7, 3), (-7, 3), (5, 6), (-5, 6), (6, 5), (-6, 5), (8, 0), (4, 7),
    (-4, 7), (7, 4), (-7, 4), (8, 1), (8, 2), (6, 6), (-6, 6), (8, 3), (5, 7), (-5, 7), (7, 5), (-7, 5), (8, 4), (6, 7),
    (-6, 7), (7, 6), (-7, 6), (8, 5), (7, 7), (-7, 7), (8, 6), (8, 7),
];

/// Order of the code length code lengths (§6.2.1).
const CODE_LENGTH_ORDER: [usize; 19] = [17, 18, 0, 1, 2, 3, 4, 5, 16, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];

// ---- bit reader (§1: LSB-first) ----------------------------------------------------------------------------

struct Bits<'a> {
    d: &'a [u8],
    pos: usize, // bit position
}

impl<'a> Bits<'a> {
    fn read(&mut self, n: u32) -> Result<u32> {
        let mut v = 0u32;
        for k in 0..n {
            let byte = *self.d.get(self.pos >> 3).ok_or(Error::Truncated)?;
            let bit = (byte >> (self.pos & 7)) & 1;
            v |= (bit as u32) << k;
            self.pos += 1;
        }
        Ok(v)
    }

    fn bit(&mut self) -> Result<u32> {
        self.read(1)
    }
}

// ---- prefix codes (§6) ---------------------------------------------------------------------------------------

/// A prefix code. A single-symbol code consumes no bits (§6.2.1).
#[derive(Clone)]
enum Code {
    Single(u16),
    Tree { count: [u16; 16], symbol: Vec<u16> },
}

impl Code {
    fn from_lengths(lengths: &[u8]) -> Result<Code> {
        let nonzero: Vec<usize> = (0..lengths.len()).filter(|&i| lengths[i] != 0).collect();
        match nonzero.len() {
            0 => return Ok(Code::Single(0)),
            1 => return Ok(Code::Single(nonzero[0] as u16)),
            _ => {}
        }
        let mut count = [0u16; 16];
        for &i in &nonzero {
            let l = lengths[i] as usize;
            if l > 15 {
                return Err(Error::BadHuffman("length over 15"));
            }
            count[l] += 1;
        }
        // Full binary tree: the sum of 2^-len must be exactly 1 (§6.2.1).
        let mut space: i64 = 1 << 15;
        for (len, &c) in count.iter().enumerate().skip(1) {
            space -= (c as i64) << (15 - len);
        }
        if space != 0 {
            return Err(Error::BadHuffman("not a full code"));
        }
        // Canonical order: by length, then symbol value (§6.2.1 / deflate-style canonical codes).
        let mut sorted = nonzero.clone();
        sorted.sort_by_key(|&i| (lengths[i], i));
        let symbol: Vec<u16> = sorted.iter().map(|&i| i as u16).collect();
        Ok(Code::Tree { count, symbol })
    }

    fn decode(&self, bits: &mut Bits) -> Result<u16> {
        match self {
            Code::Single(s) => Ok(*s),
            Code::Tree { count, symbol } => {
                let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
                for len in 1..16 {
                    code |= bits.bit()? as i32;
                    let c = count[len] as i32;
                    if code - c < first {
                        return Ok(symbol[(index + (code - first)) as usize]);
                    }
                    index += c;
                    first += c;
                    first <<= 1;
                    code <<= 1;
                }
                Err(Error::BadHuffman("no code matches"))
            }
        }
    }
}

fn read_huffman_code(bits: &mut Bits, alphabet: usize) -> Result<Code> {
    if bits.bit()? == 1 {
        // Simple code (§6.2.1): one or two symbols with length 1.
        let num = bits.bit()? as usize + 1;
        let first8 = bits.bit()?;
        let s0 = bits.read(1 + 7 * first8)? as usize;
        let mut symbols = vec![s0];
        if num == 2 {
            symbols.push(bits.read(8)? as usize);
        }
        let mut lengths = vec![0u8; alphabet];
        for &s in &symbols {
            if s >= alphabet {
                return Err(Error::BadHuffman("simple symbol out of alphabet"));
            }
            lengths[s] = 1;
        }
        return Code::from_lengths(&lengths);
    }
    // Normal code length code.
    let num_code_lengths = 4 + bits.read(4)? as usize;
    let mut cl = [0u8; 19];
    for &idx in CODE_LENGTH_ORDER.iter().take(num_code_lengths) {
        cl[idx] = bits.read(3)? as u8;
    }
    let cl_code = Code::from_lengths(&cl)?;
    let mut max_symbol = if bits.bit()? == 0 {
        alphabet
    } else {
        let length_nbits = 2 + 2 * bits.read(3)?;
        let ms = 2 + bits.read(length_nbits)? as usize;
        if ms > alphabet {
            return Err(Error::BadHuffman("max_symbol over alphabet"));
        }
        ms
    };
    let mut lengths = vec![0u8; alphabet];
    let mut i = 0usize;
    let mut prev: u8 = 8;
    while i < alphabet {
        if max_symbol == 0 {
            break;
        }
        max_symbol -= 1;
        let code = cl_code.decode(bits)? as usize;
        match code {
            0..=15 => {
                lengths[i] = code as u8;
                i += 1;
                if code != 0 {
                    prev = code as u8;
                }
            }
            16 => {
                let rep = 3 + bits.read(2)? as usize;
                if i + rep > alphabet {
                    return Err(Error::BadHuffman("repeat overflows alphabet"));
                }
                lengths[i..i + rep].fill(prev);
                i += rep;
            }
            17 => {
                let rep = 3 + bits.read(3)? as usize;
                if i + rep > alphabet {
                    return Err(Error::BadHuffman("zero run overflows alphabet"));
                }
                i += rep;
            }
            _ => {
                let rep = 11 + bits.read(7)? as usize;
                if i + rep > alphabet {
                    return Err(Error::BadHuffman("zero run overflows alphabet"));
                }
                i += rep;
            }
        }
    }
    Code::from_lengths(&lengths)
}

// ---- LZ77 value coding (§5.2.2) ----------------------------------------------------------------------------

fn lz77_value(bits: &mut Bits, prefix: u16) -> Result<usize> {
    let p = prefix as usize;
    if p < 4 {
        return Ok(p + 1);
    }
    let extra = (p - 2) >> 1;
    let offset = (2 + (p & 1)) << extra;
    Ok(offset + bits.read(extra as u32)? as usize + 1)
}

// ---- image stream (§5, §6.2.2, §6.2.3) -------------------------------------------------------------------------

#[inline]
fn argb(a: u8, r: u8, g: u8, b: u8) -> u32 {
    (a as u32) << 24 | (r as u32) << 16 | (g as u32) << 8 | b as u32
}

fn channels(p: u32) -> [u8; 4] {
    [(p >> 16) as u8, (p >> 8) as u8, p as u8, (p >> 24) as u8] // r, g, b, a
}

/// Decode an entropy-coded image of `w × h` (§7.3). `main` enables meta prefix codes (ARGB image only).
fn decode_image(bits: &mut Bits, w: usize, h: usize, main: bool) -> Result<Vec<u32>> {
    if w.checked_mul(h).map_or(true, |n| n > MAX_PIXELS) {
        return Err(Error::TooLarge);
    }
    // Colour cache (§5.2.3).
    let mut cache_bits = 0u32;
    if bits.bit()? == 1 {
        cache_bits = bits.read(4)?;
        if !(1..=11).contains(&cache_bits) {
            return Err(Error::BadSymbol("color cache bits"));
        }
    }
    let cache_size = if cache_bits > 0 { 1usize << cache_bits } else { 0 };
    // Meta prefix codes (§6.2.2): only for the main ARGB image.
    let mut meta: Option<(usize, usize, usize, Vec<u32>)> = None;
    if main && bits.bit()? == 1 {
        let prefix_bits = bits.read(3)? + 2;
        let ew = w.div_ceil(1 << prefix_bits);
        let eh = h.div_ceil(1 << prefix_bits);
        let entropy = decode_sub_image(bits, ew, eh)?;
        meta = Some((prefix_bits as usize, ew, eh, entropy));
    }
    let groups = match &meta {
        Some((_, _, _, e)) => e.iter().map(|&p| ((p >> 8) & 0xFFFF) as usize).max().unwrap_or(0) + 1,
        None => 1,
    };
    let mut codes: Vec<[Code; 5]> = Vec::with_capacity(groups);
    for _ in 0..groups {
        let green = read_huffman_code(bits, 256 + 24 + cache_size)?;
        let red = read_huffman_code(bits, 256)?;
        let blue = read_huffman_code(bits, 256)?;
        let alpha = read_huffman_code(bits, 256)?;
        let dist = read_huffman_code(bits, 40)?;
        codes.push([green, red, blue, alpha, dist]);
    }
    let mut cache = vec![0u32; cache_size];
    let mut out: Vec<u32> = Vec::with_capacity(w * h);
    let hash = |c: u32| -> usize { (c.wrapping_mul(0x1e35_a7bd) >> (32 - cache_bits)) as usize };
    while out.len() < w * h {
        let (x, y) = (out.len() % w, out.len() / w);
        let g = match &meta {
            Some((pb, ew, _, e)) => {
                let pos = (y >> pb) * ew + (x >> pb);
                ((e[pos] >> 8) & 0xFFFF) as usize
            }
            None => 0,
        };
        let c = &codes[g];
        let s = c[0].decode(bits)? as usize;
        if s < 256 {
            let r = c[1].decode(bits)? as u8;
            let b = c[2].decode(bits)? as u8;
            let a = c[3].decode(bits)? as u8;
            let px = argb(a, r, s as u8, b);
            out.push(px);
            if cache_bits > 0 {
                cache[hash(px)] = px;
            }
        } else if s < 256 + 24 {
            let len = lz77_value(bits, (s - 256) as u16)?;
            let dprefix = c[4].decode(bits)?;
            let dcode = lz77_value(bits, dprefix)?;
            let dist = if dcode > 120 {
                dcode - 120
            } else {
                let (xi, yi) = DISTANCE_MAP[dcode - 1];
                let d = xi as isize + yi as isize * w as isize;
                if d < 1 {
                    1
                } else {
                    d as usize
                }
            };
            if dist > out.len() {
                return Err(Error::BadLz77("distance before start"));
            }
            if out.len() + len > w * h {
                return Err(Error::BadLz77("copy past end of image"));
            }
            for _ in 0..len {
                let px = out[out.len() - dist];
                out.push(px);
                if cache_bits > 0 {
                    cache[hash(px)] = px;
                }
            }
        } else {
            let idx = s - 256 - 24;
            if idx >= cache_size {
                return Err(Error::BadSymbol("color cache index"));
            }
            let px = cache[idx];
            out.push(px);
            cache[hash(px)] = px;
        }
    }
    Ok(out)
}

/// A subresolution image (no transforms, no meta prefix codes), §5.1.
fn decode_sub_image(bits: &mut Bits, w: usize, h: usize) -> Result<Vec<u32>> {
    decode_image(bits, w, h, false)
}

// ---- transforms (§4) -------------------------------------------------------------------------------------------

enum Transform {
    Predictor { bits: u32, data: Vec<u32>, width: usize },
    Color { bits: u32, data: Vec<u32>, width: usize },
    SubtractGreen,
    ColorIndexing { table: Vec<u32>, width_bits: u32, width: usize },
}

fn avg2(a: u32, b: u32) -> u32 {
    let mut out = 0u32;
    for shift in [24u32, 16, 8, 0] {
        let x = ((a >> shift) & 0xFF) as u32;
        let y = ((b >> shift) & 0xFF) as u32;
        out |= ((x + y) / 2) << shift;
    }
    out
}

fn clamp255(v: i32) -> i32 {
    v.clamp(0, 255)
}

fn select(l: u32, t: u32, tl: u32) -> u32 {
    let mut p_l = 0i32;
    let mut p_t = 0i32;
    for shift in [24u32, 16, 8, 0] {
        let cl = ((l >> shift) & 0xFF) as i32;
        let ct = ((t >> shift) & 0xFF) as i32;
        let ctl = ((tl >> shift) & 0xFF) as i32;
        let est = cl + ct - ctl;
        p_l += (est - cl).abs();
        p_t += (est - ct).abs();
    }
    if p_l < p_t {
        l
    } else {
        t
    }
}

fn clamp_add_sub_full(a: u32, b: u32, c: u32) -> u32 {
    let mut out = 0u32;
    for shift in [24u32, 16, 8, 0] {
        let x = ((a >> shift) & 0xFF) as i32;
        let y = ((b >> shift) & 0xFF) as i32;
        let z = ((c >> shift) & 0xFF) as i32;
        out |= (clamp255(x + y - z) as u32) << shift;
    }
    out
}

fn clamp_add_sub_half(a: u32, b: u32) -> u32 {
    let mut out = 0u32;
    for shift in [24u32, 16, 8, 0] {
        let x = ((a >> shift) & 0xFF) as i32;
        let y = ((b >> shift) & 0xFF) as i32;
        out |= (clamp255(x + (x - y) / 2) as u32) << shift;
    }
    out
}

/// Predicted value for mode 0..=13 (§4.1). `tr` is the top-right pixel as defined by the border rules.
fn predict(mode: u32, l: u32, t: u32, tr: u32, tl: u32) -> u32 {
    match mode {
        0 => 0xFF00_0000,
        1 => l,
        2 => t,
        3 => tr,
        4 => tl,
        5 => avg2(avg2(l, tr), t),
        6 => avg2(l, tl),
        7 => avg2(l, t),
        8 => avg2(tl, t),
        9 => avg2(t, tr),
        10 => avg2(avg2(l, tl), avg2(t, tr)),
        11 => select(l, t, tl),
        12 => clamp_add_sub_full(l, t, tl),
        13 => clamp_add_sub_half(avg2(l, t), tl),
        _ => 0xFF00_0000, // modes 14 and 15 are not defined; treated as mode 0
    }
}

fn add_pixels(a: u32, b: u32) -> u32 {
    let mut out = 0u32;
    for shift in [24u32, 16, 8, 0] {
        out |= ((((a >> shift) & 0xFF) + ((b >> shift) & 0xFF)) & 0xFF) << shift;
    }
    out
}

fn inverse_predictor(data: &mut [u32], w: usize, h: usize, bits: u32, modes: &[u32], tw: usize) {
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let pred = if y == 0 && x == 0 {
                0xFF00_0000
            } else if y == 0 {
                data[i - 1]
            } else if x == 0 {
                data[i - w]
            } else {
                let mode = modes[(y >> bits) * tw + (x >> bits)];
                let l = data[i - 1];
                let t = data[i - w];
                let tl = data[i - w - 1];
                // Rightmost column: TR is the leftmost pixel of the current row (§4.1).
                let tr = if x + 1 < w { data[i - w + 1] } else { data[y * w] };
                predict(mode, l, t, tr, tl)
            };
            data[i] = add_pixels(data[i], pred);
        }
    }
}

fn color_delta(t: u8, c: u8) -> i32 {
    ((t as i8 as i32) * (c as i8 as i32)) >> 5
}

fn inverse_color(data: &mut [u32], w: usize, h: usize, bits: u32, elements: &[u32], tw: usize) {
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let e = elements[(y >> bits) * tw + (x >> bits)];
            let red_to_blue = (e >> 16) as u8;
            let green_to_blue = (e >> 8) as u8;
            let green_to_red = e as u8;
            let p = data[i];
            let a = (p >> 24) & 0xFF;
            let r = ((p >> 16) & 0xFF) as u8;
            let g = ((p >> 8) & 0xFF) as u8;
            let b = (p & 0xFF) as u8;
            let tmp_red = (r as i32 + color_delta(green_to_red, g)) & 0xFF;
            let tmp_blue = (b as i32 + color_delta(green_to_blue, g) + color_delta(red_to_blue, tmp_red as u8)) & 0xFF;
            data[i] = (a << 24) | ((tmp_red as u32) << 16) | ((g as u32) << 8) | tmp_blue as u32;
        }
    }
}

// ---- top level ---------------------------------------------------------------------------------------------------

/// Decode a WebP file (RIFF 'WEBP' with a 'VP8L' lossless chunk, optionally behind a VP8X header) to RGBA8.
pub fn decode(data: &[u8]) -> Result<Image> {
    if data.len() < 12 || &data[0..4] != b"RIFF" || &data[8..12] != b"WEBP" {
        return Err(Error::BadRiff);
    }
    let mut pos = 12usize;
    let mut vp8l: Option<&[u8]> = None;
    while pos + 8 <= data.len() {
        let tag = &data[pos..pos + 4];
        let len = u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]) as usize;
        let body = data.get(pos + 8..pos + 8 + len).ok_or(Error::Truncated)?;
        match tag {
            b"VP8L" => {
                vp8l = Some(body);
                break;
            }
            b"VP8 " => return Err(Error::Unsupported("lossy VP8 (not implemented)")),
            _ => {}
        }
        pos += 8 + len + (len & 1);
    }
    let body = vp8l.ok_or(Error::NoImage)?;
    decode_vp8l(body)
}

/// Decode a bare VP8L bitstream (the payload of a 'VP8L' chunk).
pub fn decode_vp8l(body: &[u8]) -> Result<Image> {
    let mut bits = Bits { d: body, pos: 0 };
    if bits.read(8)? != 0x2F {
        return Err(Error::BadSignature);
    }
    let width = bits.read(14)? as usize + 1;
    let height = bits.read(14)? as usize + 1;
    let _alpha_is_used = bits.bit()?;
    if bits.read(3)? != 0 {
        return Err(Error::BadVersion);
    }
    if width.checked_mul(height).map_or(true, |n| n > MAX_PIXELS) {
        return Err(Error::TooLarge);
    }

    // Transforms are read in order and inverted in reverse (§4). Each may appear once.
    let mut transforms: Vec<Transform> = Vec::new();
    let mut seen = [false; 4];
    let mut xsize = width;
    while bits.bit()? == 1 {
        let t = bits.read(2)? as usize;
        if seen[t] {
            return Err(Error::BadTransform("transform used twice"));
        }
        seen[t] = true;
        match t {
            0 | 1 => {
                let size_bits = bits.read(3)? + 2;
                let tw = xsize.div_ceil(1 << size_bits);
                let th = height.div_ceil(1 << size_bits);
                let data = decode_sub_image(&mut bits, tw, th)?;
                transforms.push(if t == 0 {
                    Transform::Predictor { bits: size_bits, data, width: tw }
                } else {
                    Transform::Color { bits: size_bits, data, width: tw }
                });
            }
            2 => transforms.push(Transform::SubtractGreen),
            _ => {
                let size = bits.read(8)? as usize + 1;
                let mut table = decode_sub_image(&mut bits, size, 1)?;
                // Entries are stored as deltas per channel (§4.4).
                for i in 1..table.len() {
                    table[i] = add_pixels(table[i], table[i - 1]);
                }
                let width_bits = match size {
                    0..=2 => 3,
                    3..=4 => 2,
                    5..=16 => 1,
                    _ => 0,
                };
                transforms.push(Transform::ColorIndexing { table, width_bits, width: xsize });
                xsize = xsize.div_ceil(1 << width_bits);
            }
        }
    }
    let mut pixels = decode_image(&mut bits, xsize, height, true)?;

    for t in transforms.iter().rev() {
        match t {
            Transform::Predictor { bits: b, data, width: tw } => {
                let w = xsize_of(&pixels, height);
                inverse_predictor(&mut pixels, w, height, *b, &channels_to_modes(data), *tw);
            }
            Transform::Color { bits: b, data, width: tw } => {
                let w = xsize_of(&pixels, height);
                inverse_color(&mut pixels, w, height, *b, data, *tw);
            }
            Transform::SubtractGreen => {
                for p in pixels.iter_mut() {
                    let g = (*p >> 8) & 0xFF;
                    let r = (((*p >> 16) & 0xFF) + g) & 0xFF;
                    let b = ((*p & 0xFF) + g) & 0xFF;
                    *p = (*p & 0xFF00_FF00) | (r << 16) | b;
                }
            }
            Transform::ColorIndexing { table, width_bits, width } => {
                let packed_w = xsize_of(&pixels, height);
                let mut out = vec![0u32; width * height];
                let wb = *width_bits;
                let per = 8u32 >> wb;
                let mask = (1u32 << per) - 1;
                for y in 0..height {
                    for x in 0..*width {
                        let packed = pixels[y * packed_w + (x >> wb)];
                        let shift = (x & ((1 << wb) - 1)) as u32 * per;
                        let idx = ((packed >> 8) & 0xFF) >> shift & mask;
                        let idx = idx as usize;
                        out[y * width + x] = if idx < table.len() { table[idx] } else { 0 };
                    }
                }
                pixels = out;
            }
        }
    }

    let mut rgba = Vec::with_capacity(width * height * 4);
    for p in &pixels {
        let c = channels(*p);
        rgba.extend_from_slice(&[c[0], c[1], c[2], c[3]]);
    }
    Ok(Image { width, height, rgba })
}

fn xsize_of(pixels: &[u32], height: usize) -> usize {
    pixels.len() / height
}

fn channels_to_modes(data: &[u32]) -> Vec<u32> {
    data.iter().map(|&p| (p >> 8) & 0xFF).collect()
}
