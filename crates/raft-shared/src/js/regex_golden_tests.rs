//! raft_shared::js::JsRegex proof against Node 24.15.0 (decisions.md D3):
//! the upstream regex corpus golden plus focused semantics tests.

use std::collections::{BTreeMap, HashMap};

use serde_json::Value as J;

use super::*;

const CORPUS: &str = include_str!("../../../../tests/golden/raft-shared/regex-corpus.json");

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

fn opt_units(j: &J) -> Option<JsString> {
    if j.is_null() { None } else { Some(units(j)) }
}

fn js(s: &str) -> JsString {
    JsString::from(s)
}

struct Mismatches {
    by_kind: BTreeMap<&'static str, usize>,
    examples: Vec<String>,
}

impl Mismatches {
    fn push(&mut self, kind: &'static str, detail: String) {
        *self.by_kind.entry(kind).or_default() += 1;
        if self.examples.len() < 30 {
            self.examples.push(format!("[{kind}] {detail}"));
        }
    }
}

#[test]
fn regex_corpus_matches_node() {
    let root: J = serde_json::from_str(CORPUS).expect("corpus is valid JSON");
    let inputs: Vec<JsString> = root["inputs"]
        .as_array()
        .expect("inputs")
        .iter()
        .map(units)
        .collect();
    let records = root["records"].as_array().expect("records");
    assert_eq!(records.len(), root["count"].as_u64().expect("count") as usize);
    let mut bad = Mismatches {
        by_kind: BTreeMap::new(),
        examples: Vec::new(),
    };
    let replacement = js("<$&|$1|$$>");

    for rec in records {
        let pattern = units(&rec["pattern"]);
        let flags = rec["flags"].as_str().expect("flags");
        let label = format!("/{}/{flags}", pattern.to_string_lossy());
        if let Some(err) = rec.get("error") {
            match JsRegex::new_js(&pattern, flags) {
                Ok(_) => bad.push("construct", format!("{label}: node threw {err}, JsRegex accepted")),
                Err(e) => {
                    let got = error_to_string(&e);
                    if err.as_str() != Some(got.as_str()) {
                        bad.push("error-text", format!("{label}: node {err}, got {got:?}"));
                    }
                }
            }
            continue;
        }
        let re = match JsRegex::new_js(&pattern, flags) {
            Ok(re) => re,
            Err(e) => {
                bad.push("construct", format!("{label}: {}", error_to_string(&e)));
                continue;
            }
        };
        let single_flags: String = flags.chars().filter(|&c| c != 'g' && c != 'y').collect();
        let single = JsRegex::new_js(&pattern, &single_flags).expect("single-match variant builds");

        let r = rec["r"].as_array().expect("r");
        let named = rec.get("named").and_then(J::as_array);
        let mut hits = Vec::new();
        for (i, input) in inputs.iter().enumerate() {
            let got = single.exec(input);
            let expected = &r[i];
            match (expected, &got) {
                (J::Number(_), None) => {}
                (J::Number(_), Some(m)) => {
                    bad.push("exec", format!("{label} on input {i}: node no match, got index {}", m.index))
                }
                (J::Array(_), None) => bad.push("exec", format!("{label} on input {i}: node {expected}, got no match")),
                (J::Array(list), Some(m)) => {
                    let want_index = list[0].as_u64().expect("index") as usize;
                    let want: Vec<Option<JsString>> = list[1..].iter().map(opt_units).collect();
                    if want_index != m.index || want != m.captures {
                        bad.push(
                            "exec",
                            format!("{label} on input {i}: node {expected}, got index {} {:?}", m.index, m.captures),
                        );
                    }
                }
                _ => panic!("bad r entry {expected}"),
            }
            if let Some(named) = named {
                let want: Option<HashMap<String, Option<JsString>>> = match &named[i] {
                    J::Object(o) => Some(o.iter().map(|(k, v)| (k.clone(), opt_units(v))).collect()),
                    _ => None,
                };
                let have: Option<HashMap<String, Option<JsString>>> = got
                    .as_ref()
                    .and_then(|m| m.groups.as_ref())
                    .map(|g| g.iter().cloned().collect());
                if want != have {
                    bad.push("named", format!("{label} on input {i}: node {}, got {have:?}", named[i]));
                }
            }
            if !matches!(expected, J::Number(_)) {
                hits.push(i);
            }
        }

        if let Some(all) = rec.get("all").and_then(J::as_array) {
            for (k, &i) in hits.iter().enumerate() {
                let want: Vec<usize> = all[k]
                    .as_array()
                    .expect("all entry")
                    .iter()
                    .map(|x| x.as_u64().expect("index") as usize)
                    .collect();
                let fresh = JsRegex::new_js(&pattern, flags).expect("builds");
                match fresh.match_all(&inputs[i]) {
                    Ok(ms) => {
                        let have: Vec<usize> = ms.iter().map(|m| m.index).collect();
                        if want != have {
                            bad.push("all", format!("{label} on input {i}: node {want:?}, got {have:?}"));
                        }
                    }
                    Err(e) => bad.push("all", format!("{label} on input {i}: {}", error_to_string(&e))),
                }
            }
        }

        let replaces = rec["replace"].as_array().expect("replace");
        let splits = rec["split"].as_array().expect("split");
        for (k, &i) in hits.iter().enumerate() {
            re.set_last_index(0);
            let want = units(&replaces[k]);
            let have = re.replace(&inputs[i], &replacement);
            if want != have {
                bad.push("replace", format!("{label} on input {i}: node {want:?}, got {have:?}"));
            }
            let want: Vec<Option<JsString>> = splits[k]
                .as_array()
                .expect("split entry")
                .iter()
                .map(opt_units)
                .collect();
            let have = single.split(&inputs[i], None);
            if want != have {
                bad.push("split", format!("{label} on input {i}: node {want:?}, got {have:?}"));
            }
        }
    }

    if !bad.by_kind.is_empty() {
        eprintln!("regex corpus mismatches by kind: {:?}", bad.by_kind);
        for e in &bad.examples {
            eprintln!("  {e}");
        }
    }
    assert!(bad.by_kind.is_empty(), "regex corpus mismatches: {:?}", bad.by_kind);
}

