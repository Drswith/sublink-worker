//! Config builders: shared state/flow (`BaseConfigBuilder`) and the three
//! client targets.

pub mod clash;
pub mod helpers;
pub mod singbox;
pub mod surge;

use crate::config::{get_outbounds, predefined_rule_set};
use crate::fetch::Fetcher;
use crate::i18n::Translator;
use crate::js::base64::decode_base64;
use crate::js::string::{is_js_whitespace, js_trim};
use crate::js::{ErrorKind, JsError, JsResult, Object, Value, deep_copy};
use crate::parsers::content::{is_config_result, parse_subscription_content};
use crate::parsers::parse_proxy;
use crate::parsers::subscription::{Format, fetch_subscription_with_format};
use crate::utils::{create_stable_provider_name, try_decode_subscription_lines};
use helpers::{ValueSet, dedupe};

/// Options shared by every builder (constructor arguments of the original).
#[derive(Clone, Debug)]
pub struct BuildOptions {
    pub input: String,
    pub selected_rules: Value,
    pub custom_rules: Vec<Value>,
    /// `undefined` means "use the platform default".
    pub base_config: Value,
    pub lang: Option<String>,
    /// Empty string behaves like a missing User-Agent.
    pub user_agent: String,
    pub group_by_country: bool,
    pub include_auto_select: bool,
    /// Clash only: `false` drops the generated `dns` section.
    pub include_clash_dns: bool,
    pub enable_clash_ui: bool,
    pub external_controller: Option<String>,
    pub external_ui_download_url: Option<String>,
    pub singbox_version: String,
}

impl Default for BuildOptions {
    fn default() -> Self {
        BuildOptions {
            input: String::new(),
            selected_rules: Value::array(Vec::new()),
            custom_rules: Vec::new(),
            base_config: Value::Undefined,
            lang: None,
            user_agent: String::new(),
            group_by_country: false,
            include_auto_select: true,
            include_clash_dns: true,
            enable_clash_ui: false,
            external_controller: None,
            external_ui_download_url: None,
            singbox_version: "1.12".into(),
        }
    }
}

impl BuildOptions {
    pub fn new(input: impl Into<String>) -> Self {
        BuildOptions { input: input.into(), ..Default::default() }
    }
}

/// Mutable state shared by all builders (`BaseConfigBuilder` fields).
pub struct Core {
    pub input: String,
    pub config: Value,
    pub custom_rules: Vec<Value>,
    pub selected_rules: Value,
    pub t: Translator,
    pub user_agent: String,
    pub applied_override_keys: Vec<String>,
    pub group_by_country: bool,
    pub include_auto_select: bool,
    pub provider_urls: Vec<String>,
    pub provider_node_names: Vec<String>,
    pub auto_provider_descriptors: Option<Vec<(String, String)>>,
    pub subscription_userinfo: Option<String>,
    pub pending_user_proxy_groups: Option<Vec<Value>>,
    pub country_group_names: Vec<String>,
    pub manual_group_name: Option<String>,
}

impl Core {
    pub fn new(opts: &BuildOptions, base_config: &Value) -> Self {
        Core {
            input: opts.input.clone(),
            config: deep_copy(base_config),
            custom_rules: opts.custom_rules.clone(),
            selected_rules: opts.selected_rules.clone(),
            t: Translator::new(opts.lang.as_deref()),
            user_agent: opts.user_agent.clone(),
            applied_override_keys: Vec::new(),
            group_by_country: opts.group_by_country,
            include_auto_select: opts.include_auto_select,
            provider_urls: Vec::new(),
            provider_node_names: Vec::new(),
            auto_provider_descriptors: None,
            subscription_userinfo: None,
            pending_user_proxy_groups: None,
            country_group_names: Vec::new(),
            manual_group_name: None,
        }
    }

    pub fn ts(&self, key: &str) -> String {
        self.t.ts(key)
    }

    fn note_override(&mut self, key: &str) {
        if !self.applied_override_keys.iter().any(|k| k == key) {
            self.applied_override_keys.push(key.to_string());
        }
    }

