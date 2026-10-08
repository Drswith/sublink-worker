//! test/anytls-protocol.test.js

use crate::common::*;
use sublink::builders::BuildOptions;
use sublink::js::base64::encode_base64;
use sublink::parsers::parse_proxy;
use sublink::parsers::protocols::parse_anytls;

const EXTENDED_URI: &str = "anytls://p%40ss@example.com:8443/?sni=any.example.com&insecure=1&alpn=h2,http%2F1.1&fp=chrome&udp=true&idle-session-check-interval=30&idle_session_timeout=120&min-idle-session=5#ANYTLS%20main";

#[test]
fn parses_official_uri_fields_and_client_extensions() {
    assert_json(
        &parse_anytls(EXTENDED_URI).unwrap(),
        r#"{
            "tag":"ANYTLS main","type":"anytls","server":"example.com","server_port":8443,"password":"p@ss","udp":true,
            "idle-session-check-interval":30,"idle-session-timeout":120,"min-idle-session":5,
            "tls":{"enabled":true,"insecure":true,"server_name":"any.example.com","alpn":["h2","http/1.1"],
                   "utls":{"enabled":true,"fingerprint":"chrome"}}
        }"#,
    );
}

#[test]
fn applies_default_port_and_keeps_links_without_fragment() {
    assert_json(
        &parse_anytls("anytls://letmein@example.com/?sni=real.example.com&insecure=0").unwrap(),
        r#"{"tag":"AnyTLS example.com:443","type":"anytls","server":"example.com","server_port":443,"password":"letmein",
            "tls":{"enabled":true,"insecure":false,"server_name":"real.example.com"}}"#,
    );
}

#[test]
fn parses_ipv6_authorities_and_encoded_fragments() {
    let r = parse_anytls("anytls://secret@[2409:8a71:6a00:1953::615]:8964/?insecure=1#IPv6%20node").unwrap();
    assert_eq!(r.get("server").as_str(), Some("2409:8a71:6a00:1953::615"));
    assert_json(r.get("server_port"), "8964");
    assert_eq!(r.get("tag").as_str(), Some("IPv6 node"));
    assert_json(r.get("tls").get("insecure"), "true");
}

#[tokio::test]
async fn generic_parser_handles_anytls_scheme() {
    let r = parse_proxy(&MockFetcher::default(), &EXTENDED_URI.replace("anytls://", "ANYTLS://"), "").await.unwrap();
    assert_eq!(r.get("type").as_str(), Some("anytls"));
    assert_eq!(r.get("tag").as_str(), Some("ANYTLS main"));
}

#[tokio::test]
async fn keeps_anytls_nodes_from_remote_uri_subscriptions_in_clash() {
    let url = "https://subscription.example.com/anytls";
    let fetcher = MockFetcher::new();
    fetcher.ok(url, "anytls://letmein@example.com/?sni=real.example.com");
    let config =
        clash_with(&BuildOptions { user_agent: "mihomo/1.0".into(), ..opts(url, "minimal".into()) }, &fetcher).await;
    let proxy = find(config.get("proxies"), "type", "anytls").expect("anytls proxy");
    assert_match_object(
        proxy,
        r#"{"name":"AnyTLS example.com:443","type":"anytls","server":"example.com","port":443,"password":"letmein","udp":true,"sni":"real.example.com","skip-cert-verify":false}"#,
    );
}

#[tokio::test]
async fn generates_native_singbox_anytls_fields_from_base64_subscriptions() {
    let o = BuildOptions { user_agent: "sing-box/1.12".into(), ..opts(&encode_base64(EXTENDED_URI), "minimal".into()) };
    let config = singbox(&o).await;
    let ob = find(config.get("outbounds"), "type", "anytls").expect("anytls outbound");
    assert_match_object(
        ob,
        r#"{"tag":"ANYTLS main","type":"anytls","server":"example.com","server_port":8443,"password":"p@ss",
            "idle_session_check_interval":"30s","idle_session_timeout":"120s","min_idle_session":5,
            "tls":{"enabled":true,"insecure":true,"server_name":"any.example.com","alpn":["h2","http/1.1"]}}"#,
    );
    assert!(ob.get("udp").is_undefined());
    for key in ["idle-session-check-interval", "idle-session-timeout", "min-idle-session"] {
        assert!(!has_prop(ob, key), "{key}");
    }
}

#[tokio::test]
async fn maps_singbox_anytls_session_options_to_clash_names() {
    let input = r#"{"outbounds":[{"tag":"Sing-box AnyTLS","type":"anytls","server":"example.com","server_port":443,"password":"secret",
        "idle_session_check_interval":30,"idle_session_timeout":120,"min_idle_session":5,"tls":{"enabled":true,"server_name":"example.com"}}]}"#;
    let config = clash(&BuildOptions { user_agent: "mihomo/1.0".into(), ..opts(input, "minimal".into()) }).await;
    let proxy = find(config.get("proxies"), "type", "anytls").expect("anytls proxy");
    assert_match_object(
        proxy,
        r#"{"name":"Sing-box AnyTLS","idle-session-check-interval":30,"idle-session-timeout":120,"min-idle-session":5}"#,
    );
}
