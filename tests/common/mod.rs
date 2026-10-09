//! Shared test helpers: canned HTTP responses instead of the network.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use sublink::app::App;
use sublink::fetch::{BoxFuture, FetchResponse, Fetcher};
use sublink::storage::Store;

/// Serves registered URLs; anything else fails like an unreachable host.
#[derive(Default)]
pub struct MockFetcher {
    routes: Mutex<HashMap<String, FetchResponse>>,
    pub calls: Mutex<Vec<(String, Option<String>)>>,
}

impl MockFetcher {
    pub fn new() -> Arc<MockFetcher> {
        Arc::new(MockFetcher::default())
    }

    pub fn route(&self, url: &str, status: u16, headers: &[(&str, &str)], body: &str) {
        self.routes.lock().unwrap().insert(
            url.to_string(),
            FetchResponse {
                status,
                headers: headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
                body: body.as_bytes().to_vec(),
            },
        );
    }

    pub fn ok(&self, url: &str, body: &str) {
        self.route(url, 200, &[], body);
    }

    pub fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

impl Fetcher for MockFetcher {
    fn get<'a>(&'a self, url: &'a str, user_agent: Option<&'a str>) -> BoxFuture<'a, Result<FetchResponse, String>> {
        self.calls.lock().unwrap().push((url.to_string(), user_agent.map(str::to_string)));
        let found = self.routes.lock().unwrap().get(url).cloned();
        Box::pin(async move { found.ok_or_else(|| "fetch failed".to_string()) })
    }
}

/// App wired like the original test runtime (memory KV, 60 s config TTL).
pub fn test_app(fetcher: Arc<MockFetcher>) -> App {
    let mut app = App::new(Store::in_memory(), fetcher);
    app.config_ttl_seconds = Some(60.0);
    app.short_link_ttl_seconds = None;
    app
}

// ---------------------------------------------------------------------------
// Builder helpers mirroring the original constructor-based tests
// ---------------------------------------------------------------------------

use sublink::builders::BuildOptions;
use sublink::builders::clash::ClashBuilder;
use sublink::builders::singbox::SingboxBuilder;
use sublink::builders::surge::SurgeBuilder;
use sublink::js::{Value, json};

/// JSON literal → JS value.
pub fn v(text: &str) -> Value {
    json::parse(text).unwrap_or_else(|e| panic!("bad test JSON {text:?}: {e}"))
}

/// `new XBuilder(input, selectedRules, [], null, 'zh-CN', 'test-agent')`
pub fn opts(input: &str, selected_rules: Value) -> BuildOptions {
    BuildOptions {
        selected_rules,
        lang: Some("zh-CN".into()),
        user_agent: "test-agent".into(),
        ..BuildOptions::new(input)
    }
}

pub fn no_network() -> Arc<MockFetcher> {
    MockFetcher::new()
}

/// Builds Clash output and returns `(builder, yamlText)`.
pub async fn clash_build(o: &BuildOptions, fetcher: &MockFetcher) -> (ClashBuilder, String) {
    let mut b = ClashBuilder::new(o);
    let text = b.build(fetcher).await.unwrap_or_else(|e| panic!("clash build failed: {e}"));
    (b, text)
}

/// `yaml.load(await new ClashConfigBuilder(...).build())`
pub async fn clash(o: &BuildOptions) -> Value {
    let (_, text) = clash_build(o, &MockFetcher::default()).await;
    sublink::yaml::load(&text).unwrap()
}

pub async fn clash_with(o: &BuildOptions, fetcher: &MockFetcher) -> Value {
    let (_, text) = clash_build(o, fetcher).await;
    sublink::yaml::load(&text).unwrap()
}

pub async fn singbox_build(o: &BuildOptions, fetcher: &MockFetcher) -> (SingboxBuilder, Value) {
    let mut b = SingboxBuilder::new(o).unwrap_or_else(|e| panic!("singbox ctor failed: {e}"));
    let config = b.build(fetcher).await.unwrap_or_else(|e| panic!("singbox build failed: {e}"));
    (b, config)
}

/// `(await builder.build(), builder.config)` for sing-box.
pub async fn singbox(o: &BuildOptions) -> Value {
    singbox_build(o, &MockFetcher::default()).await.1
}

pub async fn singbox_with(o: &BuildOptions, fetcher: &MockFetcher) -> Value {
    singbox_build(o, fetcher).await.1
}

pub async fn surge_build(o: &BuildOptions, fetcher: &MockFetcher) -> (SurgeBuilder, String) {
    let mut b = SurgeBuilder::new(o);
    let text = b.build(fetcher).await.unwrap_or_else(|e| panic!("surge build failed: {e}"));
    (b, text)
}

pub async fn surge(o: &BuildOptions) -> String {
    surge_build(o, &MockFetcher::default()).await.1
}

