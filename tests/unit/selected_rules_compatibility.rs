//! test/selectedRules-compatibility.test.js

use crate::common::*;
use sublink::app::parse_selected_rules;
use sublink::config::predefined_rule_set;

fn preset(name: &str) -> sublink::js::Value {
    predefined_rule_set(name).unwrap()
}

#[test]
fn accepts_minimal_preset_name() {
    let result = parse_selected_rules(Some("minimal"));
    assert!(deep_eq(&result, &preset("minimal")));
    for rule in ["Location:CN", "Private", "Non-China"] {
        assert!(has(&result, rule));
    }
}

#[test]
fn accepts_balanced_preset_name() {
    let result = parse_selected_rules(Some("balanced"));
    assert!(deep_eq(&result, &preset("balanced")));
    assert!(result.length().unwrap() > preset("minimal").length().unwrap());
}

#[test]
fn accepts_comprehensive_preset_name() {
    let result = parse_selected_rules(Some("comprehensive"));
    assert!(deep_eq(&result, &preset("comprehensive")));
    assert!(result.length().unwrap() >= preset("balanced").length().unwrap());
}

#[test]
fn parses_valid_json_array() {
    assert_json(&parse_selected_rules(Some(r#"["Google","Youtube","Github"]"#)), r#"["Google","Youtube","Github"]"#);
}

#[test]
fn returns_empty_array_for_empty_or_missing_input() {
    assert_json(&parse_selected_rules(Some("")), "[]");
    assert_json(&parse_selected_rules(None), "[]");
}

#[test]
fn falls_back_to_minimal_for_invalid_json_or_unknown_preset() {
    assert!(deep_eq(&parse_selected_rules(Some("invalid-json-{[")), &preset("minimal")));
    assert!(deep_eq(&parse_selected_rules(Some("unknown-preset")), &preset("minimal")));
}

#[test]
fn returns_empty_array_if_json_is_not_an_array() {
    assert_json(&parse_selected_rules(Some(r#"{"rule":"value"}"#)), "[]");
}
