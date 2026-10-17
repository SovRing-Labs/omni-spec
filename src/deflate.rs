//! DEFLATE compression (RFC 1951 writer) and the zlib wrapper (RFC 1950).
//!
//! Spec: RFC 1951 §3.1.1 (LSB-first bit packing; Huffman codes packed MSB-first), §3.2.3 (BFINAL/BTYPE), §3.2.4 (stored blocks),
//! §3.2.5 (length/distance codes + extra bits), §3.2.6 (fixed Huffman), §3.2.7 (dynamic Huffman: HLIT/HDIST/HCLEN, code-length
//! alphabet 0-18 with the repeat codes 16/17/18), §3.2.2 (canonical code assignment). RFC 1950 §2.2 for the zlib header/Adler-32.
//!
//! Encoder: LZ77 over a 32 KiB window with 3-byte hash chains and one-step lazy matching; tokens are cut into blocks of
//! `BLOCK_TOKENS`; each block is written as whichever of stored / fixed / dynamic is smallest by exact bit count. Code lengths are
//! length-limited (15 bits, 7 for the code-length alphabet) with the package-merge algorithm (Larmore & Hirschberg, 1990).
//! Output is deterministic. Round trip is checked against `inflate.rs` and against Info-ZIP / Python zlib (docs/deflate.md).
//!
//! Patent / provenance: RFC 1951 is an IETF RFC with no known patent claims; no third-party deflate source was read.

use crate::inflate::adler32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Dynamic Huffman code lengths violate the canonical assignment (§3.2.2 / §3.2.7).
    BadCode(&'static str),
    /// Output would exceed the caller's bound (mirror of inflate.rs Error::OutputLimit).
    OutputLimit,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

const WSIZE: usize = 32768;
const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;
const MAX_CHAIN: usize = 128;
const LAZY_BELOW: usize = 32;
const BLOCK_TOKENS: usize = 16384;
const HASH_BITS: u32 = 15;

// §3.2.5 tables: length symbols 257..=285 and distance symbols 0..=29.
const LEN_BASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEN_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289,
    16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];
/// §3.2.7 order in which the code-length code lengths are transmitted.
const CLEN_ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

fn len_symbol(len: usize) -> usize {
    let mut s = 28;
    while LEN_BASE[s] as usize > len {
        s -= 1;
    }
    s
}

fn dist_symbol(dist: usize) -> usize {
    let mut s = 29;
    while DIST_BASE[s] as usize > dist {
        s -= 1;
    }
    s
}

// ---- bit writer (§3.1.1: bits fill each byte from the least significant end) ----

