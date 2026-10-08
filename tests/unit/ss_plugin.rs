//! test/ss-plugin.test.js

use crate::common::*;
use sublink::js::Value;

async fn proxy(input: &str, name: &str) -> Value {
    let built = clash(&opts(input, "minimal".into())).await;
    find(built.get("proxies"), "name", name).cloned().unwrap_or_else(|| panic!("missing proxy {name}"))
}

#[tokio::test]
async fn preserves_plugin_and_plugin_opts() {
    let input = "proxies:
  - name: 🇭🇰香港
    type: ss
    server: xx.xxxx.com
    port: 99999
    cipher: aes-128-gcm
    password: xxxxxxx-xxxxxxxx-xxxx-xxxxxxx-xxxxxxxxxxxxx
    udp: true
    plugin: obfs
    plugin-opts:
      mode: http
      host: xxxxxxxxxxxxxxxxxxxxxxxxxx.baidu.com";
    let p = proxy(input, "🇭🇰香港").await;
    assert_eq!(p.get("plugin").as_str(), Some("obfs"));
    assert_eq!(p.get("plugin-opts").get("mode").as_str(), Some("http"));
    assert_eq!(p.get("plugin-opts").get("host").as_str(), Some("xxxxxxxxxxxxxxxxxxxxxxxxxx.baidu.com"));
}

#[tokio::test]
async fn preserves_v2ray_plugin_with_websocket_mode() {
    let input = "proxies:
  - name: SS-V2Ray-WS
    type: ss
    server: example.com
    port: 443
    cipher: chacha20-ietf-poly1305
    password: test-password
    plugin: v2ray-plugin
    plugin-opts:
      mode: websocket
      tls: true
      host: example.com
      path: /v2ray";
    let p = proxy(input, "SS-V2Ray-WS").await;
    assert_eq!(p.get("plugin").as_str(), Some("v2ray-plugin"));
    let o = p.get("plugin-opts");
    assert_eq!(o.get("mode").as_str(), Some("websocket"));
    assert_json(o.get("tls"), "true");
    assert_eq!(o.get("host").as_str(), Some("example.com"));
    assert_eq!(o.get("path").as_str(), Some("/v2ray"));
}

#[tokio::test]
async fn works_without_plugin_fields() {
    let input = "proxies:
  - name: SS-NoPlugin
    type: ss
    server: example.com
    port: 8388
    cipher: aes-256-gcm
    password: test-password";
    let p = proxy(input, "SS-NoPlugin").await;
    assert!(p.get("plugin").is_undefined());
    assert!(p.get("plugin-opts").is_undefined());
}

#[tokio::test]
async fn preserves_plugin_in_inline_flow_yaml() {
    let input = "proxies:
  - { name: 🇭🇰香港, type: ss, server: xx.xxxx.com, port: 99999, cipher: aes-128-gcm, password: xxxxxxx-xxxxxxxx-xxxx-xxxxxxx-xxxxxxxxxxxxx, udp: true, plugin: obfs, plugin-opts: { mode: http, host: xxxxxxxxxxxxxxxxxxxxxxxxxx.baidu.com } }";
    let p = proxy(input, "🇭🇰香港").await;
    assert_eq!(p.get("plugin").as_str(), Some("obfs"));
    assert_eq!(p.get("plugin-opts").get("mode").as_str(), Some("http"));
    assert_eq!(p.get("plugin-opts").get("host").as_str(), Some("xxxxxxxxxxxxxxxxxxxxxxxxxx.baidu.com"));
}

#[tokio::test]
async fn parses_simple_obfs_plugin_from_query_string() {
    let url = "ss://YWVzLTEyOC1nY206dGVzdC1wYXNzd29yZC0xMjM0@test.example.com:8388/?plugin=simple-obfs%3Bobfs%3Dhttp%3Bobfs-host%3Dcdn.example.com#%F0%9F%87%AD%F0%9F%87%B0Test-Node";
    let p = proxy(url, "🇭🇰Test-Node").await;
    assert_eq!(p.get("type").as_str(), Some("ss"));
    assert_eq!(p.get("server").as_str(), Some("test.example.com"));
    assert_json(p.get("port"), "8388");
    assert_eq!(p.get("cipher").as_str(), Some("aes-128-gcm"));
    assert_eq!(p.get("plugin").as_str(), Some("obfs"));
    assert_eq!(p.get("plugin-opts").get("mode").as_str(), Some("http"));
    assert_eq!(p.get("plugin-opts").get("host").as_str(), Some("cdn.example.com"));
}

#[tokio::test]
async fn parses_v2ray_plugin_from_query_string() {
    let url = "ss://Y2hhY2hhMjAtaWV0Zi1wb2x5MTMwNTp0ZXN0LXBhc3N3b3Jk@example.com:443/?plugin=v2ray-plugin%3Bmode%3Dwebsocket%3Bhost%3Dexample.com%3Bpath%3D%2Fv2ray%3Btls#SS-V2Ray-Test";
    let p = proxy(url, "SS-V2Ray-Test").await;
    assert_eq!(p.get("plugin").as_str(), Some("v2ray-plugin"));
    let o = p.get("plugin-opts");
    assert_eq!(o.get("mode").as_str(), Some("websocket"));
    assert_eq!(o.get("host").as_str(), Some("example.com"));
    assert_eq!(o.get("path").as_str(), Some("/v2ray"));
    assert_json(o.get("tls"), "true");
}
