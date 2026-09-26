//! raft_shared::js serde bridges: `to_value` (Serializer into `Value`) and `from_value` (Deserializer from `Value`) (decisions.md D3).

use std::cell::{Cell, RefCell};
use std::fmt;

use serde::de::{self, DeserializeOwned, DeserializeSeed, IntoDeserializer, Visitor};
use serde::ser::{self, Serialize};

use super::convert::{f64_to_i64_exact, f64_to_u64_exact, i64_to_f64_exact, u64_to_f64_exact};
use super::error::JsError;
use super::number::{is_safe_integer, number_to_string, to_number};
use super::object::Object;
use super::string::JsString;
use super::value::Value;

/// Newtype name that lets the bridge carry a `JsString` without losing lone surrogates.
const JS_STRING_TOKEN: &str = "$raft_shared::js::JsString";

/// Container nesting the bridges accept (serde_json's default recursion limit);
/// deeper input is a RangeError instead of a stack overflow.
const MAX_DEPTH: usize = 128;

fn enter(depth: usize) -> Result<usize, JsError> {
    if depth >= MAX_DEPTH {
        return Err(JsError::range("Maximum call stack size exceeded"));
    }
    Ok(depth + 1)
}

/// Serializes a typed value into a `js::Value` (struct fields in declaration
/// order). Containers nested deeper than 128 levels give
/// `RangeError: Maximum call stack size exceeded`.
pub fn to_value<T: Serialize + ?Sized>(value: &T) -> Result<Value, JsError> {
    value.serialize(ValueSerializer { depth: 0 })
}

/// Deserializes a typed value from a `js::Value`.
///
/// `String` fields read lone surrogates as U+FFFD (lossy, no error); use
/// `JsString` for fields that must keep them. Map keys deserialize into integer
/// or float key types when the key is the canonical `String(n)` of a number.
/// Containers nested deeper than 128 levels give
/// `RangeError: Maximum call stack size exceeded`.
pub fn from_value<T: DeserializeOwned>(value: &Value) -> Result<T, JsError> {
    T::deserialize(ValueDeserializer { value, depth: 0 })
}

// ---------------------------------------------------------------------------
// JsString serde impls

thread_local! {
    /// Set by `ValueSerializer` just before it serializes a `JsString`'s units.
    static CAPTURING: Cell<bool> = const { Cell::new(false) };
    /// The captured code units.
    static CAPTURED: RefCell<Option<Vec<u16>>> = const { RefCell::new(None) };
}

/// Arms the capture for one `Units::serialize` call; disarms on drop.
struct CaptureGuard;

impl CaptureGuard {
    fn start() -> Self {
        CAPTURED.with(|c| c.borrow_mut().take());
        CAPTURING.with(|c| c.set(true));
        CaptureGuard
    }

    fn finish(self) -> Option<Vec<u16>> {
        CAPTURED.with(|c| c.borrow_mut().take())
    }
}

impl Drop for CaptureGuard {
    fn drop(&mut self) {
        CAPTURING.with(|c| c.set(false));
    }
}

struct Units<'a>(&'a [u16]);

impl Serialize for Units<'_> {
    fn serialize<S: ser::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if CAPTURING.with(|c| c.replace(false)) {
            // `to_value`: hand over the exact code units, lone surrogates included.
            CAPTURED.with(|c| *c.borrow_mut() = Some(self.0.to_vec()));
            return serializer.serialize_unit();
        }
        // Any other serializer (serde_json, logs, ...) gets a string, with lone
        // surrogates replaced by U+FFFD as when Node writes the string to a stream.
        serializer.serialize_str(&String::from_utf16_lossy(self.0))
    }
}

/// With `to_value` the code units are kept exactly. Any other serializer sees a
/// plain string in which lone surrogates are U+FFFD (Node's stream-write
/// behaviour), so the data shape never changes.
impl Serialize for JsString {
    fn serialize<S: ser::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_newtype_struct(JS_STRING_TOKEN, &Units(self.as_units()))
    }
}

struct JsStringVisitor;

