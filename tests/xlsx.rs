//! SpreadsheetML (ECMA-376 Part 1 §18): read LibreOffice-written workbooks (tools/make-office-fixtures.sh), write our
//! own and read them back, and have LibreOffice open our output (black-box `soffice --convert-to csv`) when installed.

use omni_spec::ooxml::xlsx::{
    self, cell_ref, col_index, col_letters, decode_xstring, encode_xstring, format_kind, iso_to_serial, parse_ref, serial_to_iso, Cell, DateKind,
    DateSystem, WriteSheet,
};

const D: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/ooxml");

fn t(s: &str) -> Cell {
    Cell::Text(s.into())
}
fn n(v: f64) -> Cell {
    Cell::Number(v)
}
fn d(s: &str) -> Cell {
    Cell::Date(s.into())
}

#[test]
fn references() {
    assert_eq!(col_index("A"), Some(0));
    assert_eq!(col_index("Z"), Some(25));
    assert_eq!(col_index("AA"), Some(26));
    assert_eq!(col_index("XFD"), Some(16383));
    for i in [0, 25, 26, 51, 52, 701, 702, 16383] {
        assert_eq!(col_index(&col_letters(i)), Some(i));
    }
    assert_eq!(parse_ref("B12"), Some((11, 1)));
    assert_eq!(parse_ref("$C$3"), Some((2, 2)));
    assert_eq!(parse_ref("A0"), None);
    assert_eq!(cell_ref(0, 27), "AB1");
}

#[test]
fn xstring_escapes() {
    assert_eq!(decode_xstring("a_x000D_b"), "a\rb");
    assert_eq!(decode_xstring("_x005F_x0041_"), "_x0041_");
    assert_eq!(decode_xstring("_xD83D__xDE00_"), "😀");
    assert_eq!(decode_xstring("plain_xZZ"), "plain_xZZ");
    for s in ["tab\tx", "ctl\u{1}x", "lit _x0041_ here", "😀", "_x"] {
        assert_eq!(decode_xstring(&encode_xstring(s)), s);
    }
}

#[test]
fn date_systems() {
    // §18.17.4.1: 1900 backward-compatibility system
    assert_eq!(serial_to_iso(1.0, DateSystem::Compat1900, DateKind::Date), "1900-01-01");
    assert_eq!(serial_to_iso(59.0, DateSystem::Compat1900, DateKind::Date), "1900-02-28");
    assert_eq!(serial_to_iso(60.0, DateSystem::Compat1900, DateKind::Date), "1900-02-29");
    assert_eq!(serial_to_iso(61.0, DateSystem::Compat1900, DateKind::Date), "1900-03-01");
    assert_eq!(serial_to_iso(46301.0, DateSystem::Compat1900, DateKind::Date), "2026-10-06");
    assert_eq!(serial_to_iso(46301.6041666667, DateSystem::Compat1900, DateKind::DateTime), "2026-10-06T14:30:00");
    assert_eq!(serial_to_iso(0.545243055555556, DateSystem::Compat1900, DateKind::Time), "13:05:09");
    assert_eq!(serial_to_iso(0.99999999, DateSystem::Compat1900, DateKind::DateTime), "1900-01-01T00:00:00", "rounding carries");
    assert_eq!(serial_to_iso(1.0, DateSystem::Base1899, DateKind::Date), "1899-12-31");
    assert_eq!(serial_to_iso(0.0, DateSystem::Base1904, DateKind::Date), "1904-01-01");
    assert_eq!(serial_to_iso(46301.0 - 1462.0, DateSystem::Base1904, DateKind::Date), "2026-10-06", "1904 system is 1,462 days behind");
    // round trip through the writer's direction, every day 1900-03-01 .. 2100
    let mut serial = 61.0;
    while serial < 73415.0 {
        let iso = serial_to_iso(serial, DateSystem::Compat1900, DateKind::Date);
        assert_eq!(iso_to_serial(&iso), Some((serial, DateKind::Date)), "{iso}");
        serial += 1.0;
    }
    assert_eq!(iso_to_serial("1900-01-01"), Some((1.0, DateKind::Date)));
    assert_eq!(iso_to_serial("2026-10-06T14:30"), Some((46301.0 + 14.5 / 24.0, DateKind::DateTime)));
    assert_eq!(iso_to_serial("06:00:00"), Some((0.25, DateKind::Time)));
    assert_eq!(iso_to_serial("2026-02-30"), None);
    assert_eq!(iso_to_serial("1899-12-31"), None);
    assert_eq!(iso_to_serial("not a date"), None);
}

