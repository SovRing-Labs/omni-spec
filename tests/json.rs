//! JSON conformance: JSONTestSuite test_parsing (Nicolas Seriot, MIT; tests/data/json-suite, see LICENSE there).
//! y_* must parse; n_* must be rejected; i_* are implementation-defined and only reported.
//! Also: serialize/parse round trips of every accepted document, and RFC 8259 spot checks.

use omni_spec::json::{parse, to_string, ErrorKind, Value};

#[test]
fn json_test_suite_test_parsing() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/json-suite");
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".json"))
        .collect();
    names.sort();
    let (mut y_ok, mut n_ok, mut i_accepted, mut i_rejected) = (0, 0, 0, 0);
    let mut failures = Vec::new();
    for name in &names {
        let bytes = std::fs::read(format!("{dir}/{name}")).unwrap();
        let res = parse(&bytes);
        if name.starts_with("y_") {
            match &res {
                Ok(v) => {
                    y_ok += 1;
                    let again = parse(to_string(v).as_bytes());
                    if again.as_ref() != Ok(v) {
                        failures.push(format!("{name}: round trip differs"));
                    }
                }
                Err(e) => failures.push(format!("{name}: rejected {:?} at {}", e.kind, e.offset)),
            }
        } else if name.starts_with("n_") {
            match res {
                Err(_) => n_ok += 1,
                Ok(_) => failures.push(format!("{name}: accepted")),
            }
        } else if res.is_ok() {
            i_accepted += 1;
        } else {
            i_rejected += 1;
        }
    }
    eprintln!(
        "JSONTestSuite test_parsing: y_ accepted {y_ok}, n_ rejected {n_ok}; i_ accepted {i_accepted} / rejected {i_rejected} (implementation-defined)"
    );
    for f in failures.iter().take(30) {
        eprintln!("{f}");
    }
    assert!(failures.is_empty(), "{} conformance failures", failures.len());
}

#[test]
fn rfc_spot_checks() {
    assert_eq!(parse(b" [1, \"a\", true, null] ").unwrap(), Value::Array(vec![Value::Number(1.0), Value::String("a".into()), Value::Bool(true), Value::Null]));
    assert_eq!(parse(br#""\u00e9\ud83d\ude00""#).unwrap(), Value::String("\u{e9}\u{1F600}".into()));
    assert_eq!(parse(b"-0.5e+2").unwrap(), Value::Number(-50.0));
    assert_eq!(parse(br#"{"a":1,"a":2}"#).unwrap(), Value::Object(vec![("a".into(), Value::Number(1.0)), ("a".into(), Value::Number(2.0))]));
    assert_eq!(to_string(&Value::String("a\"b\\c\n\u{1}".into())), r#""a\"b\\c\n\u0001""#);
    assert_eq!(to_string(&Value::Number(0.1)), "0.1");
    assert_eq!(parse(b"01").unwrap_err().kind, ErrorKind::TrailingData);
    assert_eq!(parse(b"[1,]").unwrap_err().kind, ErrorKind::UnexpectedByte(b']'));
    assert_eq!(parse(&[b'"', 0xFF, b'"']).unwrap_err().kind, ErrorKind::InvalidUtf8);
}

#[test]
fn depth_limit_is_an_error() {
    let deep = format!("{}{}", "[".repeat(600), "]".repeat(600));
    assert_eq!(parse(deep.as_bytes()).unwrap_err().kind, ErrorKind::TooDeep);
    let ok = format!("{}{}", "[".repeat(100), "]".repeat(100));
    assert!(parse(ok.as_bytes()).is_ok());
}
