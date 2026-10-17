//! XML 1.0 + Namespaces conformance cases (spec examples and well-formedness errors), writer round trip, no-panic fuzz.

use omni_spec::xml::{self, decode, escape_attr, escape_text, parse, unescape, Event, Reader, Writer};

fn events(s: &str) -> Vec<Event> {
    let mut r = Reader::new(s);
    let mut v = Vec::new();
    while let Some(e) = r.next().unwrap() {
        v.push(e);
    }
    v
}

#[test]
fn declaration_comment_pi_doctype() {
    let ev = events("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE greeting [<!ELEMENT greeting (#PCDATA)> <!-- ] -->]>\n<!-- c -->\n<?app do this?>\n<greeting>Hello, world!</greeting>\n");
    assert!(matches!(&ev[0], Event::Pi { target, .. } if target == "xml"));
    assert!(matches!(&ev[1], Event::Doctype(d) if d.starts_with("greeting")));
    assert_eq!(ev[2], Event::Comment(" c ".into()));
    assert_eq!(ev[3], Event::Pi { target: "app".into(), data: "do this".into() });
    assert!(matches!(&ev[4], Event::Start { name, empty: false, .. } if name.local == "greeting"));
    assert_eq!(ev[5], Event::Text("Hello, world!".into()));
    assert!(matches!(&ev[6], Event::End { .. }));
    assert_eq!(ev.len(), 7);
}

#[test]
fn entities_and_char_refs() {
    // §4.6 predefined entities, §4.1 character references (decimal + hex, astral)
    let e = parse("<a t='&lt;&amp;&gt;&quot;&apos;'>&#60;x&#x3E; &#x1F600; &amp;amp;</a>").unwrap();
    assert_eq!(e.attr("t"), Some("<&>\"'"));
    assert_eq!(e.text(), "<x> 😀 &amp;");
    assert!(unescape("&nbsp;", 0).is_err(), "undeclared entity");
    assert!(unescape("&#xD800;", 0).is_err(), "surrogate is not a char");
    assert!(unescape("&amp", 0).is_err(), "unterminated");
}

#[test]
fn cdata_and_line_ends() {
    let e = parse("<a><![CDATA[<b>&amp;</b>]]>x\r\ny\rz</a>").unwrap();
    assert_eq!(e.text(), "<b>&amp;</b>x\ny\nz");
}

#[test]
fn attribute_normalisation() {
    // §3.3.3: literal tab/newline become spaces; references to them survive
    let e = parse("<a v=\"one\ttwo\nthree&#10;four&#9;\"/>").unwrap();
    assert_eq!(e.attr("v"), Some("one two three\nfour\t"));
}

#[test]
fn namespaces() {
    let s = r#"<w:document xmlns:w="urn:w" xmlns:r="urn:r" xmlns="urn:default">
      <w:body><p r:id="rId1" plain="1"><w:t xml:space="preserve"> a </w:t></p>
      <x:inner xmlns:x="urn:x" xmlns:w="urn:w2"><w:t/></x:inner><w:t/></w:body></w:document>"#;
    let root = parse(s).unwrap();
    assert!(root.name.is("urn:w", "document"));
    let body = root.child("urn:w", "body").unwrap();
    let p = body.child("urn:default", "p").expect("default namespace applies to unprefixed elements");
    assert_eq!(p.attr_ns("urn:r", "id"), Some("rId1"));
    assert_eq!(p.attr_ns("", "plain"), Some("1"), "unprefixed attributes have no namespace");
    let t = p.child("urn:w", "t").unwrap();
    assert_eq!(t.attr_ns(xml::XML_NS, "space"), Some("preserve"));
    assert_eq!(t.text(), " a ");
    let inner = body.child("urn:x", "inner").unwrap();
    assert!(inner.child("urn:w2", "t").is_some(), "rebinding a prefix applies inside the element");
    assert!(body.children_named("urn:w", "t").count() == 1, "and ends with it");
    assert_eq!(root.descendants("urn:w", "t").len(), 2);
}

#[test]
fn empty_elements_emit_end() {
    let ev = events("<a><b x='1'/></a>");
    assert!(matches!(&ev[1], Event::Start { empty: true, .. }));
    assert!(matches!(&ev[2], Event::End { name } if name.local == "b"));
    assert!(matches!(&ev[3], Event::End { name } if name.local == "a"));
}

