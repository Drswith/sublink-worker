//! The slice of Hono 4.12 request/response behavior the original app relied on.
//!
//! Query lookup, path decoding and default content types all leak into
//! observable responses, so they are reproduced from Hono's source rather than
//! delegated to a generic HTTP library.

use crate::js::JsError;
use crate::js::string::{decode_uri, decode_uri_component};

/// Hono's plain-text default (`text/plain; charset=UTF-8`).
pub const TEXT_PLAIN: &str = "text/plain; charset=UTF-8";
/// What `new Response(string)` reports when Hono takes its `c.text` fast path.
pub const TEXT_PLAIN_FAST: &str = "text/plain;charset=UTF-8";

#[derive(Clone, Debug)]
pub struct Request {
    pub method: String,
    /// WHATWG-serialized absolute URL, as `new Request(url).url` would hold it.
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    /// `new Request(url, { method })`; fails where the WHATWG parser would throw.
    pub fn new(method: &str, url: &str) -> Result<Request, String> {
        let parsed = url::Url::parse(url).map_err(|e| format!("Invalid URL {url:?}: {e}"))?;
        // `new Request(url)` rejects credentials, e.g. from a `Host: user@host` header.
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return Err("Request cannot be constructed from a URL that includes credentials".into());
        }
        Ok(Request { method: method.to_string(), url: parsed.into(), headers: Vec::new(), body: Vec::new() })
    }

    /// Test convenience mirroring `app.request(url)`.
    pub fn get(url: &str) -> Request {
        Request::new("GET", url).expect("valid request URL")
    }

    pub fn with_header(mut self, name: &str, value: &str) -> Request {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    pub fn with_body(mut self, body: impl Into<Vec<u8>>) -> Request {
        self.body = body.into();
        self
    }

    /// `Headers.get(name)`: case-insensitive, repeated values joined by `", "`.
    pub fn header(&self, name: &str) -> Option<String> {
        let values: Vec<&str> = self
            .headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.trim_matches(|c| matches!(c, ' ' | '\t' | '\r' | '\n')))
            .collect();
        (!values.is_empty()).then(|| values.join(", "))
    }

    /// `c.req.query(key)`
    pub fn query(&self, key: &str) -> Option<String> {
        query(&self.url, key)
    }
}

#[derive(Clone, Debug)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: u16, headers: Vec<(String, String)>, body: impl Into<Vec<u8>>) -> Response {
        Response { status, headers, body: body.into() }
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// Response headers in `Headers` semantics: later `set` replaces earlier ones.
#[derive(Default)]
pub struct HeaderMap(Vec<(String, String)>);

impl HeaderMap {
    pub fn set(&mut self, name: &str, value: impl Into<String>) {
        let name = name.to_ascii_lowercase();
        let value = value.into();
        match self.0.iter_mut().find(|(k, _)| *k == name) {
            Some(slot) => slot.1 = value,
            None => self.0.push((name, value)),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// `c.text(text)` with nothing prepared: Hono hands the string straight to `new Response`.
pub fn text_fast(body: impl Into<String>) -> Response {
    Response::new(200, vec![("content-type".into(), TEXT_PLAIN_FAST.into())], body.into())
}

/// `c.text(text, status, headers)` once headers or a status are involved.
pub fn text(body: impl Into<String>, status: u16, prepared: HeaderMap) -> Response {
    with_default_type(body.into(), status, prepared, TEXT_PLAIN)
}

/// `c.json(object)`
pub fn json(body: String, prepared: HeaderMap) -> Response {
    with_default_type(body, 200, prepared, "application/json")
}

/// `c.html(html)`
pub fn html(body: String) -> Response {
    with_default_type(body, 200, HeaderMap::default(), "text/html; charset=UTF-8")
}

/// `c.redirect(location)`: 302 without a body.
pub fn redirect(location: &str) -> Response {
    let location = if location.chars().any(|c| c as u32 > 0xFF) {
        crate::js::string::encode_uri(location)
    } else {
        location.into()
    };
    Response::new(302, vec![("location".into(), location)], Vec::new())
}

/// Hono's default not-found handler.
pub fn not_found() -> Response {
    text("404 Not Found", 404, HeaderMap::default())
}

/// Hono's default error handler.
pub fn internal_error() -> Response {
    text("Internal Server Error", 500, HeaderMap::default())
}

fn with_default_type(body: String, status: u16, prepared: HeaderMap, content_type: &str) -> Response {
    // setDefaultContentType puts Content-Type first, then caller headers win.
    let mut headers = prepared;
    if !headers.0.iter().any(|(k, _)| k == "content-type") {
        headers.set("content-type", content_type);
    }
    Response::new(status, headers.0, body)
}

/// Hono `tryDecode`: on failure, decode each `%XX` run independently.
fn try_decode(s: &str, decoder: fn(&str) -> Result<String, JsError>) -> String {
    if let Ok(d) = decoder(s) {
        return d;
    }
    let bytes = s.as_bytes();
    let is_escape = |i: usize| {
        i + 2 < bytes.len() && bytes[i] == b'%' && bytes[i + 1].is_ascii_hexdigit() && bytes[i + 2].is_ascii_hexdigit()
    };
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    let mut plain_start = 0;
    while i < bytes.len() {
        if is_escape(i) {
            let start = i;
            while is_escape(i) {
                i += 3;
            }
            out.push_str(&s[plain_start..start]);
            let run = &s[start..i];
            out.push_str(&decoder(run).unwrap_or_else(|_| run.to_string()));
            plain_start = i;
        } else {
            i += 1;
        }
    }
    out.push_str(&s[plain_start..]);
    out
}

/// `tryDecodeURIComponent`, used for route params.
pub fn decode_param(param: &str) -> String {
    if param.contains('%') { try_decode(param, decode_uri_component) } else { param.to_string() }
}

/// Hono `getPath(request)`.
pub fn get_path(url: &str) -> String {
    let after_scheme = url.find(':').map_or(3, |i| i + 4);
    let Some(start) = url.get(after_scheme..).and_then(|rest| rest.find('/')).map(|i| i + after_scheme) else {
        // Unreachable for http(s) URLs, which always serialize a path.
        return String::new();
    };
    let bytes = url.as_bytes();
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let end = [url[i..].find('?'), url[i..].find('#')].into_iter().flatten().min().map(|o| o + i);
                let path = &url[start..end.unwrap_or(url.len())];
                let path = if path.contains("%25") { path.replace("%25", "%2525") } else { path.to_string() };
                return try_decode(&path, decode_uri);
            }
            b'?' | b'#' => break,
            _ => i += 1,
        }
    }
    url[start..i].to_string()
}

