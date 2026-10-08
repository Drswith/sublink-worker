//! Surge INI parsing: `[Proxy]` lines, `[Proxy Group]` lines and the whole
//! INI → JSON conversion used for config overrides and base configs.

use crate::js::number::{parse_int, string_to_number};
use crate::js::string::{is_js_whitespace, is_word_char, js_trim};
use crate::js::{JsError, JsResult, Object, Value, json};
use crate::obj;

fn surge_bool(value: Option<&str>) -> bool {
    match value {
        None => false,
        Some(v) => matches!(v.to_lowercase().as_str(), "true" | "1" | "yes"),
    }
}

/// Surge `key=value` parameters (later keys win, first position kept).
struct SurgeParams(Vec<(String, String)>);

impl SurgeParams {
    fn parse(parts: &[&str]) -> Self {
        let mut v: Vec<(String, String)> = Vec::new();
        for part in parts {
            let trimmed = js_trim(part);
            if let Some(eq) = trimmed.find('=')
                && eq > 0
            {
                let key = js_trim(&trimmed[..eq]).to_string();
                let value = js_trim(&trimmed[eq + 1..]).to_string();
                if let Some(slot) = v.iter_mut().find(|(k, _)| *k == key) {
                    slot.1 = value;
                } else {
                    v.push((key, value));
                }
            }
        }
        SurgeParams(v)
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    fn val(&self, key: &str) -> Value {
        self.get(key).map(Value::str).unwrap_or(Value::Undefined)
    }

    /// First non-empty value among `keys`.
    fn any(&self, keys: &[&str]) -> Option<&str> {
        keys.iter().filter_map(|k| self.get(k)).find(|v| !v.is_empty())
    }

    /// `params.a || params.b || ...` (evaluates to the last operand when all are falsy)
    fn or_chain(&self, keys: &[&str]) -> Value {
        match self.any(keys) {
            Some(v) => Value::str(v),
            None => self.val(keys[keys.len() - 1]),
        }
    }
}

fn alpn_of(params: &SurgeParams) -> Value {
    match params.get("alpn").filter(|a| !a.is_empty()) {
        Some(a) => Value::Array(a.split(',').map(|x| Value::str(js_trim(x))).collect()),
        None => Value::Undefined,
    }
}

fn tls_of(params: &SurgeParams, server: &str) -> Value {
    obj! {
        "enabled" => true,
        "server_name" => params.any(&["sni", "server-name"]).unwrap_or(server),
        "insecure" => surge_bool(params.get("skip-cert-verify")),
        "alpn" => alpn_of(params),
    }
}

/// `convertSurgeProxyToObject(line)`
pub fn convert_surge_proxy_to_object(line: &Value) -> Value {
    let Some(line) = line.as_str().filter(|l| !l.is_empty()) else { return Value::Null };
    let trimmed = js_trim(line);
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
        return Value::Null;
    }
    let Some(eq) = trimmed.find('=') else { return Value::Null };
    let tag = js_trim(&trimmed[..eq]).to_string();
    let rest = js_trim(&trimmed[eq + 1..]);
    let parts: Vec<&str> = rest.split(',').map(js_trim).collect();
    if parts.len() < 3 {
        return Value::Null;
    }
    let ty = parts[0].to_lowercase();
    let server = parts[1];
    let port = parse_int(parts[2], 0);
    if server.is_empty() || port.is_nan() {
        return Value::Null;
    }
    let params = SurgeParams::parse(&parts[3..]);
    let tfo = surge_bool(params.or_chain(&["tfo", "tcp-fast-open"]).as_str());
    let build_tls = || -> Value {
        if !surge_bool(params.get("tls")) {
            return Value::Undefined;
        }
        tls_of(&params, server)
    };
    let build_transport = || -> Value {
        let ws_path = params.or_chain(&["ws-path", "path"]);
        if params.get("ws") == Some("true") || ws_path.truthy() {
            let headers = match params.get("ws-headers").filter(|h| !h.is_empty()) {
                Some(h) => obj! { "host" => h },
                None => Value::Undefined,
            };
            return obj! { "type" => "ws", "path" => ws_path, "headers" => headers };
        }
        Value::Undefined
    };
    match ty.as_str() {
        "ss" | "shadowsocks" => obj! {
            "tag" => tag,
            "type" => "shadowsocks",
            "server" => server,
            "server_port" => port,
            "method" => params.or_chain(&["encrypt-method", "method", "cipher"]),
            "password" => params.val("password"),
            "network" => "tcp",
            "tcp_fast_open" => tfo,
        },
        "vmess" => {
            let alter_id = parse_int(&params.val("alterId").to_js_string(), 0);
            obj! {
                "tag" => tag,
                "type" => "vmess",
                "server" => server,
                "server_port" => port,
                "uuid" => params.or_chain(&["username", "uuid"]),
                "alter_id" => if alter_id == 0.0 || alter_id.is_nan() { 0.0 } else { alter_id },
                "security" => params.any(&["cipher", "security"]).unwrap_or("auto"),
                "network" => "tcp",
                "tcp_fast_open" => tfo,
                "tls" => build_tls(),
                "transport" => build_transport(),
            }
        }
        "trojan" => obj! {
            "tag" => tag,
            "type" => "trojan",
            "server" => server,
            "server_port" => port,
            "password" => params.val("password"),
            "network" => "tcp",
            "tcp_fast_open" => tfo,
            "tls" => tls_of(&params, server),
            "transport" => build_transport(),
        },
        "tuic" => obj! {
            "tag" => tag,
            "type" => "tuic",
            "server" => server,
            "server_port" => port,
            "uuid" => params.val("uuid"),
            "password" => params.val("password"),
            "congestion_control" => params.or_chain(&["congestion-controller", "congestion_control"]),
            "udp_relay_mode" => params.val("udp-relay-mode"),
            "tls" => tls_of(&params, server),
        },
        "hysteria2" | "hy2" => {
            let obfs = match params.get("obfs-password").filter(|p| !p.is_empty()) {
                Some(pw) => obj! { "type" => params.any(&["obfs"]).unwrap_or("salamander"), "password" => pw },
                None => Value::Undefined,
            };
            obj! {
                "tag" => tag,
                "type" => "hysteria2",
                "server" => server,
                "server_port" => port,
                "password" => params.val("password"),
                "tls" => tls_of(&params, server),
                "obfs" => obfs,
            }
        }
        "http" | "https" | "direct" | "reject" | "reject-tinygif" => Value::Null,
        other => {
            eprintln!("Unsupported Surge proxy type: {}", other);
            Value::Null
        }
    }
}