#[test]
fn global_test_loop_advances_and_resets_last_index() {
    let re = JsRegex::new("o", "g").unwrap();
    let s = js("foo boo");
    let mut seen = Vec::new();
    while re.test(&s) {
        seen.push(re.last_index());
    }
    assert_eq!(seen, vec![2, 3, 6, 7]);
    assert_eq!(re.last_index(), 0);

    re.set_last_index(5);
    assert!(!re.test(&js("aaa")));
    assert_eq!(re.last_index(), 0);
}

#[test]
fn sticky_matches_only_at_last_index() {
    let re = JsRegex::new("o", "y").unwrap();
    let s = js("foo boo");
    re.set_last_index(1);
    assert!(re.test(&s));
    assert_eq!(re.last_index(), 2);
    assert!(re.test(&s));
    assert_eq!(re.last_index(), 3);
    assert!(!re.test(&s));
    assert_eq!(re.last_index(), 0);

    // Non-global sticky replace starts at lastIndex and updates it.
    let y = JsRegex::new("a", "y").unwrap();
    assert_eq!(y.replace(&js("xax"), &js("-")), js("xax"));
    y.set_last_index(1);
    assert_eq!(y.replace(&js("xax"), &js("-")), js("x-x"));
    assert_eq!(y.last_index(), 2);

    let gy = JsRegex::new("a", "gy").unwrap();
    assert_eq!(gy.replace(&js("aaa"), &js("x")), js("xxx"));
}

#[test]
fn non_stateful_exec_ignores_last_index() {
    let re = JsRegex::new("a", "").unwrap();
    re.set_last_index(3);
    assert_eq!(re.exec(&js("aa")).unwrap().index, 0);
    assert_eq!(re.last_index(), 3);
}

#[test]
fn global_replace_starts_at_zero_and_leaves_zero() {
    let re = JsRegex::new("a", "g").unwrap();
    re.set_last_index(2);
    assert_eq!(re.replace(&js("aba"), &js("x")), js("xbx"));
    assert_eq!(re.last_index(), 0);
}

