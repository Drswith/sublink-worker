//! test/src-ip-cidr.test.js

use crate::common::*;
use sublink::builders::BuildOptions;
use sublink::builders::helpers::emit_clash_rules;
use sublink::config::{Rule, generate_rules};
use sublink::i18n::Translator;

const INPUT: &str = "ss://YWVzLTEyOC1nY206dGVzdA@example.com:443#HK-Node-1";

fn rules_for(custom: &str) -> Vec<Rule> {
    let mut custom_rules = v(custom).as_array().unwrap().clone();
    generate_rules(&"minimal".into(), &mut custom_rules).unwrap()
}

#[test]
fn generate_rules_parses_src_ip_cidr_into_string_list() {
    let rules = rules_for(r#"[{"name":"LAN","src_ip_cidr":"192.168.11.13/32, 192.168.10.0/24"}]"#);
    assert_eq!(rules[0].outbound.as_str(), Some("LAN"));
    assert_eq!(
        rules[0].src_ip_cidr.as_deref(),
        Some(&["192.168.11.13/32".to_string(), "192.168.10.0/24".to_string()][..])
    );
}

#[test]
fn generate_rules_treats_empty_src_ip_cidr_as_empty_list() {
    let rules = rules_for(r#"[{"name":"LAN","src_ip_cidr":" , , "}]"#);
    assert_eq!(rules[0].outbound.as_str(), Some("LAN"));
    assert_eq!(rules[0].src_ip_cidr.as_deref(), Some(&[][..]));
}

#[test]
fn generate_rules_accepts_src_ip_cidr_as_string_list() {
    let rules = rules_for(r#"[{"name":"LAN","src_ip_cidr":[" 192.168.11.13/32 ","","192.168.10.0/24"]}]"#);
    assert_eq!(
        rules[0].src_ip_cidr.as_deref(),
        Some(&["192.168.11.13/32".to_string(), "192.168.10.0/24".to_string()][..])
    );
}

#[test]
fn emit_clash_rules_emits_src_ip_cidr_rules() {
    let rule =
        Rule { outbound: "LAN".into(), src_ip_cidr: Some(vec!["192.168.11.13/32".into()]), ..Default::default() };
    let lines = emit_clash_rules(&[rule], &Translator::new(Some("zh-CN")));
    assert!(lines.contains(&"SRC-IP-CIDR,192.168.11.13/32,LAN".to_string()));
}

fn with_custom(custom: &str) -> BuildOptions {
    BuildOptions { custom_rules: v(custom).as_array().unwrap().clone(), ..opts(INPUT, "minimal".into()) }
}

fn source_rule(config: &sublink::js::Value) -> sublink::js::Value {
    config
        .get("route")
        .get("rules")
        .as_array()
        .unwrap()
        .iter()
        .find(|r| has(r.get("source_ip_cidr"), "192.168.11.13/32"))
        .cloned()
        .expect("source_ip_cidr rule")
}

#[tokio::test]
async fn singbox_adds_source_ip_cidr_rules() {
    let config = singbox(&with_custom(r#"[{"name":"LAN","src_ip_cidr":"192.168.11.13/32"}]"#)).await;
    let hit = source_rule(&config);
    assert_eq!(hit.get("outbound").as_str(), Some("LAN"));
    assert!(!has_prop(&hit, "protocol"));
}

#[tokio::test]
async fn singbox_includes_protocol_when_specified() {
    let config = singbox(&with_custom(r#"[{"name":"LAN","src_ip_cidr":"192.168.11.13/32","protocol":"http"}]"#)).await;
    let hit = source_rule(&config);
    assert_eq!(hit.get("outbound").as_str(), Some("LAN"));
    assert_json(hit.get("protocol"), r#"["http"]"#);
}

#[tokio::test]
async fn surge_emits_src_ip_rules() {
    let text = surge(&with_custom(r#"[{"name":"LAN","src_ip_cidr":"192.168.11.13/32"}]"#)).await;
    assert!(text.contains("SRC-IP,192.168.11.13,LAN"));
}

#[tokio::test]
async fn surge_comments_and_skips_non_32_src_ip_cidr() {
    let text = surge(&with_custom(r#"[{"name":"LAN","src_ip_cidr":"192.168.10.0/24"}]"#)).await;
    assert!(text.contains("# SRC-IP-CIDR not supported by Surge, skipped: 192.168.10.0/24"));
    assert!(!text.contains("SRC-IP,192.168.10.0/24"));
    assert!(!text.contains("SRC-IP,192.168.10.0,LAN"));
}
