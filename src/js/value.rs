//! A JavaScript-like value model.
//!
//! The converter's behavior is defined by the original JavaScript implementation,
//! so configs are kept as dynamic values with JS semantics (undefined vs null,
//! property order, number formatting) to reproduce its output byte for byte.

use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use indexmap::IndexMap;

use super::number::{js_number_to_string, string_to_number};

#[derive(Clone, Debug, Default)]
pub enum Value {
    #[default]
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Arr),
    Object(Object),
    /// A JS `Date` (time value in ms since epoch; NaN for invalid dates).
    Date(f64),
}

pub static UNDEFINED: Value = Value::Undefined;

/// A JS array. Clones share the same allocation (a JS reference); writes
/// copy-on-write, and [`Arr::ptr_eq`] exposes object identity (js-yaml emits
/// anchors for values referenced twice).
#[derive(Clone, Debug, Default)]
pub struct Arr(Arc<Vec<Value>>);

impl Arr {
    pub fn new() -> Self {
        Arr(Arc::new(Vec::new()))
    }

    pub fn ptr_eq(&self, other: &Arr) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    pub fn identity(&self) -> usize {
        Arc::as_ptr(&self.0) as *const () as usize
    }

    pub fn into_vec(self) -> Vec<Value> {
        Arc::try_unwrap(self.0).unwrap_or_else(|a| (*a).clone())
    }
}

impl Deref for Arr {
    type Target = Vec<Value>;
    fn deref(&self) -> &Vec<Value> {
        &self.0
    }
}

impl DerefMut for Arr {
    fn deref_mut(&mut self) -> &mut Vec<Value> {
        Arc::make_mut(&mut self.0)
    }
}

impl From<Vec<Value>> for Arr {
    fn from(v: Vec<Value>) -> Self {
        Arr(Arc::new(v))
    }
}

impl FromIterator<Value> for Arr {
    fn from_iter<T: IntoIterator<Item = Value>>(iter: T) -> Self {
        Arr(Arc::new(iter.into_iter().collect()))
    }
}

impl<'a> IntoIterator for &'a Arr {
    type Item = &'a Value;
    type IntoIter = std::slice::Iter<'a, Value>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// An ordinary JS object: own enumerable string-keyed data properties.
/// Shares its allocation on clone like [`Arr`].
#[derive(Clone, Debug, Default)]
pub struct Object {
    map: Arc<IndexMap<String, Value>>,
}

/// Whether `key` is a canonical array index, which JS enumerates first in
/// ascending numeric order regardless of insertion order.
pub fn is_array_index(key: &str) -> bool {
    let bytes = key.as_bytes();
    if bytes.is_empty() || bytes.len() > 10 {
        return false;
    }
    if bytes[0] == b'0' {
        return bytes.len() == 1;
    }
    if !bytes.iter().all(u8::is_ascii_digit) {
        return false;
    }
    key.parse::<u64>().map(|n| n < 4_294_967_295).unwrap_or(false)
}

impl Object {
    pub fn new() -> Self {
        Object { map: Arc::new(IndexMap::new()) }
    }

    pub fn ptr_eq(&self, other: &Object) -> bool {
        Arc::ptr_eq(&self.map, &other.map)
    }

    pub fn identity(&self) -> usize {
        Arc::as_ptr(&self.map) as *const () as usize
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.map.get(key)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        if !self.map.contains_key(key) {
            return None;
        }
        Arc::make_mut(&mut self.map).get_mut(key)
    }

    /// `obj[key] = value`: replaces in place, or appends a new property.
    pub fn set(&mut self, key: impl Into<String>, value: Value) {
        let key = key.into();
        let map = Arc::make_mut(&mut self.map);
        if let Some(slot) = map.get_mut(&key) {
            *slot = value;
        } else {
            map.insert(key, value);
        }
    }

