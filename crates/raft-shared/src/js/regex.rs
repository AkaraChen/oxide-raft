//! raft_shared::js regular expressions: `JsRegex` over `regress` on UTF-16
//! (decisions.md D3, mapping-guide §6.2).
//!
//! Patterns without `u`/`v` run with `find_from_ucs2` (code-unit matching, the
//! pattern itself read as code units); with `u`/`v` they run with
//! `find_from_utf16`. Every index is a UTF-16 code unit. `regress` parses
//! `i m s u v`; `d g y` and `lastIndex` are implemented here.
//!
//! State: `lastIndex` lives in an `AtomicUsize` (relaxed), so every method takes
//! `&self` and a `JsRegex` can sit in a `static LazyLock` like a module-level JS
//! regex literal. A translated `while (re.test(s))` loop is `while re.test(&s)`.

use std::ops::Range;
use std::sync::atomic::{AtomicUsize, Ordering};

use regress::{Flags, Regex};

use super::convert::to_uint32;
use super::error::JsError;
use super::string::JsString;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct JsFlags {
    has_indices: bool,
    global: bool,
    ignore_case: bool,
    multiline: bool,
    dot_all: bool,
    unicode: bool,
    unicode_sets: bool,
    sticky: bool,
}

impl JsFlags {
    /// The RegExp constructor's flag validation: only `dgimsuvy`, no repeats,
    /// not both `u` and `v`.
    fn parse(flags: &str) -> Option<JsFlags> {
        let mut f = JsFlags::default();
        for c in flags.chars() {
            let slot = match c {
                'd' => &mut f.has_indices,
                'g' => &mut f.global,
                'i' => &mut f.ignore_case,
                'm' => &mut f.multiline,
                's' => &mut f.dot_all,
                'u' => &mut f.unicode,
                'v' => &mut f.unicode_sets,
                'y' => &mut f.sticky,
                _ => return None,
            };
            if *slot {
                return None;
            }
            *slot = true;
        }
        if f.unicode && f.unicode_sets {
            return None;
        }
        Some(f)
    }

    /// `RegExp.prototype.flags`: canonical order `dgimsuvy`.
    fn canonical(self) -> String {
        let mut out = String::new();
        for (on, c) in [
            (self.has_indices, 'd'),
            (self.global, 'g'),
            (self.ignore_case, 'i'),
            (self.multiline, 'm'),
            (self.dot_all, 's'),
            (self.unicode, 'u'),
            (self.unicode_sets, 'v'),
            (self.sticky, 'y'),
        ] {
            if on {
                out.push(c);
            }
        }
        out
    }

    fn full_unicode(self) -> bool {
        self.unicode || self.unicode_sets
    }

    fn engine_flags(self) -> Flags {
        Flags {
            icase: self.ignore_case,
            multiline: self.multiline,
            dot_all: self.dot_all,
            no_opt: false,
            // regress keys surrogate-pair decoding of the UTF-16 input and the
            // strict unicode grammar off `unicode` alone, so `v` sets both.
            unicode: self.full_unicode(),
            unicode_sets: self.unicode_sets,
        }
    }
}

/// `d`-flag match indices: `[start, end)` pairs in code units.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsMatchIndices {
    /// Index 0 is the whole match; `None` for a group that did not participate.
    pub ranges: Vec<Option<(usize, usize)>>,
    /// Named groups in declaration order; `None` when the pattern has none.
    pub groups: Option<Vec<(String, Option<(usize, usize)>)>>,
}

/// The result of `exec`: a JS match array.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsMatch {
    /// `m.index`, in code units.
    pub index: usize,
    /// `m.input`.
    pub input: JsString,
    /// `m[0]`, `m[1]`, ...; `None` is `undefined`.
    pub captures: Vec<Option<JsString>>,
    /// `m.groups` in declaration order; `None` when the pattern has no named groups.
    pub groups: Option<Vec<(String, Option<JsString>)>>,
    /// `m.indices` (only with the `d` flag).
    pub indices: Option<JsMatchIndices>,
}

impl JsMatch {
    /// `m[0]`.
    pub fn matched(&self) -> &JsString {
        self.captures[0].as_ref().expect("the whole match is always defined")
    }

    /// `m[i]`; `None` for `undefined` or out of range.
    pub fn get(&self, i: usize) -> Option<&JsString> {
        self.captures.get(i).and_then(Option::as_ref)
    }

    /// `m.groups?.[name]`.
    pub fn group(&self, name: &str) -> Option<&JsString> {
        self.groups
            .as_ref()?
            .iter()
            .find(|(n, _)| n == name)
            .and_then(|(_, v)| v.as_ref())
    }

