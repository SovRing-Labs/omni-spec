//! OPC (ECMA-376 Part 2) on packages written by LibreOffice (tools/make-office-fixtures.sh) and by our writer.

use omni_spec::ooxml::opc::{normalize, rel, rels_part_for, resolve, Package, PackageWriter};

const D: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/ooxml");

#[test]
fn part_name_rules() {
    assert_eq!(normalize("word/document.xml"), "/word/document.xml");
    assert_eq!(normalize("/a%20b.xml"), "/a b.xml");
    assert_eq!(rels_part_for(None), "/_rels/.rels");
    assert_eq!(rels_part_for(Some("/word/document.xml")), "/word/_rels/document.xml.rels");
    assert_eq!(rels_part_for(Some("xl/workbook.xml")), "/xl/_rels/workbook.xml.rels");
    assert_eq!(resolve(None, "xl/workbook.xml"), "/xl/workbook.xml");
    assert_eq!(resolve(Some("/xl/workbook.xml"), "worksheets/sheet1.xml"), "/xl/worksheets/sheet1.xml");
    assert_eq!(resolve(Some("/xl/worksheets/sheet1.xml"), "../drawings/d1.xml"), "/xl/drawings/d1.xml");
    assert_eq!(resolve(Some("/word/document.xml"), "/word/media/image1.png"), "/word/media/image1.png");
    assert_eq!(resolve(Some("/word/document.xml"), "./media/a.png#frag"), "/word/media/a.png");
}

#[test]
fn libreoffice_xlsx_package() {
    let b = std::fs::read(format!("{D}/soffice-rich.xlsx")).unwrap();
    let p = Package::open(&b).unwrap();
    let wb = p.main_part(rel::OFFICE_DOCUMENT).unwrap();
    assert_eq!(wb, "/xl/workbook.xml");
    assert_eq!(p.content_type(&wb), Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"));
    let sheets = p.related(&wb, rel::WORKSHEET).unwrap();
    assert_eq!(sheets.len(), 2);
    assert!(sheets.iter().all(|(_, n)| p.has_part(n)));
    assert_eq!(p.related(&wb, rel::SHARED_STRINGS).unwrap().len(), 1);
    assert_eq!(p.related(&wb, rel::STYLES).unwrap()[0].1, "/xl/styles.xml");
    assert!(p.rels(Some("/xl/styles.xml")).unwrap().is_empty(), "missing rels part = no relationships");
    assert!(p.part("/XL/Workbook.xml").is_ok(), "part names compare case-insensitively");
}

#[test]
fn libreoffice_docx_package() {
    let b = std::fs::read(format!("{D}/soffice-doc.docx")).unwrap();
    let p = Package::open(&b).unwrap();
    let doc = p.main_part(rel::OFFICE_DOCUMENT).unwrap();
    assert_eq!(doc, "/word/document.xml");
    let rels = p.rels(Some(&doc)).unwrap();
    let link = rels.iter().find(|r| r.is(rel::HYPERLINK)).unwrap();
    assert!(link.external);
    assert_eq!(link.target, "https://example.org/page?a=1&b=2");
    let img = p.related(&doc, rel::IMAGE).unwrap();
    assert_eq!(img.len(), 1);
    let png = p.part(&img[0].1).unwrap();
    assert_eq!(png, std::fs::read(format!("{D}/red.png")).unwrap());
    assert_eq!(p.content_type(&img[0].1), Some("image/png"));
    assert_eq!(p.related(&doc, rel::NUMBERING).unwrap().len(), 1);
    assert_eq!(p.related(&doc, rel::FOOTNOTES).unwrap().len(), 1);
}

#[test]
fn strict_relationship_types_match() {
    let r = omni_spec::ooxml::opc::Relationship {
        id: "rId1".into(),
        rel_type: "http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument".into(),
        target: "x".into(),
        external: false,
    };
    assert!(r.is(rel::OFFICE_DOCUMENT));
    assert!(!r.is(rel::STYLES));
}

#[test]
fn writer_round_trip() {
    let mut w = PackageWriter::new();
    w.default_type("png", "image/png");
    w.add_part("/word/document.xml", "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml", b"<w:document xmlns:w=\"urn:w\"/>".to_vec());
    w.add_part("/word/media/i.png", "image/png", vec![1, 2, 3]);
    assert_eq!(w.add_rel(None, rel::OFFICE_DOCUMENT, "word/document.xml", false), "rId1");
    assert_eq!(w.add_rel(Some("/word/document.xml"), rel::IMAGE, "media/i.png", false), "rId1");
    assert_eq!(w.add_rel(Some("/word/document.xml"), rel::HYPERLINK, "https://x.test/?a=1&b=2", true), "rId2");
    let z = w.finish().unwrap();
    let p = Package::open(&z).unwrap();
    let doc = p.main_part(rel::OFFICE_DOCUMENT).unwrap();
    assert_eq!(doc, "/word/document.xml");
    assert_eq!(p.content_type(&doc), Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"));
    assert_eq!(p.content_type("/word/media/i.png"), Some("image/png"));
    let rs = p.rels(Some(&doc)).unwrap();
    assert_eq!(rs.len(), 2);
    assert!(rs[1].external && rs[1].target == "https://x.test/?a=1&b=2");
    assert_eq!(p.part(&p.related(&doc, rel::IMAGE).unwrap()[0].1).unwrap(), vec![1, 2, 3]);
    let names = p.part_names();
    assert_eq!(names[0], "/[Content_Types].xml");
    assert_eq!(names[1], "/_rels/.rels");
}

#[test]
fn not_a_package() {
    let mut z = omni_spec::zip::Writer::new();
    z.add_stored("a.txt", b"x").unwrap();
    let b = z.finish().unwrap();
    assert!(Package::open(&b).is_err());
    assert!(Package::open(b"junk").is_err());
}
