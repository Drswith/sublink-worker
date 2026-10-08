//! test/proxy-providers.test.js

use crate::common::*;
use sublink::builders::BuildOptions;
use sublink::config::{clash_config, sing_box_config};
use sublink::js::Value;

const CLASH_YAML: &str = "
proxies:
  - name: HK-Node
    type: ss
    server: hk.example.com
    port: 443
    cipher: aes-128-gcm
    password: test123
  - name: JP-Node
    type: ss
    server: jp.example.com
    port: 443
    cipher: aes-128-gcm
    password: test456
";
const SINGBOX_JSON: &str = r#"{"outbounds":[{"type":"shadowsocks","tag":"SS-HK","server":"hk.example.com","server_port":443,"method":"aes-128-gcm","password":"test"},{"type":"shadowsocks","tag":"SS-JP","server":"jp.example.com","server_port":443,"method":"aes-128-gcm","password":"test"}]}"#;

fn serving(routes: &[(&str, &str)]) -> std::sync::Arc<MockFetcher> {
    let fetcher = MockFetcher::new();
    for (url, body) in routes {
        fetcher.ok(url, body);
    }
    fetcher
}

fn options(input: &str) -> BuildOptions {
    opts(input, v("[]"))
}

#[tokio::test]
async fn clash_uses_clash_url_as_proxy_provider() {
    let url = "https://example.com/clash-sub?token=xxx";
    let config = clash_with(&options(url), &serving(&[(url, CLASH_YAML)])).await;
    let providers = config.get("proxy-providers");
    let names = providers.object_keys();
    assert_eq!(names.len(), 1);
    let name = &names[0];
    assert_match(name, "^_auto_provider_[a-z0-9]+$");
    let p = providers.get(name);
    assert_eq!(p.get("url").as_str(), Some(url));
    assert_eq!(p.get("type").as_str(), Some("http"));
    assert_eq!(p.get("path").as_str(), Some(format!("./proxy_providers/{name}.yaml").as_str()));
    assert!(has(group(&config, "🚀 节点选择").get("use"), name));
}

#[tokio::test]
async fn clash_parses_singbox_url_instead_of_provider() {
    let url = "https://example.com/singbox-sub";
    let config = clash_with(&options(url), &serving(&[(url, SINGBOX_JSON)])).await;
    assert!(config.get("proxy-providers").object_keys().is_empty());
    assert!(config.get("proxies").length().unwrap() > 0);
}

#[tokio::test]
async fn singbox_uses_singbox_url_as_outbound_provider() {
    let url = "https://example.com/singbox-sub?token=xxx";
    let config = singbox_with(&options(url), &serving(&[(url, SINGBOX_JSON)])).await;
    let providers = config.get("outbound_providers");
    assert_eq!(providers.length(), Some(1));
    let p = &providers.as_array().unwrap()[0];
    let tag = p.get("tag").as_str().unwrap();
    assert_match(tag, "^_auto_provider_[a-z0-9]+$");
    assert_eq!(p.get("download_url").as_str(), Some(url));
    assert_eq!(p.get("type").as_str(), Some("http"));
    assert_eq!(p.get("path").as_str(), Some(format!("./providers/{tag}.json").as_str()));
    assert!(has(outbound(&config, "🚀 节点选择").get("providers"), tag));
}

fn proxy_outbounds(config: &Value) -> usize {
    config.get("outbounds").as_array().unwrap().iter().filter(|o| o.get("server").truthy()).count()
}

#[tokio::test]
async fn singbox_parses_clash_url_instead_of_provider() {
    let url = "https://example.com/clash-sub";
    let config = singbox_with(&options(url), &serving(&[(url, CLASH_YAML)])).await;
    assert!(config.get("outbound_providers").is_undefined());
    assert!(proxy_outbounds(&config) > 0);
}

#[tokio::test]
async fn singbox_1_11_does_not_use_outbound_providers() {
    let url = "https://example.com/singbox-sub";
    let o = BuildOptions { singbox_version: "1.11".into(), ..options(url) };
    let config = singbox_with(&o, &serving(&[(url, SINGBOX_JSON)])).await;
    assert!(config.get("outbound_providers").is_undefined());
    assert!(proxy_outbounds(&config) > 0);
}

