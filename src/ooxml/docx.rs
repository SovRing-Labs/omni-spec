//! WordprocessingML (.docx) — ECMA-376 5th ed. Part 1 §17, read into and written from a small document model.
//!
//! Model: headings, paragraphs, nested ordered / bulleted lists, code blocks, quotes, tables, rules, page breaks; inline
//! text with bold / italic / underline / strike / code, hyperlinks, images (bytes + alt text + size), line breaks and
//! note references (footnotes and endnotes become one numbered note list).
//!
//! Read: body paragraphs and tables (§17.3.1.22 p, §17.4.38 tbl), runs and their properties (§17.3.2: b, i, u, strike,
//! dstrike, rStyle, rFonts), text, tab and break elements (§17.3.3: t, tab, br incl. type="page"), hyperlinks
//! (§17.16.22 by relationship id or anchor, §17.16.5.25 HYPERLINK field codes), styles (§17.7: names, basedOn chains,
//! outlineLvl §17.3.1.20, numPr in styles), numbering (§17.9: num → abstractNum, lvl numFmt + start, lvlOverride
//! startOverride), footnotes / endnotes (§17.11), structured document tags and inserted runs (content kept; deleted
//! runs dropped, §17.13.5), DrawingML pictures inline or anchored (§20.4.2.8 / §20.4.2.3: docPr descr / title, extent,
//! a:blip r:embed) and VML imagedata. Merged table cells become empty cells (gridSpan §17.4.17, vMerge §17.4.84).
//! Document title from the core properties part (Part 2 §11, dc:title).
//! Write: the same model with Heading 1–6 / Quote / Source Code / List Paragraph / footnote / hyperlink styles,
//! numbering definitions (one restartable instance per list), footnotes, pictures and hyperlinks. A code block's
//! language is kept in a structured document tag (`w:sdt` with `w:tag w:val="code:<lang>"`, §17.5.2); quotes and the
//! extra blocks of a list item (code, tables, further paragraphs) are wrapped in tags `omnidoc:quote` and
//! `omnidoc:item:<level>` so they read back where they were. Documents without these tags read by style as usual.

use super::opc::{rel, Package, PackageWriter};
use super::Error;
use crate::xml::{self, Element, Writer};
use std::collections::HashMap;

pub const NS_W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const NS_W_STRICT: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NS_R_STRICT: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships";
const NS_WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const NS_PIC: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";
const NS_VML: &str = "urn:schemas-microsoft-com:vml";

// ---------------------------------------------------------------- model

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub code: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    Text { text: String, style: Style },
    Link { href: String, children: Vec<Inline> },
    /// Index into `Document::images`.
    Image(usize),
    Break,
    /// 1-based number into `Document::notes`.
    NoteRef(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Heading { level: u8, inlines: Vec<Inline> },
    Paragraph(Vec<Inline>),
    /// Each item is a list of blocks: its paragraph first, then nested lists or further blocks.
    List { ordered: bool, start: u32, items: Vec<Vec<Block>> },
    Code { lang: String, text: String },
    Quote(Vec<Block>),
    /// Rows of cells; each cell is a run of inlines. `header` = the first row is a header row.
    Table { header: bool, rows: Vec<Vec<Vec<Inline>>> },
    Rule,
    PageBreak,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    /// File name inside the package media folder, e.g. "image1.png".
    pub name: String,
    pub content_type: String,
    pub data: Vec<u8>,
    pub alt: String,
    /// Display size in EMU (914,400 per inch); 0 = unknown.
    pub width_emu: u64,
    pub height_emu: u64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Document {
    pub title: String,
    pub blocks: Vec<Block>,
    pub images: Vec<Image>,
    /// Notes in order of first reference; `Inline::NoteRef(n)` points at `notes[n - 1]`.
    pub notes: Vec<Vec<Block>>,
}

/// Plain text of inlines (images as their alt text in brackets, breaks as newlines, note references as [n]).
pub fn inline_text(v: &[Inline], images: &[Image]) -> String {
    let mut s = String::new();
    for i in v {
        match i {
            Inline::Text { text, .. } => s.push_str(text),
            Inline::Link { children, .. } => s.push_str(&inline_text(children, images)),
            Inline::Image(k) => {
                if let Some(im) = images.get(*k) {
                    s.push_str(&format!("[{}]", im.alt));
                }
            }
            Inline::Break => s.push('\n'),
            Inline::NoteRef(n) => s.push_str(&format!("[{n}]")),
        }
    }
    s
}

/// Merge adjacent text runs with the same style, merge adjacent links to the same target, drop empty text and links.
pub fn normalize_inlines(v: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::with_capacity(v.len());
    for i in v {
        match i {
            Inline::Text { text, .. } if text.is_empty() => {}
            Inline::Text { text, style } => {
                if let Some(Inline::Text { text: prev, style: ps }) = out.last_mut() {
                    if *ps == style {
                        prev.push_str(&text);
                        continue;
                    }
                }
                out.push(Inline::Text { text, style });
            }
            Inline::Link { href, children } => {
                let children = normalize_inlines(children);
                if children.is_empty() {
                    continue;
                }
                if let Some(Inline::Link { href: ph, children: pc }) = out.last_mut() {
                    if *ph == href {
                        let mut all = std::mem::take(pc);
                        all.extend(children);
                        *pc = normalize_inlines(all);
                        continue;
                    }
                }
                out.push(Inline::Link { href, children });
            }
            other => out.push(other),
        }
    }
    out
}

// ---------------------------------------------------------------- reading

fn is_wns(ns: &str) -> bool {
    ns == NS_W || ns == NS_W_STRICT
}

fn is_w(e: &Element, local: &str) -> bool {
    e.name.local == local && is_wns(&e.name.ns)
}

fn w_attr<'a>(e: &'a Element, local: &str) -> Option<&'a str> {
    e.attr_ns(NS_W, local).or_else(|| e.attr_ns(NS_W_STRICT, local)).or_else(|| e.attr_ns("", local))
}

fn r_attr<'a>(e: &'a Element, local: &str) -> Option<&'a str> {
    e.attr_ns(NS_R, local).or_else(|| e.attr_ns(NS_R_STRICT, local))
}

fn wchild<'a>(e: &'a Element, local: &str) -> Option<&'a Element> {
    e.elements().find(|c| is_w(c, local))
}

/// §17.17.4 ST_OnOff: an absent val is on; "0", "false", "off" are off.
fn on(e: Option<&Element>) -> Option<bool> {
    e.map(|x| !matches!(w_attr(x, "val"), Some("0") | Some("false") | Some("off")))
}

const MONO_FONTS: &[&str] = &["courier", "consolas", "mono", "menlo", "monaco", "source code", "lucida console", "fira code", "inconsolata", "jetbrains"];

#[derive(Debug, Clone, Copy, Default)]
struct RunProps {
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strike: Option<bool>,
    mono: Option<bool>,
}

fn run_props(rpr: Option<&Element>) -> RunProps {
    let Some(r) = rpr else { return RunProps::default() };
    let underline = wchild(r, "u").map(|u| !matches!(w_attr(u, "val"), Some("none")));
    let strike = match (on(wchild(r, "strike")), on(wchild(r, "dstrike"))) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(false) || b.unwrap_or(false)),
    };
    let mono = wchild(r, "rFonts").map(|f| {
        [w_attr(f, "ascii"), w_attr(f, "hAnsi"), w_attr(f, "cs")].iter().flatten().any(|n| {
            let l = n.to_ascii_lowercase();
            MONO_FONTS.iter().any(|m| l.contains(m))
        })
    });
    RunProps { bold: on(wchild(r, "b")), italic: on(wchild(r, "i")), underline, strike, mono: mono.filter(|m| *m) }
}