impl<'de> Visitor<'de> for JsStringVisitor {
    type Value = JsString;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a string")
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<JsString, E> {
        Ok(JsString::from(v))
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<JsString, E> {
        Ok(JsString::from(v))
    }

    fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<JsString, A::Error> {
        let mut units = Vec::new();
        while let Some(u) = seq.next_element::<u16>()? {
            units.push(u);
        }
        Ok(JsString::from_units(units))
    }

    fn visit_newtype_struct<D: de::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<JsString, D::Error> {
        deserializer.deserialize_any(JsStringVisitor)
    }
}

impl<'de> de::Deserialize<'de> for JsString {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_newtype_struct(JS_STRING_TOKEN, JsStringVisitor)
    }
}

// ---------------------------------------------------------------------------
// Serializer

fn integer_value(v: i128) -> Result<Value, JsError> {
    i64::try_from(v)
        .ok()
        .and_then(i64_to_f64_exact)
        .filter(|n| is_safe_integer(*n))
        .map(Value::Number)
        .ok_or_else(|| JsError::range(format!("integer {v} is outside the safe integer range")))
}

fn unsigned_value(v: u128) -> Result<Value, JsError> {
    u64::try_from(v)
        .ok()
        .and_then(u64_to_f64_exact)
        .filter(|n| is_safe_integer(*n))
        .map(Value::Number)
        .ok_or_else(|| JsError::range(format!("integer {v} is outside the safe integer range")))
}

/// Converts a serialized map key into a property key, as JS would.
fn property_key(mut key: Value) -> Result<JsString, JsError> {
    match &mut key {
        Value::String(s) => Ok(std::mem::take(s)),
        Value::Number(n) => Ok(JsString::from(number_to_string(*n))),
        Value::Bool(b) => Ok(JsString::from(if *b { "true" } else { "false" })),
        _ => Err(JsError::type_error(
            "map key must be a string, number, or boolean",
        )),
    }
}

#[derive(Clone, Copy)]
struct ValueSerializer {
    /// Containers entered so far.
    depth: usize,
}

impl ser::Serializer for ValueSerializer {
    type Ok = Value;
    type Error = JsError;
    type SerializeSeq = SeqSerializer;
    type SerializeTuple = SeqSerializer;
    type SerializeTupleStruct = SeqSerializer;
    type SerializeTupleVariant = TupleVariantSerializer;
    type SerializeMap = MapSerializer;
    type SerializeStruct = StructSerializer;
    type SerializeStructVariant = StructVariantSerializer;

    fn serialize_bool(self, v: bool) -> Result<Value, JsError> {
        Ok(Value::Bool(v))
    }

    fn serialize_i8(self, v: i8) -> Result<Value, JsError> {
        integer_value(i128::from(v))
    }

    fn serialize_i16(self, v: i16) -> Result<Value, JsError> {
        integer_value(i128::from(v))
    }

    fn serialize_i32(self, v: i32) -> Result<Value, JsError> {
        integer_value(i128::from(v))
    }

    fn serialize_i64(self, v: i64) -> Result<Value, JsError> {
        integer_value(i128::from(v))
    }

    fn serialize_i128(self, v: i128) -> Result<Value, JsError> {
        integer_value(v)
    }

    fn serialize_u8(self, v: u8) -> Result<Value, JsError> {
        unsigned_value(u128::from(v))
    }

    fn serialize_u16(self, v: u16) -> Result<Value, JsError> {
        unsigned_value(u128::from(v))
    }

    fn serialize_u32(self, v: u32) -> Result<Value, JsError> {
        unsigned_value(u128::from(v))
    }

    fn serialize_u64(self, v: u64) -> Result<Value, JsError> {
        unsigned_value(u128::from(v))
    }

    fn serialize_u128(self, v: u128) -> Result<Value, JsError> {
        unsigned_value(v)
    }

    fn serialize_f32(self, v: f32) -> Result<Value, JsError> {
        Ok(Value::Number(f64::from(v)))
    }

    fn serialize_f64(self, v: f64) -> Result<Value, JsError> {
        Ok(Value::Number(v))
    }

    fn serialize_char(self, v: char) -> Result<Value, JsError> {
        Ok(Value::String(JsString::from(v.to_string())))
    }