    /// `m.index + m[0].length`.
    pub fn end(&self) -> usize {
        self.index + self.matched().len()
    }
}

/// A JS `RegExp`.
#[derive(Debug)]
pub struct JsRegex {
    regex: Regex,
    source: JsString,
    flags: JsFlags,
    last_index: AtomicUsize,
}

impl Clone for JsRegex {
    fn clone(&self) -> Self {
        JsRegex {
            regex: self.regex.clone(),
            source: self.source.clone(),
            flags: self.flags,
            last_index: AtomicUsize::new(self.last_index()),
        }
    }
}

fn is_high(u: u16) -> bool {
    (0xD800..0xDC00).contains(&u)
}

fn is_low(u: u16) -> bool {
    (0xDC00..0xE000).contains(&u)
}

fn is_digit(u: u16) -> bool {
    (u16::from(b'0')..=u16::from(b'9')).contains(&u)
}

/// AdvanceStringIndex.
fn advance(units: &[u16], index: usize, unicode: bool) -> usize {
    if !unicode || index + 1 >= units.len() {
        return index + 1;
    }
    if is_high(units[index]) && is_low(units[index + 1]) {
        index + 2
    } else {
        index + 1
    }
}

/// The pattern as regress reads it: code units without `u`/`v`, code points
/// (lone surrogates kept) with them.
fn pattern_chars(units: &[u16], unicode: bool) -> Vec<u32> {
    if !unicode {
        return units.iter().map(|&u| u32::from(u)).collect();
    }
    let mut out = Vec::with_capacity(units.len());
    let mut i = 0;
    while i < units.len() {
        let u = units[i];
        if is_high(u) && i + 1 < units.len() && is_low(units[i + 1]) {
            let hi = u32::from(u) - 0xD800;
            let lo = u32::from(units[i + 1]) - 0xDC00;
            out.push(0x10000 + (hi << 10) + lo);
            i += 2;
        } else {
            out.push(u32::from(u));
            i += 1;
        }
    }
    out
}

/// Whether an unbalanced-parenthesis error is an unmatched `)` (else an
/// unterminated group), scanning past escapes and character classes.
fn has_unmatched_close(units: &[u16]) -> bool {
    let mut depth: usize = 0;
    let mut in_class = false;
    let mut i = 0;
    while i < units.len() {
        let c = units[i];
        if c == u16::from(b'\\') {
            i += 2;
            continue;
        }
        if in_class {
            if c == u16::from(b']') {
                in_class = false;
            }
        } else if c == u16::from(b'[') {
            in_class = true;
        } else if c == u16::from(b'(') {
            depth += 1;
        } else if c == u16::from(b')') {
            if depth == 0 {
                return true;
            }
            depth -= 1;
        }
        i += 1;
    }
    false
}

/// V8's reason text for a regress parse error, where the two are the same
/// failure; otherwise regress's own text.
fn v8_reason(regress_text: &str, pattern: &[u16]) -> String {
    let reason = match regress_text {
        "Unbalanced parenthesis" => {
            if has_unmatched_close(pattern) {
                "Unmatched ')'"
            } else {
                "Unterminated group"
            }
        }
        "Unbalanced bracket" => "Unterminated character class",
        "Invalid quantifier" | "Quantifier not allowed here" => "Nothing to repeat",
        "Incomplete escape" | "Unterminated escape" => "\\ at end of pattern",
        "Invalid character escape" => "Invalid escape",
        "Invalid unicode escape" => "Invalid Unicode escape",
        "Invalid property escape" | "Invalid property name" => "Invalid property name",
        "Invalid token at named capture group identifier" => "Invalid capture group name",
        "Range values reversed, start char code is greater than end char code." => {
            "Range out of order in character class"
        }
        "Invalid character range" => "Invalid character class",
        "Duplicate capture group name" => "Duplicate capture group name",
        other if other.starts_with("Backreference to invalid named capture group") => {
            "Invalid named capture referenced"
        }
        other => other,
    };
    reason.to_string()
}

impl JsRegex {
    /// `new RegExp(pattern, flags)` / `/pattern/flags`.
    pub fn new(pattern: &str, flags: &str) -> Result<JsRegex, JsError> {
        Self::new_js(&JsString::from(pattern), flags)
    }

