//! `ClashConfigBuilder`

use super::helpers::{
    DedupIndex, MemberOptions, ValueSet, build_custom_rule_members, build_node_select_members, build_selector_members,
    dedupe, emit_clash_rules, normalize_group_name, sanitize_clash_proxy_groups, stringify_without, unique_names,
};
use super::singbox::spread_iterable;
use super::{BuildOptions, ConfigBuilder, Core, expect_array, prepare};
use crate::config::{clash_config, generate_clash_rule_sets, generate_rules, is_direct_default_rule};
use crate::fetch::Fetcher;
use crate::js::{JsError, JsResult, Object, Value, set_prop, strict_equals};
use crate::obj;
use crate::parsers::subscription::Format;
use crate::utils::{COUNTRY_DATA, build_country_name_filter, group_proxies_by_country};

/// `supportsMrsFormat(userAgent)`
fn supports_mrs_format(user_agent: &str) -> bool {
    if user_agent.is_empty() {
        return true;
    }
    let ua = user_agent.to_lowercase();
    if ["mihomo", "meta", "clash-verge", "stash", "verge"].iter().any(|k| ua.contains(k)) {
        return true;
    }
    if ["merlin", "clashforwindows", "clashforandroid", "clash/"].iter().any(|k| ua.contains(k)) {
        return false;
    }
    true
}

/// `getClashUdpValue(proxy)`
fn clash_udp(proxy: &Value) -> Value {
    let udp = proxy.get("udp");
    if udp.is_undefined() { Value::Bool(true) } else { udp.clone() }
}

fn or_false(v: &Value) -> Value {
    v.clone().or_falsy(|| Value::Bool(false))
}

fn or_empty_str(v: &Value) -> Value {
    v.clone().or_falsy(|| Value::str(""))
}

fn nullish_to_undefined(v: &Value) -> Value {
    if v.is_nullish() { Value::Undefined } else { v.clone() }
}

fn transport_opts(proxy: &Value, kind: &str) -> Value {
    let transport = proxy.get("transport");
    if transport.get("type").as_str() != Some(kind) {
        return Value::Undefined;
    }
    match kind {
        "ws" => obj! { "path" => transport.get("path"), "headers" => transport.get("headers") },
        "grpc" => obj! { "grpc-service-name" => transport.get("service_name") },
        "h2" => obj! { "path" => transport.get("path"), "host" => transport.get("host") },
        _ => Value::Undefined,
    }
}

fn reality_opts(proxy: &Value) -> Value {
    let reality = proxy.get("tls").get("reality");
    if reality.get("enabled").truthy() {
        obj! { "public-key" => reality.get("public_key"), "short-id" => reality.get("short_id") }
    } else {
        Value::Undefined
    }
}