    fn serialize_str(self, v: &str) -> Result<Value, JsError> {
        Ok(Value::String(JsString::from(v)))
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Value, JsError> {
        Ok(Value::Array(
            v.iter().map(|&b| Value::Number(f64::from(b))).collect(),
        ))
    }

    fn serialize_none(self) -> Result<Value, JsError> {
        Ok(Value::Undefined)
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Value, JsError> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Value, JsError> {
        Ok(Value::Null)
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Value, JsError> {
        Ok(Value::Null)
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<Value, JsError> {
        Ok(Value::string(variant))
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        name: &'static str,
        value: &T,
    ) -> Result<Value, JsError> {
        if name != JS_STRING_TOKEN {
            return value.serialize(self);
        }
        let guard = CaptureGuard::start();
        let inner = value.serialize(self)?;
        match guard.finish() {
            Some(units) => Ok(Value::String(JsString::from_units(units))),
            None => Ok(inner),
        }
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Value, JsError> {
        let depth = enter(self.depth)?;
        let mut object = Object::new();
        object.insert(variant, value.serialize(ValueSerializer { depth })?);
        Ok(Value::Object(object))
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<SeqSerializer, JsError> {
        Ok(SeqSerializer {
            depth: enter(self.depth)?,
            items: Vec::with_capacity(len.unwrap_or(0).min(4096)),
        })
    }

    fn serialize_tuple(self, len: usize) -> Result<SeqSerializer, JsError> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<SeqSerializer, JsError> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<TupleVariantSerializer, JsError> {
        Ok(TupleVariantSerializer {
            depth: enter(self.depth)?,
            variant,
            items: Vec::with_capacity(len.min(4096)),
        })
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<MapSerializer, JsError> {
        Ok(MapSerializer {
            depth: enter(self.depth)?,
            object: Object::new(),
            next_key: None,
        })
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<StructSerializer, JsError> {
        Ok(StructSerializer {
            depth: enter(self.depth)?,
            object: Object::new(),
        })
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<StructVariantSerializer, JsError> {
        Ok(StructVariantSerializer {
            depth: enter(self.depth)?,
            variant,
            object: Object::new(),
        })
    }
}

struct SeqSerializer {
    depth: usize,
    items: Vec<Value>,
}

impl ser::SerializeSeq for SeqSerializer {
    type Ok = Value;
    type Error = JsError;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), JsError> {
        let depth = self.depth;
        self.items.push(value.serialize(ValueSerializer { depth })?);
        Ok(())
    }

    fn end(self) -> Result<Value, JsError> {
        Ok(Value::Array(self.items))
    }
}

impl ser::SerializeTuple for SeqSerializer {
    type Ok = Value;
    type Error = JsError;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), JsError> {
        ser::SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Value, JsError> {
        ser::SerializeSeq::end(self)
    }
}

impl ser::SerializeTupleStruct for SeqSerializer {
    type Ok = Value;
    type Error = JsError;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), JsError> {
        ser::SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Value, JsError> {
        ser::SerializeSeq::end(self)
    }
}

struct TupleVariantSerializer {
    depth: usize,
    variant: &'static str,
    items: Vec<Value>,
}

impl ser::SerializeTupleVariant for TupleVariantSerializer {
    type Ok = Value;
    type Error = JsError;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), JsError> {
        let depth = self.depth;
        self.items.push(value.serialize(ValueSerializer { depth })?);
        Ok(())
    }

    fn end(self) -> Result<Value, JsError> {
        let mut object = Object::new();
        object.insert(self.variant, Value::Array(self.items));
        Ok(Value::Object(object))
    }
}

struct MapSerializer {
    depth: usize,
    object: Object,
    next_key: Option<JsString>,
}

impl ser::SerializeMap for MapSerializer {
    type Ok = Value;
    type Error = JsError;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), JsError> {
        let depth = self.depth;
        self.next_key = Some(property_key(key.serialize(ValueSerializer { depth })?)?);
        Ok(())
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), JsError> {
        let key = self
            .next_key
            .take()
            .ok_or_else(|| JsError::type_error("map value serialized before its key"))?;
        let depth = self.depth;
        self.object
            .insert(key, value.serialize(ValueSerializer { depth })?);
        Ok(())
    }

    fn end(self) -> Result<Value, JsError> {
        Ok(Value::Object(self.object))
    }
}

struct StructSerializer {
    depth: usize,
    object: Object,
}

impl ser::SerializeStruct for StructSerializer {
    type Ok = Value;
    type Error = JsError;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), JsError> {
        let depth = self.depth;
        self.object
            .insert(key, value.serialize(ValueSerializer { depth })?);
        Ok(())
    }

    fn end(self) -> Result<Value, JsError> {
        Ok(Value::Object(self.object))
    }
}

struct StructVariantSerializer {
    depth: usize,
    variant: &'static str,
    object: Object,
}

impl ser::SerializeStructVariant for StructVariantSerializer {
    type Ok = Value;
    type Error = JsError;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), JsError> {
        let depth = self.depth;
        self.object
            .insert(key, value.serialize(ValueSerializer { depth })?);
        Ok(())
    }

    fn end(self) -> Result<Value, JsError> {
        let mut outer = Object::new();
        outer.insert(self.variant, Value::Object(self.object));
        Ok(Value::Object(outer))
    }
}

// ---------------------------------------------------------------------------
// Deserializer

#[derive(Clone, Copy)]
struct ValueDeserializer<'a> {
    value: &'a Value,
    /// Containers entered so far.
    depth: usize,
}

impl ValueDeserializer<'_> {
    fn unexpected(&self) -> de::Unexpected<'static> {
        match self.value {
            Value::Undefined => de::Unexpected::Other("undefined"),
            Value::Null => de::Unexpected::Unit,
            Value::Bool(b) => de::Unexpected::Bool(*b),
            Value::Number(n) => de::Unexpected::Float(*n),
            Value::String(_) => de::Unexpected::Other("string"),
            Value::Array(_) => de::Unexpected::Seq,
            Value::Object(_) => de::Unexpected::Map,
        }
    }

    fn invalid<'de, V: Visitor<'de>>(&self, visitor: &V) -> JsError {
        de::Error::invalid_type(self.unexpected(), visitor)
    }

    fn integer<'de, V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        match self.value {
            Value::Number(n) => visit_integer(*n, visitor),
            _ => Err(self.invalid(&visitor)),
        }
    }
}