    /// `new RegExp(pattern, flags)` with a JS string pattern.
    pub fn new_js(pattern: &JsString, flags: &str) -> Result<JsRegex, JsError> {
        let Some(parsed) = JsFlags::parse(flags) else {
            return Err(JsError::syntax(format!(
                "Invalid flags supplied to RegExp constructor '{flags}'"
            )));
        };
        let chars = pattern_chars(pattern.as_units(), parsed.full_unicode());
        let regex = Regex::from_unicode(chars.into_iter(), parsed.engine_flags()).map_err(|e| {
            JsError::syntax(format!(
                "Invalid regular expression: /{}/{}: {}",
                pattern.to_string_lossy(),
                parsed.canonical(),
                v8_reason(&e.text, pattern.as_units())
            ))
        })?;
        Ok(JsRegex {
            regex,
            source: pattern.clone(),
            flags: parsed,
            last_index: AtomicUsize::new(0),
        })
    }

    /// `re.source`: EscapeRegExpPattern as V8 does it (`/` outside a class and
    /// line terminators escaped, `(?:)` for an empty pattern).
    pub fn source(&self) -> JsString {
        let units = self.source.as_units();
        if units.is_empty() {
            return JsString::from("(?:)");
        }
        let mut out = JsString::new();
        let mut in_class = false;
        let mut i = 0;
        while i < units.len() {
            let c = units[i];
            if c == u16::from(b'\\') && i + 1 < units.len() {
                out.push_units(&units[i..i + 2]);
                i += 2;
                continue;
            }
            match c {
                0x0A => out.push_str("\\n"),
                0x0D => out.push_str("\\r"),
                0x2028 => out.push_str("\\u2028"),
                0x2029 => out.push_str("\\u2029"),
                0x2F if !in_class => out.push_str("\\/"),
                _ => {
                    if c == u16::from(b'[') {
                        in_class = true;
                    } else if c == u16::from(b']') {
                        in_class = false;
                    }
                    out.push_units(&[c]);
                }
            }
            i += 1;
        }
        out
    }

    /// `re.flags` (canonical order `dgimsuvy`).
    pub fn flags(&self) -> String {
        self.flags.canonical()
    }

    pub fn global(&self) -> bool {
        self.flags.global
    }

    pub fn sticky(&self) -> bool {
        self.flags.sticky
    }

    pub fn has_indices(&self) -> bool {
        self.flags.has_indices
    }

    pub fn ignore_case(&self) -> bool {
        self.flags.ignore_case
    }

    pub fn multiline(&self) -> bool {
        self.flags.multiline
    }

    pub fn dot_all(&self) -> bool {
        self.flags.dot_all
    }

    pub fn unicode(&self) -> bool {
        self.flags.unicode
    }

    pub fn unicode_sets(&self) -> bool {
        self.flags.unicode_sets
    }

    /// `re.lastIndex`.
    pub fn last_index(&self) -> usize {
        self.last_index.load(Ordering::Relaxed)
    }

    /// `re.lastIndex = n`.
    pub fn set_last_index(&self, n: usize) {
        self.last_index.store(n, Ordering::Relaxed);
    }

    /// Leftmost match starting at or after code unit `start`.
    fn find(&self, units: &[u16], start: usize) -> Option<regress::Match> {
        if self.flags.full_unicode() {
            self.regex.find_from_utf16(units, start).next()
        } else {
            self.regex.find_from_ucs2(units, start).next()
        }
    }

    /// RegExpBuiltinExec with an explicit lastIndex.
    fn exec_at(&self, s: &JsString, last_index: &mut usize) -> Option<JsMatch> {
        let units = s.as_units();
        let stateful = self.flags.global || self.flags.sticky;
        let mut start = if stateful { *last_index } else { 0 };
        if start > units.len() {
            if stateful {
                *last_index = 0;
            }
            return None;
        }
        // With u/v a lastIndex inside a surrogate pair reads from the pair's start.
        if self.flags.full_unicode()
            && start > 0
            && start < units.len()
            && is_low(units[start])
            && is_high(units[start - 1])
        {
            start -= 1;
        }
        // A leftmost search from `start` finds the sticky match at `start`
        // whenever one exists, since positions are tried in order.
        let found = self
            .find(units, start)
            .filter(|m| !self.flags.sticky || m.start() == start);
        match found {
            None => {
                if stateful {
                    *last_index = 0;
                }
                None
            }
            Some(m) => {
                if stateful {
                    *last_index = m.end();
                }
                Some(self.build(s, &m))
            }
        }
    }