/// `ClashConfigBuilder.prototype.convertProxy` (stateless).
pub fn convert_proxy(proxy: &Value) -> JsResult<Value> {
    let tls = proxy.get("tls");
    let transport = proxy.get("transport");
    let name = proxy.get("tag").clone();
    Ok(match proxy.get("type").as_str() {
        Some("shadowsocks") => {
            let mut o = Object::new();
            o.set("name", name);
            o.set("type", Value::str("ss"));
            o.set("server", proxy.get("server").clone());
            o.set("port", proxy.get("server_port").clone());
            o.set("cipher", proxy.get("method").clone());
            o.set("password", proxy.get("password").clone());
            o.set("udp", clash_udp(proxy));
            if proxy.get("plugin").truthy() {
                o.set("plugin", proxy.get("plugin").clone());
            }
            if proxy.get("plugin_opts").truthy() {
                o.set("plugin-opts", proxy.get("plugin_opts").clone());
            }
            Value::Object(o)
        }
        Some("vmess") => {
            let http_opts = if transport.get("type").as_str() == Some("http") {
                let path = transport.get("path");
                let path = if path.is_array() {
                    path.clone()
                } else {
                    Value::array(vec![path.clone().or_falsy(|| Value::str("/"))])
                };
                let mut opts = Object::new();
                opts.set("method", transport.get("method").clone().or_falsy(|| Value::str("GET")));
                opts.set("path", path);
                let headers = transport.get("headers");
                if headers.truthy() && !headers.object_keys().is_empty() {
                    opts.set("headers", headers.clone());
                }
                Value::Object(opts)
            } else {
                Value::Undefined
            };
            obj! {
                "name" => name,
                "type" => proxy.get("type"),
                "server" => proxy.get("server"),
                "port" => proxy.get("server_port"),
                "uuid" => proxy.get("uuid"),
                "alterId" => proxy.get("alter_id").clone().or_nullish(|| Value::Number(0.0)),
                "cipher" => proxy.get("security"),
                "tls" => or_false(tls.get("enabled")),
                "servername" => or_empty_str(tls.get("server_name")),
                "skip-cert-verify" => tls.get("insecure").truthy(),
                "network" => transport.get("type").clone().or_falsy(|| proxy.get("network").clone()).or_falsy(|| Value::str("tcp")),
                "ws-opts" => transport_opts(proxy, "ws"),
                "http-opts" => http_opts,
                "grpc-opts" => transport_opts(proxy, "grpc"),
                "h2-opts" => transport_opts(proxy, "h2"),
                "udp" => clash_udp(proxy),
            }
        }
        Some("vless") => {
            let mut o = Object::new();
            o.set("name", name);
            o.set("type", proxy.get("type").clone());
            o.set("server", proxy.get("server").clone());
            o.set("port", proxy.get("server_port").clone());
            o.set("uuid", proxy.get("uuid").clone());
            o.set("cipher", proxy.get("security").clone());
            o.set("tls", or_false(tls.get("enabled")));
            o.set("client-fingerprint", tls.get("utls").get("fingerprint").clone());
            o.set("servername", or_empty_str(tls.get("server_name")));
            o.set("network", transport.get("type").clone().or_falsy(|| Value::str("tcp")));
            o.set("ws-opts", transport_opts(proxy, "ws"));
            o.set("reality-opts", reality_opts(proxy));
            o.set("grpc-opts", transport_opts(proxy, "grpc"));
            o.set("tfo", proxy.get("tcp_fast_open").clone());
            o.set("skip-cert-verify", Value::Bool(tls.get("insecure").truthy()));
            o.set("udp", clash_udp(proxy));
            if proxy.get("alpn").truthy() {
                o.set("alpn", proxy.get("alpn").clone());
            }
            if proxy.get("packet_encoding").truthy() {
                o.set("packet-encoding", proxy.get("packet_encoding").clone());
            }
            o.set("flow", nullish_to_undefined(proxy.get("flow")));
            Value::Object(o)
        }
        Some("hysteria2") => {
            let mut o = Object::new();
            o.set("name", name);
            o.set("type", proxy.get("type").clone());
            o.set("server", proxy.get("server").clone());
            o.set("port", proxy.get("server_port").clone());
            if proxy.get("ports").truthy() {
                o.set("ports", proxy.get("ports").clone());
            }
            o.set("obfs", proxy.get("obfs").get("type").clone());
            o.set("obfs-password", proxy.get("obfs").get("password").clone());
            o.set("password", proxy.get("password").clone());
            o.set("auth", proxy.get("auth").clone());
            o.set("up", proxy.get("up").clone());
            o.set("down", proxy.get("down").clone());
            o.set("recv-window-conn", proxy.get("recv_window_conn").clone());
            o.set("sni", or_empty_str(tls.get("server_name")));
            o.set("skip-cert-verify", Value::Bool(tls.get("insecure").truthy()));
            if !proxy.get("hop_interval").is_undefined() {
                o.set("hop-interval", proxy.get("hop_interval").clone());
            }
            if proxy.get("alpn").truthy() {
                o.set("alpn", proxy.get("alpn").clone());
            }
            if !proxy.get("fast_open").is_undefined() {
                o.set("fast-open", proxy.get("fast_open").clone());
            }
            Value::Object(o)
        }
        Some("trojan") => {
            let mut o = Object::new();
            o.set("name", name);
            o.set("type", proxy.get("type").clone());
            o.set("server", proxy.get("server").clone());
            o.set("port", proxy.get("server_port").clone());
            o.set("password", proxy.get("password").clone());
            o.set("cipher", proxy.get("security").clone());
            o.set("tls", or_false(tls.get("enabled")));
            o.set("client-fingerprint", tls.get("utls").get("fingerprint").clone());
            o.set("sni", or_empty_str(tls.get("server_name")));
            o.set("network", transport.get("type").clone().or_falsy(|| Value::str("tcp")));
            o.set("ws-opts", transport_opts(proxy, "ws"));
            o.set("reality-opts", reality_opts(proxy));
            o.set("grpc-opts", transport_opts(proxy, "grpc"));
            o.set("tfo", proxy.get("tcp_fast_open").clone());
            o.set("skip-cert-verify", Value::Bool(tls.get("insecure").truthy()));
            if proxy.get("alpn").truthy() {
                o.set("alpn", proxy.get("alpn").clone());
            }
            o.set("flow", nullish_to_undefined(proxy.get("flow")));
            o.set("udp", clash_udp(proxy));
            Value::Object(o)
        }
        Some("tuic") => {
            let mut o = Object::new();
            o.set("name", name);
            o.set("type", proxy.get("type").clone());
            o.set("server", proxy.get("server").clone());
            o.set("port", proxy.get("server_port").clone());
            o.set("uuid", proxy.get("uuid").clone());
            o.set("password", proxy.get("password").clone());
            o.set("congestion-controller", proxy.get("congestion_control").clone());
            o.set("skip-cert-verify", Value::Bool(tls.get("insecure").truthy()));
            if !proxy.get("disable_sni").is_undefined() {
                o.set("disable-sni", proxy.get("disable_sni").clone());
            }
            if tls.get("alpn").truthy() {
                o.set("alpn", tls.get("alpn").clone());
            }
            o.set("sni", tls.get("server_name").clone());
            o.set("udp-relay-mode", proxy.get("udp_relay_mode").clone().or_falsy(|| Value::str("native")));
            for (src, dst) in [("zero_rtt", "zero-rtt"), ("reduce_rtt", "reduce-rtt"), ("fast_open", "fast-open")] {
                if !proxy.get(src).is_undefined() {
                    o.set(dst, proxy.get(src).clone());
                }
            }
            Value::Object(o)
        }
        Some("anytls") => {
            let check = proxy
                .get("idle-session-check-interval")
                .clone()
                .or_nullish(|| proxy.get("idle_session_check_interval").clone());
            let timeout =
                proxy.get("idle-session-timeout").clone().or_nullish(|| proxy.get("idle_session_timeout").clone());
            let min_idle = proxy.get("min-idle-session").clone().or_nullish(|| proxy.get("min_idle_session").clone());
            let mut o = Object::new();
            o.set("name", name);
            o.set("type", Value::str("anytls"));
            o.set("server", proxy.get("server").clone());
            o.set("port", proxy.get("server_port").clone());
            o.set("password", proxy.get("password").clone());
            o.set("udp", clash_udp(proxy));
            if tls.get("utls").get("fingerprint").truthy() {
                o.set("client-fingerprint", tls.get("utls").get("fingerprint").clone());
            }
            if tls.get("server_name").truthy() {
                o.set("sni", tls.get("server_name").clone());
            }
            if !tls.get("insecure").is_undefined() {
                o.set("skip-cert-verify", Value::Bool(tls.get("insecure").truthy()));
            }
            if tls.get("alpn").truthy() {
                o.set("alpn", tls.get("alpn").clone());
            }
            if !check.is_undefined() {
                o.set("idle-session-check-interval", check);
            }
            if !timeout.is_undefined() {
                o.set("idle-session-timeout", timeout);
            }
            if !min_idle.is_undefined() {
                o.set("min-idle-session", min_idle);
            }
            Value::Object(o)
        }
        _ => proxy.clone(),
    })
}

