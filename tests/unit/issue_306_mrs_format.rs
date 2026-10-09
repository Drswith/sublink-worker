//! test/issue-306-mrs-format.test.js

use crate::common::*;
use sublink::builders::BuildOptions;
use sublink::config::generate_clash_rule_sets;
use sublink::js::Value;

const INPUT: &str = "
proxies:
  - name: test-ss
    type: ss
    server: example.com
    port: 443
    cipher: aes-128-gcm
    password: test
";

#[test]
fn rule_providers_use_mrs_when_requested() {
    let (site, _) = generate_clash_rule_sets(&v(r#"["AI Services"]"#), &[], true).unwrap();
    let ai = site.get("category-ai-!cn").expect("provider");
    assert_eq!(ai.get("format").as_str(), Some("mrs"));
    assert!(ai.get("url").as_str().unwrap().contains(".mrs"));
}

#[test]
fn rule_providers_use_yaml_otherwise() {
    let (site, _) = generate_clash_rule_sets(&v(r#"["AI Services"]"#), &[], false).unwrap();
    let ai = site.get("category-ai-!cn").expect("provider");
    assert_eq!(ai.get("format").as_str(), Some("yaml"));
    assert!(ai.get("url").as_str().unwrap().contains(".yaml"));
}

async fn ai_provider(ua: &str) -> Value {
    let config = clash(&BuildOptions { user_agent: ua.into(), ..opts(INPUT, v(r#"["AI Services"]"#)) }).await;
    config.get("rule-providers").get("category-ai-!cn").clone()
}

#[tokio::test]
async fn legacy_clients_get_yaml_rule_sets() {
    for ua in ["Clash/1.0", "clash/0.19.0", "ClashForAndroid/2.5.12", "ClashForWindows/0.20.0", "Merlin Clash"] {
        let ai = ai_provider(ua).await;
        assert_eq!(ai.get("format").as_str(), Some("yaml"), "{ua}");
        let url = ai.get("url").as_str().unwrap();
        assert!(url.contains(".yaml") && !url.contains(".mrs"), "{ua}");
    }
}

#[tokio::test]
async fn modern_clients_get_mrs_rule_sets() {
    for ua in [
        "clash-verge/v1.5.0",
        "Clash.Meta/v1.18.0",
        "mihomo/1.18.0",
        "Stash/2.4.0",
        "ClashMetaForAndroid/2.10.0",
        "verge-rev/1.0.0",
        "unknown-client",
    ] {
        let ai = ai_provider(ua).await;
        assert_eq!(ai.get("format").as_str(), Some("mrs"), "{ua}");
        assert!(ai.get("url").as_str().unwrap().contains(".mrs"), "{ua}");
    }
}

#[tokio::test]
async fn all_rule_providers_share_the_legacy_format() {
    let config = clash(&BuildOptions {
        user_agent: "Clash/1.0".into(),
        ..opts(INPUT, v(r#"["AI Services","Google","YouTube","Telegram"]"#))
    })
    .await;
    assert!(config.get("rule-providers").is_plain_object());
    for (name, provider) in config.get("rule-providers").own_entries() {
        assert_eq!(provider.get("format").as_str(), Some("yaml"), "{name}");
        assert!(provider.get("url").as_str().unwrap().contains(".yaml"), "{name}");
    }
}
