//! raft_shared::js JSON: V8-compatible JSON.stringify, JSON.parse (with V8 error text), and res.text() (decisions.md D3).

use super::error::JsError;
use super::number::number_to_string;
use super::object::Object;
use super::string::JsString;
use super::value::Value;

// ---------------------------------------------------------------------------
// JSON.stringify

fn write_json_string(out: &mut String, units: &[u16]) {
    out.push('"');
    let mut i = 0;
    while i < units.len() {
        let u = units[i];
        match u {
            0x22 => out.push_str("\\\""),
            0x5C => out.push_str("\\\\"),
            0x08 => out.push_str("\\b"),
            0x0C => out.push_str("\\f"),
            0x0A => out.push_str("\\n"),
            0x0D => out.push_str("\\r"),
            0x09 => out.push_str("\\t"),
            0x00..=0x1F => out.push_str(&format!("\\u{u:04x}")),
            0xD800..=0xDBFF => {
                let next = units.get(i + 1).copied();
                match next {
                    Some(lo @ 0xDC00..=0xDFFF) => {
                        let cp =
                            0x10000 + ((u32::from(u) - 0xD800) << 10) + (u32::from(lo) - 0xDC00);
                        out.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                        i += 1;
                    }
                    _ => out.push_str(&format!("\\u{u:04x}")),
                }
            }
            0xDC00..=0xDFFF => out.push_str(&format!("\\u{u:04x}")),
            _ => out.push(char::from_u32(u32::from(u)).unwrap_or('\u{FFFD}')),
        }
        i += 1;
    }
    out.push('"');
}

/// A container being written: its remaining members and whether any was written yet.
enum WriteFrame<'a> {
    Array(std::slice::Iter<'a, Value>, bool),
    Object(
        Box<dyn Iterator<Item = (&'a JsString, &'a Value)> + 'a>,
        bool,
    ),
}

fn write_scalar(out: &mut String, v: &Value) {
    match v {
        Value::Undefined | Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if n.is_finite() {
                out.push_str(&number_to_string(*n));
            } else {
                out.push_str("null");
            }
        }
        Value::String(s) => write_json_string(out, s.as_units()),
        Value::Array(_) | Value::Object(_) => {}
    }
}

fn write_newline(out: &mut String, gap: &str, depth: usize) {
    if !gap.is_empty() {
        out.push('\n');
        for _ in 0..depth {
            out.push_str(gap);
        }
    }
}

/// SerializeJSONProperty over an explicit stack, so any nesting depth works.
/// (V8 recurses and throws `RangeError: Maximum call stack size exceeded`
/// beyond a few thousand levels; this writes what V8 would with an unbounded
/// stack.)
fn write_json_value(out: &mut String, root: &Value, gap: &str) {
    let mut stack: Vec<WriteFrame<'_>> = Vec::new();
    let mut pending = Some(root);
    loop {
        if let Some(v) = pending.take() {
            match v {
                Value::Array(items) => {
                    out.push('[');
                    stack.push(WriteFrame::Array(items.iter(), false));
                }
                Value::Object(object) => {
                    out.push('{');
                    let members = object
                        .iter()
                        .filter(|(_, member)| !matches!(member, Value::Undefined));
                    stack.push(WriteFrame::Object(Box::new(members), false));
                }
                scalar => write_scalar(out, scalar),
            }
        }
        let depth = stack.len();
        let Some(frame) = stack.last_mut() else {
            return;
        };
        match frame {
            WriteFrame::Array(items, started) => match items.next() {
                Some(item) => {
                    if *started {
                        out.push(',');
                    }
                    *started = true;
                    write_newline(out, gap, depth);
                    pending = Some(item);
                }
                None => {
                    if *started {
                        write_newline(out, gap, depth - 1);
                    }
                    out.push(']');
                    stack.pop();
                }
            },
            WriteFrame::Object(members, started) => match members.next() {
                Some((key, item)) => {
                    if *started {
                        out.push(',');
                    }
                    *started = true;
                    write_newline(out, gap, depth);
                    write_json_string(out, key.as_units());
                    out.push(':');
                    if !gap.is_empty() {
                        out.push(' ');
                    }
                    pending = Some(item);
                }
                None => {
                    if *started {
                        write_newline(out, gap, depth - 1);
                    }
                    out.push('}');
                    stack.pop();
                }
            },
        }
    }
}

