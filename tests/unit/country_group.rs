//! test/country-group.test.js

use crate::common::*;
use sublink::builders::BuildOptions;
use sublink::utils::{default_proxy_name, group_proxies_by_country, parse_country_from_node_name};

#[tokio::test]
async fn groups_proxies_by_country() {
    let input = "
ss://YWVzLTEyOC1nY206dGVzdA@example.com:443#HK-Node-1
ss://YWVzLTEyOC1nY206dGVzdA@example.com:444#香港节点2
ss://YWVzLTEyOC1nY206dGVzdA@example.com:445#US-Node-1
trojan://password@example.com:443?sni=example.com#美国节点2
vmess://ewogICJ2IjogIjIiLAogICJwcyI6ICJ0dzEubm9kZS5jb20iLAogICJhZGQiOiAidHcxLm5vZGUuY29tIiwKICAicG9ydCI6IDQ0MywKICAiaWQiOiAiZGE4Y2FkMTYtYjEzNS00MmZlLWEzYjYtNzUyZGFhY2E5MGIwIiwKICAiYWlkIjogMCwKICAibmV0IjogIndzIiwKICAidHlwZSI6ICJub25lIiwKICAiaG9zdCI6ICJ0dzEubm9kZS5jb20iLAogICJwYXRoIjogIi92bWVzcyIsCiAgInRscyI6ICJ0bHMiCn0=#台湾节点
    ";
    let built = clash(&BuildOptions { group_by_country: true, ..opts(input, "all".into()) }).await;
    let proxies_count = built.get("proxies").length().unwrap();
    assert!(proxies_count > 0);

    for (name, count) in [("🇭🇰 Hong Kong", 2), ("🇺🇸 United States", 2), ("🇹🇼 Taiwan", 1)] {
        let g = group(&built, name);
        assert_eq!(g.get("proxies").length(), Some(count), "{name}");
        assert_eq!(g.get("type").as_str(), Some("url-test"));
    }

    let manual_name = t("outboundNames.Manual Switch");
    let manual = group(&built, &manual_name);
    assert_eq!(manual.get("type").as_str(), Some("select"));
    assert_eq!(manual.get("proxies").length(), Some(proxies_count));

    let node_select_label = t("outboundNames.Node Select");
    let auto_name = t("outboundNames.Auto Select");
    let mut actual = strs(group(&built, &node_select_label).get("proxies"));
    let mut expected = vec![
        "DIRECT".to_string(),
        "REJECT".into(),
        auto_name.clone(),
        manual_name.clone(),
        "🇭🇰 Hong Kong".into(),
        "🇹🇼 Taiwan".into(),
        "🇺🇸 United States".into(),
    ];
    actual.sort();
    expected.sort();
    assert_eq!(actual, expected);

    if let Some(youtube) = find(built.get("proxy-groups"), "name", &t("outboundNames.Youtube")) {
        let members = strs(youtube.get("proxies"));
        for name in [&node_select_label, &auto_name, &manual_name, "🇭🇰 Hong Kong", "🇹🇼 Taiwan", "🇺🇸 United States"]
        {
            assert!(members.iter().any(|m| m == name), "youtube group misses {name}");
        }
    }
}

#[test]
fn group_proxies_by_country_normalizes_names() {
    let sample = v(
        r#"[{"name":"HK-Node-1"},{"tag":"香港节点2"},"US-Node-1 = ss, example.com, 443","台湾节点 = trojan, example.com, 443"]"#,
    );
    let grouped = group_proxies_by_country(sample.as_array().unwrap(), default_proxy_name);
    for key in ["Hong Kong", "United States", "Taiwan"] {
        assert!(grouped.contains_key(key), "{key}");
    }
    assert_eq!(grouped["Hong Kong"].proxies.len(), 2);
    assert_eq!(grouped["United States"].proxies.len(), 1);
    assert_eq!(grouped["Taiwan"].proxies.len(), 1);
}

fn code(name: &str) -> Option<&'static str> {
    parse_country_from_node_name(name).map(|c| c.code)
}

#[test]
fn does_not_match_us_inside_plus() {
    for name in ["plus-node-1", "surplus", "focus"] {
        assert_eq!(code(name), None, "{name}");
    }
}

#[test]
fn does_not_match_jp_inside_vjp123() {
    assert_eq!(code("VJP123"), None);
}

#[test]
fn does_not_match_in_inside_main_or_point() {
    assert_ne!(code("main-server"), Some("IN"));
    assert_ne!(code("endpoint-1"), Some("IN"));
}

#[test]
fn still_matches_short_codes_with_proper_delimiters() {
    assert_eq!(code("US-Node-1"), Some("US"));
    assert_eq!(code("HK 01"), Some("HK"));
    assert_eq!(code("node-JP-fast"), Some("JP"));
    assert_eq!(code("SG|premium"), Some("SG"));
}

#[test]
fn matches_chinese_aliases() {
    assert_eq!(code("香港节点1"), Some("HK"));
    assert_eq!(code("日本高速"), Some("JP"));
    assert_eq!(code("新加坡专线"), Some("SG"));
}

#[test]
fn prefers_longer_alias_over_shorter() {
    assert_eq!(code("Indonesia-1"), Some("ID"));
    assert_eq!(code("印度尼西亚节点"), Some("ID"));
}
