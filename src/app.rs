//! Routes of the original `createApp()`, request in, response out.

use std::sync::Arc;

use crate::builders::BuildOptions;
use crate::builders::clash::ClashBuilder;
use crate::builders::singbox::SingboxBuilder;
use crate::builders::surge::SurgeBuilder;
use crate::config::subconverter::{SubconverterOptions, generate_subconverter_config};
use crate::config::{predefined_rule_set, sing_box_config, sing_box_config_v1_11};
use crate::fetch::Fetcher;
use crate::hono::{self, HeaderMap, Request, Response};
use crate::i18n::Translator;
use crate::js::base64::encode_base64;
use crate::js::string::{is_js_whitespace, js_trim};
use crate::js::{ErrorKind, JsError, JsResult, Value, json};
use crate::services;
use crate::storage::Store;
use crate::utils::try_decode_subscription_lines;

const DEFAULT_USER_AGENT: &str = "curl/7.74.0";
static FAVICON: &[u8] = include_bytes!("../assets/web/favicon.ico");

pub struct App {
    pub store: Store,
    pub fetcher: Arc<dyn Fetcher>,
    pub config_ttl_seconds: Option<f64>,
    pub short_link_ttl_seconds: Option<f64>,
}

impl App {
    pub fn new(store: Store, fetcher: Arc<dyn Fetcher>) -> App {
        App {
            store,
            fetcher,
            config_ttl_seconds: Some(crate::settings::DEFAULT_CONFIG_TTL_SECONDS),
            short_link_ttl_seconds: None,
        }
    }

    /// `app.fetch(request)`
    pub async fn handle(&self, req: &Request) -> Response {
        let is_head = req.method == "HEAD";
        let method = if is_head { "GET" } else { req.method.as_str() };
        let path = hono::get_path(&req.url);
        let lang = req
            .query("lang")
            .filter(|l| !l.is_empty())
            .or_else(|| req.header("Accept-Language").map(|h| h.split(',').next().unwrap_or("").to_string()))
            .filter(|l| !l.is_empty())
            .unwrap_or_else(|| "zh-CN".into());
        let ctx = Ctx { app: self, req, t: Translator::new(Some(&lang)), lang };

        let result = match (method, path.as_str()) {
            ("GET", "/") => Ok(hono::html(crate::pages::index(&ctx.lang))),
            ("GET", "/singbox") => Ok(ctx.singbox().await),
            ("GET", "/clash") => Ok(ctx.clash().await),
            ("GET", "/surge") => Ok(ctx.surge().await),
            ("GET", "/subconverter") => Ok(ctx.subconverter()),
            ("GET", "/xray") => Ok(ctx.xray().await),
            ("GET", "/shorten-v2") => Ok(ctx.shorten()),
            ("GET", "/resolve") => Ok(ctx.resolve()),
            ("GET", "/favicon.ico") => Ok(favicon(&req.url)),
            ("POST", "/config") => Ok(ctx.save_config()),
            ("GET", p) => match short_link_route(p) {
                Some((target, code)) => Ok(ctx.redirect(target, &code)),
                None => Err(()),
            },
            _ => Err(()),
        };
        let mut res = result.unwrap_or_else(|()| hono::not_found());
        if is_head {
            res.body.clear();
        }
        res
    }
}

/// `/s/:code`, `/b/:code`, `/c/:code`, `/x/:code`
fn short_link_route(path: &str) -> Option<(&'static str, String)> {
    let rest = path.strip_prefix('/')?;
    let (prefix, code) = rest.split_once('/')?;
    if code.is_empty() || code.contains('/') {
        return None;
    }
    Some((short_link_target(prefix)?, hono::decode_param(code)))
}

fn short_link_target(prefix: &str) -> Option<&'static str> {
    match prefix {
        "b" => Some("singbox"),
        "c" => Some("clash"),
        "x" => Some("xray"),
        "s" => Some("surge"),
        _ => None,
    }
}

fn favicon(url: &str) -> Response {
    // The original served files by the raw pathname, so an encoded spelling
    // that still routes here missed the file.
    let raw_path = url::Url::parse(url).map(|u| u.path().to_string()).unwrap_or_default();
    if raw_path != "/favicon.ico" {
        return Response::new(404, vec![("content-type".into(), hono::TEXT_PLAIN_FAST.into())], "Not found");
    }
    Response::new(
        200,
        vec![("cache-control".into(), "public, max-age=86400".into()), ("content-type".into(), "image/x-icon".into())],
        FAVICON,
    )
}

