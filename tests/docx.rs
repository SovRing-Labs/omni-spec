//! WordprocessingML (ECMA-376 Part 1 §17): read a LibreOffice-written document (tools/make-office-fixtures.sh), round
//! trip docx → model → docx → model, and have LibreOffice open our output (PDF export + re-save as docx, read back).

use omni_spec::ooxml::docx::{self, image_pixels, inline_text, Block, Document, Image, Inline, Style};

const D: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/ooxml");

fn t(s: &str) -> Inline {
    Inline::Text { text: s.into(), style: Style::default() }
}
fn st(s: &str, f: impl Fn(&mut Style)) -> Inline {
    let mut style = Style::default();
    f(&mut style);
    Inline::Text { text: s.into(), style }
}
fn p(v: Vec<Inline>) -> Block {
    Block::Paragraph(v)
}

#[test]
fn libreoffice_document() {
    let d = docx::read(&std::fs::read(format!("{D}/soffice-doc.docx")).unwrap()).unwrap();
    assert_eq!(d.title, "Fixture Document");
    let b = &d.blocks;
    assert_eq!(b[0], Block::Heading { level: 1, inlines: vec![t("Fixture Heading One")] });
    let Block::Paragraph(inl) = &b[1] else { panic!("{:?}", b[1]) };
    assert!(inl.contains(&st("bold", |s| s.bold = true)));
    assert!(inl.contains(&st("italic", |s| s.italic = true)));
    assert!(inl.contains(&st("underlined", |s| s.underline = true)));
    assert!(inl.contains(&st("struck", |s| s.strike = true)));
    assert!(inl.contains(&st("mono code", |s| s.code = true)));
    assert!(inl.contains(&Inline::Link { href: "https://example.org/page?a=1&b=2".into(), children: vec![t("link to example")] }), "hyperlink style is not underline");
    assert!(inl.contains(&Inline::NoteRef(1)));
    assert_eq!(b[2], Block::Heading { level: 2, inlines: vec![t("Lists")] });
    assert_eq!(b[3], Block::List { ordered: false, start: 1, items: vec![
        vec![p(vec![t("Bullet one")]), Block::List { ordered: false, start: 1, items: vec![vec![p(vec![t("Nested bullet")])]] }],
        vec![p(vec![t("Bullet two")])],
    ] });
    assert_eq!(b[4], Block::List { ordered: true, start: 1, items: vec![
        vec![p(vec![t("First step")])],
        vec![p(vec![t("Second step")]), Block::List { ordered: true, start: 1, items: vec![vec![p(vec![t("Sub step a")])], vec![p(vec![t("Sub step b")])]] }],
    ] });
    let Block::Table { rows, .. } = &b[6] else { panic!("{:?}", b[6]) };
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[1][2], vec![st("tested", |s| s.bold = true)]);
    assert_eq!(rows[2], vec![vec![t("Merged across two")], vec![], vec![t("end")]], "gridSpan → empty cell");
    assert_eq!(b[7], Block::Code { lang: String::new(), text: "let x = 1;\nprint(x)".into() });
    assert_eq!(b[8], Block::Quote(vec![p(vec![t("A quoted paragraph.")])]));
    assert_eq!(b[10], Block::Heading { level: 4, inlines: vec![t("Level four by outline")] }, "outline level without a heading style");
    assert_eq!(b[11], p(vec![t("Image: "), Inline::Image(0)]));
    assert_eq!(b[12], Block::PageBreak);
    assert_eq!(b[13], p(vec![t("After the page break.")]));
    assert_eq!(d.images.len(), 1);
    assert_eq!(d.images[0].data, std::fs::read(format!("{D}/red.png")).unwrap());
    assert_eq!(d.images[0].alt, "Red box\nA small red rectangle");
    assert_eq!((d.images[0].width_emu, d.images[0].height_emu), (720090, 360045));
    assert_eq!(d.notes, vec![vec![p(vec![t("The footnote text.")])]]);
}