/// Visits a safe integer as i64 (negative) or u64 (non-negative).
fn visit_integer<'de, V: Visitor<'de>>(n: f64, visitor: V) -> Result<V::Value, JsError> {
    let not_safe = || {
        JsError::type_error(format!(
            "expected a safe integer, got {}",
            number_to_string(n)
        ))
    };
    if !is_safe_integer(n) {
        return Err(not_safe());
    }
    if n < 0.0 {
        visitor.visit_i64(f64_to_i64_exact(n).ok_or_else(not_safe)?)
    } else {
        visitor.visit_u64(f64_to_u64_exact(n).ok_or_else(not_safe)?)
    }
}

struct SeqDeserializer<'a> {
    iter: std::slice::Iter<'a, Value>,
    depth: usize,
}

impl<'de, 'a> de::SeqAccess<'de> for SeqDeserializer<'a> {
    type Error = JsError;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, JsError> {
        match self.iter.next() {
            Some(value) => seed
                .deserialize(ValueDeserializer {
                    value,
                    depth: self.depth,
                })
                .map(Some),
            None => Ok(None),
        }
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.iter.len())
    }
}

type Entries<'a> = Box<dyn Iterator<Item = (&'a JsString, &'a Value)> + 'a>;

struct MapDeserializer<'a> {
    iter: Entries<'a>,
    value: Option<&'a Value>,
    depth: usize,
    remaining: usize,
}

