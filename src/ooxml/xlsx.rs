//! SpreadsheetML (.xlsx) — ECMA-376 5th ed. Part 1 §18.
//!
//! Read: workbook sheets + state (§18.2.19–20), date system (`workbookPr` date1904 / dateCompatibility, §18.2.28,
//! §18.17.4.1), shared string table with rich runs, phonetic runs skipped (§18.4.8–9, §18.4.6), cell types b d e
//! inlineStr n s str (§18.18.11 ST_CellType), cached formula values (`v` beside `f`, §18.3.1.40; formulas are never
//! evaluated), number formats: built-in ids (§18.8.30) and custom codes (§18.8.31) decide whether a number is a date
//! or time, rendered as ISO 8601 text. ST_Xstring `_xHHHH_` escapes (§22.9.2.19) are decoded. Merged cells: the value
//! stays in the top-left cell (§18.3.1.55), the others read as empty.
//! Write: several sheets of typed cells (shared strings, numbers, booleans, errors, dates as serials with a date
//! format), a bold header row frozen in place (§18.3.1.66 pane), column widths (§18.3.1.13), autofilter (§18.3.1.2).

use super::opc::{rel, Package, PackageWriter};
use super::Error;
use crate::xml::{self, Event, Reader, Writer};

pub const NS_MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
pub const NS_MAIN_STRICT: &str = "http://purl.oclc.org/ooxml/spreadsheetml/main";
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NS_R_STRICT: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships";

#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Empty,
    Text(String),
    Number(f64),
    Bool(bool),
    /// Error literal such as "#DIV/0!" (§18.18.11 e).
    Error(String),
    /// ISO 8601: "YYYY-MM-DD", "YYYY-MM-DDTHH:MM:SS" or "HH:MM:SS".
    Date(String),
}

impl Cell {
    pub fn is_empty(&self) -> bool {
        matches!(self, Cell::Empty) || matches!(self, Cell::Text(s) if s.is_empty())
    }
    /// Display text: numbers without a trailing ".0", booleans TRUE/FALSE.
    pub fn as_text(&self) -> String {
        match self {
            Cell::Empty => String::new(),
            Cell::Text(s) | Cell::Error(s) | Cell::Date(s) => s.clone(),
            Cell::Number(n) => fmt_number(*n),
            Cell::Bool(b) => if *b { "TRUE".into() } else { "FALSE".into() },
        }
    }
}

/// Shortest round-trip decimal; integers without ".0".
pub fn fmt_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 { format!("{}", n as i64) } else { format!("{n}") }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sheet {
    pub name: String,
    /// "visible", "hidden" or "veryHidden" (§18.18.68).
    pub state: String,
    /// Dense rows from A1; each row ends at its last non-empty cell; trailing empty rows dropped.
    pub rows: Vec<Vec<Cell>>,
}