#[test]
fn number_format_classes() {
    assert_eq!(format_kind("dd\\.mm\\.yyyy"), Some(DateKind::Date));
    assert_eq!(format_kind("yyyy\\-mm\\-dd\\ hh:mm"), Some(DateKind::DateTime));
    assert_eq!(format_kind("hh:mm:ss"), Some(DateKind::Time));
    assert_eq!(format_kind("h:mm AM/PM"), Some(DateKind::Time));
    assert_eq!(format_kind("mmm"), Some(DateKind::Date));
    assert_eq!(format_kind("[$-409]d-mmm-yy;@"), Some(DateKind::Date));
    assert_eq!(format_kind("[h]:mm:ss"), None, "elapsed time is a duration");
    assert_eq!(format_kind("0.0%"), None);
    assert_eq!(format_kind("#,##0.00\\ [$€-407];[Red]\\-#,##0.00\\ [$€-407]"), None);
    assert_eq!(format_kind("0.00E+00"), None);
    assert_eq!(format_kind("#,##0 \"days\""), None, "quoted text is not a token");
    assert_eq!(format_kind("General"), None);
}

#[test]
fn libreoffice_rich_workbook() {
    let b = std::fs::read(format!("{D}/soffice-rich.xlsx")).unwrap();
    assert_eq!(xlsx::sheet_names(&b).unwrap(), vec![("Items".to_string(), "visible".to_string()), ("Notes & Ünïcode".to_string(), "visible".to_string())]);
    let wb = xlsx::read(&b).unwrap();
    let s = &wb.sheets[0];
    assert_eq!(s.rows[0][0], t("SKU"));
    assert_eq!(s.rows[0].len(), 11);
    let r = &s.rows[1];
    assert_eq!(r[1], t("Dell Latitude 7490 & dock"));
    assert_eq!(r[2], n(2.0));
    assert_eq!(r[4], n(171.0), "cached formula value");
    assert_eq!(r[5], d("2026-10-06"));
    assert_eq!(r[6], d("2026-10-06T14:30:00"));
    assert_eq!(r[7], n(0.255), "percent stays a number");
    assert_eq!(r[8], Cell::Error("#DIV/0!".into()));
    assert_eq!(r[10], d("13:05:09"));
    let r = &s.rows[2];
    assert_eq!(r[1], t("Ünïcödé — 日本語 ✓"));
    assert_eq!(r[5], d("1900-03-01"));
    assert_eq!(r[6], Cell::Empty);
    assert_eq!(r[7], t("  spaced  "));
    assert_eq!(r[8], t("A-002-x"), "t=str cached string");
    assert!(s.rows[3].is_empty() && s.rows[4].is_empty());
    assert_eq!(s.rows[5][..4], [t("Merged note"), Cell::Empty, Cell::Empty, n(12345678901.0)]);
    assert_eq!(s.rows.len(), 6);
    let s2 = wb.sheet("Notes & Ünïcode").unwrap();
    assert_eq!(s2.rows, vec![vec![t("line one\nline two"), n(-3.25)]]);
    // one sheet by name, and the first visible sheet
    assert_eq!(xlsx::read_sheet(&b, Some("Notes & Ünïcode")).unwrap().rows.len(), 1);
    assert_eq!(xlsx::read_sheet(&b, None).unwrap().name, "Items");
    assert!(xlsx::read_sheet(&b, Some("nope")).is_err());
}

#[test]
fn libreoffice_csv_workbook() {
    let b = std::fs::read(format!("{D}/soffice-basic.xlsx")).unwrap();
    let s = xlsx::read_sheet(&b, None).unwrap();
    assert_eq!(s.rows[0], vec![t("Name"), t("Qty"), t("Price"), t("Date"), t("Flag")]);
    assert_eq!(s.rows[1][..4], [t("Widget"), n(3.0), n(4.5), d("2026-10-06")]);
    assert_eq!(s.rows[3][..4], [t("Comma, quoted"), n(-7.0), n(1000.0), d("1999-12-31")]);
}