impl<'de, 'a> de::MapAccess<'de> for MapDeserializer<'a> {
    type Error = JsError;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, JsError> {
        match self.iter.next() {
            Some((k, v)) => {
                self.value = Some(v);
                self.remaining = self.remaining.saturating_sub(1);
                seed.deserialize(MapKeyDeserializer(k)).map(Some)
            }
            None => Ok(None),
        }
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, JsError> {
        match self.value.take() {
            Some(value) => seed.deserialize(ValueDeserializer {
                value,
                depth: self.depth,
            }),
            None => Err(JsError::type_error("map value requested before its key")),
        }
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.remaining)
    }
}

/// A property key. Keys are strings; they deserialize as numbers when the key
/// type asks for one and the key is the canonical `String(n)` of that number
/// (the inverse of how `to_value` writes numeric map keys), and as booleans
/// for `"true"` / `"false"`.
struct MapKeyDeserializer<'a>(&'a JsString);

impl MapKeyDeserializer<'_> {
    fn number(&self) -> Option<f64> {
        let n = to_number(self.0);
        self.0.eq_str(&number_to_string(n)).then_some(n)
    }

    fn invalid<'de, V: Visitor<'de>>(&self, visitor: &V) -> JsError {
        de::Error::invalid_type(de::Unexpected::Str(&self.0.to_string_lossy()), visitor)
    }

    fn integer<'de, V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        match self.number() {
            Some(n) if is_safe_integer(n) => visit_integer(n, visitor),
            _ => Err(self.invalid(&visitor)),
        }
    }

    fn float<'de, V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        match self.number() {
            Some(n) => visitor.visit_f64(n),
            None => Err(self.invalid(&visitor)),
        }
    }
}

impl<'de, 'a> de::Deserializer<'de> for MapKeyDeserializer<'a> {
    type Error = JsError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        visitor.visit_string(self.0.to_string_lossy())
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        if self.0.eq_str("true") {
            visitor.visit_bool(true)
        } else if self.0.eq_str("false") {
            visitor.visit_bool(false)
        } else {
            Err(self.invalid(&visitor))
        }
    }

    fn deserialize_i8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        self.integer(visitor)
    }

    fn deserialize_i16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        self.integer(visitor)
    }

    fn deserialize_i32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        self.integer(visitor)
    }

    fn deserialize_i64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        self.integer(visitor)
    }

    fn deserialize_i128<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        self.integer(visitor)
    }

    fn deserialize_u8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        self.integer(visitor)
    }

    fn deserialize_u16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        self.integer(visitor)
    }

    fn deserialize_u32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        self.integer(visitor)
    }

    fn deserialize_u64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        self.integer(visitor)
    }

    fn deserialize_u128<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        self.integer(visitor)
    }

    fn deserialize_f32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        self.float(visitor)
    }

    fn deserialize_f64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        self.float(visitor)
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        visitor.visit_some(self)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, JsError> {
        if name == JS_STRING_TOKEN {
            return visit_units(self.0, visitor);
        }
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, JsError> {
        let variant: de::value::StringDeserializer<JsError> =
            self.0.to_string_lossy().into_deserializer();
        visitor.visit_enum(variant)
    }

    serde::forward_to_deserialize_any! {
        char str string bytes byte_buf unit unit_struct seq tuple tuple_struct map struct
        identifier ignored_any
    }
}

struct EnumDeserializer<'a> {
    variant: &'a JsString,
    value: &'a Value,
    depth: usize,
}

impl<'de, 'a> de::EnumAccess<'de> for EnumDeserializer<'a> {
    type Error = JsError;
    type Variant = VariantDeserializer<'a>;

    fn variant_seed<V: DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, VariantDeserializer<'a>), JsError> {
        let variant = seed.deserialize(MapKeyDeserializer(self.variant))?;
        Ok((
            variant,
            VariantDeserializer {
                inner: ValueDeserializer {
                    value: self.value,
                    depth: self.depth,
                },
            },
        ))
    }
}

struct VariantDeserializer<'a> {
    inner: ValueDeserializer<'a>,
}

impl<'de, 'a> de::VariantAccess<'de> for VariantDeserializer<'a> {
    type Error = JsError;