impl Sheet {
    pub fn cell(&self, row: usize, col: usize) -> &Cell {
        self.rows.get(row).and_then(|r| r.get(col)).unwrap_or(&Cell::Empty)
    }
    pub fn width(&self) -> usize {
        self.rows.iter().map(|r| r.len()).max().unwrap_or(0)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Workbook {
    pub sheets: Vec<Sheet>,
    pub date1904: bool,
}

impl Workbook {
    pub fn sheet(&self, name: &str) -> Option<&Sheet> {
        self.sheets.iter().find(|s| s.name == name)
    }
}

/// Column letters → 0-based index ("A" = 0, "AA" = 26), §18.18.62 ST_Ref.
pub fn col_index(letters: &str) -> Option<usize> {
    if letters.is_empty() || letters.len() > 3 {
        return None;
    }
    let mut n = 0usize;
    for c in letters.bytes() {
        if !c.is_ascii_alphabetic() {
            return None;
        }
        n = n * 26 + (c.to_ascii_uppercase() - b'A' + 1) as usize;
    }
    Some(n - 1)
}

pub fn col_letters(mut i: usize) -> String {
    let mut s = Vec::new();
    loop {
        s.push(b'A' + (i % 26) as u8);
        if i < 26 {
            break;
        }
        i = i / 26 - 1;
    }
    s.reverse();
    String::from_utf8(s).unwrap()
}

/// "B12" → (11, 1) as (row, col), 0-based.
pub fn parse_ref(r: &str) -> Option<(usize, usize)> {
    let r = r.replace('$', "");
    let split = r.find(|c: char| c.is_ascii_digit())?;
    let col = col_index(&r[..split])?;
    let row: usize = r[split..].parse().ok()?;
    if row == 0 {
        return None;
    }
    Some((row - 1, col))
}

pub fn cell_ref(row: usize, col: usize) -> String {
    format!("{}{}", col_letters(col), row + 1)
}

/// §22.9.2.19 ST_Xstring: `_xHHHH_` stands for the UTF-16 code unit HHHH.
pub fn decode_xstring(s: &str) -> String {
    if !s.contains("_x") {
        return s.to_string();
    }
    let b = s.as_bytes();
    let mut units: Vec<u16> = Vec::new();
    let mut out = String::new();
    let mut i = 0;
    let flush = |units: &mut Vec<u16>, out: &mut String| {
        if !units.is_empty() {
            out.push_str(&String::from_utf16_lossy(units));
            units.clear();
        }
    };
    while i < b.len() {
        if b[i] == b'_' && i + 7 <= b.len() && b[i + 1] == b'x' && b[i + 6] == b'_' {
            if let Ok(u) = u16::from_str_radix(&s[i + 2..i + 6], 16) {
                units.push(u);
                i += 7;
                continue;
            }
        }
        flush(&mut units, &mut out);
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    flush(&mut units, &mut out);
    out
}

/// Inverse of `decode_xstring` for writing: characters XML 1.0 cannot carry, and a literal `_xHHHH_`, are escaped.
pub fn encode_xstring(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    for ch in s.chars() {
        let looks_escaped = ch == '_' && i + 7 <= b.len() && b[i + 1] == b'x' && b[i + 6] == b'_' && s[i + 2..i + 6].chars().all(|c| c.is_ascii_hexdigit());
        if looks_escaped {
            out.push_str("_x005F_");
        } else if xml::is_xml_char(ch) {
            out.push(ch);
        } else {
            let mut buf = [0u16; 2];
            for u in ch.encode_utf16(&mut buf) {
                out.push_str(&format!("_x{u:04X}_"));
            }
        }
        i += ch.len_utf8();
    }
    out
}

// ---- dates (§18.17.4.1: serial values; 1900 backward-compatibility system keeps the fictitious 1900-02-29) -------

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    // Gregorian calendar; days since 1970-01-01, computed through a year that starts in March
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateSystem {
    /// 1900 backward compatibility (default): serial 1 = 1900-01-01, serial 60 = the fictitious 1900-02-29.
    Compat1900,
    /// dateCompatibility="0": serial 0 = 1899-12-30 throughout.
    Base1899,
    /// date1904="1": serial 0 = 1904-01-01.
    Base1904,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateKind {
    Date,
    DateTime,
    Time,
}

/// Serial value → ISO 8601 text (seconds rounded).
pub fn serial_to_iso(serial: f64, sys: DateSystem, kind: DateKind) -> String {
    let mut days = serial.floor() as i64;
    let mut secs = ((serial - serial.floor()) * 86400.0).round() as i64;
    if secs >= 86400 {
        days += 1;
        secs -= 86400;
    }
    let time = format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60);
    if kind == DateKind::Time {
        return time;
    }
    let date = match sys {
        DateSystem::Compat1900 if days == 60 => "1900-02-29".to_string(),
        _ => {
            let base = match sys {
                DateSystem::Compat1900 if days < 60 => days_from_civil(1899, 12, 31),
                DateSystem::Compat1900 | DateSystem::Base1899 => days_from_civil(1899, 12, 30),
                DateSystem::Base1904 => days_from_civil(1904, 1, 1),
            };
            let (y, m, d) = civil_from_days(base + days);
            format!("{y:04}-{m:02}-{d:02}")
        }
    };
    if kind == DateKind::DateTime { format!("{date}T{time}") } else { date }
}

/// ISO 8601 text ("YYYY-MM-DD", "YYYY-MM-DD[T ]HH:MM[:SS]", "HH:MM[:SS]") → (serial, kind) in the 1900 system.
pub fn iso_to_serial(s: &str) -> Option<(f64, DateKind)> {
    let s = s.trim();
    let num = |t: &str| -> Option<i64> { if !t.is_empty() && t.bytes().all(|c| c.is_ascii_digit()) { t.parse().ok() } else { None } };
    let time = |t: &str| -> Option<f64> {
        let p: Vec<&str> = t.trim_end_matches('Z').split(':').collect();
        if p.len() < 2 || p.len() > 3 {
            return None;
        }
        let h = num(p[0])?;
        let m = num(p[1])?;
        let sec: f64 = if p.len() == 3 { p[2].parse().ok()? } else { 0.0 };
        if h > 23 || m > 59 || !(0.0..60.0).contains(&sec) {
            return None;
        }
        Some((h as f64 * 3600.0 + m as f64 * 60.0 + sec) / 86400.0)
    };
    if s.len() >= 5 && s.as_bytes()[2] == b':' {
        return time(s).map(|t| (t, DateKind::Time));
    }
    let (d, t) = match s.find(['T', ' ']) {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    let p: Vec<&str> = d.split('-').collect();
    if p.len() != 3 || p[0].len() != 4 {
        return None;
    }
    let (y, m, dd) = (num(p[0])?, num(p[1])?, num(p[2])?);
    if !(1..=12).contains(&m) || dd < 1 || dd > 31 || y < 1900 {
        return None;
    }
    let days = days_from_civil(y, m, dd);
    if civil_from_days(days) != (y, m, dd) {
        return None; // e.g. 2026-02-30
    }
    let mut serial = (days - days_from_civil(1899, 12, 30)) as f64;
    if serial < 61.0 {
        serial -= 1.0; // before 1900-03-01 the compat system is one day behind (no fictitious leap day yet)
    }
    match t {
        None => Some((serial, DateKind::Date)),
        Some(t) => Some((serial + time(t)?, DateKind::DateTime)),
    }
}

/// Built-in number format ids that show dates or times (§18.8.30; 27–36 and 50–58 are East Asian date formats).
fn builtin_kind(id: u32) -> Option<DateKind> {
    match id {
        14..=17 | 27..=31 | 34..=36 | 50..=58 => Some(DateKind::Date),
        22 => Some(DateKind::DateTime),
        18..=21 | 32 | 33 | 45..=47 => Some(DateKind::Time),
        _ => None,
    }
}

/// Classify a custom format code (§18.8.31): date/time tokens outside quoted text, escapes and bracketed sections.
/// Elapsed-time codes ([h], [mm], [ss]) are durations and stay numbers.
pub fn format_kind(code: &str) -> Option<DateKind> {
    let section = code.split(';').next().unwrap_or("");
    let mut plain = String::new();
    let mut it = section.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '"' => {
                for d in it.by_ref() {
                    if d == '"' {
                        break;
                    }
                }
            }
            '\\' | '_' | '*' => {
                it.next();
            }
            '[' => {
                let mut inner = String::new();
                for d in it.by_ref() {
                    if d == ']' {
                        break;
                    }
                    inner.push(d);
                }
                let l = inner.to_ascii_lowercase();
                if !l.is_empty() && l.chars().all(|c| matches!(c, 'h' | 'm' | 's')) {
                    return None; // elapsed time
                }
            }
            c => plain.push(c.to_ascii_lowercase()),
        }
    }
    if plain.contains("general") {
        return None;
    }
    let has_date = plain.contains('y') || plain.contains('d');
    let has_time = plain.contains('h') || plain.contains('s');
    let has_m = plain.contains('m');
    match (has_date || (has_m && !has_time), has_time) {
        (true, true) => Some(DateKind::DateTime),
        (true, false) => Some(DateKind::Date),
        (false, true) => Some(DateKind::Time),
        _ => None,
    }
}

// ---- reading ------------------------------------------------------------------------------------------------------

fn is_main(ns: &str) -> bool {
    ns == NS_MAIN || ns == NS_MAIN_STRICT
}

fn shared_strings(src: &[u8]) -> Result<Vec<String>, Error> {
    let s = xml::decode(src)?;
    let mut r = Reader::new(&s);
    let mut out = Vec::new();
    let mut cur: Option<String> = None;
    while let Some(ev) = r.next()? {
        match ev {
            Event::Start { name, .. } if is_main(&name.ns) && name.local == "si" => cur = Some(String::new()),
            Event::Start { name, .. } if is_main(&name.ns) && name.local == "rPh" => r.skip()?,
            Event::Start { name, .. } if is_main(&name.ns) && name.local == "t" => {
                let t = r.read_text()?;
                if let Some(c) = cur.as_mut() {
                    c.push_str(&decode_xstring(&t));
                }
            }
            Event::End { name } if is_main(&name.ns) && name.local == "si" => out.push(cur.take().unwrap_or_default()),
            _ => {}
        }
    }
    Ok(out)
}

struct Styles {
    /// per cellXfs index: date kind of its number format, if any
    kinds: Vec<Option<DateKind>>,
}

fn styles(src: &[u8]) -> Result<Styles, Error> {
    let root = xml::parse_bytes(src)?;
    let mut custom: Vec<(u32, String)> = Vec::new();
    if let Some(nf) = root.elements().find(|e| e.name.local == "numFmts") {
        for f in nf.elements() {
            if let (Some(id), Some(code)) = (f.attr("numFmtId").and_then(|v| v.parse().ok()), f.attr("formatCode")) {
                custom.push((id, code.to_string()));
            }
        }
    }
    let mut kinds = Vec::new();
    if let Some(xfs) = root.elements().find(|e| e.name.local == "cellXfs") {
        for xf in xfs.elements() {
            let id: u32 = xf.attr("numFmtId").and_then(|v| v.parse().ok()).unwrap_or(0);
            let k = match custom.iter().find(|(i, _)| *i == id) {
                Some((_, code)) => format_kind(code),
                None => builtin_kind(id),
            };
            kinds.push(k);
        }
    }
    Ok(Styles { kinds })
}

fn read_sheet_xml(src: &[u8], sst: &[String], st: &Styles, sys: DateSystem) -> Result<Vec<Vec<Cell>>, Error> {
    let s = xml::decode(src)?;
    let mut r = Reader::new(&s);
    let mut rows: Vec<Vec<Cell>> = Vec::new();
    let mut row_i: usize = 0;
    let mut next_row: usize = 0;
    let mut col_i: usize = 0;
    let mut in_data = false;
    // current cell
    let mut c_t = String::new();
    let mut c_s: usize = 0;
    let mut c_v: Option<String> = None;
    let mut c_is: Option<String> = None;
    while let Some(ev) = r.next()? {
        match ev {
            Event::Start { name, attrs, .. } if is_main(&name.ns) => match name.local.as_str() {
                "sheetData" => in_data = true,
                "row" if in_data => {
                    row_i = attrs.iter().find(|a| a.name.local == "r").and_then(|a| a.value.parse::<usize>().ok()).map(|v| v.saturating_sub(1)).unwrap_or(next_row);
                    next_row = row_i + 1;
                    col_i = 0;
                }
                "c" if in_data => {
                    c_t = "n".into();
                    c_s = 0;
                    c_v = None;
                    c_is = None;
                    for a in &attrs {
                        match a.name.local.as_str() {
                            "r" => {
                                if let Some((rr, cc)) = parse_ref(&a.value) {
                                    row_i = rr;
                                    col_i = cc;
                                }
                            }
                            "t" => c_t = a.value.clone(),
                            "s" => c_s = a.value.parse().unwrap_or(0),
                            _ => {}
                        }
                    }
                }
                "v" if in_data => c_v = Some(r.read_text()?),
                "is" if in_data => {
                    // inline rich text: concatenate t, skip rPh
                    let mut acc = String::new();
                    let d = r.depth();
                    loop {
                        match r.next()? {
                            Some(Event::Start { name, .. }) if name.local == "rPh" => r.skip()?,
                            Some(Event::Start { name, .. }) if name.local == "t" => acc.push_str(&decode_xstring(&r.read_text()?)),
                            Some(Event::End { .. }) if r.depth() < d => break,
                            None => return Err(Error::Invalid("unterminated <is>".into())),
                            _ => {}
                        }
                    }
                    c_is = Some(acc);
                }
                "f" if in_data => r.skip()?,
                _ => {}
            },
            Event::End { name } if is_main(&name.ns) => match name.local.as_str() {
                "sheetData" => in_data = false,
                "c" if in_data => {
                    let cell = match (c_t.as_str(), c_v.take(), c_is.take()) {
                        ("inlineStr", _, Some(t)) => Cell::Text(t),
                        ("s", Some(v), _) => {
                            let i: usize = v.trim().parse().map_err(|_| Error::Invalid(format!("shared string index {v:?}")))?;
                            Cell::Text(sst.get(i).cloned().ok_or_else(|| Error::Invalid(format!("shared string {i} out of range")))?)
                        }
                        ("b", Some(v), _) => Cell::Bool(v.trim() == "1" || v.trim().eq_ignore_ascii_case("true")),
                        ("e", Some(v), _) => Cell::Error(v),
                        ("str", Some(v), _) => Cell::Text(decode_xstring(&v)),
                        ("d", Some(v), _) => Cell::Date(v.trim().to_string()),
                        ("n", Some(v), _) | ("", Some(v), _) => {
                            let n: f64 = v.trim().parse().map_err(|_| Error::Invalid(format!("number {v:?}")))?;
                            match st.kinds.get(c_s).copied().flatten() {
                                Some(k) if n.is_finite() && n >= 0.0 => Cell::Date(serial_to_iso(n, sys, k)),
                                _ => Cell::Number(n),
                            }
                        }
                        (_, None, None) => Cell::Empty,
                        (t, Some(v), _) => return Err(Error::Invalid(format!("cell type {t:?} with value {v:?}"))),
                        (_, None, Some(t)) => Cell::Text(t),
                    };
                    if !cell.is_empty() {
                        if rows.len() <= row_i {
                            rows.resize_with(row_i + 1, Vec::new);
                        }
                        let row = &mut rows[row_i];
                        if row.len() <= col_i {
                            if col_i > 16_383 {
                                return Err(Error::Invalid(format!("column {} beyond XFD", col_i + 1)));
                            }
                            row.resize(col_i + 1, Cell::Empty);
                        }
                        row[col_i] = cell;
                    }
                    col_i += 1;
                }
                _ => {}
            },
            _ => {}
        }
    }
    while rows.last().is_some_and(|r| r.is_empty()) {
        rows.pop();
    }
    Ok(rows)
}

/// Read every sheet of an .xlsx package.
pub fn read(bytes: &[u8]) -> Result<Workbook, Error> {
    read_filtered(bytes, None)
}

/// Read one sheet by name (or the first visible sheet when `name` is None).
pub fn read_sheet(bytes: &[u8], name: Option<&str>) -> Result<Sheet, Error> {
    let wb = read_filtered(bytes, Some(name.unwrap_or("")))?;
    wb.sheets.into_iter().next().ok_or_else(|| Error::Missing(format!("sheet {:?}", name.unwrap_or("(first)"))))
}

/// Sheet names and states without reading cell data.
pub fn sheet_names(bytes: &[u8]) -> Result<Vec<(String, String)>, Error> {
    let p = Package::open(bytes)?;
    let wb = p.main_part(rel::OFFICE_DOCUMENT)?;
    let (_, list) = workbook_sheets(&p, &wb)?;
    Ok(list.into_iter().map(|(n, s, _)| (n, s)).collect())
}

fn workbook_sheets(p: &Package, wb: &str) -> Result<(DateSystem, Vec<(String, String, String)>), Error> {
    let root = xml::parse_bytes(&p.part(wb)?)?;
    let mut sys = DateSystem::Compat1900;
    if let Some(pr) = root.elements().find(|e| e.name.local == "workbookPr") {
        let truthy = |v: Option<&str>| matches!(v, Some("1") | Some("true"));
        if truthy(pr.attr("date1904")) {
            sys = DateSystem::Base1904;
        } else if matches!(pr.attr("dateCompatibility"), Some("0") | Some("false")) {
            sys = DateSystem::Base1899;
        }
    }
    let rels = p.rels(Some(wb))?;
    let mut out = Vec::new();
    if let Some(sheets) = root.elements().find(|e| e.name.local == "sheets") {
        for s in sheets.elements().filter(|e| e.name.local == "sheet") {
            let rid = s.attr_ns(NS_R, "id").or_else(|| s.attr_ns(NS_R_STRICT, "id")).unwrap_or("");
            let Some(r) = rels.iter().find(|r| r.id == rid) else { continue };
            out.push((s.attr("name").unwrap_or("").to_string(), s.attr("state").unwrap_or("visible").to_string(), super::opc::resolve(Some(wb), &r.target)));
        }
    }
    Ok((sys, out))
}

fn read_filtered(bytes: &[u8], only: Option<&str>) -> Result<Workbook, Error> {
    let p = Package::open(bytes)?;
    let wb = p.main_part(rel::OFFICE_DOCUMENT)?;
    let (sys, list) = workbook_sheets(&p, &wb)?;
    let sst = match p.related(&wb, rel::SHARED_STRINGS)?.first() {
        Some((_, part)) if p.has_part(part) => shared_strings(&p.part(part)?)?,
        _ => Vec::new(),
    };
    let st = match p.related(&wb, rel::STYLES)?.first() {
        Some((_, part)) if p.has_part(part) => styles(&p.part(part)?)?,
        _ => Styles { kinds: Vec::new() },
    };
    let mut sheets = Vec::new();
    for (name, state, part) in list {
        let wanted = match only {
            None => true,
            Some("") => state == "visible" && sheets.is_empty(),
            Some(n) => n == name,
        };
        if !wanted {
            continue;
        }
        let rows = read_sheet_xml(&p.part(&part)?, &sst, &st, sys)?;
        sheets.push(Sheet { name, state, rows });
    }
    Ok(Workbook { sheets, date1904: sys == DateSystem::Base1904 })
}

// ---- writing ------------------------------------------------------------------------------------------------------

/// One sheet to write.
#[derive(Debug, Clone, Default)]
pub struct WriteSheet {
    pub name: String,
    pub rows: Vec<Vec<Cell>>,
    /// First row is a header: bold, frozen, with an autofilter over the data.
    pub header: bool,
    /// Column widths in characters; missing entries are sized from the content.
    pub widths: Vec<f64>,
}

const CT_WORKBOOK: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml";
const CT_SHEET: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";
const CT_STYLES: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml";
const CT_SST: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml";

/// §18.2.19 sheet names: 1–31 characters, none of `[]:*?/\`, not starting or ending with an apostrophe; unique.
fn sheet_name(raw: &str, used: &[String]) -> String {
    let mut n: String = raw.chars().map(|c| if "[]:*?/\\".contains(c) { '_' } else { c }).collect();
    n = n.trim_matches('\'').to_string();
    if n.trim().is_empty() {
        n = "Sheet".into();
    }
    let base: String = n.chars().take(31).collect();
    let mut cand = base.clone();
    let mut k = 2;
    while used.iter().any(|u| u.eq_ignore_ascii_case(&cand)) {
        let suffix = format!(" ({k})");
        cand = format!("{}{suffix}", base.chars().take(31 - suffix.len()).collect::<String>());
        k += 1;
    }
    cand
}

/// Write a workbook. Style indexes: 0 normal, 1 bold header, 2 date, 3 date-time, 4 time.
pub fn write(sheets: &[WriteSheet]) -> Result<Vec<u8>, Error> {
    if sheets.is_empty() {
        return Err(Error::Invalid("a workbook needs at least one sheet (§18.2.20)".into()));
    }
    let mut sst: Vec<String> = Vec::new();
    let mut sst_index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut sst_count = 0usize;
    let mut pkg = PackageWriter::new();
    let mut names: Vec<String> = Vec::new();
    let mut defined: Vec<String> = Vec::new();
    for (si, sh) in sheets.iter().enumerate() {
        let name = sheet_name(&sh.name, &names);
        names.push(name.clone());
        let ncols = sh.rows.iter().map(|r| r.len()).max().unwrap_or(0);
        let nrows = sh.rows.len();
        let mut w = Writer::new();
        w.decl().start("worksheet").attr("xmlns", NS_MAIN).attr("xmlns:r", NS_R);
        if nrows > 0 && ncols > 0 {
            w.empty("dimension", &[("ref", &format!("A1:{}", cell_ref(nrows - 1, ncols - 1)))]);
        }
        w.start("sheetViews").start("sheetView").attr("workbookViewId", "0");
        if si == 0 {
            w.attr("tabSelected", "1");
        }
        if sh.header && nrows > 1 {
            w.empty("pane", &[("ySplit", "1"), ("topLeftCell", "A2"), ("activePane", "bottomLeft"), ("state", "frozen")]);
            w.empty("selection", &[("pane", "bottomLeft"), ("activeCell", "A2"), ("sqref", "A2")]);
        }
        w.end().end();
        w.empty("sheetFormatPr", &[("defaultRowHeight", "15")]);
        if ncols > 0 {
            w.start("cols");
            for c in 0..ncols {
                let width = sh.widths.get(c).copied().filter(|v| *v > 0.0).unwrap_or_else(|| {
                    let longest = sh.rows.iter().take(500).filter_map(|r| r.get(c)).map(|x| x.as_text().chars().count()).max().unwrap_or(0);
                    (longest.clamp(6, 60) as f64) * 1.1 + 2.0
                });
                let idx = (c + 1).to_string();
                w.empty("col", &[("min", &idx), ("max", &idx), ("width", &format!("{width:.2}")), ("customWidth", "1")]);
            }
            w.end();
        }
        w.start("sheetData");
        for (ri, row) in sh.rows.iter().enumerate() {
            w.start("row").attr("r", &(ri + 1).to_string());
            for (ci, cell) in row.iter().enumerate() {
                let r = cell_ref(ri, ci);
                let bold = sh.header && ri == 0;
                match cell {
                    Cell::Empty => {
                        if bold {
                            w.empty("c", &[("r", &r), ("s", "1")]);
                        }
                    }
                    Cell::Text(t) => {
                        let enc = encode_xstring(t);
                        let i = *sst_index.entry(enc.clone()).or_insert_with(|| {
                            sst.push(enc);
                            sst.len() - 1
                        });
                        sst_count += 1;
                        w.start("c").attr("r", &r).attr("t", "s");
                        if bold {
                            w.attr("s", "1");
                        }
                        w.leaf("v", &[], &i.to_string()).end();
                    }
                    Cell::Number(n) if n.is_finite() => {
                        w.start("c").attr("r", &r);
                        if bold {
                            w.attr("s", "1");
                        }
                        w.leaf("v", &[], &format!("{n}")).end();
                    }
                    Cell::Number(_) => {
                        w.start("c").attr("r", &r).attr("t", "e").leaf("v", &[], "#NUM!").end();
                    }
                    Cell::Bool(b) => {
                        w.start("c").attr("r", &r).attr("t", "b");
                        if bold {
                            w.attr("s", "1");
                        }
                        w.leaf("v", &[], if *b { "1" } else { "0" }).end();
                    }
                    Cell::Error(e) => {
                        w.start("c").attr("r", &r).attr("t", "e").leaf("v", &[], e).end();
                    }
                    Cell::Date(d) => match iso_to_serial(d) {
                        Some((serial, kind)) => {
                            let s = match kind {
                                DateKind::Date => "2",
                                DateKind::DateTime => "3",
                                DateKind::Time => "4",
                            };
                            w.start("c").attr("r", &r).attr("s", s).leaf("v", &[], &format!("{serial}")).end();
                        }
                        None => {
                            // not a date we can place on the 1900 axis: keep it as text
                            let enc = encode_xstring(d);
                            let i = *sst_index.entry(enc.clone()).or_insert_with(|| {
                                sst.push(enc);
                                sst.len() - 1
                            });
                            sst_count += 1;
                            w.start("c").attr("r", &r).attr("t", "s").leaf("v", &[], &i.to_string()).end();
                        }
                    },
                }
            }
            w.end();
        }
        w.end();
        if sh.header && nrows > 0 && ncols > 0 {
            let range = format!("A1:{}", cell_ref(nrows - 1, ncols - 1));
            w.empty("autoFilter", &[("ref", &range)]);
            let abs = format!("'{}'!$A$1:${}${}", name.replace('\'', "''"), col_letters(ncols - 1), nrows);
            defined.push(format!("<definedName name=\"_xlnm._FilterDatabase\" localSheetId=\"{si}\" hidden=\"1\">{}</definedName>", xml::escape_text(&abs)));
        }
        let part = format!("/xl/worksheets/sheet{}.xml", si + 1);
        pkg.add_part(&part, CT_SHEET, w.finish().into_bytes());
    }
    // workbook
    let mut w = Writer::new();
    w.decl().start("workbook").attr("xmlns", NS_MAIN).attr("xmlns:r", NS_R);
    w.start("bookViews").empty("workbookView", &[("activeTab", "0")]).end();
    w.start("sheets");
    for (i, n) in names.iter().enumerate() {
        let id = (i + 1).to_string();
        w.empty("sheet", &[("name", n), ("sheetId", &id), ("r:id", &format!("rId{id}"))]);
    }
    w.end();
    if !defined.is_empty() {
        w.start("definedNames").raw(&defined.concat()).end();
    }
    pkg.add_part("/xl/workbook.xml", CT_WORKBOOK, w.finish().into_bytes());
    for i in 0..sheets.len() {
        pkg.add_rel(Some("/xl/workbook.xml"), rel::WORKSHEET, &format!("worksheets/sheet{}.xml", i + 1), false);
    }
    // styles (§18.8): fonts 0 normal / 1 bold; fills 0 none / 1 gray125 (both required); 1 border; 5 cell formats
    let styles = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<styleSheet xmlns=\"{NS_MAIN}\">\
<numFmts count=\"3\"><numFmt numFmtId=\"164\" formatCode=\"yyyy\\-mm\\-dd\"/><numFmt numFmtId=\"165\" formatCode=\"yyyy\\-mm\\-dd\\ hh:mm:ss\"/><numFmt numFmtId=\"166\" formatCode=\"hh:mm:ss\"/></numFmts>\
<fonts count=\"2\"><font><sz val=\"11\"/><name val=\"Calibri\"/><family val=\"2\"/></font><font><b/><sz val=\"11\"/><name val=\"Calibri\"/><family val=\"2\"/></font></fonts>\
<fills count=\"2\"><fill><patternFill patternType=\"none\"/></fill><fill><patternFill patternType=\"gray125\"/></fill></fills>\
<borders count=\"1\"><border><left/><right/><top/><bottom/><diagonal/></border></borders>\
<cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs>\
<cellXfs count=\"5\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/><xf numFmtId=\"0\" fontId=\"1\" fillId=\"0\" borderId=\"0\" xfId=\"0\" applyFont=\"1\"/>\
<xf numFmtId=\"164\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\" applyNumberFormat=\"1\"/><xf numFmtId=\"165\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\" applyNumberFormat=\"1\"/>\
<xf numFmtId=\"166\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\" applyNumberFormat=\"1\"/></cellXfs>\
<cellStyles count=\"1\"><cellStyle name=\"Normal\" xfId=\"0\" builtinId=\"0\"/></cellStyles></styleSheet>"
    );
    pkg.add_part("/xl/styles.xml", CT_STYLES, styles.into_bytes());
    pkg.add_rel(Some("/xl/workbook.xml"), rel::STYLES, "styles.xml", false);
    let mut w = Writer::new();
    w.decl().start("sst").attr("xmlns", NS_MAIN).attr("count", &sst_count.to_string()).attr("uniqueCount", &sst.len().to_string());
    for s in &sst {
        w.start("si").start("t");
        if s.starts_with(char::is_whitespace) || s.ends_with(char::is_whitespace) {
            w.attr("xml:space", "preserve");
        }
        w.text(s).end().end();
    }
    pkg.add_part("/xl/sharedStrings.xml", CT_SST, w.finish().into_bytes());
    pkg.add_rel(Some("/xl/workbook.xml"), rel::SHARED_STRINGS, "sharedStrings.xml", false);
    pkg.add_rel(None, rel::OFFICE_DOCUMENT, "xl/workbook.xml", false);
    pkg.finish()
}

/// Convenience: one sheet from a header and string rows (numbers stay text unless parsed by the caller).
pub fn write_table(name: &str, header: &[&str], rows: &[Vec<Cell>]) -> Result<Vec<u8>, Error> {
    let mut all = vec![header.iter().map(|h| Cell::Text(h.to_string())).collect::<Vec<_>>()];
    all.extend(rows.iter().cloned());
    write(&[WriteSheet { name: name.into(), rows: all, header: true, widths: Vec::new() }])
}
