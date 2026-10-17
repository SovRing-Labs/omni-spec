//! XML 1.0 (Fifth Edition, W3C REC 2008-11-26) pull tokenizer + Namespaces in XML 1.0 (Third Edition, W3C REC 2009-12-08)
//! resolution, a small element tree, and a writer with escaping.
//!
//! Reader covers: XML declaration and processing instructions (§2.6, §2.8), comments (§2.5), CDATA sections (§2.7),
//! DOCTYPE skipped (internal subset bracket-matched, entity declarations not expanded), start/end/empty-element tags
//! (§3.1), attribute-value normalisation of literal whitespace (§3.3.3), the five predefined entities and character
//! references (§4.1, §4.6), end-of-line handling (§2.11), well-formed nesting (end tag must match, §3 WFC
//! "Element Type Match"), namespace scoping with default and prefixed declarations (Namespaces §5–§6), BOM-detected
//! UTF-8 / UTF-16 input (§4.3.3, Appendix F).
//! Not covered: validation, external entities (never fetched), general entities declared in a DTD.

use std::fmt;

pub const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";
const XMLNS_NS: &str = "http://www.w3.org/2000/xmlns/";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub pos: usize,
    pub msg: String,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "xml error at byte {}: {}", self.pos, self.msg)
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;

fn err<T>(pos: usize, msg: impl Into<String>) -> Result<T> {
    Err(Error { pos, msg: msg.into() })
}

/// A qualified name with its namespace resolved (Namespaces §6.1–§6.2: unprefixed elements take the default namespace,
/// unprefixed attributes have none).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QName {
    pub prefix: String,
    pub local: String,
    pub ns: String,
}
impl QName {
    pub fn is(&self, ns: &str, local: &str) -> bool {
        self.local == local && self.ns == ns
    }
    pub fn qualified(&self) -> String {
        if self.prefix.is_empty() { self.local.clone() } else { format!("{}:{}", self.prefix, self.local) }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attr {
    pub name: QName,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Start tag or empty-element tag (`empty` = true; a matching End is still emitted next).
    Start { name: QName, attrs: Vec<Attr>, empty: bool },
    End { name: QName },
    /// Character data with references resolved.
    Text(String),
    CData(String),
    Comment(String),
    /// Processing instruction; the XML declaration is reported with target "xml".
    Pi { target: String, data: String },
    Doctype(String),
}

/// Decode a document's bytes to text: UTF-8 (with or without BOM) or UTF-16 LE/BE with BOM (§4.3.3, Appendix F).
pub fn decode(bytes: &[u8]) -> Result<String> {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8(bytes[3..].to_vec()).or_else(|e| err(e.utf8_error().valid_up_to() + 3, "invalid UTF-8"));
    }
    let utf16 = |le: bool| -> Result<String> {
        let b = &bytes[2..];
        let units: Vec<u16> =
            b.chunks_exact(2).map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) }).collect();
        String::from_utf16(&units).or_else(|_| err(0, "invalid UTF-16"))
    };
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return utf16(true);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return utf16(false);
    }
    String::from_utf8(bytes.to_vec()).or_else(|e| err(e.utf8_error().valid_up_to(), "invalid UTF-8"))
}

/// Resolve the five predefined entities and character references (§4.1, §4.6) in `s`.
pub fn unescape(s: &str, pos: usize) -> Result<String> {
    if !s.contains('&') {
        return Ok(s.to_string());
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let Some(j) = after.find(';') else { return err(pos, "unterminated reference") };
        let r = &after[..j];
        let c = match r {
            "lt" => '<',
            "gt" => '>',
            "amp" => '&',
            "apos" => '\'',
            "quot" => '"',
            _ if r.starts_with("#x") => u32::from_str_radix(&r[2..], 16).ok().and_then(char::from_u32).ok_or_else(|| Error { pos, msg: format!("bad character reference &{r};") })?,
            _ if r.starts_with('#') => r[1..].parse::<u32>().ok().and_then(char::from_u32).ok_or_else(|| Error { pos, msg: format!("bad character reference &{r};") })?,
            _ => return err(pos, format!("undeclared entity &{r};")),
        };
        out.push(c);
        rest = &after[j + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// §2.11: "\r\n" and lone "\r" become "\n".
fn eol(s: &str) -> std::borrow::Cow<'_, str> {
    if s.contains('\r') { s.replace("\r\n", "\n").replace('\r', "\n").into() } else { s.into() }
}

fn is_name_end(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | b'/' | b'>' | b'=' | b'?')
}

/// Pull tokenizer. `next()` yields events until `Ok(None)` at a well-formed end of document.
pub struct Reader<'a> {
    src: &'a str,
    pos: usize,
    /// open elements: (qualified name, namespace bindings count before this element)
    stack: Vec<(String, QName, usize)>,
    ns: Vec<(String, String)>,
    pending_end: Option<QName>,
    seen_root: bool,
    /// When true (default), whitespace-only text outside the root element is skipped.
    pub skip_outer_ws: bool,
}

