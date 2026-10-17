//! Brotli decoder (RFC 7932): stream and meta-block headers, prefix codes, block switching, context modelling,
//! insert-and-copy commands, distance ring buffer, static dictionary references with the 121 transforms.
//!
//! Spec: https://www.rfc-editor.org/rfc/rfc7932.txt — §3 prefix codes (3.1 bit order, 3.2 canonical, 3.4 simple,
//! 3.5 complex), §4 distances, §5 insert-and-copy lengths, §6 block switching, §7 context modelling (Lut0/1/2),
//! §8 static dictionary, §9 format, §10 decoding algorithm, Appendix A/B tables (see `brotli_tables.rs`, generated
//! by `tools/gen_brotli_tables.py` from the RFC text with the RFC's own CRC checks). Written from the RFC text.
//!
//! Malformed input returns `Err`; reads are bounds-checked. The whole output is kept in memory, capped by the
//! caller (`decompress_bounded`).

use crate::brotli_tables::{LUT0, LUT1, LUT2, NDBITS, TRANSFORMS};

static DICT: &[u8] = include_bytes!("brotli_dict.bin");

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Truncated,
    BadStreamHeader,
    BadMetaBlock(&'static str),
    BadPrefixCode(&'static str),
    BadSymbol(&'static str),
    BadDistance,
    BadDictionary,
    /// Output would exceed the caller's limit.
    OutputLimit,
}

pub type Result<T> = core::result::Result<T, Error>;

// ---- bit reader (RFC 7932 §3.1: integers LSB-first, prefix codes MSB-first of the code) --------------------------

struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    bit: u64,
    nbits: u32,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Bits { data, pos: 0, bit: 0, nbits: 0 }
    }

    fn fill(&mut self, n: u32) -> Result<()> {
        while self.nbits < n {
            let b = *self.data.get(self.pos).ok_or(Error::Truncated)?;
            self.pos += 1;
            self.bit |= (b as u64) << self.nbits;
            self.nbits += 8;
        }
        Ok(())
    }

    /// Integer value of the next `n` bits, first bit is the least significant (n <= 24).
    fn read(&mut self, n: u32) -> Result<u32> {
        if n == 0 {
            return Ok(0);
        }
        self.fill(n)?;
        let v = (self.bit & ((1u64 << n) - 1)) as u32;
        self.bit >>= n;
        self.nbits -= n;
        Ok(v)
    }

    fn read_bit(&mut self) -> Result<u32> {
        self.read(1)
    }

    /// Skip to the next byte boundary; returns Err if the skipped bits are not zero (§9.2 fill bits).
    fn align_zero(&mut self) -> Result<()> {
        let r = self.nbits % 8;
        if r != 0 && self.read(r)? != 0 {
            return Err(Error::BadMetaBlock("non-zero fill bits"));
        }
        Ok(())
    }

    /// Bytes still unread in whole, after dropping partial bits.
    fn read_bytes(&mut self, n: usize, out: &mut Vec<u8>, max: usize) -> Result<()> {
        if out.len() + n > max {
            return Err(Error::OutputLimit);
        }
        for _ in 0..n {
            out.push(self.read(8)? as u8);
        }
        Ok(())
    }
}

// ---- prefix codes (§3.2 canonical; §3.4 simple; §3.5 complex) -------------------------------------------------

#[derive(Clone)]
enum Code {
    /// One symbol with a zero-length code: decoding reads no bits (§3.4, §3.5).
    Single(u16),
    Tree { count: [u16; 16], symbol: Vec<u16> },
}