struct BitWriter {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl BitWriter {
    fn new() -> BitWriter {
        BitWriter { out: Vec::new(), acc: 0, n: 0 }
    }
    fn put(&mut self, bits: u32, count: u32) {
        self.acc |= (bits as u64) << self.n;
        self.n += count;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }
    fn align(&mut self) {
        if self.n > 0 {
            self.out.push(self.acc as u8);
            self.acc = 0;
            self.n = 0;
        }
    }
}

// ---- Huffman: length-limited code lengths (package-merge) and canonical codes ----

/// Optimal prefix-code lengths for `freq` with no length above `max` (package-merge). Unused symbols get 0; a single used
/// symbol gets length 1 (RFC 1951 allows that incomplete code for the literal and distance trees).
fn code_lengths(freq: &[u32], max: u32) -> Vec<u8> {
    let mut lens = vec![0u8; freq.len()];
    let mut syms: Vec<usize> = (0..freq.len()).filter(|&i| freq[i] > 0).collect();
    syms.sort_by_key(|&i| (freq[i], i));
    let n = syms.len();
    if n == 0 {
        return lens;
    }
    if n == 1 {
        lens[syms[0]] = 1;
        return lens;
    }
    debug_assert!(n <= 1usize << max);
    // an item is a weight plus how many times each leaf (by rank in `syms`) is inside it
    type Item = (u64, Vec<u16>);
    let leaf = |k: usize| -> Item {
        let mut c = vec![0u16; n];
        c[k] = 1;
        (freq[syms[k]] as u64, c)
    };
    let leaves: Vec<Item> = (0..n).map(leaf).collect();
    let mut cur: Vec<Item> = leaves.clone();
    for _ in 1..max {
        let mut packages: Vec<Item> = Vec::with_capacity(cur.len() / 2);
        for pair in cur.chunks_exact(2) {
            let mut c = pair[0].1.clone();
            for (a, b) in c.iter_mut().zip(&pair[1].1) {
                *a += *b;
            }
            packages.push((pair[0].0 + pair[1].0, c));
        }
        // merge the leaf list with the packages, leaves first on equal weight
        let (mut i, mut j) = (0, 0);
        let mut next: Vec<Item> = Vec::with_capacity(leaves.len() + packages.len());
        while i < leaves.len() || j < packages.len() {
            if j >= packages.len() || (i < leaves.len() && leaves[i].0 <= packages[j].0) {
                next.push(leaves[i].clone());
                i += 1;
            } else {
                next.push(packages[j].clone());
                j += 1;
            }
        }
        cur = next;
    }
    for item in cur.iter().take(2 * n - 2) {
        for (k, &c) in item.1.iter().enumerate() {
            lens[syms[k]] += c as u8;
        }
    }
    lens
}

/// §3.2.2 canonical codes, already bit-reversed so they can be written with `BitWriter::put` (Huffman codes go MSB first).
fn canonical_codes(lens: &[u8]) -> Vec<u32> {
    let mut bl_count = [0u32; 16];
    for &l in lens {
        bl_count[l as usize] += 1;
    }
    bl_count[0] = 0;
    let mut next = [0u32; 16];
    let mut code = 0u32;
    for bits in 1..16 {
        code = (code + bl_count[bits - 1]) << 1;
        next[bits] = code;
    }
    lens.iter()
        .map(|&l| {
            if l == 0 {
                return 0;
            }
            let c = next[l as usize];
            next[l as usize] += 1;
            c.reverse_bits() >> (32 - l as u32)
        })
        .collect()
}

fn fixed_lit_lens() -> Vec<u8> {
    let mut v = vec![8u8; 288];
    v[144..256].fill(9);
    v[256..280].fill(7);
    v
}

// ---- LZ77 ----

#[derive(Clone, Copy)]
struct Token(u32); // bit 31 set = match: (len-3) << 16 | (dist-1); else a literal byte

impl Token {
    fn lit(b: u8) -> Token {
        Token(b as u32)
    }
    fn mat(len: usize, dist: usize) -> Token {
        Token(0x8000_0000 | (((len - MIN_MATCH) as u32) << 16) | (dist as u32 - 1))
    }
}

struct Matcher<'a> {
    data: &'a [u8],
    head: Vec<u32>, // hash -> position + 1 (0 = empty)
    prev: Vec<u32>, // position & (WSIZE-1) -> previous position + 1 with the same hash
    inserted: usize,
}