#[derive(Debug, Clone, Default)]
struct StyleDef {
    name: String,
    based_on: Option<String>,
    outline: Option<u8>,
    num: Option<(String, u32)>,
    run: RunProps,
}

struct Styles {
    defs: HashMap<String, StyleDef>,
}

impl Styles {
    fn parse(root: Option<&Element>) -> Styles {
        let mut defs = HashMap::new();
        for s in root.into_iter().flat_map(|r| r.elements().filter(|e| is_w(e, "style"))) {
            let ppr = wchild(s, "pPr");
            let num = ppr.and_then(|p| wchild(p, "numPr")).and_then(|n| {
                let id = wchild(n, "numId").and_then(|x| w_attr(x, "val"))?.to_string();
                let lvl = wchild(n, "ilvl").and_then(|x| w_attr(x, "val")).and_then(|v| v.parse().ok()).unwrap_or(0);
                Some((id, lvl))
            });
            defs.insert(w_attr(s, "styleId").unwrap_or("").to_string(), StyleDef {
                name: wchild(s, "name").and_then(|n| w_attr(n, "val")).unwrap_or("").to_string(),
                based_on: wchild(s, "basedOn").and_then(|n| w_attr(n, "val")).map(String::from),
                outline: ppr.and_then(|p| wchild(p, "outlineLvl")).and_then(|o| w_attr(o, "val")).and_then(|v| v.parse().ok()),
                num,
                run: run_props(wchild(s, "rPr")),
            });
        }
        Styles { defs }
    }

    /// Walk the basedOn chain (bounded against cycles) and return the first `Some` produced by `f`.
    fn find<T>(&self, id: &str, f: impl Fn(&StyleDef) -> Option<T>) -> Option<T> {
        let mut cur = Some(id.to_string());
        for _ in 0..16 {
            let d = self.defs.get(cur.as_deref()?)?;
            if let Some(v) = f(d) {
                return Some(v);
            }
            cur = d.based_on.clone();
        }
        None
    }

    /// Style id and names along the basedOn chain, lower-cased without spaces.
    fn names(&self, id: &str) -> Vec<String> {
        let mut v = vec![id.to_ascii_lowercase()];
        let mut cur = Some(id.to_string());
        for _ in 0..16 {
            let Some(d) = cur.as_deref().and_then(|c| self.defs.get(c)) else { break };
            v.push(d.name.to_ascii_lowercase().replace(' ', ""));
            cur = d.based_on.clone();
        }
        v
    }

    fn heading_level(&self, id: &str) -> Option<u8> {
        for n in self.names(id) {
            if n == "title" {
                return Some(1);
            }
            if let Some(d) = n.strip_prefix("heading") {
                if let Ok(l) = d.parse::<u8>() {
                    return Some(l.clamp(1, 6));
                }
            }
        }
        self.find(id, |d| d.outline).filter(|o| *o < 9).map(|o| (o + 1).min(6))
    }

    fn is_code(&self, id: &str) -> bool {
        self.names(id).iter().any(|n| ["preformattedtext", "htmlpreformatted", "sourcecode", "code", "plaintext", "codeblock"].contains(&n.as_str()))
    }

    fn is_quote(&self, id: &str) -> bool {
        self.names(id).iter().any(|n| ["quote", "intensequote", "blockquotation", "quotations", "blocktext"].contains(&n.as_str()))
    }

    fn char_is_code(&self, id: &str) -> bool {
        self.names(id).iter().any(|n| ["sourcetext", "htmlcode", "verbatimchar", "code", "inlinecode", "htmltypewriter"].contains(&n.as_str()))
            || self.find(id, |d| d.run.mono).unwrap_or(false)
    }
}

#[derive(Default)]
struct Numbering {
    /// numId → (abstractNumId, startOverride per level)
    nums: HashMap<String, (String, Vec<(u32, u32)>)>,
    /// abstractNumId → per level (ilvl, ordered, start)
    abstracts: HashMap<String, Vec<(u32, bool, u32)>>,
}

impl Numbering {
    fn parse(root: Option<&Element>) -> Numbering {
        let mut n = Numbering::default();
        let Some(root) = root else { return n };
        for a in root.elements().filter(|e| is_w(e, "abstractNum")) {
            let levels = a.elements().filter(|e| is_w(e, "lvl")).map(|l| {
                let ilvl = w_attr(l, "ilvl").and_then(|v| v.parse().ok()).unwrap_or(0);
                let fmt = wchild(l, "numFmt").and_then(|f| w_attr(f, "val")).unwrap_or("decimal");
                let start = wchild(l, "start").and_then(|f| w_attr(f, "val")).and_then(|v| v.parse().ok()).unwrap_or(1);
                (ilvl, !matches!(fmt, "bullet" | "none"), start)
            }).collect();
            n.abstracts.insert(w_attr(a, "abstractNumId").unwrap_or("").to_string(), levels);
        }
        for m in root.elements().filter(|e| is_w(e, "num")) {
            let abs = wchild(m, "abstractNumId").and_then(|x| w_attr(x, "val")).unwrap_or("").to_string();
            let overrides = m.elements().filter(|e| is_w(e, "lvlOverride")).filter_map(|o| {
                let l = w_attr(o, "ilvl")?.parse().ok()?;
                let s = wchild(o, "startOverride").and_then(|s| w_attr(s, "val"))?.parse().ok()?;
                Some((l, s))
            }).collect();
            n.nums.insert(w_attr(m, "numId").unwrap_or("").to_string(), (abs, overrides));
        }
        n
    }

    /// (ordered, start) of one level of a numbering instance.
    fn kind(&self, num_id: &str, ilvl: u32) -> (bool, u32) {
        let Some((abs, ovr)) = self.nums.get(num_id) else { return (false, 1) };
        let lvl = self.abstracts.get(abs).and_then(|ls| ls.iter().find(|l| l.0 == ilvl).copied());
        let (ordered, start) = lvl.map(|l| (l.1, l.2)).unwrap_or((false, 1));
        (ordered, ovr.iter().find(|o| o.0 == ilvl).map(|o| o.1).unwrap_or(start))
    }
}

/// A paragraph after its runs were collected, before block assembly.
struct Para {
    inlines: Vec<Inline>,
    heading: Option<u8>,
    list: Option<(String, u32)>,
    code: bool,
    quote: bool,
    code_lang: Option<String>,
    rule: bool,
    page_break_before: bool,
    page_breaks_after: usize,
}

#[derive(Default)]
struct FieldState {
    in_instr: bool,
    in_result: bool,
    instr: String,
    href: Option<String>,
    kids: Vec<Inline>,
}

/// §17.16.5.25 HYPERLINK field: `HYPERLINK "url"` or `HYPERLINK \l "anchor"`.
fn hyperlink_target(instr: &str) -> Option<String> {
    let rest = instr.trim().strip_prefix("HYPERLINK")?.trim();
    let anchor = rest.contains("\\l");
    let q: Vec<&str> = rest.split('"').collect();
    let url = if q.len() >= 2 { q[1].to_string() } else { rest.split_whitespace().find(|w| !w.starts_with('\\'))?.to_string() };
    Some(if anchor && !url.starts_with('#') { format!("#{url}") } else { url })
}

struct Ctx<'p, 'a> {
    pkg: &'p Package<'a>,
    /// the part whose relationships resolve r:id references (document, footnotes or endnotes)
    part: String,
    rels: Vec<super::opc::Relationship>,
    styles: Styles,
    numbering: Numbering,
    images: Vec<Image>,
    image_by_part: HashMap<String, usize>,
    /// (kind "f" or "e", source id) in order of first reference
    note_ids: Vec<(String, String)>,
    /// inside an `omnidoc:quote` tag: Quote-styled paragraphs are already grouped
    in_quote: bool,
}

enum BodyItem {
    P(Para),
    B(Block),
    /// blocks that continue the last item of the list at this level (tag `omnidoc:item:<level>`)
    Cont(usize, Vec<Block>),
}

