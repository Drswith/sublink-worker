//! `httpSubscriptionFetcher.js`: download remote subscriptions, undo Base64 /
//! URI encoding when the payload proves it is encoded, and detect the format.

use std::time::Duration;

use super::content::parse_subscription_content;
use crate::fetch::{FetchResponse, Fetcher};
use crate::js::base64::decode_base64;
use crate::js::string::{decode_uri_component, is_js_whitespace, js_trim};
use crate::js::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Clash,
    Singbox,
    Surge,
    Unknown,
}

const URI_SCHEMES: &[&str] =
    &["ss", "vmess", "vless", "hysteria", "hysteria2", "hy2", "trojan", "tuic", "anytls", "http", "https"];

fn is_subscription_uri(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    URI_SCHEMES.iter().any(|s| lower.starts_with(&format!("{}://", s)))
}

fn has_subscription_uri_line(content: &str) -> bool {
    content
        .split('\n')
        .map(|l| js_trim(l.strip_suffix('\r').unwrap_or(l)))
        .filter(|l| !l.is_empty())
        .any(is_subscription_uri)
}

fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// `/^\s*\[[^\]]+\]\s*$/m.test(content) && /^\s*[A-Za-z0-9_.-]+\s*=/.test(content)`
fn is_likely_toml_config(content: &str) -> bool {
    let chars: Vec<char> = content.chars().collect();
    let section_line = (0..=chars.len()).any(|p| {
        if p > 0 && !is_line_terminator(chars[p - 1]) {
            return false;
        }
        let mut i = p;
        while i < chars.len() && is_js_whitespace(chars[i]) {
            i += 1;
        }
        if chars.get(i) != Some(&'[') {
            return false;
        }
        let Some(close) = chars[i + 1..].iter().position(|&c| c == ']').map(|o| o + i + 1) else { return false };
        if close == i + 1 {
            return false;
        }
        let mut k = close + 1;
        loop {
            if k == chars.len() || is_line_terminator(chars[k]) {
                return true;
            }
            if !is_js_whitespace(chars[k]) {
                return false;
            }
            k += 1;
        }
    });
    if !section_line {
        return false;
    }
    let mut i = 0;
    while i < chars.len() && is_js_whitespace(chars[i]) {
        i += 1;
    }
    let start = i;
    while i < chars.len() && (chars[i].is_ascii_alphanumeric() || matches!(chars[i], '_' | '.' | '-')) {
        i += 1;
    }
    if i == start {
        return false;
    }
    while i < chars.len() && is_js_whitespace(chars[i]) {
        i += 1;
    }
    chars.get(i) == Some(&'=')
}

/// `detectFormat(content)`
pub fn detect_format(content: &str) -> Format {
    let trimmed = js_trim(content);
    if trimmed.starts_with('{')
        && let Ok(parsed) = json::parse(trimmed)
        && (parsed.get("outbounds").truthy() || parsed.get("inbounds").truthy() || parsed.get("route").truthy())
    {
        return Format::Singbox;
    }
    if trimmed.contains("proxies:") {
        return Format::Clash;
    }
    let lower = trimmed.to_ascii_lowercase();
    if ["[general]", "[proxy]", "[rule]", "[proxy group]"].iter().any(|s| lower.contains(s)) {
        return Format::Surge;
    }
    Format::Unknown
}

fn is_plain_subscription_content(content: &str) -> bool {
    detect_format(content) != Format::Unknown || has_subscription_uri_line(content) || is_likely_toml_config(content)
}

fn decode_uri_component_if_needed(text: &str) -> String {
    let trimmed = js_trim(text);
    if !trimmed.contains('%') {
        return trimmed.to_string();
    }
    match decode_uri_component(trimmed) {
        Ok(d) => js_trim(&d).to_string(),
        Err(e) => {
            eprintln!("Failed to URL decode the text: URIError: {}", e.message);
            trimmed.to_string()
        }
    }
}

fn normalize_base64_candidate(text: &str) -> Option<String> {
    let compact: String = text.chars().filter(|c| !is_js_whitespace(*c)).collect();
    if compact.is_empty() {
        return None;
    }
    let body = compact.trim_end_matches('=');
    let padding = compact.len() - body.len();
    if padding > 2 || !body.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '_' | '-')) {
        return None;
    }
    if body.len() % 4 == 1 {
        return None;
    }
    let normalized = body.replace('-', "+").replace('_', "/");
    let pad = (4 - normalized.len() % 4) % 4;
    Some(format!("{}{}", normalized, "=".repeat(pad)))
}

/// `decodeContent(text)`
pub fn decode_content(text: &str) -> String {
    let url_decoded = decode_uri_component_if_needed(text);
    if is_plain_subscription_content(&url_decoded) {
        return url_decoded;
    }
    let Some(candidate) = normalize_base64_candidate(&url_decoded) else { return url_decoded };
    let decoded = decode_uri_component_if_needed(&decode_base64(&candidate));
    if is_plain_subscription_content(&decoded) {
        return decoded;
    }
    url_decoded
}

/// The original aborted subscription downloads (body included) after 15 seconds.
const SUBSCRIPTION_TIMEOUT: Duration = Duration::from_secs(15);

async fn get_with_timeout(fetcher: &dyn Fetcher, url: &str, user_agent: &str) -> Result<FetchResponse, String> {
    let ua = if user_agent.is_empty() { None } else { Some(user_agent) };
    tokio::time::timeout(SUBSCRIPTION_TIMEOUT, fetcher.get(url, ua))
        .await
        .unwrap_or_else(|_| Err("TimeoutError: The operation was aborted due to timeout".into()))
}

/// `fetchSubscription(url, userAgent)`: parsed content, or null on any failure.
pub async fn fetch_subscription(fetcher: &dyn Fetcher, url: &str, user_agent: &str) -> Value {
    match get_with_timeout(fetcher, url, user_agent).await {
        Ok(resp) if !resp.ok() => {
            eprintln!("Error fetching or parsing HTTP(S) content: Error: HTTP error! status: {}", resp.status);
            Value::Null
        }
        Ok(FetchResponse { body_error: Some(e), .. }) => {
            eprintln!("Error fetching or parsing HTTP(S) content: {e}");
            Value::Null
        }
        Ok(resp) => parse_subscription_content(&decode_content(&resp.text())),
        Err(e) => {
            eprintln!("Error fetching or parsing HTTP(S) content: {}", e);
            Value::Null
        }
    }
}

pub struct FetchedSubscription {
    pub content: String,
    pub format: Format,
    pub url: String,
    pub subscription_userinfo: Option<String>,
}

/// `fetchSubscriptionWithFormat(url, userAgent)`
pub async fn fetch_subscription_with_format(
    fetcher: &dyn Fetcher,
    url: &str,
    user_agent: &str,
) -> Option<FetchedSubscription> {
    match get_with_timeout(fetcher, url, user_agent).await {
        Ok(resp) if !resp.ok() => {
            eprintln!("Error fetching subscription: Error: HTTP error! status: {}", resp.status);
            None
        }
        Ok(FetchResponse { body_error: Some(e), .. }) => {
            eprintln!("Error fetching subscription: {e}");
            None
        }
        Ok(resp) => {
            let content = decode_content(&resp.text());
            let format = detect_format(&content);
            let subscription_userinfo = resp.header("subscription-userinfo").filter(|s| !s.is_empty());
            Some(FetchedSubscription { content, format, url: url.to_string(), subscription_userinfo })
        }
        Err(e) => {
            eprintln!("Error fetching subscription: {}", e);
            None
        }
    }
}
