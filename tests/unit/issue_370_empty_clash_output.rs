//! test/issue-370-empty-clash-output.test.js

use crate::common::*;
use sublink::js::Value;
use sublink::js::base64::encode_base64;
use sublink::parsers::subscription::{Format, fetch_subscription_with_format};

const PLAIN_CLASH_YAML: &str = "proxies:
  - name: HK-Plain
    type: ss
    server: hk.example.com
    port: 443
    cipher: aes-128-gcm
    password: test123
";

fn serving(url: &str, body: &str) -> std::sync::Arc<MockFetcher> {
    let fetcher = MockFetcher::new();
    fetcher.ok(url, body);
    fetcher
}

fn assert_no_empty_url_test_group(config: &Value) {
    let empty = config.get("proxy-groups").as_array().unwrap().iter().any(|g| {
        g.get("type").as_str() == Some("url-test")
            && g.get("proxies").as_array().is_none_or(|a| a.is_empty())
            && g.get("use").as_array().is_none_or(|a| a.is_empty())
    });
    assert!(!empty, "found an empty url-test group");
}

#[tokio::test]
async fn keeps_plain_clash_yaml_intact_when_fetching() {
    let url = "https://example.com/plain-clash.yaml";
    let fetcher = serving(url, PLAIN_CLASH_YAML);
    let result = fetch_subscription_with_format(fetcher.as_ref(), url, "test-agent").await.unwrap();
    assert_eq!(result.format, Format::Clash);
    assert_eq!(result.content, PLAIN_CLASH_YAML.trim());
}

#[tokio::test]
async fn still_decodes_base64_wrapped_clash_yaml() {
    let url = "https://example.com/base64-clash";
    let fetcher = serving(url, &encode_base64(PLAIN_CLASH_YAML));
    let result = fetch_subscription_with_format(fetcher.as_ref(), url, "test-agent").await.unwrap();
    assert_eq!(result.format, Format::Clash);
    assert_eq!(result.content, PLAIN_CLASH_YAML.trim());
    assert!(result.content.contains("HK-Plain"));
}

#[tokio::test]
async fn uses_plain_clash_subscription_url_as_provider() {
    let url = "https://example.com/plain-clash.yaml";
    let fetcher = serving(url, PLAIN_CLASH_YAML);
    let built = clash_with(&opts(url, "minimal".into()), &fetcher).await;
    let provider = built.get("proxy-providers").object_keys()[0].clone();
    assert_match(&provider, "^_auto_provider_[a-z0-9]+$");
    assert_eq!(built.get("proxy-providers").get(&provider).get("url").as_str(), Some(url));
    assert_eq!(strs(group(&built, "⚡ 自动选择").get("use")), [provider]);
    assert_no_empty_url_test_group(&built);
}

#[tokio::test]
async fn no_empty_auto_select_group_without_proxies_or_providers() {
    let built = clash(&opts("not-a-valid-subscription", "minimal".into())).await;
    assert!(find(built.get("proxy-groups"), "name", "⚡ 自动选择").is_none());
    assert_eq!(strs(group(&built, "🚀 节点选择").get("proxies")), ["DIRECT", "REJECT"]);
    assert_no_empty_url_test_group(&built);
}