impl<'a> Matcher<'a> {
    fn new(data: &'a [u8]) -> Matcher<'a> {
        Matcher { data, head: vec![0; 1 << HASH_BITS], prev: vec![0; WSIZE], inserted: 0 }
    }
    fn hash(&self, p: usize) -> usize {
        let v = self.data[p] as u32 | (self.data[p + 1] as u32) << 8 | (self.data[p + 2] as u32) << 16;
        (v.wrapping_mul(0x9E37_79B1) >> (32 - HASH_BITS)) as usize
    }
    /// Make every position below `upto` findable.
    fn insert_to(&mut self, upto: usize) {
        while self.inserted < upto {
            let p = self.inserted;
            if p + MIN_MATCH <= self.data.len() {
                let h = self.hash(p);
                self.prev[p & (WSIZE - 1)] = self.head[h];
                self.head[h] = p as u32 + 1;
            }
            self.inserted += 1;
        }
    }
    /// Longest earlier match for `pos` (all positions below `pos` must already be inserted): (len, dist), len 0 if none.
    fn find(&self, pos: usize) -> (usize, usize) {
        let d = self.data;
        if pos + MIN_MATCH > d.len() {
            return (0, 0);
        }
        let max_len = MAX_MATCH.min(d.len() - pos);
        let (mut best_len, mut best_dist) = (MIN_MATCH - 1, 0);
        let mut cand = self.head[self.hash(pos)] as usize;
        let mut chain = MAX_CHAIN;
        while cand != 0 && chain > 0 {
            let c = cand - 1;
            if c >= pos || pos - c > WSIZE {
                break;
            }
            let probe = best_len.min(max_len - 1);
            if d[c + probe] == d[pos + probe] {
                let mut l = 0;
                while l < max_len && d[c + l] == d[pos + l] {
                    l += 1;
                }
                if l > best_len {
                    best_len = l;
                    best_dist = pos - c;
                    if l == max_len {
                        break;
                    }
                }
            }
            let next = self.prev[c & (WSIZE - 1)] as usize;
            if next == 0 || next - 1 >= c {
                break;
            }
            cand = next;
            chain -= 1;
        }
        if best_dist == 0 { (0, 0) } else { (best_len, best_dist) }
    }
}

// ---- block writers ----

struct BlockStats {
    lit_freq: Vec<u32>,  // 286 literal/length symbols (256 = end of block)
    dist_freq: Vec<u32>, // 30 distance symbols
    extra_bits: u64,     // extra bits of length + distance codes
}

fn stats(tokens: &[Token]) -> BlockStats {
    let mut s = BlockStats { lit_freq: vec![0; 286], dist_freq: vec![0; 30], extra_bits: 0 };
    for t in tokens {
        if t.0 & 0x8000_0000 == 0 {
            s.lit_freq[t.0 as usize] += 1;
        } else {
            let len = ((t.0 >> 16) & 0xFF) as usize + MIN_MATCH;
            let dist = (t.0 & 0xFFFF) as usize + 1;
            let (ls, ds) = (len_symbol(len), dist_symbol(dist));
            s.lit_freq[257 + ls] += 1;
            s.dist_freq[ds] += 1;
            s.extra_bits += LEN_EXTRA[ls] as u64 + DIST_EXTRA[ds] as u64;
        }
    }
    s.lit_freq[256] += 1;
    s
}

fn data_bits(s: &BlockStats, lit_lens: &[u8], dist_lens: &[u8]) -> u64 {
    let mut bits = s.extra_bits;
    for (i, &f) in s.lit_freq.iter().enumerate() {
        bits += f as u64 * lit_lens[i] as u64;
    }
    for (i, &f) in s.dist_freq.iter().enumerate() {
        bits += f as u64 * dist_lens[i] as u64;
    }
    bits
}

/// §3.2.7 run-length form of the concatenated literal/length + distance code lengths: (symbol 0..=18, extra bits value).
fn rle_lengths(lens: &[u8]) -> Vec<(u8, u8)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < lens.len() {
        let v = lens[i];
        let mut run = 1;
        while i + run < lens.len() && lens[i + run] == v {
            run += 1;
        }
        let mut left = run;
        if v == 0 {
            while left >= 11 {
                let r = left.min(138);
                out.push((18, (r - 11) as u8));
                left -= r;
            }
            if left >= 3 {
                out.push((17, (left - 3) as u8));
                left = 0;
            }
        } else if left >= 4 {
            out.push((v, 0));
            left -= 1;
            while left >= 3 {
                let r = left.min(6);
                out.push((16, (r - 3) as u8));
                left -= r;
            }
        }
        for _ in 0..left {
            out.push((v, 0));
        }
        i += run;
    }
    out
}

struct Dynamic {
    lit_lens: Vec<u8>,
    dist_lens: Vec<u8>,
    hlit: usize,
    hdist: usize,
    rle: Vec<(u8, u8)>,
    cl_lens: Vec<u8>,
    hclen: usize,
    header_bits: u64,
}

fn plan_dynamic(s: &BlockStats) -> Dynamic {
    let lit_lens = code_lengths(&s.lit_freq, 15);
    let mut dist_lens = code_lengths(&s.dist_freq, 15);
    if dist_lens.iter().all(|&l| l == 0) {
        dist_lens[0] = 1; // no matches: one unused distance code, so the tree is still well formed
    }
    let hlit = (lit_lens.iter().rposition(|&l| l != 0).unwrap() + 1).max(257);
    let hdist = (dist_lens.iter().rposition(|&l| l != 0).unwrap() + 1).max(1);
    let mut all = lit_lens[..hlit].to_vec();
    all.extend_from_slice(&dist_lens[..hdist]);
    let rle = rle_lengths(&all);
    let mut cl_freq = vec![0u32; 19];
    for &(sym, _) in &rle {
        cl_freq[sym as usize] += 1;
    }
    let cl_lens = code_lengths(&cl_freq, 7);
    let hclen = (CLEN_ORDER.iter().rposition(|&i| cl_lens[i] != 0).unwrap() + 1).max(4);
    let mut header_bits = 5 + 5 + 4 + 3 * hclen as u64;
    for &(sym, _) in &rle {
        header_bits += cl_lens[sym as usize] as u64
            + match sym {
                16 => 2,
                17 => 3,
                18 => 7,
                _ => 0,
            };
    }
    Dynamic { lit_lens, dist_lens, hlit, hdist, rle, cl_lens, hclen, header_bits }
}

