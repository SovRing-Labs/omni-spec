//! GIF decoder (GIF87a / GIF89a): logical screen, global and local colour tables, graphic control extension
//! (transparency, disposal, delay), image descriptors with interlacing, and LZW image data.
//!
//! Spec: "GIF89a" (Compuserve, 1989; W3C copy https://www.w3.org/Graphics/GIF/spec-gif89a.txt) — §17 logical screen
//! descriptor and colour tables, §18 extension blocks (graphic control label 0xF9, comment 0xFE, plain text 0x01,
//! application 0xFF), §20 image descriptor (interlace flag, pass order), §22 LZW compression (variable code size
//! 3..12, clear and end codes, LSB-first packing), trailer 0x3B. Written from the specification text.
//!
//! `decode` parses every frame. `Gif::first_frame_rgba` composes frame 0 onto the logical screen (background
//! transparent, or the background colour when the global table provides it) and applies the transparent index.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    BadSignature,
    Truncated,
    BadBlock(u8),
    BadLzw(&'static str),
    BadPalette,
    NoFrames,
    /// Frame indices fall outside the frame's rectangle or the palette.
    BadFrame(&'static str),
}

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// One image (frame): its rectangle, palette indices (row-major, deinterlaced), palette, and control data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub left: u16,
    pub top: u16,
    pub width: u16,
    pub height: u16,
    pub interlaced: bool,
    pub indices: Vec<u8>,
    pub palette: Vec<Rgb>,
    /// Transparent colour index from the graphic control extension (§23), if its flag is set.
    pub transparent: Option<u8>,
    /// Disposal method (bits 2-4 of the GCE packed field).
    pub disposal: u8,
    /// Delay in hundredths of a second (GCE).
    pub delay: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gif {
    pub version: [u8; 3],
    pub width: u16,
    pub height: u16,
    pub background: u8,
    pub global_palette: Vec<Rgb>,
    pub frames: Vec<Frame>,
}

struct Reader<'a> {
    d: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn u8(&mut self) -> Result<u8> {
        let b = *self.d.get(self.pos).ok_or(Error::Truncated)?;
        self.pos += 1;
        Ok(b)
    }

    fn u16le(&mut self) -> Result<u16> {
        Ok(self.u8()? as u16 | (self.u8()? as u16) << 8)
    }

    fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        let s = self.d.get(self.pos..self.pos + n).ok_or(Error::Truncated)?;
        self.pos += n;
        Ok(s)
    }

    /// Data sub-blocks (§15): length byte then data, until a zero-length block.
    fn sub_blocks(&mut self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        loop {
            let n = self.u8()? as usize;
            if n == 0 {
                return Ok(out);
            }
            out.extend_from_slice(self.bytes(n)?);
        }
    }
}

fn read_palette(r: &mut Reader, packed: u8) -> Result<Vec<Rgb>> {
    // Size: 2^(N+1) entries, N in bits 0-2 (§17, §20).
    let n = 2usize << (packed & 7);
    let raw = r.bytes(3 * n)?;
    Ok(raw.chunks(3).map(|c| Rgb(c[0], c[1], c[2])).collect())
}

/// Decode a GIF87a or GIF89a file.
pub fn decode(data: &[u8]) -> Result<Gif> {
    let mut r = Reader { d: data, pos: 0 };
    let sig = r.bytes(6)?;
    let version = match sig {
        b"GIF87a" => *b"87a",
        b"GIF89a" => *b"89a",
        _ => return Err(Error::BadSignature),
    };
    let width = r.u16le()?;
    let height = r.u16le()?;
    let packed = r.u8()?;
    let background = r.u8()?;
    let _aspect = r.u8()?;
    let global_palette = if packed & 0x80 != 0 { read_palette(&mut r, packed)? } else { Vec::new() };

    let mut frames = Vec::new();
    let mut gce: Option<(Option<u8>, u8, u16)> = None; // (transparent index, disposal, delay)
    loop {
        match r.u8()? {
            0x2C => {
                let left = r.u16le()?;
                let top = r.u16le()?;
                let w = r.u16le()?;
                let h = r.u16le()?;
                let p = r.u8()?;
                let palette = if p & 0x80 != 0 {
                    read_palette(&mut r, p)?
                } else if !global_palette.is_empty() {
                    global_palette.clone()
                } else {
                    return Err(Error::BadPalette);
                };
                let interlaced = p & 0x40 != 0;
                let min_code = r.u8()?;
                let data = r.sub_blocks()?;
                let npix = w as usize * h as usize;
                let indices = lzw_decode(&data, min_code, npix)?;
                let indices = if interlaced { deinterlace(&indices, w as usize, h as usize) } else { indices };
                if indices.iter().any(|&i| i as usize >= palette.len()) {
                    return Err(Error::BadFrame("index beyond palette"));
                }
                let (transparent, disposal, delay) = gce.take().unwrap_or((None, 0, 0));
                frames.push(Frame { left, top, width: w, height: h, interlaced, indices, palette, transparent, disposal, delay });
            }
            0x21 => {
                let label = r.u8()?;
                if label == 0xF9 {
                    // Graphic control extension (§23): block size 4, packed, delay, transparent index, terminator.
                    let size = r.u8()?;
                    if size != 4 {
                        return Err(Error::BadBlock(0xF9));
                    }
                    let flags = r.u8()?;
                    let delay = r.u16le()?;
                    let tidx = r.u8()?;
                    let term = r.u8()?;
                    if term != 0 {
                        return Err(Error::BadBlock(0xF9));
                    }
                    let transparent = if flags & 1 != 0 { Some(tidx) } else { None };
                    gce = Some((transparent, (flags >> 2) & 7, delay));
                } else {
                    // Comment, plain text, application: skip their sub-blocks.
                    r.sub_blocks()?;
                }
            }
            0x3B => break,
            other => return Err(Error::BadBlock(other)),
        }
    }
    if frames.is_empty() {
        return Err(Error::NoFrames);
    }
    Ok(Gif { version, width, height, background, global_palette, frames })
}

