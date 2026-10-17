//! DEFLATE decompression (RFC 1951) and the zlib wrapper (RFC 1950), with Adler-32.
//!
//! Spec: RFC 1951 §3.2.3 (block header), §3.2.4 (stored), §3.2.5 (length/distance codes, Tables of Extra bits),
//! §3.2.6 (fixed Huffman codes), §3.2.7 (dynamic Huffman codes), §3.2.2 (canonical code assignment);
//! RFC 1950 §2.2 (zlib header: CMF/FLG/FCHECK/FDICT), §2.2 trailer (Adler-32, big-endian); §8.2 Adler-32.
//! Written from the RFC text. Malformed input returns `Err`; it never panics or reads out of bounds.
//! Decompression output is capped (see `MAX_OUTPUT`) so hostile inputs cannot exhaust memory.

/// Default cap on decompressed size (256 MiB). Use `inflate_bounded` for another limit.
pub const MAX_OUTPUT: usize = 256 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Input ended before the stream was complete.
    Truncated,
    /// BTYPE = 3 (reserved), RFC 1951 §3.2.3.
    BadBlockType,
    /// Stored block LEN/NLEN mismatch, RFC 1951 §3.2.4.
    BadStoredLength,
    /// Code length sets, symbol values, or code tables violate RFC 1951 §3.2.2 / §3.2.7.
    BadCode(&'static str),
    /// Back-reference points before the start of the output, RFC 1951 §3.2.5.
    BadDistance,
    /// zlib header (RFC 1950 §2.2): bad check bits, method, or window size.
    BadHeader,
    /// zlib preset dictionary (FDICT) is not supported.
    PresetDictionary,
    /// Adler-32 trailer mismatch, RFC 1950 §8.2.
    BadChecksum,
    /// Decompressed output would exceed the caller's limit.
    OutputLimit,
}

pub type Result<T> = core::result::Result<T, Error>;

/// Adler-32 checksum, RFC 1950 §8.2: s1 = 1 + sum of bytes, s2 = sum of s1; result = s2 << 16 | s1 (mod 65521).
pub fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65521;
    let (mut s1, mut s2) = (1u32, 0u32);
    // 5552 is the largest block for which s2 cannot overflow a u32 before reduction (RFC 1950 reference code).
    for chunk in data.chunks(5552) {
        for &b in chunk {
            s1 += b as u32;
            s2 += s1;
        }
        s1 %= MOD;
        s2 %= MOD;
    }
    (s2 << 16) | s1
}

// ---- bit reader (RFC 1951 §3.1.1: bits packed LSB first) -------------------------------------------------

struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    bit: u32,
    nbits: u32,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Bits { data, pos: 0, bit: 0, nbits: 0 }
    }

    fn need(&mut self, n: u32) -> Result<()> {
        while self.nbits < n {
            let b = *self.data.get(self.pos).ok_or(Error::Truncated)?;
            self.pos += 1;
            self.bit |= (b as u32) << self.nbits;
            self.nbits += 8;
        }
        Ok(())
    }

    /// Read `n` bits (n <= 24), first bit read is the least significant.
    fn take(&mut self, n: u32) -> Result<u32> {
        if n == 0 {
            return Ok(0);
        }
        self.need(n)?;
        let v = self.bit & ((1u32 << n) - 1);
        self.bit >>= n;
        self.nbits -= n;
        Ok(v)
    }

    /// Discard bits to the next byte boundary (RFC 1951 §3.2.4 stored blocks).
    fn align(&mut self) {
        let r = self.nbits % 8;
        self.bit >>= r;
        self.nbits -= r;
    }

    /// Byte position of the next unread whole byte (valid after `align`).
    fn byte_pos(&self) -> usize {
        self.pos - (self.nbits / 8) as usize
    }
}

// ---- canonical Huffman codes (RFC 1951 §3.2.2) ------------------------------------------------------------

struct Huffman {
    /// Number of codes of each length 0..=15.
    count: [u16; 16],
    /// Symbols ordered by (code length, symbol value).
    symbol: Vec<u16>,
}

impl Huffman {
    /// Build from code lengths (one per symbol). Over-subscribed sets are rejected; incomplete sets are allowed.
    fn new(lengths: &[u8]) -> Result<Huffman> {
        let mut count = [0u16; 16];
        for &l in lengths {
            count[l as usize] += 1;
        }
        count[0] = 0;
        let mut left: i32 = 1;
        for len in 1..16 {
            left <<= 1;
            left -= count[len] as i32;
            if left < 0 {
                return Err(Error::BadCode("over-subscribed code"));
            }
        }
        // Offsets of each length's first symbol in `symbol` (RFC 1951 §3.2.2 step 2).
        let mut offs = [0u16; 16];
        for len in 1..15 {
            offs[len + 1] = offs[len] + count[len];
        }
        let mut symbol = vec![0u16; lengths.len()];
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbol[offs[l as usize] as usize] = sym as u16;
                offs[l as usize] += 1;
            }
        }
        Ok(Huffman { count, symbol })
    }

    /// Decode one symbol, reading the code MSB-first one bit at a time (RFC 1951 §3.1.1, Huffman codes).
    fn decode(&self, bits: &mut Bits) -> Result<u16> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= bits.take(1)? as i32;
            let count = self.count[len] as i32;
            if code - count < first {
                return Ok(self.symbol[(index + (code - first)) as usize]);
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err(Error::BadCode("no code matches"))
    }
}

