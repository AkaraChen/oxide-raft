//! raft_shared::js string helpers: `JsString` and the String.prototype operations (decisions.md D3).

use std::fmt;
use std::ops::{Add, AddAssign};

use super::convert::{to_uint32, usize_to_f64};
use super::error::JsError;
use super::number::to_integer_or_infinity;

/// V8's maximum string length on 64-bit targets (`String::kMaxLength`, 2^29 - 24).
pub const MAX_STRING_LENGTH: usize = 536_870_888;

/// A JS string: a sequence of UTF-16 code units, possibly with lone surrogates.
#[derive(Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JsString(Vec<u16>);

impl JsString {
    pub fn new() -> Self {
        JsString(Vec::new())
    }

    pub fn from_units(units: Vec<u16>) -> Self {
        JsString(units)
    }

    pub fn as_units(&self) -> &[u16] {
        &self.0
    }

    /// Length in UTF-16 code units (JS `.length`).
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// UTF-8 text as Node writes it to a stream: lone surrogates become U+FFFD.
    pub fn to_string_lossy(&self) -> String {
        String::from_utf16_lossy(&self.0)
    }

    pub fn eq_str(&self, other: &str) -> bool {
        self.0.iter().copied().eq(other.encode_utf16())
    }

    /// Appends UTF-8 text (template-literal building).
    pub fn push_str(&mut self, s: &str) {
        self.0.extend(s.encode_utf16());
    }

    pub fn push_units(&mut self, units: &[u16]) {
        self.0.extend_from_slice(units);
    }

    pub fn push_js(&mut self, s: &JsString) {
        self.0.extend_from_slice(&s.0);
    }

    /// `a + b + ...` / a template literal: the parts joined with no separator.
    pub fn concat(parts: &[&JsString]) -> JsString {
        let mut out = Vec::with_capacity(parts.iter().map(|p| p.len()).sum());
        for part in parts {
            out.extend_from_slice(&part.0);
        }
        JsString(out)
    }
}

/// Prints like `str`'s Debug, but lone surrogates show as `\u{d800}`.
impl fmt::Debug for JsString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("\"")?;
        for decoded in char::decode_utf16(self.0.iter().copied()) {
            match decoded {
                Ok(c) => {
                    for e in c.escape_debug() {
                        fmt::Write::write_char(f, e)?;
                    }
                }
                Err(e) => write!(f, "\\u{{{:x}}}", e.unpaired_surrogate())?,
            }
        }
        f.write_str("\"")
    }
}

impl Add<&JsString> for JsString {
    type Output = JsString;

    fn add(mut self, rhs: &JsString) -> JsString {
        self.push_js(rhs);
        self
    }
}

impl Add<JsString> for JsString {
    type Output = JsString;

    fn add(mut self, rhs: JsString) -> JsString {
        self.push_js(&rhs);
        self
    }
}

impl Add<&str> for JsString {
    type Output = JsString;

    fn add(mut self, rhs: &str) -> JsString {
        self.push_str(rhs);
        self
    }
}

impl AddAssign<&JsString> for JsString {
    fn add_assign(&mut self, rhs: &JsString) {
        self.push_js(rhs);
    }
}

impl AddAssign<&str> for JsString {
    fn add_assign(&mut self, rhs: &str) {
        self.push_str(rhs);
    }
}

impl Extend<u16> for JsString {
    fn extend<I: IntoIterator<Item = u16>>(&mut self, iter: I) {
        self.0.extend(iter);
    }
}

impl<'a> Extend<&'a JsString> for JsString {
    fn extend<I: IntoIterator<Item = &'a JsString>>(&mut self, iter: I) {
        for s in iter {
            self.push_js(s);
        }
    }
}

impl FromIterator<u16> for JsString {
    fn from_iter<I: IntoIterator<Item = u16>>(iter: I) -> Self {
        JsString(iter.into_iter().collect())
    }
}

/// Concatenation, like `parts.join("")`.
impl FromIterator<JsString> for JsString {
    fn from_iter<I: IntoIterator<Item = JsString>>(iter: I) -> Self {
        let mut out = JsString::new();
        for s in iter {
            out.push_js(&s);
        }
        out
    }
}