impl Code {
    /// Canonical code from (symbol, length) pairs with non-zero lengths (§3.2). Requires a complete code.
    fn from_lengths(lengths: &[u8]) -> Result<Code> {
        let mut count = [0u16; 16];
        for &l in lengths {
            count[l as usize] += 1;
        }
        count[0] = 0;
        let nonzero: usize = count.iter().map(|&c| c as usize).sum();
        if nonzero == 0 {
            return Err(Error::BadPrefixCode("no symbols"));
        }
        // Kraft sum must be exactly 1 (complete code) when there are two or more symbols.
        let mut space: i64 = 1 << 15;
        for len in 1..16 {
            space -= (count[len] as i64) << (15 - len);
        }
        if space != 0 {
            return Err(Error::BadPrefixCode("incomplete or over-subscribed code"));
        }
        let mut offs = [0u16; 16];
        for len in 1..15 {
            offs[len + 1] = offs[len] + count[len];
        }
        let mut symbol = vec![0u16; nonzero];
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbol[offs[l as usize] as usize] = sym as u16;
                offs[l as usize] += 1;
            }
        }
        Ok(Code::Tree { count, symbol })
    }

    fn decode(&self, bits: &mut Bits) -> Result<u16> {
        match self {
            Code::Single(s) => Ok(*s),
            Code::Tree { count, symbol } => {
                let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
                for len in 1..16 {
                    code |= bits.read_bit()? as i32;
                    let c = count[len] as i32;
                    if code - c < first {
                        return Ok(symbol[(index + (code - first)) as usize]);
                    }
                    index += c;
                    first += c;
                    first <<= 1;
                    code <<= 1;
                }
                Err(Error::BadPrefixCode("no code matches"))
            }
        }
    }
}

/// Smallest number of bits that represents every symbol of an alphabet of `size` (§3.4 ALPHABET_BITS).
fn alphabet_bits(size: usize) -> u32 {
    let mut b = 0;
    while (1usize << b) < size {
        b += 1;
    }
    b
}

fn read_prefix_code(bits: &mut Bits, size: usize) -> Result<Code> {
    let kind = bits.read(2)?;
    if kind == 1 {
        return simple_code(bits, size);
    }
    complex_code(bits, size, kind as usize)
}

/// Simple prefix code, §3.4.
fn simple_code(bits: &mut Bits, size: usize) -> Result<Code> {
    let nsym = bits.read(2)? as usize + 1;
    let ab = alphabet_bits(size);
    let mut syms = Vec::with_capacity(nsym);
    for _ in 0..nsym {
        let s = bits.read(ab)? as usize;
        if s >= size {
            return Err(Error::BadPrefixCode("simple symbol out of alphabet"));
        }
        if syms.contains(&(s as u16)) {
            return Err(Error::BadPrefixCode("repeated simple symbol"));
        }
        syms.push(s as u16);
    }
    let lens: Vec<u8> = match nsym {
        1 => return Ok(Code::Single(syms[0])),
        2 => vec![1, 1],
        3 => vec![1, 2, 2],
        _ => {
            if bits.read_bit()? == 0 {
                vec![2, 2, 2, 2]
            } else {
                vec![1, 2, 3, 3]
            }
        }
    };
    let mut lengths = vec![0u8; size];
    for (s, l) in syms.iter().zip(lens) {
        lengths[*s as usize] = l;
    }
    Code::from_lengths(&lengths)
}

/// Code-length-code symbol bits, read with the fixed variable-length code of §3.5 (bits in stream order).
fn read_clen_code_length(bits: &mut Bits) -> Result<u8> {
    // Codes as read from the stream, first bit first: 0:"00" 1:"1110" 2:"110" 3:"01" 4:"10" 5:"1111".
    let mut v = 0u32;
    for len in 1..=4u32 {
        v = (v << 1) | bits.read_bit()?;
        let sym = match (len, v) {
            (2, 0b00) => 0,
            (2, 0b01) => 3,
            (2, 0b10) => 4,
            (3, 0b110) => 2,
            (4, 0b1110) => 1,
            (4, 0b1111) => 5,
            _ => continue,
        };
        return Ok(sym);
    }
    Err(Error::BadPrefixCode("code length code"))
}

/// Order in which the code length code lengths are stored, §3.5.
const CLEN_ORDER: [usize; 18] = [1, 2, 3, 4, 0, 5, 17, 6, 16, 7, 8, 9, 10, 11, 12, 13, 14, 15];