    fn unit_variant(self) -> Result<(), JsError> {
        match self.inner.value {
            Value::Undefined | Value::Null => Ok(()),
            _ => Err(JsError::type_error("expected a unit variant")),
        }
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, JsError> {
        seed.deserialize(self.inner)
    }

    fn tuple_variant<V: Visitor<'de>>(self, _len: usize, visitor: V) -> Result<V::Value, JsError> {
        de::Deserializer::deserialize_seq(self.inner, visitor)
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, JsError> {
        de::Deserializer::deserialize_map(self.inner, visitor)
    }
}

fn visit_units<'de, V: Visitor<'de>>(s: &JsString, visitor: V) -> Result<V::Value, JsError> {
    let units = de::value::SeqDeserializer::<_, JsError>::new(s.as_units().iter().copied());
    visitor.visit_seq(units)
}

fn visit_array<'de, V: Visitor<'de>>(
    items: &[Value],
    depth: usize,
    visitor: V,
) -> Result<V::Value, JsError> {
    let depth = enter(depth)?;
    visitor.visit_seq(SeqDeserializer {
        iter: items.iter(),
        depth,
    })
}

fn visit_object<'de, V: Visitor<'de>>(
    object: &Object,
    depth: usize,
    visitor: V,
) -> Result<V::Value, JsError> {
    let depth = enter(depth)?;
    visitor.visit_map(MapDeserializer {
        iter: Box::new(object.iter()),
        value: None,
        depth,
        remaining: object.len(),
    })
}

impl<'de, 'a> de::Deserializer<'de> for ValueDeserializer<'a> {
    type Error = JsError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        match self.value {
            Value::Undefined | Value::Null => visitor.visit_unit(),
            Value::Bool(b) => visitor.visit_bool(*b),
            Value::Number(n) => {
                let n = *n;
                let negative_zero = n == 0.0 && n.is_sign_negative();
                if is_safe_integer(n) && !negative_zero {
                    visit_integer(n, visitor)
                } else {
                    visitor.visit_f64(n)
                }
            }
            Value::String(s) => visitor.visit_string(s.to_string_lossy()),
            Value::Array(items) => visit_array(items, self.depth, visitor),
            Value::Object(object) => visit_object(object, self.depth, visitor),
        }
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        match self.value {
            Value::Bool(b) => visitor.visit_bool(*b),
            _ => Err(self.invalid(&visitor)),
        }
    }