fn handle_error(error: JsError) -> Response {
    match error.kind {
        ErrorKind::Service(status) => hono::text(error.message, status, HeaderMap::default()),
        _ => {
            eprintln!("Unhandled error {:?}: {}", error.kind, error.message);
            hono::text(format!("Error: {}", error.message), 500, HeaderMap::default())
        }
    }
}

fn missing_config() -> Response {
    hono::text("Missing config parameter", 400, HeaderMap::default())
}

fn userinfo_headers(userinfo: Option<String>) -> HeaderMap {
    let mut headers = HeaderMap::default();
    if let Some(info) = userinfo.filter(|i| !i.is_empty()) {
        headers.set("subscription-userinfo", info);
    }
    headers
}

struct Ctx<'a> {
    app: &'a App,
    req: &'a Request,
    lang: String,
    t: Translator,
}

impl Ctx<'_> {
    fn query(&self, key: &str) -> Option<String> {
        self.req.query(key)
    }

    /// `c.req.query(key) || fallback`
    fn query_truthy(&self, key: &str) -> Option<String> {
        self.query(key).filter(|v| !v.is_empty())
    }

    fn user_agent(&self) -> String {
        self.query_truthy("ua")
            .or_else(|| self.req.header("User-Agent").filter(|v| !v.is_empty()))
            .unwrap_or_else(|| DEFAULT_USER_AGENT.into())
    }

    /// Options shared by the three builder routes.
    fn build_options(&self, input: String) -> BuildOptions {
        BuildOptions {
            input,
            selected_rules: parse_selected_rules(self.query("selectedRules").as_deref()),
            custom_rules: parse_json_array(self.query("customRules").as_deref()),
            lang: Some(self.lang.clone()),
            user_agent: self.user_agent(),
            group_by_country: self.query("group_by_country").as_deref() == Some("true"),
            include_auto_select: self.query("include_auto_select").as_deref() != Some("false"),
            enable_clash_ui: self.query("enable_clash_ui").as_deref() == Some("true"),
            external_controller: self.query("external_controller"),
            external_ui_download_url: self.query("external_ui_download_url"),
            ..BuildOptions::default()
        }
    }

    /// Stored base config when `configId` carries the route's prefix.
    fn stored_base_config(&self, prefix: &str) -> JsResult<Value> {
        match self.query("configId") {
            Some(id) if id.starts_with(prefix) => services::get_config_by_id(&self.app.store, &id),
            _ => Ok(Value::Undefined),
        }
    }

    async fn singbox(&self) -> Response {
        let Some(config) = self.query_truthy("config") else { return missing_config() };
        let result: JsResult<Response> = async {
            let mut opts = self.build_options(config);
            let requested = self
                .query_truthy("singbox_version")
                .or_else(|| self.query_truthy("sb_version"))
                .or_else(|| self.query_truthy("sb_ver"));
            let version =
                resolve_singbox_config_version(requested.as_deref(), self.req.header("User-Agent").as_deref());
            let mut base = if version == "1.11" { sing_box_config_v1_11().clone() } else { sing_box_config().clone() };
            let stored = self.stored_base_config("singbox_")?;
            if stored.truthy() {
                base = stored;
            }
            opts.base_config = base;
            opts.singbox_version = version.into();
            let mut builder = SingboxBuilder::new(&opts)?;
            let config = builder.build(self.app.fetcher.as_ref()).await?;
            let body = json::stringify(&config).unwrap_or_default();
            Ok(hono::json(body, userinfo_headers(builder.subscription_userinfo())))
        }
        .await;
        result.unwrap_or_else(handle_error)
    }

    async fn clash(&self) -> Response {
        let Some(config) = self.query_truthy("config") else { return missing_config() };
        let result: JsResult<Response> = async {
            let mut opts = self.build_options(config);
            opts.base_config = self.stored_base_config("clash_")?;
            let mut builder = ClashBuilder::new(&opts);
            builder.build(self.app.fetcher.as_ref()).await?;
            let mut headers = HeaderMap::default();
            headers.set("content-type", "text/yaml; charset=utf-8");
            if let Some(info) = builder.subscription_userinfo().filter(|i| !i.is_empty()) {
                headers.set("subscription-userinfo", info);
            }
            // The original route formats a second time, after build() already did.
            let body = builder.format_config()?;
            Ok(hono::text(body, 200, headers))
        }
        .await;
        result.unwrap_or_else(handle_error)
    }

    async fn surge(&self) -> Response {
        let Some(config) = self.query_truthy("config") else { return missing_config() };
        let mut opts = self.build_options(config);
        opts.enable_clash_ui = false;
        opts.external_controller = None;
        opts.external_ui_download_url = None;
        opts.base_config = match self.stored_base_config("surge_") {
            Ok(base) => base,
            Err(e) => return handle_error(e),
        };
        let mut builder = SurgeBuilder::new(&opts);
        builder.set_subscription_url(&self.req.url);
        if let Err(e) = builder.build(self.app.fetcher.as_ref()).await {
            return handle_error(e);
        }
        // The userinfo header is set on the context before the second
        // formatConfig(), so even its error response carries it.
        let headers = userinfo_headers(builder.subscription_userinfo());
        match builder.format_config() {
            Ok(body) if headers.is_empty() => hono::text_fast(body),
            Ok(body) => hono::text(body, 200, headers),
            Err(e) => {
                let mut res = handle_error(e);
                if let Some(info) = builder.subscription_userinfo().filter(|i| !i.is_empty()) {
                    res.headers.insert(0, ("subscription-userinfo".into(), info));
                }
                res
            }
        }
    }

    fn subconverter(&self) -> Response {
        let raw = self.query_truthy("selectedRules");
        let selected_rules = match raw.as_deref() {
            None => predefined_rule_set("balanced").expect("balanced preset"),
            Some(raw) => match predefined_rule_set(raw) {
                Some(preset) => preset,
                None => match json::parse(raw) {
                    Ok(parsed) if parsed.is_array() => parsed,
                    Ok(_) => {
                        return hono::text(
                            "Invalid selectedRules: must be a preset name (minimal, balanced, comprehensive) or a JSON array",
                            400,
                            HeaderMap::default(),
                        );
                    }
                    Err(_) => {
                        return hono::text(
                            format!(
                                "Invalid selectedRules: \"{raw}\" is not a valid preset name or JSON array. Valid presets: minimal, balanced, comprehensive"
                            ),
                            400,
                            HeaderMap::default(),
                        );
                    }
                },
            },
        };
        let mut custom_rules = parse_json_array(self.query("customRules").as_deref());
        let generated = generate_subconverter_config(SubconverterOptions {
            selected_rules,
            custom_rules: &mut custom_rules,
            lang: Some(&self.lang),
            include_auto_select: self.query("include_auto_select").as_deref() != Some("false"),
            group_by_country: self.query("group_by_country").as_deref() == Some("true"),
        });
        match generated {
            Ok(config) => {
                let mut headers = HeaderMap::default();
                headers.set("content-type", "text/plain; charset=utf-8");
                hono::text(config, 200, headers)
            }
            Err(e) => handle_error(e),
        }
    }

    async fn xray(&self) -> Response {
        let Some(input) = self.query_truthy("config") else { return missing_config() };
        let user_agent = self.user_agent();
        let mut lines: Vec<String> = Vec::new();
        let mut userinfo: Option<String> = None;
        let keep = |items: Vec<String>, out: &mut Vec<String>| {
            out.extend(items.into_iter().filter(|s| !js_trim(s).is_empty()));
        };

        for proxy in input.split('\n') {
            let trimmed = js_trim(proxy);
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
                match self.app.fetcher.get(trimmed, Some(&user_agent)).await {
                    Ok(response) => {
                        if userinfo.is_none() {
                            userinfo = response.header("subscription-userinfo").filter(|v| !v.is_empty());
                        }
                        keep(try_decode_subscription_lines(&response.text(), true).into_vec(), &mut lines);
                    }
                    Err(e) => eprintln!("Failed to fetch the proxy {e}"),
                }
            } else {
                keep(try_decode_subscription_lines(trimmed, false).into_vec(), &mut lines);
            }
        }

        let joined = lines.join("\n");
        if joined.is_empty() {
            return missing_config();
        }
        hono::text(encode_base64(&joined), 200, userinfo_headers(userinfo))
    }

    fn shorten(&self) -> Response {
        let Some(url) = self.query_truthy("url") else {
            return hono::text("Missing URL parameter", 400, HeaderMap::default());
        };
        let Ok(parsed) = url::Url::parse(&url) else {
            return hono::text("Invalid URL parameter", 400, HeaderMap::default());
        };
        let search = parsed.query().filter(|q| !q.is_empty()).map(|q| format!("?{q}")).unwrap_or_default();
        let code = self.query("shortCode");
        match services::create_short_link(&self.app.store, &search, code.as_deref(), self.app.short_link_ttl_seconds) {
            Ok(code) => hono::text_fast(code),
            Err(e) => handle_error(e),
        }
    }

    fn redirect(&self, target: &str, code: &str) -> Response {
        match services::resolve_short_code(&self.app.store, code) {
            Ok(Some(param)) if !param.is_empty() => {
                let origin =
                    url::Url::parse(&self.req.url).map(|u| u.origin().ascii_serialization()).unwrap_or_default();
                hono::redirect(&format!("{origin}/{target}{param}"))
            }
            Ok(_) => hono::text("Short URL not found", 404, HeaderMap::default()),
            Err(e) => handle_error(e),
        }
    }

    fn save_config(&self) -> Response {
        let body = crate::js::base64::utf8_decode(&self.req.body);
        let parsed = match json::parse(&body) {
            Ok(v) => v,
            Err(e) => return hono::text(format!("Invalid format: {}", e.message), 400, HeaderMap::default()),
        };
        if parsed.is_null() {
            return handle_error(JsError::type_error(
                "Cannot destructure property 'type' of '(intermediate value)' as it is null.",
            ));
        }
        let kind = parsed.get_computed("type");
        let content = parsed.get_computed("content");
        match services::save_config(&self.app.store, &kind, &content, self.app.config_ttl_seconds) {
            Ok(id) => hono::text_fast(id),
            Err(e) if e.kind == ErrorKind::SyntaxError => {
                hono::text(format!("Invalid format: {}", e.message), 400, HeaderMap::default())
            }
            Err(e) => handle_error(e),
        }
    }

    fn resolve(&self) -> Response {
        let bad_request = |key: &str| hono::text(self.t.ts(key), 400, HeaderMap::default());
        let Some(short_url) = self.query_truthy("url") else { return bad_request("missingUrl") };
        let Ok(url) = url::Url::parse(&short_url) else { return bad_request("invalidShortUrl") };
        let parts: Vec<&str> = url.path().split('/').collect();
        if parts.len() < 3 {
            return bad_request("invalidShortUrl");
        }
        let Some(target) = short_link_target(parts[1]) else { return bad_request("invalidShortUrl") };
        match services::resolve_short_code(&self.app.store, parts[2]) {
            Ok(Some(param)) if !param.is_empty() => {
                let original = format!("{}/{}{}", url.origin().ascii_serialization(), target, param);
                let mut body = String::from("{\"originalUrl\":");
                json::quote_json_string(&original, &mut body);
                body.push('}');
                hono::json(body, HeaderMap::default())
            }
            Ok(_) => hono::text(self.t.ts("shortUrlNotFound"), 404, HeaderMap::default()),
            Err(e) => handle_error(e),
        }
    }
}

