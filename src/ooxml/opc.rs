//! Open Packaging Conventions — ECMA-376 5th ed. Part 2 (= ISO/IEC 29500-2:2021).
//! Part names (§9.1.1: case-insensitive, "/"-rooted), the content types stream `[Content_Types].xml` (§10.1.2:
//! Override by part name first, then Default by extension), relationships parts `_rels/<name>.rels` (§9.3: Id, Type,
//! Target, TargetMode), target resolution relative to the source part (§9.3.5 + RFC 3986 §5.2), ZIP mapping (§10.2:
//! part name = ZIP item name without the leading "/", percent-decoded).

use super::Error;
use crate::xml::{self, Writer as XmlWriter};
use crate::zip::{Archive, Writer as ZipWriter};

pub const NS_CONTENT_TYPES: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
pub const NS_RELATIONSHIPS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

/// Relationship types (Part 1 §15.2 / Part 2 Annex). Transitional URIs; `Relationship::is` also accepts Strict ones
/// (http://purl.oclc.org/ooxml/officeDocument/relationships/…) by comparing the final path segment.
pub mod rel {
    pub const OFFICE_DOCUMENT: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
    pub const CORE_PROPERTIES: &str = "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
    pub const EXTENDED_PROPERTIES: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties";
    pub const STYLES: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles";
    pub const SHARED_STRINGS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings";
    pub const WORKSHEET: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet";
    pub const NUMBERING: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering";
    pub const FOOTNOTES: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes";
    pub const ENDNOTES: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/endnotes";
    pub const IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
    pub const HYPERLINK: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink";
    pub const SETTINGS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings";
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relationship {
    pub id: String,
    pub rel_type: String,
    pub target: String,
    pub external: bool,
}

impl Relationship {
    /// Type match, tolerant of Strict vs Transitional namespace URIs (same final segment).
    pub fn is(&self, rel_type: &str) -> bool {
        self.rel_type == rel_type || last_segment(&self.rel_type) == last_segment(rel_type)
    }
}

fn last_segment(s: &str) -> &str {
    s.rsplit('/').next().unwrap_or(s)
}

/// "/word/document.xml" → "/word/_rels/document.xml.rels"; None (the package) → "/_rels/.rels" (§9.3.3).
pub fn rels_part_for(source: Option<&str>) -> String {
    match source {
        None => "/_rels/.rels".to_string(),
        Some(p) => {
            let p = normalize(p);
            let (dir, file) = p.rsplit_once('/').unwrap_or(("", &p));
            format!("{dir}/_rels/{file}.rels")
        }
    }
}

/// Canonical part name: leading "/", percent-decoded, "\" → "/".
pub fn normalize(name: &str) -> String {
    let n = percent_decode(&name.replace('\\', "/"));
    if n.starts_with('/') { n } else { format!("/{n}") }
}

fn percent_decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Resolve a relationship target against its source part (RFC 3986 §5.2.4 dot-segment removal).
pub fn resolve(source: Option<&str>, target: &str) -> String {
    let t = target.split('#').next().unwrap_or("");
    let base_dir = match source {
        None => String::new(),
        Some(s) => normalize(s).rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default(),
    };
    let joined = if t.starts_with('/') { t.to_string() } else { format!("{base_dir}/{t}") };
    let mut segs: Vec<&str> = Vec::new();
    for s in joined.split('/') {
        match s {
            "" | "." => {}
            ".." => {
                segs.pop();
            }
            s => segs.push(s),
        }
    }
    normalize(&format!("/{}", segs.join("/")))
}

#[derive(Debug, Clone, Default)]
pub struct ContentTypes {
    pub defaults: Vec<(String, String)>,
    pub overrides: Vec<(String, String)>,
}

impl ContentTypes {
    pub fn parse(xml_src: &[u8]) -> Result<ContentTypes, Error> {
        let root = xml::parse_bytes(xml_src)?;
        let mut ct = ContentTypes::default();
        for e in root.elements() {
            match e.name.local.as_str() {
                "Default" => ct.defaults.push((e.attr("Extension").unwrap_or("").to_ascii_lowercase(), e.attr("ContentType").unwrap_or("").into())),
                "Override" => ct.overrides.push((normalize(e.attr("PartName").unwrap_or("")), e.attr("ContentType").unwrap_or("").into())),
                _ => {}
            }
        }
        Ok(ct)
    }
    /// §10.1.2.4: an Override for the part name wins, else the Default for its extension.
    pub fn get(&self, part: &str) -> Option<&str> {
        let p = normalize(part);
        if let Some((_, t)) = self.overrides.iter().find(|(n, _)| n.eq_ignore_ascii_case(&p)) {
            return Some(t);
        }
        let ext = p.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())?;
        self.defaults.iter().find(|(e, _)| *e == ext).map(|(_, t)| t.as_str())
    }
}