fn write_tokens(w: &mut BitWriter, tokens: &[Token], lit_lens: &[u8], dist_lens: &[u8]) {
    let lit_codes = canonical_codes(lit_lens);
    let dist_codes = canonical_codes(dist_lens);
    for t in tokens {
        if t.0 & 0x8000_0000 == 0 {
            let s = t.0 as usize;
            w.put(lit_codes[s], lit_lens[s] as u32);
        } else {
            let len = ((t.0 >> 16) & 0xFF) as usize + MIN_MATCH;
            let dist = (t.0 & 0xFFFF) as usize + 1;
            let ls = len_symbol(len);
            w.put(lit_codes[257 + ls], lit_lens[257 + ls] as u32);
            w.put((len - LEN_BASE[ls] as usize) as u32, LEN_EXTRA[ls] as u32);
            let ds = dist_symbol(dist);
            w.put(dist_codes[ds], dist_lens[ds] as u32);
            w.put((dist - DIST_BASE[ds] as usize) as u32, DIST_EXTRA[ds] as u32);
        }
    }
    w.put(lit_codes[256], lit_lens[256] as u32);
}

fn write_stored(w: &mut BitWriter, raw: &[u8], last: bool) {
    let mut chunks: Vec<&[u8]> = raw.chunks(65535).collect();
    if chunks.is_empty() {
        chunks.push(&[]);
    }
    let n = chunks.len();
    for (i, c) in chunks.into_iter().enumerate() {
        w.put((last && i + 1 == n) as u32, 1);
        w.put(0, 2); // BTYPE 00
        w.align();
        w.out.extend_from_slice(&(c.len() as u16).to_le_bytes());
        w.out.extend_from_slice(&(!(c.len() as u16)).to_le_bytes());
        w.out.extend_from_slice(c);
    }
}

fn write_block(w: &mut BitWriter, tokens: &[Token], raw: &[u8], last: bool) {
    let s = stats(tokens);
    let fixed_lit = fixed_lit_lens();
    let fixed_dist = vec![5u8; 30];
    let fixed_bits = 3 + data_bits(&s, &fixed_lit, &fixed_dist);
    let dynamic = plan_dynamic(&s);
    let dyn_bits = 3 + dynamic.header_bits + data_bits(&s, &dynamic.lit_lens, &dynamic.dist_lens);
    // stored: per 65535-byte chunk a 3-bit header, padding to a byte (worst case 7), and LEN/NLEN
    let chunks = raw.len().div_ceil(65535).max(1) as u64;
    let stored_bits = chunks * (3 + 7 + 32) + 8 * raw.len() as u64;
    if stored_bits < fixed_bits && stored_bits < dyn_bits {
        write_stored(w, raw, last);
    } else if fixed_bits <= dyn_bits {
        w.put(last as u32, 1);
        w.put(1, 2); // BTYPE 01
        write_tokens(w, tokens, &fixed_lit, &fixed_dist);
    } else {
        w.put(last as u32, 1);
        w.put(2, 2); // BTYPE 10
        w.put(dynamic.hlit as u32 - 257, 5);
        w.put(dynamic.hdist as u32 - 1, 5);
        w.put(dynamic.hclen as u32 - 4, 4);
        for &i in CLEN_ORDER.iter().take(dynamic.hclen) {
            w.put(dynamic.cl_lens[i] as u32, 3);
        }
        let cl_codes = canonical_codes(&dynamic.cl_lens);
        for &(sym, extra) in &dynamic.rle {
            w.put(cl_codes[sym as usize], dynamic.cl_lens[sym as usize] as u32);
            match sym {
                16 => w.put(extra as u32, 2),
                17 => w.put(extra as u32, 3),
                18 => w.put(extra as u32, 7),
                _ => {}
            }
        }
        write_tokens(w, tokens, &dynamic.lit_lens, &dynamic.dist_lens);
    }
}