fn sample() -> Document {
    let png = std::fs::read(format!("{D}/red.png")).unwrap();
    Document {
        title: "Round trip — ünïcode".into(),
        blocks: vec![
            Block::Heading { level: 1, inlines: vec![t("Title & <tags>")] },
            p(vec![t("Lead "), st("bold", |s| s.bold = true), t(" "), st("both", |s| { s.bold = true; s.italic = true }), t(" "),
                   st("u", |s| s.underline = true), t(" "), st("x", |s| s.strike = true), t(" "), st("code()", |s| s.code = true), t(" tab\there"),
                   Inline::Break, t("second line "),
                   Inline::Link { href: "https://example.org/?q=a&b=c".into(), children: vec![t("a "), st("bold link", |s| s.bold = true)] },
                   t(" and "), Inline::Link { href: "#section-two".into(), children: vec![t("an anchor")] }, Inline::NoteRef(1), t(".")]),
            Block::Heading { level: 2, inlines: vec![t("Lists")] },
            Block::List { ordered: false, start: 1, items: vec![
                vec![p(vec![t("one")]), Block::List { ordered: true, start: 1, items: vec![vec![p(vec![t("nested ordered")])]] }],
                vec![p(vec![t("two")]), Block::List { ordered: false, start: 1, items: vec![vec![p(vec![t("nested bullet")]), Block::List { ordered: false, start: 1, items: vec![vec![p(vec![t("third level")])]] }]] }],
            ] },
            Block::List { ordered: true, start: 3, items: vec![vec![p(vec![t("three")])], vec![p(vec![t("four")])]] },
            Block::List { ordered: true, start: 1, items: vec![vec![p(vec![t("restart at one")])]] },
            Block::Code { lang: "rust".into(), text: "fn main() {\n    println!(\"hi\");\n}".into() },
            Block::Code { lang: String::new(), text: "plain\n\nwith blank line".into() },
            Block::Quote(vec![p(vec![t("quoted one")]), p(vec![t("quoted two")])]),
            Block::Table { header: true, rows: vec![
                vec![vec![t("A")], vec![t("B")], vec![t("C")]],
                vec![vec![t("1")], vec![st("two", |s| s.italic = true)], vec![t("line"), Inline::Break, t("break")]],
                vec![vec![], vec![t("x")], vec![]],
            ] },
            Block::Rule,
            p(vec![t("Picture: "), Inline::Image(0)]),
            Block::PageBreak,
            Block::Heading { level: 6, inlines: vec![t("Deep")] },
            p(vec![t("End"), Inline::NoteRef(2)]),
        ],
        images: vec![Image { name: "red.png".into(), content_type: "image/png".into(), data: png, alt: "a red box".into(), width_emu: 0, height_emu: 0 }],
        notes: vec![vec![p(vec![t("First note with "), Inline::Link { href: "https://note.example/".into(), children: vec![t("a link")] }])], vec![p(vec![t("Second note.")])]],
    }
}

/// Our writer fills in an unknown picture size from the image header; compare models with that applied.
fn sized(mut d: Document) -> Document {
    for im in d.images.iter_mut() {
        if im.width_emu == 0 {
            let (w, h) = image_pixels(&im.data).unwrap();
            im.width_emu = w as u64 * 9525;
            im.height_emu = h as u64 * 9525;
        }
        im.name = String::new();
    }
    d
}
fn unnamed(mut d: Document) -> Document {
    for im in d.images.iter_mut() {
        im.name = String::new();
    }
    d
}

#[test]
fn round_trip_model() {
    let a = sample();
    let bytes = docx::write(&a).unwrap();
    let b = docx::read(&bytes).unwrap();
    assert_eq!(unnamed(b.clone()), sized(a.clone()), "write → read");
    let c = docx::read(&docx::write(&b).unwrap()).unwrap();
    assert_eq!(c, b, "model → docx → model is stable");
    // the LibreOffice fixture round-trips through our writer too
    let lo = docx::read(&std::fs::read(format!("{D}/soffice-doc.docx")).unwrap()).unwrap();
    let again = docx::read(&docx::write(&lo).unwrap()).unwrap();
    assert_eq!(unnamed(again), unnamed(lo));
}