#[test]
fn unicode_last_index_inside_pair_reads_from_pair_start() {
    let s = js("😀x");
    for flags in ["gu", "yu"] {
        let re = JsRegex::new(".", flags).unwrap();
        re.set_last_index(1);
        let m = re.exec(&s).unwrap();
        assert_eq!(m.index, 0);
        assert_eq!(m.matched(), &js("😀"));
        assert_eq!(re.last_index(), 2);
    }
}

#[test]
fn d_flag_indices() {
    let re = JsRegex::new("(?<a>b)(c)?", "d").unwrap();
    assert_eq!(re.flags(), "d");
    let m = re.exec(&js("abx")).unwrap();
    let ind = m.indices.expect("indices with d");
    assert_eq!(ind.ranges, vec![Some((1, 2)), Some((1, 2)), None]);
    assert_eq!(ind.groups, Some(vec![("a".to_string(), Some((1, 2)))]));
    assert_eq!(m.group("a"), Some(&js("b")));
    assert!(JsRegex::new("b", "").unwrap().exec(&js("b")).unwrap().indices.is_none());
}

#[test]
fn empty_matches_advance_by_code_point_only_under_u() {
    let s = js("😀😀");
    let idx = |flags: &str| -> Vec<usize> {
        JsRegex::new("(?:)", flags)
            .unwrap()
            .match_all(&s)
            .unwrap()
            .iter()
            .map(|m| m.index)
            .collect()
    };
    assert_eq!(idx("gu"), vec![0, 2, 4]);
    assert_eq!(idx("g"), vec![0, 1, 2, 3, 4]);

    let t = js("x😀y");
    assert_eq!(JsRegex::new("(?:)", "gu").unwrap().replace(&t, &js("-")), js("-x-😀-y-"));
    assert_eq!(JsRegex::new("(?:)", "g").unwrap().replace(&t, &js("-")).len(), 9);

    let split_u = JsRegex::new("(?:)", "u").unwrap().split(&js("a😀b"), None);
    assert_eq!(split_u, vec![Some(js("a")), Some(js("😀")), Some(js("b"))]);
    let split = JsRegex::new("(?:)", "").unwrap().split(&js("a😀b"), None);
    assert_eq!(split.len(), 4);
}

#[test]
fn match_all_requires_global() {
    let err = JsRegex::new("a", "").unwrap().match_all(&js("a")).unwrap_err();
    assert_eq!(
        error_to_string(&err),
        "TypeError: String.prototype.matchAll called with a non-global RegExp argument"
    );
}

#[test]
fn split_limits_and_captures() {
    let digits = JsRegex::new("(\\d)", "").unwrap();
    assert_eq!(
        digits.split(&js("a1b2c3"), Some(3.0)),
        vec![Some(js("a")), Some(js("1")), Some(js("b"))]
    );
    // -1 is ToUint32'd to 2^32 - 1.
    assert_eq!(
        JsRegex::new("\\d", "").unwrap().split(&js("a1b2c3"), Some(-1.0)),
        vec![Some(js("a")), Some(js("b")), Some(js("c")), Some(js(""))]
    );
    assert_eq!(
        JsRegex::new("(?:)", "").unwrap().split(&js("abc"), Some(2.0)),
        vec![Some(js("a")), Some(js("b"))]
    );
    assert!(digits.split(&js("a1b"), Some(0.0)).is_empty());
    assert_eq!(
        JsRegex::new("(b)|(x)", "").unwrap().split(&js("abc"), None),
        vec![Some(js("a")), Some(js("b")), None, Some(js("c"))]
    );
    assert_eq!(JsRegex::new("x", "").unwrap().split(&js(""), None), vec![Some(js(""))]);
    assert!(JsRegex::new("(?:)", "").unwrap().split(&js(""), None).is_empty());
    assert_eq!(
        JsRegex::new("x", "i").unwrap().split(&js("aXbxc"), None),
        vec![Some(js("a")), Some(js("b")), Some(js("c"))]
    );
}

