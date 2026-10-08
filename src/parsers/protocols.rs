//! Share-link parsers (`ss://`, `vmess://`, `vless://`, `trojan://`,
//! `hysteria2://`, `tuic://`, `anytls://`), ported from `parsers/protocols/*`.
//! Each returns the internal (sing-box shaped) proxy object.

use crate::js::base64::{base64_to_binary, decode_base64};
use crate::js::number::parse_int;
use crate::js::string::{decode_uri_component, js_trim};
use crate::js::{JsError, JsResult, Object, Value, json, prop};
use crate::obj;
use crate::utils::{
    Params, create_tls_config, create_transport_config, parse_array, parse_bool, parse_maybe_number, parse_server_info,
    parse_url_params,
};

/// `str.replace(pattern, '')` for a string pattern (first occurrence only).
fn replace_first(s: &str, pat: &str) -> String {
    s.replacen(pat, "", 1)
}

/// `const [a, b] = s.split(sep)`
fn split_two<'a>(s: &'a str, sep: &str) -> (&'a str, Option<&'a str>) {
    let mut it = s.split(sep);
    (it.next().unwrap_or(""), it.next())
}

fn opt_str(v: Option<&str>) -> Value {
    v.map(Value::str).unwrap_or(Value::Undefined)
}

// ---------------------------------------------------------------------------
// Shadowsocks
// ---------------------------------------------------------------------------

/// `serverPart.match(/\[([^\]]+)\]:(\d+)/)` falling back to `split(':')`.
fn parse_ss_server(server_part: &str) -> (Value, Value) {
    let chars: Vec<char> = server_part.chars().collect();
    for start in 0..chars.len() {
        if chars[start] != '[' {
            continue;
        }
        let mut j = start + 1;
        while j < chars.len() && chars[j] != ']' {
            j += 1;
        }
        if j == start + 1 || j >= chars.len() {
            continue;
        }
        if chars.get(j + 1) == Some(&':') {
            let mut k = j + 2;
            while k < chars.len() && chars[k].is_ascii_digit() {
                k += 1;
            }
            if k > j + 2 {
                let host: String = chars[start + 1..j].iter().collect();
                let port: String = chars[j + 2..k].iter().collect();
                return (Value::String(host), Value::String(port));
            }
        }
    }
    let mut parts = server_part.split(':');
    (opt_str(parts.next()), opt_str(parts.next()))
}

fn parse_plugin_string(plugin: &str) -> Option<(String, Option<Object>)> {
    let parts: Vec<&str> = plugin.split(';').collect();
    let name = parts[0];
    if name.is_empty() {
        return None;
    }
    let mut opts = Object::new();
    for part in &parts[1..] {
        match part.find('=') {
            None => {
                let key = js_trim(part);
                if !key.is_empty() {
                    opts.set(key, Value::Bool(true));
                }
            }
            Some(eq) => {
                let key = &part[..eq];
                let value = &part[eq + 1..];
                if !key.is_empty() {
                    let mapped = match key {
                        "obfs" => "mode",
                        "obfs-host" => "host",
                        "obfs-uri" => "path",
                        other => other,
                    };
                    opts.set(mapped, Value::str(value));
                }
            }
        }
    }
    let normalized = if name == "simple-obfs" { "obfs" } else { name };
    Some((normalized.to_string(), if opts.is_empty() { None } else { Some(opts) }))
}

fn ss_config(
    tag: &Value,
    server: Value,
    port: Value,
    method: Value,
    password: Value,
    plugin: &Option<(String, Option<Object>)>,
) -> Value {
    let mut c = Object::new();
    c.set("tag", if tag.truthy() { tag.clone() } else { Value::str("Shadowsocks") });
    c.set("type", Value::str("shadowsocks"));
    c.set("server", server);
    c.set("server_port", Value::Number(parse_int(&port.to_js_string(), 0)));
    c.set("method", method);
    c.set("password", password);
    c.set("tcp_fast_open", Value::Bool(false));
    if let Some((name, opts)) = plugin {
        c.set("plugin", Value::str(name));
        if let Some(opts) = opts {
            c.set("plugin_opts", Value::Object(opts.clone()));
        }
    }
    Value::Object(c)
}

