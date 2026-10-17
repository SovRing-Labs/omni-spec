//! PNG decoding (W3C Portable Network Graphics, Third Edition): chunks, CRC-32, filters, Adam7, all colour types.
//!
//! Spec: https://www.w3.org/TR/png-3/ — §5 file structure and chunk layout (length, type, data, CRC), §6 colour
//! types and bit depths (Table 11.1 valid combinations in §11.2), §7 filtering (None, Sub, Up, Average, Paeth;
//! bytes-per-complete-pixel rule of §7.2), §8 interlacing (Adam7 passes), §11.2.3 PLTE, §11.3.2.1 tRNS.
//! Written from the specification text; the image is decoded with `crate::inflate` (RFC 1950/1951).
//!
//! Output is always RGBA8. Sub-8-bit gray and palette samples are scaled per §13.12 (sample × 255 / (2^d − 1));
//! 16-bit samples keep their most significant byte. tRNS keys compare against the raw sample value.

use crate::inflate;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    BadSignature,
    Truncated,
    /// CRC mismatch on a chunk, §5.4.
    BadCrc(&'static str),
    /// Missing or misplaced IHDR / IDAT / IEND, §5.3 and §5.6.
    BadChunkOrder(&'static str),
    /// Unknown critical chunk (uppercase first letter), §5.4.
    UnknownCritical([u8; 4]),
    /// Width/height/colour/depth/method combination not allowed by Table 11.1 or §7/§8.
    BadHeader(&'static str),
    /// Filter type outside 0–4 on a scanline, §7.
    BadFilter(u8),
    /// Palette index beyond PLTE, or PLTE missing for colour type 3, §11.2.3.
    BadPalette,
    Inflate(inflate::Error),
    /// Image dimensions would exceed `MAX_PIXELS`.
    TooLarge,
}

impl From<inflate::Error> for Error {
    fn from(e: inflate::Error) -> Self {
        Error::Inflate(e)
    }
}

pub type Result<T> = core::result::Result<T, Error>;

/// Decoded image: RGBA8, row-major, `width * height * 4` bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Upper bound on width × height accepted by the decoder (2^28 pixels, i.e. 1 GiB of RGBA8).
pub const MAX_PIXELS: u64 = 1 << 28;

// ---- CRC-32 (ISO 3309 / V.42, used by §5.4) ---------------------------------------------------------------

const fn crc_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        t[n] = c;
        n += 1;
    }
    t
}

static CRC_TABLE: [u32; 256] = crc_table();

