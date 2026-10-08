//! `convertYamlProxyToObject`: Clash YAML proxy → internal proxy object.

use crate::js::number::parse_int;
use crate::js::{Object, Value};
use crate::obj;

fn to_array(value: &Value) -> Value {
    match value {
        Value::Undefined | Value::Null => Value::Undefined,
        Value::Array(_) => value.clone(),
        other => Value::array(vec![other.clone()]),
    }
}

/// `typeof v !== 'undefined' ? !!v : undefined`
fn opt_bool(v: &Value) -> Value {
    if v.is_undefined() { Value::Undefined } else { Value::Bool(v.truthy()) }
}

/// `v ?? undefined`
fn nullish_to_undefined(v: &Value) -> Value {
    if v.is_nullish() { Value::Undefined } else { v.clone() }
}

fn int_of(v: &Value) -> Value {
    Value::Number(parse_int(&v.to_js_string(), 0))
}

fn opts_or_empty(v: &Value) -> Value {
    if v.truthy() { v.clone() } else { Value::Object(Object::new()) }
}

fn transport_for(p: &Value, net: &Value) -> Value {
    match net.as_str() {
        Some("ws") => {
            let w = opts_or_empty(p.get("ws-opts"));
            obj! { "type" => "ws", "path" => w.get("path"), "headers" => w.get("headers") }
        }
        Some("grpc") => {
            let g = opts_or_empty(p.get("grpc-opts"));
            obj! { "type" => "grpc", "service_name" => g.get("grpc-service-name") }
        }
        Some("http") => {
            let h = opts_or_empty(p.get("http-opts"));
            obj! {
                "type" => "http",
                "method" => h.get("method").clone().or_falsy(|| Value::str("GET")),
                "path" => h.get("path"),
                "headers" => h.get("headers"),
            }
        }
        Some("h2") => {
            let h2 = opts_or_empty(p.get("h2-opts"));
            obj! { "type" => "h2", "path" => h2.get("path"), "host" => h2.get("host") }
        }
        _ => Value::Undefined,
    }
}

fn utls_and_reality_tls(p: &Value) -> Value {
    let tls_enabled = p.get("tls").truthy();
    let reality = p.get("reality-opts");
    let mut tls = if tls_enabled {
        let mut t = Object::new();
        t.set("enabled", Value::Bool(true));
        t.set("server_name", p.get("servername").clone().or_falsy(|| p.get("sni").clone()));
        t.set("insecure", Value::Bool(p.get("skip-cert-verify").truthy()));
        if reality.truthy() {
            t.set(
                "reality",
                obj! { "enabled" => true, "public_key" => reality.get("public-key"), "short_id" => reality.get("short-id") },
            );
        }
        t
    } else {
        let mut t = Object::new();
        t.set("enabled", Value::Bool(false));
        t
    };
    let fp = p.get("client-fingerprint");
    if fp.truthy() {
        tls.set("utls", obj! { "enabled" => true, "fingerprint" => fp });
    }
    Value::Object(tls)
}

fn transport_type_or(transport: &Value, fallback: impl FnOnce() -> Value) -> Value {
    transport.get("type").clone().or_falsy(fallback)
}

