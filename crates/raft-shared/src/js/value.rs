//! raft_shared::js value model: `Value`, truthiness, String(x), Number(x), and `<` (decisions.md D3).
//!
//! `Value` trees can be as deep as `JSON.parse` allows (V8 has no depth limit),
//! so drop, clone, equality, and `String(x)` walk them with explicit stacks
//! instead of recursion.

use std::fmt;

use super::number::{number_to_string, to_number};
use super::object::Object;
use super::string::JsString;

/// A JS value as it appears in JSON-shaped data.
///
/// `Value` implements `Drop`, so its contents cannot be moved out by pattern
/// matching; use `into_array`, `into_object`, `into_js_string`, or
/// `std::mem::take` instead.
#[derive(Default)]
pub enum Value {
    #[default]
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(JsString),
    Array(Vec<Value>),
    Object(Object),
}

impl Value {
    fn has_children(&self) -> bool {
        match self {
            Value::Array(items) => !items.is_empty(),
            Value::Object(object) => !object.is_empty(),
            _ => false,
        }
    }
}

/// Moves every non-empty container child of `v` onto `stack`.
fn take_nested(v: &mut Value, stack: &mut Vec<Value>) {
    match v {
        Value::Array(items) => {
            for item in items.iter_mut() {
                if item.has_children() {
                    stack.push(std::mem::take(item));
                }
            }
        }
        Value::Object(object) => {
            for (_, item) in object.iter_mut() {
                if item.has_children() {
                    stack.push(std::mem::take(item));
                }
            }
        }
        _ => {}
    }
}

impl Drop for Value {
    fn drop(&mut self) {
        if !self.has_children() {
            return;
        }
        let mut stack = Vec::new();
        take_nested(self, &mut stack);
        while let Some(mut v) = stack.pop() {
            take_nested(&mut v, &mut stack);
            // `v` now holds no nested containers, so dropping it does not recurse.
        }
    }
}

impl Clone for Value {
    fn clone(&self) -> Self {
        enum Task<'a> {
            Visit(&'a Value),
            Array(usize),
            Object(&'a Object),
        }
        let mut tasks = vec![Task::Visit(self)];
        let mut done: Vec<Value> = Vec::new();
        while let Some(task) = tasks.pop() {
            match task {
                Task::Visit(v) => match v {
                    Value::Undefined => done.push(Value::Undefined),
                    Value::Null => done.push(Value::Null),
                    Value::Bool(b) => done.push(Value::Bool(*b)),
                    Value::Number(n) => done.push(Value::Number(*n)),
                    Value::String(s) => done.push(Value::String(s.clone())),
                    Value::Array(items) => {
                        tasks.push(Task::Array(items.len()));
                        tasks.extend(items.iter().rev().map(Task::Visit));
                    }
                    Value::Object(object) => {
                        tasks.push(Task::Object(object));
                        tasks.extend(object.values().rev().map(Task::Visit));
                    }
                },
                Task::Array(len) => {
                    let items = done.split_off(done.len() - len);
                    done.push(Value::Array(items));
                }
                Task::Object(source) => {
                    let values = done.split_off(done.len() - source.len());
                    let mut object = Object::new();
                    for (k, v) in source.keys().zip(values) {
                        object.insert(k.clone(), v);
                    }
                    done.push(Value::Object(object));
                }
            }
        }
        done.pop().unwrap_or_default()
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        let mut stack = vec![(self, other)];
        while let Some(pair) = stack.pop() {
            let equal = match pair {
                (Value::Undefined, Value::Undefined) | (Value::Null, Value::Null) => true,
                (Value::Bool(a), Value::Bool(b)) => a == b,
                (Value::Number(a), Value::Number(b)) => a == b || (a.is_nan() && b.is_nan()),
                (Value::String(a), Value::String(b)) => a == b,
                (Value::Array(a), Value::Array(b)) => {
                    stack.extend(a.iter().zip(b.iter()));
                    a.len() == b.len()
                }
                (Value::Object(a), Value::Object(b)) => {
                    let mut same_keys = a.len() == b.len();
                    for ((ka, va), (kb, vb)) in a.iter().zip(b.iter()) {
                        same_keys &= ka == kb;
                        stack.push((va, vb));
                    }
                    same_keys
                }
                _ => false,
            };
            if !equal {
                return false;
            }
        }
        true
    }
}

/// Debug output stops descending after this many levels.
const DEBUG_MAX_DEPTH: usize = 64;

struct DebugAt<'a>(&'a Value, usize);

