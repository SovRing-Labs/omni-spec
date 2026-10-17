//! ZIP archives — read and write, from PKWARE APPNOTE.TXT 6.3.10 (the .ZIP File Format Specification).
//!
//! Read: end of central directory (§4.3.16), ZIP64 end of central directory locator + record (§4.3.15, §4.3.14),
//! central directory headers (§4.3.12), local file headers (§4.3.7), the ZIP64 extended information extra field
//! (§4.5.3), the Info-ZIP Unicode Path extra field (§4.6.9), compression methods 0 (stored) and 8 (deflate, RFC 1951,
//! via `crate::inflate`), CRC-32 check (§4.4.7). Entry names: UTF-8 when general purpose bit 11 is set (§4.4.4,
//! Appendix D), else IBM code page 437 (Appendix D).
//! Write: stored entries (method 0) with CRC-32 and sizes in the local header (no data descriptor), central directory,
//! end of central directory. That is the subset Office Open XML packages need (ECMA-376 Part 2 §8.2 / Annex C).
//! Not supported: encryption (bit 0), multi-disk archives, methods other than 0 and 8, ZIP64 on write.

use crate::png::crc32 as crc32_parts;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// No end of central directory record was found (§4.3.16).
    NotZip,
    Truncated,
    BadSignature(&'static str),
    /// Archive spans several disks (§4.3.16 "number of this disk" != 0).
    MultiDisk,
    Encrypted(String),
    UnsupportedMethod(String, u16),
    BadCrc(String),
    BadSize(String),
    Inflate(String, crate::inflate::Error),
    NotFound(String),
    /// Declared or produced size is over the caller's limit.
    TooLarge(String),
    /// Write side: a field does not fit the classic (non-ZIP64) record.
    Overflow,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// CRC-32 (ISO 3309 polynomial 0xEDB88320, reflected; APPNOTE §4.4.7) over one buffer.
pub fn crc32(data: &[u8]) -> u32 {
    crc32_parts(&[data])
}

const SIG_LOCAL: u32 = 0x0403_4b50; // §4.3.7
const SIG_CENTRAL: u32 = 0x0201_4b50; // §4.3.12
const SIG_EOCD: u32 = 0x0605_4b50; // §4.3.16
const SIG_EOCD64: u32 = 0x0606_4b50; // §4.3.14
const SIG_EOCD64_LOC: u32 = 0x0706_4b50; // §4.3.15

/// Default cap on one entry's uncompressed size (512 MiB) — a zip bomb guard; see `Archive::read_limited`.
pub const MAX_ENTRY: u64 = 512 << 20;

fn u16le(d: &[u8], p: usize) -> Result<u16> {
    d.get(p..p + 2).map(|b| u16::from_le_bytes([b[0], b[1]])).ok_or(Error::Truncated)
}
fn u32le(d: &[u8], p: usize) -> Result<u32> {
    d.get(p..p + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok_or(Error::Truncated)
}
fn u64le(d: &[u8], p: usize) -> Result<u64> {
    d.get(p..p + 8).map(|b| u64::from_le_bytes(b.try_into().unwrap())).ok_or(Error::Truncated)
}

/// IBM PC code page 437, bytes 0x80..=0xFF (APPNOTE Appendix D: the default name encoding when bit 11 is clear).
const CP437_HIGH: &str = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■\u{a0}";

pub fn decode_cp437(b: &[u8]) -> String {
    let high: Vec<char> = CP437_HIGH.chars().collect();
    b.iter().map(|&c| if c < 0x80 { c as char } else { high[(c - 0x80) as usize] }).collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub method: u16,
    pub flags: u16,
    pub crc32: u32,
    pub compressed_size: u64,
    pub size: u64,
    /// Offset of the local file header (§4.3.12 "relative offset of local header").
    pub local_offset: u64,
    /// MS-DOS date and time fields (§4.4.6), raw.
    pub dos_date: u16,
    pub dos_time: u16,
}

impl Entry {
    pub fn is_dir(&self) -> bool {
        self.name.ends_with('/')
    }
    /// (year, month, day, hour, minute, second) from the MS-DOS fields (§4.4.6; seconds have 2 s resolution).
    pub fn modified(&self) -> (u16, u8, u8, u8, u8, u8) {
        let d = self.dos_date;
        let t = self.dos_time;
        (1980 + (d >> 9), ((d >> 5) & 0xF) as u8, (d & 0x1F) as u8, (t >> 11) as u8, ((t >> 5) & 0x3F) as u8, ((t & 0x1F) * 2) as u8)
    }
    /// True when the name is a relative path that stays inside an extraction root (no "..", no leading "/" or drive).
    pub fn is_safe_name(&self) -> bool {
        let n = self.name.replace('\\', "/");
        !(n.starts_with('/') || n.split('/').any(|s| s == "..") || n.get(1..2) == Some(":"))
    }
}

/// A parsed archive over borrowed bytes.
pub struct Archive<'a> {
    data: &'a [u8],
    entries: Vec<Entry>,
    pub comment: Vec<u8>,
}

impl<'a> Archive<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Archive<'a>> {
        // §4.3.16: the EOCD record is 22 bytes plus a comment of up to 65,535 bytes, at the end of the file.
        if data.len() < 22 {
            return Err(Error::NotZip);
        }
        let lo = data.len().saturating_sub(22 + 0xFFFF);
        let mut eocd = None;
        let mut p = data.len() - 22;
        loop {
            if u32le(data, p)? == SIG_EOCD {
                let clen = u16le(data, p + 20)? as usize;
                if p + 22 + clen == data.len() {
                    eocd = Some(p);
                    break;
                }
            }
            if p == lo {
                break;
            }
            p -= 1;
        }
        // A trailing-garbage archive: accept the last signature whose comment still fits in the file.
        if eocd.is_none() {
            let mut p = data.len() - 22;
            loop {
                if u32le(data, p)? == SIG_EOCD && p + 22 + u16le(data, p + 20)? as usize <= data.len() {
                    eocd = Some(p);
                    break;
                }
                if p == lo {
                    break;
                }
                p -= 1;
            }
        }
        let e = eocd.ok_or(Error::NotZip)?;
        let disk = u16le(data, e + 4)?;
        let cd_disk = u16le(data, e + 6)?;
        let mut total = u16le(data, e + 10)? as u64;
        let mut cd_size = u32le(data, e + 12)? as u64;
        let mut cd_off = u32le(data, e + 16)? as u64;
        let clen = u16le(data, e + 20)? as usize;
        let comment = data.get(e + 22..(e + 22 + clen).min(data.len())).unwrap_or(&[]).to_vec();
        // §4.3.15: the ZIP64 locator sits immediately before the EOCD record.
        let mut anchor = e as u64; // where the central directory is expected to end
        if e >= 20 && u32le(data, e - 20)? == SIG_EOCD64_LOC {
            let off64 = u64le(data, e - 20 + 8)? as usize;
            if u32le(data, off64)? != SIG_EOCD64 {
                return Err(Error::BadSignature("zip64 end of central directory"));
            }
            if u32le(data, off64 + 16)? != 0 {
                return Err(Error::MultiDisk);
            }
            total = u64le(data, off64 + 32)?;
            cd_size = u64le(data, off64 + 40)?;
            cd_off = u64le(data, off64 + 48)?;
            anchor = off64 as u64;
        } else if disk != 0 || cd_disk != 0 {
            return Err(Error::MultiDisk);
        }
        // Archives with a prefix (self-extractors): central directory offsets are relative to the archive start.
        let cd_end = cd_off.checked_add(cd_size).ok_or(Error::Truncated)?;
        let shift = anchor.checked_sub(cd_end).unwrap_or(0);
        let mut entries = Vec::with_capacity(total.min(1 << 16) as usize);
        let mut p = usize::try_from(cd_off + shift).map_err(|_| Error::Truncated)?;
        let end = p.checked_add(cd_size as usize).ok_or(Error::Truncated)?;
        if end > data.len() {
            return Err(Error::Truncated);
        }
        while p < end {
            if u32le(data, p)? != SIG_CENTRAL {
                return Err(Error::BadSignature("central directory header"));
            }
            let flags = u16le(data, p + 8)?;
            let method = u16le(data, p + 10)?;
            let dos_time = u16le(data, p + 12)?;
            let dos_date = u16le(data, p + 14)?;
            let crc = u32le(data, p + 16)?;
            let mut csize = u32le(data, p + 20)? as u64;
            let mut usize_ = u32le(data, p + 24)? as u64;
            let nlen = u16le(data, p + 28)? as usize;
            let xlen = u16le(data, p + 30)? as usize;
            let klen = u16le(data, p + 32)? as usize;
            let mut disk_start = u16le(data, p + 34)? as u32;
            let mut loff = u32le(data, p + 42)? as u64;
            let raw_name = data.get(p + 46..p + 46 + nlen).ok_or(Error::Truncated)?;
            let extra = data.get(p + 46 + nlen..p + 46 + nlen + xlen).ok_or(Error::Truncated)?;
            let mut name =
                if flags & (1 << 11) != 0 { String::from_utf8_lossy(raw_name).into_owned() } else { decode_cp437(raw_name) };
            // Extra fields (§4.5.1): header id u16, data size u16, data.
            let mut x = 0;
            while x + 4 <= extra.len() {
                let id = u16le(extra, x)?;
                let sz = u16le(extra, x + 2)? as usize;
                let body = extra.get(x + 4..x + 4 + sz).ok_or(Error::Truncated)?;
                match id {
                    0x0001 => {
                        // §4.5.3: present only for the fields whose 32-bit value is 0xFFFFFFFF, in this order.
                        let mut q = 0;
                        if usize_ == 0xFFFF_FFFF {
                            usize_ = u64le(body, q)?;
                            q += 8;
                        }
                        if csize == 0xFFFF_FFFF {
                            csize = u64le(body, q)?;
                            q += 8;
                        }
                        if loff == 0xFFFF_FFFF {
                            loff = u64le(body, q)?;
                            q += 8;
                        }
                        if disk_start == 0xFFFF {
                            disk_start = u32le(body, q)?;
                        }
                    }
                    0x7075 if body.len() >= 5 && body[0] == 1 => {
                        // §4.6.9 Info-ZIP Unicode Path: version 1, CRC-32 of the header name, UTF-8 name.
                        if u32le(body, 1)? == crc32(raw_name) {
                            if let Ok(s) = std::str::from_utf8(&body[5..]) {
                                name = s.to_string();
                            }
                        }
                    }
                    _ => {}
                }
                x += 4 + sz;
            }
            if disk_start != 0 {
                return Err(Error::MultiDisk);
            }
            entries.push(Entry {
                name,
                method,
                flags,
                crc32: crc,
                compressed_size: csize,
                size: usize_,
                local_offset: loff.saturating_add(shift),
                dos_date,
                dos_time,
            });
            p += 46 + nlen + xlen + klen;
        }
        Ok(Archive { data, entries, comment })
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn find(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// Case-insensitive lookup (OPC part names compare case-insensitively, ECMA-376 Part 2 §9.1.1.1).
    pub fn find_ci(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.name.eq_ignore_ascii_case(name))
    }

    /// Raw (still compressed) bytes of an entry, located through its local header (§4.3.7: the local name and
    /// extra lengths may differ from the central directory's).
    pub fn raw(&self, e: &Entry) -> Result<&'a [u8]> {
        let p = usize::try_from(e.local_offset).map_err(|_| Error::Truncated)?;
        if u32le(self.data, p)? != SIG_LOCAL {
            return Err(Error::BadSignature("local file header"));
        }
        let nlen = u16le(self.data, p + 26)? as usize;
        let xlen = u16le(self.data, p + 28)? as usize;
        let start = p + 30 + nlen + xlen;
        let end = usize::try_from(e.compressed_size).ok().and_then(|c| start.checked_add(c)).ok_or(Error::Truncated)?;
        self.data.get(start..end).ok_or(Error::Truncated)
    }

    pub fn read(&self, e: &Entry) -> Result<Vec<u8>> {
        self.read_limited(e, MAX_ENTRY)
    }

    /// Decompress and CRC-check one entry, refusing anything whose declared or produced size exceeds `limit`.
    pub fn read_limited(&self, e: &Entry, limit: u64) -> Result<Vec<u8>> {
        if e.flags & 1 != 0 {
            return Err(Error::Encrypted(e.name.clone()));
        }
        if e.size > limit {
            return Err(Error::TooLarge(e.name.clone()));
        }
        let raw = self.raw(e)?;
        let out = match e.method {
            0 => {
                if e.compressed_size != e.size {
                    return Err(Error::BadSize(e.name.clone()));
                }
                raw.to_vec()
            }
            8 => {
                let (v, _) = crate::inflate::inflate_raw(raw, e.size as usize).map_err(|x| match x {
                    crate::inflate::Error::OutputLimit => Error::BadSize(e.name.clone()),
                    x => Error::Inflate(e.name.clone(), x),
                })?;
                v
            }
            m => return Err(Error::UnsupportedMethod(e.name.clone(), m)),
        };
        if out.len() as u64 != e.size {
            return Err(Error::BadSize(e.name.clone()));
        }
        if crc32(&out) != e.crc32 {
            return Err(Error::BadCrc(e.name.clone()));
        }
        Ok(out)
    }

    pub fn read_name(&self, name: &str) -> Result<Vec<u8>> {
        let e = self.find(name).ok_or_else(|| Error::NotFound(name.to_string()))?;
        self.read(e)
    }
}

