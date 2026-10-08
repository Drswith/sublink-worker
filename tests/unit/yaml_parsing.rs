//! test/yaml-parsing.test.js — drives `parseCustomItems()` on a bare base builder.

use crate::common::*;
use sublink::builders::{ConfigBuilder, Core, parse_custom_items};
use sublink::js::{JsResult, Value, json};

/// `new BaseConfigBuilder(input, {}, 'zh-CN', 'test-agent')`: only parsing is exercised.
struct BaseBuilder {
    core: Core,
}

impl ConfigBuilder for BaseBuilder {
    fn core(&self) -> &Core {
        &self.core
    }
    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }
    fn get_proxies(&self) -> JsResult<Vec<Value>> {
        unreachable!()
    }
    fn get_proxy_name(&self, _: &Value) -> JsResult<Value> {
        unreachable!()
    }
    fn convert_proxy(&self, _: &Value) -> JsResult<Value> {
        unreachable!()
    }
    fn add_custom_items(&mut self, _: Vec<Value>) -> JsResult<()> {
        unreachable!()
    }
    fn add_auto_select_group(&mut self, _: &[Value]) -> JsResult<()> {
        unreachable!()
    }
    fn add_node_select_group(&mut self, _: &[Value]) -> JsResult<()> {
        unreachable!()
    }
    fn add_country_groups(&mut self) -> JsResult<()> {
        unreachable!()
    }
    fn add_outbound_groups(&mut self, _: &[String], _: &[Value]) -> JsResult<()> {
        unreachable!()
    }
    fn add_custom_rule_groups(&mut self, _: &[Value]) -> JsResult<()> {
        unreachable!()
    }
    fn add_fall_back_group(&mut self, _: &[Value]) -> JsResult<()> {
        unreachable!()
    }
}

async fn parse(input: &str) -> (Vec<Value>, BaseBuilder) {
    let mut b = BaseBuilder { core: Core::new(&opts(input, v("[]")), &v("{}")) };
    let items = parse_custom_items(&mut b, &MockFetcher::default()).await.unwrap();
    (items, b)
}

fn tags(items: &[Value]) -> Vec<String> {
    items.iter().filter_map(|i| i.get("tag").as_str().map(str::to_string)).collect()
}

fn type_count(items: &[Value], ty: &str) -> usize {
    items.iter().filter(|i| i.get("type").as_str() == Some(ty)).count()
}

fn by_path<'a>(item: &'a Value, path: &str) -> &'a Value {
    path.split('.').fold(item, |acc, key| acc.get(key))
}

#[tokio::test]
async fn original_yaml_config() {
    let input = "proxies:
  - name: HY2-main
    type: hysteria2
    server: hajimi.com
    port: 443
    ports: 20000-20100
    hop-interval: 15
    up: \"200 Mbps\"
    down: \"200 Mbps\"
    password: REPLACE_HY2_PASS
    sni: hajimi.com
    obfs: salamander
    obfs-password: REPLACE_OBFS_PASS
    alpn:
      - h3
    skip-cert-verify: false
    fast-open: true

  - name: TUIC-main
    type: tuic
    server: hajimi.com
    port: 444
    uuid: REPLACE_TUIC_UUID
    password: REPLACE_TUIC_PASS
    sni: hajimi.com
    alpn:
      - h3
    reduce-rtt: true
    congestion-controller: bbr
    udp-relay-mode: native
    zero-rtt: false
    fast-open: true
    skip-cert-verify: false

  - name: VLESS-REALITY
    type: vless
    server: hajimi.com
    port: 445
    uuid: REPLACE_VLESS_UUID
    udp: true
    flow: xtls-rprx-vision
    tls: true
    servername: www.apple.com
    alpn:
      - h2
      - http/1.1
    client-fingerprint: chrome
    reality-opts:
      public-key: REPLACE_PUBLIC_KEY
      short-id: a1b2c3
    packet-encoding: xudp
    skip-cert-verify: false";
    let (items, _) = parse(input).await;
    assert_eq!(items.len(), 3);
    assert_eq!(tags(&items), ["HY2-main", "TUIC-main", "VLESS-REALITY"]);
    for ty in ["hysteria2", "tuic", "vless"] {
        assert_eq!(type_count(&items, ty), 1, "{ty}");
    }
}

