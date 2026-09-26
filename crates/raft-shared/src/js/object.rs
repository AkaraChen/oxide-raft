//! raft_shared::js ordered object with V8 property order, and Object.assign (decisions.md D3).

use std::collections::{BTreeMap, HashMap, btree_map};
use std::fmt;

use super::string::JsString;
use super::value::Value;

/// A plain JS object. Iteration order is V8's: canonical array-index keys
/// ascending, then all other keys in insertion order. Overwriting a key keeps
/// its position.
///
/// The state is boxed so `Value` stays small. Array-index keys are O(log n);
/// other keys are a linear scan up to `INDEX_THRESHOLD` keys and O(1) expected
/// through a key index above it. Deletes leave tombstones that are compacted
/// once they outnumber the live keys, so removal is amortized O(1).
#[derive(Clone, Default)]
pub struct Object {
    data: Box<Data>,
}

#[derive(Clone, Default)]
struct Data {
    /// Array-index keys, ordered by their numeric value.
    indexed: BTreeMap<u32, (JsString, Value)>,
    /// All other keys in insertion order; `None` is a deleted slot.
    named: Vec<Option<(JsString, Value)>>,
    /// Live entries in `named`.
    named_live: usize,
    /// Slot of each key in `named`, built once `named` passes `INDEX_THRESHOLD`.
    slots: Option<HashMap<JsString, usize>>,
}

/// Objects with at most this many non-index slots are searched linearly.
const INDEX_THRESHOLD: usize = 16;

impl Data {
    fn slot(&self, key: &JsString) -> Option<usize> {
        match &self.slots {
            Some(slots) => slots.get(key).copied(),
            None => self
                .named
                .iter()
                .position(|e| e.as_ref().is_some_and(|(k, _)| k == key)),
        }
    }

    fn build_index(&mut self) {
        let mut slots = HashMap::with_capacity(self.named.len());
        for (slot, entry) in self.named.iter().enumerate() {
            if let Some((k, _)) = entry {
                slots.insert(k.clone(), slot);
            }
        }
        self.slots = Some(slots);
    }

    /// Drops tombstones and renumbers the slots.
    fn compact(&mut self) {
        self.named.retain(Option::is_some);
        if self.named.len() > INDEX_THRESHOLD {
            self.build_index();
        } else {
            self.slots = None;
        }
    }

    fn compact_if_sparse(&mut self) {
        let dead = self.named.len() - self.named_live;
        if dead > INDEX_THRESHOLD && dead > self.named_live {
            self.compact();
        }
    }
}

/// Returns the array index for a canonical decimal key in 0..=4294967294.
fn array_index(key: &JsString) -> Option<u32> {
    let units = key.as_units();
    if units.is_empty() || units.len() > 10 {
        return None;
    }
    if units.len() > 1 && units[0] == u16::from(b'0') {
        return None;
    }
    let mut value: u64 = 0;
    for &u in units {
        if !(u16::from(b'0')..=u16::from(b'9')).contains(&u) {
            return None;
        }
        value = value * 10 + u64::from(u - u16::from(b'0'));
    }
    if value > 4_294_967_294 {
        return None;
    }
    u32::try_from(value).ok()
}

impl Object {
    pub fn new() -> Self {
        Object::default()
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.get_js(&JsString::from(key))
    }

    pub fn get_js(&self, key: &JsString) -> Option<&Value> {
        let data = &*self.data;
        match array_index(key) {
            Some(index) => data.indexed.get(&index).map(|(_, v)| v),
            None => data
                .slot(key)
                .and_then(|slot| data.named[slot].as_ref())
                .map(|(_, v)| v),
        }
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.get_js_mut(&JsString::from(key))
    }

    pub fn get_js_mut(&mut self, key: &JsString) -> Option<&mut Value> {
        let data = &mut *self.data;
        match array_index(key) {
            Some(index) => data.indexed.get_mut(&index).map(|(_, v)| v),
            None => {
                let slot = data.slot(key)?;
                data.named[slot].as_mut().map(|(_, v)| v)
            }
        }
    }

    pub fn insert(&mut self, key: impl Into<JsString>, value: Value) {
        let key = key.into();
        let data = &mut *self.data;
        match array_index(&key) {
            Some(index) => match data.indexed.entry(index) {
                btree_map::Entry::Occupied(mut entry) => entry.get_mut().1 = value,
                btree_map::Entry::Vacant(entry) => {
                    entry.insert((key, value));
                }
            },
            None => match data.slot(&key) {
                Some(slot) => {
                    if let Some(entry) = data.named[slot].as_mut() {
                        entry.1 = value;
                    }
                }
                None => {
                    if let Some(slots) = data.slots.as_mut() {
                        slots.insert(key.clone(), data.named.len());
                    }
                    data.named.push(Some((key, value)));
                    data.named_live += 1;
                    if data.slots.is_none() && data.named.len() > INDEX_THRESHOLD {
                        data.build_index();
                    }
                }
            },
        }
    }

    /// JS `delete obj[key]`.
    pub fn remove(&mut self, key: &str) -> Option<Value> {
        self.remove_js(&JsString::from(key))
    }