/// `parseSelectedRules(raw)`: preset name, JSON array, or the minimal preset
/// when the JSON is malformed.
pub fn parse_selected_rules(raw: Option<&str>) -> Value {
    let Some(raw) = raw.filter(|r| !r.is_empty()) else { return Value::array(Vec::new()) };
    if let Some(preset) = predefined_rule_set(raw) {
        return preset;
    }
    match json::parse(raw) {
        Ok(parsed) if parsed.is_array() => parsed,
        Ok(_) => Value::array(Vec::new()),
        Err(_) => {
            eprintln!("Failed to parse selectedRules: {raw}, falling back to minimal");
            predefined_rule_set("minimal").expect("minimal preset")
        }
    }
}

/// `parseJsonArray(raw)`
fn parse_json_array(raw: Option<&str>) -> Vec<Value> {
    match raw.filter(|r| !r.is_empty()).map(json::parse) {
        Some(Ok(Value::Array(items))) => items.to_vec(),
        _ => Vec::new(),
    }
}

struct Semver {
    major: f64,
    minor: f64,
}

/// First `(\d+)\.(\d+)(?:\.(\d+))?` match; the patch part never matters.
fn parse_semver_like(value: &str) -> Option<Semver> {
    let s = js_trim(value).as_bytes();
    let digits = |from: usize| s[from..].iter().take_while(|b| b.is_ascii_digit()).count();
    for start in 0..s.len() {
        let major_len = digits(start);
        if major_len == 0 || s.get(start + major_len) != Some(&b'.') {
            continue;
        }
        let minor_start = start + major_len + 1;
        let minor_len = digits(minor_start);
        if minor_len == 0 {
            continue;
        }
        let num = |a: usize, b: usize| std::str::from_utf8(&s[a..b]).unwrap().parse::<f64>().unwrap_or(f64::INFINITY);
        return Some(Semver { major: num(start, start + major_len), minor: num(minor_start, minor_start + minor_len) });
    }
    None
}

