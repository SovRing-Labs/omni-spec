//! JSON (RFC 8259): parser and serializer.
//!
//! Spec: RFC 8259 — §2 grammar (value, object, array, number, string), §7 strings (escapes; UTF-8 encoding),
//! §8.1 character encoding (UTF-8 input), §9 parsers (numbers are IEEE 754 binary64 here; objects keep member order).
//! Written from the RFC text. Parsing returns `Err` on any grammar violation, on invalid UTF-8, on a lone surrogate
//! escape, and on nesting deeper than `MAX_DEPTH` (so hostile input cannot exhaust the stack).

/// Nesting limit for arrays and objects.
pub const MAX_DEPTH: usize = 512;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    /// Number as binary64, RFC 8259 §6 (the grammar is checked; precision follows the nearest double).
    Number(f64),
    String(String),
    Array(Vec<Value>),
    /// Members in source order (duplicate names are kept; lookups return the first).
    Object(Vec<(String, Value)>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub offset: usize,
    pub kind: ErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    InvalidUtf8,
    UnexpectedEnd,
    UnexpectedByte(u8),
    BadNumber,
    BadEscape,
    BadUnicodeEscape,
    LoneSurrogate,
    ControlCharacterInString,
    TooDeep,
    TrailingData,
}

pub type Result<T> = core::result::Result<T, Error>;