/// MS-DOS date and time fields (§4.4.6) for a calendar time; years before 1980 clamp to 1980-01-01 00:00:00.
pub fn dos_datetime(year: u16, month: u8, day: u8, hour: u8, min: u8, sec: u8) -> (u16, u16) {
    if year < 1980 {
        return (0x21, 0);
    }
    let date = ((year - 1980).min(127) << 9) | ((month as u16 & 0xF) << 5) | (day as u16 & 0x1F);
    let time = ((hour as u16) << 11) | ((min as u16) << 5) | (sec as u16 / 2);
    (date, time)
}

/// Writes a ZIP with stored (method 0) or deflated (method 8) entries. Sizes and CRC go in the local header, so no data descriptor is used.
pub struct Writer {
    out: Vec<u8>,
    central: Vec<u8>,
    count: u64,
    date: u16,
    time: u16,
}

impl Default for Writer {
    fn default() -> Self {
        Self::new()
    }
}

impl Writer {
    /// Entries stamped 1980-01-01 00:00 (deterministic output) unless `set_time` is called.
    pub fn new() -> Writer {
        Writer { out: Vec::new(), central: Vec::new(), count: 0, date: 0x21, time: 0 }
    }

    pub fn set_time(&mut self, year: u16, month: u8, day: u8, hour: u8, min: u8, sec: u8) {
        let (d, t) = dos_datetime(year, month, day, hour, min, sec);
        self.date = d;
        self.time = t;
    }

