//! WOFF2 container decoder (W3C Recommendation "WOFF File Format 2.0", 2018).
//!
//! Spec sections: §3 header (48 bytes), §4.1 UIntBase128, §4.2 255UInt16, §5.1 table directory (known-tag index, flags byte
//! with the transform version in bits 6-7, optional `transformLength`), §5.2 collection directory (NOT supported here),
//! §5.3 compressed data (one Brotli stream, RFC 7932 via `brotli.rs`), §5.4 metadata/private blocks (ignored), §6.1 glyf/loca
//! transform (streams, triplet coordinate encoding §6.1.1.1, bbox bitmap, overlap bitmap), §6.2 hmtx transform. The output
//! is the table set of the original font; `to_sfnt` writes it as an OpenType/TrueType file (OpenType spec ch. 2).
//!
//! The rebuilt glyf table is functionally equal to the original, not always byte-equal: 36 of the 40 reference fonts rebuild to
//! exactly the directory's `origLength`, 4 are a few bytes shorter (`Woff2::glyf_size_differs`).
//!
//! Reconstructed glyf bytes use the plain TrueType glyph layout (OpenType `glyf`): a single flag byte per point with repeat
//! compression, x/y deltas as 1 or 2 bytes, records padded to 4 bytes, loca in the format the transform header names.
//!
//! Provenance: written from the specification text; fontTools was RUN as a black-box oracle (`tools/oracle_woff2.py`), its
//! source was not read.

