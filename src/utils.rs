//! Port of the shared helpers in the original `src/utils.js`.

use indexmap::IndexMap;

use crate::js::base64::decode_base64;
use crate::js::number::{parse_int, string_to_number};
use crate::js::string::{decode_uri_component, is_word_char, js_slice, js_trim, utf16};
use crate::js::{JsError, JsResult, Object, Value, str_array};
use crate::obj;

/// Result of `tryDecodeSubscriptionLines`: a single string or a list of lines.
#[derive(Clone, Debug, PartialEq)]
pub enum Lines {
    One(String),
    Many(Vec<String>),
}

impl Lines {
    pub fn into_vec(self) -> Vec<String> {
        match self {
            Lines::One(s) => vec![s],
            Lines::Many(v) => v,
        }
    }
}

fn split_if_multiple(value: &str) -> Lines {
    let normalized = value.replace("\r\n", "\n");
    let segments: Vec<String> =
        normalized.split('\n').map(|s| js_trim(s).to_string()).filter(|s| !s.is_empty()).collect();
    if segments.len() > 1 && segments.iter().any(|s| s.contains("://")) {
        return Lines::Many(segments);
    }
    Lines::One(js_trim(&normalized).to_string())
}

/// `tryDecodeSubscriptionLines(input, { decodeUriComponent })`
pub fn try_decode_subscription_lines(input: &str, decode_uri: bool) -> Lines {
    let trimmed = js_trim(input);
    if trimmed.is_empty() {
        return Lines::One(String::new());
    }
    match split_if_multiple(trimmed) {
        Lines::Many(v) => return Lines::Many(v),
        Lines::One(s) if s.contains("://") => return Lines::One(s),
        _ => {}
    }
    let mut decoded = decode_base64(trimmed);
    if decode_uri
        && decoded.contains('%')
        && !decoded.contains("://")
        && let Ok(d) = decode_uri_component(&decoded)
    {
        decoded = d;
    }
    match split_if_multiple(&decoded) {
        Lines::Many(v) => return Lines::Many(v),
        Lines::One(s) if s.contains("://") => return Lines::One(s),
        _ => {}
    }
    Lines::One(trimmed.to_string())
}

// ---------------------------------------------------------------------------
// Countries
// ---------------------------------------------------------------------------

pub struct Country {
    pub code: &'static str,
    pub name: &'static str,
    pub emoji: &'static str,
    pub aliases: &'static [&'static str],
}

