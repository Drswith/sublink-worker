//! test/issue-428-hy2-port-hopping.test.js

use crate::common::*;
use sublink::builders::{clash, singbox};
use sublink::parsers::clash_proxy::convert_yaml_proxy_to_object;
use sublink::parsers::protocols::parse_hysteria2;

#[test]
fn parses_mport_alias_from_share_links() {
    let proxy =
        parse_hysteria2("hysteria2://pass@example.com:443?mport=20000-30000&hop_interval=30&sni=example.com#hop")
            .unwrap();
    assert_json(proxy.get("ports"), r#""20000-30000""#);
    assert_json(proxy.get("hop_interval"), "30");
}

#[test]
fn still_parses_the_canonical_ports_param() {
    let proxy = parse_hysteria2("hysteria2://pass@example.com:443?ports=20000-30000&hop-interval=15#hop").unwrap();
    assert_json(proxy.get("ports"), r#""20000-30000""#);
    assert_json(proxy.get("hop_interval"), "15");
}

#[test]
fn parses_mport_alias_from_clash_yaml_proxies() {
    let proxy = convert_yaml_proxy_to_object(&v(
        r#"{"name":"hop","type":"hysteria2","server":"example.com","port":443,"password":"pass","mport":"20000-30000","hop-interval":30}"#,
    ));
    assert_json(proxy.get("ports"), r#""20000-30000""#);
    assert_json(proxy.get("hop_interval"), "30");
}

#[test]
fn emits_singbox_hysteria2_fields_that_singbox_accepts() {
    let converted = singbox::convert_proxy(&v(r#"{
        "tag":"hop","type":"hysteria2","server":"example.com","server_port":443,"password":"pass",
        "ports":"20000-30000","hop_interval":30,"up":100,"down":200,"auth":"x","recv_window_conn":1000,
        "fast_open":true,"tls":{"enabled":true,"server_name":"example.com"}}"#))
    .unwrap();
    assert_json(converted.get("server_ports"), r#"["20000:30000"]"#);
    assert_json(converted.get("hop_interval"), r#""30s""#);
    assert_json(converted.get("up_mbps"), "100");
    assert_json(converted.get("down_mbps"), "200");
    for key in ["ports", "up", "down", "auth", "recv_window_conn", "fast_open"] {
        assert!(!has_prop(&converted, key), "{key} must be dropped");
    }
}

#[test]
fn converts_comma_separated_port_ranges_for_singbox() {
    let converted = singbox::convert_proxy(&v(
        r#"{"tag":"hop","type":"hysteria2","server":"example.com","server_port":443,"ports":"20000-30000, 40000-50000"}"#,
    ))
    .unwrap();
    assert_json(converted.get("server_ports"), r#"["20000:30000","40000:50000"]"#);
}

#[test]
fn keeps_clash_hysteria2_port_hopping_fields_as_is() {
    let converted = clash::convert_proxy(&v(r#"{
        "tag":"hop","type":"hysteria2","server":"example.com","server_port":443,"password":"pass",
        "ports":"20000-30000","hop_interval":30,"tls":{"enabled":true,"server_name":"example.com"}}"#))
    .unwrap();
    assert_json(converted.get("ports"), r#""20000-30000""#);
    assert_json(converted.get("hop-interval"), "30");
}