pub struct ClashBuilder {
    pub core: Core,
    pub enable_clash_ui: bool,
    pub external_controller: Option<String>,
    pub external_ui_download_url: Option<String>,
    pub include_dns: bool,
}

impl ClashBuilder {
    pub fn new(opts: &BuildOptions) -> Self {
        let base = if opts.base_config.truthy() { opts.base_config.clone() } else { clash_config().clone() };
        ClashBuilder {
            core: Core::new(opts, &base),
            enable_clash_ui: opts.enable_clash_ui,
            external_controller: opts.external_controller.clone().filter(|s| !s.is_empty()),
            external_ui_download_url: opts.external_ui_download_url.clone().filter(|s| !s.is_empty()),
            include_dns: opts.include_clash_dns,
        }
    }

    fn config(&self) -> &Value {
        &self.core.config
    }

    fn groups(&self) -> &Value {
        self.core.config.get("proxy-groups")
    }

    fn groups_or_empty(&self) -> JsResult<Vec<Value>> {
        let g = self.groups();
        if !g.truthy() {
            return Ok(Vec::new());
        }
        expect_array(g, "(this.config['proxy-groups'] || [])", "some").cloned().map(|a| a.to_vec())
    }

    fn ensure_groups(&mut self) -> JsResult<()> {
        if !self.groups().truthy() {
            set_prop(&mut self.core.config, "proxy-groups", Value::array(Vec::new()))?;
        }
        Ok(())
    }

