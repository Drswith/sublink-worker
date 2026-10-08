//! test/udp-handling.test.js

use crate::common::*;
use sublink::builders::{BuildOptions, clash, singbox};
use sublink::parsers::clash_proxy::convert_yaml_proxy_to_object;
use sublink::parsers::protocols::parse_vless;

#[test]
fn parses_udp_true_from_vless_url() {
    let r = parse_vless("vless://test-uuid@example.com:443?security=tls&sni=example.com&udp=true#TestVless").unwrap();
    assert_json(r.get("udp"), "true");
    assert_eq!(r.get("type").as_str(), Some("vless"));
    assert_eq!(r.get("tag").as_str(), Some("TestVless"));
}

#[test]
fn parses_udp_false_from_vless_url() {
    let r = parse_vless("vless://test-uuid@example.com:443?security=tls&sni=example.com&udp=false#TestVless").unwrap();
    assert_json(r.get("udp"), "false");
}

#[test]
fn omits_udp_when_not_specified() {
    let r = parse_vless("vless://test-uuid@example.com:443?security=tls&sni=example.com#TestVless").unwrap();
    assert!(r.get("udp").is_undefined());
}

#[test]
fn singbox_strips_udp_field() {
    let converted = singbox::convert_proxy(&v(
        r#"{"tag":"TestProxy","type":"vless","server":"example.com","server_port":443,"uuid":"test-uuid","udp":true,"tls":{"enabled":true,"server_name":"example.com"}}"#,
    ))
    .unwrap();
    assert!(converted.get("udp").is_undefined());
    assert_eq!(converted.get("tag").as_str(), Some("TestProxy"));
    assert_eq!(converted.get("type").as_str(), Some("vless"));
}

#[test]
fn singbox_moves_root_alpn_into_tls() {
    let converted = singbox::convert_proxy(&v(
        r#"{"tag":"TestProxy","type":"vless","server":"example.com","server_port":443,"uuid":"test-uuid","alpn":["h2","http/1.1"],"tls":{"enabled":true,"server_name":"example.com"}}"#,
    ))
    .unwrap();
    assert!(converted.get("alpn").is_undefined());
    assert_json(converted.get("tls").get("alpn"), r#"["h2","http/1.1"]"#);
}

#[test]
fn clash_keeps_udp_field() {
    let converted = clash::convert_proxy(&v(
        r#"{"tag":"TestProxy","type":"vless","server":"example.com","server_port":443,"uuid":"test-uuid","udp":true,"tls":{"enabled":true,"server_name":"example.com"}}"#,
    ))
    .unwrap();
    assert_json(converted.get("udp"), "true");
    assert_eq!(converted.get("name").as_str(), Some("TestProxy"));
    assert_eq!(converted.get("type").as_str(), Some("vless"));
}

#[tokio::test]
async fn clash_enables_udp_by_default_for_uri_proxies() {
    let built = clash(&BuildOptions {
        user_agent: String::new(),
        ..opts("ss://YWVzLTEyOC1nY206dGVzdA@example.com:443#TestSS", "minimal".into())
    })
    .await;
    let proxies = built.get("proxies").as_array().unwrap();
    assert_eq!(proxies.len(), 1);
    assert_eq!(proxies[0].get("type").as_str(), Some("ss"));
    assert_json(proxies[0].get("udp"), "true");
}

#[test]
fn clash_keeps_explicit_udp_false() {
    let converted = clash::convert_proxy(&v(
        r#"{"tag":"TestProxy","type":"vmess","server":"example.com","server_port":443,"uuid":"test-uuid","udp":false,"tls":{"enabled":true,"server_name":"example.com"}}"#,
    ))
    .unwrap();
    assert_json(converted.get("udp"), "false");
}

#[test]
fn clash_yaml_udp_is_parsed_but_stripped_for_singbox() {
    let parsed = convert_yaml_proxy_to_object(&v(
        r#"{"name":"VLESS-Test","type":"vless","server":"example.com","port":443,"uuid":"test-uuid","udp":true,"tls":true,"servername":"example.com"}"#,
    ));
    assert_json(parsed.get("udp"), "true");
    assert!(singbox::convert_proxy(&parsed).unwrap().get("udp").is_undefined());
}