#[tokio::test]
async fn empty_proxy_list() {
    assert!(parse("proxies: []").await.0.is_empty());
}

#[tokio::test]
async fn mixed_types_and_invalid_nodes() {
    let input = "proxies:
  - name: Valid-SS
    type: ss
    server: example.com
    port: 443
    cipher: aes-128-gcm
    password: test

  - name: Invalid-Type
    type: unknown
    server: invalid.com
    port: 80";
    let (items, _) = parse(input).await;
    assert_eq!(items.len(), 1);
    assert_eq!(tags(&items), ["Valid-SS"]);
    assert_eq!(type_count(&items, "shadowsocks"), 1);
}

#[tokio::test]
async fn base64_encoded_yaml() {
    let input = "cHJveGllczoKICAtIG5hbWU6IEJhc2U2NC1ZQU1MLVRlc3QKICAgIHR5cGU6IHR1aWMKICAgIHNlcnZlcjogdGVzdC5jb20KICAgIHBvcnQ6IDQ1NQogICAgdXVpZDogNzJlMjQ1YzUtYzY4MS00Y2JjLTljY2QtN2IxMGJiOGYyYzUzCiAgICBwYXNzd29yZDogdGVzdC1wYXNzCiAgICBzbmk6IHRlc3QuY29tCiAgICBhbHBuOgogICAgICAtIGgzCiAgICB4dWRwOiB0cnVlCiAgICB6aXA6IHh1ZHAKICAgIHNraXAtY2VydC12ZXJpZnk6IGZhbHNlCg==";
    let (items, _) = parse(input).await;
    assert_eq!(items.len(), 1);
    assert_eq!(tags(&items), ["Base64-YAML-Test"]);
    assert_eq!(type_count(&items, "tuic"), 1);
}

#[tokio::test]
async fn anytls_basic_parsing() {
    let input = "proxies:
  - name: ANYTLS-main
    type: anytls
    server: example.com
    port: 443
    password: REPLACE_ANYTLS_PASS
    udp: true
    sni: example.com
    alpn:
      - h2
      - http/1.1
    client-fingerprint: chrome
    skip-cert-verify: false
    idle-session-check-interval: 30
    idle-session-timeout: 120
    min-idle-session: 5";
    let (items, _) = parse(input).await;
    assert_eq!(items.len(), 1);
    assert_eq!(type_count(&items, "anytls"), 1);
    let item = &items[0];
    assert_eq!(item.get("tag").as_str(), Some("ANYTLS-main"));
    for (path, expected) in [
        ("type", r#""anytls""#),
        ("udp", "true"),
        ("tls.server_name", r#""example.com""#),
        ("tls.insecure", "false"),
        ("tls.alpn", r#"["h2","http/1.1"]"#),
        ("tls.utls.fingerprint", r#""chrome""#),
        ("idle-session-check-interval", "30"),
        ("idle-session-timeout", "120"),
        ("min-idle-session", "5"),
    ] {
        assert_eq!(json::stringify(by_path(item, path)).as_deref(), Some(expected), "{path}");
    }
}

#[tokio::test]
async fn collects_proxy_groups_for_later_merge() {
    let input = "proxies:
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
      - NotExist";
    let (items, b) = parse(input).await;
    assert_eq!(items.len(), 1);
    assert_eq!(type_count(&items, "shadowsocks"), 1);
    let pending = Value::array(b.core.pending_user_proxy_groups.clone().expect("pending groups"));
    assert_eq!(
        json::stringify(&pending).as_deref(),
        Some(r#"[{"name":"自定义选择","type":"select","proxies":["DIRECT","REJECT","Valid-SS","NotExist"]}]"#)
    );
}
