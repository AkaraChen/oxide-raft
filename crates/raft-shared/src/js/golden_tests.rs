//! raft_shared::js golden tests against Node 24.15.0 records (decisions.md D3).

use serde_json::Value as J;

use super::*;

const GOLDEN: &str = include_str!("../../../../tests/golden/raft-shared/js-core.json");

fn records(category: &str) -> Vec<J> {
    let root: J = serde_json::from_str(GOLDEN).expect("golden file is valid JSON");
    root.get(category)
        .and_then(J::as_array)
        .unwrap_or_else(|| panic!("golden category {category} is missing"))
        .clone()
}

fn units(j: &J) -> JsString {
    let list = j
        .as_array()
        .unwrap_or_else(|| panic!("expected code-unit array, got {j}"));
    JsString::from_units(
        list.iter()
            .map(|u| u16::try_from(u.as_u64().expect("code unit")).expect("code unit fits u16"))
            .collect(),
    )
}

fn num(j: &J) -> f64 {
    match j {
        J::String(s) => match s.as_str() {
            "NaN" => f64::NAN,
            "Infinity" => f64::INFINITY,
            "-Infinity" => f64::NEG_INFINITY,
            "-0" => -0.0,
            other => panic!("unexpected number string {other}"),
        },
        _ => j
            .as_f64()
            .unwrap_or_else(|| panic!("expected number, got {j}")),
    }
}

fn opt_num(j: &J) -> Option<f64> {
    if j.is_null() { None } else { Some(num(j)) }
}

fn same_num(a: f64, b: f64) -> bool {
    (a.is_nan() && b.is_nan()) || a.to_bits() == b.to_bits()
}

fn val(j: &J) -> Value {
    let tag = j.get("t").and_then(J::as_str).expect("tagged value");
    let v = j.get("v");
    match tag {
        "undefined" => Value::Undefined,
        "null" => Value::Null,
        "bool" => Value::Bool(v.and_then(J::as_bool).expect("bool")),
        "number" => Value::Number(num(v.expect("number"))),
        "string" => Value::String(units(v.expect("string"))),
        "array" => Value::Array(
            v.and_then(J::as_array)
                .expect("array")
                .iter()
                .map(val)
                .collect(),
        ),
        "object" => Value::Object(entries(v.expect("object"))),
        other => panic!("unknown tag {other}"),
    }
}

fn entries(j: &J) -> Object {
    let mut object = Object::new();
    for pair in j.as_array().expect("entries") {
        let pair = pair.as_array().expect("entry");
        object.insert(units(&pair[0]), val(&pair[1]));
    }
    object
}

/// Compares `got` with a golden tagged value directly, without building the
/// expected side through `Object`: object keys must come out in exactly the
/// order V8 recorded, so the key-order rule itself is checked against V8.
fn matches_golden(got: &Value, expected: &J) -> bool {
    let tag = expected.get("t").and_then(J::as_str).expect("tagged value");
    let v = expected.get("v");
    match (tag, got) {
        ("array", Value::Array(items)) => {
            let list = v.and_then(J::as_array).expect("array");
            items.len() == list.len() && items.iter().zip(list).all(|(g, e)| matches_golden(g, e))
        }
        ("object", Value::Object(object)) => {
            let pairs = v.and_then(J::as_array).expect("entries");
            object.len() == pairs.len()
                && object.iter().zip(pairs).all(|((k, item), pair)| {
                    let pair = pair.as_array().expect("entry");
                    *k == units(&pair[0]) && matches_golden(item, &pair[1])
                })
        }
        ("array" | "object", _) => false,
        _ => same_value(got, &val(expected)),
    }
}

/// Bit-exact comparison: -0 differs from 0, NaN equals NaN, objects compare in order.
fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => same_num(*x, *y),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same_value(p, q))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .zip(y.iter())
                    .all(|((ka, va), (kb, vb))| ka == kb && same_value(va, vb))
        }
        _ => a == b,
    }
}