#[tokio::test]
async fn multiple_clash_urls_become_multiple_providers() {
    let fetcher = serving(&[("https://example.com/sub1", CLASH_YAML), ("https://example.com/sub2", CLASH_YAML)]);
    let config = clash_with(&options("https://example.com/sub1\nhttps://example.com/sub2"), &fetcher).await;
    let names = config.get("proxy-providers").object_keys();
    assert_eq!(names.len(), 2);
    assert_ne!(names[0], names[1]);
    assert!(names.iter().all(|n| n.starts_with("_auto_provider_")));
    let uses = strs(group(&config, "🚀 节点选择").get("use"));
    assert!(names.iter().all(|n| uses.contains(n)));
}

async fn clash_provider(url: &str) -> (String, Value) {
    let config = clash_with(&options(url), &serving(&[(url, CLASH_YAML)])).await;
    let name = config.get("proxy-providers").object_keys()[0].clone();
    let provider = config.get("proxy-providers").get(&name).clone();
    (name, provider)
}

#[tokio::test]
async fn clash_provider_paths_are_distinct_and_stable() {
    let (a, pa) = clash_provider("https://provider-a.example.com/sub?token=aaa").await;
    let (b, pb) = clash_provider("https://provider-b.example.com/sub?token=bbb").await;
    assert_ne!(a, b);
    assert_ne!(pa.get("path").as_str(), pb.get("path").as_str());
    assert_eq!(pa.get("path").as_str(), Some(format!("./proxy_providers/{a}.yaml").as_str()));
    assert_eq!(pb.get("path").as_str(), Some(format!("./proxy_providers/{b}.yaml").as_str()));
}

#[tokio::test]
async fn clash_provider_name_is_stable_for_same_url() {
    let url = "https://stable.example.com/sub?token=same";
    assert_eq!(clash_provider(url).await.0, clash_provider(url).await.0);
}

#[tokio::test]
async fn singbox_provider_paths_are_distinct_and_stable() {
    let build = |url: &'static str| async move {
        let config = singbox_with(&options(url), &serving(&[(url, SINGBOX_JSON)])).await;
        config.get("outbound_providers").as_array().unwrap()[0].clone()
    };
    let a = build("https://provider-a.example.com/singbox?token=aaa").await;
    let b = build("https://provider-b.example.com/singbox?token=bbb").await;
    assert_ne!(a.get("tag").as_str(), b.get("tag").as_str());
    assert_ne!(a.get("path").as_str(), b.get("path").as_str());
    for p in [&a, &b] {
        assert_eq!(
            p.get("path").as_str(),
            Some(format!("./providers/{}.json", p.get("tag").as_str().unwrap()).as_str())
        );
    }
}

#[tokio::test]
async fn clash_keeps_user_defined_providers_next_to_auto_providers() {
    let url = "https://auto.example.com/clash-sub";
    let mut base_config = clash_config().clone();
    base_config.as_object_mut().unwrap().set(
        "proxy-providers",
        v(r#"{"provider1":{"type":"http","url":"https://user.example.com/sub","path":"./user.yaml","interval":3600}}"#),
    );
    let o = BuildOptions { base_config, ..options(url) };
    let config = clash_with(&o, &serving(&[(url, CLASH_YAML)])).await;
    let providers = config.get("proxy-providers");
    assert_eq!(providers.get("provider1").get("url").as_str(), Some("https://user.example.com/sub"));
    let auto = providers.object_keys().into_iter().find(|n| n.starts_with("_auto_provider_")).unwrap();
    assert_eq!(providers.get(&auto).get("url").as_str(), Some(url));
    let uses = group(&config, "🚀 节点选择").get("use");
    assert!(has(uses, "provider1"));
    assert!(has(uses, &auto));
}

#[tokio::test]
async fn singbox_merges_user_defined_outbound_providers() {
    let url = "https://auto.example.com/singbox-sub";
    let mut base_config = sing_box_config().clone();
    base_config.as_object_mut().unwrap().set(
        "outbound_providers",
        v(r#"[{"tag":"user-provider","type":"http","download_url":"https://user.example.com/sub","path":"./providers/user.json","download_interval":"24h"}]"#),
    );
    let o = BuildOptions { base_config, ..options(url) };
    let config = singbox_with(&o, &serving(&[(url, SINGBOX_JSON)])).await;
    let providers = config.get("outbound_providers");
    assert_eq!(providers.length(), Some(2));
    assert!(names(providers, "tag").contains(&"user-provider".to_string()));
    let auto = providers
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p.get("tag").as_str().unwrap().starts_with("_auto_provider_"))
        .unwrap();
    assert_eq!(auto.get("download_url").as_str(), Some(url));
    let node_select = outbound(&config, "🚀 节点选择").get("providers");
    assert!(has(node_select, "user-provider"));
    assert!(has(node_select, auto.get("tag").as_str().unwrap()));
}