pub static COUNTRY_DATA: &[Country] = &[
    Country { code: "HK", name: "Hong Kong", emoji: "🇭🇰", aliases: &["香港", "Hong Kong", "HK"] },
    Country { code: "TW", name: "Taiwan", emoji: "🇹🇼", aliases: &["台湾", "Taiwan", "TW"] },
    Country { code: "JP", name: "Japan", emoji: "🇯🇵", aliases: &["日本", "Japan", "JP"] },
    Country { code: "KR", name: "Korea", emoji: "🇰🇷", aliases: &["韩国", "Korea", "KR"] },
    Country { code: "SG", name: "Singapore", emoji: "🇸🇬", aliases: &["新加坡", "Singapore", "SG"] },
    Country { code: "US", name: "United States", emoji: "🇺🇸", aliases: &["美国", "United States", "US"] },
    Country {
        code: "GB", name: "United Kingdom", emoji: "🇬🇧", aliases: &["英国", "United Kingdom", "UK", "GB"]
    },
    Country { code: "DE", name: "Germany", emoji: "🇩🇪", aliases: &["德国", "Germany"] },
    Country { code: "FR", name: "France", emoji: "🇫🇷", aliases: &["法国", "France"] },
    Country { code: "RU", name: "Russia", emoji: "🇷🇺", aliases: &["俄罗斯", "Russia"] },
    Country { code: "CA", name: "Canada", emoji: "🇨🇦", aliases: &["加拿大", "Canada"] },
    Country { code: "AU", name: "Australia", emoji: "🇦🇺", aliases: &["澳大利亚", "Australia"] },
    Country { code: "IN", name: "India", emoji: "🇮🇳", aliases: &["印度", "India"] },
    Country { code: "BR", name: "Brazil", emoji: "🇧🇷", aliases: &["巴西", "Brazil"] },
    Country { code: "ZA", name: "South Africa", emoji: "🇿🇦", aliases: &["南非", "South Africa"] },
    Country { code: "AR", name: "Argentina", emoji: "🇦🇷", aliases: &["阿根廷", "Argentina"] },
    Country { code: "TR", name: "Turkey", emoji: "🇹🇷", aliases: &["土耳其", "Turkey"] },
    Country { code: "NL", name: "Netherlands", emoji: "🇳🇱", aliases: &["荷兰", "Netherlands"] },
    Country { code: "CH", name: "Switzerland", emoji: "🇨🇭", aliases: &["瑞士", "Switzerland"] },
    Country { code: "SE", name: "Sweden", emoji: "🇸🇪", aliases: &["瑞典", "Sweden"] },
    Country { code: "IT", name: "Italy", emoji: "🇮🇹", aliases: &["意大利", "Italy"] },
    Country { code: "ES", name: "Spain", emoji: "🇪🇸", aliases: &["西班牙", "Spain"] },
    Country { code: "IE", name: "Ireland", emoji: "🇮🇪", aliases: &["爱尔兰", "Ireland"] },
    Country { code: "MY", name: "Malaysia", emoji: "🇲🇾", aliases: &["马来西亚", "Malaysia"] },
    Country { code: "TH", name: "Thailand", emoji: "🇹🇭", aliases: &["泰国", "Thailand"] },
    Country { code: "VN", name: "Vietnam", emoji: "🇻🇳", aliases: &["越南", "Vietnam"] },
    Country { code: "PH", name: "Philippines", emoji: "🇵🇭", aliases: &["菲律宾", "Philippines"] },
    Country { code: "ID", name: "Indonesia", emoji: "🇮🇩", aliases: &["印度尼西亚", "Indonesia"] },
    Country { code: "NZ", name: "New Zealand", emoji: "🇳🇿", aliases: &["新西兰", "New Zealand"] },
    Country {
        code: "AE", name: "United Arab Emirates", emoji: "🇦🇪", aliases: &["阿联酋", "United Arab Emirates"]
    },
];

fn is_short_ascii_alias(alias: &str) -> bool {
    alias.chars().count() <= 3 && !alias.is_empty() && alias.chars().all(|c| c.is_ascii_alphabetic())
}

/// Aliases in regex alternation order: longest first, stable otherwise.
fn sorted_aliases() -> &'static [&'static str] {
    static SORTED: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    SORTED.get_or_init(|| {
        let mut all: Vec<&'static str> = COUNTRY_DATA.iter().flat_map(|c| c.aliases.iter().copied()).collect();
        all.sort_by_key(|a| std::cmp::Reverse(utf16(a).len()));
        all
    })
}

fn match_alias_at(chars: &[char], pos: usize, alias: &str) -> bool {
    let a: Vec<char> = alias.chars().collect();
    if pos + a.len() > chars.len() {
        return false;
    }
    for (i, ac) in a.iter().enumerate() {
        let c = chars[pos + i];
        let eq = if ac.is_ascii() { c.is_ascii() && c.eq_ignore_ascii_case(ac) } else { c == *ac };
        if !eq {
            return false;
        }
    }
    if is_short_ascii_alias(alias) {
        let word = |c: Option<&char>| c.is_some_and(|c| is_word_char(*c));
        let before = if pos == 0 { None } else { chars.get(pos - 1) };
        let after = chars.get(pos + a.len());
        if word(before) || word(after) {
            return false;
        }
    }
    true
}

/// `parseCountryFromNodeName`
pub fn parse_country_from_node_name(node_name: &str) -> Option<&'static Country> {
    let chars: Vec<char> = node_name.chars().collect();
    let aliases = sorted_aliases();
    for pos in 0..=chars.len() {
        for alias in aliases {
            if match_alias_at(&chars, pos, alias) {
                let matched: String = chars[pos..pos + alias.chars().count()].iter().collect();
                let matched_lower = matched.to_lowercase();
                return COUNTRY_DATA.iter().find(|c| c.aliases.iter().any(|a| a.to_lowercase() == matched_lower));
            }
        }
    }
    None
}