pub fn parse_shadowsocks(url: &str) -> JsResult<Value> {
    let stripped = replace_first(url, "ss://");
    let mut hash_parts = stripped.split('#');
    let mut main_part = hash_parts.next().unwrap_or("").to_string();
    let mut tag = opt_str(hash_parts.next());
    if let Value::String(t) = &tag
        && t.contains('%')
    {
        tag = Value::String(decode_uri_component(t)?);
    }
    let mut query = String::new();
    if let Some(q) = main_part.find('?') {
        query = main_part[q + 1..].to_string();
        main_part.truncate(q);
    }
    let mut plugin = None;
    if !query.is_empty() {
        let params = Params::parse(&query);
        if let Some(p) = params.first("plugin").filter(|p| !p.is_empty()) {
            plugin = parse_plugin_string(p);
        }
    }
    // Everything below sat inside the original try/catch, which returned null.
    let parsed = (|| -> Option<Value> {
        let (base64, server_part) = split_two(&main_part, "@");
        match server_part.filter(|s| !s.is_empty()) {
            None => {
                let decoded = base64_to_binary(&main_part);
                let (method_and_pass, server_info) = split_two(&decoded, "@");
                let (method, password) = split_two(method_and_pass, ":");
                let (server, port) = parse_ss_server(server_info?);
                Some(ss_config(&tag, server, port, Value::str(method), opt_str(password), &plugin))
            }
            Some(server_part) => {
                let decoded = base64_to_binary(&decode_uri_component(base64).ok()?);
                let mut parts = decoded.split(':');
                let method = parts.next().unwrap_or("").to_string();
                let password = parts.collect::<Vec<_>>().join(":");
                let (server, port) = parse_ss_server(server_part);
                Some(ss_config(&tag, server, port, Value::String(method), Value::String(password), &plugin))
            }
        }
    })();
    Ok(parsed.unwrap_or(Value::Null))
}

// ---------------------------------------------------------------------------
// VMess
// ---------------------------------------------------------------------------

fn normalize_array(value: &Value) -> Option<Vec<Value>> {
    if !value.truthy() {
        return None;
    }
    Some(match value {
        Value::Array(items) => items.to_vec(),
        other => vec![other.clone()],
    })
}

fn build_http_headers(cfg: &Value) -> Value {
    let host_header = normalize_array(&cfg.get("host").clone().or_falsy(|| cfg.get("sni").clone()));
    let headers = cfg.get("headers");
    if headers.truthy() && headers.is_object_like() {
        let mut normalized = Object::new();
        for (key, value) in headers.own_entries() {
            if let Some(items) = normalize_array(&value) {
                let strings: Vec<Value> = items.iter().map(|e| Value::String(e.to_js_string())).collect();
                if !strings.is_empty() {
                    normalized.set(key, Value::array(strings));
                }
            }
        }
        if let Some(h) = &host_header
            && !normalized.get("host").is_some_and(Value::truthy)
        {
            normalized.set("host", Value::array(h.clone()));
        }
        if !normalized.is_empty() {
            return Value::Object(normalized);
        }
    }
    match host_header {
        Some(h) => obj! { "host" => Value::array(h) },
        None => Value::Undefined,
    }
}