/// CRC-32 over a chunk's type and data fields (§5.4: the CRC covers the chunk type code and chunk data).
pub fn crc32(parts: &[&[u8]]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for part in parts {
        for &b in *part {
            c = CRC_TABLE[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
        }
    }
    c ^ 0xFFFF_FFFF
}

// ---- chunks -----------------------------------------------------------------------------------------------

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

struct Header {
    width: u32,
    height: u32,
    depth: u8,
    color: u8,
    interlace: u8,
}

impl Header {
    /// Channels per pixel for the colour type (§6.1 Table 6.1).
    fn channels(&self) -> usize {
        match self.color {
            0 | 3 => 1,
            2 => 3,
            4 => 2,
            _ => 4,
        }
    }

    /// Bits per pixel, §7.2.
    fn bits_per_pixel(&self) -> usize {
        self.channels() * self.depth as usize
    }
}

fn check_header(h: &Header) -> Result<()> {
    // Table 11.1: allowed bit depths per colour type.
    let ok = match h.color {
        0 => matches!(h.depth, 1 | 2 | 4 | 8 | 16),
        2 | 4 | 6 => matches!(h.depth, 8 | 16),
        3 => matches!(h.depth, 1 | 2 | 4 | 8),
        _ => false,
    };
    if !ok {
        return Err(Error::BadHeader("colour type / bit depth"));
    }
    if h.width == 0 || h.height == 0 {
        return Err(Error::BadHeader("zero dimension"));
    }
    if h.width > 0x7FFF_FFFF || h.height > 0x7FFF_FFFF {
        return Err(Error::BadHeader("dimension over 2^31-1"));
    }
    if h.interlace > 1 {
        return Err(Error::BadHeader("interlace method"));
    }
    if (h.width as u64) * (h.height as u64) > MAX_PIXELS {
        return Err(Error::TooLarge);
    }
    Ok(())
}

// ---- filters (§7.4 - §7.6) -------------------------------------------------------------------------------

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i16 + b as i16 - c as i16;
    let (pa, pb, pc) = ((p - a as i16).abs(), (p - b as i16).abs(), (p - c as i16).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Reverse the scanline filters of one (sub-)image. `data` holds `height` rows of (1 filter byte + `stride` bytes).
fn unfilter(data: &[u8], stride: usize, height: usize, bpp: usize) -> Result<Vec<u8>> {
    let row_len = stride + 1;
    if data.len() < row_len * height {
        return Err(Error::Truncated);
    }
    let mut out = vec![0u8; stride * height];
    for y in 0..height {
        let filter = data[y * row_len];
        let src = &data[y * row_len + 1..(y + 1) * row_len];
        let (before, rest) = out.split_at_mut(y * stride);
        let cur = &mut rest[..stride];
        let prev: Option<&[u8]> = if y > 0 { Some(&before[(y - 1) * stride..]) } else { None };
        for x in 0..stride {
            let a = if x >= bpp { cur[x - bpp] } else { 0 };
            let b = prev.map_or(0, |p| p[x]);
            let c = if x >= bpp { prev.map_or(0, |p| p[x - bpp]) } else { 0 };
            let pred = match filter {
                0 => 0,
                1 => a,
                2 => b,
                3 => ((a as u16 + b as u16) / 2) as u8,
                4 => paeth(a, b, c),
                f => return Err(Error::BadFilter(f)),
            };
            cur[x] = src[x].wrapping_add(pred);
        }
    }
    Ok(out)
}

// ---- sample extraction and colour conversion (§6, §13.12) ---------------------------------------------------

/// Sample `i` (0-based, counted in samples across the row) of a row packed at `depth` bits.
fn sample(row: &[u8], i: usize, depth: u8) -> u16 {
    match depth {
        16 => ((row[2 * i] as u16) << 8) | row[2 * i + 1] as u16,
        8 => row[i] as u16,
        d => {
            let d = d as usize;
            let bit = i * d;
            let byte = row[bit / 8];
            let shift = 8 - d - (bit % 8);
            ((byte >> shift) as u16) & ((1u16 << d) - 1)
        }
    }
}

/// Scale a sample of `depth` bits to 8 bits (§13.12 for gray; 16-bit keeps the high byte).
fn to8(v: u16, depth: u8) -> u8 {
    match depth {
        16 => (v >> 8) as u8,
        8 => v as u8,
        d => {
            let max = (1u32 << d) - 1;
            ((v as u32 * 255) / max) as u8
        }
    }
}

struct Trns {
    gray: Option<u16>,
    rgb: Option<(u16, u16, u16)>,
    alpha: Vec<u8>,
}

fn parse_trns(data: &[u8], color: u8) -> Result<Trns> {
    let mut t = Trns { gray: None, rgb: None, alpha: Vec::new() };
    match color {
        0 => {
            if data.len() < 2 {
                return Err(Error::Truncated);
            }
            t.gray = Some(u16::from_be_bytes([data[0], data[1]]));
        }
        2 => {
            if data.len() < 6 {
                return Err(Error::Truncated);
            }
            t.rgb = Some((
                u16::from_be_bytes([data[0], data[1]]),
                u16::from_be_bytes([data[2], data[3]]),
                u16::from_be_bytes([data[4], data[5]]),
            ));
        }
        3 => t.alpha = data.to_vec(),
        _ => {}
    }
    Ok(t)
}

/// Convert one scanline of `n` pixels (the full width, or a pass width for Adam7) to RGBA8.
fn convert_row(h: &Header, row: &[u8], n: usize, palette: &[[u8; 3]], trns: &Trns, out: &mut [u8]) -> Result<()> {
    for x in 0..n {
        let o = 4 * x;
        match h.color {
            0 => {
                let v = sample(row, x, h.depth);
                let g = to8(v, h.depth);
                let a = if trns.gray == Some(v) { 0 } else { 255 };
                out[o..o + 4].copy_from_slice(&[g, g, g, a]);
            }
            2 => {
                let r = sample(row, 3 * x, h.depth);
                let g = sample(row, 3 * x + 1, h.depth);
                let b = sample(row, 3 * x + 2, h.depth);
                let a = if trns.rgb == Some((r, g, b)) { 0 } else { 255 };
                out[o..o + 4].copy_from_slice(&[to8(r, h.depth), to8(g, h.depth), to8(b, h.depth), a]);
            }
            3 => {
                let idx = sample(row, x, h.depth) as usize;
                let p = palette.get(idx).ok_or(Error::BadPalette)?;
                let a = trns.alpha.get(idx).copied().unwrap_or(255);
                out[o..o + 4].copy_from_slice(&[p[0], p[1], p[2], a]);
            }
            4 => {
                let g = to8(sample(row, 2 * x, h.depth), h.depth);
                let a = to8(sample(row, 2 * x + 1, h.depth), h.depth);
                out[o..o + 4].copy_from_slice(&[g, g, g, a]);
            }
            _ => {
                let r = to8(sample(row, 4 * x, h.depth), h.depth);
                let g = to8(sample(row, 4 * x + 1, h.depth), h.depth);
                let b = to8(sample(row, 4 * x + 2, h.depth), h.depth);
                let a = to8(sample(row, 4 * x + 3, h.depth), h.depth);
                out[o..o + 4].copy_from_slice(&[r, g, b, a]);
            }
        }
    }
    Ok(())
}

// ---- Adam7 (§8.2) ---------------------------------------------------------------------------------------------

/// (x origin, y origin, x step, y step) for the seven passes.
const ADAM7: [(usize, usize, usize, usize); 7] = [(0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4), (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)];

/// Pass dimensions; a pass with no pixels has no scanlines at all (§8.2).
fn pass_size(w: usize, h: usize, pass: (usize, usize, usize, usize)) -> (usize, usize) {
    let (x0, y0, dx, dy) = pass;
    let pw = if w > x0 { (w - x0).div_ceil(dx) } else { 0 };
    let ph = if h > y0 { (h - y0).div_ceil(dy) } else { 0 };
    (pw, ph)
}

// ---- top level -------------------------------------------------------------------------------------------------

/// Decode a PNG file (signature, chunks, IDAT zlib stream, filters, interlace, colour conversion) to RGBA8.
pub fn decode(data: &[u8]) -> Result<Image> {
    if data.len() < 8 || data[..8] != SIGNATURE {
        return Err(Error::BadSignature);
    }
    let mut pos = 8usize;
    let mut header: Option<Header> = None;
    let mut palette: Vec<[u8; 3]> = Vec::new();
    let mut trns_data: Option<Vec<u8>> = None;
    let mut idat: Vec<u8> = Vec::new();
    let mut seen_idat = false;
    let mut seen_end = false;
    while pos < data.len() {
        let len_bytes = data.get(pos..pos + 4).ok_or(Error::Truncated)?;
        let len = u32::from_be_bytes([len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3]]) as usize;
        let ty: [u8; 4] = data.get(pos + 4..pos + 8).ok_or(Error::Truncated)?.try_into().unwrap();
        let body = data.get(pos + 8..pos + 8 + len).ok_or(Error::Truncated)?;
        let crc_bytes = data.get(pos + 8 + len..pos + 12 + len).ok_or(Error::Truncated)?;
        let want = u32::from_be_bytes([crc_bytes[0], crc_bytes[1], crc_bytes[2], crc_bytes[3]]);
        if crc32(&[&ty, body]) != want {
            return Err(Error::BadCrc("chunk"));
        }
        pos += 12 + len;
        match &ty {
            b"IHDR" => {
                if header.is_some() || body.len() != 13 {
                    return Err(Error::BadChunkOrder("IHDR"));
                }
                if body[10] != 0 || body[11] != 0 {
                    return Err(Error::BadHeader("compression or filter method"));
                }
                let h = Header {
                    width: u32::from_be_bytes([body[0], body[1], body[2], body[3]]),
                    height: u32::from_be_bytes([body[4], body[5], body[6], body[7]]),
                    depth: body[8],
                    color: body[9],
                    interlace: body[12],
                };
                check_header(&h)?;
                header = Some(h);
            }
            b"PLTE" => {
                if header.is_none() || body.len() % 3 != 0 || body.is_empty() || body.len() > 768 {
                    return Err(Error::BadPalette);
                }
                palette = body.chunks(3).map(|c| [c[0], c[1], c[2]]).collect();
            }
            b"tRNS" => trns_data = Some(body.to_vec()),
            b"IDAT" => {
                if header.is_none() {
                    return Err(Error::BadChunkOrder("IDAT before IHDR"));
                }
                seen_idat = true;
                idat.extend_from_slice(body);
            }
            b"IEND" => {
                seen_end = true;
                break;
            }
            other => {
                // §5.4: a chunk whose first letter is uppercase is critical and must be understood.
                if other[0].is_ascii_uppercase() {
                    return Err(Error::UnknownCritical(*other));
                }
            }
        }
    }
    let h = header.ok_or(Error::BadChunkOrder("missing IHDR"))?;
    if !seen_end || !seen_idat {
        return Err(Error::BadChunkOrder("missing IDAT or IEND"));
    }
    if h.color == 3 && palette.is_empty() {
        return Err(Error::BadPalette);
    }
    let trns = match trns_data {
        Some(d) => parse_trns(&d, h.color)?,
        None => Trns { gray: None, rgb: None, alpha: Vec::new() },
    };

    let bpp = h.bits_per_pixel().div_ceil(8).max(1);
    let (w, hgt) = (h.width as usize, h.height as usize);
    // Upper bound on the inflated size: every pass's rows (filter byte + row bytes).
    let raw_cap: usize = if h.interlace == 1 {
        ADAM7
            .iter()
            .map(|&p| {
                let (pw, ph) = pass_size(w, hgt, p);
                if pw == 0 || ph == 0 { 0 } else { ph * (1 + (pw * h.bits_per_pixel()).div_ceil(8)) }
            })
            .sum()
    } else {
        hgt * (1 + (w * h.bits_per_pixel()).div_ceil(8))
    };
    let raw = inflate::zlib_decompress_bounded(&idat, raw_cap)?;

    let mut rgba = vec![0u8; w * hgt * 4];
    let mut at = 0usize;
    let passes: Vec<(usize, usize, usize, usize)> = if h.interlace == 1 { ADAM7.to_vec() } else { vec![(0, 0, 1, 1)] };
    for pass in passes {
        let (pw, ph) = pass_size(w, hgt, pass);
        if pw == 0 || ph == 0 {
            continue;
        }
        let stride = (pw * h.bits_per_pixel()).div_ceil(8);
        let len = ph * (stride + 1);
        let chunk = raw.get(at..at + len).ok_or(Error::Truncated)?;
        at += len;
        let pixels = unfilter(chunk, stride, ph, bpp)?;
        let (x0, y0, dx, dy) = pass;
        let mut row_rgba = vec![0u8; pw * 4];
        for j in 0..ph {
            let row = &pixels[j * stride..(j + 1) * stride];
            convert_row(&h, row, pw, &palette, &trns, &mut row_rgba)?;
            let y = y0 + j * dy;
            for i in 0..pw {
                let x = x0 + i * dx;
                let dst = (y * w + x) * 4;
                rgba[dst..dst + 4].copy_from_slice(&row_rgba[4 * i..4 * i + 4]);
            }
        }
    }
    Ok(Image { width: h.width, height: h.height, rgba })
}