/// Compress `data` into a raw DEFLATE stream (RFC 1951). `max_out` bounds the output; exceeding it is `OutputLimit`.
pub fn deflate_raw(data: &[u8], max_out: usize) -> Result<Vec<u8>> {
    if data.len() > u32::MAX as usize - 1 {
        return Err(Error::OutputLimit);
    }
    let mut w = BitWriter::new();
    let mut m = Matcher::new(data);
    let mut tokens: Vec<Token> = Vec::with_capacity(BLOCK_TOKENS);
    let mut block_start = 0usize;
    let mut pos = 0usize;
    while pos < data.len() {
        m.insert_to(pos);
        let (len, dist) = m.find(pos);
        let mut take_match = len >= MIN_MATCH;
        if take_match && len < LAZY_BELOW && pos + 1 < data.len() {
            m.insert_to(pos + 1);
            if m.find(pos + 1).0 > len {
                take_match = false; // the next position matches longer: emit this byte as a literal and re-search there
            }
        }
        if take_match {
            tokens.push(Token::mat(len, dist));
            pos += len;
        } else {
            tokens.push(Token::lit(data[pos]));
            pos += 1;
        }
        if tokens.len() >= BLOCK_TOKENS && pos < data.len() {
            write_block(&mut w, &tokens, &data[block_start..pos], false);
            tokens.clear();
            block_start = pos;
            if w.out.len() > max_out {
                return Err(Error::OutputLimit);
            }
        }
    }
    write_block(&mut w, &tokens, &data[block_start..pos], true);
    w.align();
    if w.out.len() > max_out {
        return Err(Error::OutputLimit);
    }
    Ok(w.out)
}

/// RFC 1950 zlib stream: header 78 9C, DEFLATE data, big-endian Adler-32.
pub fn zlib_compress(data: &[u8], max_out: usize) -> Result<Vec<u8>> {
    let mut out = vec![0x78, 0x9C];
    out.extend(deflate_raw(data, max_out)?);
    out.extend_from_slice(&adler32(data).to_be_bytes());
    if out.len() > max_out {
        return Err(Error::OutputLimit);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inflate::inflate_raw;

    fn rt(d: &[u8]) -> usize {
        let c = deflate_raw(d, usize::MAX).unwrap();
        let (back, used) = inflate_raw(&c, usize::MAX).unwrap();
        assert_eq!(back, d);
        assert_eq!(used, c.len());
        c.len()
    }

    #[test]
    fn empty_and_tiny() {
        assert_eq!(deflate_raw(b"", 16).unwrap(), vec![0x03, 0x00]);
        rt(b"a");
        rt(b"ab");
        rt(b"hello office-format");
    }

    #[test]
    fn repeats_compress_hard() {
        let d = vec![b'x'; 100_000];
        assert!(rt(&d) < 300);
        let t: Vec<u8> = b"<row r=\"1\"><c r=\"A1\" t=\"s\"><v>0</v></c></row>".iter().cycle().take(50_000).cloned().collect();
        assert!(rt(&t) < 600);
    }

    #[test]
    fn package_merge_respects_the_limit_and_kraft() {
        // fibonacci frequencies force deep trees; the limit must hold and the code stay complete
        let mut f = vec![1u32, 1];
        for i in 2..40 {
            f.push(f[i - 1] + f[i - 2]);
        }
        let l = code_lengths(&f, 15);
        assert!(l.iter().all(|&x| (1..=15).contains(&x)));
        let kraft: f64 = l.iter().map(|&x| 0.5f64.powi(x as i32)).sum();
        assert!((kraft - 1.0).abs() < 1e-9, "kraft {kraft}");
    }

    #[test]
    fn deterministic() {
        let d: Vec<u8> = (0..20_000u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
        assert_eq!(deflate_raw(&d, usize::MAX).unwrap(), deflate_raw(&d, usize::MAX).unwrap());
    }

    #[test]
    fn output_limit() {
        let d: Vec<u8> = (0..5_000u32).map(|i| (i.wrapping_mul(2654435761) >> 11) as u8).collect();
        assert_eq!(deflate_raw(&d, 100), Err(Error::OutputLimit));
    }
}