pub fn parse_vmess(url: &str) -> JsResult<Value> {
    let mut base64 = replace_first(url, "vmess://");
    let mut tag_override = Value::Undefined;
    if let Some(hash) = base64.find('#') {
        tag_override = Value::String(decode_uri_component(&base64[hash + 1..])?);
        base64.truncate(hash);
    }
    let cfg = json::parse(&decode_base64(&base64))?;
    let net = prop(&cfg, "net")?.clone();
    let network_type = net.or_falsy(|| Value::str("tcp"));
    let transport_type = cfg.get("type").clone().or_falsy(|| network_type.clone());

    let tls_value = cfg.get("tls");
    let tls_enabled = tls_value.truthy() && tls_value.as_str() != Some("") && tls_value.as_str() != Some("none");
    let tls = if tls_enabled {
        obj! {
            "enabled" => true,
            "server_name" => cfg.get("sni"),
            "insecure" => cfg.get("skip-cert-verify").clone().or_falsy(|| Value::Bool(false)),
        }
    } else {
        Value::Undefined
    };

    let net_is = |s: &str| network_type.as_str() == Some(s);
    let transport = if net_is("ws") {
        let host = if cfg.get("host").truthy() { cfg.get("host").clone() } else { cfg.get("sni").clone() };
        obj! { "type" => "ws", "path" => cfg.get("path"), "headers" => obj! { "host" => host } }
    } else if (net_is("tcp") && transport_type.as_str() == Some("http")) || net_is("http") {
        let method = cfg.get("method").clone().or_falsy(|| Value::str("GET"));
        let path = cfg.get("path").clone().or_falsy(|| Value::str("/"));
        let path = if path.is_array() { path } else { Value::array(vec![path]) };
        obj! { "type" => "http", "method" => method, "path" => path, "headers" => build_http_headers(&cfg) }
    } else if net_is("grpc") {
        obj! { "type" => "grpc", "service_name" => cfg.get("path").clone().or_falsy(|| cfg.get("serviceName").clone()) }
    } else if net_is("h2") {
        let host_value = cfg.get("host").clone().or_falsy(|| cfg.get("sni").clone());
        let host = if host_value.truthy() {
            if host_value.is_array() { host_value } else { Value::array(vec![host_value]) }
        } else {
            Value::Undefined
        };
        obj! { "type" => "h2", "path" => cfg.get("path"), "host" => host }
    } else {
        Value::Undefined
    };

    let alter_id = parse_int(&cfg.get("aid").to_js_string(), 0);
    Ok(obj! {
        "tag" => tag_override.or_falsy(|| cfg.get("ps").clone()),
        "type" => "vmess",
        "server" => cfg.get("add"),
        "server_port" => parse_int(&cfg.get("port").to_js_string(), 0),
        "uuid" => cfg.get("id"),
        "alter_id" => if alter_id == 0.0 || alter_id.is_nan() { 0.0 } else { alter_id },
        "security" => cfg.get("scy").clone().or_falsy(|| Value::str("auto")),
        "tcp_fast_open" => false,
        "transport" => transport,
        "tls" => tls,
    })
}

// ---------------------------------------------------------------------------
// VLESS / Trojan / Hysteria2 / TUIC
// ---------------------------------------------------------------------------

pub fn parse_vless(url: &str) -> JsResult<Value> {
    let parts = parse_url_params(url)?;
    let (uuid, server_info) = split_two(&parts.address_part, "@");
    let (host, port) = parse_server_info(&opt_str(server_info));
    let params = &parts.params;
    let mut tls = create_tls_config(params);
    if tls.get("reality").truthy()
        && let Some(o) = tls.as_object_mut()
    {
        o.set("utls", obj! { "enabled" => true, "fingerprint" => "chrome" });
    }
    let transport = if params.get("type") != Some("tcp") { create_transport_config(params) } else { Value::Undefined };
    let udp = if params.has("udp") { parse_bool(&params.val("udp"), Value::Undefined) } else { Value::Undefined };
    let mut o = Object::new();
    o.set("type", Value::str("vless"));
    o.set("tag", Value::String(parts.name.clone()));
    o.set("server", host);
    o.set("server_port", port);
    o.set("uuid", Value::String(decode_uri_component(uuid)?));
    o.set("tcp_fast_open", Value::Bool(false));
    o.set("tls", tls);
    o.set("transport", transport);
    o.set("flow", params.val("flow"));
    if !udp.is_undefined() {
        o.set("udp", udp);
    }
    Ok(Value::Object(o))
}

