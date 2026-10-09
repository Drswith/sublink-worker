//! test/upstream-fetch-failure.test.js

use std::time::Duration;

use crate::common::*;
use sublink::app::App;
use sublink::fetch::{BoxFuture, FetchResponse, Fetcher};
use sublink::storage::Store;

const SOURCE: &str = "https://example.com/sub/private-token";

fn url(endpoint: &str) -> String {
    format!("http://localhost/{endpoint}?config={}", enc(SOURCE))
}

#[tokio::test]
async fn returns_502_for_a_failed_fetch_instead_of_an_empty_config() {
    for endpoint in ["clash", "singbox", "surge"] {
        let res = request(&test_app(MockFetcher::new()), &url(endpoint)).await;
        assert_eq!(res.status, 502, "{endpoint}");
        let body = res.text();
        assert!(body.contains("upstream subscription"), "{endpoint}: {body}");
        assert!(!body.contains("private-token"), "{endpoint}: {body}");
    }
}

#[tokio::test]
async fn does_not_treat_an_upstream_http_error_as_a_subscription() {
    let fetcher = MockFetcher::new();
    fetcher.route(SOURCE, 403, &[], "");
    assert_eq!(request(&test_app(fetcher), &url("clash")).await.status, 502);
}

#[tokio::test]
async fn imports_all_nodes_from_a_base64_subscription_and_keeps_the_dns_switch() {
    let nodes: Vec<String> = (30000..30004)
        .map(|port| {
            format!(
                "vless://00000000-0000-4000-8000-000000000001@example.com:{port}?security=none&type=tcp#Node-{port}"
            )
        })
        .collect();
    let fetcher = MockFetcher::new();
    fetcher.ok(SOURCE, &sublink::js::base64::encode_base64(&nodes.join("\n")));
    let res = request(&test_app(fetcher), &format!("{}&include_clash_dns=false", url("clash"))).await;
    assert_eq!(res.status, 200);
    let config = sublink::yaml::load(&res.text()).unwrap();
    let proxies = config.get("proxies").as_array().unwrap();
    assert_eq!(proxies.len(), 4);
    assert!(!has_prop(&config, "dns"));
    let groups = config.get("proxy-groups").as_array().unwrap();
    let group = groups.iter().find(|g| g.get("name").as_str() == Some("🚀 节点选择")).unwrap();
    let members = group.get("proxies").as_array().unwrap();
    for node in proxies {
        assert!(members.iter().any(|m| m.as_str() == node.get("name").as_str()));
    }
}

/// An upstream that never answers.
struct Stalled;

impl Fetcher for Stalled {
    fn get<'a>(&'a self, _: &'a str, _: Option<&'a str>) -> BoxFuture<'a, Result<FetchResponse, String>> {
        Box::pin(async {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            Ok(FetchResponse { status: 200, ..Default::default() })
        })
    }
}

#[tokio::test(start_paused = true)]
async fn gives_up_on_a_stalled_upstream_after_15_seconds() {
    let app = App::new(Store::in_memory(), std::sync::Arc::new(Stalled));
    let started = tokio::time::Instant::now();
    let res = request(&app, &url("clash")).await;
    assert_eq!(res.status, 502);
    assert_eq!(started.elapsed(), Duration::from_secs(15));
}