impl fmt::Display for JsString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_lossy())
    }
}

impl From<&str> for JsString {
    fn from(s: &str) -> Self {
        JsString(s.encode_utf16().collect())
    }
}

impl From<String> for JsString {
    fn from(s: String) -> Self {
        JsString::from(s.as_str())
    }
}

impl From<&String> for JsString {
    fn from(s: &String) -> Self {
        JsString::from(s.as_str())
    }
}

impl From<&JsString> for JsString {
    fn from(s: &JsString) -> Self {
        s.clone()
    }
}

impl PartialEq<str> for JsString {
    fn eq(&self, other: &str) -> bool {
        self.eq_str(other)
    }
}

impl PartialEq<&str> for JsString {
    fn eq(&self, other: &&str) -> bool {
        self.eq_str(other)
    }
}

/// JS WhiteSpace plus LineTerminator (the `\s` class, `trim`, `Number()`, and
/// `parseInt`). U+0085 is not in it (Node 24.15.0: `"\u0085x".trim().length === 2`).
pub(crate) fn is_js_whitespace(u: u16) -> bool {
    matches!(
        u,
        0x0009 | 0x000A | 0x000B | 0x000C | 0x000D | 0x0020 | 0x00A0 | 0x1680 | 0x2000
            ..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000 | 0xFEFF
    )
}

pub fn utf16_len(s: &JsString) -> usize {
    s.len()
}

/// Resolves a relative index the way String.prototype.slice does.
fn relative_index(value: f64, len: usize) -> usize {
    let n = to_integer_or_infinity(value);
    let len_f = usize_to_f64(len);
    let resolved = if n < 0.0 {
        (len_f + n).max(0.0)
    } else {
        n.min(len_f)
    };
    // An integer in [0, len] after clamping, so the conversion is exact.
    super::convert::f64_to_usize_exact(resolved).unwrap_or(0)
}

/// String.prototype.slice(start, end).
pub fn utf16_slice(s: &JsString, start: f64, end: Option<f64>) -> JsString {
    let len = s.len();
    let from = relative_index(start, len);
    let to = match end {
        Some(e) => relative_index(e, len),
        None => len,
    };
    if from >= to {
        return JsString::new();
    }
    JsString(s.0[from..to].to_vec())
}

pub fn trim(s: &JsString) -> JsString {
    let units = &s.0;
    let start = units
        .iter()
        .position(|&u| !is_js_whitespace(u))
        .unwrap_or(units.len());
    let end = units
        .iter()
        .rposition(|&u| !is_js_whitespace(u))
        .map_or(start, |i| i + 1);
    JsString(units[start..end.max(start)].to_vec())
}

pub fn trim_start(s: &JsString) -> JsString {
    let units = &s.0;
    let start = units
        .iter()
        .position(|&u| !is_js_whitespace(u))
        .unwrap_or(units.len());
    JsString(units[start..].to_vec())
}

pub fn trim_end(s: &JsString) -> JsString {
    let units = &s.0;
    let end = units
        .iter()
        .rposition(|&u| !is_js_whitespace(u))
        .map_or(0, |i| i + 1);
    JsString(units[..end].to_vec())
}

/// ToLength of the requested length, saturated at `usize::MAX`.
fn to_length(value: f64) -> usize {
    let n = to_integer_or_infinity(value);
    if n <= 0.0 {
        return 0;
    }
    // ToLength caps at 2^53-1; anything that large is over MAX_STRING_LENGTH anyway.
    super::convert::f64_to_usize_exact(n.min(9_007_199_254_740_991.0)).unwrap_or(usize::MAX)
}

