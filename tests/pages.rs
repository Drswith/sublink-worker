//! The home page matches the original JSX rendering byte for byte, apart from
//! the inlined client script, which the original served as esbuild's reprint
//! of the same function.

mod common;

use std::io::Read;

use common::{MockFetcher, test_app};
use sublink::hono::Request;
use sublink::js::json;

fn mask(html: &str, year: i64) -> String {
    let start = html.find("\n    ((t) => {").expect("form script") + 6;
    let end = html[start..].find(")();\n  </script>").expect("form script end") + start;
    let masked = format!("{}{{{{form_logic}}}}{}", &html[..start], &html[end..]);
    masked.replace(&format!("© {year} "), "© {{year}} ")
}

fn fixture() -> sublink::js::Value {
    let mut raw = String::new();
    flate2::read::GzDecoder::new(&include_bytes!("fixtures/pages.json.gz")[..]).read_to_string(&mut raw).unwrap();
    json::parse(&raw).unwrap()
}

#[tokio::test]
async fn home_page_matches_node_rendering() {
    let fixture = fixture();
    let cases = fixture.get("pages");
    let app = test_app(MockFetcher::new());
    let year = sublink::js::date::current_year();
    for case in cases.as_array().unwrap() {
        let query = case.get("query").as_str().unwrap();
        let mut req = Request::get(&format!("http://localhost/{query}"));
        for (k, v) in case.get("headers").own_entries() {
            req = req.with_header(&k, v.as_str().unwrap());
        }
        let res = app.handle(&req).await;
        assert_eq!(res.status, 200);
        assert_eq!(res.header("content-type"), case.get("contentType").as_str());
        // The fixture was rendered at v2.4.2; later releases only change the version text.
        let got = mask(&res.text(), year).replace(sublink::i18n::APP_VERSION, "{{version}}");
        let want = case.get("html").as_str().unwrap().replace("2.4.2", "{{version}}");
        let want = want.as_str();
        if got != want {
            let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
            std::fs::write(dir.join("page-got.html"), &got).unwrap();
            std::fs::write(dir.join("page-want.html"), want).unwrap();
            let at = got.chars().zip(want.chars()).position(|(a, b)| a != b).unwrap_or(0);
            let around = |s: &str| s.chars().skip(at.saturating_sub(120)).take(240).collect::<String>();
            panic!("page for {query:?} differs at char {at}:\n got: {:?}\nwant: {:?}", around(&got), around(want));
        }
    }
}

#[test]
fn client_script_matches_node_source() {
    // The page comparison masks this script, so dev changes to it would otherwise go unnoticed.
    let fixture = fixture();
    let source = fixture.get("formLogic").as_str().unwrap();
    let want = source.strip_prefix("export const formLogicFn = ").expect("formLogicFn export").trim_end();
    let want = want.strip_suffix(';').unwrap_or(want);
    assert!(
        include_str!("../assets/web/form-logic.js").trim_end() == want,
        "assets/web/form-logic.js differs from dev"
    );
}

#[test]
fn client_script_defines_form_data() {
    // Same guarantees the original formLogic test-suite checked.
    let script = include_str!("../assets/web/form-logic.js");
    assert!(script.contains("window.formData = function"));
    for name in ["parseSurgeConfigInput", "parseSurgeValue", "convertSurgeIniToJson"] {
        assert!(script.contains(&format!("const {name} =")), "{name} must be an inline arrow function");
        assert!(!script.contains(&format!("function {name}")), "{name} must not be a function declaration");
    }
    for member in ["submitForm", "toggleAccordion", "showAdvanced: false"] {
        assert!(script.contains(member), "formData() must define {member}");
    }
}