impl fmt::Debug for DebugAt<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (v, depth) = (self.0, self.1);
        match v {
            Value::Undefined => f.write_str("Undefined"),
            Value::Null => f.write_str("Null"),
            Value::Bool(b) => write!(f, "Bool({b})"),
            Value::Number(n) => write!(f, "Number({n:?})"),
            Value::String(s) => write!(f, "String({s:?})"),
            Value::Array(_) | Value::Object(_) if depth >= DEBUG_MAX_DEPTH => f.write_str("…"),
            Value::Array(items) => f
                .debug_list()
                .entries(items.iter().map(|item| DebugAt(item, depth + 1)))
                .finish(),
            Value::Object(object) => f
                .debug_map()
                .entries(object.iter().map(|(k, item)| (k, DebugAt(item, depth + 1))))
                .finish(),
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        DebugAt(self, 0).fmt(f)
    }
}

impl Value {
    pub fn string(s: impl Into<JsString>) -> Self {
        Value::String(s.into())
    }

    pub fn object<K: Into<JsString>>(pairs: impl IntoIterator<Item = (K, Value)>) -> Self {
        Value::Object(Object::from_pairs(pairs))
    }

    pub fn as_object(&self) -> Option<&Object> {
        match self {
            Value::Object(o) => Some(o),
            _ => None,
        }
    }