    fn groups_mut(&mut self, method: &str) -> JsResult<&mut Vec<Value>> {
        let current = self.groups().clone();
        match self.core.config.as_object_mut().and_then(|c| c.get_mut("proxy-groups")) {
            Some(Value::Array(a)) => Ok(&mut *a),
            _ if current.is_nullish() => Err(JsError::read_prop(&current, method)),
            _ => Err(JsError::not_function(&format!("this.config['proxy-groups'].{}", method))),
        }
    }

    fn existing_provider_names(&self) -> Vec<Value> {
        let providers = self.config().get("proxy-providers");
        if providers.truthy() && providers.is_object_like() {
            providers.object_keys().into_iter().map(Value::String).collect()
        } else {
            Vec::new()
        }
    }

    fn provider_names(&mut self) -> JsResult<Vec<Value>> {
        let existing = self.existing_provider_names();
        Ok(self.core.get_auto_provider_descriptors(&existing)?.into_iter().map(|(n, _)| Value::String(n)).collect())
    }

    /// `getAllProviderNames()`
    fn all_provider_names(&mut self) -> JsResult<Vec<Value>> {
        let mut all = self.existing_provider_names();
        all.extend(self.provider_names()?);
        Ok(dedupe(all))
    }

    fn generate_proxy_providers(&mut self) -> JsResult<Object> {
        let existing = self.existing_provider_names();
        let mut providers = Object::new();
        for (name, url) in self.core.get_auto_provider_descriptors(&existing)? {
            providers.set(
                name.clone(),
                obj! {
                    "type" => "http",
                    "url" => url,
                    "path" => format!("./proxy_providers/{}.yaml", name),
                    "interval" => 3600,
                    "health-check" => obj! {
                        "enable" => true,
                        "url" => "https://www.gstatic.com/generate_204",
                        "interval" => 300,
                        "timeout" => 5000,
                        "lazy" => true,
                    },
                },
            );
        }
        Ok(providers)
    }

    fn has_proxy_group(&self, name: &Value) -> JsResult<bool> {
        let target = normalize_group_name(name);
        Ok(self
            .groups_or_empty()?
            .iter()
            .any(|g| g.truthy() && strict_equals(&normalize_group_name(g.get("name")), &target)))
    }

    fn should_include_auto_select(&mut self, proxy_list: &[Value]) -> JsResult<bool> {
        if !self.core.include_auto_select {
            return Ok(false);
        }
        Ok(!unique_names(proxy_list).is_empty() || !self.all_provider_names()?.is_empty())
    }

    fn member_options<'a>(&'a self, proxy_list: &'a [Value], include_auto_select: bool) -> MemberOptions<'a> {
        MemberOptions {
            proxy_list,
            t: &self.core.t,
            group_by_country: self.core.group_by_country,
            manual_group_name: self.core.manual_group_name.as_deref(),
            country_group_names: &self.core.country_group_names,
            include_auto_select,
            include_reject: true,
        }
    }

    fn select_group_members(&mut self, proxy_list: &[Value]) -> JsResult<Vec<String>> {
        let include = self.should_include_auto_select(proxy_list)?;
        Ok(build_selector_members(&self.member_options(proxy_list, include)))
    }

    fn with_providers(&mut self, mut group: Object) -> JsResult<Value> {
        let providers = self.all_provider_names()?;
        if !providers.is_empty() {
            group.set("use", Value::array(providers));
        }
        Ok(Value::Object(group))
    }

