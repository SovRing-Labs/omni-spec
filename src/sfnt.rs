//! sfnt font reading (OpenType / TrueType): table directory, TrueType collections, and the tables needed for
//! layout and metrics: head, hhea, maxp, hmtx, cmap (formats 4 and 12), name (family), OS/2 (vertical metrics).
//!
//! Spec: Microsoft OpenType specification — "OpenType Font File" (chapter 2: Table Directory, TTC header),
//! "head" (unitsPerEm @18, indexToLocFormat @50), "hhea" (numberOfHMetrics @34), "maxp" (numGlyphs @4),
//! "hmtx" (longHorMetric records, §hmtx), "cmap" (subtable formats 4 and 12, §cmap; platform/encoding IDs),
//! "name" (name records, §name), "OS/2" (sTypo*, usWin* metrics). https://learn.microsoft.com/en-us/typography/opentype/spec/
//! Written from the specification text. WOFF2 is decoded by `crate::woff2`, which produces an sfnt byte sequence
//! that this module then reads.
//!
//! All reads are bounds-checked: malformed input returns `Err`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Truncated,
    BadVersion,
    MissingTable([u8; 4]),
    BadFormat(&'static str),
    NoCollectionFont,
}

pub type Result<T> = core::result::Result<T, Error>;

fn u16_at(d: &[u8], off: usize) -> Result<u16> {
    let b = d.get(off..off + 2).ok_or(Error::Truncated)?;
    Ok(u16::from_be_bytes([b[0], b[1]]))
}

fn i16_at(d: &[u8], off: usize) -> Result<i16> {
    Ok(u16_at(d, off)? as i16)
}

fn u32_at(d: &[u8], off: usize) -> Result<u32> {
    let b = d.get(off..off + 4).ok_or(Error::Truncated)?;
    Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// One table directory record (OpenType chapter 2, Table Directory).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableRecord {
    pub tag: [u8; 4],
    pub offset: usize,
    pub length: usize,
}

/// A parsed sfnt font: the full byte slice and its table directory.
#[derive(Debug, Clone)]
pub struct Font<'a> {
    data: &'a [u8],
    tables: Vec<TableRecord>,
}

/// Parse a single-font sfnt (TrueType 0x00010000, 'true', or CFF 'OTTO'). Use `parse_collection` for 'ttcf'.
pub fn parse(data: &[u8]) -> Result<Font<'_>> {
    parse_at(data, 0)
}

/// Parse font `index` of a TrueType collection ('ttcf'), or the single font of a plain sfnt when `index` is 0.
pub fn parse_collection(data: &[u8], index: usize) -> Result<Font<'_>> {
    if data.get(0..4) == Some(b"ttcf") {
        let n = u32_at(data, 8)? as usize;
        if index >= n {
            return Err(Error::NoCollectionFont);
        }
        let off = u32_at(data, 12 + 4 * index)? as usize;
        return parse_at(data, off);
    }
    if index != 0 {
        return Err(Error::NoCollectionFont);
    }
    parse(data)
}

fn parse_at(data: &[u8], offset_table: usize) -> Result<Font<'_>> {
    let version = u32_at(data, offset_table)?;
    if !matches!(version, 0x0001_0000 | 0x4F54_544F | 0x7472_7565) {
        return Err(Error::BadVersion);
    }
    let n = u16_at(data, offset_table + 4)? as usize;
    let mut tables = Vec::with_capacity(n);
    for i in 0..n {
        let rec = offset_table + 12 + 16 * i;
        let tag: [u8; 4] = data.get(rec..rec + 4).ok_or(Error::Truncated)?.try_into().unwrap();
        let offset = u32_at(data, rec + 8)? as usize;
        let length = u32_at(data, rec + 12)? as usize;
        if offset.checked_add(length).map_or(true, |end| end > data.len()) {
            return Err(Error::Truncated);
        }
        tables.push(TableRecord { tag, offset, length });
    }
    Ok(Font { data, tables })
}

impl<'a> Font<'a> {
    pub fn tables(&self) -> &[TableRecord] {
        &self.tables
    }

