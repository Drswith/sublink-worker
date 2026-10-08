//! test/clash-builder.test.js

use crate::common::*;
use sublink::builders::helpers::sanitize_clash_proxy_groups;

#[tokio::test]
async fn cleans_up_proxy_groups_and_removes_non_existent_proxies() {
    let input = "
proxies:
  - name: Valid-SS
    type: ss
    server: example.com
    port: 443
    cipher: aes-128-gcm
    password: test
proxy-groups:
  - name: 自定义选择
    type: select
    proxies:
      - DIRECT
      - REJECT
      - Valid-SS
      - NotExist
    ";
    let built = clash(&opts(input, "minimal".into())).await;
    let grp = group(&built, "自定义选择");
    assert_eq!(strs(grp.get("proxies")), ["DIRECT", "REJECT", "Valid-SS"]);
}

#[tokio::test]
async fn references_user_defined_proxy_providers_in_generated_groups() {
    let input = "
proxy-providers:
  my-provider:
    type: http
    url: https://example.com/sub
    path: ./my.yaml
    interval: 3600

proxies:
  - name: local
    type: ss
    server: 127.0.0.1
    port: 1080
    cipher: aes-256-gcm
    password: test
";
    let built = clash(&opts(input, "minimal".into())).await;
    assert!(has(group(&built, "🚀 节点选择").get("use"), "my-provider"));
}

#[test]
fn sanitize_keeps_provider_node_references_when_group_uses_providers() {
    let mut config = v(r#"{
        "proxies": [],
        "proxy-groups": [{ "name": "Custom Group", "type": "select", "use": ["my-provider"], "proxies": ["node-from-provider"] }]
    }"#);
    sanitize_clash_proxy_groups(&mut config).unwrap();
    let grp = &config.get("proxy-groups").as_array().unwrap()[0];
    assert!(has(grp.get("proxies"), "node-from-provider"));
}

#[tokio::test]
async fn defaults_private_and_location_cn_groups_to_direct() {
    let input = "
ss://YWVzLTEyOC1nY206dGVzdA@example.com:443#HK-Node-1
ss://YWVzLTEyOC1nY206dGVzdA@example.com:444#US-Node-1
    ";
    let built = clash(&opts(input, "minimal".into())).await;
    let private = group(&built, &t("outboundNames.Private"));
    let cn = group(&built, &t("outboundNames.Location:CN"));
    assert_eq!(strs(private.get("proxies"))[0], "DIRECT");
    assert_eq!(strs(cn.get("proxies"))[0], "DIRECT");
    let fallback = group(&built, &t("outboundNames.Fall Back"));
    assert_ne!(strs(fallback.get("proxies"))[0], "DIRECT");
}