fn check(category: &str, mismatches: Vec<String>) {
    assert!(
        mismatches.is_empty(),
        "{category}: {} mismatch(es):\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}

#[test]
fn to_number_golden() {
    let mut bad = Vec::new();
    for r in records("to_number") {
        let input = units(&r["input"]);
        let expected = num(&r["output"]);
        let got = to_number(&input);
        if !same_num(got, expected) {
            bad.push(format!(
                "Number({input:?}) = {got:?}, expected {expected:?}"
            ));
        }
    }
    check("to_number", bad);
}

#[test]
fn number_to_string_golden() {
    let mut bad = Vec::new();
    for r in records("number_to_string") {
        let x = num(&r["input"]);
        let expected = units(&r["output"]);
        let got = number_to_string(x);
        if !expected.eq_str(&got) {
            bad.push(format!("String({x:?}) = {got:?}, expected {expected:?}"));
        }
    }
    check("number_to_string", bad);
}

#[test]
fn to_fixed_golden() {
    let mut bad = Vec::new();
    for r in records("to_fixed") {
        let x = num(&r["input"]);
        let digits = u32::try_from(r["digits"].as_u64().expect("digits")).expect("digits fit");
        let expected = units(&r["output"]);
        match to_fixed(x, digits) {
            Ok(got) if expected.eq_str(&got) => {}
            other => bad.push(format!(
                "({x:?}).toFixed({digits}) = {other:?}, expected {expected:?}"
            )),
        }
    }
    check("to_fixed", bad);
}

#[test]
fn parse_int_golden() {
    let mut bad = Vec::new();
    for r in records("parse_int") {
        let input = units(&r["input"]);
        let radix = opt_num(&r["radix"]);
        let expected = num(&r["output"]);
        let got = parse_int(&input, radix);
        if !same_num(got, expected) {
            bad.push(format!(
                "parseInt({input:?}, {radix:?}) = {got:?}, expected {expected:?}"
            ));
        }
    }
    check("parse_int", bad);
}

#[test]
fn math_round_golden() {
    let mut bad = Vec::new();
    for r in records("math_round") {
        let x = num(&r["input"]);
        let expected = num(&r["output"]);
        let got = math_round(x);
        if !same_num(got, expected) {
            bad.push(format!(
                "Math.round({x:?}) = {got:?}, expected {expected:?}"
            ));
        }
    }
    check("math_round", bad);
}

#[test]
fn math_floor_div_golden() {
    let mut bad = Vec::new();
    for r in records("math_floor_div") {
        let a = num(&r["a"]);
        let b = num(&r["b"]);
        let expected = num(&r["output"]);
        let got = math_floor_div(a, b);
        if !same_num(got, expected) {
            bad.push(format!(
                "Math.floor({a:?} / {b:?}) = {got:?}, expected {expected:?}"
            ));
        }
    }
    check("math_floor_div", bad);
}

#[test]
fn to_integer_or_infinity_golden() {
    let mut bad = Vec::new();
    for r in records("to_integer_or_infinity") {
        let x = num(&r["input"]);
        let expected = num(&r["output"]);
        let got = to_integer_or_infinity(x);
        if !same_num(got, expected) {
            bad.push(format!(
                "ToIntegerOrInfinity({x:?}) = {got:?}, expected {expected:?}"
            ));
        }
    }
    check("to_integer_or_infinity", bad);
}

#[test]
fn is_safe_integer_golden() {
    let mut bad = Vec::new();
    for r in records("is_safe_integer") {
        let x = num(&r["input"]);
        let safe = r["output"].as_bool().expect("bool");
        let integer = r["integer"].as_bool().expect("bool");
        if is_safe_integer(x) != safe {
            bad.push(format!("Number.isSafeInteger({x:?}) != {safe}"));
        }
        if is_integer(x) != integer {
            bad.push(format!("Number.isInteger({x:?}) != {integer}"));
        }
    }
    check("is_safe_integer", bad);
}

#[test]
fn utf16_len_golden() {
    let mut bad = Vec::new();
    for r in records("utf16_len") {
        let input = units(&r["input"]);
        let expected = usize::try_from(r["output"].as_u64().expect("length")).expect("fits");
        let got = utf16_len(&input);
        if got != expected {
            bad.push(format!("{input:?}.length = {got}, expected {expected}"));
        }
    }
    check("utf16_len", bad);
}

#[test]
fn utf16_slice_golden() {
    let mut bad = Vec::new();
    for r in records("utf16_slice") {
        let input = units(&r["input"]);
        let start = num(&r["start"]);
        let end = opt_num(&r["end"]);
        let expected = units(&r["output"]);
        let got = utf16_slice(&input, start, end);
        if got != expected {
            bad.push(format!(
                "{input:?}.slice({start:?}, {end:?}) = {got:?}, expected {expected:?}"
            ));
        }
    }
    check("utf16_slice", bad);
}

type StringOp = fn(&JsString) -> JsString;

#[test]
fn trim_golden() {
    let mut bad = Vec::new();
    for r in records("trim") {
        let input = units(&r["input"]);
        let cases: [(&str, StringOp); 3] = [
            ("trim", trim),
            ("trim_start", trim_start),
            ("trim_end", trim_end),
        ];
        for (name, f) in cases {
            let expected = units(&r[name]);
            let got = f(&input);
            if got != expected {
                bad.push(format!(
                    "{name}({:?}) = {:?}, expected {:?}",
                    input.as_units(),
                    got.as_units(),
                    expected.as_units()
                ));
            }
        }
    }
    check("trim", bad);
}

#[test]
fn pad_golden() {
    let mut bad = Vec::new();
    for r in records("pad") {
        let input = units(&r["input"]);
        let length = num(&r["length"]);
        let fill = units(&r["fill"]);
        let start = pad_start(&input, length, &fill).expect("golden lengths are small");
        let end = pad_end(&input, length, &fill).expect("golden lengths are small");
        if start != units(&r["start"]) {
            bad.push(format!(
                "padStart({input:?}, {length}, {fill:?}) = {:?}",
                start.as_units()
            ));
        }
        if end != units(&r["end"]) {
            bad.push(format!(
                "padEnd({input:?}, {length}, {fill:?}) = {:?}",
                end.as_units()
            ));
        }
    }
    check("pad", bad);
}

#[test]
fn split_golden() {
    let mut bad = Vec::new();
    for r in records("split") {
        let input = units(&r["input"]);
        let sep = units(&r["sep"]);
        let limit = opt_num(&r["limit"]);
        let expected: Vec<JsString> = r["output"]
            .as_array()
            .expect("array")
            .iter()
            .map(units)
            .collect();
        let got = split(&input, &sep, limit);
        if got != expected {
            bad.push(format!(
                "{input:?}.split({sep:?}, {limit:?}) = {got:?}, expected {expected:?}"
            ));
        }
    }
    check("split", bad);
}

#[test]
fn replace_golden() {
    let mut bad = Vec::new();
    for r in records("replace") {
        let input = units(&r["input"]);
        let pattern = units(&r["pattern"]);
        let replacement = units(&r["replacement"]);
        let first = string_replace_first(&input, &pattern, &replacement);
        let all = string_replace_all(&input, &pattern, &replacement);
        if first != units(&r["first"]) {
            bad.push(format!(
                "{input:?}.replace({pattern:?}, {replacement:?}) = {first:?}"
            ));
        }
        if all != units(&r["all"]) {
            bad.push(format!(
                "{input:?}.replaceAll({pattern:?}, {replacement:?}) = {all:?}"
            ));
        }
    }
    check("replace", bad);
}

#[test]
fn encode_uri_component_golden() {
    let mut bad = Vec::new();
    for r in records("encode_uri_component") {
        let input = units(&r["input"]);
        let got = encode_uri_component(&input);
        match (&got, r.get("output"), r.get("error").and_then(J::as_str)) {
            (Ok(text), Some(expected), _) if units(expected).eq_str(text) => {}
            (Err(e), _, Some(expected)) if format!("{}: {}", e.js_name(), e) == expected => {}
            _ => bad.push(format!(
                "encodeURIComponent({:?}) = {got:?}, expected {r}",
                input.as_units()
            )),
        }
    }
    check("encode_uri_component", bad);
}

#[test]
fn to_display_string_golden() {
    let mut bad = Vec::new();
    for r in records("to_display_string") {
        let input = val(&r["input"]);
        let expected = units(&r["output"]);
        let got = to_display_string(&input);
        if got != expected {
            bad.push(format!(
                "String({input:?}) = {got:?}, expected {expected:?}"
            ));
        }
    }
    check("to_display_string", bad);
}

#[test]
fn is_truthy_golden() {
    let mut bad = Vec::new();
    for r in records("is_truthy") {
        let input = val(&r["input"]);
        let expected = r["output"].as_bool().expect("bool");
        if is_truthy(&input) != expected {
            bad.push(format!("Boolean({input:?}) != {expected}"));
        }
    }
    check("is_truthy", bad);
}

#[test]
fn to_number_value_golden() {
    let mut bad = Vec::new();
    for r in records("to_number_value") {
        let input = val(&r["input"]);
        let expected = num(&r["output"]);
        let got = to_number_value(&input);
        if !same_num(got, expected) {
            bad.push(format!(
                "Number({input:?}) = {got:?}, expected {expected:?}"
            ));
        }
    }
    check("to_number_value", bad);
}

#[test]
fn less_than_golden() {
    let mut bad = Vec::new();
    for r in records("less_than") {
        let a = val(&r["a"]);
        let b = val(&r["b"]);
        let expected = r["output"].as_bool().expect("bool");
        if less_than(&a, &b) != expected {
            bad.push(format!("{a:?} < {b:?} != {expected}"));
        }
    }
    check("less_than", bad);
}

#[test]
fn json_stringify_golden() {
    let mut bad = Vec::new();
    for r in records("json_stringify") {
        let input = val(&r["input"]);
        let compact = json_stringify(&input);
        let pretty = json_stringify_pretty(&input, 2);
        let expected = units(&r["output"]);
        let expected_pretty = units(&r["pretty"]);
        if !compact.as_deref().is_some_and(|s| expected.eq_str(s)) {
            bad.push(format!(
                "JSON.stringify({input:?}) = {compact:?}, expected {expected:?}"
            ));
        }
        if !pretty.as_deref().is_some_and(|s| expected_pretty.eq_str(s)) {
            bad.push(format!(
                "JSON.stringify({input:?}, null, 2) = {pretty:?}, expected {expected_pretty:?}"
            ));
        }
    }
    if json_stringify(&Value::Undefined).is_some() {
        bad.push("JSON.stringify(undefined) should be None".to_string());
    }
    check("json_stringify", bad);
}

#[test]
fn json_parse_golden() {
    let mut bad = Vec::new();
    for r in records("json_parse") {
        let input = String::from_utf16(units(&r["input"]).as_units())
            .expect("golden JSON texts are well-formed");
        let got = json_parse(&input);
        match (&got, r.get("output"), r.get("error").and_then(J::as_str)) {
            (Ok(v), Some(expected), _) if matches_golden(v, expected) => {}
            (Err(e), _, Some(expected)) if format!("{}: {}", e.js_name(), e) == expected => {}
            _ => {
                let got_text = match &got {
                    Ok(v) => format!("{v:?}"),
                    Err(e) => format!("{}: {}", e.js_name(), e),
                };
                let expected = r
                    .get("error")
                    .map_or_else(|| format!("{:?}", r.get("output").map(val)), J::to_string);
                bad.push(format!(
                    "JSON.parse({input:?}) = {got_text}, expected {expected}"
                ));
            }
        }
    }
    check("json_parse", bad);
}

#[test]
fn response_text_golden() {
    let mut bad = Vec::new();
    for r in records("response_text") {
        let bytes: Vec<u8> = r["input"]
            .as_array()
            .expect("bytes")
            .iter()
            .map(|b| u8::try_from(b.as_u64().expect("byte")).expect("byte fits"))
            .collect();
        let expected = units(&r["output"]);
        let got = response_text(&bytes);
        if !expected.eq_str(&got) {
            bad.push(format!("text({bytes:?}) = {got:?}, expected {expected:?}"));
        }
    }
    check("response_text", bad);
}

#[test]
fn sort_default_golden() {
    let mut bad = Vec::new();
    for r in records("sort_default") {
        let mut list: Vec<JsString> = r["input"]
            .as_array()
            .expect("list")
            .iter()
            .map(units)
            .collect();
        let expected: Vec<JsString> = r["output"]
            .as_array()
            .expect("list")
            .iter()
            .map(units)
            .collect();
        sort_default(&mut list);
        if list != expected {
            bad.push(format!("sort = {list:?}, expected {expected:?}"));
        }
    }
    check("sort_default", bad);
}

#[test]
fn object_assign_golden() {
    let mut bad = Vec::new();
    for r in records("object_assign") {
        let a = val(&r["a"]);
        let b = val(&r["b"]);
        let (Some(a), Some(b)) = (a.as_object(), b.as_object()) else {
            bad.push(format!("inputs are not objects: {r}"));
            continue;
        };
        let mut target = Object::new();
        object_assign(&mut target, a);
        object_assign(&mut target, b);
        let got = Value::Object(target);
        for field in ["output", "spread"] {
            if !matches_golden(&got, &r[field]) {
                bad.push(format!(
                    "{field}: assign({a:?}, {b:?}) = {got:?}, expected {}",
                    r[field]
                ));
            }
        }
    }
    check("object_assign", bad);
}

#[test]
fn object_from_entries_golden() {
    let mut bad = Vec::new();
    for r in records("object_from_entries") {
        let pairs: Vec<(JsString, Value)> = r["input"]
            .as_array()
            .expect("entries")
            .iter()
            .map(|p| (units(&p[0]), val(&p[1])))
            .collect();
        let got = Value::Object(Object::from_pairs(pairs));
        if !matches_golden(&got, &r["output"]) {
            bad.push(format!(
                "Object.fromEntries = {got:?}, expected {}",
                r["output"]
            ));
        }
    }
    check("object_from_entries", bad);
}

#[test]
fn error_to_string_golden() {
    let mut bad = Vec::new();
    for r in records("error_to_string") {
        let name = r["name"].as_str().expect("name");
        let message = r["message"].as_str().expect("message");
        let expected = r["output"].as_str().expect("output");
        let err = JsError {
            name: name.to_string(),
            message: message.to_string(),
        };
        let got = error_to_string(&err);
        if got != expected {
            bad.push(format!(
                "String(err {name:?}, {message:?}) = {got:?}, expected {expected:?}"
            ));
        }
    }
    check("error_to_string", bad);
}

// ---------------------------------------------------------------------------
// Focused unit tests for the serde bridges.

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct Sample {
    zeta: String,
    alpha: i64,
    maybe: Option<u32>,
    list: Vec<f64>,
    text: JsString,
}

#[test]
fn to_value_keeps_declaration_order_and_undefined_none() {
    let sample = Sample {
        zeta: "z".to_string(),
        alpha: -3,
        maybe: None,
        list: vec![1.5],
        text: JsString::from_units(vec![0x61, 0xD83D]),
    };
    let value = to_value(&sample).expect("serializes");
    let object = value.as_object().expect("object");
    let keys: Vec<String> = object.keys().map(JsString::to_string_lossy).collect();
    assert_eq!(keys, ["zeta", "alpha", "maybe", "list", "text"]);
    assert_eq!(object.get("alpha"), Some(&Value::Number(-3.0)));
    assert_eq!(object.get("maybe"), Some(&Value::Undefined));
    assert_eq!(
        object.get("text"),
        Some(&Value::String(JsString::from_units(vec![0x61, 0xD83D])))
    );
    assert_eq!(
        json_stringify(&value).as_deref(),
        Some(r#"{"zeta":"z","alpha":-3,"list":[1.5],"text":"a\ud83d"}"#)
    );
}

#[test]
fn from_value_round_trips() {
    let sample = Sample {
        zeta: "z".to_string(),
        alpha: 9_007_199_254_740_991,
        maybe: Some(7),
        list: vec![-0.5, 2.0],
        text: JsString::from_units(vec![0xDC00, 0x7A]),
    };
    let value = to_value(&sample).expect("serializes");
    let back: Sample = from_value(&value).expect("deserializes");
    assert_eq!(back, sample);
}

#[test]
fn from_value_accepts_missing_and_null_options() {
    let value = json_parse(r#"{"zeta":"z","alpha":1,"maybe":null,"list":[],"text":"\ud800"}"#)
        .expect("parses");
    let parsed: Sample = from_value(&value).expect("deserializes");
    assert_eq!(parsed.maybe, None);
    assert_eq!(parsed.text, JsString::from_units(vec![0xD800]));
    let value = json_parse(r#"{"zeta":"z","alpha":1,"list":[],"text":""}"#).expect("parses");
    let parsed: Sample = from_value(&value).expect("deserializes");
    assert_eq!(parsed.maybe, None);
}

#[test]
fn integers_must_be_safe_and_whole() {
    assert!(to_value(&9_007_199_254_740_992_i64).is_err());
    assert!(to_value(&u64::MAX).is_err());
    assert_eq!(to_value(&42_u8).expect("small"), Value::Number(42.0));
    assert!(from_value::<i64>(&Value::Number(1.5)).is_err());
    assert!(from_value::<i64>(&Value::Number(9_007_199_254_740_992.0)).is_err());
    assert!(from_value::<u8>(&Value::Number(300.0)).is_err());
    assert!(from_value::<u32>(&Value::Number(-1.0)).is_err());
    assert_eq!(from_value::<i32>(&Value::Number(-12.0)).expect("int"), -12);
    assert_eq!(from_value::<f64>(&Value::Number(2.0)).expect("float"), 2.0);
}

#[test]
fn string_deserializes_lossy() {
    let value = Value::String(JsString::from_units(vec![0x61, 0xD83D]));
    assert_eq!(from_value::<String>(&value).expect("string"), "a\u{FFFD}");
}