#[test]
fn replace_dollar_edge_cases() {
    let b = JsRegex::new("b", "").unwrap();
    assert_eq!(b.replace(&js("abc"), &js("$`|$'|$&|$$|$")), js("aa|c|b|$|$c"));
    let one = JsRegex::new("(b)", "").unwrap();
    assert_eq!(one.replace(&js("abc"), &js("[$10|$01|$2|$<x>|$0|$]")), js("a[b0|b|$2|$<x>|$0|$]c"));
    let named = JsRegex::new("(?<x>b)", "").unwrap();
    assert_eq!(named.replace(&js("abc"), &js("[$<x>|$<y>|$<x]")), js("a[b||$<x]c"));
    assert_eq!(named.replace(&js("abc"), &js("$<n>$<m>$<")), js("a$<c"));
    let g = JsRegex::new("(b)", "g").unwrap();
    assert_eq!(g.replace(&js("abc"), &js("$11")), js("ab1c"));
}

#[test]
fn replace_with_callback_sees_match() {
    let re = JsRegex::new("(?<d>\\d)", "g").unwrap();
    let out = re.replace_with(&js("a1b22"), |m| {
        js(&format!("<{}@{}:{}>", m.matched(), m.index, m.group("d").unwrap()))
    });
    assert_eq!(out, js("a<1@1:1>b<2@3:2><2@4:2>"));
}

#[test]
fn replace_all_requires_global() {
    let err = JsRegex::new("a", "").unwrap().replace_all(&js("a"), &js("b")).unwrap_err();
    assert_eq!(err.name, "TypeError");
    assert_eq!(
        err.message,
        "String.prototype.replaceAll called with a non-global RegExp argument"
    );
    let ok = JsRegex::new("a", "g").unwrap().replace_all(&js("aba"), &js("x")).unwrap();
    assert_eq!(ok, js("xbx"));
}

#[test]
fn invalid_patterns_and_flags_throw_v8_syntax_errors() {
    let cases = [
        ("a", "gg", "Invalid flags supplied to RegExp constructor 'gg'"),
        ("a", "x", "Invalid flags supplied to RegExp constructor 'x'"),
        ("a", "uv", "Invalid flags supplied to RegExp constructor 'uv'"),
        ("(", "yig", "Invalid regular expression: /(/giy: Unterminated group"),
        (")", "", "Invalid regular expression: /)/: Unmatched ')'"),
        ("[", "", "Invalid regular expression: /[/: Unterminated character class"),
        ("*", "", "Invalid regular expression: /*/: Nothing to repeat"),
        ("a/(", "", "Invalid regular expression: /a/(/: Unterminated group"),
        (
            "(?<a>x)(?<a>y)",
            "",
            "Invalid regular expression: /(?<a>x)(?<a>y)/: Duplicate capture group name",
        ),
    ];
    for (pattern, flags, message) in cases {
        let err = JsRegex::new(pattern, flags).unwrap_err();
        assert_eq!(err.name, "SyntaxError", "/{pattern}/{flags}");
        assert_eq!(err.message, message, "/{pattern}/{flags}");
    }
}

#[test]
fn source_and_flags_accessors() {
    let re = JsRegex::new("a/b[/]\\/", "gimsyd").unwrap();
    assert_eq!(re.source(), js("a\\/b[/]\\/"));
    assert_eq!(re.flags(), "dgimsy");
    assert!(re.global());
    assert_eq!(JsRegex::new("", "").unwrap().source(), js("(?:)"));
    assert_eq!(JsRegex::new("\n", "").unwrap().source(), js("\\n"));
}

#[test]
fn non_unicode_pattern_reads_code_units() {
    // Without u, [😀] is a class of two code units, so ^[😀]$ cannot match "😀".
    assert!(JsRegex::new("^[😀]$", "").unwrap().exec(&js("😀")).is_none());
    assert!(JsRegex::new("^[😀]$", "u").unwrap().test(&js("😀")));
    assert!(JsRegex::new("\\ud83d", "").unwrap().test(&js("😀")));
    assert!(!JsRegex::new("\\ud83d", "u").unwrap().test(&js("😀")));
}

#[test]
fn case_folding_matches_node() {
    assert!(!JsRegex::new("\\u212A", "i").unwrap().test(&js("k")));
    assert!(JsRegex::new("\\u212A", "iu").unwrap().test(&js("k")));
    assert!(JsRegex::new("STRASSE", "i").unwrap().exec(&js("Straße")).is_none());
}