impl<'p, 'a> Ctx<'p, 'a> {
    fn image(&mut self, rid: &str, alt: &str, cx: u64, cy: u64) -> Option<usize> {
        let r = self.rels.iter().find(|r| r.id == rid && !r.external)?;
        let part = super::opc::resolve(Some(&self.part), &r.target);
        if let Some(&i) = self.image_by_part.get(&part) {
            return Some(i);
        }
        let data = self.pkg.part(&part).ok()?;
        let ct = self.pkg.content_type(&part).unwrap_or("application/octet-stream").to_string();
        let name = part.rsplit('/').next().unwrap_or("image").to_string();
        self.images.push(Image { name, content_type: ct, data, alt: alt.to_string(), width_emu: cx, height_emu: cy });
        self.image_by_part.insert(part, self.images.len() - 1);
        Some(self.images.len() - 1)
    }

    fn note(&mut self, kind: &str, id: &str) -> usize {
        let key = (kind.to_string(), id.to_string());
        match self.note_ids.iter().position(|k| *k == key) {
            Some(i) => i + 1,
            None => {
                self.note_ids.push(key);
                self.note_ids.len()
            }
        }
    }

    fn drawing(&mut self, d: &Element) -> Vec<Inline> {
        let mut out = Vec::new();
        for holder in d.elements().filter(|e| e.name.ns == NS_WP && (e.name.local == "inline" || e.name.local == "anchor")) {
            let doc_pr = holder.child(NS_WP, "docPr");
            let descr = doc_pr.and_then(|p| p.attr_ns("", "descr")).unwrap_or("");
            let title = doc_pr.and_then(|p| p.attr_ns("", "title")).unwrap_or("");
            let alt = if !descr.is_empty() { descr } else { title }.trim().to_string();
            let ext = holder.child(NS_WP, "extent");
            let cx = ext.and_then(|e| e.attr_ns("", "cx")).and_then(|v| v.parse().ok()).unwrap_or(0);
            let cy = ext.and_then(|e| e.attr_ns("", "cy")).and_then(|v| v.parse().ok()).unwrap_or(0);
            for blip in holder.descendants(NS_A, "blip") {
                if let Some(i) = r_attr(blip, "embed").and_then(|rid| self.image(rid, &alt, cx, cy)) {
                    out.push(Inline::Image(i));
                }
            }
        }
        out
    }

    fn vml(&mut self, pict: &Element) -> Vec<Inline> {
        let mut out = Vec::new();
        for e in pict.descendants(NS_VML, "imagedata") {
            let alt = e.attr_ns("", "title").unwrap_or("").to_string();
            if let Some(i) = r_attr(e, "id").and_then(|rid| self.image(rid, &alt, 0, 0)) {
                out.push(Inline::Image(i));
            }
        }
        out
    }

    /// Run containers of a paragraph (hyperlink, fldSimple, sdt, ins, smartTag) into inlines.
    fn runs(&mut self, parent: &Element, out: &mut Vec<Inline>, para: &mut Para, field: &mut FieldState) {
        for c in parent.elements() {
            if !is_wns(&c.name.ns) {
                continue; // mc:AlternateContent (text boxes, shapes) and extension elements are skipped
            }
            match c.name.local.as_str() {
                "r" => self.run(c, out, para, field),
                "hyperlink" => {
                    let href = match r_attr(c, "id").and_then(|id| self.rels.iter().find(|r| r.id == id)) {
                        Some(r) => r.target.clone(),
                        None => w_attr(c, "anchor").map(|a| format!("#{a}")).unwrap_or_default(),
                    };
                    let mut kids = Vec::new();
                    self.runs(c, &mut kids, para, field);
                    if href.is_empty() { out.extend(kids) } else { out.push(Inline::Link { href, children: kids }) }
                }
                "fldSimple" => {
                    let mut kids = Vec::new();
                    self.runs(c, &mut kids, para, field);
                    match hyperlink_target(w_attr(c, "instr").unwrap_or("")) {
                        Some(h) => out.push(Inline::Link { href: h, children: kids }),
                        None => out.extend(kids),
                    }
                }
                "sdt" => {
                    if let Some(content) = wchild(c, "sdtContent") {
                        self.runs(content, out, para, field);
                    }
                }
                "ins" | "smartTag" | "customXml" | "moveTo" => self.runs(c, out, para, field),
                _ => {}
            }
        }
    }

    fn run(&mut self, r: &Element, out: &mut Vec<Inline>, para: &mut Para, field: &mut FieldState) {
        let rpr = wchild(r, "rPr");
        let direct = run_props(rpr);
        let rstyle = rpr.and_then(|p| wchild(p, "rStyle")).and_then(|s| w_attr(s, "val")).unwrap_or("").to_string();
        let from_style = |f: fn(&RunProps) -> Option<bool>| -> bool {
            f(&direct).unwrap_or_else(|| !rstyle.is_empty() && self.styles.find(&rstyle, |d| f(&d.run)).unwrap_or(false))
        };
        let style = Style {
            bold: from_style(|p| p.bold),
            italic: from_style(|p| p.italic),
            // the Hyperlink character style underlines by definition: that is the link, not emphasis
            underline: from_style(|p| p.underline) && !rstyle.eq_ignore_ascii_case("hyperlink") && !rstyle.eq_ignore_ascii_case("internetlink"),
            strike: from_style(|p| p.strike),
            code: direct.mono.unwrap_or(false) || (!rstyle.is_empty() && self.styles.char_is_code(&rstyle)),
        };
        let emit = |out: &mut Vec<Inline>, field: &mut FieldState, i: Inline| {
            if field.in_result && field.href.is_some() { field.kids.push(i) } else { out.push(i) }
        };
        for c in r.elements() {
            if !is_wns(&c.name.ns) {
                continue;
            }
            match c.name.local.as_str() {
                "t" => emit(out, field, Inline::Text { text: c.text(), style }),
                "tab" | "ptab" => emit(out, field, Inline::Text { text: "\t".into(), style }),
                "noBreakHyphen" => emit(out, field, Inline::Text { text: "-".into(), style }),
                "br" | "cr" => match w_attr(c, "type") {
                    Some("page") => {
                        if out.iter().any(|i| !matches!(i, Inline::Text { text, .. } if text.trim().is_empty())) {
                            para.page_breaks_after += 1;
                        } else {
                            para.page_break_before = true;
                        }
                    }
                    Some("column") => {}
                    _ => emit(out, field, Inline::Break),
                },
                "drawing" => {
                    for i in self.drawing(c) {
                        emit(out, field, i);
                    }
                }
                "pict" | "object" => {
                    for i in self.vml(c) {
                        emit(out, field, i);
                    }
                }
                "footnoteReference" | "endnoteReference" => {
                    let kind = if c.name.local.starts_with("foot") { "f" } else { "e" };
                    let n = self.note(kind, w_attr(c, "id").unwrap_or(""));
                    emit(out, field, Inline::NoteRef(n));
                }
                "fldChar" => match w_attr(c, "fldCharType") {
                    Some("begin") => *field = FieldState { in_instr: true, ..FieldState::default() },
                    Some("separate") => {
                        field.in_instr = false;
                        field.in_result = true;
                        field.href = hyperlink_target(&field.instr);
                    }
                    Some("end") => {
                        if let Some(h) = field.href.take() {
                            out.push(Inline::Link { href: h, children: std::mem::take(&mut field.kids) });
                        }
                        *field = FieldState::default();
                    }
                    _ => {}
                },
                "instrText" if field.in_instr => field.instr.push_str(&c.text()),
                _ => {}
            }
        }
    }