use crate::brotli;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Truncated,
    BadSignature,
    BadDirectory(&'static str),
    Brotli(brotli::Error),
    /// Font collections (flavor `ttcf`) and transforms this module does not read.
    Unsupported(&'static str),
    BadGlyf(&'static str),
    BadHmtx(&'static str),
    OutputLimit,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
impl From<brotli::Error> for Error {
    fn from(e: brotli::Error) -> Self {
        Error::Brotli(e)
    }
}
pub type Result<T> = std::result::Result<T, Error>;

/// §5.1 known table tags, indexed by the low 6 bits of the flags byte (63 = an explicit 4-byte tag follows).
const KNOWN: [&[u8; 4]; 63] = [
    b"cmap", b"head", b"hhea", b"hmtx", b"maxp", b"name", b"OS/2", b"post", b"cvt ", b"fpgm", b"glyf", b"loca", b"prep", b"CFF ",
    b"VORG", b"EBDT", b"EBLC", b"gasp", b"hdmx", b"kern", b"LTSH", b"PCLT", b"VDMX", b"vhea", b"vmtx", b"BASE", b"GDEF", b"GPOS",
    b"GSUB", b"EBSC", b"JSTF", b"MATH", b"CBDT", b"CBLC", b"COLR", b"CPAL", b"SVG ", b"sbix", b"acnt", b"avar", b"bdat", b"bloc",
    b"bsln", b"cvar", b"fdsc", b"feat", b"fmtx", b"fvar", b"gvar", b"hsty", b"just", b"lcar", b"mort", b"morx", b"opbd", b"prop",
    b"trak", b"Zapf", b"Silf", b"Glat", b"Gloc", b"Feat", b"Sill",
];

#[derive(Debug, Clone)]
pub struct Table {
    pub tag: [u8; 4],
    /// the table as it was in the original font (after reversing any transform)
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Woff2 {
    /// sfnt version of the original font (0x00010000, `OTTO`, `true`)
    pub flavor: u32,
    /// tables in directory order
    pub tables: Vec<Table>,
    pub total_sfnt_size: u32,
    /// the rebuilt glyf is not the same length as the directory's `origLength` (an equivalent, not identical, encoding)
    pub glyf_size_differs: bool,
}

impl Woff2 {
    pub fn table(&self, tag: &[u8; 4]) -> Option<&[u8]> {
        self.tables.iter().find(|t| &t.tag == tag).map(|t| t.data.as_slice())
    }
}

struct Rd<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Rd<'a> {
    fn new(b: &'a [u8]) -> Rd<'a> {
        Rd { b, p: 0 }
    }
    fn u8(&mut self) -> Result<u8> {
        let v = *self.b.get(self.p).ok_or(Error::Truncated)?;
        self.p += 1;
        Ok(v)
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let s = self.b.get(self.p..self.p.checked_add(n).ok_or(Error::Truncated)?).ok_or(Error::Truncated)?;
        self.p += n;
        Ok(s)
    }
    /// §4.1 UIntBase128: big-endian 7-bit groups, high bit = more; at most 5 bytes, no leading 0x80, result fits u32.
    fn base128(&mut self) -> Result<u32> {
        let mut acc: u32 = 0;
        for i in 0..5 {
            let b = self.u8()?;
            if i == 0 && b == 0x80 {
                return Err(Error::BadDirectory("UIntBase128 with a leading zero group"));
            }
            if acc & 0xFE00_0000 != 0 {
                return Err(Error::BadDirectory("UIntBase128 overflows 32 bits"));
            }
            acc = (acc << 7) | (b & 0x7F) as u32;
            if b & 0x80 == 0 {
                return Ok(acc);
            }
        }
        Err(Error::BadDirectory("UIntBase128 longer than 5 bytes"))
    }
    /// §4.2 255UInt16: one byte < 253 is itself; 253 = a following u16; 254 = 506 + next byte; 255 = 253 + next byte.
    fn u255(&mut self) -> Result<u32> {
        match self.u8()? {
            253 => Ok(self.u16()? as u32),
            254 => Ok(506 + self.u8()? as u32),
            255 => Ok(253 + self.u8()? as u32),
            b => Ok(b as u32),
        }
    }
}

struct Entry {
    tag: [u8; 4],
    transform: u8,
    orig_len: u32,
    /// length of this table's span in the decompressed stream
    stream_len: u32,
    transformed: bool,
}

/// Decode a WOFF2 file into the original font's tables. `max_out` bounds the decompressed stream and every rebuilt table.
pub fn decode(data: &[u8], max_out: usize) -> Result<Woff2> {
    let mut r = Rd::new(data);
    if r.u32()? != 0x774F_4632 {
        return Err(Error::BadSignature);
    }
    let flavor = r.u32()?;
    let length = r.u32()? as usize;
    let num_tables = r.u16()? as usize;
    let _reserved = r.u16()?;
    let total_sfnt_size = r.u32()?;
    let total_compressed = r.u32()? as usize;
    r.take(4 + 4 * 5)?; // major, minor, metaOffset/Length/OrigLength, privOffset/Length (§3: metadata and private data are not needed)
    if length != data.len() && length > data.len() {
        return Err(Error::Truncated);
    }
    if flavor == 0x7474_6366 {
        return Err(Error::Unsupported("font collections (ttcf)"));
    }
    if num_tables == 0 {
        return Err(Error::BadDirectory("no tables"));
    }
    let mut entries: Vec<Entry> = Vec::with_capacity(num_tables);
    for _ in 0..num_tables {
        let flags = r.u8()?;
        let idx = (flags & 0x3F) as usize;
        let transform = flags >> 6;
        let tag: [u8; 4] = if idx == 63 { r.take(4)?.try_into().unwrap() } else { *KNOWN[idx] };
        let orig_len = r.base128()?;
        // §5.1: transformLength is present for glyf/loca with transform version 0 and for any other table with version != 0
        let is_gl = &tag == b"glyf" || &tag == b"loca";
        let transformed = if is_gl { transform == 0 } else { transform != 0 };
        let stream_len = if transformed { r.base128()? } else { orig_len };
        entries.push(Entry { tag, transform, orig_len, stream_len, transformed });
    }
    let comp = data.get(r.p..r.p.checked_add(total_compressed).ok_or(Error::Truncated)?).ok_or(Error::Truncated)?;
    let stream = brotli::decompress_bounded(comp, max_out)?;
    let want: usize = entries.iter().map(|e| e.stream_len as usize).sum();
    if want != stream.len() {
        return Err(Error::BadDirectory("decompressed size differs from the directory"));
    }
    let mut spans: Vec<&[u8]> = Vec::with_capacity(entries.len());
    let mut at = 0;
    for e in &entries {
        spans.push(&stream[at..at + e.stream_len as usize]);
        at += e.stream_len as usize;
    }
    let find = |t: &[u8; 4]| entries.iter().position(|e| &e.tag == t);
    let mut out: Vec<Option<Vec<u8>>> = entries.iter().map(|e| if e.transformed { None } else { Some(Vec::new()) }).collect();
    for (i, e) in entries.iter().enumerate() {
        if !e.transformed {
            if spans[i].len() != e.orig_len as usize {
                return Err(Error::BadDirectory("untransformed table length"));
            }
            out[i] = Some(spans[i].to_vec());
        }
    }
    // §6.1: glyf and loca are transformed together; loca's transform length is 0 and it is rebuilt from the glyf stream
    let mut x_mins: Option<Vec<i16>> = None;
    let mut glyf_size_differs = false;
    if let Some(gi) = find(b"glyf").filter(|&i| entries[i].transformed) {
        let li = find(b"loca").ok_or(Error::BadGlyf("transformed glyf without loca"))?;
        if !entries[li].transformed || entries[li].stream_len != 0 {
            return Err(Error::BadGlyf("loca must be transformed with length 0 alongside a transformed glyf"));
        }
        let (glyf, loca, xm) = reconstruct_glyf(spans[gi], max_out)?;
        // loca has a fixed size; glyf is rebuilt to be functionally equal, not byte-equal (the encoder's original may spend
        // extra bytes on flag repeats or padding), so only a size difference is recorded, not refused (§6.1: reconstruction
        // yields an equivalent font)
        if loca.len() != entries[li].orig_len as usize {
            return Err(Error::BadGlyf("rebuilt loca length differs from the directory"));
        }
        glyf_size_differs = glyf.len() != entries[gi].orig_len as usize;
        out[gi] = Some(glyf);
        out[li] = Some(loca);
        x_mins = Some(xm);
    } else if let Some(li) = find(b"loca") {
        if entries[li].transformed {
            return Err(Error::BadGlyf("transformed loca without a transformed glyf"));
        }
    }
    if let Some(hi) = find(b"hmtx").filter(|&i| entries[i].transformed) {
        if entries[hi].transform != 1 {
            return Err(Error::Unsupported("hmtx transform version"));
        }
        let hhea = find(b"hhea").and_then(|i| out[i].as_ref()).ok_or(Error::BadHmtx("hhea needed"))?;
        let maxp = find(b"maxp").and_then(|i| out[i].as_ref()).ok_or(Error::BadHmtx("maxp needed"))?;
        let xm = x_mins.as_ref().ok_or(Error::BadHmtx("hmtx transform needs the glyf transform"))?;
        let h = reconstruct_hmtx(spans[hi], hhea, maxp, xm)?;
        if h.len() != entries[hi].orig_len as usize {
            return Err(Error::BadHmtx("rebuilt hmtx length differs from the directory"));
        }
        out[hi] = Some(h);
    }
    for (e, o) in entries.iter().zip(&out) {
        if o.is_none() {
            return Err(Error::Unsupported(if &e.tag == b"glyf" || &e.tag == b"loca" { "glyf/loca" } else { "table transform" }));
        }
    }
    let tables = entries.iter().zip(out).map(|(e, o)| Table { tag: e.tag, data: o.unwrap() }).collect();
    Ok(Woff2 { flavor, tables, total_sfnt_size, glyf_size_differs })
}

// ---- §6.1 glyf transform ----

fn s16(b: &[u8], i: usize) -> i16 {
    i16::from_be_bytes([b[i], b[i + 1]])
}

/// §6.1.1.1 triplet decode: one flag byte (bit 7 = off-curve, low 7 bits select the encoding) + 1..4 data bytes -> (dx, dy).
fn triplet(flag: u8, g: &mut Rd) -> Result<(i32, i32)> {
    let f = (flag & 0x7F) as i32;
    let sign = |bit: i32, v: i32| if bit & 1 != 0 { v } else { -v };
    let n = if f < 84 { 1 } else if f < 120 { 2 } else if f < 124 { 3 } else { 4 };
    let d = g.take(n)?;
    let d: Vec<i32> = d.iter().map(|&x| x as i32).collect();
    Ok(if f < 10 {
        (0, sign(f, ((f & 14) << 7) + d[0]))
    } else if f < 20 {
        (sign(f, (((f - 10) & 14) << 7) + d[0]), 0)
    } else if f < 84 {
        let b0 = f - 20;
        (sign(f, 1 + (b0 & 0x30) + (d[0] >> 4)), sign(f >> 1, 1 + ((b0 & 0x0C) << 2) + (d[0] & 0x0F)))
    } else if f < 120 {
        let b0 = f - 84;
        (sign(f, 1 + ((b0 / 12) << 8) + d[0]), sign(f >> 1, 1 + (((b0 % 12) >> 2) << 8) + d[1]))
    } else if f < 124 {
        (sign(f, (d[0] << 4) + (d[1] >> 4)), sign(f >> 1, ((d[1] & 0x0F) << 8) + d[2]))
    } else {
        (sign(f, (d[0] << 8) + d[1]), sign(f >> 1, (d[2] << 8) + d[3]))
    })
}

/// Rebuild glyf + loca from the transformed glyf stream (§6.1); also returns each glyph's xMin (for the hmtx transform).
fn reconstruct_glyf(t: &[u8], max_out: usize) -> Result<(Vec<u8>, Vec<u8>, Vec<i16>)> {
    let mut h = Rd::new(t);
    let _version = h.u16()?;
    let option_flags = h.u16()?;
    let num_glyphs = h.u16()? as usize;
    let index_format = h.u16()?;
    if index_format > 1 {
        return Err(Error::BadGlyf("indexFormat"));
    }
    let sizes: Vec<usize> = (0..7).map(|_| h.u32().map(|v| v as usize)).collect::<Result<_>>()?;
    let mut at = h.p;
    let mut streams: Vec<&[u8]> = Vec::new();
    for s in &sizes {
        streams.push(t.get(at..at.checked_add(*s).ok_or(Error::Truncated)?).ok_or(Error::Truncated)?);
        at += s;
    }
    let overlap_bitmap: Option<&[u8]> = if option_flags & 1 != 0 { Some(t.get(at..at + num_glyphs.div_ceil(8)).ok_or(Error::Truncated)?) } else { None };
    let (n_contour, n_points, flag_s, glyph_s, comp_s, bbox_s, instr_s) = (streams[0], streams[1], streams[2], streams[3], streams[4], streams[5], streams[6]);
    let bitmap_len = 4 * num_glyphs.div_ceil(32);
    if bbox_s.len() < bitmap_len {
        return Err(Error::BadGlyf("bbox stream shorter than its bitmap"));
    }
    if n_contour.len() != num_glyphs * 2 {
        return Err(Error::BadGlyf("nContour stream length"));
    }
    let (bbox_bitmap, bbox_vals) = bbox_s.split_at(bitmap_len);
    let (mut np, mut fl, mut gl, mut cs, mut bb, mut ins) = (Rd::new(n_points), Rd::new(flag_s), Rd::new(glyph_s), Rd::new(comp_s), Rd::new(bbox_vals), Rd::new(instr_s));
    let mut glyf: Vec<u8> = Vec::new();
    let mut offsets: Vec<u32> = Vec::with_capacity(num_glyphs + 1);
    let mut x_mins: Vec<i16> = Vec::with_capacity(num_glyphs);
    for gid in 0..num_glyphs {
        offsets.push(glyf.len() as u32);
        let nc = s16(n_contour, gid * 2);
        let has_bbox = bbox_bitmap[gid >> 3] & (0x80 >> (gid & 7)) != 0;
        if nc == 0 {
            if has_bbox {
                return Err(Error::BadGlyf("empty glyph with a bbox"));
            }
            x_mins.push(0);
            continue;
        }
        let rec_start = glyf.len();
        if nc > 0 {
            let nc = nc as usize;
            let mut end_pts: Vec<u16> = Vec::with_capacity(nc);
            let mut total: usize = 0;
            for _ in 0..nc {
                total += np.u255()? as usize;
                end_pts.push(u16::try_from(total.wrapping_sub(1)).map_err(|_| Error::BadGlyf("too many points"))?);
            }
            let flags = fl.take(total)?;
            let (mut x, mut y) = (0i32, 0i32);
            let mut pts: Vec<(i32, i32, bool)> = Vec::with_capacity(total);
            for &f in flags {
                let (dx, dy) = triplet(f, &mut gl)?;
                x += dx;
                y += dy;
                pts.push((x, y, f & 0x80 == 0));
            }
            let ilen = gl.u255()? as usize;
            let instr = ins.take(ilen)?;
            let (xmin, ymin, xmax, ymax) = if has_bbox {
                let b = bb.take(8)?;
                (s16(b, 0), s16(b, 2), s16(b, 4), s16(b, 6))
            } else {
                let xs = pts.iter().map(|p| p.0);
                let ys = pts.iter().map(|p| p.1);
                (xs.clone().min().unwrap_or(0) as i16, ys.clone().min().unwrap_or(0) as i16, xs.max().unwrap_or(0) as i16, ys.max().unwrap_or(0) as i16)
            };
            glyf.extend((nc as i16).to_be_bytes());
            for v in [xmin, ymin, xmax, ymax] {
                glyf.extend(v.to_be_bytes());
            }
            for e in &end_pts {
                glyf.extend(e.to_be_bytes());
            }
            glyf.extend((ilen as u16).to_be_bytes());
            glyf.extend(instr);
            let overlap = overlap_bitmap.is_some_and(|m| m[gid >> 3] & (0x80 >> (gid & 7)) != 0);
            write_simple_points(&mut glyf, &pts, overlap);
            x_mins.push(xmin);
        } else if nc == -1 {
            if !has_bbox {
                return Err(Error::BadGlyf("composite glyph without a bbox"));
            }
            let b = bb.take(8)?;
            let (xmin, ymin, xmax, ymax) = (s16(b, 0), s16(b, 2), s16(b, 4), s16(b, 6));
            glyf.extend((-1i16).to_be_bytes());
            for v in [xmin, ymin, xmax, ymax] {
                glyf.extend(v.to_be_bytes());
            }
            // component records (OpenType glyf: flags, glyphIndex, arguments, optional scale) up to one without MORE_COMPONENTS
            let mut have_instr = false;
            loop {
                let start = cs.p;
                let flags = cs.u16()?;
                cs.u16()?;
                cs.take(if flags & 0x0001 != 0 { 4 } else { 2 })?;
                cs.take(if flags & 0x0008 != 0 { 2 } else if flags & 0x0040 != 0 { 4 } else if flags & 0x0080 != 0 { 8 } else { 0 })?;
                glyf.extend(&cs.b[start..cs.p]);
                have_instr |= flags & 0x0100 != 0;
                if flags & 0x0020 == 0 {
                    break;
                }
            }
            if have_instr {
                let ilen = gl.u255()? as usize;
                glyf.extend((ilen as u16).to_be_bytes());
                glyf.extend(ins.take(ilen)?);
            }
            x_mins.push(xmin);
        } else {
            return Err(Error::BadGlyf("numberOfContours"));
        }
        while (glyf.len() - rec_start) % 4 != 0 {
            glyf.push(0);
        }
        if glyf.len() > max_out {
            return Err(Error::OutputLimit);
        }
    }
    offsets.push(glyf.len() as u32);
    let mut loca = Vec::new();
    for o in &offsets {
        if index_format == 0 {
            loca.extend(u16::try_from(o / 2).map_err(|_| Error::BadGlyf("short loca overflow"))?.to_be_bytes());
        } else {
            loca.extend(o.to_be_bytes());
        }
    }
    Ok((glyf, loca, x_mins))
}

/// OpenType `glyf` simple-glyph point data: flags (repeat-compressed), then x deltas, then y deltas.
fn write_simple_points(out: &mut Vec<u8>, pts: &[(i32, i32, bool)], overlap_first: bool) {
    let mut flags: Vec<u8> = Vec::with_capacity(pts.len());
    let (mut xs, mut ys): (Vec<u8>, Vec<u8>) = (Vec::new(), Vec::new());
    let (mut px, mut py) = (0i32, 0i32);
    for (i, &(x, y, on)) in pts.iter().enumerate() {
        let mut f: u8 = if on { 0x01 } else { 0 };
        if i == 0 && overlap_first {
            f |= 0x40; // OVERLAP_SIMPLE
        }
        let (dx, dy) = (x - px, y - py);
        if dx == 0 {
            f |= 0x10; // x is same as previous
        } else if dx.abs() < 256 {
            f |= 0x02 | if dx > 0 { 0x10 } else { 0 };
            xs.push(dx.unsigned_abs() as u8);
        } else {
            xs.extend((dx as i16).to_be_bytes());
        }
        if dy == 0 {
            f |= 0x20;
        } else if dy.abs() < 256 {
            f |= 0x04 | if dy > 0 { 0x20 } else { 0 };
            ys.push(dy.unsigned_abs() as u8);
        } else {
            ys.extend((dy as i16).to_be_bytes());
        }
        flags.push(f);
        (px, py) = (x, y);
    }
    let mut i = 0;
    while i < flags.len() {
        let f = flags[i];
        let mut run = 1;
        while i + run < flags.len() && flags[i + run] == f && run < 256 {
            run += 1;
        }
        if run >= 2 {
            out.push(f | 0x08);
            out.push((run - 1) as u8);
        } else {
            out.push(f);
        }
        i += run;
    }
    out.extend(xs);
    out.extend(ys);
}

// ---- §6.2 hmtx transform ----

fn reconstruct_hmtx(t: &[u8], hhea: &[u8], maxp: &[u8], x_mins: &[i16]) -> Result<Vec<u8>> {
    let num_h = u16::from_be_bytes(hhea.get(34..36).ok_or(Error::BadHmtx("hhea too short"))?.try_into().unwrap()) as usize;
    let num_glyphs = u16::from_be_bytes(maxp.get(4..6).ok_or(Error::BadHmtx("maxp too short"))?.try_into().unwrap()) as usize;
    if num_h == 0 || num_h > num_glyphs || x_mins.len() != num_glyphs {
        return Err(Error::BadHmtx("metric counts"));
    }
    let mut r = Rd::new(t);
    let flags = r.u8()?;
    if flags & 0xFC != 0 {
        return Err(Error::BadHmtx("reserved flag bits"));
    }
    let advances: Vec<u16> = (0..num_h).map(|_| r.u16()).collect::<Result<_>>()?;
    let lsb: Vec<i16> = if flags & 1 == 0 { (0..num_h).map(|_| r.u16().map(|v| v as i16)).collect::<Result<_>>()? } else { x_mins[..num_h].to_vec() };
    let rest: Vec<i16> = if flags & 2 == 0 { (num_h..num_glyphs).map(|_| r.u16().map(|v| v as i16)).collect::<Result<_>>()? } else { x_mins[num_h..].to_vec() };
    let mut out = Vec::with_capacity(4 * num_h + 2 * (num_glyphs - num_h));
    for i in 0..num_h {
        out.extend(advances[i].to_be_bytes());
        out.extend(lsb[i].to_be_bytes());
    }
    for v in rest {
        out.extend(v.to_be_bytes());
    }
    Ok(out)
}

// ---- sfnt writer (OpenType spec ch. 2) ----

fn checksum(b: &[u8]) -> u32 {
    let mut s: u32 = 0;
    for c in b.chunks(4) {
        let mut w = [0u8; 4];
        w[..c.len()].copy_from_slice(c);
        s = s.wrapping_add(u32::from_be_bytes(w));
    }
    s
}

/// The decoded font as an sfnt file: tables sorted by tag, 4-byte aligned, table and `head.checkSumAdjustment` checksums set.
pub fn to_sfnt(w: &Woff2) -> Result<Vec<u8>> {
    let mut tabs: Vec<&Table> = w.tables.iter().collect();
    tabs.sort_by_key(|t| t.tag);
    let n = tabs.len() as u16;
    let mut sr = 1u16;
    let mut es = 0u16;
    while sr * 2 <= n {
        sr *= 2;
        es += 1;
    }
    let (search_range, range_shift) = (sr * 16, n * 16 - sr * 16);
    let dir_len = 12 + 16 * tabs.len();
    let mut out = Vec::new();
    out.extend(w.flavor.to_be_bytes());
    for v in [n, search_range, es, range_shift] {
        out.extend(v.to_be_bytes());
    }
    let mut body: Vec<u8> = Vec::new();
    let mut recs: Vec<([u8; 4], u32, u32, u32)> = Vec::new();
    let mut head_at: Option<usize> = None;
    for t in &tabs {
        let off = dir_len + body.len();
        let mut data = t.data.clone();
        if &t.tag == b"head" {
            if data.len() < 12 {
                return Err(Error::BadDirectory("head too short"));
            }
            data[8..12].fill(0); // checkSumAdjustment is computed over the file with this field zero (OpenType `head`)
            head_at = Some(off + 8);
        }
        recs.push((t.tag, checksum(&data), off as u32, data.len() as u32));
        body.extend(&data);
        while body.len() % 4 != 0 {
            body.push(0);
        }
    }
    for (tag, sum, off, len) in &recs {
        out.extend(tag);
        out.extend(sum.to_be_bytes());
        out.extend(off.to_be_bytes());
        out.extend(len.to_be_bytes());
    }
    out.extend(&body);
    if let Some(p) = head_at {
        let adj = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out));
        out[p..p + 4].copy_from_slice(&adj.to_be_bytes());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base128_and_255uint16() {
        let mut r = Rd::new(&[0x3F, 0x85, 0x01, 0x81, 0x80, 0x00]);
        assert_eq!(r.base128().unwrap(), 63);
        assert_eq!(r.base128().unwrap(), 5 * 128 + 1);
        assert_eq!(r.base128().unwrap(), 1 << 14);
        assert!(Rd::new(&[0x80, 0x01]).base128().is_err()); // leading zero group
        assert!(Rd::new(&[0xFF, 0xFF, 0xFF, 0xFF, 0x7F]).base128().is_err()); // overflow
        let mut r = Rd::new(&[10, 253, 0x01, 0x02, 254, 7, 255, 7]);
        assert_eq!((r.u255().unwrap(), r.u255().unwrap(), r.u255().unwrap(), r.u255().unwrap()), (10, 0x0102, 513, 260));
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(decode(b"nope", 1 << 20).unwrap_err(), Error::BadSignature);
        assert_eq!(decode(b"wOF2", 1 << 20).unwrap_err(), Error::Truncated);
        assert_eq!(decode(&[0u8; 64], 1 << 20).unwrap_err(), Error::BadSignature);
    }

    #[test]
    fn triplet_known_cases() {
        // flag 0: dx=0, dy=-(data) ; flag 1: dy=+data
        assert_eq!(triplet(0, &mut Rd::new(&[5])).unwrap(), (0, -5));
        assert_eq!(triplet(1, &mut Rd::new(&[5])).unwrap(), (0, 5));
        // flag 10: dx=-(data), dy=0 ; flag 11: dx=+data
        assert_eq!(triplet(10, &mut Rd::new(&[9])).unwrap(), (-9, 0));
        assert_eq!(triplet(11, &mut Rd::new(&[9])).unwrap(), (9, 0));
        // flag 124: two 16-bit signed-by-flag values
        assert_eq!(triplet(127, &mut Rd::new(&[0x01, 0x00, 0x02, 0x00])).unwrap(), (256, 512));
        assert_eq!(triplet(124, &mut Rd::new(&[0x01, 0x00, 0x02, 0x00])).unwrap(), (-256, -512));
    }
}