/// The filler for padStart/padEnd, or None when the string is returned unchanged.
fn fill_string(
    s: &JsString,
    max_length: f64,
    fill: &JsString,
) -> Result<Option<Vec<u16>>, JsError> {
    let int_max = to_length(max_length);
    let len = s.len();
    if int_max <= len || fill.is_empty() {
        return Ok(None);
    }
    if int_max > MAX_STRING_LENGTH {
        return Err(JsError::range("Invalid string length"));
    }
    let fill_len = int_max - len;
    let mut out = Vec::with_capacity(fill_len);
    while out.len() < fill_len {
        let remaining = fill_len - out.len();
        let take = remaining.min(fill.len());
        out.extend_from_slice(&fill.0[..take]);
    }
    Ok(Some(out))
}

/// String.prototype.padStart(maxLength, fillString); RangeError past V8's
/// maximum string length.
pub fn pad_start(s: &JsString, max_length: f64, fill: &JsString) -> Result<JsString, JsError> {
    Ok(match fill_string(s, max_length, fill)? {
        None => s.clone(),
        Some(mut out) => {
            out.extend_from_slice(&s.0);
            JsString(out)
        }
    })
}

/// String.prototype.padEnd(maxLength, fillString); RangeError past V8's
/// maximum string length.
pub fn pad_end(s: &JsString, max_length: f64, fill: &JsString) -> Result<JsString, JsError> {
    Ok(match fill_string(s, max_length, fill)? {
        None => s.clone(),
        Some(out) => {
            let mut result = s.0.clone();
            result.extend_from_slice(&out);
            JsString(result)
        }
    })
}

/// StringIndexOf(s, search, from_index).
pub(crate) fn index_of(s: &[u16], search: &[u16], from: usize) -> Option<usize> {
    let len = s.len();
    if search.is_empty() {
        return if from <= len { Some(from) } else { None };
    }
    if search.len() > len {
        return None;
    }
    (from..=len - search.len()).find(|&i| &s[i..i + search.len()] == search)
}

/// String.prototype.split(separator, limit) with a string separator.
pub fn split(s: &JsString, separator: &JsString, limit: Option<f64>) -> Vec<JsString> {
    let lim = match limit {
        None => u32::MAX,
        Some(l) => to_uint32(l),
    };
    // u32 fits in usize on every supported (32/64-bit) target.
    let lim = usize::try_from(lim).unwrap_or(usize::MAX);
    let mut out = Vec::new();
    if lim == 0 {
        return out;
    }
    let units = &s.0;
    let sep = &separator.0;
    if sep.is_empty() {
        return units.iter().take(lim).map(|&u| JsString(vec![u])).collect();
    }
    if units.is_empty() {
        out.push(JsString::new());
        return out;
    }
    let mut i = 0;
    let mut j = index_of(units, sep, 0);
    while let Some(pos) = j {
        out.push(JsString(units[i..pos].to_vec()));
        if out.len() == lim {
            return out;
        }
        i = pos + sep.len();
        j = index_of(units, sep, i);
    }
    out.push(JsString(units[i..].to_vec()));
    out
}

/// GetSubstitution for a string pattern (no captures, no named groups).
fn get_substitution(
    out: &mut Vec<u16>,
    s: &[u16],
    position: usize,
    matched_len: usize,
    replacement: &[u16],
) {
    let dollar = u16::from(b'$');
    let tail_pos = (position + matched_len).min(s.len());
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
                out.extend_from_slice(&s[position..position + matched_len]);
                i += 2;
                continue;
            }
            if next == u16::from(b'`') {
                out.extend_from_slice(&s[..position]);
                i += 2;
                continue;
            }
            if next == u16::from(b'\'') {
                out.extend_from_slice(&s[tail_pos..]);
                i += 2;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
}

/// String.prototype.replace(pattern, replacement) with a string pattern.
pub fn string_replace_first(s: &JsString, pattern: &JsString, replacement: &JsString) -> JsString {
    let units = &s.0;
    let Some(pos) = index_of(units, &pattern.0, 0) else {
        return s.clone();
    };
    let mut out = Vec::with_capacity(units.len());
    out.extend_from_slice(&units[..pos]);
    get_substitution(&mut out, units, pos, pattern.len(), &replacement.0);
    out.extend_from_slice(&units[pos + pattern.len()..]);
    JsString(out)
}