pub fn parse_trojan(url: &str) -> JsResult<Value> {
    let parts = parse_url_params(url)?;
    let (password, server_info) = split_two(&parts.address_part, "@");
    let (host, port) = parse_server_info(&opt_str(server_info));
    let mut params = parts.params.clone();
    if !params.val("security").truthy() {
        params.set("security", "tls");
    }
    let tls = create_tls_config(&params);
    let transport = if params.get("type") != Some("tcp") { create_transport_config(&params) } else { Value::Undefined };
    let decoded = decode_uri_component(password)?;
    Ok(obj! {
        "type" => "trojan",
        "tag" => parts.name.clone(),
        "server" => host,
        "server_port" => port,
        "password" => if decoded.is_empty() { Value::Undefined } else { Value::String(decoded) },
        "tcp_fast_open" => false,
        "tls" => tls,
        "transport" => transport,
        "flow" => params.val("flow"),
    })
}

pub fn parse_hysteria2(url: &str) -> JsResult<Value> {
    let parts = parse_url_params(url)?;
    let mut params = parts.params.clone();
    let (host, port, password) = if parts.address_part.contains('@') {
        let (uuid, server_info) = split_two(&parts.address_part, "@");
        let (h, p) = parse_server_info(&opt_str(server_info));
        (h, p, Value::String(decode_uri_component(uuid)?))
    } else {
        let (h, p) = parse_server_info(&Value::String(parts.address_part.clone()));
        (h, p, params.val("auth"))
    };
    if !params.val("security").truthy() {
        params.set("security", "tls");
    }
    let tls = create_tls_config(&params);
    let mut obfs = Object::new();
    if params.val("obfs-password").truthy() {
        obfs.set("type", params.val("obfs"));
        obfs.set("password", params.val("obfs-password"));
    }
    let hop_interval = parse_maybe_number(&params.val("hop-interval").or_nullish(|| params.val("hop_interval")));
    let up = params.val("up").or_nullish(|| {
        let v = params.val("upmbps");
        if v.truthy() { parse_maybe_number(&v) } else { Value::Undefined }
    });
    let down = params.val("down").or_nullish(|| {
        let v = params.val("downmbps");
        if v.truthy() { parse_maybe_number(&v) } else { Value::Undefined }
    });
    Ok(obj! {
        "tag" => parts.name.clone(),
        "type" => "hysteria2",
        "server" => host,
        "server_port" => port,
        "password" => password,
        "tls" => tls,
        "obfs" => if obfs.is_empty() { Value::Undefined } else { Value::Object(obfs) },
        "auth" => params.val("auth"),
        "recv_window_conn" => params.val("recv_window_conn"),
        "up" => up,
        "down" => down,
        "ports" => params.either("mport", "ports"),
        "hop_interval" => hop_interval,
        "alpn" => parse_array(&params.val("alpn")),
        "fast_open" => parse_bool(&params.val("fast-open"), Value::Undefined),
    })
}

pub fn parse_tuic(url: &str) -> JsResult<Value> {
    let parts = parse_url_params(url)?;
    let (userinfo, server_info) = split_two(&parts.address_part, "@");
    let (host, port) = parse_server_info(&opt_str(server_info));
    let params = &parts.params;
    let insecure_raw =
        params.val("skip-cert-verify").or_nullish(|| params.val("insecure").or_nullish(|| params.val("allowInsecure")));
    let tls = obj! {
        "enabled" => true,
        "server_name" => params.val("sni"),
        "alpn" => parse_array(&params.val("alpn")),
        "insecure" => parse_bool(&insecure_raw, Value::Bool(true)),
    };
    let decoded = decode_uri_component(userinfo)?;
    let mut creds = decoded.split(':');
    let uuid = opt_str(creds.next());
    let decoded2 = decode_uri_component(userinfo)?;
    let password = opt_str(decoded2.split(':').nth(1));
    Ok(obj! {
        "tag" => parts.name.clone(),
        "type" => "tuic",
        "server" => host,
        "server_port" => port,
        "uuid" => uuid,
        "password" => password,
        "congestion_control" => params.val("congestion_control"),
        "tls" => tls,
        "flow" => params.val("flow"),
        "udp_relay_mode" => params.either("udp-relay-mode", "udp_relay_mode"),
        "zero_rtt" => parse_bool(&params.val("zero-rtt"), Value::Undefined),
        "reduce_rtt" => parse_bool(&params.val("reduce-rtt"), Value::Undefined),
        "fast_open" => parse_bool(&params.val("fast-open"), Value::Undefined),
        "disable_sni" => parse_bool(&params.val("disable-sni"), Value::Undefined),
    })
}

