//! test/issue-334-rule-provider-collision.test.js

use crate::common::*;
use sublink::builders::BuildOptions;
use sublink::js::Value;

const SS_INPUT: &str = "
ss://YWVzLTEyOC1nY206dGVzdA@example.com:443#HK-Node-1
ss://YWVzLTEyOC1nY206dGVzdA@example.com:444#US-Node-1
";

async fn build() -> Value {
    clash(&BuildOptions { user_agent: "mihomo/1.0".into(), ..opts(SS_INPUT, "balanced".into()) }).await
}

#[tokio::test]
async fn google_site_provider_is_not_overwritten_by_ip_provider() {
    let config = build().await;
    let providers = config.get("rule-providers");
    assert_eq!(providers.get("google").get("behavior").as_str(), Some("domain"));
    assert!(providers.get("google").get("url").as_str().unwrap().contains("geosite"));
    assert_eq!(providers.get("google-ip").get("behavior").as_str(), Some("ipcidr"));
    assert!(providers.get("google-ip").get("url").as_str().unwrap().contains("geoip"));
}

#[tokio::test]
async fn cn_site_provider_is_not_overwritten_by_ip_provider() {
    let config = build().await;
    let providers = config.get("rule-providers");
    assert_eq!(providers.get("cn").get("behavior").as_str(), Some("domain"));
    assert_eq!(providers.get("cn-ip").get("behavior").as_str(), Some("ipcidr"));
}

#[tokio::test]
async fn rules_reference_correct_provider_keys() {
    let config = build().await;
    let rules = config.get("rules");
    assert_any_match(rules, "^RULE-SET,google,.*谷歌");
    assert_any_match(rules, "^RULE-SET,google-ip,.*谷歌.*no-resolve");
    assert_any_match(rules, "^RULE-SET,geolocation-!cn,.*非中国");
}

#[tokio::test]
async fn google_domain_rule_comes_before_non_china_rule() {
    let config = build().await;
    let rules = config.get("rules");
    let google = find_index(rules, |r| r.as_str().unwrap().starts_with("RULE-SET,google,"));
    let non_china = find_index(rules, |r| r.as_str().unwrap().contains("geolocation-!cn"));
    assert!(google > -1 && non_china > -1);
    assert!(google < non_china);
}