#[test]
fn read_text_and_skip() {
    let mut r = Reader::new("<r><a>one<i>two</i><![CDATA[three]]></a><b><c><d/></c></b><e>x</e></r>");
    r.next().unwrap();
    r.next().unwrap(); // <a>
    assert_eq!(r.read_text().unwrap(), "onetwothree");
    r.next().unwrap(); // <b>
    r.skip().unwrap();
    assert!(matches!(r.next().unwrap(), Some(Event::Start { name, .. }) if name.local == "e"));
}

#[test]
fn malformed_documents_are_errors() {
    for bad in [
        "<a><b></a></b>",       // element type match
        "<a>",                  // unclosed
        "</a>",                 // end without start
        "<a></a><b></b>",       // two roots
        "text<a/>",             // text outside root
        "<a x='1' x='2'/>",     // duplicate attribute
        "<a x=1/>",             // unquoted
        "<a x='<'/>",           // '<' in attribute
        "<a><!-- x</a>",        // unterminated comment
        "<a><![CDATA[x</a>",    // unterminated CDATA
        "<a>&bogus;</a>",       // undeclared entity
        "",                     // no root
        "<a/",                  // unterminated tag
    ] {
        let mut r = Reader::new(bad);
        let mut res = Ok(());
        loop {
            match r.next() {
                Ok(Some(_)) => continue,
                Ok(None) => break,
                Err(e) => {
                    res = Err(e);
                    break;
                }
            }
        }
        assert!(res.is_err(), "should fail: {bad:?}");
    }
}

#[test]
fn decode_boms() {
    assert_eq!(decode(b"\xEF\xBB\xBF<a/>").unwrap(), "<a/>");
    let le: Vec<u8> = [0xFF, 0xFE].into_iter().chain("<a>é</a>".encode_utf16().flat_map(|u| u.to_le_bytes())).collect();
    assert_eq!(decode(&le).unwrap(), "<a>é</a>");
    let be: Vec<u8> = [0xFE, 0xFF].into_iter().chain("<a/>".encode_utf16().flat_map(|u| u.to_be_bytes())).collect();
    assert_eq!(decode(&be).unwrap(), "<a/>");
    assert!(decode(b"\xC3\x28").is_err());
}

#[test]
fn writer_round_trip() {
    let mut w = Writer::new();
    w.decl().start("w:document").attr("xmlns:w", "urn:w").start("w:p").attr("w:val", "a\"b<c>&\nd");
    w.leaf("w:t", &[("xml:space", "preserve")], " x < y & z \u{1} ");
    w.empty("w:br", &[]);
    w.end();
    let s = w.finish();
    assert!(s.starts_with("<?xml version=\"1.0\""));
    assert!(s.contains("<w:br/>"));
    let root = parse(&s).unwrap();
    let p = root.child("urn:w", "p").unwrap();
    assert_eq!(p.attr_ns("urn:w", "val"), Some("a\"b<c>&\nd"));
    assert_eq!(p.child("urn:w", "t").unwrap().text(), " x < y & z  ", "control char U+0001 dropped");
    assert_eq!(escape_text("a\u{FFFE}b"), "ab");
    assert_eq!(escape_attr("\t"), "&#9;");
}

#[test]
fn random_inputs_never_panic() {
    let mut seed: u64 = 0xDEAD_BEEF_1234_5678;
    let mut rnd = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let alphabet = b"<>/=\"'&;#x!-[]?: abAB\n\t";
    let base = r#"<?xml version="1.0"?><a:r xmlns:a="u"><b x="1">t&amp;<![CDATA[c]]></b><!--k--><c/></a:r>"#;
    for _ in 0..20000 {
        let s: String = if rnd() % 2 == 0 {
            let mut v = base.as_bytes().to_vec();
            for _ in 0..1 + rnd() % 3 {
                let i = (rnd() as usize) % v.len();
                v[i] = alphabet[(rnd() as usize) % alphabet.len()];
            }
            String::from_utf8_lossy(&v).into_owned()
        } else {
            (0..rnd() % 40).map(|_| alphabet[(rnd() as usize) % alphabet.len()] as char).collect()
        };
        let mut r = Reader::new(&s);
        for _ in 0..200 {
            match r.next() {
                Ok(Some(_)) => {}
                _ => break,
            }
        }
        let _ = parse(&s);
    }
}
