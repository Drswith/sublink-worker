//! test/surge-unsupported-proxy.test.js

use crate::common::*;
use sublink::builders::BuildOptions;

const MIXED: &str = "hysteria2://580ef251-af2b-49f4-aea4-56a8a8f7a391@demo.de:8443?peer=demo.de&insecure=0&sni=demo.de&alpn=h3#USA-HY2
vless://580ef251-af2b-49f4-aea4-56a8a8f7a391@1.2.3.4:8443?encryption=none&security=reality&type=tcp&sni=m.media-amazon.com&fp=chrome&pbk=testpublickey&sid=testsid&flow=xtls-rprx-vision#USA-VLESS
hysteria2://11111251-af2b-49f4-aea4-56a8a8f7a391@demo1.de:8443?peer=demo1.de&insecure=0&sni=demo1.de&alpn=h3#USA-HY22";

async fn build(input: &str) -> String {
    surge(&BuildOptions { user_agent: String::new(), ..opts(input, "minimal".into()) }).await
}

/// `text.match(/\[Name\]([\s\S]*?)(?=\n\[|$)/)[1]`
fn section<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let start = text.find(&format!("[{name}]"))? + name.len() + 2;
    let rest = &text[start..];
    Some(&rest[..rest.find("\n[").unwrap_or(rest.len())])
}

#[tokio::test]
async fn proxy_groups_contain_no_comment_strings() {
    let result = build(MIXED).await;
    let groups = section(&result, "Proxy Group").expect("proxy group section");
    assert!(!groups.contains("# USA-VLESS"));
    assert!(!groups.contains("Unsupported proxy type"));
}

#[tokio::test]
async fn proxy_groups_only_include_supported_proxies() {
    let result = build(MIXED).await;
    let groups = section(&result, "Proxy Group").unwrap();
    assert!(groups.contains("USA-HY2"));
    assert!(groups.contains("USA-HY22"));
    assert!(!regex::Regex::new(r",\s*#[^,\n]*").unwrap().is_match(groups));
}

#[tokio::test]
async fn proxy_section_lists_supported_proxies() {
    let result = build(MIXED).await;
    let start = result.find("[Proxy]").unwrap() + "[Proxy]".len();
    let end = result.find("\n[Proxy Group]").unwrap();
    let proxies = &result[start..end];
    assert!(proxies.contains("USA-HY2"));
    assert!(proxies.contains("USA-HY22"));
}

#[tokio::test]
async fn generates_valid_surge_group_syntax() {
    let result = build(MIXED).await;
    let line = result.lines().find_map(|l| l.split_once("⚡ 自动选择 = url-test,")).expect("auto select group").1;
    for item in line
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.starts_with("url=") && !s.starts_with("interval="))
    {
        assert!(!item.starts_with('#'));
        assert!(!item.contains("Unsupported"));
    }
}

#[tokio::test]
async fn handles_only_unsupported_protocols_gracefully() {
    let result = build("vless://580ef251-af2b-49f4-aea4-56a8a8f7a391@1.2.3.4:8443?encryption=none&security=reality&type=tcp&sni=example.com&fp=chrome&pbk=testkey&sid=testsid#VLESS-Only").await;
    for header in ["[General]", "[Proxy]", "[Proxy Group]", "[Rule]"] {
        assert!(result.contains(header), "{header}");
    }
    if let Some(groups) = section(&result, "Proxy Group") {
        assert!(!groups.contains("# VLESS-Only"));
    }
}