/// `JSON.stringify(v)`; None when `v` is undefined.
pub fn json_stringify(v: &Value) -> Option<String> {
    json_stringify_pretty(v, 0)
}

/// `JSON.stringify(v, null, indent)`.
pub fn json_stringify_pretty(v: &Value, indent: usize) -> Option<String> {
    if matches!(v, Value::Undefined) {
        return None;
    }
    let gap = " ".repeat(indent.min(10));
    let mut out = String::new();
    write_json_value(&mut out, v, &gap);
    Some(out)
}

// ---------------------------------------------------------------------------
// JSON.parse

/// V8 shows at most this many code units on each side of the error.
const MAX_CONTEXT_CHARACTERS: usize = 10;
/// Sources up to this length are quoted whole.
const MIN_ORIGINAL_SOURCE_LENGTH_FOR_CONTEXT: usize = MAX_CONTEXT_CHARACTERS * 2;

enum Frame {
    Array(Vec<Value>),
    Object(Object, JsString),
}

struct Parser<'a> {
    src: &'a [u16],
    pos: usize,
}

fn is_json_ws(u: u16) -> bool {
    matches!(u, 0x20 | 0x09 | 0x0A | 0x0D)
}

fn is_digit(u: u16) -> bool {
    (u16::from(b'0')..=u16::from(b'9')).contains(&u)
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<u16> {
        self.src.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(is_json_ws) {
            self.pos += 1;
        }
    }

    /// "<message> in JSON at position N (line L column C)".
    fn positioned(&self, message: &str, pos: usize) -> JsError {
        let mut line = 1usize;
        let mut line_start = 0usize;
        let mut i = 0usize;
        while i < pos && i < self.src.len() {
            let u = self.src[i];
            if u == 0x0A {
                line += 1;
                line_start = i + 1;
            } else if u == 0x0D {
                if self.src.get(i + 1) == Some(&0x0A) && i + 1 < pos {
                    i += 1;
                }
                line += 1;
                line_start = i + 1;
            }
            i += 1;
        }
        let column = pos - line_start + 1;
        // V8 omits " in JSON" when the message already ends with "JSON".
        let context = if message.ends_with("JSON") {
            ""
        } else {
            " in JSON"
        };
        JsError::syntax(format!(
            "{message}{context} at position {pos} (line {line} column {column})"
        ))
    }

    fn unexpected_end(&self) -> JsError {
        JsError::syntax("Unexpected end of JSON input")
    }

    /// V8's generic "unexpected token" report for the code unit at `pos`.
    fn unexpected_token(&self, pos: usize) -> JsError {
        let Some(&token) = self.src.get(pos) else {
            return self.unexpected_end();
        };
        let source = JsString::from_units(self.src.to_vec()).to_string_lossy();
        if matches!(
            source.as_str(),
            "[object Object]" | "undefined" | "Infinity" | "NaN"
        ) {
            return JsError::syntax(format!("\"{source}\" is not valid JSON"));
        }
        let token = String::from_utf16_lossy(&[token]);
        let len = self.src.len();
        if len <= MIN_ORIGINAL_SOURCE_LENGTH_FOR_CONTEXT {
            return JsError::syntax(format!(
                "Unexpected token '{token}', \"{source}\" is not valid JSON"
            ));
        }
        let snippet = |start: usize, end: usize| String::from_utf16_lossy(&self.src[start..end]);
        let message = if pos < MAX_CONTEXT_CHARACTERS {
            format!(
                "Unexpected token '{token}', \"{}\"... is not valid JSON",
                snippet(0, pos + MAX_CONTEXT_CHARACTERS)
            )
        } else if pos < len - MAX_CONTEXT_CHARACTERS {
            format!(
                "Unexpected token '{token}', ...\"{}\"... is not valid JSON",
                snippet(pos - MAX_CONTEXT_CHARACTERS, pos + MAX_CONTEXT_CHARACTERS)
            )
        } else {
            format!(
                "Unexpected token '{token}', ...\"{}\" is not valid JSON",
                snippet(pos - MAX_CONTEXT_CHARACTERS, len)
            )
        };
        JsError::syntax(message)
    }

    fn expect_literal(&mut self, literal: &str) -> Result<(), JsError> {
        for b in literal.bytes() {
            match self.peek() {
                None => return Err(self.unexpected_end()),
                Some(u) if u == u16::from(b) => self.pos += 1,
                Some(_) => return Err(self.unexpected_token(self.pos)),
            }
        }
        Ok(())
    }

    /// Parses a string starting at the opening quote.
    fn parse_string(&mut self) -> Result<JsString, JsError> {
        self.pos += 1;
        let mut out: Vec<u16> = Vec::new();
        loop {
            let Some(u) = self.peek() else {
                return Err(self.positioned("Unterminated string", self.pos));
            };
            match u {
                0x22 => {
                    self.pos += 1;
                    return Ok(JsString::from_units(out));
                }
                0x5C => {
                    self.pos += 1;
                    let Some(e) = self.peek() else {
                        return Err(self.unexpected_end());
                    };
                    let decoded = match e {
                        0x22 => 0x22,
                        0x5C => 0x5C,
                        0x2F => 0x2F,
                        0x62 => 0x08,
                        0x66 => 0x0C,
                        0x6E => 0x0A,
                        0x72 => 0x0D,
                        0x74 => 0x09,
                        0x75 => {
                            let mut value: u16 = 0;
                            for k in 1..=4 {
                                let at = self.pos + k;
                                let digit = self
                                    .src
                                    .get(at)
                                    .and_then(|&h| char::from_u32(u32::from(h)))
                                    .and_then(|c| c.to_digit(16));
                                match digit {
                                    // review: a hex digit is < 16, so it fits in u16.
                                    Some(d) => value = value * 16 + u16::try_from(d).unwrap_or(0),
                                    None => return Err(self.positioned("Bad Unicode escape", at)),
                                }
                            }
                            self.pos += 4;
                            value
                        }
                        _ => return Err(self.positioned("Bad escaped character", self.pos)),
                    };
                    out.push(decoded);
                    self.pos += 1;
                }
                0x00..=0x1F => {
                    return Err(
                        self.positioned("Bad control character in string literal", self.pos)
                    );
                }
                _ => {
                    out.push(u);
                    self.pos += 1;
                }
            }
        }
    }

    fn parse_number(&mut self) -> Result<f64, JsError> {
        let start = self.pos;
        if self.peek() == Some(u16::from(b'-')) {
            self.pos += 1;
            if !self.peek().is_some_and(is_digit) {
                return Err(self.positioned("No number after minus sign", self.pos));
            }
        }
        if self.peek() == Some(u16::from(b'0')) {
            self.pos += 1;
            if self.peek().is_some_and(is_digit) {
                return Err(self.positioned("Unexpected number", self.pos));
            }
        } else {
            while self.peek().is_some_and(is_digit) {
                self.pos += 1;
            }
        }
        if self.peek() == Some(u16::from(b'.')) {
            self.pos += 1;
            if !self.peek().is_some_and(is_digit) {
                return Err(self.positioned("Unterminated fractional number", self.pos));
            }
            while self.peek().is_some_and(is_digit) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some(0x65 | 0x45)) {
            self.pos += 1;
            if matches!(self.peek(), Some(0x2B | 0x2D)) {
                self.pos += 1;
            }
            if !self.peek().is_some_and(is_digit) {
                return Err(self.positioned("Exponent part is missing a number", self.pos));
            }
            while self.peek().is_some_and(is_digit) {
                self.pos += 1;
            }
        }
        let text: String = self.src[start..self.pos]
            .iter()
            .filter_map(|&u| char::from_u32(u32::from(u)))
            .collect();
        // The grammar above is a subset of what Rust's correctly rounded parser accepts.
        Ok(text.parse::<f64>().unwrap_or(f64::NAN))
    }

    /// Reads `"key"` and the following `:`; the cursor is at the opening quote.
    fn parse_property_name(&mut self) -> Result<JsString, JsError> {
        let key = self.parse_string()?;
        self.skip_ws();
        if self.peek() != Some(u16::from(b':')) {
            return Err(self.positioned("Expected ':' after property name", self.pos));
        }
        self.pos += 1;
        Ok(key)
    }

    fn parse(&mut self) -> Result<Value, JsError> {
        let mut stack: Vec<Frame> = Vec::new();
        loop {
            // Parse one value, or open a container and go parse its first element.
            self.skip_ws();
            let mut value = match self.peek() {
                None => return Err(self.unexpected_end()),
                Some(0x7B) => {
                    self.pos += 1;
                    self.skip_ws();
                    match self.peek() {
                        Some(0x7D) => {
                            self.pos += 1;
                            Value::Object(Object::new())
                        }
                        Some(0x22) => {
                            let key = self.parse_property_name()?;
                            stack.push(Frame::Object(Object::new(), key));
                            continue;
                        }
                        _ => return Err(self.positioned("Expected property name or '}'", self.pos)),
                    }
                }
                Some(0x5B) => {
                    self.pos += 1;
                    self.skip_ws();
                    if self.peek() == Some(0x5D) {
                        self.pos += 1;
                        Value::Array(Vec::new())
                    } else {
                        stack.push(Frame::Array(Vec::new()));
                        continue;
                    }
                }
                Some(0x22) => Value::String(self.parse_string()?),
                Some(u) if u == u16::from(b'-') || is_digit(u) => {
                    Value::Number(self.parse_number()?)
                }
                Some(0x74) => {
                    self.expect_literal("true")?;
                    Value::Bool(true)
                }
                Some(0x66) => {
                    self.expect_literal("false")?;
                    Value::Bool(false)
                }
                Some(0x6E) => {
                    self.expect_literal("null")?;
                    Value::Null
                }
                Some(_) => return Err(self.unexpected_token(self.pos)),
            };
            // Attach the finished value to its parents, closing containers as they end.
            loop {
                match stack.last_mut() {
                    None => {
                        self.skip_ws();
                        if self.pos < self.src.len() {
                            return Err(self.positioned(
                                "Unexpected non-whitespace character after JSON",
                                self.pos,
                            ));
                        }
                        return Ok(value);
                    }
                    Some(Frame::Array(items)) => {
                        items.push(value);
                        self.skip_ws();
                        match self.peek() {
                            Some(0x2C) => {
                                self.pos += 1;
                                break;
                            }
                            Some(0x5D) => {
                                self.pos += 1;
                                let Some(Frame::Array(items)) = stack.pop() else {
                                    unreachable!("the top frame was just matched as an array");
                                };
                                value = Value::Array(items);
                            }
                            _ => {
                                return Err(self.positioned(
                                    "Expected ',' or ']' after array element",
                                    self.pos,
                                ));
                            }
                        }
                    }
                    Some(Frame::Object(object, key)) => {
                        object.insert(std::mem::take(key), value);
                        self.skip_ws();
                        match self.peek() {
                            Some(0x2C) => {
                                self.pos += 1;
                                self.skip_ws();
                                if self.peek() != Some(0x22) {
                                    return Err(self.positioned(
                                        "Expected double-quoted property name",
                                        self.pos,
                                    ));
                                }
                                let next_key = self.parse_property_name()?;
                                if let Some(Frame::Object(_, key)) = stack.last_mut() {
                                    *key = next_key;
                                }
                                break;
                            }
                            Some(0x7D) => {
                                self.pos += 1;
                                let Some(Frame::Object(object, _)) = stack.pop() else {
                                    unreachable!("the top frame was just matched as an object");
                                };
                                value = Value::Object(object);
                            }
                            _ => {
                                return Err(self.positioned(
                                    "Expected ',' or '}' after property value",
                                    self.pos,
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
}

/// `JSON.parse(text)`.
pub fn json_parse(text: &str) -> Result<Value, JsError> {
    json_parse_js(&JsString::from(text))
}

/// `JSON.parse(text)` for text that may hold lone surrogates.
///
/// Error messages are `String`s, so a lone surrogate quoted in a message
/// becomes U+FFFD (V8 keeps it); the error kind and positions are unchanged.
pub fn json_parse_js(text: &JsString) -> Result<Value, JsError> {
    let mut parser = Parser {
        src: text.as_units(),
        pos: 0,
    };
    parser.parse()
}

// ---------------------------------------------------------------------------
// res.text()

/// `res.text()`: UTF-8 decode with WHATWG replacement, one leading BOM stripped.
pub fn response_text(bytes: &[u8]) -> String {
    // Rust replaces each maximal subpart of an invalid sequence with one U+FFFD,
    // which is the WHATWG TextDecoder behavior.
    let decoded = String::from_utf8_lossy(bytes);
    match decoded.strip_prefix('\u{FEFF}') {
        Some(rest) => rest.to_string(),
        None => decoded.into_owned(),
    }
}