    /// The bytes of table `tag`, if present.
    pub fn table(&self, tag: &[u8; 4]) -> Option<&'a [u8]> {
        self.tables
            .iter()
            .find(|t| &t.tag == tag)
            .map(|t| &self.data[t.offset..t.offset + t.length])
    }

    fn required(&self, tag: &[u8; 4]) -> Result<&'a [u8]> {
        self.table(tag).ok_or(Error::MissingTable(*tag))
    }

    /// head.unitsPerEm (offset 18).
    pub fn units_per_em(&self) -> Result<u16> {
        u16_at(self.required(b"head")?, 18)
    }

    /// head.indexToLocFormat (offset 50): 0 = short, 1 = long (loca).
    pub fn index_to_loc_format(&self) -> Result<i16> {
        i16_at(self.required(b"head")?, 50)
    }

    /// maxp.numGlyphs (offset 4).
    pub fn num_glyphs(&self) -> Result<u16> {
        u16_at(self.required(b"maxp")?, 4)
    }

    /// hhea.numberOfHMetrics (offset 34).
    pub fn num_h_metrics(&self) -> Result<u16> {
        u16_at(self.required(b"hhea")?, 34)
    }

    /// (advanceWidth, lsb) for glyph `gid` from hmtx. Glyphs past numberOfHMetrics reuse the last advance.
    pub fn h_metrics(&self, gid: u16) -> Result<(u16, i16)> {
        let n = self.num_h_metrics()? as usize;
        if n == 0 {
            return Err(Error::BadFormat("numberOfHMetrics is zero"));
        }
        let hmtx = self.required(b"hmtx")?;
        let g = gid as usize;
        if g < n {
            let adv = u16_at(hmtx, 4 * g)?;
            let lsb = i16_at(hmtx, 4 * g + 2)?;
            Ok((adv, lsb))
        } else {
            let adv = u16_at(hmtx, 4 * (n - 1))?;
            let lsb = i16_at(hmtx, 4 * n + 2 * (g - n))?;
            Ok((adv, lsb))
        }
    }

    /// Advance width of glyph `gid` in font units.
    pub fn advance(&self, gid: u16) -> Result<u16> {
        self.h_metrics(gid).map(|(a, _)| a)
    }

    /// Unicode cmap lookup. Subtable preference (first found wins): (3,10) format 12, (0,4) format 12/any,
    /// (3,1) format 4, (0,3) format 4, (0,0..2) format 4. Returns None for unmapped code points (glyph 0).
    pub fn cmap(&self, codepoint: u32) -> Result<Option<u16>> {
        let cm = self.required(b"cmap")?;
        let n = u16_at(cm, 2)? as usize;
        let mut subtables: Vec<(u16, u16, usize)> = Vec::with_capacity(n);
        for i in 0..n {
            let rec = 4 + 8 * i;
            let pid = u16_at(cm, rec)?;
            let eid = u16_at(cm, rec + 2)?;
            let off = u32_at(cm, rec + 4)? as usize;
            subtables.push((pid, eid, off));
        }
        let rank = |pid: u16, eid: u16| -> u8 {
            match (pid, eid) {
                (3, 10) => 0,
                (0, 4) => 1,
                (3, 1) => 2,
                (0, 3) => 3,
                (0, _) => 4,
                _ => 255,
            }
        };
        let mut best: Option<(u8, usize)> = None;
        for &(pid, eid, off) in &subtables {
            let r = rank(pid, eid);
            if r != 255 && best.map_or(true, |(b, _)| r < b) {
                best = Some((r, off));
            }
        }
        let Some((_, off)) = best else {
            return Ok(None);
        };
        match u16_at(cm, off)? {
            4 => cmap4(cm, off, codepoint),
            12 => cmap12(cm, off, codepoint),
            _ => Err(Error::BadFormat("cmap subtable format (only 4 and 12 are read)")),
        }
    }

    /// Family name (name ID 1), preferring Windows Unicode (platform 3) and falling back to Mac Roman (platform 1).
    pub fn family_name(&self) -> Result<Option<String>> {
        let nm = self.required(b"name")?;
        let count = u16_at(nm, 2)? as usize;
        let string_off = u16_at(nm, 4)? as usize;
        let mut win: Option<String> = None;
        let mut mac: Option<String> = None;
        for i in 0..count {
            let rec = 6 + 12 * i;
            let pid = u16_at(nm, rec)?;
            let name_id = u16_at(nm, rec + 6)?;
            if name_id != 1 {
                continue;
            }
            let len = u16_at(nm, rec + 8)? as usize;
            let off = string_off + u16_at(nm, rec + 10)? as usize;
            let bytes = nm.get(off..off + len).ok_or(Error::Truncated)?;
            match pid {
                3 if win.is_none() => {
                    let units: Vec<u16> = bytes.chunks(2).filter(|c| c.len() == 2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
                    win = Some(String::from_utf16_lossy(&units));
                }
                1 if mac.is_none() => mac = Some(bytes.iter().map(|&b| b as char).collect()),
                _ => {}
            }
        }
        Ok(win.or(mac))
    }

    /// Typographic metrics from OS/2 (sTypoAscender @68, sTypoDescender @70, sTypoLineGap @72, usWinAscent @74,
    /// usWinDescent @76), and usWeightClass @4.
    pub fn os2_metrics(&self) -> Result<Os2Metrics> {
        let t = self.required(b"OS/2")?;
        Ok(Os2Metrics {
            weight_class: u16_at(t, 4)?,
            typo_ascender: i16_at(t, 68)?,
            typo_descender: i16_at(t, 70)?,
            typo_line_gap: i16_at(t, 72)?,
            win_ascent: u16_at(t, 74)?,
            win_descent: u16_at(t, 76)?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Os2Metrics {
    pub weight_class: u16,
    pub typo_ascender: i16,
    pub typo_descender: i16,
    pub typo_line_gap: i16,
    pub win_ascent: u16,
    pub win_descent: u16,
}

/// cmap format 4 (segment mapping to delta values), §cmap format 4.
fn cmap4(cm: &[u8], off: usize, c: u32) -> Result<Option<u16>> {
    if c > 0xFFFF {
        return Ok(None);
    }
    let c = c as u16;
    let seg_count = u16_at(cm, off + 6)? as usize / 2;
    let end_codes = off + 14;
    let start_codes = end_codes + 2 * seg_count + 2; // + reservedPad
    let id_delta = start_codes + 2 * seg_count;
    let id_range_offset = id_delta + 2 * seg_count;
    // Segments are sorted by endCode: binary search for the first segment with endCode >= c.
    let (mut lo, mut hi) = (0usize, seg_count);
    while lo < hi {
        let mid = (lo + hi) / 2;
        if u16_at(cm, end_codes + 2 * mid)? < c {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    if lo == seg_count {
        return Ok(None);
    }
    let i = lo;
    let start = u16_at(cm, start_codes + 2 * i)?;
    if c < start {
        return Ok(None);
    }
    let delta = u16_at(cm, id_delta + 2 * i)?;
    let range = u16_at(cm, id_range_offset + 2 * i)?;
    let gid = if range == 0 {
        c.wrapping_add(delta)
    } else {
        let addr = id_range_offset + 2 * i + range as usize + 2 * (c - start) as usize;
        let g = u16_at(cm, addr)?;
        if g == 0 {
            0
        } else {
            g.wrapping_add(delta)
        }
    };
    Ok(if gid == 0 { None } else { Some(gid) })
}

/// cmap format 12 (segmented coverage, 32-bit), §cmap format 12.
fn cmap12(cm: &[u8], off: usize, c: u32) -> Result<Option<u16>> {
    let n = u32_at(cm, off + 12)? as usize;
    let (mut lo, mut hi) = (0usize, n);
    while lo < hi {
        let mid = (lo + hi) / 2;
        let rec = off + 16 + 12 * mid;
        let start = u32_at(cm, rec)?;
        let end = u32_at(cm, rec + 4)?;
        if c < start {
            hi = mid;
        } else if c > end {
            lo = mid + 1;
        } else {
            let start_gid = u32_at(cm, rec + 8)?;
            let gid = start_gid.wrapping_add(c - start);
            return Ok(if gid == 0 || gid > 0xFFFF { None } else { Some(gid as u16) });
        }
    }
    Ok(None)
}