    fn build(&self, input: &JsString, m: &regress::Match) -> JsMatch {
        let units = input.as_units();
        let slice = |r: &Range<usize>| JsString::from_units(units[r.clone()].to_vec());
        let mut captures = Vec::with_capacity(m.captures.len() + 1);
        captures.push(Some(slice(&m.range)));
        captures.extend(m.captures.iter().map(|c| c.as_ref().map(slice)));
        let names: Vec<(String, Option<Range<usize>>)> = m
            .named_groups()
            .map(|(n, r)| (n.to_string(), r))
            .collect();
        let has_named = !names.is_empty();
        let groups = has_named.then(|| {
            names
                .iter()
                .map(|(n, r)| (n.clone(), r.as_ref().map(slice)))
                .collect()
        });
        let indices = self.flags.has_indices.then(|| {
            let pair = |r: &Range<usize>| (r.start, r.end);
            let mut ranges = Vec::with_capacity(m.captures.len() + 1);
            ranges.push(Some(pair(&m.range)));
            ranges.extend(m.captures.iter().map(|c| c.as_ref().map(pair)));
            JsMatchIndices {
                ranges,
                groups: has_named.then(|| {
                    names
                        .iter()
                        .map(|(n, r)| (n.clone(), r.as_ref().map(pair)))
                        .collect()
                }),
            }
        });
        JsMatch {
            index: m.start(),
            input: input.clone(),
            captures,
            groups,
            indices,
        }
    }

    /// `re.exec(s)`; reads and updates `lastIndex` for `g`/`y`.
    pub fn exec(&self, s: &JsString) -> Option<JsMatch> {
        let mut last = self.last_index();
        let m = self.exec_at(s, &mut last);
        if self.flags.global || self.flags.sticky {
            self.set_last_index(last);
        }
        m
    }

    /// `re.test(s)`; reads and updates `lastIndex` for `g`/`y`.
    pub fn test(&self, s: &JsString) -> bool {
        self.exec(s).is_some()
    }

    /// `s.match(re)` without `g`: the same as `exec`.
    pub fn match_first(&self, s: &JsString) -> Option<JsMatch> {
        self.exec(s)
    }

    /// `[...s.matchAll(re)]`. The iterator runs on a copy of the regex (its
    /// lastIndex starts at `re.lastIndex`), so `re` itself is not changed.
    /// Throws V8's TypeError when `re` is not global.
    pub fn match_all(&self, s: &JsString) -> Result<Vec<JsMatch>, JsError> {
        if !self.flags.global {
            return Err(JsError::type_error(
                "String.prototype.matchAll called with a non-global RegExp argument",
            ));
        }
        let mut last = self.last_index();
        let mut out = Vec::new();
        while let Some(m) = self.exec_at(s, &mut last) {
            if m.matched().is_empty() {
                last = advance(s.as_units(), last, self.flags.full_unicode());
            }
            out.push(m);
        }
        Ok(out)
    }

    /// The match list RegExp.prototype[@@replace] builds.
    fn replace_matches(&self, s: &JsString) -> Vec<JsMatch> {
        if !self.flags.global {
            return self.exec(s).into_iter().collect();
        }
        let mut last = 0;
        let mut out = Vec::new();
        while let Some(m) = self.exec_at(s, &mut last) {
            if m.matched().is_empty() {
                last = advance(s.as_units(), last, self.flags.full_unicode());
            }
            out.push(m);
        }
        self.set_last_index(0);
        out
    }

    fn replace_impl(
        &self,
        s: &JsString,
        mut substitute: impl FnMut(&JsMatch, &mut Vec<u16>),
    ) -> JsString {
        let matches = self.replace_matches(s);
        let units = s.as_units();
        let mut out = Vec::with_capacity(units.len());
        let mut next = 0;
        for m in &matches {
            let position = m.index.min(units.len());
            let mut rep = Vec::new();
            substitute(m, &mut rep);
            if position >= next {
                out.extend_from_slice(&units[next..position]);
                out.extend_from_slice(&rep);
                next = position + m.matched().len();
            }
        }
        if next < units.len() {
            out.extend_from_slice(&units[next..]);
        }
        JsString::from_units(out)
    }

    /// `s.replace(re, replacement)`: every match with `g`, else the first
    /// (from lastIndex when `y`); `$` patterns expanded per GetSubstitution.
    pub fn replace(&self, s: &JsString, replacement: &JsString) -> JsString {
        self.replace_impl(s, |m, out| get_substitution(out, m, replacement.as_units()))
    }

    /// `s.replaceAll(re, replacement)`; V8's TypeError when `re` is not global.
    pub fn replace_all(&self, s: &JsString, replacement: &JsString) -> Result<JsString, JsError> {
        if !self.flags.global {
            return Err(JsError::type_error(
                "String.prototype.replaceAll called with a non-global RegExp argument",
            ));
        }
        Ok(self.replace(s, replacement))
    }