struct Parser<'a> {
    s: &'a str,
    b: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn err<T>(&self, kind: ErrorKind) -> Result<T> {
        Err(Error { offset: self.pos, kind })
    }

    fn skip_ws(&mut self) {
        // RFC 8259 §2: ws = *( %x20 / %x09 / %x0A / %x0D )
        while let Some(&c) = self.b.get(self.pos) {
            if matches!(c, b' ' | b'\t' | b'\n' | b'\r') {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Result<u8> {
        match self.b.get(self.pos) {
            Some(&c) => Ok(c),
            None => self.err(ErrorKind::UnexpectedEnd),
        }
    }

    fn expect_lit(&mut self, lit: &[u8], v: Value) -> Result<Value> {
        if self.b.get(self.pos..self.pos + lit.len()) == Some(lit) {
            self.pos += lit.len();
            Ok(v)
        } else {
            let c = self.b.get(self.pos).copied().unwrap_or(0);
            self.err(ErrorKind::UnexpectedByte(c))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value> {
        self.skip_ws();
        let v = match self.peek()? {
            b'{' => self.object(depth + 1)?,
            b'[' => self.array(depth + 1)?,
            b'"' => Value::String(self.string()?),
            b't' => self.expect_lit(b"true", Value::Bool(true))?,
            b'f' => self.expect_lit(b"false", Value::Bool(false))?,
            b'n' => self.expect_lit(b"null", Value::Null)?,
            b'-' | b'0'..=b'9' => Value::Number(self.number()?),
            c => return self.err(ErrorKind::UnexpectedByte(c)),
        };
        self.skip_ws();
        Ok(v)
    }

    fn array(&mut self, depth: usize) -> Result<Value> {
        if depth > MAX_DEPTH {
            return self.err(ErrorKind::TooDeep);
        }
        self.pos += 1; // [
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek()? == b']' {
            self.pos += 1;
            return Ok(Value::Array(items));
        }
        loop {
            items.push(self.value(depth)?);
            match self.peek()? {
                b',' => self.pos += 1,
                b']' => {
                    self.pos += 1;
                    return Ok(Value::Array(items));
                }
                c => return self.err(ErrorKind::UnexpectedByte(c)),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Value> {
        if depth > MAX_DEPTH {
            return self.err(ErrorKind::TooDeep);
        }
        self.pos += 1; // {
        let mut members = Vec::new();
        self.skip_ws();
        if self.peek()? == b'}' {
            self.pos += 1;
            return Ok(Value::Object(members));
        }
        loop {
            self.skip_ws();
            if self.peek()? != b'"' {
                return self.err(ErrorKind::UnexpectedByte(self.b[self.pos]));
            }
            let key = self.string()?;
            self.skip_ws();
            if self.peek()? != b':' {
                return self.err(ErrorKind::UnexpectedByte(self.b[self.pos]));
            }
            self.pos += 1;
            let v = self.value(depth)?;
            members.push((key, v));
            match self.peek()? {
                b',' => self.pos += 1,
                b'}' => {
                    self.pos += 1;
                    return Ok(Value::Object(members));
                }
                c => return self.err(ErrorKind::UnexpectedByte(c)),
            }
        }
    }

    /// RFC 8259 §6: number = [ minus ] int [ frac ] [ exp ].
    fn number(&mut self) -> Result<f64> {
        let start = self.pos;
        if self.peek()? == b'-' {
            self.pos += 1;
        }
        match self.peek()? {
            b'0' => self.pos += 1,
            b'1'..=b'9' => {
                while matches!(self.b.get(self.pos), Some(b'0'..=b'9')) {
                    self.pos += 1;
                }
            }
            _ => return self.err(ErrorKind::BadNumber),
        }
        if self.b.get(self.pos) == Some(&b'.') {
            self.pos += 1;
            if !matches!(self.b.get(self.pos), Some(b'0'..=b'9')) {
                return self.err(ErrorKind::BadNumber);
            }
            while matches!(self.b.get(self.pos), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        if matches!(self.b.get(self.pos), Some(b'e') | Some(b'E')) {
            self.pos += 1;
            if matches!(self.b.get(self.pos), Some(b'+') | Some(b'-')) {
                self.pos += 1;
            }
            if !matches!(self.b.get(self.pos), Some(b'0'..=b'9')) {
                return self.err(ErrorKind::BadNumber);
            }
            while matches!(self.b.get(self.pos), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        // The grammar above guarantees the slice is a valid Rust float literal.
        self.s[start..self.pos].parse::<f64>().or_else(|_| self.err(ErrorKind::BadNumber))
    }

    fn hex4(&mut self) -> Result<u16> {
        let mut v = 0u16;
        for _ in 0..4 {
            let c = self.peek()?;
            let d = match c {
                b'0'..=b'9' => c - b'0',
                b'a'..=b'f' => c - b'a' + 10,
                b'A'..=b'F' => c - b'A' + 10,
                _ => return self.err(ErrorKind::BadUnicodeEscape),
            };
            v = (v << 4) | d as u16;
            self.pos += 1;
        }
        Ok(v)
    }

    /// RFC 8259 §7: string = quotation-mark *char quotation-mark.
    fn string(&mut self) -> Result<String> {
        self.pos += 1; // opening quote
        let mut out = String::new();
        let mut run = self.pos;
        loop {
            let c = self.peek()?;
            match c {
                b'"' => {
                    out.push_str(&self.s[run..self.pos]);
                    self.pos += 1;
                    return Ok(out);
                }
                b'\\' => {
                    out.push_str(&self.s[run..self.pos]);
                    self.pos += 1;
                    let e = self.peek()?;
                    self.pos += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hi = self.hex4()?;
                            let cp = if (0xD800..0xDC00).contains(&hi) {
                                // High surrogate must be followed by \u low surrogate (§7).
                                if self.b.get(self.pos..self.pos + 2) != Some(b"\\u") {
                                    return self.err(ErrorKind::LoneSurrogate);
                                }
                                self.pos += 2;
                                let lo = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&lo) {
                                    return self.err(ErrorKind::LoneSurrogate);
                                }
                                0x10000 + (((hi as u32) - 0xD800) << 10) + (lo as u32 - 0xDC00)
                            } else if (0xDC00..0xE000).contains(&hi) {
                                return self.err(ErrorKind::LoneSurrogate);
                            } else {
                                hi as u32
                            };
                            out.push(char::from_u32(cp).ok_or(Error { offset: self.pos, kind: ErrorKind::BadUnicodeEscape })?);
                        }
                        _ => {
                            self.pos -= 1;
                            return self.err(ErrorKind::BadEscape);
                        }
                    }
                    run = self.pos;
                }
                0x00..=0x1F => return self.err(ErrorKind::ControlCharacterInString),
                _ => self.pos += 1,
            }
        }
    }
}

/// Parse one JSON text (RFC 8259 §2: a value surrounded by optional whitespace). Input must be UTF-8.
pub fn parse(input: &[u8]) -> Result<Value> {
    let s = core::str::from_utf8(input).map_err(|e| Error { offset: e.valid_up_to(), kind: ErrorKind::InvalidUtf8 })?;
    let mut p = Parser { s, b: input, pos: 0 };
    let v = p.value(0)?;
    if p.pos != input.len() {
        return p.err(ErrorKind::TrailingData);
    }
    Ok(v)
}

/// Serialize a value as compact JSON text (RFC 8259 §7 escapes). Non-finite numbers are written as `null`
/// because JSON cannot represent them.
pub fn to_string(v: &Value) -> String {
    let mut out = String::new();
    write_value(v, &mut out);
    out
}

fn write_value(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(n) => {
            if n.is_finite() {
                // Debug formatting of f64 is the shortest representation that round-trips, and is a valid JSON number.
                out.push_str(&format!("{n:?}"));
            } else {
                out.push_str("null");
            }
        }
        Value::String(s) => write_string(s, out),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(item, out);
            }
            out.push(']');
        }
        Value::Object(members) => {
            out.push('{');
            for (i, (k, item)) in members.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(k, out);
                out.push(':');
                write_value(item, out);
            }
            out.push('}');
        }
    }
}

fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}