    fn paragraph(&mut self, p: &Element) -> Para {
        let ppr = wchild(p, "pPr");
        let style = ppr.and_then(|x| wchild(x, "pStyle")).and_then(|s| w_attr(s, "val")).unwrap_or("").to_string();
        let outline = ppr.and_then(|x| wchild(x, "outlineLvl")).and_then(|o| w_attr(o, "val")).and_then(|v| v.parse::<u8>().ok());
        let heading = match outline {
            Some(o) if o < 9 => Some((o + 1).min(6)),
            Some(_) => None,
            None if style.is_empty() => None,
            None => self.styles.heading_level(&style),
        };
        let num = ppr.and_then(|x| wchild(x, "numPr")).map(|n| {
            let id = wchild(n, "numId").and_then(|x| w_attr(x, "val")).unwrap_or("0").to_string();
            let lvl = wchild(n, "ilvl").and_then(|x| w_attr(x, "val")).and_then(|v| v.parse().ok()).unwrap_or(0);
            (id, lvl)
        }).or_else(|| if style.is_empty() { None } else { self.styles.find(&style, |d| d.num.clone()) });
        let list = num.filter(|(id, _)| heading.is_none() && self.numbering.nums.contains_key(id) && id != "0");
        let mut para = Para {
            inlines: Vec::new(),
            heading,
            list,
            code: !style.is_empty() && self.styles.is_code(&style),
            quote: !self.in_quote && !style.is_empty() && self.styles.is_quote(&style),
            code_lang: None,
            rule: ppr.and_then(|x| wchild(x, "pBdr")).and_then(|b| wchild(b, "bottom")).is_some(),
            page_break_before: on(ppr.and_then(|x| wchild(x, "pageBreakBefore"))).unwrap_or(false),
            page_breaks_after: 0,
        };
        let mut out = Vec::new();
        let mut field = FieldState::default();
        self.runs(p, &mut out, &mut para, &mut field);
        para.inlines = normalize_inlines(out);
        para
    }

    fn table(&mut self, t: &Element) -> Block {
        let mut rows = Vec::new();
        let mut header = false;
        for (ri, tr) in t.elements().filter(|e| is_w(e, "tr")).enumerate() {
            if ri == 0 {
                header = wchild(tr, "trPr").and_then(|p| wchild(p, "tblHeader")).is_some();
            }
            let mut cells = Vec::new();
            for tc in tr.elements() {
                let tc = if is_w(tc, "tc") {
                    tc
                } else if is_w(tc, "sdt") {
                    match wchild(tc, "sdtContent").and_then(|c| wchild(c, "tc")) {
                        Some(x) => x,
                        None => continue,
                    }
                } else {
                    continue;
                };
                let tcpr = wchild(tc, "tcPr");
                let span: usize = tcpr.and_then(|p| wchild(p, "gridSpan")).and_then(|g| w_attr(g, "val")).and_then(|v| v.parse().ok()).unwrap_or(1).clamp(1, 64);
                let continued = tcpr.and_then(|p| wchild(p, "vMerge")).map(|v| w_attr(v, "val").unwrap_or("continue") == "continue").unwrap_or(false);
                let mut inl = Vec::new();
                if !continued {
                    for p in tc.elements().filter(|e| is_w(e, "p")) {
                        let para = self.paragraph(p);
                        if para.inlines.is_empty() {
                            continue;
                        }
                        if !inl.is_empty() {
                            inl.push(Inline::Break);
                        }
                        inl.extend(para.inlines);
                    }
                }
                cells.push(normalize_inlines(inl));
                for _ in 1..span {
                    cells.push(Vec::new());
                }
            }
            rows.push(cells);
        }
        let width = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        for r in rows.iter_mut() {
            r.resize(width, Vec::new());
        }
        Block::Table { header, rows }
    }

    fn collect(&mut self, parent: &Element, items: &mut Vec<BodyItem>, code_lang: Option<&str>) {
        for c in parent.elements() {
            if is_w(c, "p") {
                let mut p = self.paragraph(c);
                if let Some(l) = code_lang {
                    p.code_lang = Some(l.to_string());
                }
                items.push(BodyItem::P(p));
            } else if is_w(c, "tbl") {
                items.push(BodyItem::B(self.table(c)));
            } else if is_w(c, "sdt") {
                let tag = wchild(c, "sdtPr").and_then(|p| wchild(p, "tag")).and_then(|t| w_attr(t, "val")).unwrap_or("").to_string();
                let Some(content) = wchild(c, "sdtContent") else { continue };
                if tag == "omnidoc:quote" {
                    let saved = std::mem::replace(&mut self.in_quote, true);
                    let inner = self.body(content);
                    self.in_quote = saved;
                    items.push(BodyItem::B(Block::Quote(inner)));
                } else if let Some(lvl) = tag.strip_prefix("omnidoc:item:").and_then(|l| l.parse::<usize>().ok()) {
                    let saved = std::mem::replace(&mut self.in_quote, false);
                    let inner = self.body(content);
                    self.in_quote = saved;
                    items.push(BodyItem::Cont(lvl, inner));
                } else if let Some(lang) = tag.strip_prefix("code:") {
                    // one tagged code block: its paragraphs' text, exactly (never merged with a neighbour)
                    let lines: Vec<String> = content.elements().filter(|e| is_w(e, "p")).map(|p| {
                        let para = self.paragraph(p);
                        inline_text(&para.inlines, &[])
                    }).collect();
                    items.push(BodyItem::B(Block::Code { lang: lang.to_string(), text: lines.join("\n") }));
                } else {
                    self.collect(content, items, code_lang);
                }
            } else if is_w(c, "ins") || is_w(c, "customXml") {
                self.collect(c, items, code_lang);
            }
        }
    }

    fn body(&mut self, parent: &Element) -> Vec<Block> {
        let mut items = Vec::new();
        self.collect(parent, &mut items, None);
        assemble(items, &self.numbering)
    }
}

