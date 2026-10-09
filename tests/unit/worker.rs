//! test/worker.test.js and test/index.test.js

use crate::common::*;
use sublink::hono::Request;
use sublink::js::json;

const VMESS: &str = "vmess://ew0KICAidiI6ICIyIiwNCiAgInBzIjogInRlc3QiLA0KICAiYWRkIjogIjEuMS4xLjEiLA0KICAicG9ydCI6ICI0NDMiLA0KICAiaWQiOiAiYWRkNjY2NjYtODg4OC04ODg4LTg4ODgtODg4ODg4ODg4ODg4IiwNCiAgImFpZCI6ICIwIiwNCiAgInNjeSI6ICJhdXRvIiwNCiAgIm5ldCI6ICJ3cyIsDQogICJ0eXBlIjogIm5vbmUiLA0KICAiaG9zdCI6ICIiLA0KICAicGF0aCI6ICIvIiwNCiAgInRscyI6ICJ0bHMiDQp9";

fn app() -> sublink::app::App {
    test_app(MockFetcher::new())
}

#[tokio::test]
async fn root_returns_html() {
    for url in ["http://localhost/", "http://example.com/"] {
        let res = request(&app(), url).await;
        assert_eq!(res.status, 200);
        assert!(res.header("content-type").unwrap().contains("text/html"));
        let text = res.text();
        assert!(text.contains("<!DOCTYPE html>"));
        assert!(text.contains("Sublink Worker"));
    }
}

#[tokio::test]
async fn unknown_paths_return_404() {
    assert_eq!(request(&app(), "http://example.com/unknown-path").await.status, 404);
}

#[tokio::test]
async fn singbox_returns_json() {
    let res = request(&app(), &format!("http://localhost/singbox?config={}", enc(VMESS))).await;
    assert_eq!(res.status, 200);
    assert!(res.header("content-type").unwrap().contains("application/json"));
    assert!(has_prop(&json::parse(&res.text()).unwrap(), "outbounds"));
}

#[tokio::test]
async fn singbox_ignores_a_clash_base_config_id() {
    let app = app();
    app.store
        .put("clash_test", r#"{"proxy-groups":[{"name":"Custom","type":"select","proxies":["DIRECT"]}]}"#, None)
        .unwrap();
    let res = request(&app, &format!("http://localhost/singbox?config={}&configId=clash_test", enc(VMESS))).await;
    assert_eq!(res.status, 200);
    let config = json::parse(&res.text()).unwrap();
    assert!(has_prop(config.get("route"), "rule_set"));
}

async fn singbox_with_ua(ua: &str) -> sublink::js::Value {
    let req = Request::get(&format!("http://localhost/singbox?config={}", enc(VMESS))).with_header("User-Agent", ua);
    let res = app().handle(&req).await;
    assert_eq!(res.status, 200);
    json::parse(&res.text()).unwrap()
}

#[tokio::test]
async fn singbox_returns_legacy_config_for_1_11_ua() {
    let config = singbox_with_ua("SFI/1.12.2 (Build 2; sing-box 1.11.4; language zh_CN)").await;
    let server = &config.get("dns").get("servers").as_array().unwrap()[0];
    assert!(has_prop(server, "address"));
    assert!(!has_prop(server, "type"));
    assert!(!has_prop(config.get("route"), "default_domain_resolver"));
}

#[tokio::test]
async fn singbox_returns_1_12_config_for_1_12_ua() {
    let config = singbox_with_ua("SFA/1.12.12 (587; sing-box 1.12.12; language zh_Hans_CN)").await;
    let server = &config.get("dns").get("servers").as_array().unwrap()[0];
    assert!(has_prop(server, "type"));
    assert!(!has_prop(server, "address"));
    assert_eq!(config.get("route").get("default_domain_resolver").as_str(), Some("dns_resolver"));
}

#[tokio::test]
async fn clash_returns_yaml() {
    let res = request(&app(), &format!("http://localhost/clash?config={}", enc(VMESS))).await;
    assert_eq!(res.status, 200);
    assert!(res.header("content-type").unwrap().contains("text/yaml"));
    assert!(res.text().contains("proxies:"));
}

#[tokio::test]
async fn clash_can_omit_generated_dns_config() {
    let config = enc("ss://YWVzLTEyOC1nY206dGVzdA@example.com:443#TestSS");
    let default_res = request(&app(), &format!("http://localhost/clash?config={config}")).await;
    assert_eq!(default_res.status, 200);
    assert!(has_prop(&sublink::yaml::load(&default_res.text()).unwrap(), "dns"));

    let no_dns_res = request(&app(), &format!("http://localhost/clash?config={config}&include_clash_dns=false")).await;
    assert_eq!(no_dns_res.status, 200);
    assert!(!has_prop(&sublink::yaml::load(&no_dns_res.text()).unwrap(), "dns"));
}

#[tokio::test]
async fn clash_rejects_empty_url_test_groups_with_diagnostic() {
    let config = "
proxies:
  - name: Node-A
    type: ss
    server: a.example.com
    port: 443
    cipher: aes-128-gcm
    password: test
proxy-groups:
  - name: Empty Test Group
    type: url-test
    proxies: []
";
    let res = request(&app(), &format!("http://localhost/clash?config={}", enc(config))).await;
    assert_eq!(res.status, 400);
    let text = res.text();
    assert!(text.contains(r#"Invalid proxy group "Empty Test Group""#));
    assert!(text.contains("requires at least one proxy or provider reference"));
}

#[tokio::test]
async fn shorten_v2_returns_short_code() {
    let app = app();
    let res = request(&app, &format!("http://localhost/shorten-v2?url={}", enc("http://example.com"))).await;
    assert_eq!(res.status, 200);
    let code = res.text();
    assert!(!code.is_empty());
    assert!(app.store.get(&code).unwrap().is_some(), "short code must be stored");
}

#[test]
fn request_urls_with_credentials_are_rejected() {
    // The Node entry built `new Request(url)` from the Host header, which throws (500) for userinfo.
    assert!(Request::new("GET", "http://user@host/clash").is_err());
    assert!(Request::new("GET", "http://:p@host/").is_err());
    assert!(Request::new("GET", "http://@host/").is_ok());
}
