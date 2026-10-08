//! test/issue-403-provider-country-group.test.js

use crate::common::*;
use sublink::builders::BuildOptions;
use sublink::js::Value;

const CLASH_YAML: &str = "
proxies:
  - name: 香港节点01
    type: ss
    server: hk.example.com
    port: 443
    cipher: aes-128-gcm
    password: test123
  - name: 日本节点01
    type: ss
    server: jp.example.com
    port: 443
    cipher: aes-128-gcm
    password: test456
";
const SUB_URL: &str = "https://example.com/clash-sub?token=xxx";

async fn build(input: &str, yaml: &str) -> Value {
    let fetcher = MockFetcher::new();
    fetcher.ok(SUB_URL, yaml);
    let o = BuildOptions { group_by_country: true, ..opts(input, v("[]")) };
    clash_with(&o, &fetcher).await
}

/// mihomo compiles filters with RE2: `(?i)` inline flag and ASCII `\b`.
fn re2(filter: &Value) -> regex::Regex {
    regex::Regex::new(&filter.as_str().unwrap().replace(r"\b", r"(?-u:\b)")).unwrap()
}

#[tokio::test]
async fn creates_country_groups_with_use_and_filter_for_clash_subscription() {
    let config = build(SUB_URL, CLASH_YAML).await;
    assert!(config.get("proxy-providers").is_object_like());
    let hk = group(&config, "🇭🇰 Hong Kong");
    assert_eq!(hk.get("type").as_str(), Some("url-test"));
    assert_eq!(hk.get("proxies").length().unwrap_or(0), 0);
    assert_eq!(strs(hk.get("use")), config.get("proxy-providers").object_keys());
    assert!(!hk.get("filter").is_undefined());
    let jp = group(&config, "🇯🇵 Japan");
    assert_eq!(strs(jp.get("use")), strs(hk.get("use")));
    assert!(!jp.get("filter").is_undefined());
    assert_ne!(jp.get("filter").as_str(), hk.get("filter").as_str());
}

#[tokio::test]
async fn filter_matches_only_its_own_country() {
    let config = build(SUB_URL, CLASH_YAML).await;
    let hk = re2(group(&config, "🇭🇰 Hong Kong").get("filter"));
    let jp = re2(group(&config, "🇯🇵 Japan").get("filter"));
    assert!(hk.is_match("香港节点01"));
    assert!(!hk.is_match("日本节点01"));
    assert!(jp.is_match("日本节点01"));
    assert!(!jp.is_match("香港节点01"));

    let config2 = build(SUB_URL, &CLASH_YAML.replace("日本节点01", "US-Node")).await;
    let us = re2(group(&config2, "🇺🇸 United States").get("filter"));
    assert!(!us.is_match("plus-node"));
    assert!(us.is_match("US-Node"));
}

#[tokio::test]
async fn keeps_inline_proxies_and_narrows_provider_members_in_mixed_mode() {
    let inline = "ss://YWVzLTEyOC1nY206dGVzdA@example.com:443#HK-Inline";
    let config = build(&format!("{inline}\n{SUB_URL}"), CLASH_YAML).await;
    let hk = group(&config, "🇭🇰 Hong Kong");
    assert!(has(hk.get("proxies"), "HK-Inline"));
    assert!(!hk.get("use").is_undefined());
    assert!(!hk.get("filter").is_undefined());
    let jp = group(&config, "🇯🇵 Japan");
    assert_eq!(jp.get("proxies").length().unwrap_or(0), 0);
    assert!(!jp.get("filter").is_undefined());
}

#[tokio::test]
async fn no_filter_without_providers() {
    let config = build("ss://YWVzLTEyOC1nY206dGVzdA@example.com:443#香港节点", CLASH_YAML).await;
    let hk = group(&config, "🇭🇰 Hong Kong");
    assert!(hk.get("use").is_undefined());
    assert!(hk.get("filter").is_undefined());
}

#[tokio::test]
async fn node_select_references_country_groups() {
    let config = build(SUB_URL, CLASH_YAML).await;
    let node_select = group(&config, "🚀 节点选择");
    assert!(has(node_select.get("proxies"), "🇭🇰 Hong Kong"));
    assert!(has(node_select.get("proxies"), "🇯🇵 Japan"));
}