fn parse_surge_value(raw: &str) -> Value {
    let trimmed = js_trim(raw);
    if trimmed.is_empty() {
        return Value::str("");
    }
    let unquoted = if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };
    match unquoted.to_lowercase().as_str() {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        _ => {}
    }
    if is_numeric_literal(unquoted) {
        return Value::Number(string_to_number(unquoted));
    }
    Value::str(unquoted)
}

/// `/^-?\d+(\.\d+)?$/`
fn is_numeric_literal(s: &str) -> bool {
    let s = s.strip_prefix('-').unwrap_or(s);
    let (int, frac) = match s.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (s, None),
    };
    !int.is_empty()
        && int.bytes().all(|b| b.is_ascii_digit())
        && frac.is_none_or(|f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()))
}

/// `convertSurgeIniToJson(content)`
pub fn convert_surge_ini_to_json(content: &str) -> JsResult<Value> {
    let mut config = Object::new();
    let mut current_section: Option<String> = None;
    for raw_line in content.split('\n') {
        let raw_line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let line = js_trim(raw_line);
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') && line.chars().count() > 2 && !line_has_terminator(line) {
            current_section = Some(js_trim(&line[1..line.len() - 1]).to_string());
            continue;
        }
        let Some(section) = &current_section else { continue };
        let section_name = section.to_lowercase();
        if section_name == "general" || section_name == "replica" {
            let Some(eq) = line.find('=') else { continue };
            let key = js_trim(&line[..eq]).to_string();
            let value = js_trim(&line[eq + 1..]);
            if key.is_empty() {
                continue;
            }
            if !config.get(&section_name).is_some_and(Value::truthy) {
                config.set(section_name.clone(), Value::Object(Object::new()));
            }
            if let Some(Value::Object(target)) = config.get_mut(&section_name) {
                target.set(key, parse_surge_value(value));
            }
        } else {
            let key = match section_name.as_str() {
                "proxy" => "proxies".to_string(),
                "proxy group" => "proxy-groups".to_string(),
                "rule" => "rules".to_string(),
                other => other.to_string(),
            };
            if !config.get(&key).is_some_and(Value::truthy) {
                config.set(key.clone(), Value::array(Vec::new()));
            }
            if let Some(Value::Array(items)) = config.get_mut(&key) {
                items.push(Value::str(line));
            }
        }
    }
    let has = |k: &str| config.get(k).is_some_and(Value::truthy);
    if !has("general") && !has("replica") && !has("proxies") && !has("proxy-groups") {
        return Err(JsError::error("Unable to parse Surge INI content"));
    }
    Ok(Value::Object(config))
}

