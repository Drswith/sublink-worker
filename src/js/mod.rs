//! JavaScript semantics needed to reproduce the original implementation.

pub mod base64;
pub mod date;
pub mod json;
pub mod number;
pub mod string;
pub mod value;

pub use value::{
    Arr, Object, UNDEFINED, Value, includes, loose_equals, same_value_zero, str_array, strict_equals, strings_of,
};

use std::fmt;

/// A thrown JS error. `message` is what `error.message` would contain.
#[derive(Clone, Debug, PartialEq)]
pub struct JsError {
    pub kind: ErrorKind,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ErrorKind {
    Error,
    TypeError,
    SyntaxError,
    UriError,
    RangeError,
    /// A `ServiceError` subclass carrying its HTTP status (e.g. InvalidConfigError → 400).
    Service(u16),
}

impl JsError {
    pub fn error(msg: impl Into<String>) -> Self {
        JsError { kind: ErrorKind::Error, message: msg.into() }
    }
    pub fn type_error(msg: impl Into<String>) -> Self {
        JsError { kind: ErrorKind::TypeError, message: msg.into() }
    }
    pub fn syntax(msg: impl Into<String>) -> Self {
        JsError { kind: ErrorKind::SyntaxError, message: msg.into() }
    }
    pub fn uri(msg: impl Into<String>) -> Self {
        JsError { kind: ErrorKind::UriError, message: msg.into() }
    }
    pub fn service(status: u16, msg: impl Into<String>) -> Self {
        JsError { kind: ErrorKind::Service(status), message: msg.into() }
    }

    /// `Cannot read properties of undefined (reading 'key')`
    pub fn read_prop(base: &Value, key: &str) -> Self {
        JsError::type_error(format!("Cannot read properties of {} (reading '{}')", nullish_name(base), key))
    }

    /// `Cannot set properties of undefined (setting 'key')`
    pub fn set_prop(base: &Value, key: &str) -> Self {
        JsError::type_error(format!("Cannot set properties of {} (setting '{}')", nullish_name(base), key))
    }

    /// `<expr> is not a function`
    pub fn not_function(expr: &str) -> Self {
        JsError::type_error(format!("{} is not a function", expr))
    }

    /// `<expr> is not iterable`
    pub fn not_iterable(expr: &str) -> Self {
        JsError::type_error(format!("{} is not iterable", expr))
    }
}

fn nullish_name(v: &Value) -> &'static str {
    if v.is_null() { "null" } else { "undefined" }
}

impl fmt::Display for JsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for JsError {}

pub type JsResult<T> = Result<T, JsError>;

/// Property read that throws like JS on `undefined` / `null` receivers.
pub fn prop<'a>(base: &'a Value, key: &str) -> JsResult<&'a Value> {
    if base.is_nullish() {
        return Err(JsError::read_prop(base, key));
    }
    Ok(base.get(key))
}

/// Property read returning computed values for primitives (`str.length`).
pub fn prop_computed(base: &Value, key: &str) -> JsResult<Value> {
    if base.is_nullish() {
        return Err(JsError::read_prop(base, key));
    }
    Ok(base.get_computed(key))
}

/// `base[key] = value` with JS failure semantics (primitives silently ignore
/// the write; null/undefined throw).
pub fn set_prop(base: &mut Value, key: &str, value: Value) -> JsResult<()> {
    match base {
        Value::Object(o) => {
            o.set(key, value);
            Ok(())
        }
        Value::Array(items) => {
            if let Ok(i) = key.parse::<usize>()
                && value::is_array_index(key)
            {
                if i >= items.len() {
                    items.resize(i + 1, Value::Undefined);
                }
                items[i] = value;
            }
            Ok(())
        }
        Value::Undefined | Value::Null => Err(JsError::set_prop(base, key)),
        // ES modules run in strict mode, where writes to primitives throw.
        Value::Bool(_) | Value::Number(_) | Value::String(_) => Err(JsError::type_error(format!(
            "Cannot create property '{}' on {} '{}'",
            key,
            base.typeof_(),
            base.to_js_string()
        ))),
        Value::Date(_) => Ok(()),
    }
}

/// Deletes an own property (`delete base[key]`), a no-op for non-objects.
pub fn delete_prop(base: &mut Value, key: &str) {
    if let Value::Object(o) = base {
        o.remove(key);
    }
}

/// `deepCopy` from the original utils: plain objects and arrays are copied
/// recursively, other objects (e.g. Dates) lose their identity and become `{}`.
pub fn deep_copy(v: &Value) -> Value {
    match v {
        Value::Array(items) => Value::Array(items.iter().map(deep_copy).collect()),
        Value::Object(o) => Value::Object(o.entries().into_iter().map(|(k, v)| (k.clone(), deep_copy(v))).collect()),
        Value::Date(_) => Value::Object(Object::new()),
        other => other.clone(),
    }
}