pub struct CountryGroup {
    pub country: &'static Country,
    pub proxies: Vec<String>,
}

fn normalize_node_name(value: &Value) -> Option<String> {
    let s = value.as_str()?;
    let trimmed = js_trim(s);
    if trimmed.is_empty() {
        return None;
    }
    if let Some(eq) = trimmed.find('=') {
        let before = js_trim(&trimmed[..eq]);
        if !before.is_empty() {
            return Some(before.to_string());
        }
    }
    Some(trimmed.to_string())
}

/// `groupProxiesByCountry(proxies, { getName })`, keyed by country name in
/// first-seen order.
pub fn group_proxies_by_country<T>(proxies: &[T], get_name: impl Fn(&T) -> Value) -> IndexMap<String, CountryGroup> {
    let mut grouped: IndexMap<String, CountryGroup> = IndexMap::new();
    for proxy in proxies {
        let Some(name) = normalize_node_name(&get_name(proxy)) else { continue };
        let Some(country) = parse_country_from_node_name(&name) else { continue };
        grouped
            .entry(country.name.to_string())
            .or_insert_with(|| CountryGroup { country, proxies: Vec::new() })
            .proxies
            .push(name);
    }
    grouped
}

/// Default name extractor used when `getName` is not supplied.
pub fn default_proxy_name(proxy: &Value) -> Value {
    match proxy {
        Value::Undefined | Value::Null => Value::Undefined,
        Value::String(_) => proxy.clone(),
        Value::Object(_) | Value::Array(_) | Value::Date(_) => {
            for key in ["name", "tag", "id", "ps"] {
                let v = proxy.get(key);
                if !v.is_nullish() {
                    return v.clone();
                }
            }
            Value::Undefined
        }
        _ => Value::Undefined,
    }
}