    fn deserialize_i8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        self.integer(visitor)
    }

    fn deserialize_i16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        self.integer(visitor)
    }

    fn deserialize_i32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        self.integer(visitor)
    }

    fn deserialize_i64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        self.integer(visitor)
    }

    fn deserialize_i128<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        self.integer(visitor)
    }

    fn deserialize_u8<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        self.integer(visitor)
    }

    fn deserialize_u16<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        self.integer(visitor)
    }

    fn deserialize_u32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        self.integer(visitor)
    }

    fn deserialize_u64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        self.integer(visitor)
    }

    fn deserialize_u128<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        self.integer(visitor)
    }

    fn deserialize_f32<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        self.deserialize_f64(visitor)
    }

    fn deserialize_f64<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        match self.value {
            Value::Number(n) => visitor.visit_f64(*n),
            _ => Err(self.invalid(&visitor)),
        }
    }

    fn deserialize_char<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        self.deserialize_string(visitor)
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        self.deserialize_string(visitor)
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        match self.value {
            Value::String(s) => visitor.visit_string(s.to_string_lossy()),
            _ => Err(self.invalid(&visitor)),
        }
    }

    fn deserialize_bytes<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        self.deserialize_any(visitor)
    }

    fn deserialize_byte_buf<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        self.deserialize_any(visitor)
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        match self.value {
            Value::Undefined | Value::Null => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        match self.value {
            Value::Undefined | Value::Null => visitor.visit_unit(),
            _ => Err(self.invalid(&visitor)),
        }
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, JsError> {
        self.deserialize_unit(visitor)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, JsError> {
        match self.value {
            Value::String(s) if name == JS_STRING_TOKEN => visit_units(s, visitor),
            _ => visitor.visit_newtype_struct(self),
        }
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        match self.value {
            Value::Array(items) => visit_array(items, self.depth, visitor),
            _ => Err(self.invalid(&visitor)),
        }
    }

    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, JsError> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, JsError> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        match self.value {
            Value::Object(object) => visit_object(object, self.depth, visitor),
            _ => Err(self.invalid(&visitor)),
        }
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, JsError> {
        self.deserialize_map(visitor)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, JsError> {
        match self.value {
            Value::String(s) => {
                let variant: de::value::StringDeserializer<JsError> =
                    s.to_string_lossy().into_deserializer();
                visitor.visit_enum(variant)
            }
            Value::Object(object) if object.len() == 1 => {
                let (variant, value) = object
                    .iter()
                    .next()
                    .ok_or_else(|| JsError::type_error("expected an enum variant"))?;
                visitor.visit_enum(EnumDeserializer {
                    variant,
                    value,
                    depth: enter(self.depth)?,
                })
            }
            _ => Err(self.invalid(&visitor)),
        }
    }

    fn deserialize_identifier<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        self.deserialize_string(visitor)
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, JsError> {
        visitor.visit_unit()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};

    use super::*;

    #[test]
    fn numeric_and_bool_map_keys_round_trip() {
        let map = BTreeMap::from([(10u32, 1), (2, 2)]);
        let value = to_value(&map).expect("serializes");
        let keys: Vec<String> = value
            .as_object()
            .expect("object")
            .keys()
            .map(JsString::to_string_lossy)
            .collect();
        assert_eq!(keys, ["2", "10"]);
        let back: BTreeMap<u32, i32> = from_value(&value).expect("deserializes");
        assert_eq!(back, map);

        let signed = HashMap::from([(-3i64, true)]);
        let back: HashMap<i64, bool> = from_value(&to_value(&signed).expect("ser")).expect("de");
        assert_eq!(back, signed);

        let bools = BTreeMap::from([(true, 1u8)]);
        let back: BTreeMap<bool, u8> = from_value(&to_value(&bools).expect("ser")).expect("de");
        assert_eq!(back, bools);

        let non_canonical = Value::object([("01", Value::Number(1.0))]);
        assert!(from_value::<BTreeMap<u32, f64>>(&non_canonical).is_err());
        let strings: BTreeMap<String, f64> = from_value(&non_canonical).expect("string keys");
        assert_eq!(strings.get("01"), Some(&1.0));
    }

    #[test]
    fn js_string_is_a_lossy_string_for_foreign_serializers() {
        let s = JsString::from_units(vec![0x61, 0xD800]);
        assert_eq!(serde_json::to_string(&s).expect("json"), "\"a\u{FFFD}\"");
        assert_eq!(
            serde_json::to_string(&JsString::from("ok")).expect("json"),
            "\"ok\""
        );
        let back: JsString = serde_json::from_str("\"ok\"").expect("json");
        assert_eq!(back, JsString::from("ok"));
        // The bridge keeps the code units exactly.
        assert_eq!(to_value(&s).expect("value"), Value::String(s.clone()));
        let map = BTreeMap::from([(s.clone(), 1u8)]);
        let value = to_value(&map).expect("value");
        let back: BTreeMap<JsString, u8> = from_value(&value).expect("keys keep units");
        assert_eq!(back, map);
    }

    #[test]
    fn deep_nesting_is_a_range_error() {
        let mut deep = Value::Null;
        for _ in 0..1_000 {
            deep = Value::Array(vec![deep]);
        }
        let err = from_value::<serde_json::Value>(&deep).expect_err("too deep");
        assert_eq!(err.name, "RangeError");
        assert_eq!(err.message, "Maximum call stack size exceeded");
        let mut shallow = Value::Null;
        for _ in 0..100 {
            shallow = Value::Array(vec![shallow]);
        }
        let json: serde_json::Value = from_value(&shallow).expect("within limit");
        let err = to_value(&json_nest(1_000)).expect_err("too deep");
        assert_eq!(err.name, "RangeError");
        assert!(to_value(&json).is_ok());
    }

    fn json_nest(depth: usize) -> serde_json::Value {
        let mut v = serde_json::Value::Null;
        for _ in 0..depth {
            v = serde_json::Value::Array(vec![v]);
        }
        v
    }
}