/// Turn the flat paragraph sequence into blocks: lists nest by ilvl, consecutive code / quote paragraphs merge.
fn assemble(items: Vec<BodyItem>, numbering: &Numbering) -> Vec<Block> {
    struct Open {
        num: String,
        ordered: bool,
        start: u32,
        items: Vec<Vec<Block>>,
    }
    struct St {
        out: Vec<Block>,
        lists: Vec<Open>,
        code: Option<(String, Vec<String>)>,
        quote: Option<Vec<Block>>,
    }
    impl St {
        fn close_lists(&mut self, depth: usize) {
            while self.lists.len() > depth {
                let l = self.lists.pop().unwrap();
                let b = Block::List { ordered: l.ordered, start: l.start, items: l.items };
                match self.lists.last_mut() {
                    Some(parent) => {
                        if parent.items.is_empty() {
                            parent.items.push(Vec::new());
                        }
                        parent.items.last_mut().unwrap().push(b);
                    }
                    None => self.out.push(b),
                }
            }
        }
        fn flush_code(&mut self) {
            if let Some((lang, lines)) = self.code.take() {
                self.out.push(Block::Code { lang, text: lines.join("\n") });
            }
        }
        fn flush_quote(&mut self) {
            if let Some(q) = self.quote.take() {
                self.out.push(Block::Quote(q));
            }
        }
        fn flush_all(&mut self) {
            self.close_lists(0);
            self.flush_code();
            self.flush_quote();
        }
    }
    let mut st = St { out: Vec::new(), lists: Vec::new(), code: None, quote: None };
    for it in items {
        let p = match it {
            BodyItem::B(b) => {
                st.flush_all();
                st.out.push(b);
                continue;
            }
            BodyItem::Cont(lvl, blocks) => {
                st.flush_code();
                st.flush_quote();
                if st.lists.len() > lvl {
                    st.close_lists(lvl + 1);
                    let l = &mut st.lists[lvl];
                    if l.items.is_empty() {
                        l.items.push(Vec::new());
                    }
                    l.items.last_mut().unwrap().extend(blocks);
                } else {
                    st.flush_all();
                    st.out.extend(blocks);
                }
                continue;
            }
            BodyItem::P(p) => p,
        };
        if p.page_break_before {
            st.flush_all();
            st.out.push(Block::PageBreak);
        }
        if (p.code || p.code_lang.is_some()) && p.list.is_none() && p.heading.is_none() {
            st.close_lists(0);
            st.flush_quote();
            let lang = p.code_lang.clone().unwrap_or_default();
            let text = inline_text(&p.inlines, &[]);
            match st.code.as_mut() {
                Some((l, lines)) if *l == lang => lines.push(text),
                _ => {
                    st.flush_code();
                    st.code = Some((lang, vec![text]));
                }
            }
        } else if let Some((num, lvl)) = &p.list {
            st.flush_code();
            st.flush_quote();
            let depth = (*lvl as usize).min(8) + 1;
            st.close_lists(depth);
            // a different numbering instance at the top level starts a new list
            if depth == 1 && st.lists.len() == 1 && st.lists[0].num != *num {
                st.close_lists(0);
            }
            while st.lists.len() < depth {
                let (ordered, start) = numbering.kind(num, st.lists.len() as u32);
                if st.lists.last().is_some_and(|l| l.items.is_empty()) {
                    st.lists.last_mut().unwrap().items.push(Vec::new());
                }
                st.lists.push(Open { num: num.clone(), ordered, start, items: Vec::new() });
            }
            let item = if p.inlines.is_empty() { Vec::new() } else { vec![Block::Paragraph(p.inlines.clone())] };
            st.lists.last_mut().unwrap().items.push(item);
        } else {
            st.flush_code();
            st.close_lists(0);
            let text = inline_text(&p.inlines, &[]);
            let block = if let Some(l) = p.heading {
                (!p.inlines.is_empty()).then(|| Block::Heading { level: l, inlines: p.inlines.clone() })
            } else if p.rule && text.trim().is_empty() {
                Some(Block::Rule)
            } else if p.inlines.is_empty() {
                None
            } else {
                Some(Block::Paragraph(p.inlines.clone()))
            };
            if p.quote && !matches!(block, Some(Block::Heading { .. })) {
                if let Some(b) = block {
                    st.quote.get_or_insert_with(Vec::new).push(b);
                }
            } else {
                st.flush_quote();
                if let Some(b) = block {
                    st.out.push(b);
                }
            }
        }
        for _ in 0..p.page_breaks_after {
            st.flush_all();
            st.out.push(Block::PageBreak);
        }
    }
    st.flush_all();
    st.out
}

/// Read a .docx package into the model.
pub fn read(bytes: &[u8]) -> Result<Document, Error> {
    let pkg = Package::open(bytes)?;
    let main = pkg.main_part(rel::OFFICE_DOCUMENT)?;
    let related = |rt: &str| -> Option<(String, Element)> {
        let (_, part) = pkg.related(&main, rt).ok()?.into_iter().next()?;
        let root = xml::parse_bytes(&pkg.part(&part).ok()?).ok()?;
        Some((part, root))
    };
    let styles = related(rel::STYLES);
    let numbering = related(rel::NUMBERING);
    let notes_parts = [("f", related(rel::FOOTNOTES)), ("e", related(rel::ENDNOTES))];
    let mut ctx = Ctx {
        pkg: &pkg,
        part: main.clone(),
        rels: pkg.rels(Some(&main))?,
        styles: Styles::parse(styles.as_ref().map(|s| &s.1)),
        numbering: Numbering::parse(numbering.as_ref().map(|n| &n.1)),
        images: Vec::new(),
        image_by_part: HashMap::new(),
        note_ids: Vec::new(),
        in_quote: false,
    };
    let doc = xml::parse_bytes(&pkg.part(&main)?)?;
    let body = wchild(&doc, "body").ok_or_else(|| Error::Missing("w:body".into()))?;
    let blocks = ctx.body(body);
    // notes in reference order; a note can reference another, so iterate until no new ids appear
    let mut notes = Vec::new();
    let mut i = 0;
    while i < ctx.note_ids.len() {
        let (kind, id) = ctx.note_ids[i].clone();
        let src = notes_parts.iter().find(|(k, _)| *k == kind).and_then(|(_, p)| p.as_ref());
        let mut blocks = Vec::new();
        if let Some((part, root)) = src {
            let el = root.elements().find(|e| (is_w(e, "footnote") || is_w(e, "endnote")) && w_attr(e, "id") == Some(id.as_str())).cloned();
            if let Some(el) = el {
                let saved = (std::mem::replace(&mut ctx.part, part.clone()), std::mem::take(&mut ctx.rels));
                ctx.rels = pkg.rels(Some(part)).unwrap_or_default();
                blocks = ctx.body(&el);
                ctx.part = saved.0;
                ctx.rels = saved.1;
                // the note's own number mark (footnoteRef) carries no text; trim the space that follows it
                if let Some(Block::Paragraph(inl)) = blocks.first_mut() {
                    if let Some(Inline::Text { text, .. }) = inl.first_mut() {
                        *text = text.trim_start().to_string();
                    }
                    *inl = normalize_inlines(std::mem::take(inl));
                }
            }
        }
        notes.push(blocks);
        i += 1;
    }
    let title = pkg
        .rels(None)
        .ok()
        .and_then(|rs| rs.into_iter().find(|r| r.is(rel::CORE_PROPERTIES)))
        .and_then(|r| pkg.part(&super::opc::resolve(None, &r.target)).ok())
        .and_then(|b| xml::parse_bytes(&b).ok())
        .and_then(|root| root.elements().find(|e| e.name.local == "title").map(|t| t.text().trim().to_string()))
        .unwrap_or_default();
    Ok(Document { title, blocks, images: ctx.images, notes })
}

// ---------------------------------------------------------------- writing

const CT_DOC: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
const CT_STYLES: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml";
const CT_NUMBERING: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml";
const CT_FOOTNOTES: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml";
const CT_CORE: &str = "application/vnd.openxmlformats-package.core-properties+xml";

/// Pixel size of a PNG, GIF or JPEG from its header (PNG IHDR, GIF logical screen descriptor, JPEG SOFn).
pub fn image_pixels(b: &[u8]) -> Option<(u32, u32)> {
    if b.starts_with(b"\x89PNG\r\n\x1a\n") && b.len() >= 24 {
        return Some((u32::from_be_bytes(b[16..20].try_into().ok()?), u32::from_be_bytes(b[20..24].try_into().ok()?)));
    }
    if (b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a")) && b.len() >= 10 {
        return Some((u16::from_le_bytes([b[6], b[7]]) as u32, u16::from_le_bytes([b[8], b[9]]) as u32));
    }
    if b.starts_with(&[0xFF, 0xD8]) {
        let mut i = 2;
        while i + 9 < b.len() {
            if b[i] != 0xFF {
                i += 1;
                continue;
            }
            let m = b[i + 1];
            if matches!(m, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF) {
                return Some((u16::from_be_bytes([b[i + 7], b[i + 8]]) as u32, u16::from_be_bytes([b[i + 5], b[i + 6]]) as u32));
            }
            if m == 0xD8 || m == 0x01 || m == 0xFF || (0xD0..=0xD7).contains(&m) {
                i += if m == 0xFF { 1 } else { 2 };
                continue;
            }
            i += 2 + u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
        }
    }
    None
}

fn ext_for(ct: &str, name: &str) -> String {
    match ct {
        "image/png" => "png".into(),
        "image/jpeg" => "jpeg".into(),
        "image/gif" => "gif".into(),
        "image/webp" => "webp".into(),
        "image/svg+xml" => "svg".into(),
        "image/bmp" => "bmp".into(),
        _ => name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).filter(|e| e.chars().all(|c| c.is_ascii_alphanumeric())).unwrap_or_else(|| "bin".into()),
    }
}