#[test]
fn image_headers() {
    assert_eq!(image_pixels(&std::fs::read(format!("{D}/red.png")).unwrap()), Some((20, 10)));
    let gif = b"GIF89a\x10\x00\x08\x00\x80\x00\x00";
    assert_eq!(image_pixels(gif), Some((16, 8)));
    // minimal JPEG header: SOI, APP0 (length 16), SOF0 with height 0x0102 / width 0x0304
    let mut jpg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
    jpg.extend([0u8; 14]);
    jpg.extend([0xFF, 0xC0, 0x00, 0x11, 0x08, 0x01, 0x02, 0x03, 0x04, 0x03]);
    jpg.extend([0u8; 9]);
    assert_eq!(image_pixels(&jpg), Some((0x0304, 0x0102)));
    assert_eq!(image_pixels(b"nope"), None);
}

#[test]
fn errors_and_empty() {
    assert!(docx::read(b"not a zip").is_err());
    let xlsx = std::fs::read(format!("{D}/soffice-basic.xlsx")).unwrap();
    assert!(docx::read(&xlsx).is_err(), "a workbook has no w:body");
    let empty = docx::read(&docx::write(&Document::default()).unwrap()).unwrap();
    assert_eq!(empty, Document::default());
    assert_eq!(inline_text(&[t("a"), Inline::Break, Inline::NoteRef(2)], &[]), "a\n[2]");
}

#[test]
fn libreoffice_opens_our_output() {
    if std::process::Command::new("soffice").arg("--version").output().is_err() {
        eprintln!("soffice not installed: skipped");
        return;
    }
    let dir = std::env::temp_dir().join(format!("omni-spec-docx-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("resaved")).unwrap();
    let f = dir.join("ours.docx");
    std::fs::write(&f, docx::write(&sample()).unwrap()).unwrap();
    let lo = |args: &[&str]| {
        let o = std::process::Command::new("soffice")
            .arg(format!("-env:UserInstallation=file://{}/profile", dir.display()))
            .arg("--headless")
            .args(args)
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    };
    lo(&["--convert-to", "pdf", "--outdir", dir.to_str().unwrap(), f.to_str().unwrap()]);
    let pdf = std::fs::read(dir.join("ours.pdf")).expect("pdf written");
    assert!(pdf.starts_with(b"%PDF") && pdf.len() > 5000, "pdf {} bytes", pdf.len());
    // LibreOffice re-saves our docx; what it writes back still carries our structure
    lo(&["--convert-to", "docx:MS Word 2007 XML", "--outdir", dir.join("resaved").to_str().unwrap(), f.to_str().unwrap()]);
    let back = docx::read(&std::fs::read(dir.join("resaved/ours.docx")).unwrap()).unwrap();
    let text: Vec<String> = back.blocks.iter().map(|b| format!("{b:?}")).collect();
    let all = text.join("\n");
    assert!(matches!(&back.blocks[0], Block::Heading { level: 1, .. }), "{}", text[0]);
    assert!(all.contains("Title & <tags>") && all.contains("nested ordered") && all.contains("third level"));
    assert!(back.blocks.iter().any(|b| matches!(b, Block::Table { rows, .. } if rows.len() == 3)));
    assert!(back.blocks.iter().any(|b| matches!(b, Block::Code { text, .. } if text.contains("println!"))));
    assert!(back.blocks.iter().any(|b| matches!(b, Block::List { ordered: true, start: 3, .. })), "start override survives LibreOffice");
    assert_eq!(back.images.len(), 1);
    assert_eq!(back.notes.len(), 2);
    assert_eq!(back.title, "Round trip — ünïcode");
    let _ = std::fs::remove_dir_all(&dir);
}