    /// `delete obj[key]`
    pub fn remove(&mut self, key: &str) -> Option<Value> {
        if !self.map.contains_key(key) {
            return None;
        }
        Arc::make_mut(&mut self.map).shift_remove(key)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.map.contains_key(key)
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Property keys in JS enumeration order.
    pub fn keys(&self) -> Vec<&String> {
        self.entries().into_iter().map(|(k, _)| k).collect()
    }

    /// Entries in JS enumeration order (array indices first, ascending).
    pub fn entries(&self) -> Vec<(&String, &Value)> {
        if !self.map.keys().any(|k| is_array_index(k)) {
            return self.map.iter().collect();
        }
        let mut indexed: Vec<(&String, &Value)> = self.map.iter().filter(|(k, _)| is_array_index(k)).collect();
        indexed.sort_by_key(|(k, _)| k.parse::<u64>().unwrap_or(0));
        indexed.extend(self.map.iter().filter(|(k, _)| !is_array_index(k)));
        indexed
    }

    pub fn into_entries(self) -> Vec<(String, Value)> {
        let order: Vec<String> = self.keys().into_iter().cloned().collect();
        let mut map = Arc::try_unwrap(self.map).unwrap_or_else(|m| (*m).clone());
        order
            .into_iter()
            .map(|k| {
                let v = map.shift_remove(&k).unwrap_or_default();
                (k, v)
            })
            .collect()
    }
}

impl FromIterator<(String, Value)> for Object {
    fn from_iter<T: IntoIterator<Item = (String, Value)>>(iter: T) -> Self {
        let mut obj = Object::new();
        for (k, v) in iter {
            obj.set(k, v);
        }
        obj
    }
}

/// Builds an object literal; `Value::Undefined` entries are kept as own
/// properties, exactly like `{ a: undefined }` in JS.
#[macro_export]
macro_rules! obj {
    () => { $crate::js::Value::Object($crate::js::Object::new()) };
    ($($k:expr => $v:expr),+ $(,)?) => {{
        let mut o = $crate::js::Object::new();
        $( o.set($k, $crate::js::Value::from($v)); )+
        $crate::js::Value::Object(o)
    }};
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::String(s.to_string())
    }
}
impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::String(s)
    }
}
impl From<&String> for Value {
    fn from(s: &String) -> Self {
        Value::String(s.clone())
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
impl From<i64> for Value {
    fn from(n: i64) -> Self {
        Value::Number(n as f64)
    }
}
impl From<i32> for Value {
    fn from(n: i32) -> Self {
        Value::Number(n as f64)
    }
}
impl From<u32> for Value {
    fn from(n: u32) -> Self {
        Value::Number(n as f64)
    }
}
impl From<usize> for Value {
    fn from(n: usize) -> Self {
        Value::Number(n as f64)
    }
}
impl From<Object> for Value {
    fn from(o: Object) -> Self {
        Value::Object(o)
    }
}
impl From<Vec<Value>> for Value {
    fn from(v: Vec<Value>) -> Self {
        Value::Array(Arr::from(v))
    }
}
impl From<Arr> for Value {
    fn from(v: Arr) -> Self {
        Value::Array(v)
    }
}
impl From<Vec<String>> for Value {
    fn from(v: Vec<String>) -> Self {
        Value::Array(v.into_iter().map(Value::String).collect())
    }
}
impl From<Vec<&str>> for Value {
    fn from(v: Vec<&str>) -> Self {
        Value::Array(v.into_iter().map(Value::from).collect())
    }
}
impl From<Option<String>> for Value {
    fn from(v: Option<String>) -> Self {
        v.map(Value::String).unwrap_or(Value::Undefined)
    }
}
impl From<&Value> for Value {
    fn from(v: &Value) -> Self {
        v.clone()
    }
}

impl Value {
    pub fn str(s: impl Into<String>) -> Value {
        Value::String(s.into())
    }

    pub fn is_undefined(&self) -> bool {
        matches!(self, Value::Undefined)
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// `v == null` (undefined or null)
    pub fn is_nullish(&self) -> bool {
        matches!(self, Value::Undefined | Value::Null)
    }

    pub fn is_string(&self) -> bool {
        matches!(self, Value::String(_))
    }

    pub fn is_array(&self) -> bool {
        matches!(self, Value::Array(_))
    }

    /// `typeof v === 'object' && v !== null` (arrays and dates included)
    pub fn is_object_like(&self) -> bool {
        matches!(self, Value::Object(_) | Value::Array(_) | Value::Date(_))
    }

    /// A plain object (not an array).
    pub fn is_plain_object(&self) -> bool {
        matches!(self, Value::Object(_))
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    /// `Value::Array` from a plain vector (a new array identity).
    pub fn array(items: Vec<Value>) -> Value {
        Value::Array(Arr::from(items))
    }

    pub fn as_array(&self) -> Option<&Vec<Value>> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_array_mut(&mut self) -> Option<&mut Vec<Value>> {
        match self {
            Value::Array(a) => Some(&mut *a),
            _ => None,
        }
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

    pub fn as_number(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn typeof_(&self) -> &'static str {
        match self {
            Value::Undefined => "undefined",
            Value::Null => "object",
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) | Value::Object(_) | Value::Date(_) => "object",
        }
    }

    /// ToBoolean
    pub fn truthy(&self) -> bool {
        match self {
            Value::Undefined | Value::Null => false,
            Value::Bool(b) => *b,
            Value::Number(n) => !(*n == 0.0 || n.is_nan()),
            Value::String(s) => !s.is_empty(),
            Value::Array(_) | Value::Object(_) | Value::Date(_) => true,
        }
    }

    /// ToString (String(v), template literal interpolation)
    pub fn to_js_string(&self) -> String {
        match self {
            Value::Undefined => "undefined".into(),
            Value::Null => "null".into(),
            Value::Bool(b) => {
                if *b {
                    "true".into()
                } else {
                    "false".into()
                }
            }
            Value::Number(n) => js_number_to_string(*n),
            Value::String(s) => s.clone(),
            Value::Array(items) => items
                .iter()
                .map(|v| if v.is_nullish() { String::new() } else { v.to_js_string() })
                .collect::<Vec<_>>()
                .join(","),
            Value::Object(_) => "[object Object]".into(),
            Value::Date(t) => super::date::date_to_string(*t),
        }
    }

    /// ToNumber
    pub fn to_number(&self) -> f64 {
        match self {
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
            Value::String(s) => string_to_number(s),
            Value::Date(t) => *t,
            Value::Array(_) | Value::Object(_) => string_to_number(&self.to_js_string()),
        }
    }

    /// `a ?? b`
    pub fn or_nullish(self, fallback: impl FnOnce() -> Value) -> Value {
        if self.is_nullish() { fallback() } else { self }
    }

    /// `a || b`
    pub fn or_falsy(self, fallback: impl FnOnce() -> Value) -> Value {
        if self.truthy() { self } else { fallback() }
    }

    /// `a?.[key]` / `a?.key` for a string key: undefined when missing.
    pub fn get(&self, key: &str) -> &Value {
        match self {
            Value::Object(o) => o.get(key).unwrap_or(&UNDEFINED),
            Value::Array(a) => {
                if is_array_index(key) {
                    key.parse::<usize>().ok().and_then(|i| a.get(i)).unwrap_or(&UNDEFINED)
                } else {
                    &UNDEFINED
                }
            }
            _ => &UNDEFINED,
        }
    }

    /// Like [`Value::get`] but also exposes the computed properties of
    /// primitives that the original code relies on (`length`, string indices).
    pub fn get_computed(&self, key: &str) -> Value {
        match self {
            Value::String(s) => {
                let units: Vec<u16> = s.encode_utf16().collect();
                if key == "length" {
                    return Value::Number(units.len() as f64);
                }
                if is_array_index(key)
                    && let Ok(i) = key.parse::<usize>()
                    && i < units.len()
                {
                    return Value::String(String::from_utf16_lossy(&units[i..=i]));
                }
                Value::Undefined
            }
            Value::Array(a) if key == "length" => Value::Number(a.len() as f64),
            _ => self.get(key).clone(),
        }
    }

    /// `v.length` for strings (UTF-16 units) and arrays.
    pub fn length(&self) -> Option<usize> {
        match self {
            Value::String(s) => Some(s.encode_utf16().count()),
            Value::Array(a) => Some(a.len()),
            _ => None,
        }
    }

    /// `v.length` read as a property: objects answer with their own `length` key.
    pub fn length_prop(&self) -> Value {
        match self {
            Value::String(_) | Value::Array(_) => Value::Number(self.length().unwrap_or(0) as f64),
            Value::Object(o) => o.get("length").cloned().unwrap_or(Value::Undefined),
            _ => Value::Undefined,
        }
    }

    /// `Object.keys(v)` for any value (`[]` for primitives without own keys).
    pub fn object_keys(&self) -> Vec<String> {
        match self {
            Value::Object(o) => o.keys().into_iter().cloned().collect(),
            Value::Array(a) => (0..a.len()).map(|i| i.to_string()).collect(),
            Value::String(s) => (0..s.encode_utf16().count()).map(|i| i.to_string()).collect(),
            _ => Vec::new(),
        }
    }

    /// Own enumerable entries as used by object spread / `Object.entries`.
    pub fn own_entries(&self) -> Vec<(String, Value)> {
        match self {
            Value::Object(o) => o.entries().into_iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            Value::Array(a) => a.iter().enumerate().map(|(i, v)| (i.to_string(), v.clone())).collect(),
            Value::String(s) => {
                let units: Vec<u16> = s.encode_utf16().collect();
                units
                    .iter()
                    .enumerate()
                    .map(|(i, u)| (i.to_string(), Value::String(String::from_utf16_lossy(&[*u]))))
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    /// `{ ...v }`
    pub fn spread(&self) -> Object {
        self.own_entries().into_iter().collect()
    }

    /// `key in v` for objects (own properties only; prototype keys are ignored).
    pub fn has_own(&self, key: &str) -> bool {
        match self {
            Value::Object(o) => o.contains_key(key),
            Value::Array(a) => {
                key == "length" || (is_array_index(key) && key.parse::<usize>().map(|i| i < a.len()).unwrap_or(false))
            }
            _ => false,
        }
    }
}

/// `===`
pub fn strict_equals(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Undefined, Value::Undefined) | (Value::Null, Value::Null) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Number(x), Value::Number(y)) => x == y,
        (Value::String(x), Value::String(y)) => x == y,
        // Distinct object identities never compare equal after a copy; the
        // original code only compares primitives with `===`.
        _ => false,
    }
}

/// SameValueZero (Array.prototype.includes, Set membership)
pub fn same_value_zero(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x == y || (x.is_nan() && y.is_nan()),
        _ => strict_equals(a, b),
    }
}

/// `==` (only the primitive cases the converter can reach)
pub fn loose_equals(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Undefined | Value::Null, Value::Undefined | Value::Null) => true,
        (Value::Undefined | Value::Null, _) | (_, Value::Undefined | Value::Null) => false,
        (Value::Number(_), Value::Number(_))
        | (Value::String(_), Value::String(_))
        | (Value::Bool(_), Value::Bool(_)) => strict_equals(a, b),
        (Value::Number(x), Value::String(_)) => *x == b.to_number(),
        (Value::String(_), Value::Number(y)) => a.to_number() == *y,
        (Value::Bool(_), _) => loose_equals(&Value::Number(a.to_number()), b),
        (_, Value::Bool(_)) => loose_equals(a, &Value::Number(b.to_number())),
        (Value::Number(_) | Value::String(_), Value::Array(_) | Value::Object(_) | Value::Date(_)) => {
            loose_equals(a, &to_primitive(b))
        }
        (Value::Array(_) | Value::Object(_) | Value::Date(_), Value::Number(_) | Value::String(_)) => {
            loose_equals(&to_primitive(a), b)
        }
        _ => false,
    }
}

fn to_primitive(v: &Value) -> Value {
    match v {
        Value::Date(t) => Value::String(super::date::date_to_string(*t)),
        Value::Array(_) | Value::Object(_) => Value::String(v.to_js_string()),
        other => other.clone(),
    }
}

/// `Array.prototype.includes` with SameValueZero, or `String.prototype.includes`
/// when the receiver is a string (the original code relies on both).
pub fn includes(haystack: &Value, needle: &Value) -> bool {
    match haystack {
        Value::Array(items) => items.iter().any(|v| same_value_zero(v, needle)),
        Value::String(s) => s.contains(&needle.to_js_string()),
        _ => false,
    }
}

/// Utility to build `Value::Array` of strings.
pub fn str_array<I, S>(items: I) -> Value
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    Value::Array(items.into_iter().map(|s| Value::String(s.into())).collect())
}

/// Collects the string elements of an array value.
pub fn strings_of(v: &Value) -> Vec<String> {
    match v {
        Value::Array(items) => items.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
        _ => Vec::new(),
    }
}
