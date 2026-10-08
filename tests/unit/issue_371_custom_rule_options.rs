//! test/issue-371-custom-rule-options.test.js

use crate::common::*;
use sublink::builders::{BuildOptions, ConfigBuilder};

const INPUT: &str = "ss://YWVzLTI1Ni1nY206dGVzdA==@us1.example.com:8388#US-Node-1\nss://YWVzLTI1Ni1nY206dGVzdA==@uk1.example.com:8388#UK-Node-1";
const SINGBOX_COUNTRY: &[&str] = &["🚀 节点选择", "⚡ 自动选择", "🖐️ 手动切换", "US-Node-1", "UK-Node-1", "DIRECT"];
const SINGBOX_FLAT: &[&str] = &["🚀 节点选择", "⚡ 自动选择", "US-Node-1", "UK-Node-1", "DIRECT"];

fn options(group_by_country: bool) -> BuildOptions {
    BuildOptions {
        custom_rules: v(
            r#"[{"name":"Custom-Rule","site_rules":["google"],"ip_rules":[],"domain_suffix":[],"domain_keyword":[]}]"#,
        )
        .as_array()
        .unwrap()
        .clone(),
        user_agent: String::new(),
        group_by_country,
        ..opts(INPUT, "minimal".into())
    }
}

fn with_reject(members: &[&str]) -> Vec<String> {
    members.iter().map(|s| s.to_string()).chain(["REJECT".to_string()]).collect()
}

#[track_caller]
fn assert_complete_without_countries(members: &[String], expected: &[String]) {
    assert_eq!(members, expected);
    assert!(
        !members.iter().any(|m| m.contains("🇺🇸")
            || m.contains("🇬🇧")
            || m.contains("United States")
            || m.contains("United Kingdom"))
    );
}

#[tokio::test]
async fn singbox_custom_rule_members() {
    for (country, expected) in [(true, SINGBOX_COUNTRY), (false, SINGBOX_FLAT)] {
        let (builder, _) = singbox_build(&options(country), &MockFetcher::default()).await;
        let rule = outbound(&builder.core().config, "Custom-Rule");
        let expected: Vec<String> = expected.iter().map(|s| s.to_string()).collect();
        assert_complete_without_countries(&strs(rule.get("outbounds")), &expected);
    }
}

#[tokio::test]
async fn clash_custom_rule_members() {
    for (country, expected) in [(true, with_reject(SINGBOX_COUNTRY)), (false, with_reject(SINGBOX_FLAT))] {
        let (builder, _) = clash_build(&options(country), &MockFetcher::default()).await;
        let rule = group(&builder.core().config, "Custom-Rule");
        assert_complete_without_countries(&strs(rule.get("proxies")), &expected);
    }
}

#[tokio::test]
async fn surge_custom_rule_members() {
    for (country, expected) in [(true, with_reject(SINGBOX_COUNTRY)), (false, with_reject(SINGBOX_FLAT))] {
        let (builder, _) = surge_build(&options(country), &MockFetcher::default()).await;
        let groups = strs(builder.core().config.get("proxy-groups"));
        let line = groups.iter().find(|g| g.starts_with("Custom-Rule = select")).expect("custom rule group");
        let members: Vec<String> = line.split(',').skip(1).map(|m| m.trim().to_string()).collect();
        assert_complete_without_countries(&members, &expected);
    }
}