pub fn convert_yaml_proxy_to_object(p: &Value) -> Value {
    if !p.truthy() || !p.is_object_like() || !p.get("type").truthy() {
        return Value::Null;
    }
    let ty = p.get("type").to_js_string().to_lowercase();
    let name = p.get("name").clone().or_falsy(|| p.get("tag").clone()).or_falsy(|| Value::str("proxy"));
    let fast_open_or_false = || {
        if p.get("fast-open").is_undefined() { Value::Bool(false) } else { Value::Bool(p.get("fast-open").truthy()) }
    };
    match ty.as_str() {
        "ss" | "shadowsocks" => obj! {
            "tag" => name,
            "type" => "shadowsocks",
            "server" => p.get("server"),
            "server_port" => int_of(p.get("port")),
            "method" => p.get("cipher").clone().or_falsy(|| p.get("method").clone()),
            "password" => p.get("password"),
            "network" => "tcp",
            "tcp_fast_open" => p.get("fast-open").truthy(),
            "udp" => opt_bool(p.get("udp")),
            "plugin" => p.get("plugin"),
            "plugin_opts" => p.get("plugin-opts"),
        },
        "vmess" => {
            let tls = if p.get("tls").truthy() {
                obj! {
                    "enabled" => true,
                    "server_name" => p.get("servername").clone().or_falsy(|| p.get("sni").clone()),
                    "insecure" => p.get("skip-cert-verify").truthy(),
                }
            } else {
                obj! { "enabled" => false }
            };
            let net = p.get("network").clone().or_falsy(|| p.get("network-type").clone());
            let transport = transport_for(p, &net);
            obj! {
                "tag" => name,
                "type" => "vmess",
                "server" => p.get("server"),
                "server_port" => int_of(p.get("port")),
                "uuid" => p.get("uuid"),
                "alter_id" => if p.get("alterId").is_undefined() { Value::Number(0.0) } else { int_of(p.get("alterId")) },
                "security" => p.get("cipher").clone().or_falsy(|| p.get("security").clone()).or_falsy(|| Value::str("auto")),
                "network" => transport_type_or(&transport, || p.get("network").clone().or_falsy(|| Value::str("tcp"))),
                "tcp_fast_open" => fast_open_or_false(),
                "transport" => transport.clone(),
                "tls" => tls,
                "udp" => opt_bool(p.get("udp")),
                "packet_encoding" => p.get("packet-encoding"),
                "alpn" => to_array(p.get("alpn")),
            }
        }
        "vless" => {
            let tls = utls_and_reality_tls(p);
            let transport = transport_for(p, p.get("network"));
            obj! {
                "tag" => name,
                "type" => "vless",
                "server" => p.get("server"),
                "server_port" => int_of(p.get("port")),
                "uuid" => p.get("uuid"),
                "tcp_fast_open" => fast_open_or_false(),
                "tls" => tls,
                "transport" => transport.clone(),
                "network" => transport_type_or(&transport, || Value::str("tcp")),
                "flow" => nullish_to_undefined(p.get("flow")),
                "udp" => opt_bool(p.get("udp")),
                "packet_encoding" => p.get("packet-encoding"),
                "alpn" => to_array(p.get("alpn")),
            }
        }
        "trojan" => {
            let tls = utls_and_reality_tls(p);
            let transport = transport_for(p, p.get("network"));
            obj! {
                "type" => "trojan",
                "tag" => name,
                "server" => p.get("server"),
                "server_port" => int_of(p.get("port")),
                "password" => p.get("password"),
                "network" => transport_type_or(&transport, || p.get("network").clone().or_falsy(|| Value::str("tcp"))),
                "tcp_fast_open" => fast_open_or_false(),
                "tls" => tls,
                "transport" => transport.clone(),
                "flow" => nullish_to_undefined(p.get("flow")),
                "alpn" => to_array(p.get("alpn")),
            }
        }
        "hysteria2" | "hysteria" | "hy2" => {
            let tls = obj! {
                "enabled" => true,
                "server_name" => p.get("sni"),
                "insecure" => p.get("skip-cert-verify").truthy(),
            };
            let mut obfs = Object::new();
            if p.get("obfs").truthy() {
                obfs.set("type", p.get("obfs").clone());
                obfs.set("password", p.get("obfs-password").clone());
            }
            let hop_raw = p.get("hop-interval");
            let hop = hop_raw.to_number();
            obj! {
                "tag" => name,
                "type" => "hysteria2",
                "server" => p.get("server"),
                "server_port" => int_of(p.get("port")),
                "password" => p.get("password"),
                "tls" => tls,
                "obfs" => if obfs.is_empty() { Value::Undefined } else { Value::Object(obfs) },
                "auth" => p.get("auth"),
                "recv_window_conn" => p.get("recv-window-conn"),
                "up" => p.get("up"),
                "down" => p.get("down"),
                "ports" => p.get("mport").clone().or_falsy(|| p.get("ports").clone()),
                "hop_interval" => if hop.is_nan() { hop_raw.clone() } else { Value::Number(hop) },
                "alpn" => to_array(p.get("alpn")),
                "fast_open" => opt_bool(p.get("fast-open")),
            }
        }
        "tuic" => obj! {
            "tag" => name,
            "type" => "tuic",
            "server" => p.get("server"),
            "server_port" => int_of(p.get("port")),
            "uuid" => p.get("uuid"),
            "password" => p.get("password"),
            "congestion_control" => p.get("congestion-controller").clone().or_falsy(|| p.get("congestion_control").clone()),
            "tls" => obj! {
                "enabled" => true,
                "server_name" => p.get("sni"),
                "alpn" => to_array(p.get("alpn")),
                "insecure" => p.get("skip-cert-verify").truthy(),
            },
            "flow" => nullish_to_undefined(p.get("flow")),
            "udp_relay_mode" => p.get("udp-relay-mode"),
            "zero_rtt" => opt_bool(p.get("zero-rtt")),
            "reduce_rtt" => opt_bool(p.get("reduce-rtt")),
            "fast_open" => opt_bool(p.get("fast-open")),
            "disable_sni" => opt_bool(p.get("disable-sni")),
        },
        "anytls" => {
            let mut tls = Object::new();
            tls.set("enabled", Value::Bool(true));
            tls.set("server_name", p.get("sni").clone());
            tls.set("insecure", Value::Bool(p.get("skip-cert-verify").truthy()));
            tls.set("alpn", to_array(p.get("alpn")));
            let fp = p.get("client-fingerprint");
            if fp.truthy() {
                tls.set("utls", obj! { "enabled" => true, "fingerprint" => fp });
            }
            obj! {
                "tag" => name,
                "type" => "anytls",
                "server" => p.get("server"),
                "server_port" => int_of(p.get("port")),
                "password" => p.get("password"),
                "udp" => p.get("udp").truthy(),
                "idle-session-check-interval" => p.get("idle-session-check-interval"),
                "idle-session-timeout" => p.get("idle-session-timeout"),
                "min-idle-session" => p.get("min-idle-session"),
                "tls" => Value::Object(tls),
            }
        }
        _ => Value::Null,
    }
}