/// `.` in `/^\[(.+)]$/` does not match line terminators.
fn line_has_terminator(s: &str) -> bool {
    s.chars().any(|c| matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
}

/// `parseSurgeConfigInput(content)` → (configObject, convertedFromIni)
pub fn parse_surge_config_input(content: &str) -> JsResult<(Value, bool)> {
    let trimmed = js_trim(content);
    if trimmed.is_empty() {
        return Err(JsError::error("Config content is empty"));
    }
    match json::parse(trimmed) {
        Ok(v) => Ok((v, false)),
        Err(_) => Ok((convert_surge_ini_to_json(content)?, true)),
    }
}

fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// Matches `\s*=\s*(\w+[-\w]*)(?:,\s*(.*))?$` against `rest`.
fn match_group_suffix(rest: &[char]) -> Option<(String, Option<String>)> {
    let mut i = 0;
    while i < rest.len() && is_js_whitespace(rest[i]) {
        i += 1;
    }
    if rest.get(i) != Some(&'=') {
        return None;
    }
    i += 1;
    while i < rest.len() && is_js_whitespace(rest[i]) {
        i += 1;
    }
    let start = i;
    if !rest.get(i).is_some_and(|c| is_word_char(*c)) {
        return None;
    }
    while i < rest.len() && (is_word_char(rest[i]) || rest[i] == '-') {
        i += 1;
    }
    let ty: String = rest[start..i].iter().collect();
    if i == rest.len() {
        return Some((ty, None));
    }
    if rest[i] != ',' {
        return None;
    }
    i += 1;
    while i < rest.len() && is_js_whitespace(rest[i]) {
        i += 1;
    }
    let tail = &rest[i..];
    // `(.*)$` can never span a line terminator
    if tail.iter().any(|c| is_line_terminator(*c)) {
        return None;
    }
    Some((ty, Some(tail.iter().collect())))
}

/// `parseSurgeProxyGroupLine(line)` → Clash-style group object.
pub fn parse_surge_proxy_group_line(line: &Value) -> Value {
    let Some(line) = line.as_str().filter(|l| !l.is_empty()) else { return Value::Null };
    let chars: Vec<char> = line.chars().collect();
    let mut found: Option<(String, String, Option<String>)> = None;
    for split in 1..=chars.len() {
        if is_line_terminator(chars[split - 1]) {
            break;
        }
        if let Some((ty, rest)) = match_group_suffix(&chars[split..]) {
            found = Some((chars[..split].iter().collect(), ty, rest));
            break;
        }
    }
    let Some((name, ty, rest)) = found else { return Value::Null };
    let rest = rest.unwrap_or_default();
    let parts = split_comma_ws(&rest);
    let mut proxies: Vec<Value> = Vec::new();
    let mut extras: Vec<(String, String)> = Vec::new();
    for part in parts.iter().filter(|p| !js_trim(p).is_empty()) {
        let trimmed = js_trim(part);
        if let Some(eq) = trimmed.find('=') {
            let key = js_trim(&trimmed[..eq]).to_string();
            let value = js_trim(&trimmed[eq + 1..]).to_string();
            if let Some(slot) = extras.iter_mut().find(|(k, _)| *k == key) {
                slot.1 = value;
            } else {
                extras.push((key, value));
            }
        } else if !trimmed.is_empty() {
            proxies.push(Value::str(trimmed));
        }
    }
    let extra = |k: &str| extras.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone()).filter(|v| !v.is_empty());
    let mut group = Object::new();
    group.set("name", Value::str(js_trim(&name)));
    group.set("type", Value::str(if ty.to_lowercase() == "url-test" { "url-test" } else { "select" }));
    group.set("proxies", Value::array(proxies));
    if let Some(url) = extra("url") {
        group.set("url", Value::String(url));
    }
    if let Some(interval) = extra("interval") {
        let n = parse_int(&interval, 0);
        group.set("interval", Value::Number(if n == 0.0 || n.is_nan() { 300.0 } else { n }));
    }
    Value::Object(group)
}

/// `str.split(/,\s*/)`
fn split_comma_ws(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == ',' {
            out.push(std::mem::take(&mut cur));
            while chars.peek().is_some_and(|n| is_js_whitespace(*n)) {
                chars.next();
            }
        } else {
            cur.push(c);
        }
    }
    out.push(cur);
    out
}