/// LZW decoding (§22). `min_code` is the LZW minimum code size; `npix` the expected number of indices.
pub fn lzw_decode(data: &[u8], min_code: u8, npix: usize) -> Result<Vec<u8>> {
    if !(1..=8).contains(&min_code) {
        return Err(Error::BadLzw("minimum code size"));
    }
    let clear = 1u16 << min_code;
    let end = clear + 1;
    // Dictionary: each entry is (prefix code, last byte); first-byte and length are derived.
    let mut prefix: Vec<u16> = vec![0; 4096];
    let mut suffix: Vec<u8> = vec![0; 4096];
    for i in 0..clear as usize {
        suffix[i] = i as u8;
    }
    let mut next = end + 1;
    let mut code_size = min_code as u32 + 1;
    let mut prev: Option<u16> = None;
    let mut out: Vec<u8> = Vec::with_capacity(npix);
    let mut bitpos = 0usize;
    let total_bits = data.len() * 8;
    let mut stack: Vec<u8> = Vec::with_capacity(4096);

    // Write the string for `code` into `stack` in reverse, and return its first byte.
    fn expand(code: u16, clear: u16, prefix: &[u16], suffix: &[u8], stack: &mut Vec<u8>) -> u8 {
        stack.clear();
        let mut c = code;
        loop {
            stack.push(suffix[c as usize]);
            // Codes below `clear` are literals (their own byte); everything above is a dictionary entry.
            if c < clear {
                break;
            }
            c = prefix[c as usize];
        }
        *stack.last().unwrap()
    }

    while out.len() < npix {
        if bitpos + code_size as usize > total_bits {
            break;
        }
        // LSB-first code extraction (§22).
        let mut code: u16 = 0;
        for k in 0..code_size as usize {
            let bit = (data[(bitpos + k) / 8] >> ((bitpos + k) % 8)) & 1;
            code |= (bit as u16) << k;
        }
        bitpos += code_size as usize;

        if code == clear {
            code_size = min_code as u32 + 1;
            next = end + 1;
            prev = None;
            continue;
        }
        if code == end {
            break;
        }
        match prev {
            None => {
                if code >= clear {
                    return Err(Error::BadLzw("first code after clear is not a literal"));
                }
                out.push(code as u8);
            }
            Some(p) => {
                if code < next {
                    let first = expand(code, clear, &prefix, &suffix, &mut stack);
                    // stack holds the string reversed; emit it in order.
                    for &b in stack.iter().rev() {
                        out.push(b);
                    }
                    if next < 4096 {
                        prefix[next as usize] = p;
                        suffix[next as usize] = first;
                        next += 1;
                    }
                } else if code == next {
                    // KwKwK: the new entry is prev's string followed by its own first byte.
                    let pfirst = expand(p, clear, &prefix, &suffix, &mut stack);
                    for &b in stack.iter().rev() {
                        out.push(b);
                    }
                    out.push(pfirst);
                    if next < 4096 {
                        prefix[next as usize] = p;
                        suffix[next as usize] = pfirst;
                        next += 1;
                    }
                } else {
                    return Err(Error::BadLzw("code beyond dictionary"));
                }
            }
        }
        prev = Some(code);
        if next as u32 == (1u32 << code_size) && code_size < 12 {
            code_size += 1;
        }
    }
    if out.len() < npix {
        return Err(Error::Truncated);
    }
    out.truncate(npix);
    Ok(out)
}

/// Reorder interlaced rows into display order (§20: passes at rows 0,8.. / 4,12.. / 2,6.. / 1,3,5..).
pub fn deinterlace(src: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * h];
    let passes = [(0usize, 8usize), (4, 8), (2, 4), (1, 2)];
    let mut row_src = 0usize;
    for (start, step) in passes {
        let mut y = start;
        while y < h {
            out[y * w..(y + 1) * w].copy_from_slice(&src[row_src * w..(row_src + 1) * w]);
            row_src += 1;
            y += step;
        }
    }
    out
}

impl Gif {
    /// Frame 0 composed onto the logical screen as RGBA8 (width × height × 4 bytes).
    /// Pixels outside frame 0 are transparent when frame 0 has a transparent index (the spec leaves the area
    /// undefined; this matches Pillow). Otherwise they take the background colour of the global table, if any.
    /// The transparent index of frame 0 gives alpha 0.
    pub fn first_frame_rgba(&self) -> Vec<u8> {
        let (w, h) = (self.width as usize, self.height as usize);
        let bg = if self.frames[0].transparent.is_some() {
            None
        } else {
            self.global_palette.get(self.background as usize).copied()
        };
        let mut canvas = vec![0u8; w * h * 4];
        if let Some(Rgb(r, g, b)) = bg {
            for px in canvas.chunks_mut(4) {
                px.copy_from_slice(&[r, g, b, 255]);
            }
        }
        let f = &self.frames[0];
        for y in 0..f.height as usize {
            for x in 0..f.width as usize {
                let cx = f.left as usize + x;
                let cy = f.top as usize + y;
                if cx >= w || cy >= h {
                    continue;
                }
                let idx = f.indices[y * f.width as usize + x];
                let o = (cy * w + cx) * 4;
                if f.transparent == Some(idx) {
                    canvas[o..o + 4].copy_from_slice(&[0, 0, 0, 0]);
                } else {
                    let Rgb(r, g, b) = f.palette[idx as usize];
                    canvas[o..o + 4].copy_from_slice(&[r, g, b, 255]);
                }
            }
        }
        canvas
    }
}
