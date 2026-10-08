//! test/proxy-helpers.test.js

use crate::common::*;
use sublink::builders::helpers::add_proxy_with_dedup;

#[test]
fn renames_exact_duplicates_without_treating_substrings_as_duplicates() {
    let mut proxies = Vec::new();
    for (name, server) in
        [("edge-hy2", "hy2.example"), ("edge", "reality.example"), ("edge", "ws-1.example"), ("edge", "ws-2.example")]
    {
        add_proxy_with_dedup(&mut proxies, v(&format!(r#"{{"name":"{name}","server":"{server}"}}"#))).unwrap();
    }
    let names: Vec<String> = proxies.iter().map(|p| p.get("name").to_js_string()).collect();
    assert_eq!(names, ["edge-hy2", "edge", "edge 2", "edge 3"]);
}
