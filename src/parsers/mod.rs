//! Input parsing: share links, remote subscriptions and full configs.

pub mod clash_proxy;
pub mod content;
pub mod protocols;
pub mod subscription;
pub mod surge;

use crate::fetch::Fetcher;
use crate::js::string::js_trim;
use crate::js::{JsResult, Value};

/// `ProxyParser.parse(url, userAgent)`: dispatches on the URL scheme.
/// Returns `undefined` for unknown schemes; http(s) URLs are fetched and their
/// parsed content returned.
pub async fn parse_proxy(fetcher: &dyn Fetcher, url: &str, user_agent: &str) -> JsResult<Value> {
    if url.is_empty() {
        return Ok(Value::Undefined);
    }
    let trimmed = js_trim(url);
    let scheme = trimmed.split("://").next().unwrap_or("").to_lowercase();
    match scheme.as_str() {
        "ss" => protocols::parse_shadowsocks(trimmed),
        "vmess" => protocols::parse_vmess(trimmed),
        "vless" => protocols::parse_vless(trimmed),
        "hysteria" | "hysteria2" | "hy2" => protocols::parse_hysteria2(trimmed),
        "trojan" => protocols::parse_trojan(trimmed),
        "tuic" => protocols::parse_tuic(trimmed),
        "anytls" => protocols::parse_anytls(trimmed),
        "http" | "https" => Ok(subscription::fetch_subscription(fetcher, trimmed, user_agent).await),
        _ => Ok(Value::Undefined),
    }
}
