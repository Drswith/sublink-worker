//! Byte-for-byte parity with the original Node implementation.
//!
//! `fixtures/golden.json.gz` holds request sequences and the responses the
//! JavaScript app produced for them (see `fixtures/golden-gen.mjs`).

mod common;

use std::io::Read;

use common::{MockFetcher, test_app};
use sublink::hono::Request;
use sublink::js::{Value, json};

fn load() -> Value {
    let mut raw = String::new();
    flate2::read::GzDecoder::new(&include_bytes!("fixtures/golden.json.gz")[..]).read_to_string(&mut raw).unwrap();
    json::parse(&raw).unwrap()
}

fn str_of(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

fn first_difference(got: &str, want: &str) -> String {
    let (g, w): (Vec<&str>, Vec<&str>) = (got.lines().collect(), want.lines().collect());
    for i in 0..g.len().max(w.len()) {
        if g.get(i) != w.get(i) {
            return format!("line {}:\n      got:  {:?}\n      want: {:?}", i + 1, g.get(i), w.get(i));
        }
    }
    "trailing newline / whitespace difference".into()
}

#[tokio::test]
async fn responses_match_node_implementation() {
    let golden = load();
    let fetcher = MockFetcher::new();
    for mock in golden.get("mocks").as_array().unwrap() {
        let headers: Vec<(String, String)> =
            mock.get("headers").own_entries().into_iter().map(|(k, v)| (k, v.to_js_string())).collect();
        let refs: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        fetcher.route(
            str_of(mock.get("url")),
            mock.get("status").as_number().unwrap() as u16,
            &refs,
            str_of(mock.get("body")),
        );
    }

    let filter = std::env::var("GOLDEN_FILTER").unwrap_or_default();
    let fill = |s: &str, vars: &[String]| {
        let mut out = s.to_string();
        for (i, v) in vars.iter().enumerate() {
            out = out.replace(&format!("{{{{{i}}}}}"), v);
        }
        out
    };
    let mut failures = Vec::new();
    let mut checked = 0;
    for case in golden.get("cases").as_array().unwrap() {
        let name = str_of(case.get("name"));
        if !name.contains(&filter) {
            continue;
        }
        checked += 1;
        let app = test_app(fetcher.clone());
        let mut vars: Vec<String> = Vec::new();
        for (index, step) in case.get("steps").as_array().unwrap().iter().enumerate() {
            let mut req = Request::new(str_of(step.get("method")), &fill(str_of(step.get("url")), &vars)).unwrap();
            for (k, v) in step.get("headers").own_entries() {
                req = req.with_header(&k, &v.to_js_string());
            }
            if let Some(body) = step.get("body").as_str() {
                req = req.with_body(body.as_bytes().to_vec());
            }
            let res = app.handle(&req).await;
            let expect = step.get("expect");
            let label = format!("{name} [step {index}] {} {}", str_of(step.get("method")), str_of(step.get("url")));
            let mut problems = Vec::new();

            let want_status = expect.get("status").as_number().unwrap() as u16;
            if res.status != want_status {
                problems.push(format!("status {} != {}", res.status, want_status));
            }
            for header in ["content-type", "subscription-userinfo", "location", "cache-control"] {
                let want = expect.get("headers").get(header).as_str().map(|w| fill(w, &vars));
                let got = res.header(header);
                if got != want.as_deref() {
                    problems.push(format!("header {header}: {got:?} != {want:?}"));
                }
            }
            if expect.get("binary").truthy() {
                let want: Vec<u8> = sublink::js::base64::base64_to_binary(str_of(expect.get("body")))
                    .chars()
                    .map(|c| c as u8)
                    .collect();
                if res.body != want {
                    problems.push(format!("binary body differs ({} vs {} bytes)", res.body.len(), want.len()));
                }
            } else if step.get("capture").truthy() {
                let body = res.text();
                let shape_ok = !body.is_empty() && body.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
                if !shape_ok {
                    problems.push(format!("captured body has unexpected shape: {body:?}"));
                }
                vars.push(body);
            } else {
                let (body, want) = (res.text(), fill(str_of(expect.get("body")), &vars));
                if body != want {
                    problems.push(format!("body differs at {}", first_difference(&body, &want)));
                }
            }
            if !problems.is_empty() {
                failures.push(format!("--- {label}\n    {}", problems.join("\n    ")));
            }
        }
    }
    assert!(checked > 0, "no golden cases matched GOLDEN_FILTER={filter:?}");
    assert!(failures.is_empty(), "{} of {} cases differ:\n{}", failures.len(), checked, failures.join("\n"));
}