    /// `applyConfigOverrides(overrides)`
    pub fn apply_config_overrides(&mut self, overrides: &Value) -> JsResult<()> {
        if !overrides.truthy() || !overrides.is_object_like() {
            return Ok(());
        }
        for (key, value) in overrides.own_entries() {
            if matches!(key.as_str(), "proxies" | "rules" | "rule-providers" | "proxy-groups") {
                continue;
            }
            if value.is_undefined() {
                crate::js::delete_prop(&mut self.config, &key);
            // `typeof value === 'object'`: null takes this branch and throws in the merge, as in the original.
            } else if key == "dns" && value.typeof_() == "object" && !value.is_array() {
                let merged = merge_dns_config(self.config.get("dns"), &value)?;
                crate::js::set_prop(&mut self.config, &key, merged)?;
            } else {
                crate::js::set_prop(&mut self.config, &key, deep_copy(&value))?;
            }
            self.note_override(&key);
        }
        if let Value::Array(groups) = overrides.get("proxy-groups") {
            self.pending_user_proxy_groups.get_or_insert_with(Vec::new).extend(groups.iter().cloned());
        }
        Ok(())
    }

    /// `getOutboundsList()`
    pub fn get_outbounds_list(&self) -> Vec<String> {
        let selected = &self.selected_rules;
        if let Value::String(name) = selected
            && let Some(set) = predefined_rule_set(name)
        {
            return get_outbounds(&set);
        }
        if selected.truthy() && !selected.object_keys().is_empty() {
            return get_outbounds(selected);
        }
        get_outbounds(&predefined_rule_set("minimal").unwrap())
    }

    /// `getAutoProviderDescriptors(reservedNames)` → (name, url) pairs, cached
    /// after the first call.
    pub fn get_auto_provider_descriptors(&mut self, reserved: &[Value]) -> JsResult<Vec<(String, String)>> {
        if let Some(d) = &self.auto_provider_descriptors {
            return Ok(d.clone());
        }
        let mut used = ValueSet::from_values(reserved);
        let mut by_url: Vec<String> = Vec::new();
        let mut descriptors = Vec::new();
        for url in &self.provider_urls {
            let normalized = js_trim(url).to_string();
            if normalized.is_empty() {
                return Err(JsError::error("Provider URL must be a non-empty string"));
            }
            if by_url.contains(&normalized) {
                continue;
            }
            let base = create_stable_provider_name(&normalized)?;
            let mut name = base.clone();
            let mut suffix = 2;
            while used.has(&Value::str(&name)) {
                name = format!("{}_{}", base, suffix);
                suffix += 1;
            }
            used.add(Value::str(&name));
            by_url.push(normalized.clone());
            descriptors.push((name, normalized));
        }
        self.auto_provider_descriptors = Some(descriptors.clone());
        Ok(descriptors)
    }

    /// `collectProviderNodeNames(content)`
    fn collect_provider_node_names(&mut self, content: &str) {
        let result = parse_subscription_content(content);
        if let Value::Array(proxies) = result.get("proxies") {
            for proxy in proxies {
                let name = proxy.get("tag").clone().or_nullish(|| proxy.get("name").clone());
                if let Value::String(s) = name {
                    let trimmed = js_trim(&s);
                    if !trimmed.is_empty() {
                        self.provider_node_names.push(trimmed.to_string());
                    }
                }
            }
        }
    }
}

/// `mergeDnsConfig(existing, incoming)`
pub fn merge_dns_config(existing: &Value, incoming: &Value) -> JsResult<Value> {
    if !existing.truthy() || !existing.is_object_like() {
        return Ok(deep_copy(incoming));
    }
    if incoming.is_nullish() {
        return Err(JsError::type_error("Cannot convert undefined or null to object"));
    }
    let mut result = deep_copy(existing);
    for (key, value) in incoming.own_entries() {
        let merged = if matches!(key.as_str(), "nameserver" | "fallback" | "fake-ip-filter") && value.is_array() {
            match result.get(&key) {
                Value::Array(current) => {
                    let mut all = current.to_vec();
                    all.extend(value.as_array().unwrap().iter().cloned());
                    Value::array(dedupe(all))
                }
                _ => deep_copy(&value),
            }
        } else if key == "nameserver-policy" && value.typeof_() == "object" && !value.is_array() {
            let current = result.get(&key).clone().or_falsy(|| Value::Object(Object::new()));
            Value::Object(helpers::merge_objects(&current, &deep_copy(&value)))
        } else {
            deep_copy(&value)
        };
        crate::js::set_prop(&mut result, &key, merged)?;
    }
    Ok(result)
}

