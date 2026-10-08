//! test/issue-362-subscription-userinfo.test.js

use crate::common::*;
use sublink::js::base64::decode_base64;
use sublink::js::json;

const USERINFO: &str = "upload=123; download=456; total=1024; expire=1893456000";
const REMOTE: &str = "https://airport.example.com/sub?token=abc";
const PROXY: &str = "ss://YWVzLTEyOC1nY206cGFzcw@example.com:443#Issue362";

fn app() -> sublink::app::App {
    let fetcher = MockFetcher::new();
    fetcher.route(REMOTE, 200, &[("subscription-userinfo", USERINFO)], PROXY);
    test_app(fetcher)
}

#[tokio::test]
async fn preserves_userinfo_for_clash() {
    let res = request(&app(), &format!("http://localhost/clash?config={}", enc(REMOTE))).await;
    assert_eq!(res.status, 200);
    assert_eq!(res.header("subscription-userinfo"), Some(USERINFO));
    assert!(res.text().contains("Issue362"));
}

#[tokio::test]
async fn preserves_userinfo_for_singbox() {
    let res = request(&app(), &format!("http://localhost/singbox?config={}", enc(REMOTE))).await;
    assert_eq!(res.status, 200);
    assert_eq!(res.header("subscription-userinfo"), Some(USERINFO));
    let config = json::parse(&res.text()).unwrap();
    assert!(find(config.get("outbounds"), "tag", "Issue362").is_some());
}

#[tokio::test]
async fn preserves_userinfo_for_xray() {
    let res = request(&app(), &format!("http://localhost/xray?config={}", enc(REMOTE))).await;
    assert_eq!(res.status, 200);
    assert_eq!(res.header("subscription-userinfo"), Some(USERINFO));
    assert_eq!(decode_base64(&res.text()), PROXY);
}