/// A package opened for reading.
pub struct Package<'a> {
    pub zip: Archive<'a>,
    pub types: ContentTypes,
}

impl<'a> Package<'a> {
    pub fn open(bytes: &'a [u8]) -> Result<Package<'a>, Error> {
        let zip = Archive::parse(bytes)?;
        let ct = zip.find_ci("[Content_Types].xml").ok_or_else(|| Error::Missing("[Content_Types].xml".into()))?;
        let types = ContentTypes::parse(&zip.read(ct)?)?;
        Ok(Package { zip, types })
    }

    /// ZIP item names as part names ("/xl/workbook.xml").
    pub fn part_names(&self) -> Vec<String> {
        self.zip.entries().iter().filter(|e| !e.is_dir()).map(|e| normalize(&e.name)).collect()
    }

    pub fn has_part(&self, name: &str) -> bool {
        self.find(name).is_some()
    }

    fn find(&self, name: &str) -> Option<&crate::zip::Entry> {
        let n = normalize(name);
        let item = &n[1..];
        self.zip.find(item).or_else(|| self.zip.find_ci(item)).or_else(|| {
            // ZIP item names may themselves be percent-encoded (§10.2.3)
            self.zip.entries().iter().find(|e| normalize(&e.name).eq_ignore_ascii_case(&n))
        })
    }

    pub fn part(&self, name: &str) -> Result<Vec<u8>, Error> {
        let e = self.find(name).ok_or_else(|| Error::Missing(normalize(name)))?;
        Ok(self.zip.read(e)?)
    }

    pub fn content_type(&self, name: &str) -> Option<&str> {
        self.types.get(name)
    }

    /// Relationships whose source is `source` (None = the package itself). A missing rels part is an empty list.
    pub fn rels(&self, source: Option<&str>) -> Result<Vec<Relationship>, Error> {
        let rp = rels_part_for(source);
        if !self.has_part(&rp) {
            return Ok(Vec::new());
        }
        let root = xml::parse_bytes(&self.part(&rp)?)?;
        Ok(root
            .elements()
            .filter(|e| e.name.local == "Relationship")
            .map(|e| Relationship {
                id: e.attr("Id").unwrap_or("").into(),
                rel_type: e.attr("Type").unwrap_or("").into(),
                target: e.attr("Target").unwrap_or("").into(),
                external: e.attr("TargetMode").map(|m| m.eq_ignore_ascii_case("External")).unwrap_or(false),
            })
            .collect())
    }

    /// Part name targeted by the first package relationship of `rel_type` (e.g. the main document).
    pub fn main_part(&self, rel_type: &str) -> Result<String, Error> {
        let r = self.rels(None)?.into_iter().find(|r| r.is(rel_type) && !r.external).ok_or_else(|| Error::Missing(format!("relationship {rel_type}")))?;
        Ok(resolve(None, &r.target))
    }

    /// Part names targeted from `source` by internal relationships of `rel_type`.
    pub fn related(&self, source: &str, rel_type: &str) -> Result<Vec<(Relationship, String)>, Error> {
        Ok(self.rels(Some(source))?.into_iter().filter(|r| r.is(rel_type) && !r.external).map(|r| {
            let p = resolve(Some(source), &r.target);
            (r, p)
        }).collect())
    }
}

/// Builds a package: parts + relationships + content types, written as a stored ZIP.
#[derive(Default)]
pub struct PackageWriter {
    parts: Vec<(String, String, Vec<u8>)>,
    rels: Vec<(Option<String>, Relationship)>,
    defaults: Vec<(String, String)>,
}

impl PackageWriter {
    pub fn new() -> PackageWriter {
        let mut w = PackageWriter::default();
        w.default_type("rels", "application/vnd.openxmlformats-package.relationships+xml");
        w.default_type("xml", "application/xml");
        w
    }

