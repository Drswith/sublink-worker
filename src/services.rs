//! Short links and stored base configs on top of the embedded KV store.

use crate::js::{JsError, JsResult, Value, json};
use crate::storage::Store;
use crate::utils::generate_web_path;
use crate::yaml;

/// Default short-code length (`PATH_LENGTH`).
const SHORT_CODE_LENGTH: usize = 7;

fn storage_error(message: String) -> JsError {
    JsError::error(message)
}

/// `ShortLinkService.createShortLink(queryString, providedCode)`
pub fn create_short_link(
    store: &Store,
    query_string: &str,
    provided_code: Option<&str>,
    ttl_seconds: Option<f64>,
) -> JsResult<String> {
    let code = match provided_code {
        Some(c) if !c.is_empty() => c.to_string(),
        _ => generate_web_path(SHORT_CODE_LENGTH),
    };
    store.put(&code, query_string, ttl_seconds).map_err(storage_error)?;
    Ok(code)
}

/// `ShortLinkService.resolveShortCode(code)`
pub fn resolve_short_code(store: &Store, code: &str) -> JsResult<Option<String>> {
    store.get(code).map_err(storage_error)
}

/// `ConfigStorageService.getConfigById(configId)`; `None` mirrors a `null` return.
pub fn get_config_by_id(store: &Store, config_id: &str) -> JsResult<Value> {
    match store.get(config_id).map_err(storage_error)? {
        Some(stored) if !stored.is_empty() => {
            json::parse(&stored).map_err(|_| JsError::service(400, "Stored config is not valid JSON"))
        }
        _ => Ok(Value::Null),
    }
}

/// `ConfigStorageService.serializeConfig(type, content)`; the result may be a
/// non-string, which later gets coerced exactly like the original did.
fn serialize_config(kind: &Value, content: &Value) -> JsResult<Value> {
    let stringify = |v: &Value| json::stringify(v).map(Value::String).unwrap_or(Value::Undefined);
    if kind.as_str() == Some("clash") {
        if let Some(text) = content.as_str()
            && (crate::js::string::js_trim(text).starts_with('-') || text.contains(':'))
        {
            let loaded = yaml::load(text).map_err(|e| JsError::error(e.message))?;
            return Ok(stringify(&loaded));
        }
        return Ok(if content.typeof_() == "object" { stringify(content) } else { content.clone() });
    }
    match content {
        _ if content.typeof_() == "object" => Ok(stringify(content)),
        Value::String(_) => Ok(content.clone()),
        _ => Err(JsError::service(400, "Unsupported config content type")),
    }
}

/// `ConfigStorageService.saveConfig(type, content)`; JSON syntax errors keep
/// their `SyntaxError` kind so the route can answer 400.
pub fn save_config(store: &Store, kind: &Value, content: &Value, ttl_seconds: Option<f64>) -> JsResult<String> {
    if !kind.truthy() {
        return Err(JsError::service(400, "Missing config type"));
    }
    let config_id = format!("{}_{}", kind.to_js_string(), generate_web_path(8));
    let config_string = serialize_config(kind, content)?.to_js_string();
    json::parse(&config_string)?;
    store.put(&config_id, &config_string, ttl_seconds).map_err(storage_error)?;
    Ok(config_id)
}