fn resolve_singbox_config_tier(v: &Semver) -> &'static str {
    let legacy = if v.major != 1.0 { v.major < 1.0 } else { v.minor < 12.0 };
    if legacy {
        return "1.11";
    }
    let modern = if v.major != 1.0 { v.major > 1.0 } else { v.minor >= 14.0 };
    if modern { "1.14" } else { "1.12" }
}

/// Version from `sing-box/X.Y[.Z]` or `sing-box X.Y[.Z]` (ASCII case-insensitive).
fn singbox_version_from_ua(ua: &str) -> Option<String> {
    let find = |slash: bool| -> Option<String> {
        let lower = ua.to_ascii_lowercase();
        let mut from = 0;
        while let Some(pos) = lower[from..].find("sing-box").map(|p| p + from) {
            from = pos + 1;
            let rest = &ua[pos + "sing-box".len()..];
            let after = if slash {
                rest.strip_prefix('/')
            } else {
                let trimmed = rest.trim_start_matches(is_js_whitespace);
                (trimmed.len() < rest.len()).then_some(trimmed)
            };
            let Some(after) = after else { continue };
            if let Some(version) = leading_version(after) {
                return Some(version);
            }
        }
        None
    };
    find(true).or_else(|| find(false))
}

/// `\d+\.\d+(?:\.\d+)?` anchored at the start of `s`.
fn leading_version(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let digits = |from: usize| b[from..].iter().take_while(|c| c.is_ascii_digit()).count();
    let major = digits(0);
    if major == 0 || b.get(major) != Some(&b'.') {
        return None;
    }
    let minor = digits(major + 1);
    if minor == 0 {
        return None;
    }
    let mut end = major + 1 + minor;
    if b.get(end) == Some(&b'.') {
        let patch = digits(end + 1);
        if patch > 0 {
            end += 1 + patch;
        }
    }
    Some(s[..end].to_string())
}