fn escape_regex(alias: &str) -> String {
    let mut out = String::new();
    for c in alias.chars() {
        if "-/\\^$*+?.()|[]{}".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// `buildCountryNameFilter({ emoji, aliases })`
pub fn build_country_name_filter(country: &Country) -> Option<String> {
    let mut patterns: Vec<String> = country
        .aliases
        .iter()
        .map(|alias| {
            let escaped = escape_regex(alias);
            if is_short_ascii_alias(alias) { format!("\\b{}\\b", escaped) } else { escaped }
        })
        .collect();
    if !country.emoji.is_empty() {
        patterns.push(country.emoji.to_string());
    }
    if patterns.is_empty() {
        return None;
    }
    Some(format!("(?i){}", patterns.join("|")))
}

/// `createStableProviderName(url)` (FNV-1a over UTF-16 code units, base 36).
pub fn create_stable_provider_name(url: &str) -> JsResult<String> {
    let normalized = js_trim(url);
    if normalized.is_empty() {
        return Err(JsError::error("Provider URL must be a non-empty string"));
    }
    let mut hash: u32 = 0x811c9dc5;
    for unit in normalized.encode_utf16() {
        hash ^= unit as u32;
        hash = hash.wrapping_mul(0x01000193);
    }
    Ok(format!("_auto_provider_{}", to_base36(hash)))
}

fn to_base36(mut n: u32) -> String {
    if n == 0 {
        return "0".into();
    }
    let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    while n > 0 {
        out.push(digits[(n % 36) as usize]);
        n /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap()
}

/// `generateWebPath(length)`
pub fn generate_web_path(length: usize) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    (0..length).map(|_| CHARS[fastrand::usize(..CHARS.len())] as char).collect()
}

/// `parseServerInfo(serverInfo)` → (host, port) with JS slicing quirks kept.
pub fn parse_server_info(server_info: &Value) -> (Value, Value) {
    let Some(info) = server_info.as_str().filter(|s| !s.is_empty()) else {
        return (Value::Null, Value::Null);
    };
    let units = utf16(info);
    let (host, port) = if info.starts_with('[') {
        let close = units.iter().position(|&u| u == ']' as u16).map(|p| p as isize).unwrap_or(-1);
        (js_slice(info, 1, Some(close)), js_slice(info, close + 2, None))
    } else {
        let last = units.iter().rposition(|&u| u == ':' as u16).map(|p| p as isize).unwrap_or(-1);
        (js_slice(info, 0, Some(last)), js_slice(info, last + 1, None))
    };
    (Value::String(host), Value::Number(parse_int(&port, 0)))
}

/// Parsed `application/x-www-form-urlencoded` query parameters.
///
/// `get`/`val` follow `Object.fromEntries(searchParams)` (last value wins),
/// `first` follows `URLSearchParams.get` (first value wins).
#[derive(Clone, Debug, Default)]
pub struct Params {
    pairs: Vec<(String, String)>,
    overrides: Vec<(String, String)>,
}

impl Params {
    pub fn parse(query: &str) -> Params {
        let q = query.strip_prefix('?').unwrap_or(query);
        Params { pairs: parse_search_params(q), overrides: Vec::new() }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        if let Some((_, v)) = self.overrides.iter().rev().find(|(k, _)| k == key) {
            return Some(v);
        }
        self.pairs.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    /// `[...searchParams]`
    pub fn entries(&self) -> &[(String, String)] {
        &self.pairs
    }

    pub fn first(&self, key: &str) -> Option<&str> {
        self.pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    /// The parameter as a JS value (`undefined` when absent).
    pub fn val(&self, key: &str) -> Value {
        self.get(key).map(Value::str).unwrap_or(Value::Undefined)
    }

    pub fn has(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// `params[key] = value`
    pub fn set(&mut self, key: &str, value: &str) {
        self.overrides.push((key.to_string(), value.to_string()));
    }

    /// `params.a || params.b`
    pub fn either(&self, a: &str, b: &str) -> Value {
        self.val(a).or_falsy(|| self.val(b))
    }
}

/// Node's WHATWG `URLSearchParams` parser: `+` is a space, and a key or value
/// goes through `querystring.unescape` only when it holds a valid `%XX`.
fn parse_search_params(qs: &str) -> Vec<(String, String)> {
    let bytes = qs.as_bytes();
    let mut flat: Vec<String> = Vec::new();
    let mut buf = String::new();
    let (mut seen_sep, mut encoded, mut encode_check) = (false, false, 0u8);
    let (mut pair_start, mut last_pos) = (0usize, 0usize);
    let take = |buf: &mut String, encoded: bool| {
        let s = std::mem::take(buf);
        if encoded { qs_unescape(&s) } else { s }
    };
    for (i, &c) in bytes.iter().enumerate() {
        if c == b'&' {
            if pair_start == i {
                pair_start = i + 1;
                last_pos = i + 1;
                continue;
            }
            buf.push_str(&qs[last_pos..i]);
            flat.push(take(&mut buf, encoded));
            if !seen_sep {
                flat.push(String::new());
            }
            (seen_sep, encoded, encode_check) = (false, false, 0);
            pair_start = i + 1;
            last_pos = i + 1;
        } else if !seen_sep && c == b'=' {
            buf.push_str(&qs[last_pos..i]);
            flat.push(take(&mut buf, encoded));
            (seen_sep, encoded, encode_check) = (true, false, 0);
            last_pos = i + 1;
        } else if c == b'+' {
            buf.push_str(&qs[last_pos..i]);
            buf.push(' ');
            last_pos = i + 1;
        } else if !encoded {
            if c == b'%' {
                encode_check = 1;
            } else if encode_check > 0 && c.is_ascii_hexdigit() {
                encode_check += 1;
                encoded = encode_check == 3;
            } else {
                encode_check = 0;
            }
        }
    }
    if pair_start != bytes.len() {
        buf.push_str(&qs[last_pos..]);
        flat.push(take(&mut buf, encoded));
        if !seen_sep {
            flat.push(String::new());
        }
    }
    let mut it = flat.into_iter();
    std::iter::from_fn(|| Some((it.next()?, it.next()?))).collect()
}

/// `querystring.unescape`: when `decodeURIComponent` throws, Node rebuilds the
/// bytes from each UTF-16 unit's low byte, which mangles non-ASCII text.
fn qs_unescape(s: &str) -> String {
    if let Ok(decoded) = decode_uri_component(s) {
        return decoded;
    }
    let units = utf16(s);
    let hex = |u: u16| char::from_u32(u as u32).and_then(|c| c.to_digit(16)).map(|d| d as u16);
    let (len, mut i) = (units.len(), 0usize);
    let mut out = Vec::with_capacity(len);
    while i < len {
        let mut unit = units[i];
        if unit == b'%' as u16 && i + 2 < len {
            i += 1;
            unit = units[i];
            match hex(unit) {
                None => {
                    out.push(b'%');
                    continue;
                }
                Some(high) => match hex(units[i + 1]) {
                    None => out.push(b'%'),
                    Some(low) => {
                        i += 1;
                        unit = high * 16 + low;
                    }
                },
            }
        }
        // Buffer element assignment keeps only the low byte.
        out.push(unit as u8);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub struct UrlParts {
    pub address_part: String,
    pub params: Params,
    pub name: String,
}

/// `parseUrlParams(url)`
pub fn parse_url_params(url: &str) -> JsResult<UrlParts> {
    let mut parts = url.split("://");
    parts.next();
    let Some(rest) = parts.next() else {
        return Err(JsError::read_prop(&Value::Undefined, "split"));
    };
    let mut q = rest.split('?');
    let address_part = q.next().unwrap_or("").to_string();
    let params_part = q.collect::<Vec<_>>().join("?");
    let mut h = params_part.split('#');
    let params_only = h.next().unwrap_or("");
    let fragment: Vec<&str> = h.collect();
    let mut name = if fragment.is_empty() { String::new() } else { fragment.join("#") };
    if let Ok(decoded) = decode_uri_component(&name) {
        name = decoded;
    }
    Ok(UrlParts { address_part, params: Params::parse(params_only), name })
}

/// `createTlsConfig(params)`
pub fn create_tls_config(params: &Params) -> Value {
    let security = params.val("security");
    if !(security.truthy() && security.as_str() != Some("none")) {
        return obj! { "enabled" => false };
    }
    let insecure = params.val("allowInsecure").truthy()
        || params.val("insecure").truthy()
        || params.val("allow_insecure").truthy();
    let mut tls = Object::new();
    tls.set("enabled", Value::Bool(true));
    tls.set("server_name", params.either("sni", "host"));
    tls.set("insecure", Value::Bool(insecure));
    if security.as_str() == Some("reality") {
        tls.set(
            "reality",
            obj! { "enabled" => true, "public_key" => params.val("pbk"), "short_id" => params.val("sid") },
        );
    }
    Value::Object(tls)
}

/// `createTransportConfig(params)`
pub fn create_transport_config(params: &Params) -> Value {
    let mut t = Object::new();
    t.set("type", params.val("type"));
    t.set("path", params.val("path"));
    let host = params.val("host");
    if host.truthy() {
        t.set("headers", obj! { "host" => host });
    }
    if params.get("type") == Some("grpc") {
        t.set("service_name", params.val("serviceName"));
    }
    Value::Object(t)
}

/// `parseBool(value, fallback)`
pub fn parse_bool(value: &Value, fallback: Value) -> Value {
    match value {
        Value::Undefined | Value::Null => fallback,
        Value::Bool(_) => value.clone(),
        other => match other.to_js_string().to_lowercase().as_str() {
            "true" | "1" => Value::Bool(true),
            "false" | "0" => Value::Bool(false),
            _ => fallback,
        },
    }
}

/// `parseMaybeNumber(value)`
pub fn parse_maybe_number(value: &Value) -> Value {
    if value.is_nullish() {
        return Value::Undefined;
    }
    let n = value.to_number();
    if n.is_nan() { Value::Undefined } else { Value::Number(n) }
}

/// `parseArray(value)`
pub fn parse_array(value: &Value) -> Value {
    if !value.truthy() {
        return Value::Undefined;
    }
    if value.is_array() {
        return value.clone();
    }
    str_array(value.to_js_string().split(',').map(js_trim).filter(|s| !s.is_empty()))
}

/// `Number(x)` on a string.
pub fn number(s: &str) -> f64 {
    string_to_number(s)
}
