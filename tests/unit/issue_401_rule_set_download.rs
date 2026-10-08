//! test/issue-401-rule-set-download.test.js

use crate::common::*;
use sublink::builders::BuildOptions;
use sublink::config::sing_box_config;
use sublink::hono::Request;
use sublink::js::{Value, json};

const VLESS: &str =
    "vless://12345678-1234-1234-1234-123456789abc@example.com:443?security=tls&sni=example.com#TestVless";
const VMESS: &str = "vmess://ew0KICAidiI6ICIyIiwNCiAgInBzIjogInRlc3QiLA0KICAiYWRkIjogIjEuMS4xLjEiLA0KICAicG9ydCI6ICI0NDMiLA0KICAiaWQiOiAiYWRkNjY2NjYtODg4OC04ODg4LTg4ODgtODg4ODg4ODg4ODg4IiwNCiAgImFpZCI6ICIwIiwNCiAgInNjeSI6ICJhdXRvIiwNCiAgIm5ldCI6ICJ3cyIsDQogICJ0eXBlIjogIm5vbmUiLA0KICAiaG9zdCI6ICIiLA0KICAicGF0aCI6ICIvIiwNCiAgInRscyI6ICJ0bHMiDQp9";

async fn build_with_version(version: &str, base_config: Value) -> Value {
    singbox(&BuildOptions {
        user_agent: String::new(),
        singbox_version: version.into(),
        base_config,
        ..opts(VLESS, v("[]"))
    })
    .await
}

fn assert_remote_sets_use_detour(config: &Value) {
    let sets = config.get("route").get("rule_set").as_array().unwrap();
    assert!(!sets.is_empty());
    for rs in sets {
        if rs.get("type").as_str() == Some("remote") {
            assert_eq!(rs.get("download_detour").as_str(), Some("DIRECT"));
            assert!(!has_prop(rs, "http_client"));
        }
    }
}

#[tokio::test]
async fn adds_download_detour_on_default_1_12_tier() {
    let config = build_with_version("1.12", Value::Null).await;
    assert_remote_sets_use_detour(&config);
    assert!(!has_prop(&config, "http_clients"));
    assert!(!has_prop(config.get("route"), "default_http_client"));
}

#[tokio::test]
async fn adds_download_detour_on_1_11_tier() {
    assert_remote_sets_use_detour(&build_with_version("1.11", Value::Null).await);
}

#[tokio::test]
async fn uses_shared_http_client_on_1_14_tier() {
    let config = build_with_version("1.14", Value::Null).await;
    for rs in config.get("route").get("rule_set").as_array().unwrap() {
        assert!(!has_prop(rs, "download_detour"));
        assert!(!has_prop(rs, "http_client"));
    }
    assert_json(config.get("http_clients"), r#"[{"tag":"rule-set-download","detour":"DIRECT"}]"#);
    assert_eq!(config.get("route").get("default_http_client").as_str(), Some("rule-set-download"));
}

#[tokio::test]
async fn respects_existing_http_clients_on_1_14_tier() {
    let mut base_config = sing_box_config().clone();
    base_config.as_object_mut().unwrap().set("http_clients", v(r#"[{"tag":"my-client","detour":"DIRECT"}]"#));
    let config = build_with_version("1.14", base_config).await;
    assert_json(config.get("http_clients"), r#"[{"tag":"my-client","detour":"DIRECT"}]"#);
    assert_eq!(config.get("route").get("default_http_client").as_str(), Some("my-client"));
}

async fn fetch_config(query: &str, user_agent: Option<&str>) -> Value {
    let app = test_app(MockFetcher::new());
    let mut req = Request::get(&format!("http://localhost/singbox?config={}{query}", enc(VMESS)));
    if let Some(ua) = user_agent {
        req = req.with_header("User-Agent", ua);
    }
    let res = app.handle(&req).await;
    assert_eq!(res.status, 200);
    json::parse(&res.text()).unwrap()
}

#[tokio::test]
async fn returns_1_14_shape_for_sb_ver_1_14_ua_or_latest() {
    for config in [
        fetch_config("&sb_ver=1.14", None).await,
        fetch_config("", Some("SFA/1.14.0 (100; sing-box 1.14.0; language zh_Hans_CN)")).await,
        fetch_config("&sb_ver=latest", None).await,
    ] {
        assert_eq!(config.get("route").get("default_http_client").as_str(), Some("rule-set-download"));
    }
}

#[tokio::test]
async fn keeps_download_detour_for_1_12_or_undetectable_version() {
    for config in [fetch_config("&sb_ver=1.12", None).await, fetch_config("", None).await] {
        assert!(!has_prop(&config, "http_clients"));
        for rs in config.get("route").get("rule_set").as_array().unwrap() {
            if rs.get("type").as_str() == Some("remote") {
                assert_eq!(rs.get("download_detour").as_str(), Some("DIRECT"));
            }
        }
    }
}