struct W<'d> {
    doc: &'d Document,
    pkg: PackageWriter,
    /// the part being written: relationship ids are per part
    part: String,
    rids: HashMap<(String, String), String>,
    media: Vec<String>,
    nums: Vec<(bool, u32)>,
    pic_id: u32,
}

impl<'d> W<'d> {
    fn rid(&mut self, rel_type: &str, target: &str, external: bool) -> String {
        let key = (self.part.clone(), format!("{rel_type} {target}"));
        if let Some(id) = self.rids.get(&key) {
            return id.clone();
        }
        let id = self.pkg.add_rel(Some(&self.part.clone()), rel_type, target, external);
        self.rids.insert(key, id.clone());
        id
    }

    fn rpr(w: &mut Writer, s: &Style, char_style: Option<&str>) {
        if !(s.bold || s.italic || s.underline || s.strike || s.code || char_style.is_some()) {
            return;
        }
        w.start("w:rPr");
        if let Some(st) = char_style.or(if s.code { Some("SourceText") } else { None }) {
            w.empty("w:rStyle", &[("w:val", st)]);
        }
        if s.code && char_style.is_some() {
            w.empty("w:rFonts", &[("w:ascii", "Courier New"), ("w:hAnsi", "Courier New"), ("w:cs", "Courier New")]);
        }
        if s.bold {
            w.empty("w:b", &[]);
        }
        if s.italic {
            w.empty("w:i", &[]);
        }
        if s.strike {
            w.empty("w:strike", &[]);
        }
        if s.underline {
            w.empty("w:u", &[("w:val", "single")]);
        }
        w.end();
    }

    /// One run; tabs and newlines in the text become w:tab / w:br (§17.3.3).
    fn text_run(w: &mut Writer, text: &str, s: &Style, char_style: Option<&str>) {
        w.start("w:r");
        Self::rpr(w, s, char_style);
        for (li, line) in text.split('\n').enumerate() {
            if li > 0 {
                w.empty("w:br", &[]);
            }
            for (k, part) in line.split('\t').enumerate() {
                if k > 0 {
                    w.empty("w:tab", &[]);
                }
                if !part.is_empty() {
                    w.leaf("w:t", &[("xml:space", "preserve")], part);
                }
            }
        }
        w.end();
    }

    fn inlines(&mut self, w: &mut Writer, v: &[Inline], in_link: bool) {
        for i in v {
            match i {
                Inline::Text { text, style } => Self::text_run(w, text, style, if in_link { Some("Hyperlink") } else { None }),
                Inline::Break => {
                    w.start("w:r").empty("w:br", &[]).end();
                }
                Inline::NoteRef(n) => {
                    w.start("w:r").start("w:rPr").empty("w:rStyle", &[("w:val", "FootnoteReference")]).end();
                    w.empty("w:footnoteReference", &[("w:id", &(n + 1).to_string())]).end();
                }
                Inline::Link { href, children } => {
                    w.start("w:hyperlink");
                    if let Some(a) = href.strip_prefix('#') {
                        w.attr("w:anchor", a);
                    } else {
                        let id = self.rid(rel::HYPERLINK, href, true);
                        w.attr("r:id", &id);
                    }
                    w.attr("w:history", "1");
                    self.inlines(w, children, true);
                    w.end();
                }
                Inline::Image(k) => self.picture(w, *k),
            }
        }
    }

    fn picture(&mut self, w: &mut Writer, k: usize) {
        let Some(im) = self.doc.images.get(k) else { return };
        let rid = self.rid(rel::IMAGE, &format!("media/{}", self.media[k]), false);
        let (mut cx, mut cy) = (im.width_emu, im.height_emu);
        if cx == 0 || cy == 0 {
            let (pw, ph) = image_pixels(&im.data).unwrap_or((300, 200));
            cx = pw as u64 * 9525; // 96 dpi
            cy = ph as u64 * 9525;
        }
        let max = 5_486_400; // 6 inches of text width
        if cx > max {
            cy = cy * max / cx;
            cx = max;
        }
        self.pic_id += 1;
        let id = self.pic_id.to_string();
        let (cxs, cys) = (cx.to_string(), cy.to_string());
        w.start("w:r").start("w:drawing").start("wp:inline").attr("distT", "0").attr("distB", "0").attr("distL", "0").attr("distR", "0");
        w.empty("wp:extent", &[("cx", &cxs), ("cy", &cys)]);
        w.empty("wp:docPr", &[("id", &id), ("name", &format!("Picture {id}")), ("descr", &im.alt)]);
        w.start("a:graphic").start("a:graphicData").attr("uri", NS_PIC);
        w.start("pic:pic");
        w.start("pic:nvPicPr").empty("pic:cNvPr", &[("id", &id), ("name", &self.media[k]), ("descr", &im.alt)]).empty("pic:cNvPicPr", &[]).end();
        w.start("pic:blipFill").empty("a:blip", &[("r:embed", &rid)]).start("a:stretch").empty("a:fillRect", &[]).end().end();
        w.start("pic:spPr").start("a:xfrm").empty("a:off", &[("x", "0"), ("y", "0")]).empty("a:ext", &[("cx", &cxs), ("cy", &cys)]).end();
        w.start("a:prstGeom").attr("prst", "rect").empty("a:avLst", &[]).end().end();
        w.end().end().end(); // pic:pic, a:graphicData, a:graphic
        w.end().end().end(); // wp:inline, w:drawing, w:r
    }

    fn para(&mut self, w: &mut Writer, style: Option<&str>, num: Option<(u32, u32)>, inl: &[Inline]) {
        w.start("w:p");
        if style.is_some() || num.is_some() {
            w.start("w:pPr");
            if let Some(s) = style {
                w.empty("w:pStyle", &[("w:val", s)]);
            }
            if let Some((id, lvl)) = num {
                w.start("w:numPr").empty("w:ilvl", &[("w:val", &lvl.to_string())]).empty("w:numId", &[("w:val", &id.to_string())]).end();
            }
            w.end();
        }
        self.inlines(w, inl, false);
        w.end();
    }

    fn blocks(&mut self, w: &mut Writer, blocks: &[Block], quote: bool) {
        for b in blocks {
            match b {
                Block::Heading { level, inlines } => {
                    let s = format!("Heading{}", (*level).clamp(1, 6));
                    self.para(w, Some(&s), None, inlines);
                }
                Block::Paragraph(inl) => self.para(w, if quote { Some("Quote") } else { None }, None, inl),
                Block::List { .. } => self.list(w, b, 0, None),
                Block::Code { lang, text } => {
                    // always tagged, so two code blocks in a row stay two blocks
                    w.start("w:sdt").start("w:sdtPr").empty("w:tag", &[("w:val", &format!("code:{lang}"))]).end().start("w:sdtContent");
                    for line in text.split('\n') {
                        w.start("w:p").start("w:pPr").empty("w:pStyle", &[("w:val", "SourceCode")]).end();
                        if !line.is_empty() {
                            Self::text_run(w, line, &Style::default(), None);
                        }
                        w.end();
                    }
                    w.end().end();
                }
                Block::Quote(inner) => {
                    Self::sdt_open(w, "omnidoc:quote");
                    self.blocks(w, inner, true);
                    w.end().end();
                }
                Block::Table { header, rows } => self.table(w, *header, rows),
                Block::Rule => {
                    w.start("w:p").start("w:pPr").start("w:pBdr");
                    w.empty("w:bottom", &[("w:val", "single"), ("w:sz", "6"), ("w:space", "1"), ("w:color", "auto")]);
                    w.end().end().end();
                }
                Block::PageBreak => {
                    w.start("w:p").start("w:r").empty("w:br", &[("w:type", "page")]).end().end();
                }
            }
        }
    }