    /// `validateProxyGroups()`
    fn validate_proxy_groups(&self) -> JsResult<()> {
        for group in self.groups_or_empty()? {
            let ty = group.get("type");
            if !matches!(ty.as_str(), Some("url-test" | "fallback")) {
                continue;
            }
            let has = |k: &str| group.get(k).as_array().is_some_and(|a| !a.is_empty());
            if has("proxies") || has("use") {
                continue;
            }
            let name = group.get("name").clone().or_falsy(|| Value::str("(unnamed group)"));
            return Err(JsError::service(
                400,
                format!(
                    "Invalid proxy group \"{}\": type \"{}\" requires at least one proxy or provider reference",
                    name.to_js_string(),
                    ty.to_js_string()
                ),
            ));
        }
        Ok(())
    }

    /// `formatConfig()` → YAML text.
    pub fn format_config(&mut self) -> JsResult<String> {
        let rules = generate_rules(&self.core.selected_rules, &mut self.core.custom_rules)?;
        let use_mrs = supports_mrs_format(&self.core.user_agent);
        let (site, ip) = generate_clash_rule_sets(&self.core.selected_rules, &self.core.custom_rules, use_mrs)?;
        let mut providers = site;
        for (k, v) in ip.into_entries() {
            providers.set(k, v);
        }
        set_prop(&mut self.core.config, "rule-providers", Value::Object(providers))?;
        let rule_lines = emit_clash_rules(&rules, &self.core.t);

        if !self.core.provider_urls.is_empty() {
            let mut merged = self.config().get("proxy-providers").spread();
            for (k, v) in self.generate_proxy_providers()?.into_entries() {
                merged.set(k, v);
            }
            set_prop(&mut self.core.config, "proxy-providers", Value::Object(merged))?;
        }

        sanitize_clash_proxy_groups(&mut self.core.config)?;
        self.validate_proxy_groups()?;

        let mut all_rules: Vec<Value> = rule_lines.into_iter().map(Value::String).collect();
        all_rules.push(Value::String(format!("MATCH,{}", self.core.ts("outboundNames.Fall Back"))));
        set_prop(&mut self.core.config, "rules", Value::array(all_rules))?;

        if self.enable_clash_ui || self.external_controller.is_some() || self.external_ui_download_url.is_some() {
            let cfg = self.config().clone();
            let controller = Value::from(self.external_controller.clone())
                .or_falsy(|| cfg.get("external-controller").clone())
                .or_falsy(|| Value::str("0.0.0.0:9090"));
            let ui_path = cfg.get("external-ui").clone().or_falsy(|| Value::str("./ui"));
            let ui_name = cfg.get("external-ui-name").clone().or_falsy(|| Value::str("zashboard"));
            let ui_url = Value::from(self.external_ui_download_url.clone())
                .or_falsy(|| cfg.get("external-ui-url").clone())
                .or_falsy(|| {
                    Value::str(
                        "https://gh-proxy.com/https://github.com/Zephyruso/zashboard/archive/refs/heads/gh-pages.zip",
                    )
                });
            let secret = cfg.get("secret").clone().or_nullish(|| Value::str(""));
            set_prop(&mut self.core.config, "external-controller", controller)?;
            set_prop(&mut self.core.config, "external-ui", ui_path)?;
            set_prop(&mut self.core.config, "external-ui-name", ui_name)?;
            set_prop(&mut self.core.config, "external-ui-url", ui_url)?;
            set_prop(&mut self.core.config, "secret", secret)?;
        }
        if !self.include_dns
            && let Some(config) = self.core.config.as_object_mut()
        {
            config.remove("dns");
        }
        crate::yaml::dump(&self.core.config)
    }

    /// `await builder.build()`
    pub async fn build(&mut self, fetcher: &dyn Fetcher) -> JsResult<String> {
        prepare(self, fetcher).await?;
        self.format_config()
    }

    pub fn subscription_userinfo(&self) -> Option<String> {
        self.core.subscription_userinfo.clone()
    }
}

impl ConfigBuilder for ClashBuilder {
    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn is_compatible_provider_format(&self, format: Format) -> bool {
        format == Format::Clash
    }

