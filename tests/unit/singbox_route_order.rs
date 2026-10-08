//! test/singbox-route-order.test.js

use crate::common::*;
use sublink::builders::BuildOptions;

const VLESS: &str =
    "vless://12345678-1234-1234-1234-123456789abc@example.com:443?security=tls&sni=example.com#TestVless";

fn options() -> BuildOptions {
    BuildOptions { user_agent: String::new(), ..opts(VLESS, v("[]")) }
}

#[tokio::test]
async fn hijack_dns_sits_before_any_clash_mode_rule() {
    let config = singbox(&options()).await;
    let rules = config.get("route").get("rules");
    let dns = find_index(rules, |r| {
        r.get("action").as_str() == Some("hijack-dns") && r.get("protocol").as_str() == Some("dns")
    });
    let clash_mode = find_index(rules, |r| r.get("clash_mode").truthy());
    assert!(dns >= 0);
    assert!(clash_mode >= 0);
    assert!(dns < clash_mode);
}

#[tokio::test]
async fn sniff_action_is_present_and_precedes_hijack_dns() {
    let config = singbox(&options()).await;
    let rules = config.get("route").get("rules");
    let sniff = find_index(rules, |r| r.get("action").as_str() == Some("sniff") && !r.get("protocol").truthy());
    let dns = find_index(rules, |r| r.get("action").as_str() == Some("hijack-dns"));
    assert!(sniff >= 0);
    assert!(sniff < dns);
}