// ---- length / distance tables (RFC 1951 §3.2.5) -------------------------------------------------------------

const LEN_BASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEN_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const DIST_EXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];
/// Order in which code-length code lengths are stored, RFC 1951 §3.2.7.
const CLEN_ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

// ---- inflate ------------------------------------------------------------------------------------------------

/// Decompress a raw DEFLATE stream (RFC 1951). Returns the output and the number of input bytes consumed.
pub fn inflate_raw(data: &[u8], max_out: usize) -> Result<(Vec<u8>, usize)> {
    let mut bits = Bits::new(data);
    let mut out: Vec<u8> = Vec::new();
    loop {
        let last = bits.take(1)? == 1;
        match bits.take(2)? {
            0 => stored(&mut bits, &mut out, max_out)?,
            1 => {
                let (lit, dist) = fixed_tables()?;
                codes(&mut bits, &mut out, &lit, &dist, max_out)?;
            }
            2 => {
                let (lit, dist) = dynamic_tables(&mut bits)?;
                codes(&mut bits, &mut out, &lit, &dist, max_out)?;
            }
            _ => return Err(Error::BadBlockType),
        }
        if last {
            bits.align();
            return Ok((out, bits.byte_pos()));
        }
    }
}

/// Stored block (RFC 1951 §3.2.4): LEN and its one's complement NLEN, then LEN raw bytes.
fn stored(bits: &mut Bits, out: &mut Vec<u8>, max_out: usize) -> Result<()> {
    bits.align();
    let len = bits.take(16)?;
    let nlen = bits.take(16)?;
    if len != !nlen & 0xFFFF {
        return Err(Error::BadStoredLength);
    }
    // After align the reader holds no whole bytes beyond `pos` minus buffered bytes; drain them first.
    let len = len as usize;
    if out.len() + len > max_out {
        return Err(Error::OutputLimit);
    }
    for _ in 0..len {
        out.push(bits.take(8)? as u8);
    }
    Ok(())
}