/// `list.find(item => item?.[key] === value)`
pub fn find<'a>(list: &'a Value, key: &str, value: &str) -> Option<&'a Value> {
    list.as_array()?.iter().find(|item| item.get(key).as_str() == Some(value))
}

/// Proxy group / outbound lookup by name or tag.
pub fn group<'a>(config: &'a Value, name: &str) -> &'a Value {
    find(config.get("proxy-groups"), "name", name).unwrap_or_else(|| panic!("missing proxy group {name:?}"))
}

pub fn outbound<'a>(config: &'a Value, tag: &str) -> &'a Value {
    find(config.get("outbounds"), "tag", tag).unwrap_or_else(|| panic!("missing outbound {tag:?}"))
}

/// String items of an array value (non-strings are skipped).
pub fn strs(value: &Value) -> Vec<String> {
    value.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// vitest `toEqual` on a string array: unlike `strs`, nothing is filtered out.
pub fn exact_strs(value: &Value) -> Vec<String> {
    let list = value.as_array().unwrap_or_else(|| panic!("expected an array, got {value:?}"));
    list.iter().map(|x| x.as_str().unwrap_or_else(|| panic!("non-string item {x:?}")).to_string()).collect()
}

pub fn names(list: &Value, key: &str) -> Vec<String> {
    list.as_array().map(|a| a.iter().map(|x| x.get(key).to_js_string()).collect()).unwrap_or_default()
}

/// vitest `toEqual`: key order and `undefined` properties are ignored.
pub fn deep_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| deep_eq(p, q)),
        (Value::Object(x), Value::Object(y)) => {
            let defined = |o: &sublink::js::Object| {
                o.entries().into_iter().filter(|(_, v)| !v.is_undefined()).map(|(k, _)| k.clone()).collect::<Vec<_>>()
            };
            let (kx, ky) = (defined(x), defined(y));
            kx.len() == ky.len() && kx.iter().all(|k| y.get(k).is_some_and(|w| deep_eq(x.get(k).unwrap(), w)))
        }
        (Value::Number(x), Value::Number(y)) => x == y || (x.is_nan() && y.is_nan()),
        _ => sublink::js::strict_equals(a, b),
    }
}

/// `expect(actual).toEqual(<JSON literal>)`
#[track_caller]
pub fn assert_json(actual: &Value, expected: &str) {
    let want = v(expected);
    assert!(
        deep_eq(actual, &want),
        "values differ:\n  got:  {}\n  want: {}",
        json::stringify_or_undefined(actual),
        expected
    );
}

pub fn has(list: &Value, item: &str) -> bool {
    strs(list).iter().any(|s| s == item)
}

pub fn t(key: &str) -> String {
    sublink::i18n::Translator::new(Some("zh-CN")).ts(key)
}

/// `expect(text).toMatch(/re/)`
#[track_caller]
pub fn assert_match(text: &str, re: &str) {
    assert!(regex::Regex::new(re).unwrap().is_match(text), "{text:?} does not match /{re}/");
}

/// `expect(list).toContainEqual(expect.stringMatching(/re/))`
#[track_caller]
pub fn assert_any_match(list: &Value, re: &str) {
    let re_c = regex::Regex::new(re).unwrap();
    assert!(
        strs(list).iter().any(|s| re_c.is_match(s)),
        "no item matches /{re}/ in {}",
        json::stringify_or_undefined(list)
    );
}

/// `list.findIndex(pred)` (-1 when absent)
pub fn find_index(list: &Value, pred: impl Fn(&Value) -> bool) -> isize {
    list.as_array().and_then(|a| a.iter().position(pred)).map_or(-1, |i| i as isize)
}

/// `expect(obj).toHaveProperty(key)` (own key, even when `undefined`).
pub fn has_prop(value: &Value, key: &str) -> bool {
    value.as_object().is_some_and(|o| o.contains_key(key))
}

/// `await app.request(url)`
pub async fn request(app: &App, url: &str) -> sublink::hono::Response {
    app.handle(&sublink::hono::Request::get(url)).await
}

pub fn enc(s: &str) -> String {
    sublink::js::string::encode_uri_component(s)
}

/// vitest `toMatchObject`: objects match as subsets, arrays element-wise.
pub fn matches_object(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::Object(a), Value::Object(e)) => e.entries().into_iter().all(|(k, ev)| {
            a.get(k).is_some_and(|av| matches_object(av, ev)) || (ev.is_undefined() && a.get(k).is_none())
        }),
        (Value::Array(a), Value::Array(e)) => {
            a.len() == e.len() && a.iter().zip(e.iter()).all(|(x, y)| matches_object(x, y))
        }
        _ => deep_eq(actual, expected),
    }
}

#[track_caller]
pub fn assert_match_object(actual: &Value, expected: &str) {
    assert!(
        matches_object(actual, &v(expected)),
        "object does not match:\n  got:  {}\n  want: {}",
        json::stringify_or_undefined(actual),
        expected
    );
}
