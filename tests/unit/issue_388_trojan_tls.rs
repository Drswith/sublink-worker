//! test/issue-388-trojan-tls.test.js

use crate::common::*;
use sublink::parsers::protocols::parse_trojan;

#[test]
fn enables_tls_when_security_param_is_absent() {
    let r = parse_trojan(
        "trojan://pass@example.com:443?allowInsecure=1&peer=www.apple.com.cn&sni=www.apple.com.cn&type=tcp#JP-03",
    )
    .unwrap();
    assert_eq!(r.get("type").as_str(), Some("trojan"));
    assert_json(r.get("tls").get("enabled"), "true");
    assert_eq!(r.get("tls").get("server_name").as_str(), Some("www.apple.com.cn"));
    assert_json(r.get("tls").get("insecure"), "true");
}

#[test]
fn keeps_tls_enabled_when_security_tls_is_explicit() {
    let r = parse_trojan("trojan://pass@example.com:443?security=tls&sni=example.org#explicit-tls").unwrap();
    assert_json(r.get("tls").get("enabled"), "true");
    assert_eq!(r.get("tls").get("server_name").as_str(), Some("example.org"));
}

#[test]
fn respects_security_none_when_explicitly_set() {
    let r = parse_trojan("trojan://pass@example.com:443?security=none#no-tls").unwrap();
    assert_json(r.get("tls").get("enabled"), "false");
}
