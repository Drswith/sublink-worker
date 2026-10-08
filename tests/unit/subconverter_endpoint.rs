//! test/subconverter-endpoint.test.js

use crate::common::*;
use sublink::config::predefined_rule_set;

async fn get(query: &str) -> sublink::hono::Response {
    request(&test_app(MockFetcher::new()), &format!("http://localhost/subconverter{query}")).await
}

async fn text(query: &str) -> String {
    get(query).await.text()
}

fn line<'a>(text: &'a str, a: &str, b: &str) -> &'a str {
    text.split('\n').find(|l| l.contains(a) && l.contains(b)).unwrap_or_else(|| panic!("no line with {a} and {b}"))
}

fn re(pattern: &str) -> regex::Regex {
    regex::Regex::new(pattern).unwrap()
}

#[tokio::test]
async fn returns_text_plain_ini() {
    let res = get("").await;
    assert_eq!(res.status, 200);
    assert!(res.header("content-type").unwrap().contains("text/plain"));
    let text = res.text();
    for s in ["[custom]", "enable_rule_generator=true", "overwrite_original_rules=true"] {
        assert!(text.contains(s), "{s}");
    }
}

#[tokio::test]
async fn defaults_to_balanced_preset() {
    let text = text("").await;
    assert!(predefined_rule_set("balanced").unwrap().length().unwrap() > 0);
    assert!(text.contains("ruleset="));
    for s in ["GEOSITE,google", "GEOSITE,youtube", "GEOIP,telegram"] {
        assert!(text.contains(s), "{s}");
    }
}

#[tokio::test]
async fn accepts_minimal_preset() {
    let text = text("?selectedRules=minimal").await;
    for s in ["GEOSITE,geolocation-cn", "GEOIP,private", "GEOSITE,geolocation-!cn"] {
        assert!(text.contains(s), "{s}");
    }
    assert!(!text.contains("GEOSITE,google"));
    assert!(!text.contains("GEOSITE,youtube"));
}

#[tokio::test]
async fn accepts_comprehensive_preset() {
    let text = text("?selectedRules=comprehensive").await;
    for s in [
        "GEOSITE,category-ads-all",
        "GEOSITE,category-ai-!cn",
        "GEOSITE,google",
        "GEOSITE,bilibili",
        "GEOSITE,youtube",
        "GEOSITE,netflix",
        "GEOSITE,steam",
        "GEOIP,telegram",
    ] {
        assert!(text.contains(s), "{s}");
    }
}

