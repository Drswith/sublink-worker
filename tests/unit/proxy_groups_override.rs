//! test/proxy-groups-override.test.js

use crate::common::*;
use sublink::builders::clash::ClashBuilder;
use sublink::builders::merge_dns_config;
use sublink::js::{ErrorKind, Value};
use sublink::parsers::content::{parse_clash_yaml, parse_singbox_json, parse_surge_ini};

fn assert_group(config: &Value, name: &str, ty: &str) {
    let g = find(config.get("proxy-groups"), "name", name).unwrap_or_else(|| panic!("missing group {name}"));
    assert_eq!(g.get("type").as_str(), Some(ty), "{name}");
    assert!(has(g.get("proxies"), "HK-Node"), "{name}");
}

#[test]
fn parse_clash_yaml_preserves_proxy_groups() {
    let r = parse_clash_yaml(
        "
proxies:
  - name: HK-Node
    type: ss
    server: hk.example.com
    port: 443
    cipher: aes-128-gcm
    password: test
proxy-groups:
  - name: 自定义选择
    type: select
    proxies:
      - DIRECT
      - REJECT
      - HK-Node
  - name: 自动测速
    type: url-test
    proxies:
      - HK-Node
    url: http://www.gstatic.com/generate_204
    interval: 300
",
    );
    assert_eq!(r.get("type").as_str(), Some("yamlConfig"));
    assert_eq!(r.get("proxies").length(), Some(1));
    assert_eq!(r.get("config").get("proxy-groups").length(), Some(2));
    assert_group(r.get("config"), "自定义选择", "select");
}

#[test]
fn parse_singbox_json_converts_selector_and_urltest_to_groups() {
    let r = parse_singbox_json(
        r#"{"outbounds":[
        {"type":"shadowsocks","tag":"HK-Node","server":"hk.example.com","server_port":443,"method":"aes-128-gcm","password":"test"},
        {"type":"selector","tag":"自定义选择","outbounds":["DIRECT","REJECT","HK-Node"]},
        {"type":"urltest","tag":"自动测速","outbounds":["HK-Node"],"url":"http://www.gstatic.com/generate_204","interval":"5m"},
        {"type":"direct","tag":"DIRECT"},{"type":"block","tag":"REJECT"}]}"#,
    );
    assert_eq!(r.get("type").as_str(), Some("singboxConfig"));
    assert_eq!(r.get("proxies").length(), Some(1));
    assert_eq!(r.get("proxies").as_array().unwrap()[0].get("tag").as_str(), Some("HK-Node"));
    assert_eq!(r.get("config").get("proxy-groups").length(), Some(2));
    assert_group(r.get("config"), "自定义选择", "select");
    assert_group(r.get("config"), "自动测速", "url-test");
}

#[test]
fn parse_surge_ini_parses_group_strings_into_objects() {
    let r = parse_surge_ini(
        "
[General]
loglevel = notify

[Proxy]
HK-Node = ss, hk.example.com, 443, encrypt-method=aes-128-gcm, password=test

[Proxy Group]
自定义选择 = select, DIRECT, REJECT, HK-Node
自动测速 = url-test, HK-Node, url=http://www.gstatic.com/generate_204, interval=300

[Rule]
FINAL,DIRECT
",
    );
    assert_eq!(r.get("type").as_str(), Some("surgeConfig"));
    assert_eq!(r.get("proxies").length(), Some(1));
    assert_eq!(r.get("config").get("proxy-groups").length(), Some(2));
    assert_group(r.get("config"), "自定义选择", "select");
    assert_group(r.get("config"), "自动测速", "url-test");
}

const CLASH_INPUT: &str = "
proxies:
  - name: HK-Node
    type: ss
    server: hk.example.com
    port: 443
    cipher: aes-128-gcm
    password: test
proxy-groups:
  - name: 自定义选择
    type: select
    proxies:
      - DIRECT
      - REJECT
      - HK-Node
