//! Built-in JS behaviors checked against values captured from Node.js.

use sublink::js::base64::{base64_to_binary, decode_base64, encode_base64};
use sublink::js::number::{js_number_to_string, parse_float, parse_int, string_to_number};
use sublink::js::string::{decode_uri, decode_uri_component, encode_uri, encode_uri_component, js_trim};
use sublink::js::{Value, json};
use sublink::utils::Params;

fn show(n: f64) -> String {
    if n == 0.0 && n.is_sign_negative() { "-0".into() } else { js_number_to_string(n) }
}

fn field<'a>(case: &'a Value, key: &str) -> &'a str {
    case.get(key).as_str().unwrap_or_else(|| panic!("missing {key}"))
}

fn uri_result(r: Result<String, sublink::js::JsError>) -> String {
    match r {
        Ok(s) => s,
        Err(e) => format!("ERR:URIError:{}", e.message),
    }
}

#[test]
fn js_core_matches_node() {
    let fixture = json::parse(include_str!("fixtures/js_core.json")).unwrap();
    let mut failures = Vec::new();
    for case in fixture.as_array().unwrap() {
        let input = field(case, "input");
        let mut check = |what: &str, actual: String, expected: &str| {
            if actual != expected {
                failures.push(format!("{what}({input:?}): got {actual:?}, want {expected:?}"));
            }
        };
        match field(case, "fn") {
            "num" => {
                check("Number", show(string_to_number(input)), field(case, "n"));
                check("parseFloat", show(parse_float(input)), field(case, "pf"));
                check("parseInt", show(parse_int(input, 0)), field(case, "pi"));
            }
            "uri" => {
                check("decodeURIComponent", uri_result(decode_uri_component(input)), field(case, "component"));
                check("decodeURI", uri_result(decode_uri(input)), field(case, "uri"));
            }
            "enc" => {
                check("encodeURIComponent", encode_uri_component(input), field(case, "component"));
                check("encodeURI", encode_uri(input), field(case, "uri"));
            }
            "b64" => {
                check("base64ToBinary", base64_to_binary(input), field(case, "binary"));
                check("decodeBase64", decode_base64(input), field(case, "decoded"));
            }
            "b64enc" => check("encodeBase64", encode_base64(input), field(case, "output")),
            "json" => {
                let actual = match json::parse(input) {
                    Ok(v) => format!("ok:{}", json::stringify(&v).unwrap()),
                    Err(e) => format!("err:SyntaxError:{}", e.message),
                };
                let expected = match case.get("ok").as_str() {
                    Some(ok) => format!("ok:{ok}"),
                    None => format!("err:{}", field(case, "err")),
                };
                check("JSON.parse", actual, &expected);
            }
            "trim" => check("trim", js_trim(input).to_string(), field(case, "output")),
            "qs" => {
                let entries: Vec<Value> = Params::parse(input)
                    .entries()
                    .iter()
                    .map(|(k, v)| Value::array(vec![Value::str(k), Value::str(v)]))
                    .collect();
                check("URLSearchParams", json::stringify(&Value::array(entries)).unwrap(), field(case, "entries"));
            }
            other => panic!("unknown fn {other}"),
        }
    }
    assert!(failures.is_empty(), "{} mismatches:\n{}", failures.len(), failures.join("\n"));
}