    pub fn as_object_mut(&mut self) -> Option<&mut Object> {
        match self {
            Value::Object(o) => Some(o),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&Vec<Value>> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_array_mut(&mut self) -> Option<&mut Vec<Value>> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_js_string(&self) -> Option<&JsString> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// Takes the array out of an `Array` value.
    pub fn into_array(mut self) -> Option<Vec<Value>> {
        match &mut self {
            Value::Array(a) => Some(std::mem::take(a)),
            _ => None,
        }
    }

    /// Takes the object out of an `Object` value.
    pub fn into_object(mut self) -> Option<Object> {
        match &mut self {
            Value::Object(o) => Some(std::mem::take(o)),
            _ => None,
        }
    }

    /// Takes the string out of a `String` value.
    pub fn into_js_string(mut self) -> Option<JsString> {
        match &mut self {
            Value::String(s) => Some(std::mem::take(s)),
            _ => None,
        }
    }

    pub fn is_string_eq(&self, s: &str) -> bool {
        matches!(self, Value::String(js) if js.eq_str(s))
    }

    /// Object member access; None for non-objects and missing keys.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_object().and_then(|o| o.get(key))
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}

impl From<f64> for Value {
    fn from(n: f64) -> Self {
        Value::Number(n)
    }
}

impl From<i32> for Value {
    fn from(n: i32) -> Self {
        Value::Number(f64::from(n))
    }
}

impl From<u32> for Value {
    fn from(n: u32) -> Self {
        Value::Number(f64::from(n))
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::String(JsString::from(s))
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::String(JsString::from(s))
    }
}

impl From<JsString> for Value {
    fn from(s: JsString) -> Self {
        Value::String(s)
    }
}

impl From<Vec<Value>> for Value {
    fn from(items: Vec<Value>) -> Self {
        Value::Array(items)
    }
}

impl From<Object> for Value {
    fn from(object: Object) -> Self {
        Value::Object(object)
    }
}

/// `None` becomes `undefined`.
impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(v: Option<T>) -> Self {
        v.map_or(Value::Undefined, Into::into)
    }
}

impl FromIterator<Value> for Value {
    fn from_iter<I: IntoIterator<Item = Value>>(iter: I) -> Self {
        Value::Array(iter.into_iter().collect())
    }
}

/// `x == null` (the `??` test).
pub fn is_nullish(v: &Value) -> bool {
    matches!(v, Value::Undefined | Value::Null)
}

/// ToBoolean.
pub fn is_truthy(v: &Value) -> bool {
    match v {
        Value::Undefined | Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => *n != 0.0 && !n.is_nan(),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// `String(x)` / template-literal interpolation.
///
/// Iterative, so nesting depth is unbounded. (V8 throws `RangeError: Maximum
/// call stack size exceeded` for arrays nested a few thousand deep; this
/// returns the string V8 would build with an unbounded stack.)
pub fn to_display_string(v: &Value) -> JsString {
    enum Task<'a> {
        Value(&'a Value),
        Comma,
    }
    let mut out = JsString::new();
    let mut tasks = vec![Task::Value(v)];
    while let Some(task) = tasks.pop() {
        match task {
            Task::Comma => out.push_str(","),
            Task::Value(v) => match v {
                Value::Undefined => out.push_str("undefined"),
                Value::Null => out.push_str("null"),
                Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
                Value::Number(n) => out.push_str(&number_to_string(*n)),
                Value::String(s) => out.push_js(s),
                Value::Array(items) => {
                    // Array.prototype.join: nullish elements become "".
                    for (i, item) in items.iter().enumerate().rev() {
                        if !is_nullish(item) {
                            tasks.push(Task::Value(item));
                        }
                        if i > 0 {
                            tasks.push(Task::Comma);
                        }
                    }
                }
                Value::Object(_) => out.push_str("[object Object]"),
            },
        }
    }
    out
}

/// `Number(x)`.
pub fn to_number_value(v: &Value) -> f64 {
    match v {
        Value::Undefined => f64::NAN,
        Value::Null => 0.0,
        Value::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Value::Number(n) => *n,
        Value::String(s) => to_number(s),
        Value::Array(_) | Value::Object(_) => to_number(&to_display_string(v)),
    }
}

/// ToPrimitive with hint number: arrays and plain objects become their string form.
fn to_primitive(v: &Value) -> Value {
    match v {
        Value::Array(_) | Value::Object(_) => Value::String(to_display_string(v)),
        other => other.clone(),
    }
}

/// `a < b` (IsLessThan with LeftFirst).
pub fn less_than(a: &Value, b: &Value) -> bool {
    let pa = to_primitive(a);
    let pb = to_primitive(b);
    if let (Value::String(sa), Value::String(sb)) = (&pa, &pb) {
        return sa < sb;
    }
    let na = to_number_value(&pa);
    let nb = to_number_value(&pb);
    na < nb
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::js::{json_parse, json_stringify, json_stringify_pretty};

    /// Runs `f` on a thread with a 256 KiB stack: any recursion per nesting
    /// level would overflow it long before the depths used below.
    fn on_small_stack(f: impl FnOnce() + Send + 'static) {
        std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(f)
            .expect("spawns")
            .join()
            .expect("no stack overflow");
    }

    fn deep_array_text(depth: usize) -> String {
        let mut text = "[".repeat(depth);
        text.push_str(&"]".repeat(depth));
        text
    }

    #[test]
    fn million_deep_parse_then_drop_does_not_overflow() {
        on_small_stack(|| {
            let value = json_parse(&deep_array_text(1_000_000)).expect("V8 parses any depth");
            drop(value);
        });
    }

    #[test]
    fn deep_object_parse_then_drop_does_not_overflow() {
        on_small_stack(|| {
            let depth = 50_000;
            let mut text = r#"{"a":"#.repeat(depth);
            text.push('1');
            text.push_str(&"}".repeat(depth));
            let value = json_parse(&text).expect("V8 parses any depth");
            let copy = value.clone();
            assert!(copy == value);
            assert_eq!(json_stringify(&value).as_deref(), Some(text.as_str()));
        });
    }

    #[test]
    fn deep_values_stringify_clone_compare_and_display() {
        on_small_stack(deep_array_operations);
        // Debug recurses, but stops after DEBUG_MAX_DEPTH levels.
        let value = json_parse(&deep_array_text(10_000)).expect("parses");
        assert!(format!("{value:?}").contains('…'));
    }

    fn deep_array_operations() {
        let text = deep_array_text(100_000);
        let value = json_parse(&text).expect("parses");
        assert_eq!(json_stringify(&value).as_deref(), Some(text.as_str()));
        let shallower = json_parse(&deep_array_text(3_000)).expect("parses");
        let pretty = json_stringify_pretty(&shallower, 1).expect("defined");
        assert!(pretty.starts_with("[\n ["));
        assert!(pretty.ends_with("]\n]"));
        let copy = value.clone();
        assert!(copy == value);
        assert!(to_display_string(&value).is_empty());
        assert!(to_number_value(&value) == 0.0);
        assert!(!less_than(&value, &copy));
    }
}