/// Complex prefix code, §3.5. `hskip` is HSKIP (0, 2 or 3).
fn complex_code(bits: &mut Bits, size: usize, hskip: usize) -> Result<Code> {
    if hskip == 1 {
        return Err(Error::BadPrefixCode("HSKIP 1 is a simple code"));
    }
    let mut clen = [0u8; 18];
    let mut space: i32 = 32;
    let mut nonzero = 0;
    for &idx in CLEN_ORDER.iter().skip(hskip) {
        let l = read_clen_code_length(bits)?;
        clen[idx] = l;
        if l != 0 {
            space -= 32 >> l;
            nonzero += 1;
            if space <= 0 {
                break;
            }
        }
    }
    let cl_code = if nonzero == 1 {
        let s = clen.iter().position(|&l| l != 0).unwrap() as u16;
        Code::Single(s)
    } else {
        if space != 0 {
            return Err(Error::BadPrefixCode("code length code not complete"));
        }
        Code::from_lengths(&clen)?
    };

    // Code lengths of the alphabet itself.
    let mut lengths = vec![0u8; size];
    let mut i = 0usize;
    let mut space: i64 = 32768;
    let mut prev_nonzero: u8 = 8;
    let mut last: Option<(u16, usize)> = None; // (16 or 17, repeat count of the last repeat code)
    while space > 0 {
        if i >= size {
            return Err(Error::BadPrefixCode("code lengths overflow alphabet"));
        }
        let sym = cl_code.decode(bits)?;
        match sym {
            0..=15 => {
                lengths[i] = sym as u8;
                i += 1;
                last = None;
                if sym != 0 {
                    prev_nonzero = sym as u8;
                    space -= 32768 >> sym;
                }
            }
            16 | 17 => {
                let (fill, extra_bits, base) = if sym == 16 { (prev_nonzero, 2, 3usize) } else { (0u8, 3, 3usize) };
                let extra = bits.read(extra_bits)? as usize + base;
                let (old, prev_count) = match last {
                    Some((s, c)) if s == sym => (c, c),
                    _ => (0, 0),
                };
                // Repeat counts: 16 -> 4*(old-2) + extra, 17 -> 8*(old-2) + extra, when following the same code.
                let total = if prev_count > 0 {
                    let mult = if sym == 16 { 4 } else { 8 };
                    mult * (old - 2) + extra
                } else {
                    extra
                };
                let add = total - prev_count;
                if i + add > size {
                    return Err(Error::BadPrefixCode("repeat overflows alphabet"));
                }
                for k in 0..add {
                    lengths[i + k] = fill;
                }
                if sym == 16 {
                    space -= (add as i64) * (32768 >> fill);
                }
                i += add;
                last = Some((sym, total));
            }
            _ => return Err(Error::BadPrefixCode("code length symbol")),
        }
    }
    if space != 0 {
        return Err(Error::BadPrefixCode("code lengths not complete"));
    }
    Code::from_lengths(&lengths)
}

// ---- variable-length integers used in the meta-block header (§9.2) ---------------------------------------------

/// NBLTYPES / NTREES style value, 1..=256.
fn read_var_count(bits: &mut Bits) -> Result<usize> {
    if bits.read_bit()? == 0 {
        return Ok(1);
    }
    let n = bits.read(3)?;
    if n == 0 {
        return Ok(2);
    }
    let extra = bits.read(n)?;
    Ok(((1usize << n) + extra as usize) + 1)
}

/// Block count code + extra bits, §6 table.
const BLOCK_LEN_BASE: [(u32, u32); 26] = [
    (1, 2), (5, 2), (9, 2), (13, 2), (17, 3), (25, 3), (33, 3), (41, 3), (49, 4), (65, 4), (81, 4), (97, 4), (113, 5),
    (145, 5), (177, 5), (209, 5), (241, 6), (305, 6), (369, 7), (497, 8), (753, 9), (1265, 10), (2289, 11), (4337, 12),
    (8433, 13), (16625, 24),
];

fn read_block_count(bits: &mut Bits, code: &Code) -> Result<u32> {
    let c = code.decode(bits)? as usize;
    let (base, extra) = *BLOCK_LEN_BASE.get(c).ok_or(Error::BadSymbol("block count code"))?;
    Ok(base + bits.read(extra)?)
}

/// Block category state for literal, insert-and-copy, or distance block types (§6, §10).
struct BlockState {
    ntypes: usize,
    type_code: Option<Code>,
    count_code: Option<Code>,
    cur: usize,
    prev: usize,
    remaining: u32,
}