    fn sdt_open(w: &mut Writer, tag: &str) {
        w.start("w:sdt").start("w:sdtPr").empty("w:tag", &[("w:val", tag)]).end().start("w:sdtContent");
    }

    fn list(&mut self, w: &mut Writer, b: &Block, lvl: u32, parent: Option<u32>) {
        let Block::List { ordered, start, items } = b else { return };
        // a top-level list gets its own numbering instance (so ordered lists restart); a nested list of the same
        // kind shares its parent's instance at the next level, otherwise it gets its own
        let num_id = match parent {
            Some(id) if self.nums[(id - 1) as usize].0 == *ordered && *start == 1 => id,
            _ => {
                self.nums.push((*ordered, *start));
                self.nums.len() as u32
            }
        };
        let lvl = lvl.min(8);
        for item in items {
            let mut first = true;
            for blk in item {
                match blk {
                    Block::Paragraph(inl) if first => self.para(w, Some("ListParagraph"), Some((num_id, lvl)), inl),
                    Block::List { .. } => {
                        if first {
                            self.para(w, Some("ListParagraph"), Some((num_id, lvl)), &[]);
                        }
                        self.list(w, blk, lvl + 1, Some(num_id))
                    }
                    other => {
                        if first {
                            self.para(w, Some("ListParagraph"), Some((num_id, lvl)), &[]);
                        }
                        // further blocks of this item, tagged with the item's level so they read back inside it
                        Self::sdt_open(w, &format!("omnidoc:item:{lvl}"));
                        match other {
                            Block::Paragraph(inl) => self.para(w, Some("ListParagraph"), None, inl),
                            _ => self.blocks(w, std::slice::from_ref(other), false),
                        }
                        w.end().end();
                    }
                }
                first = false;
            }
            if item.is_empty() {
                self.para(w, Some("ListParagraph"), Some((num_id, lvl)), &[]);
            }
        }
    }

    fn table(&mut self, w: &mut Writer, header: bool, rows: &[Vec<Vec<Inline>>]) {
        let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0).max(1);
        let width = (9360 / cols as u32).to_string(); // 6.5 in of text width, in twentieths of a point
        w.start("w:tbl").start("w:tblPr").empty("w:tblStyle", &[("w:val", "TableGrid")]).empty("w:tblW", &[("w:w", "5000"), ("w:type", "pct")]);
        w.empty("w:tblLook", &[("w:val", "04A0"), ("w:firstRow", "1"), ("w:lastRow", "0"), ("w:firstColumn", "0"), ("w:lastColumn", "0"), ("w:noHBand", "0"), ("w:noVBand", "1")]).end();
        w.start("w:tblGrid");
        for _ in 0..cols {
            w.empty("w:gridCol", &[("w:w", &width)]);
        }
        w.end();
        for (ri, row) in rows.iter().enumerate() {
            w.start("w:tr");
            if ri == 0 && header {
                w.start("w:trPr").empty("w:tblHeader", &[]).end();
            }
            for c in 0..cols {
                w.start("w:tc").start("w:tcPr").empty("w:tcW", &[("w:w", &width), ("w:type", "dxa")]).end();
                // breaks in a cell become separate paragraphs; the reader joins them back with breaks
                let mut chunks: Vec<Vec<Inline>> = vec![Vec::new()];
                for i in row.get(c).map(|v| v.as_slice()).unwrap_or(&[]) {
                    if matches!(i, Inline::Break) {
                        chunks.push(Vec::new());
                    } else {
                        chunks.last_mut().unwrap().push(i.clone());
                    }
                }
                for ch in chunks {
                    self.para(w, None, None, &ch);
                }
                w.end();
            }
            w.end();
        }
        w.end();
    }
}

fn styles_xml() -> String {
    let mut w = Writer::new();
    w.decl().start("w:styles").attr("xmlns:w", NS_W);
    w.start("w:docDefaults").start("w:rPrDefault").start("w:rPr");
    w.empty("w:rFonts", &[("w:ascii", "Calibri"), ("w:hAnsi", "Calibri"), ("w:eastAsia", "Calibri"), ("w:cs", "Calibri")]);
    w.empty("w:sz", &[("w:val", "22")]).empty("w:szCs", &[("w:val", "22")]).empty("w:lang", &[("w:val", "en-US")]);
    w.end().end().start("w:pPrDefault").start("w:pPr").empty("w:spacing", &[("w:after", "120"), ("w:line", "264"), ("w:lineRule", "auto")]).end().end().end();
    let pstyle = |w: &mut Writer, id: &str, name: &str, outline: Option<u8>, size: Option<u32>, ind: Option<(&str, &str)>| {
        w.start("w:style").attr("w:type", "paragraph").attr("w:styleId", id);
        if id == "Normal" {
            w.attr("w:default", "1");
        }
        w.empty("w:name", &[("w:val", name)]);
        if id != "Normal" {
            w.empty("w:basedOn", &[("w:val", "Normal")]).empty("w:next", &[("w:val", "Normal")]);
        }
        w.empty("w:qFormat", &[]);
        w.start("w:pPr");
        if outline.is_some() {
            w.empty("w:keepNext", &[]).empty("w:spacing", &[("w:before", "240"), ("w:after", "80")]);
        }
        if let Some((left, right)) = ind {
            w.empty("w:ind", &[("w:left", left), ("w:right", right)]);
        }
        if let Some(o) = outline {
            w.empty("w:outlineLvl", &[("w:val", &o.to_string())]);
        }
        w.end();
        if let Some(sz) = size {
            w.start("w:rPr").empty("w:b", &[]).empty("w:sz", &[("w:val", &sz.to_string())]).empty("w:szCs", &[("w:val", &sz.to_string())]).end();
        }
        w.end();
    };
    pstyle(&mut w, "Normal", "Normal", None, None, None);
    for (l, sz) in [(1u8, 36u32), (2, 30), (3, 26), (4, 24), (5, 22), (6, 22)] {
        pstyle(&mut w, &format!("Heading{l}"), &format!("heading {l}"), Some(l - 1), Some(sz), None);
    }
    pstyle(&mut w, "Quote", "Quote", None, None, Some(("720", "720")));
    pstyle(&mut w, "ListParagraph", "List Paragraph", None, None, Some(("720", "0")));
    w.start("w:style").attr("w:type", "paragraph").attr("w:styleId", "SourceCode").empty("w:name", &[("w:val", "Source Code")]).empty("w:basedOn", &[("w:val", "Normal")]).empty("w:qFormat", &[]);
    w.start("w:pPr").empty("w:spacing", &[("w:after", "0"), ("w:line", "240"), ("w:lineRule", "auto")]).empty("w:ind", &[("w:left", "284")]).end();
    w.start("w:rPr").empty("w:rFonts", &[("w:ascii", "Courier New"), ("w:hAnsi", "Courier New"), ("w:cs", "Courier New")]).empty("w:sz", &[("w:val", "19")]).end().end();
    w.start("w:style").attr("w:type", "paragraph").attr("w:styleId", "FootnoteText").empty("w:name", &[("w:val", "footnote text")]).empty("w:basedOn", &[("w:val", "Normal")]);
    w.start("w:pPr").empty("w:spacing", &[("w:after", "0")]).end().start("w:rPr").empty("w:sz", &[("w:val", "18")]).end().end();
    w.start("w:style").attr("w:type", "character").attr("w:default", "1").attr("w:styleId", "DefaultParagraphFont").empty("w:name", &[("w:val", "Default Paragraph Font")]).end();
    w.start("w:style").attr("w:type", "character").attr("w:styleId", "Hyperlink").empty("w:name", &[("w:val", "Hyperlink")]).empty("w:basedOn", &[("w:val", "DefaultParagraphFont")]);
    w.start("w:rPr").empty("w:color", &[("w:val", "0563C1")]).empty("w:u", &[("w:val", "single")]).end().end();
    w.start("w:style").attr("w:type", "character").attr("w:styleId", "SourceText").empty("w:name", &[("w:val", "Source Text")]).empty("w:basedOn", &[("w:val", "DefaultParagraphFont")]);
    w.start("w:rPr").empty("w:rFonts", &[("w:ascii", "Courier New"), ("w:hAnsi", "Courier New"), ("w:cs", "Courier New")]).end().end();
    w.start("w:style").attr("w:type", "character").attr("w:styleId", "FootnoteReference").empty("w:name", &[("w:val", "footnote reference")]).empty("w:basedOn", &[("w:val", "DefaultParagraphFont")]);
    w.start("w:rPr").empty("w:vertAlign", &[("w:val", "superscript")]).end().end();
    w.start("w:style").attr("w:type", "table").attr("w:styleId", "TableGrid").empty("w:name", &[("w:val", "Table Grid")]);
    w.start("w:tblPr").start("w:tblBorders");
    for side in ["top", "left", "bottom", "right", "insideH", "insideV"] {
        w.empty(&format!("w:{side}"), &[("w:val", "single"), ("w:sz", "4"), ("w:space", "0"), ("w:color", "auto")]);
    }
    w.end().start("w:tblCellMar").empty("w:left", &[("w:w", "108"), ("w:type", "dxa")]).empty("w:right", &[("w:w", "108"), ("w:type", "dxa")]).end().end().end();
    w.finish()
}