/// Hono `_decodeURI`: `+` means space, then lenient `decodeURIComponent`.
fn decode_query_part(value: &str) -> String {
    if !value.contains(['%', '+']) {
        return value.to_string();
    }
    let value = value.replace('+', " ");
    if value.contains('%') { try_decode(&value, decode_uri_component) } else { value }
}

fn find_from(haystack: &str, needle: &str, from: usize) -> Option<usize> {
    haystack.get(from..)?.find(needle).map(|i| i + from)
}

/// Hono `getQueryParam(url, key)` including its fast path, whose results
/// differ from a spec-compliant parser for odd inputs.
pub fn query(url: &str, key: &str) -> Option<String> {
    let bytes = url.as_bytes();
    let qmark = find_from(url, "?", 8)?;
    let encoded = if !key.is_empty() && !key.contains(['%', '+']) {
        let amp_key = format!("&{key}");
        let mut idx = if url[qmark + 1..].starts_with(key) { Some(qmark) } else { find_from(url, &amp_key, qmark + 1) };
        while let Some(k) = idx {
            match bytes.get(k + key.len() + 1) {
                Some(b'=') => {
                    let value_index = k + key.len() + 2;
                    let end = find_from(url, "&", value_index).unwrap_or(url.len());
                    return Some(decode_query_part(&url[value_index..end]));
                }
                Some(b'&') | None => return Some(String::new()),
                _ => {}
            }
            idx = find_from(url, &amp_key, k + 1);
        }
        if !url.contains(['%', '+']) {
            return None;
        }
        true
    } else {
        url.contains(['%', '+'])
    };

    let mut key_index = Some(qmark);
    while let Some(k) = key_index {
        let next = find_from(url, "&", k + 1);
        let mut value_index = find_from(url, "=", k);
        if let (Some(v), Some(n)) = (value_index, next)
            && v > n
        {
            value_index = None;
        }
        let name_end = value_index.or(next).unwrap_or(url.len());
        let mut name = url[k + 1..name_end].to_string();
        if encoded {
            name = decode_query_part(&name);
        }
        key_index = next;
        if name.is_empty() {
            continue;
        }
        if name == key {
            // First occurrence wins (`results[name] ??= value`).
            return Some(match value_index {
                None => String::new(),
                Some(v) => {
                    let raw = &url[v + 1..next.unwrap_or(url.len())];
                    if encoded { decode_query_part(raw) } else { raw.to_string() }
                }
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_matches_hono() {
        let u = "http://h/p?a=1&b=x+y&c&d=%E4%B8%AD&e=%ZZ%41&ab=2";
        assert_eq!(query(u, "a").as_deref(), Some("1"));
        assert_eq!(query(u, "b").as_deref(), Some("x y"));
        assert_eq!(query(u, "c").as_deref(), Some(""));
        assert_eq!(query(u, "d").as_deref(), Some("中"));
        assert_eq!(query(u, "e").as_deref(), Some("%ZZA"));
        assert_eq!(query(u, "ab").as_deref(), Some("2"));
        assert_eq!(query(u, "z"), None);
        assert_eq!(query("http://h/p?con%66ig=1", "config").as_deref(), Some("1"));
        assert_eq!(query("http://h/p", "a"), None);
        assert_eq!(query("http://h/p?uax=1&ua=2", "ua").as_deref(), Some("2"));
        assert_eq!(query("http://h/p?a=1&a=2", "a").as_deref(), Some("1"));
    }

    #[test]
    fn path_matches_hono() {
        assert_eq!(get_path("http://h/singbox?x=1"), "/singbox");
        assert_eq!(get_path("http://h/b/a%2Fb"), "/b/a%2Fb");
        assert_eq!(get_path("http://h/b/%E4%B8%AD?x"), "/b/中");
        assert_eq!(get_path("http://h/b/a%2541"), "/b/a%2541");
        assert_eq!(decode_param("a%2Fb"), "a/b");
        assert_eq!(decode_param("a%2541"), "a%41");
        assert_eq!(decode_param("%E4%B8%AD%FF"), "%E4%B8%AD%FF");
    }
}