    fn get_proxies(&self) -> JsResult<Vec<Value>> {
        let proxies = self.config().get("proxies");
        if !proxies.truthy() {
            return Ok(Vec::new());
        }
        expect_array(proxies, "this.getProxies()", "map").map(|a| a.to_vec())
    }

    fn get_proxy_name(&self, proxy: &Value) -> JsResult<Value> {
        Ok(crate::js::prop(proxy, "name")?.clone())
    }

    fn convert_proxy(&self, proxy: &Value) -> JsResult<Value> {
        convert_proxy(proxy)
    }

    fn add_custom_items(&mut self, items: Vec<Value>) -> JsResult<()> {
        let get_name = |item: &Value| -> JsResult<Value> { Ok(item.get("name").clone()) };
        let set_name = |mut item: Value, name: &str| -> Value {
            let _ = set_prop(&mut item, "name", Value::str(name));
            item
        };
        let same_key = |item: &Value| -> Option<String> { Some(stringify_without(item, "name")) };
        let mut index: Option<DedupIndex> = None;
        for item in items.into_iter().filter(|i| !i.is_nullish()) {
            if !item.get("tag").truthy() {
                continue;
            }
            let converted = self.convert_proxy(&item)?;
            if !converted.truthy() {
                continue;
            }
            if !self.config().get("proxies").truthy() {
                set_prop(&mut self.core.config, "proxies", Value::array(Vec::new()))?;
            }
            let list = match self.core.config.as_object_mut().and_then(|c| c.get_mut("proxies")) {
                Some(Value::Array(a)) => &mut **a,
                _ => return Err(JsError::error("addProxyWithDedup expects the target collection to be an array")),
            };
            if index.is_none() {
                index = Some(DedupIndex::new(list, &get_name, &same_key, Some(("name", "a")))?);
            }
            index.as_mut().unwrap().add(list, converted, &get_name, &set_name, &same_key)?;
        }
        Ok(())
    }

    fn add_auto_select_group(&mut self, proxy_list: &[Value]) -> JsResult<()> {
        if !self.core.include_auto_select {
            return Ok(());
        }
        self.ensure_groups()?;
        let name = self.core.t.t("outboundNames.Auto Select");
        if self.has_proxy_group(&name)? {
            return Ok(());
        }
        let providers = self.all_provider_names()?;
        let members = unique_names(proxy_list);
        if members.is_empty() && providers.is_empty() {
            return Ok(());
        }
        let mut group = Object::new();
        group.set("name", name);
        group.set("type", Value::str("url-test"));
        group.set("proxies", Value::from(members));
        group.set("url", Value::str("https://www.gstatic.com/generate_204"));
        group.set("interval", Value::Number(300.0));
        group.set("lazy", Value::Bool(false));
        if !providers.is_empty() {
            group.set("use", Value::array(providers));
        }
        self.groups_mut("push")?.push(Value::Object(group));
        Ok(())
    }

    fn add_node_select_group(&mut self, proxy_list: &[Value]) -> JsResult<()> {
        self.ensure_groups()?;
        let name = self.core.t.t("outboundNames.Node Select");
        if self.has_proxy_group(&name)? {
            return Ok(());
        }
        let include = self.should_include_auto_select(proxy_list)?;
        let members = build_node_select_members(&self.member_options(proxy_list, include));
        let mut group = Object::new();
        group.set("type", Value::str("select"));
        group.set("name", name);
        group.set("proxies", Value::from(members));
        let group = self.with_providers(group)?;
        self.groups_mut("unshift")?.insert(0, group);
        Ok(())
    }