";
const SINGBOX_INPUT: &str = r#"{"outbounds":[
    {"type":"shadowsocks","tag":"HK-Node","server":"hk.example.com","server_port":443,"method":"aes-128-gcm","password":"test"},
    {"type":"selector","tag":"自定义选择","outbounds":["DIRECT","REJECT","HK-Node"]},
    {"type":"direct","tag":"DIRECT"},{"type":"block","tag":"REJECT"}]}"#;
const SURGE_INPUT: &str = "
[General]
loglevel = notify

[Proxy]
HK-Node = ss, hk.example.com, 443, encrypt-method=aes-128-gcm, password=test

[Proxy Group]
自定义选择 = select, DIRECT, REJECT, HK-Node

[Rule]
FINAL,DIRECT
";

#[tokio::test]
async fn clash_preserves_custom_group_from_every_input_format() {
    for input in [CLASH_INPUT, SINGBOX_INPUT, SURGE_INPUT] {
        let built = clash(&opts(input, "minimal".into())).await;
        assert_group(&built, "自定义选择", "select");
    }
}

#[tokio::test]
async fn singbox_includes_proxy_and_standard_groups_from_singbox_input() {
    let config = singbox(&opts(SINGBOX_INPUT, "minimal".into())).await;
    outbound(&config, "HK-Node");
    assert!(find(config.get("outbounds"), "type", "urltest").is_some());
}

#[tokio::test]
async fn surge_includes_proxies_from_surge_input() {
    let text = surge(&opts(SURGE_INPUT, "minimal".into())).await;
    assert!(text.contains("HK-Node"));
    assert!(text.contains("[Proxy Group]"));
}

#[tokio::test]
async fn surge_handles_clash_input_with_object_groups() {
    let text = surge(&opts(CLASH_INPUT, "minimal".into())).await;
    assert!(text.contains("[Proxy Group]"));
    assert!(text.contains("HK-Node"));
    assert!(!text.contains("[object Object]"));
}

#[tokio::test]
async fn surge_with_country_groups_handles_clash_input_groups() {
    let o = sublink::builders::BuildOptions { group_by_country: true, ..opts(CLASH_INPUT, "minimal".into()) };
    let text = surge(&o).await;
    assert!(text.contains("[Proxy Group]"));
    assert!(!text.contains("[object Object]"));
}

#[tokio::test]
async fn merges_user_group_with_system_group_name() {
    let input = "
proxies:
  - name: HK-Node
    type: ss
    server: hk.example.com
    port: 443
    cipher: aes-128-gcm
    password: test
proxy-groups:
  - name: ⚡ 自动选择
    type: url-test
    proxies:
      - HK-Node
    url: http://custom.test/204
    interval: 600
";
    let config = clash(&opts(input, "minimal".into())).await;
    let auto: Vec<&Value> = config
        .get("proxy-groups")
        .as_array()
        .unwrap()
        .iter()
        .filter(|g| g.get("name").as_str().is_some_and(|n| n.contains("自动选择")))
        .collect();
    assert_eq!(auto.len(), 1);
    assert!(has(auto[0].get("proxies"), "HK-Node"));
    assert_json(auto[0].get("interval"), "600");
    assert_eq!(auto[0].get("url").as_str(), Some("http://custom.test/204"));
}

#[tokio::test]
async fn rejects_empty_url_test_groups() {
    let input = "
proxies:
  - name: Node-A
    type: ss
    server: a.example.com
    port: 443
    cipher: aes-128-gcm
    password: test
  - name: Node-B
    type: ss
    server: b.example.com
    port: 443
    cipher: aes-128-gcm
    password: test
proxy-groups:
  - name: Empty Test Group
    type: url-test
    proxies: []
";
    let mut b = ClashBuilder::new(&opts(input, "minimal".into()));
    let err = b.build(&MockFetcher::default()).await.unwrap_err();
    assert_eq!(err.kind, ErrorKind::Service(400));
    assert!(err.message.contains(
        r#"Invalid proxy group "Empty Test Group": type "url-test" requires at least one proxy or provider reference"#
    ));
}

