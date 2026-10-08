//! test/surge-input-parsing.test.js

use crate::common::*;
use sublink::builders::BuildOptions;
use sublink::js::Value;
use sublink::parsers::content::parse_surge_ini;
use sublink::parsers::surge::convert_surge_proxy_to_object;

const SAMPLE: &str = "
[General]
loglevel = notify
skip-proxy = 127.0.0.1, localhost

[Proxy]
HK-SS = ss, hk.example.com, 443, encrypt-method=aes-256-gcm, password=test-password
US-VMess = vmess, us.example.com, 443, username=12345678-1234-1234-1234-123456789abc, tls=true
JP-Trojan = trojan, jp.example.com, 443, password=trojan-password, sni=jp.example.com
TW-TUIC = tuic, tw.example.com, 443, uuid=tuic-uuid, password=tuic-pass, congestion-controller=bbr
SG-HY2 = hysteria2, sg.example.com, 443, password=hy2-password, sni=sg.example.com

[Proxy Group]
Proxy = select, HK-SS, US-VMess, JP-Trojan

[Rule]
DOMAIN-SUFFIX,google.com,Proxy
FINAL,DIRECT
";

fn surge_line(line: &str) -> Value {
    convert_surge_proxy_to_object(&Value::str(line))
}

#[test]
fn parses_shadowsocks_line() {
    let r = surge_line("SS-Node = ss, example.com, 8388, encrypt-method=aes-256-gcm, password=test123");
    assert_match_object(
        &r,
        r#"{"tag":"SS-Node","type":"shadowsocks","server":"example.com","server_port":8388,"method":"aes-256-gcm","password":"test123"}"#,
    );
}

#[test]
fn parses_vmess_line() {
    let r = surge_line("VMess-Node = vmess, example.com, 443, username=test-uuid, tls=true, sni=vmess.example.com");
    assert_match_object(
        &r,
        r#"{"tag":"VMess-Node","type":"vmess","server":"example.com","server_port":443,"uuid":"test-uuid","tls":{"enabled":true,"server_name":"vmess.example.com"}}"#,
    );
}

#[test]
fn parses_trojan_line() {
    let r = surge_line("Trojan-Node = trojan, example.com, 443, password=trojan-pass, sni=trojan.example.com");
    assert_match_object(
        &r,
        r#"{"tag":"Trojan-Node","type":"trojan","password":"trojan-pass","tls":{"server_name":"trojan.example.com"}}"#,
    );
}

#[test]
fn parses_tuic_line() {
    let r = surge_line("TUIC-Node = tuic, example.com, 443, uuid=my-uuid, password=my-pass, congestion-controller=bbr");
    assert_match_object(
        &r,
        r#"{"tag":"TUIC-Node","type":"tuic","uuid":"my-uuid","password":"my-pass","congestion_control":"bbr"}"#,
    );
}

#[test]
fn parses_hysteria2_line() {
    let r = surge_line("HY2-Node = hysteria2, example.com, 443, password=hy2-pass, sni=hy2.example.com");
    assert_match_object(
        &r,
        r#"{"tag":"HY2-Node","type":"hysteria2","password":"hy2-pass","tls":{"server_name":"hy2.example.com"}}"#,
    );
}

#[test]
fn returns_null_for_invalid_lines() {
    for line in ["", "# comment", "invalid line", "DIRECT = direct"] {
        assert!(surge_line(line).is_null(), "{line:?}");
    }
}

#[test]
fn handles_case_insensitive_booleans() {
    assert_json(
        surge_line("VMess-Upper = vmess, example.com, 443, username=uuid, tls=TRUE").get("tls").get("enabled"),
        "true",
    );
    assert_json(
        surge_line("VMess-Mixed = vmess, example.com, 443, username=uuid, tls=True").get("tls").get("enabled"),
        "true",
    );
    assert_json(
        surge_line("Trojan-Test = trojan, example.com, 443, password=pass, skip-cert-verify=TRUE")
            .get("tls")
            .get("insecure"),
        "true",
    );
}

#[test]
fn parse_surge_ini_extracts_proxies_and_types() {
    let r = parse_surge_ini(SAMPLE);
    assert_eq!(r.get("type").as_str(), Some("surgeConfig"));
    assert_eq!(r.get("proxies").length(), Some(5));
    let types = names(r.get("proxies"), "type");
    for t in ["shadowsocks", "vmess", "trojan", "tuic", "hysteria2"] {
        assert!(types.iter().any(|x| x == t), "{t}");
    }
}

#[test]
fn parse_surge_ini_preserves_config_overrides() {
    let r = parse_surge_ini(SAMPLE);
    assert!(!r.get("config").is_null());
    assert!(!r.get("config").get("general").is_undefined());
    assert!(!r.get("config").get("rules").is_undefined());
}

#[test]
fn parse_surge_ini_converts_proxy_groups_to_objects() {
    let r = parse_surge_ini(SAMPLE);
    let groups = r.get("config").get("proxy-groups");
    assert_eq!(groups.length(), Some(1));
    let g = &groups.as_array().unwrap()[0];
    assert_eq!(g.get("name").as_str(), Some("Proxy"));
    assert_eq!(g.get("type").as_str(), Some("select"));
    for p in ["HK-SS", "US-VMess", "JP-Trojan"] {
        assert!(has(g.get("proxies"), p));
    }
}

#[test]
fn parse_surge_ini_returns_null_for_non_surge_content() {
    for content in ["not a surge config", r#"{"outbounds": []}"#, "proxies:\n  - name: test"] {
        assert!(parse_surge_ini(content).is_null(), "{content:?}");
    }
}

fn options() -> BuildOptions {
    BuildOptions { user_agent: String::new(), ..opts(SAMPLE, v("[]")) }
}

#[tokio::test]
async fn works_with_singbox_builder() {
    let config = singbox(&options()).await;
    let proxies: Vec<&Value> =
        config.get("outbounds").as_array().unwrap().iter().filter(|o| o.get("server").truthy()).collect();
    assert_eq!(proxies.len(), 5);
    let tags: Vec<&str> = proxies.iter().filter_map(|p| p.get("tag").as_str()).collect();
    for t in ["HK-SS", "US-VMess", "JP-Trojan"] {
        assert!(tags.contains(&t), "{t}");
    }
}

#[tokio::test]
async fn works_with_clash_builder() {
    let config = clash(&options()).await;
    let proxy_names = names(config.get("proxies"), "name");
    assert_eq!(proxy_names.len(), 5);
    assert!(proxy_names.contains(&"HK-SS".to_string()));
    assert!(proxy_names.contains(&"US-VMess".to_string()));
}