impl<'a> Reader<'a> {
    pub fn new(src: &'a str) -> Reader<'a> {
        let src = src.strip_prefix('\u{feff}').unwrap_or(src);
        Reader {
            src,
            pos: 0,
            stack: Vec::new(),
            ns: vec![("xml".into(), XML_NS.into()), ("xmlns".into(), XMLNS_NS.into())],
            pending_end: None,
            seen_root: false,
            skip_outer_ws: true,
        }
    }

    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    pub fn position(&self) -> usize {
        self.pos
    }

    fn resolve(&self, prefix: &str) -> Option<&str> {
        self.ns.iter().rev().find(|(p, _)| p == prefix).map(|(_, u)| u.as_str())
    }

    fn split(q: &str) -> (&str, &str) {
        match q.find(':') {
            Some(i) => (&q[..i], &q[i + 1..]),
            None => ("", q),
        }
    }

    pub fn next(&mut self) -> Result<Option<Event>> {
        if let Some(name) = self.pending_end.take() {
            let (_, _, mark) = self.stack.pop().unwrap();
            self.ns.truncate(mark);
            return Ok(Some(Event::End { name }));
        }
        loop {
            let b = self.src.as_bytes();
            if self.pos >= b.len() {
                if let Some((q, ..)) = self.stack.last() {
                    return err(self.pos, format!("unclosed element <{q}>"));
                }
                if !self.seen_root {
                    return err(self.pos, "no root element");
                }
                return Ok(None);
            }
            if b[self.pos] != b'<' {
                let start = self.pos;
                let end = self.src[start..].find('<').map(|i| start + i).unwrap_or(b.len());
                self.pos = end;
                let raw = &self.src[start..end];
                if self.stack.is_empty() {
                    if raw.trim().is_empty() && self.skip_outer_ws {
                        continue;
                    }
                    if !raw.trim().is_empty() {
                        return err(start, "text outside the root element");
                    }
                }
                return Ok(Some(Event::Text(unescape(&eol(raw), start)?)));
            }
            let rest = &self.src[self.pos..];
            if let Some(r) = rest.strip_prefix("<!--") {
                let Some(i) = r.find("-->") else { return err(self.pos, "unterminated comment") };
                let c = r[..i].to_string();
                self.pos += 4 + i + 3;
                return Ok(Some(Event::Comment(c)));
            }
            if let Some(r) = rest.strip_prefix("<![CDATA[") {
                let Some(i) = r.find("]]>") else { return err(self.pos, "unterminated CDATA section") };
                if self.stack.is_empty() {
                    return err(self.pos, "CDATA outside the root element");
                }
                let c = eol(&r[..i]).into_owned();
                self.pos += 9 + i + 3;
                return Ok(Some(Event::CData(c)));
            }
            if rest.starts_with("<!DOCTYPE") {
                // skip to the matching '>' outside the bracketed internal subset and quoted literals
                let bytes = rest.as_bytes();
                let (mut i, mut depth, mut quote) = (9, 0i32, 0u8);
                while i < bytes.len() {
                    let c = bytes[i];
                    if quote != 0 {
                        if c == quote {
                            quote = 0;
                        }
                    } else if depth > 0 && rest[i..].starts_with("<!--") {
                        // comments and PIs inside the internal subset (§2.8 markupdecl) may contain brackets
                        match rest[i + 4..].find("-->") {
                            Some(j) => i += 4 + j + 2,
                            None => i = bytes.len(),
                        }
                    } else if depth > 0 && rest[i..].starts_with("<?") {
                        match rest[i + 2..].find("?>") {
                            Some(j) => i += 2 + j + 1,
                            None => i = bytes.len(),
                        }
                    } else if c == b'"' || c == b'\'' {
                        quote = c;
                    } else if c == b'[' {
                        depth += 1;
                    } else if c == b']' {
                        depth -= 1;
                    } else if c == b'>' && depth <= 0 {
                        break;
                    }
                    i += 1;
                }
                if i >= bytes.len() {
                    return err(self.pos, "unterminated DOCTYPE");
                }
                let d = rest[9..i].trim().to_string();
                self.pos += i + 1;
                return Ok(Some(Event::Doctype(d)));
            }
            if let Some(r) = rest.strip_prefix("<?") {
                let Some(i) = r.find("?>") else { return err(self.pos, "unterminated processing instruction") };
                let body = &r[..i];
                let t_end = body.find(|c: char| c.is_ascii_whitespace()).unwrap_or(body.len());
                let target = body[..t_end].to_string();
                let data = body[t_end..].trim_start().to_string();
                self.pos += 2 + i + 2;
                return Ok(Some(Event::Pi { target, data }));
            }
            if let Some(r) = rest.strip_prefix("</") {
                let n_end = r.bytes().position(is_name_end).unwrap_or(r.len());
                let q = &r[..n_end];
                let Some(gt) = r[n_end..].find('>') else { return err(self.pos, "unterminated end tag") };
                if !r[n_end..n_end + gt].trim().is_empty() {
                    return err(self.pos, "junk in end tag");
                }
                let Some((open, name, mark)) = self.stack.pop() else { return err(self.pos, format!("end tag </{q}> with no open element")) };
                if open != q {
                    return err(self.pos, format!("end tag </{q}> does not match <{open}>"));
                }
                self.ns.truncate(mark);
                self.pos += 2 + n_end + gt + 1;
                return Ok(Some(Event::End { name }));
            }
            return self.start_tag().map(Some);
        }
    }

    fn start_tag(&mut self) -> Result<Event> {
        let at = self.pos;
        if self.stack.is_empty() && self.seen_root {
            return err(at, "second root element");
        }
        let b = self.src.as_bytes();
        let mut i = self.pos + 1;
        let n0 = i;
        while i < b.len() && !is_name_end(b[i]) {
            i += 1;
        }
        let qname = &self.src[n0..i];
        if qname.is_empty() {
            return err(at, "empty element name");
        }
        let mut raw_attrs: Vec<(&str, String)> = Vec::new();
        let empty;
        loop {
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= b.len() {
                return err(at, "unterminated start tag");
            }
            if b[i] == b'>' {
                empty = false;
                i += 1;
                break;
            }
            if b[i] == b'/' {
                if b.get(i + 1) != Some(&b'>') {
                    return err(i, "expected '/>'");
                }
                empty = true;
                i += 2;
                break;
            }
            let a0 = i;
            while i < b.len() && !is_name_end(b[i]) {
                i += 1;
            }
            let an = &self.src[a0..i];
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if an.is_empty() || b.get(i) != Some(&b'=') {
                return err(i, format!("attribute {an:?} without a value"));
            }
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            let Some(&qc) = b.get(i) else { return err(i, "unterminated attribute") };
            if qc != b'"' && qc != b'\'' {
                return err(i, "attribute value must be quoted");
            }
            let v0 = i + 1;
            let Some(len) = self.src[v0..].find(qc as char) else { return err(i, "unterminated attribute value") };
            let rawv = &self.src[v0..v0 + len];
            if rawv.contains('<') {
                return err(v0, "'<' in attribute value");
            }
            // §3.3.3: literal whitespace characters become spaces before references are resolved
            let norm: String = eol(rawv).chars().map(|c| if c == '\t' || c == '\n' { ' ' } else { c }).collect();
            if raw_attrs.iter().any(|(n, _)| *n == an) {
                return err(a0, format!("duplicate attribute {an}"));
            }
            raw_attrs.push((an, unescape(&norm, v0)?));
            i = v0 + len + 1;
        }
        self.pos = i;
        let mark = self.ns.len();
        for (n, v) in &raw_attrs {
            if *n == "xmlns" {
                self.ns.push((String::new(), v.clone()));
            } else if let Some(p) = n.strip_prefix("xmlns:") {
                self.ns.push((p.to_string(), v.clone()));
            }
        }
        let (p, l) = Self::split(qname);
        let name = QName { prefix: p.into(), local: l.into(), ns: self.resolve(p).unwrap_or("").to_string() };
        let mut attrs = Vec::with_capacity(raw_attrs.len());
        for (n, v) in raw_attrs {
            let (ap, al) = Self::split(n);
            let ns = if n == "xmlns" {
                XMLNS_NS.to_string()
            } else if ap.is_empty() {
                String::new()
            } else {
                self.resolve(ap).unwrap_or("").to_string()
            };
            attrs.push(Attr { name: QName { prefix: ap.into(), local: al.into(), ns }, value: v });
        }
        self.stack.push((qname.to_string(), name.clone(), mark));
        self.seen_root = true;
        if empty {
            self.pending_end = Some(name.clone());
        }
        Ok(Event::Start { name, attrs, empty })
    }

    /// Concatenated text and CDATA of the current element up to its end tag (child elements' text included);
    /// call right after its Start event.
    pub fn read_text(&mut self) -> Result<String> {
        let d = self.depth();
        let mut s = String::new();
        loop {
            match self.next()? {
                Some(Event::Text(t)) | Some(Event::CData(t)) => s.push_str(&t),
                Some(Event::End { .. }) if self.depth() < d => return Ok(s),
                None => return err(self.pos, "unexpected end of document"),
                _ => {}
            }
        }
    }

    /// Skip the rest of the current element (call right after its Start event).
    pub fn skip(&mut self) -> Result<()> {
        let d = self.depth();
        loop {
            match self.next()? {
                Some(Event::End { .. }) if self.depth() < d => return Ok(()),
                None => return err(self.pos, "unexpected end of document"),
                _ => {}
            }
        }
    }
}

/// Small element tree for parts that fit in memory comfortably.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Element(Element),
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub name: QName,
    pub attrs: Vec<Attr>,
    pub children: Vec<Node>,
}

