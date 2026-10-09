//! test/node-runtime.test.js, now covering the binary's environment settings.

use sublink::settings::{DEFAULT_CONFIG_TTL_SECONDS, DEFAULT_PORT, Settings};

fn settings(vars: &[(&str, &str)]) -> Settings {
    Settings::from_vars(|name| vars.iter().find(|(k, _)| *k == name).map(|(_, v)| v.to_string())).unwrap()
}

#[test]
fn preserves_zero_as_the_no_expiration_config_ttl() {
    assert_eq!(settings(&[("CONFIG_TTL_SECONDS", "0")]).config_ttl_seconds, Some(0.0));
}

#[test]
fn defaults_match_the_node_runtime() {
    let s = settings(&[]);
    assert_eq!(s.port, DEFAULT_PORT);
    assert_eq!(s.config_ttl_seconds, Some(DEFAULT_CONFIG_TTL_SECONDS));
    assert_eq!(s.short_link_ttl_seconds, None);
}

#[test]
fn parses_numbers_like_javascript() {
    let s = settings(&[("CONFIG_TTL_SECONDS", " 0x10 "), ("SHORT_LINK_TTL_SECONDS", "0"), ("PORT", "8080")]);
    assert_eq!(s.config_ttl_seconds, Some(16.0));
    assert_eq!(s.short_link_ttl_seconds, None);
    assert_eq!(s.port, 8080);
    assert_eq!(settings(&[("CONFIG_TTL_SECONDS", "abc")]).config_ttl_seconds, Some(DEFAULT_CONFIG_TTL_SECONDS));
    assert_eq!(settings(&[("SHORT_LINK_TTL_SECONDS", "3600")]).short_link_ttl_seconds, Some(3600.0));
}

#[test]
fn rejects_invalid_port() {
    assert!(Settings::from_vars(|name| (name == "PORT").then(|| "abc".to_string())).is_err());
}