#[tokio::test]
async fn accepts_json_array_selected_rules() {
    let text = text(&format!("?selectedRules={}", enc(r#"["Google","Telegram"]"#))).await;
    for s in ["GEOSITE,google", "GEOIP,google", "GEOIP,telegram"] {
        assert!(text.contains(s), "{s}");
    }
    assert!(!text.contains("GEOSITE,youtube"));
    assert!(!text.contains("GEOSITE,bilibili"));
}

#[tokio::test]
async fn emits_src_ip_cidr_custom_rules() {
    let text = text(&format!(
        "?selectedRules=minimal&customRules={}",
        enc(r#"[{"name":"LAN","src_ip_cidr":"192.168.1.13/32"}]"#)
    ))
    .await;
    assert!(text.contains("ruleset=LAN,[]SRC-IP-CIDR,192.168.1.13/32"));
}

#[tokio::test]
async fn generates_proxy_group_structure() {
    let text = text("?selectedRules=minimal").await;
    assert!(re("custom_proxy_group=.*节点选择.*select").is_match(&text));
    assert!(re("custom_proxy_group=.*自动选择.*url-test").is_match(&text));
    assert!(re("custom_proxy_group=.*漏网之鱼.*select").is_match(&text));
    assert!(text.contains("[]FINAL"));
}

#[tokio::test]
async fn maps_ad_block_to_reject() {
    let text = text(&format!("?selectedRules={}", enc(r#"["Ad Block","Google"]"#))).await;
    assert!(re(r"custom_proxy_group=.*广告拦截.*select.*\[\]REJECT").is_match(&text));
}

#[tokio::test]
async fn maps_private_and_location_cn_to_direct() {
    let text = text(&format!("?selectedRules={}", enc(r#"["Private","Location:CN","Google"]"#))).await;
    assert!(re(r"custom_proxy_group=.*私有网络.*select.*\[\]DIRECT").is_match(&text));
    assert!(re(r"custom_proxy_group=.*国内服务.*select.*\[\]DIRECT").is_match(&text));
}

#[tokio::test]
async fn maps_other_rules_to_node_select() {
    let text = text(&format!("?selectedRules={}", enc(r#"["Google"]"#))).await;
    assert!(re(r"custom_proxy_group=.*谷歌服务.*select.*\[\].*节点选择").is_match(&text));
}

#[tokio::test]
async fn respects_include_auto_select_false() {
    let text = text("?selectedRules=minimal&include_auto_select=false").await;
    assert!(!re("custom_proxy_group=.*自动选择.*url-test").is_match(&text));
    assert!(!line(&text, "节点选择", "custom_proxy_group").contains("自动选择"));
}

#[tokio::test]
async fn orders_domain_rules_before_ip_rules() {
    let text = text(&format!("?selectedRules={}", enc(r#"["Google"]"#))).await;
    let site = text.find("GEOSITE,google").unwrap();
    let ip = text.find("GEOIP,google").unwrap();
    assert!(site < ip);
}

#[tokio::test]
async fn supports_lang_parameter() {
    let text = text("?selectedRules=minimal&lang=en").await;
    for s in ["Node Select", "Auto Select", "Fall Back"] {
        assert!(text.contains(s), "{s}");
    }
}

#[tokio::test]
async fn country_groups_use_regex_patterns() {
    let text = text("?selectedRules=minimal&group_by_country=true").await;
    assert!(re(r"custom_proxy_group=🇭🇰 Hong Kong`url-test`\(\?i\)\(香港\|\\bHong Kong\\b\|\\bHK\\b\)").is_match(&text));
    assert!(re(r"custom_proxy_group=🇯🇵 Japan`url-test`\(\?i\)\(日本\|\\bJapan\\b\|\\bJP\\b\)").is_match(&text));
    assert!(
        re(r"custom_proxy_group=🇺🇸 United States`url-test`\(\?i\)\(美国\|\\bUnited States\\b\|\\bUS\\b\)")
            .is_match(&text)
    );
}

#[tokio::test]
async fn manual_switch_selects_all_nodes() {
    let text = text("?selectedRules=minimal&group_by_country=true").await;
    assert!(re(r"custom_proxy_group=.*手动切换.*`select`\.\*").is_match(&text));
}

#[tokio::test]
async fn node_select_references_country_groups() {
    let text = text("?selectedRules=minimal&group_by_country=true").await;
    let l = line(&text, "节点选择", "`select`");
    assert!(l.contains("[]🇭🇰 Hong Kong"));
    assert!(l.contains("[]🇯🇵 Japan"));
    assert!(!l.ends_with(".*"));
    assert!(!l.contains("`.*`"));
}

#[tokio::test]
async fn outbound_groups_reference_country_groups() {
    let text = text(&format!("?selectedRules={}&group_by_country=true", enc(r#"["Google"]"#))).await;
    let l = line(&text, "谷歌服务", "`select`");
    assert!(l.contains("[]🇭🇰 Hong Kong"));
    assert!(l.contains("[]🇺🇸 United States"));
    assert!(!l.contains("`.*"));
}

#[tokio::test]
async fn country_groups_without_auto_select() {
    let text = text("?selectedRules=minimal&group_by_country=true&include_auto_select=false").await;
    assert!(!re("custom_proxy_group=.*自动选择.*url-test").is_match(&text));
    let l = line(&text, "节点选择", "`select`");
    assert!(l.contains("手动切换"));
    assert!(!l.contains("自动选择"));
}

#[tokio::test]
async fn generates_all_30_country_groups() {
    let text = text("?selectedRules=minimal&group_by_country=true").await;
    assert_eq!(re(r"custom_proxy_group=.+`url-test`\(\?i\)\(.+\)`http").find_iter(&text).count(), 30);
}

#[tokio::test]
async fn english_group_names_with_lang_en() {
    let text = text("?selectedRules=minimal&group_by_country=true&lang=en").await;
    for s in ["Manual Switch", "Node Select", "🇯🇵 Japan"] {
        assert!(text.contains(s), "{s}");
    }
}

#[tokio::test]
async fn rejects_invalid_selected_rules() {
    let res = get("?selectedRules=balancde").await;
    assert_eq!(res.status, 400);
    let body = res.text();
    assert!(body.contains("Invalid selectedRules") && body.contains("balancde"));

    let res = get("?selectedRules=foobar").await;
    assert_eq!(res.status, 400);
    assert!(res.text().contains("Invalid selectedRules"));

    let res = get(&format!("?selectedRules={}", enc(r#"{"rule":"Google"}"#))).await;
    assert_eq!(res.status, 400);
    assert!(res.text().contains("must be a preset name"));
}

#[tokio::test]
async fn defaults_to_balanced_without_selected_rules() {
    let res = get("").await;
    assert_eq!(res.status, 200);
    let body = res.text();
    assert!(body.contains("GEOSITE,google") && body.contains("GEOSITE,youtube"));
}