// ---------------------------------------------------------------------------
// AnyTLS
// ---------------------------------------------------------------------------

fn decode_component(value: &str) -> String {
    decode_uri_component(value).unwrap_or_else(|_| value.to_string())
}

fn first_param(params: &Params, keys: &[&str]) -> Value {
    for key in keys {
        if let Some(v) = params.first(key) {
            return Value::str(v);
        }
    }
    Value::Undefined
}

fn parse_optional_number(value: &Value) -> Value {
    if value.is_nullish() || js_trim(&value.to_js_string()).is_empty() {
        return Value::Undefined;
    }
    match parse_maybe_number(value) {
        Value::Number(n) if n.is_finite() => Value::Number(n),
        _ => Value::Undefined,
    }
}

pub fn parse_anytls(url: &str) -> JsResult<Value> {
    let parsed = url::Url::parse(url).map_err(|_| JsError::type_error("Invalid URL"))?;
    if parsed.scheme().to_lowercase() != "anytls" {
        return Ok(Value::Undefined);
    }
    let hostname = parsed.host_str().unwrap_or("");
    let server = if hostname.starts_with('[') && hostname.ends_with(']') && hostname.len() >= 2 {
        hostname[1..hostname.len() - 1].to_string()
    } else {
        hostname.to_string()
    };
    let port = parsed.port().map(f64::from).unwrap_or(443.0);
    let password = decode_component(parsed.username());
    let fragment = decode_component(parsed.fragment().unwrap_or(""));
    let default_tag_server = if server.contains(':') { format!("[{}]", server) } else { server.clone() };
    let params = Params::parse(parsed.query().unwrap_or(""));

    let mut tls = Object::new();
    tls.set("enabled", Value::Bool(true));
    tls.set(
        "insecure",
        parse_bool(
            &first_param(&params, &["insecure", "skip-cert-verify", "allowInsecure", "allow_insecure"]),
            Value::Bool(false),
        ),
    );
    let server_name = first_param(&params, &["sni", "servername", "host"]);
    if server_name.truthy() {
        tls.set("server_name", server_name);
    }
    let alpn = parse_array(&first_param(&params, &["alpn"]));
    if alpn.truthy() {
        tls.set("alpn", alpn);
    }
    let fingerprint = first_param(&params, &["fp", "fingerprint", "client-fingerprint"]);
    if fingerprint.truthy() {
        tls.set("utls", obj! { "enabled" => true, "fingerprint" => fingerprint });
    }
    let udp = parse_bool(&first_param(&params, &["udp"]), Value::Undefined);
    let check =
        parse_optional_number(&first_param(&params, &["idle-session-check-interval", "idle_session_check_interval"]));
    let timeout = parse_optional_number(&first_param(&params, &["idle-session-timeout", "idle_session_timeout"]));
    let min_idle = parse_optional_number(&first_param(&params, &["min-idle-session", "min_idle_session"]));

    let mut o = Object::new();
    let tag = if fragment.is_empty() {
        format!("AnyTLS {}:{}", default_tag_server, crate::js::number::js_number_to_string(port))
    } else {
        fragment
    };
    o.set("tag", Value::String(tag));
    o.set("type", Value::str("anytls"));
    o.set("server", Value::String(server));
    o.set("server_port", Value::Number(port));
    o.set("password", Value::String(password));
    if !udp.is_undefined() {
        o.set("udp", udp);
    }
    if !check.is_undefined() {
        o.set("idle-session-check-interval", check);
    }
    if !timeout.is_undefined() {
        o.set("idle-session-timeout", timeout);
    }
    if !min_idle.is_undefined() {
        o.set("min-idle-session", min_idle);
    }
    o.set("tls", Value::Object(tls));
    Ok(Value::Object(o))
}