impl BlockState {
    fn new(bits: &mut Bits, ntypes: usize) -> Result<BlockState> {
        if ntypes >= 2 {
            let type_code = read_prefix_code(bits, ntypes + 2)?;
            let count_code = read_prefix_code(bits, 26)?;
            let remaining = read_block_count(bits, &count_code)?;
            Ok(BlockState { ntypes, type_code: Some(type_code), count_code: Some(count_code), cur: 0, prev: 1, remaining })
        } else {
            Ok(BlockState { ntypes, type_code: None, count_code: None, cur: 0, prev: 1, remaining: 1 << 24 })
        }
    }

    /// Advance to the next block when the current one is used up (§6 block type codes 0, 1, 2..).
    fn next_block(&mut self, bits: &mut Bits) -> Result<()> {
        if self.remaining != 0 {
            return Ok(());
        }
        let tc = self.type_code.as_ref().expect("ntypes >= 2");
        let sym = tc.decode(bits)? as usize;
        let new = match sym {
            0 => self.prev,
            1 => (self.cur + 1) % self.ntypes,
            s => s - 2,
        };
        if new >= self.ntypes {
            return Err(Error::BadSymbol("block type"));
        }
        self.prev = self.cur;
        self.cur = new;
        self.remaining = read_block_count(bits, self.count_code.as_ref().expect("ntypes >= 2"))?;
        Ok(())
    }

    /// Take one item from the current block (after switching if it is used up).
    fn take(&mut self, bits: &mut Bits) -> Result<usize> {
        self.next_block(bits)?;
        self.remaining -= 1;
        Ok(self.cur)
    }
}

/// Context map (§7.3): `size` entries with values below `ntrees`.
fn read_context_map(bits: &mut Bits, size: usize, ntrees: usize) -> Result<Vec<u8>> {
    let rlemax = if bits.read_bit()? == 0 { 0 } else { bits.read(4)? as usize + 1 };
    let code = read_prefix_code(bits, ntrees + rlemax)?;
    let mut map: Vec<u8> = Vec::with_capacity(size);
    while map.len() < size {
        let sym = code.decode(bits)? as usize;
        if sym == 0 {
            map.push(0);
        } else if sym <= rlemax {
            let run = (1usize << sym) + bits.read(sym as u32)? as usize;
            if map.len() + run > size {
                return Err(Error::BadMetaBlock("context map run overflow"));
            }
            map.extend(std::iter::repeat_n(0u8, run));
        } else {
            let v = sym - rlemax;
            if v >= ntrees {
                return Err(Error::BadSymbol("context map value"));
            }
            map.push(v as u8);
        }
    }
    if bits.read_bit()? == 1 {
        inverse_move_to_front(&mut map);
    }
    Ok(map)
}

/// Inverse move-to-front transform, §7.3.
fn inverse_move_to_front(v: &mut [u8]) {
    let mut mtf: [u8; 256] = core::array::from_fn(|i| i as u8);
    for x in v.iter_mut() {
        let index = *x as usize;
        let value = mtf[index];
        *x = value;
        mtf.copy_within(0..index, 1);
        mtf[0] = value;
    }
}

// ---- insert-and-copy lengths (§5) ----------------------------------------------------------------------------

const INSERT_BASE: [(u32, u32); 24] = [
    (0, 0), (1, 0), (2, 0), (3, 0), (4, 0), (5, 0), (6, 1), (8, 1), (10, 2), (14, 2), (18, 3), (26, 3), (34, 4), (50, 4),
    (66, 5), (98, 5), (130, 6), (194, 7), (322, 8), (578, 9), (1090, 10), (2114, 12), (6210, 14), (22594, 24),
];
const COPY_BASE: [(u32, u32); 24] = [
    (2, 0), (3, 0), (4, 0), (5, 0), (6, 0), (7, 0), (8, 0), (9, 0), (10, 1), (12, 1), (14, 2), (18, 2), (22, 3), (30, 3),
    (38, 4), (54, 4), (70, 5), (102, 5), (134, 6), (198, 7), (326, 8), (582, 9), (1094, 10), (2118, 24),
];