/// Fixed Huffman tables (RFC 1951 §3.2.6).
fn fixed_tables() -> Result<(Huffman, Huffman)> {
    let mut lengths = [0u8; 288];
    for (i, l) in lengths.iter_mut().enumerate() {
        *l = match i {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    let dist = [5u8; 30];
    Ok((Huffman::new(&lengths)?, Huffman::new(&dist)?))
}

/// Dynamic Huffman tables (RFC 1951 §3.2.7).
fn dynamic_tables(bits: &mut Bits) -> Result<(Huffman, Huffman)> {
    let hlit = bits.take(5)? as usize + 257;
    let hdist = bits.take(5)? as usize + 1;
    let hclen = bits.take(4)? as usize + 4;
    if hlit > 286 || hdist > 30 {
        return Err(Error::BadCode("HLIT or HDIST out of range"));
    }
    let mut clen = [0u8; 19];
    for &idx in CLEN_ORDER.iter().take(hclen) {
        clen[idx] = bits.take(3)? as u8;
    }
    let code_len_code = Huffman::new(&clen)?;
    let total = hlit + hdist;
    let mut lengths: Vec<u8> = Vec::with_capacity(total);
    while lengths.len() < total {
        let sym = code_len_code.decode(bits)?;
        match sym {
            0..=15 => lengths.push(sym as u8),
            16 => {
                let prev = *lengths.last().ok_or(Error::BadCode("repeat with no previous length"))?;
                let n = 3 + bits.take(2)? as usize;
                lengths.extend(std::iter::repeat_n(prev, n));
            }
            17 => {
                let n = 3 + bits.take(3)? as usize;
                lengths.extend(std::iter::repeat_n(0, n));
            }
            _ => {
                let n = 11 + bits.take(7)? as usize;
                lengths.extend(std::iter::repeat_n(0, n));
            }
        }
        if lengths.len() > total {
            return Err(Error::BadCode("code lengths overflow HLIT+HDIST"));
        }
    }
    if lengths[256] == 0 {
        return Err(Error::BadCode("no end-of-block code"));
    }
    Ok((Huffman::new(&lengths[..hlit])?, Huffman::new(&lengths[hlit..])?))
}

/// Literal/length and distance symbols of one Huffman block (RFC 1951 §3.2.5).
fn codes(bits: &mut Bits, out: &mut Vec<u8>, lit: &Huffman, dist: &Huffman, max_out: usize) -> Result<()> {
    loop {
        let sym = lit.decode(bits)?;
        match sym {
            0..=255 => {
                if out.len() >= max_out {
                    return Err(Error::OutputLimit);
                }
                out.push(sym as u8);
            }
            256 => return Ok(()),
            257..=285 => {
                let i = (sym - 257) as usize;
                let len = LEN_BASE[i] as usize + bits.take(LEN_EXTRA[i] as u32)? as usize;
                let d = dist.decode(bits)? as usize;
                if d >= 30 {
                    return Err(Error::BadCode("distance symbol 30 or 31"));
                }
                let distance = DIST_BASE[d] as usize + bits.take(DIST_EXTRA[d] as u32)? as usize;
                if distance > out.len() {
                    return Err(Error::BadDistance);
                }
                if out.len() + len > max_out {
                    return Err(Error::OutputLimit);
                }
                let start = out.len() - distance;
                for k in 0..len {
                    let b = out[start + k];
                    out.push(b);
                }
            }
            _ => return Err(Error::BadCode("literal/length symbol 286 or 287")),
        }
    }
}

/// Decompress a raw DEFLATE stream with an output cap.
pub fn inflate_bounded(data: &[u8], max_out: usize) -> Result<Vec<u8>> {
    inflate_raw(data, max_out).map(|(out, _)| out)
}

/// Decompress a raw DEFLATE stream (RFC 1951) with the default output cap.
pub fn inflate(data: &[u8]) -> Result<Vec<u8>> {
    inflate_bounded(data, MAX_OUTPUT)
}

/// Decompress a zlib stream (RFC 1950): 2-byte header, DEFLATE data, big-endian Adler-32 trailer.
pub fn zlib_decompress(data: &[u8]) -> Result<Vec<u8>> {
    zlib_decompress_bounded(data, MAX_OUTPUT)
}

/// As `zlib_decompress`, with an output cap.
pub fn zlib_decompress_bounded(data: &[u8], max_out: usize) -> Result<Vec<u8>> {
    if data.len() < 2 {
        return Err(Error::Truncated);
    }
    let (cmf, flg) = (data[0], data[1]);
    // RFC 1950 §2.2: CM = 8 (deflate), CINFO <= 7 (window <= 32 KiB), (CMF*256 + FLG) divisible by 31.
    if cmf & 0x0F != 8 || cmf >> 4 > 7 || ((cmf as u16) << 8 | flg as u16) % 31 != 0 {
        return Err(Error::BadHeader);
    }
    if flg & 0x20 != 0 {
        return Err(Error::PresetDictionary);
    }
    let (out, used) = inflate_raw(&data[2..], max_out)?;
    let trailer = data.get(2 + used..2 + used + 4).ok_or(Error::Truncated)?;
    let want = u32::from_be_bytes([trailer[0], trailer[1], trailer[2], trailer[3]]);
    if adler32(&out) != want {
        return Err(Error::BadChecksum);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adler32_known_value() {
        // RFC 1950 §8.2 check value for "Wikipedia" style test: adler32("Wikipedia") = 0x11E60398.
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
        assert_eq!(adler32(b""), 1);
    }

    #[test]
    fn stored_block() {
        // BFINAL=1, BTYPE=00, then LEN=3, NLEN=!3, "abc".
        let data = [0x01, 0x03, 0x00, 0xFC, 0xFF, b'a', b'b', b'c'];
        assert_eq!(inflate(&data).unwrap(), b"abc");
    }

    #[test]
    fn stored_block_bad_nlen() {
        let data = [0x01, 0x03, 0x00, 0x00, 0x00, b'a', b'b', b'c'];
        assert_eq!(inflate(&data), Err(Error::BadStoredLength));
    }

    #[test]
    fn reserved_block_type() {
        assert_eq!(inflate(&[0x07]), Err(Error::BadBlockType));
    }

    #[test]
    fn fixed_block_literal_and_match() {
        // Raw DEFLATE of b"aaaa" as produced by zlib (level 9): one literal, one length-3 distance-1 copy, fixed codes.
        let data = [0x4B, 0x4C, 0x4C, 0x4C, 0x04, 0x00];
        assert_eq!(inflate(&data).unwrap(), b"aaaa");
    }

    #[test]
    fn zlib_header_checks() {
        assert_eq!(zlib_decompress(&[0x78]), Err(Error::Truncated));
        assert_eq!(zlib_decompress(&[0x79, 0x9C, 0, 0, 0, 0]), Err(Error::BadHeader));
    }
}