/// Target-specific behavior of a builder (the methods subclasses override).
pub trait ConfigBuilder: Send {
    fn core(&self) -> &Core;
    fn core_mut(&mut self) -> &mut Core;

    /// Whether a fetched subscription of `format` can be referenced as a provider.
    fn is_compatible_provider_format(&self, _format: Format) -> bool {
        false
    }

    fn get_proxies(&self) -> JsResult<Vec<Value>>;
    fn get_proxy_name(&self, proxy: &Value) -> JsResult<Value>;

    fn get_proxy_list(&self) -> JsResult<Vec<Value>> {
        self.get_proxies()?.iter().map(|p| self.get_proxy_name(p)).collect()
    }

    fn convert_proxy(&self, proxy: &Value) -> JsResult<Value>;

    /// `addCustomItems(items)`: convert and add every tagged item.
    fn add_custom_items(&mut self, items: Vec<Value>) -> JsResult<()>;

    fn add_auto_select_group(&mut self, proxy_list: &[Value]) -> JsResult<()>;
    fn add_node_select_group(&mut self, proxy_list: &[Value]) -> JsResult<()>;
    fn add_country_groups(&mut self) -> JsResult<()>;
    fn add_outbound_groups(&mut self, outbounds: &[String], proxy_list: &[Value]) -> JsResult<()>;
    fn add_custom_rule_groups(&mut self, proxy_list: &[Value]) -> JsResult<()>;
    fn add_fall_back_group(&mut self, proxy_list: &[Value]) -> JsResult<()>;

    fn merge_user_proxy_groups(&mut self, _groups: &[Value]) -> JsResult<()> {
        Ok(())
    }

    /// `addSelectors()`
    fn add_selectors(&mut self) -> JsResult<()> {
        let outbounds = self.core().get_outbounds_list();
        let proxy_list = self.get_proxy_list()?;
        self.add_auto_select_group(&proxy_list)?;
        self.add_node_select_group(&proxy_list)?;
        if self.core().group_by_country {
            self.add_country_groups()?;
        }
        self.add_outbound_groups(&outbounds, &proxy_list)?;
        self.add_custom_rule_groups(&proxy_list)?;
        self.add_fall_back_group(&proxy_list)?;
        if let Some(groups) = self.core().pending_user_proxy_groups.clone()
            && !groups.is_empty()
        {
            self.merge_user_proxy_groups(&groups)?;
        }
        Ok(())
    }
}

fn push_tagged(items: &mut Vec<Value>, proxies: &Value, require_object: bool) {
    if let Value::Array(list) = proxies {
        for proxy in list {
            if proxy.truthy() && (!require_object || proxy.is_object_like()) && proxy.get("tag").truthy() {
                items.push(proxy.clone());
            }
        }
    }
}

/// `/^[A-Za-z0-9+/=\r\n]+$/.test(input) && input.replace(/[\r\n]/g, '').length % 4 === 0`
fn is_base64_like(input: &str) -> bool {
    !input.is_empty()
        && input.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '\r' | '\n'))
        && input.chars().filter(|c| !matches!(c, '\r' | '\n')).count() % 4 == 0
}

/// Handles one entry of a fetched URI list: objects with a tag are kept,
/// strings are parsed again. Errors abort the rest of that subscription.
async fn push_list_items(
    items: &mut Vec<Value>,
    list: &[Value],
    fetcher: &dyn Fetcher,
    user_agent: &str,
) -> JsResult<()> {
    for item in list {
        if item.truthy() && item.is_object_like() && item.get("tag").truthy() {
            items.push(item.clone());
        } else if let Value::String(s) = item {
            let sub = Box::pin(parse_proxy(fetcher, s, user_agent)).await?;
            if sub.truthy() {
                items.push(sub);
            }
        }
    }
    Ok(())
}

