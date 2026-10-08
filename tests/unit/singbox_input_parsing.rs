//! test/singbox-input-parsing.test.js

use crate::common::*;
use sublink::builders::BuildOptions;
use sublink::js::Value;

const SAMPLE: &str = r#"{"outbounds":[
    {"type":"shadowsocks","tag":"SS-Test","server":"ss.example.com","server_port":8388,"method":"aes-256-gcm","password":"test-password"},
    {"type":"vless","tag":"VLESS-Test","server":"vless.example.com","server_port":443,"uuid":"12345678-1234-1234-1234-123456789abc","tls":{"enabled":true,"server_name":"vless.example.com"}},
    {"type":"vmess","tag":"VMess-Test","server":"vmess.example.com","server_port":443,"uuid":"87654321-4321-4321-4321-cba987654321","alter_id":0,"security":"auto","tls":{"enabled":true,"server_name":"vmess.example.com"},"transport":{"type":"ws","path":"/ws"}},
    {"type":"direct","tag":"DIRECT"},
    {"type":"block","tag":"REJECT"},
    {"type":"selector","tag":"节点选择","outbounds":["SS-Test","VLESS-Test","VMess-Test"]}
  ],
  "dns":{"servers":[{"type":"udp","tag":"dns_direct","server":"223.5.5.5"}]}}"#;

fn options(input: &str) -> BuildOptions {
    BuildOptions { user_agent: String::new(), ..opts(input, v("[]")) }
}

fn proxies(config: &Value) -> Vec<Value> {
    config.get("outbounds").as_array().unwrap().iter().filter(|o| o.get("server").truthy()).cloned().collect()
}

fn tags(list: &[Value]) -> Vec<String> {
    list.iter().map(|p| p.get("tag").to_js_string()).collect()
}

#[tokio::test]
async fn extracts_proxy_nodes_from_singbox_json() {
    let p = proxies(&singbox(&options(SAMPLE)).await);
    assert_eq!(p.len(), 3);
    let t = tags(&p);
    for tag in ["SS-Test", "VLESS-Test", "VMess-Test"] {
        assert!(t.contains(&tag.to_string()), "{tag}");
    }
    for tag in ["DIRECT", "REJECT", "节点选择"] {
        assert!(!t.contains(&tag.to_string()), "{tag}");
    }
}

#[tokio::test]
async fn preserves_tls_and_transport_settings() {
    let config = singbox(&options(SAMPLE)).await;
    let vmess = outbound(&config, "VMess-Test");
    assert_json(vmess.get("tls").get("enabled"), "true");
    assert_eq!(vmess.get("transport").get("type").as_str(), Some("ws"));
    assert_eq!(vmess.get("transport").get("path").as_str(), Some("/ws"));
}

#[tokio::test]
async fn works_with_clash_builder() {
    let config = clash(&options(SAMPLE)).await;
    let n = names(config.get("proxies"), "name");
    assert_eq!(n.len(), 3);
    for name in ["SS-Test", "VLESS-Test", "VMess-Test"] {
        assert!(n.contains(&name.to_string()), "{name}");
    }
}

#[tokio::test]
async fn handles_json_with_only_outbounds() {
    let input = r#"{"outbounds":[{"type":"trojan","tag":"Trojan-Minimal","server":"trojan.example.com","server_port":443,"password":"trojan-password","tls":{"enabled":true}}]}"#;
    let p = proxies(&singbox(&options(input)).await);
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].get("tag").as_str(), Some("Trojan-Minimal"));
    assert_eq!(p[0].get("type").as_str(), Some("trojan"));
}

#[tokio::test]
async fn handles_hysteria2_and_tuic() {
    let input = r#"{"outbounds":[
        {"type":"hysteria2","tag":"HY2-Test","server":"hy2.example.com","server_port":443,"password":"hy2-password","tls":{"enabled":true,"server_name":"hy2.example.com"}},
        {"type":"tuic","tag":"TUIC-Test","server":"tuic.example.com","server_port":443,"uuid":"tuic-uuid","password":"tuic-password","congestion_control":"bbr","tls":{"enabled":true,"server_name":"tuic.example.com"}}]}"#;
    let p = proxies(&singbox(&options(input)).await);
    assert_eq!(p.len(), 2);
    let t = tags(&p);
    assert!(t.contains(&"HY2-Test".to_string()) && t.contains(&"TUIC-Test".to_string()));
    let tuic = p.iter().find(|x| x.get("tag").as_str() == Some("TUIC-Test")).unwrap();
    assert_eq!(tuic.get("congestion_control").as_str(), Some("bbr"));
}
