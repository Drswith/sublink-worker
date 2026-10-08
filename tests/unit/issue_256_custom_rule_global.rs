//! test/issue-256-custom-rule-global.test.js

use crate::common::*;
use sublink::builders::{BuildOptions, ConfigBuilder};

const INPUT: &str = "ss://YWVzLTI1Ni1nY206dGVzdA==@us1.example.com:8388#US-Node-1\nss://YWVzLTI1Ni1nY206dGVzdA==@uk1.example.com:8388#UK-Node-1";
const NODE_SELECT: &str = "🚀 节点选择";
const AUTO_SELECT: &str = "⚡ 自动选择";

fn options() -> BuildOptions {
    BuildOptions {
        custom_rules: v(r#"[
            {"name":"Custom-Rule-1","site_rules":["google"],"ip_rules":[],"domain_suffix":[],"domain_keyword":[]},
            {"name":"Custom-Rule-2","site_rules":["github"],"ip_rules":[],"domain_suffix":[],"domain_keyword":[]}
        ]"#)
        .as_array()
        .unwrap()
        .clone(),
        user_agent: String::new(),
        group_by_country: true,
        ..opts(INPUT, "minimal".into())
    }
}

fn has_country(members: &[String]) -> bool {
    members
        .iter()
        .any(|m| m.contains("🇺🇸") || m.contains("🇬🇧") || m.contains("United States") || m.contains("United Kingdom"))
}

#[tokio::test]
async fn singbox_custom_rules_exclude_country_groups() {
    let (builder, _) = singbox_build(&options(), &MockFetcher::default()).await;
    let config = &builder.core().config;
    let r1 = strs(outbound(config, "Custom-Rule-1").get("outbounds"));
    let r2 = strs(outbound(config, "Custom-Rule-2").get("outbounds"));
    for m in [NODE_SELECT, AUTO_SELECT, "DIRECT"] {
        assert!(r1.iter().any(|x| x == m), "{m}");
    }
    assert!(!r1.iter().any(|x| x == "REJECT"));
    assert!(!has_country(&r1));
    assert!(r2.iter().any(|x| x == NODE_SELECT));
    assert!(!has_country(&r2));
}

#[tokio::test]
async fn singbox_node_select_keeps_country_groups() {
    let (builder, _) = singbox_build(&options(), &MockFetcher::default()).await;
    assert!(has_country(&strs(outbound(&builder.core().config, NODE_SELECT).get("outbounds"))));
}

#[tokio::test]
async fn clash_custom_rules_exclude_country_groups() {
    let (builder, _) = clash_build(&options(), &MockFetcher::default()).await;
    let config = &builder.core().config;
    let r1 = strs(group(config, "Custom-Rule-1").get("proxies"));
    group(config, "Custom-Rule-2");
    for m in [NODE_SELECT, AUTO_SELECT, "DIRECT", "REJECT"] {
        assert!(r1.iter().any(|x| x == m), "{m}");
    }
    assert!(!has_country(&r1));
}

#[tokio::test]
async fn clash_node_select_keeps_country_groups() {
    let (builder, _) = clash_build(&options(), &MockFetcher::default()).await;
    assert!(has_country(&strs(group(&builder.core().config, NODE_SELECT).get("proxies"))));
}

#[tokio::test]
async fn surge_custom_rules_exclude_country_groups() {
    let (builder, _) = surge_build(&options(), &MockFetcher::default()).await;
    let groups = strs(builder.core().config.get("proxy-groups"));
    let line = |name: &str| groups.iter().find(|g| g.split('=').next().unwrap().trim() == name).cloned();
    let r1 = line("Custom-Rule-1").expect("Custom-Rule-1");
    assert!(line("Custom-Rule-2").is_some());
    assert!(r1.contains(NODE_SELECT));
    assert!(!r1.contains("🇺🇸"));
    assert!(!r1.contains("🇬🇧"));
}
