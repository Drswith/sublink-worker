//! test/surge-config-parser.test.js

use crate::common::*;
use sublink::parsers::surge::{convert_surge_ini_to_json, parse_surge_config_input};

#[test]
fn returns_json_object_untouched_when_valid_json_is_provided() {
    let (config, converted) = parse_surge_config_input(
        r#"{"general":{"allow-wifi-access":false,"wifi-access-http-port":6152},"replica":{"hide-udp":true}}"#,
    )
    .unwrap();
    assert!(!converted);
    assert_json(config.get("general").get("allow-wifi-access"), "false");
    assert_json(config.get("replica").get("hide-udp"), "true");
}

#[test]
fn converts_basic_surge_ini_content_into_json_structure() {
    let ini = "
[General]
allow-wifi-access = false
wifi-access-http-port = 6152
skip-proxy = 127.0.0.1,localhost

[Replica]
hide-udp = true

[Proxy]
DIRECT = direct

[Proxy Group]
Auto = select, DIRECT

[Rule]
DOMAIN-SUFFIX,google.com,Auto
";
    let (config, converted) = parse_surge_config_input(ini).unwrap();
    assert!(converted);
    assert_json(config.get("general").get("allow-wifi-access"), "false");
    assert_json(config.get("general").get("skip-proxy"), r#""127.0.0.1,localhost""#);
    assert_json(config.get("replica").get("hide-udp"), "true");
    assert_json(config.get("proxies"), r#"["DIRECT = direct"]"#);
    assert_json(config.get("proxy-groups"), r#"["Auto = select, DIRECT"]"#);
    assert_json(config.get("rules"), r#"["DOMAIN-SUFFIX,google.com,Auto"]"#);
}

#[test]
#[allow(clippy::approx_constant)]
fn normalizes_primitive_values_within_ini_sections() {
    let converted = convert_surge_ini_to_json(
        "
[General]
enabled = true
timeout = 5
ratio = 3.14
quoted = \"Text Value\"
",
    )
    .unwrap();
    let general = converted.get("general");
    assert_json(general.get("enabled"), "true");
    assert_json(general.get("timeout"), "5");
    assert!((general.get("ratio").as_number().unwrap() - 3.14).abs() < 0.005);
    assert_json(general.get("quoted"), r#""Text Value""#);
}

#[test]
fn throws_when_content_is_neither_json_nor_ini() {
    assert!(parse_surge_config_input("invalid content without sections").is_err());
}