#[tokio::test]
async fn filters_invalid_proxy_references_from_user_groups() {
    let input = "
proxies:
  - name: Valid-Node
    type: ss
    server: valid.example.com
    port: 443
    cipher: aes-128-gcm
    password: test
proxy-groups:
  - name: Custom Group
    type: select
    proxies:
      - DIRECT
      - REJECT
      - Valid-Node
      - NonExistent-Node
      - AnotherMissing
";
    let config = clash(&opts(input, "minimal".into())).await;
    let proxies = group(&config, "Custom Group").get("proxies");
    for kept in ["DIRECT", "REJECT", "Valid-Node"] {
        assert!(has(proxies, kept), "{kept}");
    }
    for dropped in ["NonExistent-Node", "AnotherMissing"] {
        assert!(!has(proxies, dropped), "{dropped}");
    }
}

#[tokio::test]
async fn singbox_auto_select_contains_parsed_proxy() {
    let input = r#"{"outbounds":[{"type":"shadowsocks","tag":"HK-Node","server":"hk.example.com","server_port":443,"method":"aes-128-gcm","password":"test"},{"type":"direct","tag":"DIRECT"},{"type":"block","tag":"REJECT"}]}"#;
    let config = singbox(&opts(input, "minimal".into())).await;
    outbound(&config, "HK-Node");
    let auto = find(config.get("outbounds"), "type", "urltest").expect("urltest outbound");
    assert!(has(auto.get("outbounds"), "HK-Node"));
}

fn count(list: &Value, item: &str) -> usize {
    strs(list).iter().filter(|s| *s == item).count()
}

#[test]
fn merges_dns_nameserver_arrays() {
    let merged = merge_dns_config(
        &v(r#"{"enable":true,"nameserver":["8.8.8.8","8.8.4.4"],"fallback":["1.0.0.1"]}"#),
        &v(r#"{"nameserver":["1.1.1.1","8.8.8.8"],"fallback":["9.9.9.9"]}"#),
    )
    .unwrap();
    for ns in ["8.8.8.8", "8.8.4.4", "1.1.1.1"] {
        assert!(has(merged.get("nameserver"), ns), "{ns}");
    }
    assert_eq!(count(merged.get("nameserver"), "8.8.8.8"), 1);
    assert!(has(merged.get("fallback"), "1.0.0.1") && has(merged.get("fallback"), "9.9.9.9"));
    assert_json(merged.get("enable"), "true");
}

#[test]
fn merges_fake_ip_filter_arrays() {
    let merged = merge_dns_config(
        &v(r#"{"fake-ip-filter":["*.lan","*.local"]}"#),
        &v(r#"{"fake-ip-filter":["*.local","*.internal","localhost"]}"#),
    )
    .unwrap();
    let filter = merged.get("fake-ip-filter");
    for f in ["*.lan", "*.local", "*.internal", "localhost"] {
        assert!(has(filter, f), "{f}");
    }
    assert_eq!(count(filter, "*.local"), 1);
}

#[test]
fn merges_nameserver_policy_objects() {
    let merged = merge_dns_config(
        &v(r#"{"nameserver-policy":{"+.google.com":"8.8.8.8"}}"#),
        &v(r#"{"nameserver-policy":{"+.github.com":"1.1.1.1","+.google.com":"8.8.4.4"}}"#),
    )
    .unwrap();
    let policy = merged.get("nameserver-policy");
    assert_eq!(policy.get("+.github.com").as_str(), Some("1.1.1.1"));
    assert_eq!(policy.get("+.google.com").as_str(), Some("8.8.4.4"));
}

#[test]
fn merge_dns_handles_missing_existing_config() {
    let merged = merge_dns_config(&Value::Null, &v(r#"{"nameserver":["1.1.1.1"],"enable":true}"#)).unwrap();
    assert_json(merged.get("nameserver"), r#"["1.1.1.1"]"#);
    assert_json(merged.get("enable"), "true");
}