impl Element {
    /// Attribute by namespace URI ("" for unprefixed) and local name.
    pub fn attr_ns(&self, ns: &str, local: &str) -> Option<&str> {
        self.attrs.iter().find(|a| a.name.local == local && a.name.ns == ns).map(|a| a.value.as_str())
    }
    /// Attribute by local name, any namespace (first match).
    pub fn attr(&self, local: &str) -> Option<&str> {
        self.attrs.iter().find(|a| a.name.local == local && a.name.ns != XMLNS_NS).map(|a| a.value.as_str())
    }
    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| if let Node::Element(e) = n { Some(e) } else { None })
    }
    pub fn child(&self, ns: &str, local: &str) -> Option<&Element> {
        self.elements().find(|e| e.name.is(ns, local))
    }
    pub fn children_named<'s>(&'s self, ns: &'s str, local: &'s str) -> impl Iterator<Item = &'s Element> + 's {
        self.elements().filter(move |e| e.name.is(ns, local))
    }
    /// All descendant text, in document order.
    pub fn text(&self) -> String {
        let mut s = String::new();
        fn walk(e: &Element, s: &mut String) {
            for c in &e.children {
                match c {
                    Node::Text(t) => s.push_str(t),
                    Node::Element(x) => walk(x, s),
                }
            }
        }
        walk(self, &mut s);
        s
    }
    /// Depth-first search for descendants with this name.
    pub fn descendants<'s>(&'s self, ns: &str, local: &str) -> Vec<&'s Element> {
        let mut v = Vec::new();
        fn walk<'x>(e: &'x Element, ns: &str, local: &str, v: &mut Vec<&'x Element>) {
            for c in e.elements() {
                if c.name.is(ns, local) {
                    v.push(c);
                }
                walk(c, ns, local, v);
            }
        }
        walk(self, ns, local, &mut v);
        v
    }
}