    pub fn add_stored(&mut self, name: &str, data: &[u8]) -> Result<()> {
        self.add_entry(name, data, 0, data)
    }

    /// Method 8 (deflate, RFC 1951 via `deflate.rs`); falls back to stored when deflating does not shrink the entry.
    pub fn add_deflated(&mut self, name: &str, data: &[u8]) -> Result<()> {
        match crate::deflate::deflate_raw(data, data.len()) {
            Ok(c) if c.len() < data.len() => self.add_entry(name, data, 8, &c),
            _ => self.add_entry(name, data, 0, data),
        }
    }

    /// `payload` is what goes in the archive for `data`: the bytes themselves (method 0) or their deflate stream (method 8).
    fn add_entry(&mut self, name: &str, data: &[u8], method: u16, payload: &[u8]) -> Result<()> {
        let off = self.out.len() as u64;
        if off > 0xFFFF_FFFE || data.len() as u64 > 0xFFFF_FFFE || name.len() > 0xFFFF || self.count >= 0xFFFF {
            return Err(Error::Overflow);
        }
        let crc = crc32(data);
        let flags: u16 = if name.is_ascii() { 0 } else { 1 << 11 }; // §4.4.4 bit 11: name is UTF-8
        let n = data.len() as u32;
        let cn = payload.len() as u32;
        let version: u16 = if method == 8 { 20 } else { 10 }; // §4.4.3: 2.0 for deflate, 1.0 for stored
        // §4.3.7 local file header
        let o = &mut self.out;
        o.extend(SIG_LOCAL.to_le_bytes());
        o.extend(version.to_le_bytes());
        o.extend(flags.to_le_bytes());
        o.extend(method.to_le_bytes());
        o.extend(self.time.to_le_bytes());
        o.extend(self.date.to_le_bytes());
        o.extend(crc.to_le_bytes());
        o.extend(cn.to_le_bytes());
        o.extend(n.to_le_bytes());
        o.extend((name.len() as u16).to_le_bytes());
        o.extend(0u16.to_le_bytes());
        o.extend(name.as_bytes());
        o.extend(payload);
        // §4.3.12 central directory header
        let c = &mut self.central;
        c.extend(SIG_CENTRAL.to_le_bytes());
        c.extend(20u16.to_le_bytes()); // made by: MS-DOS (0), spec 2.0
        c.extend(version.to_le_bytes());
        c.extend(flags.to_le_bytes());
        c.extend(method.to_le_bytes());
        c.extend(self.time.to_le_bytes());
        c.extend(self.date.to_le_bytes());
        c.extend(crc.to_le_bytes());
        c.extend(cn.to_le_bytes());
        c.extend(n.to_le_bytes());
        c.extend((name.len() as u16).to_le_bytes());
        c.extend(0u16.to_le_bytes()); // extra
        c.extend(0u16.to_le_bytes()); // comment
        c.extend(0u16.to_le_bytes()); // disk start
        c.extend(0u16.to_le_bytes()); // internal attributes
        c.extend((if name.ends_with('/') { 0x10u32 } else { 0 }).to_le_bytes()); // external: MS-DOS directory bit
        c.extend((off as u32).to_le_bytes());
        c.extend(name.as_bytes());
        self.count += 1;
        Ok(())
    }

    pub fn add_dir(&mut self, name: &str) -> Result<()> {
        let n = if name.ends_with('/') { name.to_string() } else { format!("{name}/") };
        self.add_stored(&n, &[])
    }

    pub fn finish(mut self) -> Result<Vec<u8>> {
        let cd_off = self.out.len() as u64;
        let cd_size = self.central.len() as u64;
        if cd_off + cd_size > 0xFFFF_FFFE {
            return Err(Error::Overflow);
        }
        self.out.extend_from_slice(&self.central);
        // §4.3.16 end of central directory record
        let o = &mut self.out;
        o.extend(SIG_EOCD.to_le_bytes());
        o.extend(0u16.to_le_bytes());
        o.extend(0u16.to_le_bytes());
        o.extend((self.count as u16).to_le_bytes());
        o.extend((self.count as u16).to_le_bytes());
        o.extend((cd_size as u32).to_le_bytes());
        o.extend((cd_off as u32).to_le_bytes());
        o.extend(0u16.to_le_bytes());
        Ok(self.out)
    }
}