/// String.prototype.replaceAll(pattern, replacement) with a string pattern.
pub fn string_replace_all(s: &JsString, pattern: &JsString, replacement: &JsString) -> JsString {
    let units = &s.0;
    let search_len = pattern.len();
    let advance = search_len.max(1);
    let mut positions = Vec::new();
    let mut next = index_of(units, &pattern.0, 0);
    while let Some(p) = next {
        positions.push(p);
        next = index_of(units, &pattern.0, p + advance);
    }
    let mut end_of_last = 0;
    let mut out = Vec::with_capacity(units.len());
    for p in positions {
        out.extend_from_slice(&units[end_of_last..p]);
        get_substitution(&mut out, units, p, search_len, &replacement.0);
        end_of_last = p + search_len;
    }
    if end_of_last < units.len() {
        out.extend_from_slice(&units[end_of_last..]);
    }
    JsString(out)
}

/// encodeURIComponent.
pub fn encode_uri_component(s: &JsString) -> Result<String, JsError> {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(s.len());
    for decoded in char::decode_utf16(s.0.iter().copied()) {
        let Ok(c) = decoded else {
            return Err(JsError::uri("URI malformed"));
        };
        let unreserved = c.is_ascii_alphanumeric()
            || matches!(c, '-' | '_' | '.' | '!' | '~' | '*' | '\'' | '(' | ')');
        if unreserved {
            out.push(c);
        } else {
            let mut buf = [0u8; 4];
            for &b in c.encode_utf8(&mut buf).as_bytes() {
                out.push('%');
                out.push(char::from(HEX[usize::from(b >> 4)]));
                out.push(char::from(HEX[usize::from(b & 0x0F)]));
            }
        }
    }
    Ok(out)
}

/// `s.replace(/\s+/g, " ")`.
pub fn collapse_js_whitespace(s: &JsString) -> JsString {
    let mut out = Vec::with_capacity(s.len());
    let mut in_run = false;
    for &u in &s.0 {
        if is_js_whitespace(u) {
            if !in_run {
                out.push(u16::from(b' '));
                in_run = true;
            }
        } else {
            out.push(u);
            in_run = false;
        }
    }
    JsString(out)
}

/// Array.prototype.sort() without a comparator: stable, UTF-16 code-unit order.
pub fn sort_default(items: &mut [JsString]) {
    items.sort();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u0085_is_not_whitespace() {
        let s = JsString::from("\u{85}a\u{85}");
        assert_eq!(trim(&s), s);
        assert_eq!(trim_start(&s), s);
        assert_eq!(trim_end(&s), s);
        assert_eq!(collapse_js_whitespace(&s), s);
    }

    #[test]
    fn pad_past_max_string_length_is_range_error() {
        let a = JsString::from("a");
        let space = JsString::from(" ");
        let err = pad_start(&a, 1e15, &space).expect_err("too long");
        assert_eq!(
            (err.name.as_str(), err.message.as_str()),
            ("RangeError", "Invalid string length")
        );
        let too_long = usize_to_f64(MAX_STRING_LENGTH + 1);
        assert!(pad_end(&a, too_long, &space).is_err());
        assert_eq!(
            pad_start(&a, 1e15, &JsString::new()).expect("empty fill"),
            a
        );
        assert_eq!(
            pad_start(&a, 3.0, &space).expect("pads"),
            JsString::from("  a")
        );
    }

    #[test]
    fn building_and_debug() {
        let a = JsString::from("a");
        let lone = JsString::from_units(vec![0xD800]);
        let built = a.clone() + ":" + &lone + JsString::from("b");
        assert_eq!(built.as_units(), &[0x61, 0x3A, 0xD800, 0x62]);
        assert_eq!(JsString::concat(&[&a, &lone]).as_units(), &[0x61, 0xD800]);
        let joined: JsString = vec![a.clone(), a.clone()].into_iter().collect();
        assert_eq!(joined, JsString::from("aa"));
        assert_eq!(format!("{built:?}"), "\"a:\\u{d800}b\"");
    }
}