    /// `s.replace(re, (m, ...groups, offset, input, groups) => ...)`; the
    /// callback reads captures, `index` (offset), `input`, and `groups` from
    /// the `JsMatch`. The result is inserted verbatim (no `$` expansion).
    pub fn replace_with(&self, s: &JsString, mut f: impl FnMut(&JsMatch) -> JsString) -> JsString {
        self.replace_impl(s, |m, out| out.extend_from_slice(f(m).as_units()))
    }

    /// `s.split(re, limit)`: captures spliced in (`None` for `undefined`).
    /// Does not read or change `re.lastIndex` (the spec splits with a copy).
    pub fn split(&self, s: &JsString, limit: Option<f64>) -> Vec<Option<JsString>> {
        let lim = limit.map_or(u32::MAX, to_uint32);
        let lim = usize::try_from(lim).unwrap_or(usize::MAX);
        let mut out = Vec::new();
        if lim == 0 {
            return out;
        }
        let units = s.as_units();
        let size = units.len();
        if size == 0 {
            if self.find(units, 0).is_some_and(|m| m.start() == 0) {
                return out;
            }
            out.push(Some(s.clone()));
            return out;
        }
        let unicode = self.flags.full_unicode();
        let piece = |r: Range<usize>| JsString::from_units(units[r].to_vec());
        let mut p = 0;
        let mut q = 0;
        // The spec tries a sticky match at each q; a leftmost search from q
        // lands on the first q that has one, so the loop jumps there directly.
        while q < size {
            let Some(m) = self.find(units, q) else {
                break;
            };
            let start = m.start();
            if start >= size {
                break;
            }
            let e = m.end().min(size);
            if e == p {
                q = advance(units, start, unicode);
                continue;
            }
            out.push(Some(piece(p..start)));
            if out.len() == lim {
                return out;
            }
            p = e;
            for c in &m.captures {
                out.push(c.clone().map(piece));
                if out.len() == lim {
                    return out;
                }
            }
            q = p;
        }
        out.push(Some(piece(p..size)));
        out
    }
}

/// GetSubstitution with captures and named groups, as V8 applies it:
/// `$$ $& $\` $' $n $nn $<name>`; `$nn` falls back to `$n` + digit when there
/// are fewer than nn groups; `$0`, `$00`, and out-of-range numbers stay
/// literal; `$<` stays literal when the pattern has no named groups or no `>`.
fn get_substitution(out: &mut Vec<u16>, m: &JsMatch, replacement: &[u16]) {
    let dollar = u16::from(b'$');
    let s = m.input.as_units();
    let position = m.index;
    let matched = m.matched().as_units();
    let tail = (position + matched.len()).min(s.len());
    let group_count = m.captures.len() - 1;
    let push_group = |out: &mut Vec<u16>, n: usize| {
        if let Some(Some(g)) = m.captures.get(n) {
            out.extend_from_slice(g.as_units());
        }
    };
    let mut i = 0;
    while i < replacement.len() {
        let c = replacement[i];
        if c == dollar && i + 1 < replacement.len() {
            let next = replacement[i + 1];
            if next == dollar {
                out.push(dollar);
                i += 2;
                continue;
            }
            if next == u16::from(b'&') {
                out.extend_from_slice(matched);
                i += 2;
                continue;
            }
            if next == u16::from(b'`') {
                out.extend_from_slice(&s[..position.min(s.len())]);
                i += 2;
                continue;
            }
            if next == u16::from(b'\'') {
                out.extend_from_slice(&s[tail..]);
                i += 2;
                continue;
            }
            if is_digit(next) {
                let d1 = usize::from(next - u16::from(b'0'));
                if i + 2 < replacement.len() && is_digit(replacement[i + 2]) {
                    let two = d1 * 10 + usize::from(replacement[i + 2] - u16::from(b'0'));
                    if (1..=group_count).contains(&two) {
                        push_group(out, two);
                        i += 3;
                        continue;
                    }
                }
                if (1..=group_count).contains(&d1) {
                    push_group(out, d1);
                    i += 2;
                    continue;
                }
            }
            if next == u16::from(b'<') {
                if let Some(groups) = &m.groups {
                    let close = replacement[i + 2..]
                        .iter()
                        .position(|&u| u == u16::from(b'>'))
                        .map(|k| i + 2 + k);
                    if let Some(close) = close {
                        let name = &replacement[i + 2..close];
                        let value = groups
                            .iter()
                            .find(|(n, _)| JsString::from(n.as_str()).as_units() == name)
                            .and_then(|(_, v)| v.as_ref());
                        if let Some(v) = value {
                            out.extend_from_slice(v.as_units());
                        }
                        i = close + 1;
                        continue;
                    }
                }
            }
        }
        out.push(c);
        i += 1;
    }
}