/// `parseCustomItems()`: turns the raw input into proxy objects, applying
/// config overrides and collecting provider URLs on the way.
pub async fn parse_custom_items<B: ConfigBuilder + ?Sized>(b: &mut B, fetcher: &dyn Fetcher) -> JsResult<Vec<Value>> {
    let input = b.core().input.clone();
    let mut items: Vec<Value> = Vec::new();

    let direct = parse_subscription_content(&input);
    if direct.is_plain_object() && direct.get("type").truthy() {
        let config = direct.get("config").clone();
        if config.truthy() {
            b.core_mut().apply_config_overrides(&config)?;
        }
        if direct.get("proxies").is_array() {
            push_tagged(&mut items, direct.get("proxies"), false);
            if !items.is_empty() {
                return Ok(items);
            }
        }
    }

    if is_base64_like(&input) {
        let sanitized: String = input.chars().filter(|c| !is_js_whitespace(*c)).collect();
        let decoded = parse_subscription_content(&decode_base64(&sanitized));
        if decoded.is_plain_object() && decoded.get("type").truthy() {
            let config = decoded.get("config").clone();
            // The original wrapped this block in an empty catch.
            let applied = if config.truthy() { b.core_mut().apply_config_overrides(&config) } else { Ok(()) };
            if applied.is_ok() && decoded.get("proxies").is_array() {
                push_tagged(&mut items, decoded.get("proxies"), false);
                if !items.is_empty() {
                    return Ok(items);
                }
            }
        }
    }

    let user_agent = b.core().user_agent.clone();
    for line in input.split('\n').filter(|l| !js_trim(l).is_empty()) {
        for processed in try_decode_subscription_lines(line, false).into_vec() {
            let trimmed = js_trim(&processed).to_string();
            if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
                let outcome: JsResult<()> = async {
                    let Some(fetched) = fetch_subscription_with_format(fetcher, &trimmed, &user_agent).await else {
                        return Err(JsError::service(
                            502,
                            "Unable to fetch upstream subscription; check its URL and server connectivity",
                        ));
                    };
                    if let Some(info) = &fetched.subscription_userinfo
                        && b.core().subscription_userinfo.is_none()
                    {
                        b.core_mut().subscription_userinfo = Some(info.clone());
                    }
                    if b.is_compatible_provider_format(fetched.format) {
                        b.core_mut().provider_urls.push(fetched.url.clone());
                        b.core_mut().collect_provider_node_names(&fetched.content);
                        return Ok(());
                    }
                    let result = parse_subscription_content(&fetched.content);
                    if is_config_result(&result) {
                        let config = result.get("config").clone();
                        if config.truthy() {
                            b.core_mut().apply_config_overrides(&config)?;
                        }
                        push_tagged(&mut items, result.get("proxies"), true);
                        return Ok(());
                    }
                    if let Value::Array(list) = &result {
                        push_list_items(&mut items, list, fetcher, &user_agent).await?;
                    }
                    Ok(())
                }
                .await;
                // A failed upstream must not degrade into a config without its nodes.
                match outcome {
                    Err(e) if matches!(e.kind, ErrorKind::Service(_)) => return Err(e),
                    Err(_) => return Err(JsError::service(502, "Unable to process upstream subscription")),
                    Ok(()) => {}
                }
                continue;
            }

            let result = parse_proxy(fetcher, &processed, &user_agent).await?;
            if is_config_result(&result) {
                let config = result.get("config").clone();
                if config.truthy() {
                    b.core_mut().apply_config_overrides(&config)?;
                }
                push_tagged(&mut items, result.get("proxies"), true);
                continue;
            }
            if let Value::Array(list) = &result {
                push_list_items(&mut items, list, fetcher, &user_agent).await?;
            } else if result.truthy() {
                items.push(result);
            }
        }
    }
    Ok(items)
}

/// `build()` up to (but excluding) `formatConfig()`.
pub async fn prepare<B: ConfigBuilder + ?Sized>(b: &mut B, fetcher: &dyn Fetcher) -> JsResult<()> {
    let items = parse_custom_items(b, fetcher).await?;
    b.add_custom_items(items)?;
    b.add_selectors()
}

/// Typed accessor for array-valued config paths with JS failure messages.
pub(crate) fn expect_array<'a>(v: &'a Value, expr: &str, method: &str) -> JsResult<&'a Vec<Value>> {
    match v {
        Value::Array(a) => Ok(a),
        Value::Undefined | Value::Null => Err(JsError::read_prop(v, method)),
        _ => Err(JsError::not_function(&format!("{}.{}", expr, method))),
    }
}

pub(crate) fn expect_array_mut<'a>(v: &'a mut Value, expr: &str, method: &str) -> JsResult<&'a mut Vec<Value>> {
    match v {
        Value::Array(a) => Ok(a),
        Value::Undefined | Value::Null => Err(JsError::read_prop(v, method)),
        _ => Err(JsError::not_function(&format!("{}.{}", expr, method))),
    }
}