/// Parse a whole document into its root element (comments, PIs and DOCTYPE dropped; CDATA becomes text).
pub fn parse(src: &str) -> Result<Element> {
    let mut r = Reader::new(src);
    let mut stack: Vec<Element> = Vec::new();
    let mut root = None;
    while let Some(ev) = r.next()? {
        match ev {
            Event::Start { name, attrs, .. } => stack.push(Element { name, attrs, children: Vec::new() }),
            Event::End { .. } => {
                let e = stack.pop().unwrap();
                match stack.last_mut() {
                    Some(p) => p.children.push(Node::Element(e)),
                    None => root = Some(e),
                }
            }
            Event::Text(t) | Event::CData(t) => {
                if let Some(p) = stack.last_mut() {
                    if let Some(Node::Text(prev)) = p.children.last_mut() {
                        prev.push_str(&t);
                    } else {
                        p.children.push(Node::Text(t));
                    }
                }
            }
            _ => {}
        }
    }
    root.ok_or(Error { pos: 0, msg: "no root element".into() })
}

pub fn parse_bytes(b: &[u8]) -> Result<Element> {
    parse(&decode(b)?)
}

/// XML 1.0 §2.2 Char: #x9 | #xA | #xD | [#x20-#xD7FF] | [#xE000-#xFFFD] | [#x10000-#x10FFFF].
pub fn is_xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}')
}