fn sample() -> Vec<WriteSheet> {
    vec![
        WriteSheet {
            name: "Inventory".into(),
            header: true,
            widths: vec![10.0],
            rows: vec![
                vec![t("SKU"), t("Title"), t("Price"), t("Listed"), t("Sold at"), t("Active"), t("Note")],
                vec![t("A-1"), t("Dell 7490"), n(129.99), d("2026-10-06"), d("2026-10-06T14:30:00"), Cell::Bool(true), t("  lead space")],
                vec![t("A-2"), t("Ünïcödé 日本語 & <tags>"), n(-0.5), d("1900-01-01"), Cell::Empty, Cell::Bool(false), t("line1\nline2\ttab _x0041_ \u{1}")],
                vec![t("A-3"), t("big"), n(12345678901.0), d("12:00:00"), Cell::Error("#N/A".into()), Cell::Empty, d("not-a-date")],
            ],
        },
        WriteSheet { name: "Bad:Name/[1]".into(), header: false, widths: vec![], rows: vec![vec![n(1.0), n(2.0)], vec![], vec![t("x")]] },
        WriteSheet { name: "Bad:Name/[1]".into(), header: false, widths: vec![], rows: vec![] },
    ]
}

#[test]
fn write_then_read() {
    let bytes = xlsx::write(&sample()).unwrap();
    let wb = xlsx::read(&bytes).unwrap();
    assert_eq!(wb.sheets.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Inventory", "Bad_Name__1_", "Bad_Name__1_ (2)"]);
    let s = &wb.sheets[0];
    assert_eq!(s.rows[0][0], t("SKU"));
    assert_eq!(s.rows[1], vec![t("A-1"), t("Dell 7490"), n(129.99), d("2026-10-06"), d("2026-10-06T14:30:00"), Cell::Bool(true), t("  lead space")]);
    assert_eq!(s.rows[2][1], t("Ünïcödé 日本語 & <tags>"));
    assert_eq!(s.rows[2][3], d("1900-01-01"));
    assert_eq!(s.rows[2][6], t("line1\nline2\ttab _x0041_ \u{1}"));
    assert_eq!(s.rows[3][3], d("12:00:00"));
    assert_eq!(s.rows[3][4], Cell::Error("#N/A".into()));
    assert_eq!(s.rows[3][6], t("not-a-date"));
    assert_eq!(wb.sheets[1].rows, vec![vec![n(1.0), n(2.0)], vec![], vec![t("x")]]);
    assert!(wb.sheets[2].rows.is_empty());
    // structure: frozen header, autofilter, column widths, bold header style
    let p = omni_spec::ooxml::opc::Package::open(&bytes).unwrap();
    let sheet1 = String::from_utf8(p.part("/xl/worksheets/sheet1.xml").unwrap()).unwrap();
    assert!(sheet1.contains("state=\"frozen\"") && sheet1.contains("<autoFilter ref=\"A1:G4\"/>"));
    assert!(sheet1.contains("<col min=\"1\" max=\"1\" width=\"10.00\""));
    assert!(sheet1.contains("<c r=\"A1\" t=\"s\" s=\"1\">"));
    assert!(xlsx::write(&[]).is_err());
}

#[test]
fn libreoffice_opens_our_output() {
    if std::process::Command::new("soffice").arg("--version").output().is_err() {
        eprintln!("soffice not installed: skipped");
        return;
    }
    let dir = std::env::temp_dir().join(format!("omni-spec-xlsx-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("ours.xlsx");
    std::fs::write(&f, xlsx::write(&sample()).unwrap()).unwrap();
    let out = std::process::Command::new("soffice")
        .arg(format!("-env:UserInstallation=file://{}/profile", dir.display()))
        .args(["--headless", "--convert-to", "csv:Text - txt - csv (StarCalc):44,34,76,1,,0,false,true,false,false,false,-1", "--outdir"])
        .arg(&dir)
        .arg(&f)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    // sheet -1 = every sheet, one CSV per sheet: ours-Inventory.csv, …
    let csv = std::fs::read_to_string(dir.join("ours-Inventory.csv")).expect("csv for sheet 1");
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines[0], "SKU,Title,Price,Listed,Sold at,Active,Note");
    assert!(lines[1].starts_with("A-1,Dell 7490,129.99,2026-10-06,2026-10-06 14:30:00,TRUE,"), "{}", lines[1]);
    assert!(csv.contains("Ünïcödé 日本語 & <tags>"));
    assert!(csv.contains("12345678901") && csv.contains("12:00:00") && csv.contains("#N/A"));
    let csv2 = std::fs::read_to_string(dir.join("ours-Bad_Name__1_.csv")).unwrap();
    assert!(csv2.starts_with("1,2"));
    let _ = std::fs::remove_dir_all(&dir);
}