    /// JS `delete obj[key]`; amortized O(1).
    pub fn remove_js(&mut self, key: &JsString) -> Option<Value> {
        let data = &mut *self.data;
        match array_index(key) {
            Some(index) => data.indexed.remove(&index).map(|(_, v)| v),
            None => {
                let slot = data.slot(key)?;
                if let Some(slots) = data.slots.as_mut() {
                    slots.remove(key);
                }
                let (_, value) = data.named[slot].take()?;
                data.named_live -= 1;
                data.compact_if_sparse();
                Some(value)
            }
        }
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = (&JsString, &Value)> {
        self.data
            .indexed
            .values()
            .chain(self.data.named.iter().flatten())
            .map(|(k, v)| (k, v))
    }

    pub fn iter_mut(&mut self) -> impl DoubleEndedIterator<Item = (&JsString, &mut Value)> {
        let data = &mut *self.data;
        data.indexed
            .values_mut()
            .chain(data.named.iter_mut().flatten())
            .map(|(k, v)| (&*k, v))
    }

    pub fn keys(&self) -> impl DoubleEndedIterator<Item = &JsString> {
        self.iter().map(|(k, _)| k)
    }

    pub fn values(&self) -> impl DoubleEndedIterator<Item = &Value> {
        self.iter().map(|(_, v)| v)
    }

    pub fn len(&self) -> usize {
        self.data.indexed.len() + self.data.named_live
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Keeps only the properties for which `keep` returns true; order is unchanged.
    pub fn retain(&mut self, mut keep: impl FnMut(&JsString, &mut Value) -> bool) {
        let data = &mut *self.data;
        data.indexed.retain(|_, (k, v)| keep(k, v));
        for entry in data.named.iter_mut() {
            let drop_it = entry.as_mut().is_some_and(|(k, v)| !keep(k, v));
            if drop_it {
                *entry = None;
                data.named_live -= 1;
            }
        }
        data.compact();
    }

    /// Object.fromEntries.
    pub fn from_pairs<K: Into<JsString>>(pairs: impl IntoIterator<Item = (K, Value)>) -> Self {
        let mut object = Object::new();
        for (k, v) in pairs {
            object.insert(k, v);
        }
        object
    }
}

impl PartialEq for Object {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len()
            && self
                .iter()
                .zip(other.iter())
                .all(|((ka, va), (kb, vb))| ka == kb && va == vb)
    }
}

impl fmt::Debug for Object {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl<K: Into<JsString>> FromIterator<(K, Value)> for Object {
    fn from_iter<I: IntoIterator<Item = (K, Value)>>(iter: I) -> Self {
        Object::from_pairs(iter)
    }
}

impl<K: Into<JsString>> Extend<(K, Value)> for Object {
    fn extend<I: IntoIterator<Item = (K, Value)>>(&mut self, iter: I) {
        for (k, v) in iter {
            self.insert(k, v);
        }
    }
}

type OwnedEntries = std::iter::Chain<
    btree_map::IntoValues<u32, (JsString, Value)>,
    std::iter::Flatten<std::vec::IntoIter<Option<(JsString, Value)>>>,
>;

/// Owned iteration in V8 property order.
impl IntoIterator for Object {
    type Item = (JsString, Value);
    type IntoIter = OwnedEntries;

    fn into_iter(self) -> OwnedEntries {
        let data = *self.data;
        data.indexed
            .into_values()
            .chain(data.named.into_iter().flatten())
    }
}

impl<'a> IntoIterator for &'a Object {
    type Item = (&'a JsString, &'a Value);
    type IntoIter = Box<dyn DoubleEndedIterator<Item = (&'a JsString, &'a Value)> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

/// Object.assign(target, source) / `{...target, ...source}`: every own
/// property of `source`, including `undefined` ones, is written to `target`.
pub fn object_assign(target: &mut Object, source: &Object) {
    for (k, v) in source.iter() {
        target.insert(k.clone(), v.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn many_keys_keep_order_and_overwrite_in_place() {
        let n = 20_000u32;
        let mut object = Object::new();
        for i in 0..n {
            object.insert(format!("k{i}"), Value::Number(f64::from(i)));
            object.insert(format!("{}", n - i), Value::Null);
        }
        object.insert("k5", Value::Bool(true));
        assert_eq!(object.len(), 40_000);
        let keys: Vec<JsString> = object.keys().take(2).cloned().collect();
        assert_eq!(keys, [JsString::from("1"), JsString::from("2")]);
        let named: Vec<JsString> = object.keys().skip(20_000).take(6).cloned().collect();
        let expected: Vec<JsString> = (0..6).map(|i| JsString::from(format!("k{i}"))).collect();
        assert_eq!(named, expected);
        assert_eq!(object.get("k5"), Some(&Value::Bool(true)));
        assert_eq!(object.remove("k2"), Some(Value::Number(2.0)));
        assert_eq!(object.get("k3"), Some(&Value::Number(3.0)));
        assert_eq!(object.get("k2"), None);
        object.insert("k2", Value::Null);
        assert_eq!(object.keys().last(), Some(&JsString::from("k2")));
        // Deleting most keys goes through compaction and keeps order.
        for i in (0..n).filter(|i| i % 4 != 0) {
            object.remove(&format!("k{i}"));
        }
        let named: Vec<JsString> = object.keys().skip(20_000).take(3).cloned().collect();
        let expected: Vec<JsString> = ["k0", "k4", "k8"].map(JsString::from).to_vec();
        assert_eq!(named, expected);
        assert_eq!(object.len(), 20_000 + 5_000);
        object.insert("k4", Value::Bool(false));
        assert_eq!(object.keys().nth(20_001), Some(&JsString::from("k4")));
    }

    #[test]
    fn small_objects_use_linear_lookup() {
        let mut object = Object::from_pairs([("a", Value::Null), ("b", Value::Null)]);
        assert!(object.data.slots.is_none());
        object.remove("a");
        object.insert("a", Value::Bool(true));
        let keys: Vec<JsString> = object.keys().cloned().collect();
        assert_eq!(keys, [JsString::from("b"), JsString::from("a")]);
        assert!(std::mem::size_of::<Value>() <= 32);
    }
}