fn numbering_xml(nums: &[(bool, u32)]) -> String {
    let mut w = Writer::new();
    w.decl().start("w:numbering").attr("xmlns:w", NS_W);
    let bullets = ["•", "◦", "▪"];
    let formats = ["decimal", "lowerLetter", "lowerRoman"];
    for (abs, ordered) in [(0u32, false), (1, true)] {
        w.start("w:abstractNum").attr("w:abstractNumId", &abs.to_string()).empty("w:multiLevelType", &[("w:val", "hybridMultilevel")]);
        for l in 0..9u32 {
            w.start("w:lvl").attr("w:ilvl", &l.to_string()).empty("w:start", &[("w:val", "1")]);
            if ordered {
                w.empty("w:numFmt", &[("w:val", formats[(l % 3) as usize])]).empty("w:lvlText", &[("w:val", &format!("%{}.", l + 1))]);
            } else {
                w.empty("w:numFmt", &[("w:val", "bullet")]).empty("w:lvlText", &[("w:val", bullets[(l % 3) as usize])]);
            }
            w.empty("w:lvlJc", &[("w:val", "left")]);
            w.start("w:pPr").empty("w:ind", &[("w:left", &(720 * (l + 1)).to_string()), ("w:hanging", "360")]).end();
            w.end();
        }
        w.end();
    }
    for (i, (ordered, start)) in nums.iter().enumerate() {
        w.start("w:num").attr("w:numId", &(i + 1).to_string()).empty("w:abstractNumId", &[("w:val", if *ordered { "1" } else { "0" })]);
        // every instance restarts at its own start value (§17.9.27 lvlOverride, §17.9.26 startOverride)
        w.start("w:lvlOverride").attr("w:ilvl", "0").empty("w:startOverride", &[("w:val", &(*start).max(1).to_string())]).end();
        w.end();
    }
    w.finish()
}

fn open_root(w: &mut Writer, name: &str) {
    w.decl().start(name).attr("xmlns:w", NS_W).attr("xmlns:r", NS_R).attr("xmlns:wp", NS_WP).attr("xmlns:a", NS_A).attr("xmlns:pic", NS_PIC);
}

/// Write the model as a .docx package.
pub fn write(doc: &Document) -> Result<Vec<u8>, Error> {
    let mut wr = W { doc, pkg: PackageWriter::new(), part: "/word/document.xml".into(), rids: HashMap::new(), media: Vec::new(), nums: Vec::new(), pic_id: 0 };
    for (i, im) in doc.images.iter().enumerate() {
        let ext = ext_for(&im.content_type, &im.name);
        let name = format!("image{}.{ext}", i + 1);
        let ct = if im.content_type.is_empty() { "application/octet-stream" } else { im.content_type.as_str() };
        wr.pkg.default_type(&ext, ct);
        wr.pkg.add_part(&format!("/word/media/{name}"), ct, im.data.clone());
        wr.media.push(name);
    }
    let mut w = Writer::new();
    open_root(&mut w, "w:document");
    w.start("w:body");
    wr.blocks(&mut w, &doc.blocks, false);
    w.start("w:sectPr").empty("w:pgSz", &[("w:w", "12240"), ("w:h", "15840")]);
    w.empty("w:pgMar", &[("w:top", "1440"), ("w:right", "1440"), ("w:bottom", "1440"), ("w:left", "1440"), ("w:header", "720"), ("w:footer", "720"), ("w:gutter", "0")]).end();
    w.end();
    wr.pkg.add_part("/word/document.xml", CT_DOC, w.finish().into_bytes());
    wr.pkg.add_part("/word/styles.xml", CT_STYLES, styles_xml().into_bytes());
    wr.rid(rel::STYLES, "styles.xml", false);
    if !doc.notes.is_empty() {
        // footnotes: separator (0) and continuation separator (1), then notes from id 2 (NoteRef n → id n + 1)
        wr.rid(rel::FOOTNOTES, "footnotes.xml", false);
        wr.part = "/word/footnotes.xml".into();
        let mut f = Writer::new();
        open_root(&mut f, "w:footnotes");
        f.start("w:footnote").attr("w:type", "separator").attr("w:id", "0").start("w:p").start("w:r").empty("w:separator", &[]).end().end().end();
        f.start("w:footnote").attr("w:type", "continuationSeparator").attr("w:id", "1").start("w:p").start("w:r").empty("w:continuationSeparator", &[]).end().end().end();
        for (i, note) in doc.notes.iter().enumerate() {
            f.start("w:footnote").attr("w:id", &(i + 2).to_string());
            let (first, rest): (&[Inline], &[Block]) = match note.first() {
                Some(Block::Paragraph(inl)) => (inl, &note[1..]),
                _ => (&[], &note[..]),
            };
            f.start("w:p").start("w:pPr").empty("w:pStyle", &[("w:val", "FootnoteText")]).end();
            f.start("w:r").start("w:rPr").empty("w:rStyle", &[("w:val", "FootnoteReference")]).end().empty("w:footnoteRef", &[]).end();
            f.start("w:r").leaf("w:t", &[("xml:space", "preserve")], " ").end();
            wr.inlines(&mut f, first, false);
            f.end();
            wr.blocks(&mut f, rest, false);
            f.end();
        }
        wr.pkg.add_part("/word/footnotes.xml", CT_FOOTNOTES, f.finish().into_bytes());
        wr.part = "/word/document.xml".into();
    }
    if !wr.nums.is_empty() {
        wr.pkg.add_part("/word/numbering.xml", CT_NUMBERING, numbering_xml(&wr.nums).into_bytes());
        wr.rid(rel::NUMBERING, "numbering.xml", false);
    }
    let mut core = Writer::new();
    core.decl().start("cp:coreProperties").attr("xmlns:cp", "http://schemas.openxmlformats.org/package/2006/metadata/core-properties")
        .attr("xmlns:dc", "http://purl.org/dc/elements/1.1/").attr("xmlns:dcterms", "http://purl.org/dc/terms/");
    if !doc.title.is_empty() {
        core.leaf("dc:title", &[], &doc.title);
    }
    core.leaf("dc:creator", &[], "omni-spec");
    wr.pkg.add_part("/docProps/core.xml", CT_CORE, core.finish().into_bytes());
    wr.pkg.add_rel(None, rel::OFFICE_DOCUMENT, "word/document.xml", false);
    wr.pkg.add_rel(None, rel::CORE_PROPERTIES, "docProps/core.xml", false);
    wr.pkg.finish()
}
