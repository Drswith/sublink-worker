//! `subscriptionContentParser.js`: detect and parse whole subscription bodies
//! (Sing-Box JSON, Clash YAML, Surge INI, or line lists).

use super::clash_proxy::convert_yaml_proxy_to_object;
use super::surge::{convert_surge_ini_to_json, convert_surge_proxy_to_object, parse_surge_proxy_group_line};
use crate::js::number::parse_int;
use crate::js::string::js_trim;
use crate::js::{Object, Value, deep_copy, delete_prop, json};
use crate::yaml;

fn is_non_proxy_type(t: &Value) -> bool {
    matches!(t.as_str(), Some("direct" | "block" | "dns" | "selector" | "urltest"))
}

fn is_group_type(t: &Value) -> bool {
    matches!(t.as_str(), Some("selector" | "urltest"))
}

fn parse_interval(interval: &Value) -> Value {
    match interval {
        Value::Number(_) => interval.clone(),
        Value::String(s) => {
            let digits = s.bytes().take_while(u8::is_ascii_digit).count();
            let unit = &s[digits..];
            if digits > 0 && matches!(unit, "" | "s" | "m" | "h") {
                let value = parse_int(&s[..digits], 0);
                return Value::Number(match unit {
                    "h" => value * 3600.0,
                    "m" => value * 60.0,
                    _ => value,
                });
            }
            let n = parse_int(s, 0);
            Value::Number(if n == 0.0 || n.is_nan() { 300.0 } else { n })
        }
        _ => Value::Number(300.0),
    }
}

fn convert_singbox_group_to_clash(outbound: &Value) -> Value {
    if !outbound.truthy() || !outbound.get("tag").truthy() || !outbound.get("type").truthy() {
        return Value::Null;
    }
    let mut group = Object::new();
    group.set("name", outbound.get("tag").clone());
    group
        .set("type", Value::str(if outbound.get("type").as_str() == Some("selector") { "select" } else { "url-test" }));
    group.set("proxies", outbound.get("outbounds").clone().or_falsy(|| Value::array(Vec::new())));
    if outbound.get("type").as_str() == Some("urltest") {
        group.set("url", outbound.get("url").clone().or_falsy(|| Value::str("http://www.gstatic.com/generate_204")));
        let interval = outbound.get("interval");
        group.set("interval", if interval.truthy() { parse_interval(interval) } else { Value::Number(300.0) });
    }
    Value::Object(group)
}

fn config_result(ty: &str, proxies: Vec<Value>, overrides: Value) -> Value {
    let config = if overrides.as_object().is_some_and(|o| !o.is_empty()) { overrides } else { Value::Null };
    crate::obj! { "type" => ty, "proxies" => Value::array(proxies), "config" => config }
}

/// `parseSingboxJson(content)`
pub fn parse_singbox_json(content: &str) -> Value {
    let Ok(parsed) = json::parse(content) else { return Value::Null };
    if !(parsed.truthy() && parsed.is_object_like()) {
        return Value::Null;
    }
    let Some(outbounds) = parsed.get("outbounds").as_array() else { return Value::Null };
    let proxies: Vec<Value> = outbounds
        .iter()
        .filter(|o| {
            o.truthy()
                && o.is_object_like()
                && o.get("server").truthy()
                && o.get("type").truthy()
                && !is_non_proxy_type(o.get("type"))
        })
        .cloned()
        .collect();
    if proxies.is_empty() {
        return Value::Null;
    }
    let mut overrides = deep_copy(&parsed);
    delete_prop(&mut overrides, "outbounds");
    let groups: Vec<Value> = outbounds
        .iter()
        .filter(|o| o.truthy() && is_group_type(o.get("type")))
        .map(convert_singbox_group_to_clash)
        .filter(|g| !g.is_nullish())
        .collect();
    if !groups.is_empty()
        && let Some(o) = overrides.as_object_mut()
    {
        o.set("proxy-groups", Value::array(groups));
    }
    config_result("singboxConfig", proxies, overrides)
}

/// `parseClashYaml(content)`
pub fn parse_clash_yaml(content: &str) -> Value {
    let Ok(parsed) = yaml::load(content) else { return Value::Null };
    if !(parsed.truthy() && parsed.is_object_like()) {
        return Value::Null;
    }
    let Some(items) = parsed.get("proxies").as_array() else { return Value::Null };
    let proxies: Vec<Value> = items.iter().map(convert_yaml_proxy_to_object).filter(|p| !p.is_nullish()).collect();
    if proxies.is_empty() {
        return Value::Null;
    }
    let mut overrides = deep_copy(&parsed);
    delete_prop(&mut overrides, "proxies");
    config_result("yamlConfig", proxies, overrides)
}

fn contains_section_ci(content: &str, section: &str) -> bool {
    content.to_ascii_lowercase().contains(&format!("[{}]", section.to_ascii_lowercase()))
}

/// `parseSurgeIni(content)`
pub fn parse_surge_ini(content: &str) -> Value {
    let has_surge_section = contains_section_ci(content, "Proxy")
        || (contains_section_ci(content, "General") && contains_section_ci(content, "Rule"));
    if !has_surge_section {
        return Value::Null;
    }
    let parsed = match convert_surge_ini_to_json(content) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Surge INI parsing failed: {}", e.message);
            return Value::Null;
        }
    };
    let Some(lines) = parsed.get("proxies").as_array().filter(|l| !l.is_empty()) else { return Value::Null };
    let proxies: Vec<Value> = lines.iter().map(convert_surge_proxy_to_object).filter(|p| !p.is_nullish()).collect();
    if proxies.is_empty() {
        return Value::Null;
    }
    let mut overrides = deep_copy(&parsed);
    delete_prop(&mut overrides, "proxies");
    let groups: Vec<Value> = match parsed.get("proxy-groups").as_array() {
        Some(lines) if !lines.is_empty() => {
            lines.iter().map(parse_surge_proxy_group_line).filter(|g| !g.is_nullish()).collect()
        }
        _ => Vec::new(),
    };
    if groups.is_empty() {
        delete_prop(&mut overrides, "proxy-groups");
    } else if let Some(o) = overrides.as_object_mut() {
        o.set("proxy-groups", Value::array(groups));
    }
    config_result("surgeConfig", proxies, overrides)
}

/// `parseSubscriptionContent(content)`: a `{type, proxies, config}` object or
/// an array of non-empty lines.
pub fn parse_subscription_content(content: &str) -> Value {
    let trimmed = js_trim(content);
    if trimmed.is_empty() {
        return Value::array(Vec::new());
    }
    let singbox = parse_singbox_json(trimmed);
    if !singbox.is_nullish() {
        return singbox;
    }
    let clash = parse_clash_yaml(trimmed);
    if !clash.is_nullish() {
        return clash;
    }
    let surge = parse_surge_ini(trimmed);
    if !surge.is_nullish() {
        return surge;
    }
    Value::Array(trimmed.split('\n').filter(|l| !js_trim(l).is_empty()).map(Value::str).collect())
}

/// Whether a parsed value is one of the full-config results.
pub fn is_config_result(v: &Value) -> bool {
    matches!(v.get("type").as_str(), Some("yamlConfig" | "singboxConfig" | "surgeConfig")) && v.is_plain_object()
}