/// `resolveSingboxConfigVersion(requestedVersion, userAgent)`
pub fn resolve_singbox_config_version(requested: Option<&str>, user_agent: Option<&str>) -> &'static str {
    let normalized = requested.map(|r| js_trim(r).to_lowercase()).unwrap_or_default();
    if !normalized.is_empty() && normalized != "auto" {
        if normalized == "legacy" {
            return "1.11";
        }
        if normalized == "latest" {
            return "1.14";
        }
        if let Some(v) = parse_semver_like(&normalized) {
            return resolve_singbox_config_tier(&v);
        }
    }
    if let Some(ua) = user_agent.filter(|u| !u.is_empty())
        && let Some(v) = singbox_version_from_ua(ua).and_then(|s| parse_semver_like(&s))
    {
        return resolve_singbox_config_tier(&v);
    }
    "1.12"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn singbox_version_resolution() {
        assert_eq!(
            resolve_singbox_config_version(None, Some("SFI/1.12.2 (Build 2; sing-box 1.11.4; language zh_CN)")),
            "1.11"
        );
        assert_eq!(
            resolve_singbox_config_version(None, Some("SFA/1.12.12 (587; sing-box 1.12.12; language zh_Hans_CN)")),
            "1.12"
        );
        assert_eq!(resolve_singbox_config_version(None, Some("Sing-Box/1.14.0")), "1.14");
        assert_eq!(resolve_singbox_config_version(Some(" Legacy "), None), "1.11");
        assert_eq!(resolve_singbox_config_version(Some("latest"), Some("sing-box/1.10")), "1.14");
        assert_eq!(resolve_singbox_config_version(Some("auto"), Some("sing-box/1.10")), "1.11");
        assert_eq!(resolve_singbox_config_version(Some("v2.0"), None), "1.14");
        assert_eq!(resolve_singbox_config_version(Some("0.9"), None), "1.11");
        assert_eq!(resolve_singbox_config_version(Some("junk"), Some("curl/8")), "1.12");
        assert_eq!(resolve_singbox_config_version(None, Some("sing-box/x sing-box 1.13")), "1.12");
    }
}