    fn add_country_groups(&mut self) -> JsResult<()> {
        let proxies = self.get_proxies()?;
        let mut names = Vec::with_capacity(proxies.len());
        for p in &proxies {
            names.push(self.get_proxy_name(p)?);
        }
        let mut groups = group_proxies_by_country(&names, |n| n.clone());
        // One array object shared by every country group, as in the original
        // (js-yaml then prints it once with an anchor).
        let provider_names = Value::array(self.all_provider_names()?);
        let has_providers = provider_names.length().unwrap_or(0) > 0;
        if has_providers && !self.core.provider_node_names.is_empty() {
            let provider_node_names: Vec<Value> = self.core.provider_node_names.iter().map(Value::str).collect();
            let provider_groups = group_proxies_by_country(&provider_node_names, |n| n.clone());
            for (country, g) in provider_groups {
                groups.entry(country).or_insert(crate::utils::CountryGroup { country: g.country, proxies: Vec::new() });
            }
        }
        let mut existing = ValueSet::new();
        for g in self.groups_or_empty()? {
            let n = normalize_group_name(g.get("name"));
            if n.truthy() {
                existing.add(n);
            }
        }
        let manual_names: Vec<Value> = proxies.iter().map(|p| p.get("name").clone()).filter(Value::truthy).collect();
        let manual_group_name =
            if manual_names.is_empty() { None } else { Some(self.core.ts("outboundNames.Manual Switch")) };
        if let Some(name) = &manual_group_name {
            let norm = normalize_group_name(&Value::str(name));
            if !existing.has(&norm) {
                let mut group = Object::new();
                group.set("name", Value::str(name));
                group.set("type", Value::str("select"));
                group.set("proxies", Value::array(manual_names.clone()));
                let group = self.with_providers(group)?;
                self.groups_mut("push")?.push(group);
                existing.add(norm);
            }
        }
        let mut countries: Vec<String> = groups.keys().cloned().collect();
        countries.sort();
        let mut country_group_names = Vec::new();
        for country in countries {
            let g = &groups[&country];
            let group_name = format!("{} {}", g.country.emoji, g.country.name);
            let norm = normalize_group_name(&Value::str(&group_name));
            if !existing.has(&norm) {
                let mut group = Object::new();
                group.set("name", Value::str(&group_name));
                group.set("type", Value::str("url-test"));
                group.set("proxies", Value::from(g.proxies.clone()));
                group.set("url", Value::str("https://www.gstatic.com/generate_204"));
                group.set("interval", Value::Number(300.0));
                group.set("lazy", Value::Bool(false));
                if has_providers {
                    group.set("use", provider_names.clone());
                    let data = COUNTRY_DATA.iter().find(|c| c.name == g.country.name).unwrap_or(g.country);
                    if let Some(filter) = build_country_name_filter(data) {
                        group.set("filter", Value::String(filter));
                    }
                }
                self.groups_mut("push")?.push(Value::Object(group));
                existing.add(norm);
            }
            country_group_names.push(group_name);
        }
        let node_select = self.core.t.t("outboundNames.Node Select");
        let proxy_list = self.get_proxy_list()?;
        let include = self.should_include_auto_select(&proxy_list)?;
        let rebuilt = build_node_select_members(&MemberOptions {
            proxy_list: &[],
            t: &self.core.t,
            group_by_country: true,
            manual_group_name: manual_group_name.as_deref(),
            country_group_names: &country_group_names,
            include_auto_select: include,
            include_reject: true,
        });
        let list = self.groups_mut("find")?;
        if let Some(group) = list.iter_mut().find(|g| g.truthy() && strict_equals(g.get("name"), &node_select))
            && group.get("proxies").is_array()
        {
            set_prop(group, "proxies", Value::from(rebuilt))?;
        }
        self.core.country_group_names = country_group_names;
        self.core.manual_group_name = manual_group_name;
        Ok(())
    }

    fn add_outbound_groups(&mut self, outbounds: &[String], proxy_list: &[Value]) -> JsResult<()> {
        let node_select = self.core.ts("outboundNames.Node Select");
        for outbound in outbounds {
            if *outbound == node_select {
                continue;
            }
            let name = self.core.t.outbound(outbound);
            if self.has_proxy_group(&Value::str(&name))? {
                continue;
            }
            let mut members = self.select_group_members(proxy_list)?;
            if is_direct_default_rule(&Value::str(outbound)) {
                let mut reordered = vec!["DIRECT".to_string()];
                reordered.extend(members.into_iter().filter(|m| m != "DIRECT"));
                members = reordered;
            }
            let mut group = Object::new();
            group.set("type", Value::str("select"));
            group.set("name", Value::String(name));
            group.set("proxies", Value::from(members));
            let group = self.with_providers(group)?;
            self.groups_mut("push")?.push(group);
        }
        Ok(())
    }

