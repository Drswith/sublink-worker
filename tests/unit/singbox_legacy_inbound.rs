//! test/singbox-legacy-inbound.test.js

use crate::common::*;
use sublink::builders::BuildOptions;
use sublink::config::{sing_box_config, sing_box_config_v1_11};
use sublink::js::Value;

const SAMPLE: &str = r#"{"outbounds":[{"type":"vless","tag":"test-proxy","server":"example.com","server_port":443,"uuid":"00000000-0000-0000-0000-000000000000","tls":{"enabled":true,"server_name":"example.com"}}]}"#;
const LEGACY_INBOUND_FIELDS: &[&str] =
    &["sniff", "sniff_timeout", "sniff_override_destination", "domain_strategy", "udp_disable_domain_unmapping"];

fn base() -> BuildOptions {
    BuildOptions { user_agent: String::new(), ..opts(SAMPLE, v("[]")) }
}

fn assert_no_legacy_inbound_fields(config: &Value) {
    for inbound in config.get("inbounds").as_array().unwrap() {
        for field in LEGACY_INBOUND_FIELDS {
            assert!(!has_prop(inbound, field), "inbound {:?} should not have {field}", inbound.get("tag").as_str());
        }
    }
}

fn legacy_outbounds(config: &Value) -> usize {
    config
        .get("outbounds")
        .as_array()
        .unwrap()
        .iter()
        .filter(|o| matches!(o.get("type").as_str(), Some("block" | "dns")))
        .count()
}

fn has_sniff_rule(config: &Value) -> bool {
    find(config.get("route").get("rules"), "action", "sniff").is_some()
}

#[test]
fn base_config_inbounds_have_no_legacy_fields() {
    assert_no_legacy_inbound_fields(sing_box_config());
    assert_no_legacy_inbound_fields(sing_box_config_v1_11());
}

#[test]
fn base_configs_have_no_legacy_special_outbounds() {
    assert_eq!(legacy_outbounds(sing_box_config()), 0);
    assert_eq!(legacy_outbounds(sing_box_config_v1_11()), 0);
}

#[tokio::test]
async fn built_v1_12_config_sniffs_via_route_rule() {
    let config = singbox(&base()).await;
    assert_no_legacy_inbound_fields(&config);
    assert!(has_sniff_rule(&config));
}

#[tokio::test]
async fn built_v1_11_config_sniffs_via_route_rule() {
    let o = BuildOptions { base_config: sing_box_config_v1_11().clone(), singbox_version: "1.11".into(), ..base() };
    let config = singbox(&o).await;
    assert_no_legacy_inbound_fields(&config);
    assert!(has_sniff_rule(&config));
    assert_eq!(legacy_outbounds(&config), 0);
}

#[tokio::test]
async fn custom_base_config_does_not_reintroduce_legacy_fields() {
    let base_config = v(r#"{
        "inbounds": [
            {"type":"mixed","tag":"mixed-in","listen":"0.0.0.0","listen_port":2080},
            {"type":"tun","tag":"tun-in","address":"172.19.0.1/30","auto_route":true,"strict_route":true,"stack":"mixed","sniff":true}
        ],
        "outbounds": [{"type":"block","tag":"REJECT"},{"type":"direct","tag":"DIRECT"}],
        "route": {"rule_set":[],"rules":[]}
    }"#);
    let config = singbox(&BuildOptions { base_config, ..base() }).await;
    assert!(has_sniff_rule(&config));
    assert_eq!(legacy_outbounds(&config), 0);
    let members: Vec<String> =
        config.get("outbounds").as_array().unwrap().iter().flat_map(|o| strs(o.get("outbounds"))).collect();
    assert!(!members.iter().any(|m| m == "REJECT"));
}

#[tokio::test]
async fn emits_reject_action_for_ad_blocking_rules() {
    let config = singbox(&BuildOptions { selected_rules: v(r#"["Ad Block"]"#), ..base() }).await;
    let reject = config
        .get("route")
        .get("rules")
        .as_array()
        .unwrap()
        .iter()
        .find(|r| has(r.get("rule_set"), "category-ads-all"))
        .expect("ad block rule");
    assert_eq!(reject.get("action").as_str(), Some("reject"));
    assert!(!has_prop(reject, "outbound"));
    assert!(find(config.get("outbounds"), "tag", "🛑 广告拦截").is_none());
}
