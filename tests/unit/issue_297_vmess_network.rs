//! test/issue-297-vmess-network.test.js

use crate::common::*;
use sublink::builders::BuildOptions;
use sublink::js::base64::encode_base64;
use sublink::js::json;
use sublink::parsers::protocols::parse_vmess;

fn vmess_url(overrides: &str) -> String {
    let mut config = v(r#"{
        "v":"2","ps":"VMess-WS-Test","add":"vmess.example.com","port":"443",
        "id":"12345678-1234-1234-1234-123456789abc","aid":"0","scy":"auto","net":"ws","type":"none",
        "host":"vmess.example.com","path":"/ws","tls":"tls","sni":"vmess.example.com"}"#);
    for (k, val) in v(overrides).own_entries() {
        config.as_object_mut().unwrap().set(k, val);
    }
    format!("vmess://{}", encode_base64(&json::stringify(&config).unwrap()))
}

#[test]
fn no_top_level_network_for_websocket() {
    let r = parse_vmess(&vmess_url("{}")).unwrap();
    assert!(r.get("network").is_undefined());
    assert_eq!(r.get("transport").get("type").as_str(), Some("ws"));
    assert_eq!(r.get("transport").get("path").as_str(), Some("/ws"));
}

#[test]
fn no_top_level_network_for_grpc_http_h2() {
    for (net, path) in [("grpc", "grpc-service"), ("http", "/http"), ("h2", "/h2")] {
        let r = parse_vmess(&vmess_url(&format!(r#"{{"ps":"VMess-{net}","net":"{net}","path":"{path}"}}"#))).unwrap();
        assert!(r.get("network").is_undefined(), "{net}");
        assert_eq!(r.get("transport").get("type").as_str(), Some(net));
    }
}

#[test]
fn no_top_level_network_for_plain_tcp() {
    let r = parse_vmess(&vmess_url(r#"{"ps":"VMess-TCP-Test","net":"tcp","type":"none"}"#)).unwrap();
    assert!(r.get("network").is_undefined());
    assert!(r.get("transport").is_undefined());
}

#[tokio::test]
async fn singbox_does_not_emit_network_on_vmess_ws_outbound() {
    let config = singbox(&BuildOptions { user_agent: String::new(), ..opts(&vmess_url("{}"), v("[]")) }).await;
    let proxy = outbound(&config, "VMess-WS-Test");
    assert!(proxy.get("network").is_undefined());
    assert_eq!(proxy.get("transport").get("type").as_str(), Some("ws"));
}