    pub fn default_type(&mut self, ext: &str, content_type: &str) {
        if !self.defaults.iter().any(|(e, _)| e.eq_ignore_ascii_case(ext)) {
            self.defaults.push((ext.to_ascii_lowercase(), content_type.into()));
        }
    }

    pub fn add_part(&mut self, name: &str, content_type: &str, bytes: Vec<u8>) {
        self.parts.push((normalize(name), content_type.into(), bytes));
    }

    /// Adds a relationship and returns its Id ("rId<n>", numbered per source).
    pub fn add_rel(&mut self, source: Option<&str>, rel_type: &str, target: &str, external: bool) -> String {
        let src = source.map(normalize);
        let n = self.rels.iter().filter(|(s, _)| *s == src).count() + 1;
        let id = format!("rId{n}");
        self.rels.push((src, Relationship { id: id.clone(), rel_type: rel_type.into(), target: target.into(), external }));
        id
    }

    fn rels_xml(rels: &[&Relationship]) -> Vec<u8> {
        let mut w = XmlWriter::new();
        w.decl().start("Relationships").attr("xmlns", NS_RELATIONSHIPS);
        for r in rels {
            w.start("Relationship").attr("Id", &r.id).attr("Type", &r.rel_type).attr("Target", &r.target);
            if r.external {
                w.attr("TargetMode", "External");
            }
            w.end();
        }
        w.finish().into_bytes()
    }

    pub fn finish(self) -> Result<Vec<u8>, Error> {
        let mut ct = XmlWriter::new();
        ct.decl().start("Types").attr("xmlns", NS_CONTENT_TYPES);
        for (e, t) in &self.defaults {
            ct.empty("Default", &[("Extension", e), ("ContentType", t)]);
        }
        for (n, t, _) in &self.parts {
            let ext = n.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
            if self.defaults.iter().any(|(e, dt)| *e == ext && dt == t) {
                continue;
            }
            ct.empty("Override", &[("PartName", n), ("ContentType", t)]);
        }
        let mut z = ZipWriter::new();
        z.add_deflated("[Content_Types].xml", ct.finish().as_bytes())?;
        let mut sources: Vec<Option<String>> = Vec::new();
        for (s, _) in &self.rels {
            if !sources.contains(s) {
                sources.push(s.clone());
            }
        }
        // package relationships first (§9.3.1), then each part followed by its own relationships part
        if sources.contains(&None) {
            let rs: Vec<&Relationship> = self.rels.iter().filter(|(s, _)| s.is_none()).map(|(_, r)| r).collect();
            z.add_deflated("_rels/.rels", &Self::rels_xml(&rs))?;
        }
        for (n, t, b) in &self.parts {
            // XML/text parts are deflated; media already compressed (images) stay stored
            if t.starts_with("image/") { z.add_stored(&n[1..], b)?; } else { z.add_deflated(&n[1..], b)?; }
            let key = Some(n.clone());
            if sources.contains(&key) {
                let rs: Vec<&Relationship> = self.rels.iter().filter(|(s, _)| *s == key).map(|(_, r)| r).collect();
                z.add_deflated(&rels_part_for(Some(n))[1..], &Self::rels_xml(&rs))?;
            }
        }
        Ok(z.finish()?)
    }
}
