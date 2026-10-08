//! js-yaml load/dump behavior checked against output captured from js-yaml 4.3.0.

use sublink::js::number::js_number_to_string;
use sublink::js::{Object, Value, json};
use sublink::yaml;

fn describe(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Undefined => "undefined".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => {
            if *n == 0.0 && n.is_sign_negative() {
                "n:-0".into()
            } else {
                format!("n:{}", js_number_to_string(*n))
            }
        }
        Value::String(s) => json::stringify(&Value::String(s.clone())).unwrap(),
        Value::Date(t) => format!("d:{}", sublink::js::date::to_iso_string(*t).unwrap_or_else(|| "Invalid".into())),
        Value::Array(items) => format!("[{}]", items.iter().map(describe).collect::<Vec<_>>().join(",")),
        Value::Object(o) => format!(
            "{{{}}}",
            o.entries()
                .iter()
                .map(|(k, v)| format!("{}:{}", json::stringify(&Value::String((*k).clone())).unwrap(), describe(v)))
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

fn decode_input(v: &Value) -> Value {
    match v {
        Value::Array(items) => Value::Array(items.iter().map(decode_input).collect()),
        Value::Object(o) => {
            if o.contains_key("$nan") {
                return Value::Number(f64::NAN);
            }
            if o.contains_key("$inf") {
                return Value::Number(f64::INFINITY);
            }
            if o.contains_key("$ninf") {
                return Value::Number(f64::NEG_INFINITY);
            }
            if o.contains_key("$negzero") {
                return Value::Number(-0.0);
            }
            if o.contains_key("$undef") {
                return Value::Undefined;
            }
            if let Some(Value::String(iso)) = o.get("$date") {
                let t = match yaml::load(iso).unwrap() {
                    Value::Date(t) => t,
                    other => panic!("bad date {other:?}"),
                };
                return Value::Date(t);
            }
            Value::Object(o.entries().into_iter().map(|(k, v)| (k.clone(), decode_input(v))).collect::<Object>())
        }
        other => other.clone(),
    }
}

#[test]
fn yaml_matches_js_yaml() {
    let cases = json::parse(include_str!("fixtures/yaml_cases.json")).unwrap();
    let mut failures = Vec::new();
    for case in cases.as_array().unwrap() {
        let kind = case.get("kind").as_str().unwrap();
        let expected = match case.get("ok").as_str() {
            Some(ok) => format!("ok:{ok}"),
            None => format!("err:{}", case.get("err").as_str().unwrap()),
        };
        let actual = match kind {
            "load" => {
                let input = case.get("input").as_str().unwrap();
                match yaml::load(input) {
                    Ok(v) => format!("ok:{}", describe(&v)),
                    Err(e) => format!("err:{}", e.message),
                }
            }
            "roundtrip" => match yaml::load(case.get("input").as_str().unwrap()).and_then(|v| {
                yaml::dump(&v).map_err(|e| yaml::YamlError { reason: e.message.clone(), message: e.message })
            }) {
                Ok(s) => format!("ok:{s}"),
                Err(e) => format!("err:{}", e.message),
            },
            "dump" => match yaml::dump(&decode_input(case.get("input"))) {
                Ok(s) => format!("ok:{s}"),
                Err(e) => format!("err:{}", e.message),
            },
            _ => unreachable!(),
        };
        if actual != expected {
            failures.push(format!(
                "--- {kind} {}\n  got:  {actual:?}\n  want: {expected:?}",
                json::stringify(case.get("input")).unwrap().chars().take(120).collect::<String>()
            ));
        }
    }
    assert!(failures.is_empty(), "{} mismatches:\n{}", failures.len(), failures.join("\n"));
}