/// Escape character data; characters that XML 1.0 cannot carry are dropped.
pub fn escape_text(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '&' => o.push_str("&amp;"),
            '\r' => o.push_str("&#13;"),
            c if is_xml_char(c) => o.push(c),
            _ => {}
        }
    }
    o
}

/// Escape an attribute value (quotes, and whitespace that §3.3.3 would otherwise normalise away).
pub fn escape_attr(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '&' => o.push_str("&amp;"),
            '"' => o.push_str("&quot;"),
            '\t' => o.push_str("&#9;"),
            '\n' => o.push_str("&#10;"),
            '\r' => o.push_str("&#13;"),
            c if is_xml_char(c) => o.push(c),
            _ => {}
        }
    }
    o
}

/// Streaming writer. Start tags stay open until content or `end()`, so childless elements come out as `<x/>`.
pub struct Writer {
    out: String,
    stack: Vec<String>,
    open: bool,
}

impl Default for Writer {
    fn default() -> Self {
        Self::new()
    }
}

impl Writer {
    pub fn new() -> Writer {
        Writer { out: String::new(), stack: Vec::new(), open: false }
    }
    /// `<?xml version="1.0" encoding="UTF-8" standalone="yes"?>` + newline.
    pub fn decl(&mut self) -> &mut Self {
        self.out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n");
        self
    }
    fn close_open(&mut self) {
        if self.open {
            self.out.push('>');
            self.open = false;
        }
    }
    pub fn start(&mut self, name: &str) -> &mut Self {
        self.close_open();
        self.out.push('<');
        self.out.push_str(name);
        self.stack.push(name.to_string());
        self.open = true;
        self
    }
    /// Add an attribute to the start tag just opened.
    pub fn attr(&mut self, name: &str, value: &str) -> &mut Self {
        debug_assert!(self.open, "attr() after content");
        self.out.push(' ');
        self.out.push_str(name);
        self.out.push_str("=\"");
        self.out.push_str(&escape_attr(value));
        self.out.push('"');
        self
    }
    pub fn text(&mut self, s: &str) -> &mut Self {
        self.close_open();
        self.out.push_str(&escape_text(s));
        self
    }
    /// Insert already-serialised XML (caller guarantees well-formedness).
    pub fn raw(&mut self, s: &str) -> &mut Self {
        self.close_open();
        self.out.push_str(s);
        self
    }
    pub fn end(&mut self) -> &mut Self {
        let name = self.stack.pop().expect("end() with no open element");
        if self.open {
            self.out.push_str("/>");
            self.open = false;
        } else {
            self.out.push_str("</");
            self.out.push_str(&name);
            self.out.push('>');
        }
        self
    }
    /// `<name attrs...>text</name>` in one call.
    pub fn leaf(&mut self, name: &str, attrs: &[(&str, &str)], text: &str) -> &mut Self {
        self.start(name);
        for (k, v) in attrs {
            self.attr(k, v);
        }
        if !text.is_empty() {
            self.text(text);
        }
        self.end()
    }
    /// `<name attrs.../>`.
    pub fn empty(&mut self, name: &str, attrs: &[(&str, &str)]) -> &mut Self {
        self.leaf(name, attrs, "")
    }
    pub fn finish(mut self) -> String {
        while !self.stack.is_empty() {
            self.end();
        }
        self.out
    }
}
