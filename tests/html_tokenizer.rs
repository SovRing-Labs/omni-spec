//! HTML tokenizer conformance: html5lib-tests tokenizer suite (tests/data/html5lib-tokenizer/, MIT; LICENSE there).
//! Each test runs the tokenizer from every listed initial state and compares the coalesced token list with the
//! expected output (JSON, parsed with omni_spec::json). CDATA-section initial states and xmlViolation are skipped.

use omni_spec::html::{State, Token, Tokenizer};
use omni_spec::json::{self, Value};

fn state_of(name: &str) -> Option<State> {
    Some(match name {
        "Data state" => State::Data,
        "PLAINTEXT state" => State::PlainText,
        "RCDATA state" => State::RcData,
        "RAWTEXT state" => State::RawText,
        "Script data state" => State::ScriptData,
        _ => return None,
    })
}

/// Decode literal \uXXXX escapes (html5lib "doubleEscaped" tests).
fn unescape_u(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("\\u") {
        out.push_str(&rest[..i]);
        let hex = &rest[i + 2..(i + 6).min(rest.len())];
        match u32::from_str_radix(hex, 16).ok().and_then(char::from_u32) {
            Some(c) => {
                out.push(c);
                rest = &rest[i + 6..];
            }
            None => {
                out.push_str("\\u");
                rest = &rest[i + 2..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn unescape_value(v: &Value) -> Value {
    match v {
        Value::String(s) => Value::String(unescape_u(s)),
        Value::Array(a) => Value::Array(a.iter().map(unescape_value).collect()),
        Value::Object(m) => Value::Object(m.iter().map(|(k, x)| (unescape_u(k), unescape_value(x))).collect()),
        other => other.clone(),
    }
}

/// Our tokens in the html5lib JSON shape, with consecutive characters coalesced.
fn to_json(tokens: &[Token]) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut text: Option<String> = None;
    let flush = |text: &mut Option<String>, out: &mut Vec<Value>| {
        if let Some(t) = text.take() {
            out.push(Value::Array(vec![Value::String("Character".into()), Value::String(t)]));
        }
    };
    let attrs_obj = |attrs: &[(String, String)]| Value::Object(attrs.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect());
    for t in tokens {
        match t {
            Token::Character(c) => text.get_or_insert_with(String::new).push(*c),
            other => {
                flush(&mut text, &mut out);
                out.push(match other {
                    Token::StartTag { name, attrs, self_closing } => {
                        let mut v = vec![Value::String("StartTag".into()), Value::String(name.clone()), attrs_obj(attrs)];
                        if *self_closing {
                            v.push(Value::Bool(true));
                        }
                        Value::Array(v)
                    }
                    Token::EndTag { name, .. } => Value::Array(vec![Value::String("EndTag".into()), Value::String(name.clone())]),
                    Token::Comment(s) => Value::Array(vec![Value::String("Comment".into()), Value::String(s.clone())]),
                    Token::Doctype { name, public_id, system_id, force_quirks } => {
                        let opt = |o: &Option<String>| o.clone().map_or(Value::Null, Value::String);
                        Value::Array(vec![
                            Value::String("DOCTYPE".into()),
                            opt(name),
                            opt(public_id),
                            opt(system_id),
                            Value::Bool(!force_quirks),
                        ])
                    }
                    Token::Character(_) => unreachable!(),
                });
            }
        }
    }
    flush(&mut text, &mut out);
    out
}

/// Object members are unordered in the expected data: sort them for comparison.
fn normalise(v: &Value) -> Value {
    match v {
        Value::Array(a) => Value::Array(a.iter().map(normalise).collect()),
        Value::Object(m) => {
            let mut m: Vec<(String, Value)> = m.iter().map(|(k, x)| (k.clone(), normalise(x))).collect();
            m.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(m)
        }
        other => other.clone(),
    }
}

#[test]
fn html5lib_tokenizer_suite() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/html5lib-tokenizer");
    let mut files: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".test") && !n.starts_with("xmlViolation"))
        .collect();
    files.sort();
    let (mut total, mut passed, mut skipped, mut obsolete) = (0usize, 0usize, 0usize, 0usize);
    let mut failures: Vec<String> = Vec::new();
    for file in &files {
        let doc = json::parse(&std::fs::read(format!("{dir}/{file}")).unwrap()).expect("test file is JSON");
        let Value::Object(top) = doc else { panic!("{file}: not an object") };
        let tests = match &top[0].1 {
            Value::Array(t) => t,
            _ => panic!("{file}: no tests"),
        };
        for t in tests {
            let Value::Object(fields) = t else { continue };
            let get = |k: &str| fields.iter().find(|(n, _)| n == k).map(|(_, v)| v);
            let double = matches!(get("doubleEscaped"), Some(Value::Bool(true)));
            let (input, output) = {
                let i = get("input").cloned().unwrap_or(Value::String(String::new()));
                let o = get("output").cloned().unwrap_or(Value::Array(vec![]));
                if double {
                    (unescape_value(&i), unescape_value(&o))
                } else {
                    (i, o)
                }
            };
            let Value::String(input) = input else { continue };
            // Processing instructions (ProcessingInstruction tokens, eof-in-processing-instruction errors) belong to an
            // obsolete tokenizer draft. The current WHATWG tokenizer turns "<?" into a bogus comment, so these
            // expectations are excluded and counted separately.
            let obsolete_pi = matches!(&output, Value::Array(a) if a.iter().any(|o| matches!(o, Value::Array(x) if matches!(x.first(), Some(Value::String(k)) if k == "ProcessingInstruction"))))
                || matches!(get("errors"), Some(Value::Array(es)) if es.iter().any(|e| matches!(e, Value::Object(m) if m.iter().any(|(k, v)| k == "code" && matches!(v, Value::String(c) if c.contains("processing-instruction"))))));
            if obsolete_pi {
                obsolete += 1;
                continue;
            }
            let last_start = match get("lastStartTag") {
                Some(Value::String(s)) => s.clone(),
                _ => String::new(),
            };
            let states: Vec<String> = match get("initialStates") {
                Some(Value::Array(a)) => a.iter().filter_map(|x| if let Value::String(s) = x { Some(s.clone()) } else { None }).collect(),
                _ => vec!["Data state".to_string()],
            };
            let want = normalise(&Value::Array(match &output {
                Value::Array(a) => a.clone(),
                _ => vec![],
            }));
            for st in states {
                let Some(state) = state_of(&st) else {
                    skipped += 1;
                    continue;
                };
                total += 1;
                let tokens = Tokenizer::all_tokens(&input, state, &last_start);
                let got = normalise(&Value::Array(to_json(&tokens)));
                if got == want {
                    passed += 1;
                } else {
                    let desc = match get("description") {
                        Some(Value::String(d)) => d.clone(),
                        _ => String::new(),
                    };
                    failures.push(format!("{file} [{st}] {desc:?}: input {input:?}"));
                }
            }
        }
    }
    eprintln!("html5lib tokenizer: {passed}/{total} pass ({skipped} initial states skipped: CDATA; {obsolete} obsolete processing-instruction tests excluded)");
    for f in failures.iter().take(25) {
        eprintln!("{f}");
    }
    assert!(failures.is_empty(), "{} of {total} failed", failures.len());
}