/// Insert length, copy length, and whether the distance is implicit (cmd < 128), §5.
fn decode_cmd(bits: &mut Bits, cmd: u16) -> Result<(usize, usize, bool)> {
    let cmd = cmd as usize;
    let cell = cmd >> 6;
    const INS_GROUP: [usize; 11] = [0, 0, 0, 0, 8, 8, 0, 16, 8, 16, 16];
    const CPY_GROUP: [usize; 11] = [0, 8, 0, 8, 0, 8, 16, 0, 16, 8, 16];
    let ins_code = INS_GROUP[cell] + ((cmd >> 3) & 7);
    let cpy_code = CPY_GROUP[cell] + (cmd & 7);
    let (ib, ie) = INSERT_BASE[ins_code];
    let (cb, ce) = COPY_BASE[cpy_code];
    let ilen = ib as usize + bits.read(ie)? as usize;
    let clen = cb as usize + bits.read(ce)? as usize;
    Ok((ilen, clen, cell < 2))
}

// ---- distances (§4) ---------------------------------------------------------------------------------------------

/// Special distance codes 0..=15 as (ring index, delta), §4.
const SPECIAL_DIST: [(usize, i64); 16] = [
    (0, 0), (1, 0), (2, 0), (3, 0), (0, -1), (0, 1), (0, -2), (0, 2), (0, -3), (0, 3), (1, -1), (1, 1), (1, -2), (1, 2),
    (1, -3), (1, 3),
];

struct DistanceCtx {
    ring: [i64; 4], // ring[0] = last distance, ring[1] = second-to-last, ...
    npostfix: u32,
    ndirect: u32,
}

impl DistanceCtx {
    fn decode(&mut self, bits: &mut Bits, dcode: usize) -> Result<(i64, bool)> {
        let ndirect = self.ndirect as usize;
        if dcode < 16 {
            let (idx, delta) = SPECIAL_DIST[dcode];
            let d = self.ring[idx] + delta;
            if d <= 0 {
                return Err(Error::BadDistance);
            }
            return Ok((d, dcode != 0));
        }
        if dcode < 16 + ndirect {
            return Ok(((dcode - 15) as i64, true));
        }
        let np = self.npostfix;
        let x = dcode - ndirect - 16;
        let ndistbits = 1 + (x >> (np + 1)) as u32;
        let dextra = bits.read(ndistbits)? as i64;
        let hcode = (x >> np) as i64;
        let lcode = (x & ((1usize << np) - 1)) as i64;
        let offset = ((2 + (hcode & 1)) << ndistbits) - 4;
        let d = ((offset + dextra) << np) + lcode + ndirect as i64 + 1;
        Ok((d, true))
    }

    fn push(&mut self, d: i64) {
        self.ring = [d, self.ring[0], self.ring[1], self.ring[2]];
    }
}

// ---- transforms (§8, Appendix B) -------------------------------------------------------------------------------

/// Apply `FermentFirst` / `FermentAll` (§8), `Identity` and `Omit*` to `word`.
fn ferment(word: &mut [u8], pos: usize) -> usize {
    let len = word.len();
    let b = word[pos];
    if b < 192 {
        if (97..=122).contains(&b) {
            word[pos] ^= 32;
        }
        1
    } else if b < 224 {
        if pos + 1 < len {
            word[pos + 1] ^= 32;
        }
        2
    } else {
        if pos + 2 < len {
            word[pos + 2] ^= 5;
        }
        3
    }
}