    fn add_custom_rule_groups(&mut self, proxy_list: &[Value]) -> JsResult<()> {
        let rules = self.core.custom_rules.clone();
        for rule in &rules {
            let rule_name = crate::js::prop(rule, "name")?.to_js_string();
            let name = self.core.t.outbound(&rule_name);
            if self.has_proxy_group(&Value::str(&name))? {
                continue;
            }
            let include = self.should_include_auto_select(proxy_list)?;
            let members = build_custom_rule_members(&self.member_options(proxy_list, include));
            let mut group = Object::new();
            group.set("type", Value::str("select"));
            group.set("name", Value::String(name));
            group.set("proxies", Value::from(members));
            let group = self.with_providers(group)?;
            self.groups_mut("push")?.push(group);
        }
        Ok(())
    }

    fn add_fall_back_group(&mut self, proxy_list: &[Value]) -> JsResult<()> {
        let name = self.core.t.t("outboundNames.Fall Back");
        if self.has_proxy_group(&name)? {
            return Ok(());
        }
        let members = self.select_group_members(proxy_list)?;
        let mut group = Object::new();
        group.set("type", Value::str("select"));
        group.set("name", name);
        group.set("proxies", Value::from(members));
        let group = self.with_providers(group)?;
        self.groups_mut("push")?.push(group);
        Ok(())
    }

    fn merge_user_proxy_groups(&mut self, user_groups: &[Value]) -> JsResult<()> {
        let proxy_list = self.get_proxy_list()?;
        let all_providers = ValueSet::from_values(&self.all_provider_names()?);
        let mut valid_refs = ValueSet::new();
        valid_refs.add(Value::str("DIRECT"));
        valid_refs.add(Value::str("REJECT"));
        for n in &proxy_list {
            valid_refs.add(n.clone());
        }
        for g in self.groups_or_empty()? {
            let n = normalize_group_name(g.get("name"));
            if n.truthy() {
                valid_refs.add(n);
            }
        }
        for user_group in user_groups {
            if !user_group.get("name").truthy() {
                continue;
            }
            let target = normalize_group_name(user_group.get("name"));
            let existing_index = match self.groups() {
                Value::Array(list) => {
                    list.iter().position(|g| g.truthy() && strict_equals(&normalize_group_name(g.get("name")), &target))
                }
                _ => None,
            };
            if let Some(idx) = existing_index {
                let list = self.groups_mut("findIndex")?;
                let existing = &mut list[idx];
                if let Value::Array(uses) = user_group.get("use")
                    && !uses.is_empty()
                {
                    let mut merged = spread_iterable(existing.get("use"), "(existing.use || [])")?;
                    merged.extend(uses.iter().filter(|p| all_providers.has(p)).cloned());
                    set_prop(existing, "use", Value::array(dedupe(merged)))?;
                }
                if let Value::Array(proxies) = user_group.get("proxies") {
                    let mut merged = spread_iterable(existing.get("proxies"), "(existing.proxies || [])")?;
                    merged.extend(proxies.iter().filter(|p| valid_refs.has(p)).cloned());
                    set_prop(existing, "proxies", Value::array(dedupe(merged)))?;
                }
                if user_group.get("url").truthy() {
                    set_prop(existing, "url", user_group.get("url").clone())?;
                }
                if let Value::Number(_) = user_group.get("interval") {
                    set_prop(existing, "interval", user_group.get("interval").clone())?;
                }
                if let Value::Bool(_) = user_group.get("lazy") {
                    set_prop(existing, "lazy", user_group.get("lazy").clone())?;
                }
            } else {
                let mut new_group = user_group.spread();
                if let Some(Value::Array(proxies)) = new_group.get("proxies").cloned() {
                    new_group
                        .set("proxies", Value::array(proxies.iter().filter(|p| valid_refs.has(p)).cloned().collect()));
                }
                if let Some(Value::Array(uses)) = new_group.get("use").cloned() {
                    new_group.set("use", Value::array(uses.iter().filter(|p| all_providers.has(p)).cloned().collect()));
                }
                let non_empty = |k: &str| new_group.get(k).and_then(Value::length).is_some_and(|l| l > 0);
                if non_empty("proxies") || non_empty("use") || new_group.get("type").is_some_and(Value::truthy) {
                    self.groups_mut("push")?.push(Value::Object(new_group));
                }
            }
        }
        Ok(())
    }
}