fn transform(base: &[u8], id: usize) -> Vec<u8> {
    let (prefix, kind, suffix) = TRANSFORMS[id];
    let mut w: Vec<u8> = base.to_vec();
    match kind {
        0 => {}
        1 => {
            if !w.is_empty() {
                ferment(&mut w, 0);
            }
        }
        2 => {
            let mut i = 0;
            while i < w.len() {
                i += ferment(&mut w, i);
            }
        }
        3..=11 => {
            let k = (kind - 2) as usize;
            w = if w.len() < k { Vec::new() } else { w[k..].to_vec() };
        }
        _ => {
            let k = (kind - 11) as usize;
            w = if w.len() < k { Vec::new() } else { w[..w.len() - k].to_vec() };
        }
    }
    let mut out = Vec::with_capacity(prefix.len() + w.len() + suffix.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(&w);
    out.extend_from_slice(suffix);
    out
}

/// Static dictionary reference for copy length `len` and distance `dist` (§8).
fn dictionary_word(len: usize, dist: i64, max_distance: i64) -> Result<Vec<u8>> {
    if !(4..=24).contains(&len) {
        return Err(Error::BadDictionary);
    }
    // NDBITS[len] and DOFFSET from §8.
    let ndbits = NDBITS[len] as u32;
    let nwords = 1usize << ndbits;
    let mut doffset = 0usize;
    for l in 0..len {
        let nw = if l < 4 { 0 } else { 1usize << NDBITS[l] };
        doffset += l * nw;
    }
    let word_id = (dist - (max_distance + 1)) as usize;
    let index = word_id % nwords;
    let transform_id = word_id >> ndbits;
    if transform_id > 120 {
        return Err(Error::BadDictionary);
    }
    let start = doffset + index * len;
    let base = DICT.get(start..start + len).ok_or(Error::BadDictionary)?;
    Ok(transform(base, transform_id))
}

// ---- stream ---------------------------------------------------------------------------------------------------

/// WBITS from the variable-length code of §9.1.
fn read_wbits(bits: &mut Bits) -> Result<u32> {
    if bits.read_bit()? == 0 {
        return Ok(16);
    }
    let n = bits.read(3)?;
    if n != 0 {
        return Ok(17 + n);
    }
    let m = bits.read(3)?;
    match m {
        0 => Ok(17),
        1 => Err(Error::BadStreamHeader),
        _ => Ok(8 + m),
    }
}

/// Decompress a brotli stream with the default output cap (256 MiB).
pub fn decompress(data: &[u8]) -> Result<Vec<u8>> {
    decompress_bounded(data, 256 * 1024 * 1024)
}

pub fn decompress_bounded(data: &[u8], max_out: usize) -> Result<Vec<u8>> {
    let mut bits = Bits::new(data);
    let wbits = read_wbits(&mut bits)?;
    let window = (1i64 << wbits) - 16;
    let mut out: Vec<u8> = Vec::new();
    let mut ring = DistanceCtx { ring: [4, 11, 15, 16], npostfix: 0, ndirect: 0 };

    loop {
        let islast = bits.read_bit()? == 1;
        if islast && bits.read_bit()? == 1 {
            break; // ISLASTEMPTY
        }
        let mnibbles = match bits.read(2)? {
            3 => 0,
            v => v + 4,
        };
        if mnibbles == 0 {
            // Metadata meta-block (§9.2): reserved bit, MSKIPBYTES, MSKIPLEN, skipped bytes.
            if bits.read_bit()? != 0 {
                return Err(Error::BadMetaBlock("reserved bit"));
            }
            let mskipbytes = bits.read(2)?;
            let mut skip = 0usize;
            if mskipbytes > 0 {
                skip = bits.read(8 * mskipbytes)? as usize + 1;
            }
            bits.align_zero()?;
            let mut sink = Vec::new();
            bits.read_bytes(skip, &mut sink, usize::MAX)?;
            if islast {
                break;
            }
            continue;
        }
        let mlen = bits.read(4 * mnibbles)? as usize + 1;
        if mnibbles > 4 && mlen - 1 >> (4 * (mnibbles - 1)) == 0 {
            return Err(Error::BadMetaBlock("MLEN has a zero high nibble"));
        }
        if !islast && bits.read_bit()? == 1 {
            // Uncompressed meta-block (ISUNCOMPRESSED).
            bits.align_zero()?;
            bits.read_bytes(mlen, &mut out, max_out)?;
            if out.len() > max_out {
                return Err(Error::OutputLimit);
            }
            continue;
        }
        if out.len() + mlen > max_out {
            return Err(Error::OutputLimit);
        }
        let target = out.len() + mlen;

        // Block categories L (literal), I (insert-and-copy), D (distance).
        let nl = read_var_count(&mut bits)?;
        let mut bl = BlockState::new(&mut bits, nl)?;
        let ni = read_var_count(&mut bits)?;
        let mut bi = BlockState::new(&mut bits, ni)?;
        let nd = read_var_count(&mut bits)?;
        let mut bd = BlockState::new(&mut bits, nd)?;

        let npostfix = bits.read(2)?;
        let ndirect = bits.read(4)? << npostfix;
        ring.npostfix = npostfix;
        ring.ndirect = ndirect;

        let mut cmode = Vec::with_capacity(bl.ntypes);
        for _ in 0..bl.ntypes {
            cmode.push(bits.read(2)? as u8);
        }

        let ntreesl = read_var_count(&mut bits)?;
        let cmapl = if ntreesl >= 2 { read_context_map(&mut bits, 64 * bl.ntypes, ntreesl)? } else { vec![0u8; 64 * bl.ntypes] };
        let ntreesd = read_var_count(&mut bits)?;
        let cmapd = if ntreesd >= 2 { read_context_map(&mut bits, 4 * bd.ntypes, ntreesd)? } else { vec![0u8; 4 * bd.ntypes] };

        let mut lit_codes = Vec::with_capacity(ntreesl);
        for _ in 0..ntreesl {
            lit_codes.push(read_prefix_code(&mut bits, 256)?);
        }
        let mut ins_codes = Vec::with_capacity(bi.ntypes);
        for _ in 0..bi.ntypes {
            ins_codes.push(read_prefix_code(&mut bits, 704)?);
        }
        let dist_alphabet = 16 + ndirect as usize + (48usize << npostfix);
        let mut dist_codes = Vec::with_capacity(ntreesd);
        for _ in 0..ntreesd {
            dist_codes.push(read_prefix_code(&mut bits, dist_alphabet)?);
        }

        let mut produced = out.len();
        while produced < target {
            let it = bi.take(&mut bits)?;
            let cmd = ins_codes[it].decode(&mut bits)?;
            let (ilen, clen, implicit_dist) = decode_cmd(&mut bits, cmd)?;

            for _ in 0..ilen {
                let bt = bl.take(&mut bits)?;
                if produced >= target {
                    return Err(Error::BadMetaBlock("literals exceed MLEN"));
                }
                let p1 = out.last().copied().unwrap_or(0) as usize;
                let p2 = if out.len() >= 2 { out[out.len() - 2] as usize } else { 0 };
                let cid = match cmode[bt] {
                    0 => p1 & 0x3F,
                    1 => p1 >> 2,
                    2 => (LUT0[p1] | LUT1[p2]) as usize,
                    _ => ((LUT2[p1] as usize) << 3) | LUT2[p2] as usize,
                };
                let tree = cmapl[64 * bt + cid] as usize;
                let lit = lit_codes[tree].decode(&mut bits)?;
                out.push(lit as u8);
                produced += 1;
            }
            if produced >= target {
                break; // copy length of the last command is ignored (§9.3)
            }

            let dist = if implicit_dist {
                ring.ring[0]
            } else {
                let bd_cur = bd.take(&mut bits)?;
                let cid = match clen {
                    2 => 0,
                    3 => 1,
                    4 => 2,
                    _ => 3,
                };
                let tree = cmapd[4 * bd_cur + cid] as usize;
                let dcode = dist_codes[tree].decode(&mut bits)? as usize;
                let (d, push) = ring.decode(&mut bits, dcode)?;
                // Dictionary references are never pushed (§4); the check below happens after decode.
                if push && d <= window.min(produced as i64) {
                    ring.push(d);
                }
                d
            };
            let maxd = window.min(produced as i64);
            if dist > maxd {
                let word = dictionary_word(clen, dist, maxd)?;
                if produced + word.len() > target {
                    return Err(Error::BadMetaBlock("dictionary word exceeds MLEN"));
                }
                out.extend_from_slice(&word);
                produced += word.len();
            } else {
                if dist <= 0 {
                    return Err(Error::BadDistance);
                }
                if produced + clen > target {
                    return Err(Error::BadMetaBlock("copy exceeds MLEN"));
                }
                let start = produced - dist as usize;
                for k in 0..clen {
                    let b = out[start + k];
                    out.push(b);
                }
                produced += clen;
            }
        }
        if out.len() > max_out {
            return Err(Error::OutputLimit);
        }
        if islast {
            break;
        }
    }
    Ok(out)
}
