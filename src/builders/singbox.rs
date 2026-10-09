//! `SingboxConfigBuilder`

use super::helpers::{
    DedupIndex, MemberOptions, ValueSet, build_custom_rule_members, build_node_select_members, build_selector_members,
    dedupe, normalize_group_name, stringify_without, unique_names,
};
use super::{BuildOptions, ConfigBuilder, Core, expect_array, prepare};
use crate::config::{
    Rule, generate_rule_sets, generate_rules, is_direct_default_rule, is_reject_action_rule, sing_box_config,
};
use crate::fetch::Fetcher;
use crate::js::number::js_number_to_string;
use crate::js::{JsError, JsResult, Object, Value, set_prop, strict_equals};
use crate::obj;
use crate::parsers::subscription::Format;
use crate::utils::group_proxies_by_country;

const RULE_SET_HTTP_CLIENT_TAG: &str = "rule-set-download";
const ANYTLS_OPTION_KEYS: [(&str, &str); 3] = [
    ("idle-session-check-interval", "idle_session_check_interval"),
    ("idle-session-timeout", "idle_session_timeout"),
    ("min-idle-session", "min_idle_session"),
];

pub struct SingboxBuilder {
    pub core: Core,
    pub enable_clash_ui: bool,
    pub external_controller: Option<String>,
    pub external_ui_download_url: Option<String>,
    pub singbox_version: String,
}

/// `SingboxConfigBuilder.prototype.convertProxy` (stateless).
pub fn convert_proxy(proxy: &Value) -> JsResult<Value> {
    let mut s = proxy.spread();
    if s.get("type").and_then(Value::as_str) == Some("anytls") {
        for (source, target) in ANYTLS_OPTION_KEYS {
            let src = s.get(source).cloned().unwrap_or_default();
            let tgt_undefined = s.get(target).is_none_or(Value::is_undefined);
            if !src.is_undefined() && tgt_undefined {
                s.set(target, src);
            }
            s.remove(source);
        }
        for key in ["idle_session_check_interval", "idle_session_timeout"] {
            if let Some(Value::Number(n)) = s.get(key) {
                let v = format!("{}s", js_number_to_string(*n));
                s.set(key, Value::String(v));
            }
        }
    }
    s.remove("udp");
    s.remove("network");
    let alpn = s.get("alpn").cloned().unwrap_or_default();
    let tls = s.get("tls").cloned().unwrap_or_default();
    if alpn.truthy() && tls.truthy() {
        if !tls.get("alpn").truthy() {
            let mut t = tls.spread();
            t.set("alpn", alpn);
            s.set("tls", Value::Object(t));
        }
        s.remove("alpn");
    } else if alpn.truthy() {
        s.remove("alpn");
    }
    s.remove("packet_encoding");
    if s.get("type").and_then(Value::as_str) == Some("hysteria2") {
        let ports = s.get("ports").cloned().unwrap_or_default();
        if ports.truthy() {
            let ranges: Vec<Value> = ports
                .to_js_string()
                .split(',')
                .map(|r| crate::js::string::js_trim(r).replacen('-', ":", 1))
                .filter(|r| !r.is_empty())
                .map(Value::String)
                .collect();
            if !ranges.is_empty() {
                s.set("server_ports", Value::array(ranges));
            }
            s.remove("ports");
        }
        if let Some(Value::Number(n)) = s.get("hop_interval") {
            let v = format!("{}s", js_number_to_string(*n));
            s.set("hop_interval", Value::String(v));
        }
        if let Some(up) = s.get("up").cloned().filter(|v| !v.is_undefined()) {
            s.set("up_mbps", up);
            s.remove("up");
        }
        if let Some(down) = s.get("down").cloned().filter(|v| !v.is_undefined()) {
            s.set("down_mbps", down);
            s.remove("down");
        }
        s.remove("auth");
        s.remove("recv_window_conn");
        s.remove("fast_open");
    }
    Ok(Value::Object(s))
}

fn has_match_values(v: &Option<Vec<String>>) -> bool {
    v.as_ref().is_some_and(|x| !x.is_empty())
}

impl SingboxBuilder {
    pub fn new(opts: &BuildOptions) -> JsResult<Self> {
        let base = if opts.base_config.is_nullish() { sing_box_config().clone() } else { opts.base_config.clone() };
        let mut core = Core::new(opts, &base);
        let node_select = core.t.t("outboundNames.Node Select");
        let has_servers = core.config.get("dns").get("servers").length_prop().to_number() > 0.0;
        if has_servers {
            let servers = core
                .config
                .as_object_mut()
                .and_then(|c| c.get_mut("dns"))
                .and_then(|d| d.as_object_mut())
                .and_then(|d| d.get_mut("servers"));
            match servers {
                Some(Value::Array(list)) => set_prop(&mut list[0], "detour", node_select)?,
                // An array-like object: `servers[0]` is its "0" key.
                Some(Value::Object(o)) => match o.get_mut("0") {
                    Some(first) => set_prop(first, "detour", node_select)?,
                    None => return Err(JsError::set_prop(&Value::Undefined, "detour")),
                },
                Some(Value::String(s)) => {
                    let first: String = s.chars().take(1).collect();
                    return Err(JsError::type_error(format!("Cannot create property 'detour' on string '{}'", first)));
                }
                _ => {}
            }
        }
        Ok(SingboxBuilder {
            core,
            enable_clash_ui: opts.enable_clash_ui,
            external_controller: opts.external_controller.clone().filter(|s| !s.is_empty()),
            external_ui_download_url: opts.external_ui_download_url.clone().filter(|s| !s.is_empty()),
            singbox_version: opts.singbox_version.clone(),
        })
    }

    fn config(&self) -> &Value {
        &self.core.config
    }

    fn outbounds(&self) -> &Value {
        self.core.config.get("outbounds")
    }

    /// `this.config.outbounds || []`
    fn outbounds_or_empty(&self) -> JsResult<Vec<Value>> {
        let o = self.outbounds();
        if !o.truthy() {
            return Ok(Vec::new());
        }
        expect_array(o, "(this.config.outbounds || [])", "some").cloned()
    }

    fn outbounds_mut(&mut self, method: &str) -> JsResult<&mut Vec<Value>> {
        let config = &mut self.core.config;
        if !config.is_object_like() {
            return Err(JsError::read_prop(&Value::Undefined, method));
        }
        let o = config.as_object_mut().unwrap();
        if o.get("outbounds").is_none() {
            return Err(JsError::read_prop(&Value::Undefined, method));
        }
        let v = o.get_mut("outbounds").unwrap();
        super::expect_array_mut(v, "this.config.outbounds", method)
    }

    fn ensure_outbounds(&mut self) -> JsResult<()> {
        if !self.outbounds().truthy() {
            set_prop(&mut self.core.config, "outbounds", Value::array(Vec::new()))?;
        }
        Ok(())
    }

    fn existing_provider_tags(&self) -> Vec<Value> {
        match self.config().get("outbound_providers") {
            Value::Array(list) => list.iter().map(|p| p.get("tag").clone()).filter(Value::truthy).collect(),
            _ => Vec::new(),
        }
    }

    fn provider_tags(&mut self) -> JsResult<Vec<Value>> {
        let existing = self.existing_provider_tags();
        Ok(self.core.get_auto_provider_descriptors(&existing)?.into_iter().map(|(n, _)| Value::String(n)).collect())
    }

    /// `getAllProviderTags()`
    fn all_provider_tags(&mut self) -> JsResult<Vec<Value>> {
        if self.singbox_version == "1.11" {
            return Ok(Vec::new());
        }
        let mut all = self.existing_provider_tags();
        all.extend(self.provider_tags()?);
        Ok(dedupe(all))
    }

    fn generate_outbound_providers(&mut self) -> JsResult<Vec<Value>> {
        let existing = self.existing_provider_tags();
        Ok(self
            .core
            .get_auto_provider_descriptors(&existing)?
            .into_iter()
            .map(|(name, url)| {
                obj! {
                    "tag" => name.clone(),
                    "type" => "http",
                    "download_url" => url,
                    "path" => format!("./providers/{}.json", name),
                    "download_interval" => "24h",
                    "health_check" => obj! {
                        "enabled" => true,
                        "url" => "https://www.gstatic.com/generate_204",
                        "interval" => "5m",
                    },
                }
            })
            .collect())
    }

    fn has_outbound_tag(&self, tag: &Value) -> JsResult<bool> {
        let target = normalize_group_name(tag);
        Ok(self.outbounds_or_empty()?.iter().any(|o| strict_equals(&normalize_group_name(o.get("tag")), &target)))
    }

    fn has_auto_select_candidates(&mut self, proxy_list: &[Value]) -> JsResult<bool> {
        Ok(!proxy_list.is_empty() || !self.all_provider_tags()?.is_empty())
    }

    fn member_options<'a>(&'a self, proxy_list: &'a [Value], include_auto_select: bool) -> MemberOptions<'a> {
        MemberOptions {
            proxy_list,
            t: &self.core.t,
            group_by_country: self.core.group_by_country,
            manual_group_name: self.core.manual_group_name.as_deref(),
            country_group_names: &self.core.country_group_names,
            include_auto_select,
            include_reject: false,
        }
    }

    fn selector_members(&mut self, proxy_list: &[Value]) -> JsResult<Vec<String>> {
        let include = self.core.include_auto_select && self.has_auto_select_candidates(proxy_list)?;
        Ok(build_selector_members(&self.member_options(proxy_list, include)))
    }

    fn validate_outbounds(&mut self) -> JsResult<()> {
        let proxy_list = self.get_proxy_list()?;
        let provider_tags = self.all_provider_tags()?;
        let mut invalid = ValueSet::new();
        if self.outbounds().truthy() {
            let list = self.outbounds_mut("forEach")?;
            for outbound in list.iter_mut() {
                if outbound.is_nullish() {
                    return Err(JsError::read_prop(outbound, "type"));
                }
                let empty = |v: &Value| !v.truthy() || matches!(v.length_prop(), Value::Number(n) if n == 0.0);
                if outbound.get("type").as_str() == Some("urltest")
                    && empty(outbound.get("outbounds"))
                    && empty(outbound.get("providers"))
                {
                    set_prop(outbound, "outbounds", Value::array(proxy_list.clone()))?;
                    if !provider_tags.is_empty() {
                        set_prop(outbound, "providers", Value::array(provider_tags.clone()))?;
                    }
                    if empty(outbound.get("outbounds")) && empty(outbound.get("providers")) {
                        invalid.add(normalize_group_name(outbound.get("tag")));
                    }
                }
            }
        }
        if !invalid.is_empty() {
            let list = self.outbounds_or_empty()?;
            let mut kept = Vec::new();
            for mut o in list.into_iter() {
                if invalid.has(&normalize_group_name(o.get("tag"))) {
                    continue;
                }
                if o.is_nullish() {
                    return Err(JsError::read_prop(&o, "outbounds"));
                }
                if let Value::Array(members) = o.get("outbounds").clone() {
                    let filtered: Vec<Value> =
                        members.iter().filter(|t| !invalid.has(&normalize_group_name(t))).cloned().collect();
                    set_prop(&mut o, "outbounds", Value::array(filtered))?;
                }
                kept.push(o);
            }
            set_prop(&mut self.core.config, "outbounds", Value::array(kept))?;
        }
        Ok(())
    }

    fn sanitize_legacy_special_outbounds(&mut self) -> JsResult<()> {
        let list = self.outbounds_or_empty()?;
        let mut legacy = ValueSet::new();
        for o in &list {
            if matches!(o.get("type").as_str(), Some("block" | "dns")) {
                let n = normalize_group_name(o.get("tag"));
                if n.truthy() {
                    legacy.add(n);
                }
            }
        }
        legacy.add(Value::str("REJECT"));
        let mut out = Vec::new();
        for mut o in list.into_iter() {
            if legacy.has(&normalize_group_name(o.get("tag"))) {
                continue;
            }
            if o.is_nullish() {
                return Err(JsError::read_prop(&o, "outbounds"));
            }
            if let Value::Array(members) = o.get("outbounds").clone() {
                let filtered: Vec<Value> =
                    members.iter().filter(|t| !legacy.has(&normalize_group_name(t))).cloned().collect();
                set_prop(&mut o, "outbounds", Value::array(filtered))?;
            }
            let ty = o.get("type").as_str();
            let is_group = matches!(ty, Some("selector" | "urltest"));
            let non_empty = |v: &Value| v.length_prop().to_number() > 0.0;
            if !is_group || non_empty(o.get("outbounds")) || non_empty(o.get("providers")) {
                out.push(o);
            }
        }
        set_prop(&mut self.core.config, "outbounds", Value::array(out))
    }

    fn build_route_target(&self, rule: &Rule) -> Object {
        let mut o = Object::new();
        if is_reject_action_rule(&rule.outbound) || rule.outbound.as_str() == Some("REJECT") {
            o.set("action", Value::str("reject"));
        } else {
            o.set("outbound", Value::String(self.core.t.outbound(&rule.outbound_str())));
        }
        o
    }

    fn route(&self) -> &Value {
        self.core.config.get("route")
    }

    fn route_mut(&mut self, setting: &str) -> JsResult<&mut Object> {
        let base = self.core.config.get("route").clone();
        match self.core.config.as_object_mut().and_then(|c| c.get_mut("route")) {
            Some(Value::Object(r)) => Ok(r),
            _ if base.is_nullish() => Err(JsError::set_prop(&base, setting)),
            _ => Err(JsError::type_error(format!(
                "Cannot create property '{}' on {} '{}'",
                setting,
                base.typeof_(),
                base.to_js_string()
            ))),
        }
    }

    fn push_route_rule(&mut self, entry: Object) -> JsResult<()> {
        let rules = self.route().get("rules").clone();
        let route = self.route_mut("rules")?;
        match route.get_mut("rules") {
            Some(Value::Array(list)) => {
                list.push(Value::Object(entry));
                Ok(())
            }
            _ => Err(match rules {
                Value::Undefined | Value::Null => JsError::read_prop(&rules, "push"),
                _ => JsError::not_function("this.config.route.rules.push"),
            }),
        }
    }

    fn configure_rule_set_download(&mut self) -> JsResult<()> {
        if self.singbox_version == "1.14" {
            if !self.route().get("default_http_client").truthy() {
                let clients = self.config().get("http_clients");
                if !clients.is_array() || clients.length() == Some(0) {
                    set_prop(
                        &mut self.core.config,
                        "http_clients",
                        Value::array(vec![obj! { "tag" => RULE_SET_HTTP_CLIENT_TAG, "detour" => "DIRECT" }]),
                    )?;
                }
                let first = self.config().get("http_clients").get("0").clone();
                if first.is_nullish() {
                    return Err(JsError::read_prop(&first, "tag"));
                }
                let tag = first.get("tag").clone();
                self.route_mut("default_http_client")?.set("default_http_client", tag);
            }
            return self.ensure_download_target_not_empty_direct();
        }
        let route = self.route_mut("rule_set")?;
        if let Some(Value::Array(sets)) = route.get_mut("rule_set") {
            for rs in sets.iter_mut() {
                if rs.get("type").as_str() == Some("remote") && !rs.get("download_detour").truthy() {
                    set_prop(rs, "download_detour", Value::str("DIRECT"))?;
                }
            }
        }
        Ok(())
    }

    /// sing-box >=1.12 refuses a detour to an option-less direct outbound, which
    /// broke every remote rule-set download on the 1.14 tier. A domain_resolver
    /// mirroring the route default makes the target non-empty without changing
    /// how it dials.
    fn ensure_download_target_not_empty_direct(&mut self) -> JsResult<()> {
        let client_tag = self.route().get("default_http_client").clone();
        let clients = self.config().get("http_clients");
        let client = if clients.truthy() {
            expect_array(clients, "(this.config.http_clients || [])", "find")?
                .iter()
                .find(|c| strict_equals(c.get("tag"), &client_tag))
                .cloned()
        } else {
            None
        };
        let detour = client.map(|c| c.get("detour").clone()).unwrap_or_default();
        if !detour.truthy() {
            return Ok(());
        }
        let outbounds = self.outbounds();
        let index = if outbounds.truthy() {
            expect_array(outbounds, "(this.config.outbounds || [])", "find")?
                .iter()
                .position(|o| strict_equals(o.get("tag"), &detour))
        } else {
            None
        };
        let Some(index) = index else { return Ok(()) };
        let Some(target) = self.outbounds().get(&index.to_string()).as_object() else { return Ok(()) };
        if target.get("type").and_then(Value::as_str) != Some("direct")
            || target.keys().iter().any(|k| *k != "type" && *k != "tag")
        {
            return Ok(());
        }

        let servers = self.config().get("dns").get("servers");
        let candidates: Vec<&Value> = if servers.truthy() {
            expect_array(servers, "((intermediate value) || [])", "filter")?
                .iter()
                .filter(|s| {
                    s.get("tag").truthy() && s.get("type").as_str() != Some("fakeip") && !s.get("detour").truthy()
                })
                .collect()
        } else {
            Vec::new()
        };
        let default_resolver = self.route().get("default_domain_resolver");
        let resolver = if default_resolver.as_str().is_some() {
            default_resolver.clone()
        } else {
            candidates
                .iter()
                .find(|s| s.get("type").as_str() == Some("udp"))
                .or(candidates.first())
                .map(|s| s.get("tag").clone())
                .unwrap_or_default()
        };
        if resolver.truthy() {
            set_prop(&mut self.outbounds_mut("find")?[index], "domain_resolver", resolver)?;
        }
        Ok(())
    }

    /// `formatConfig()`: finishes the config in place and returns it.
    pub fn format_config(&mut self) -> JsResult<Value> {
        let rules = generate_rules(&self.core.selected_rules, &mut self.core.custom_rules)?;
        let (site_sets, ip_sets) = generate_rule_sets(&self.core.selected_rules, &self.core.custom_rules)?;
        let mut all_sets = site_sets;
        all_sets.extend(ip_sets);
        self.route_mut("rule_set")?.set("rule_set", Value::array(all_sets));
        self.configure_rule_set_download()?;

        if !self.core.provider_urls.is_empty() {
            let mut providers = match self.config().get("outbound_providers") {
                Value::Array(list) => list.clone(),
                _ => crate::js::Arr::from(Vec::new()),
            };
            providers.extend(self.generate_outbound_providers()?);
            set_prop(&mut self.core.config, "outbound_providers", Value::Array(providers))?;
        }

        self.validate_outbounds()?;
        self.sanitize_legacy_special_outbounds()?;

        let with_protocol = |mut entry: Object, rule: &Rule| -> Object {
            if let Some(p) = rule.protocol.as_ref().filter(|p| !p.is_empty()) {
                entry.set("protocol", Value::from(p.clone()));
            }
            entry
        };

        for rule in rules.iter().filter(|r| has_match_values(&r.src_ip_cidr)) {
            let mut entry = Object::new();
            entry.set("source_ip_cidr", Value::from(rule.src_ip_cidr.clone().unwrap()));
            for (k, v) in self.build_route_target(rule).into_entries() {
                entry.set(k, v);
            }
            self.push_route_rule(with_protocol(entry, rule))?;
        }
        for rule in rules.iter().filter(|r| has_match_values(&r.domain_suffix) || has_match_values(&r.domain_keyword)) {
            let mut entry = self.build_route_target(rule);
            if has_match_values(&rule.domain_suffix) {
                entry.set("domain_suffix", Value::from(rule.domain_suffix.clone().unwrap()));
            }
            if has_match_values(&rule.domain_keyword) {
                entry.set("domain_keyword", Value::from(rule.domain_keyword.clone().unwrap()));
            }
            self.push_route_rule(with_protocol(entry, rule))?;
        }
        for rule in rules.iter().filter(|r| r.site_rules.first().is_some_and(|s| !s.is_empty())) {
            let mut entry = Object::new();
            entry.set("rule_set", Value::from(rule.site_rules.clone()));
            for (k, v) in self.build_route_target(rule).into_entries() {
                entry.set(k, v);
            }
            self.push_route_rule(with_protocol(entry, rule))?;
        }
        for rule in rules.iter().filter(|r| r.ip_rules.first().is_some_and(|s| !s.is_empty())) {
            let sets: Vec<String> = rule
                .ip_rules
                .iter()
                .map(|ip| crate::js::string::js_trim(ip).to_string())
                .filter(|ip| !ip.is_empty())
                .map(|ip| format!("{}-ip", ip))
                .collect();
            let mut entry = Object::new();
            entry.set("rule_set", Value::from(sets));
            for (k, v) in self.build_route_target(rule).into_entries() {
                entry.set(k, v);
            }
            self.push_route_rule(with_protocol(entry, rule))?;
        }
        for rule in rules.iter().filter(|r| has_match_values(&r.ip_cidr)) {
            let mut entry = Object::new();
            entry.set("ip_cidr", Value::from(rule.ip_cidr.clone().unwrap()));
            for (k, v) in self.build_route_target(rule).into_entries() {
                entry.set(k, v);
            }
            self.push_route_rule(with_protocol(entry, rule))?;
        }

        let node_select = self.core.ts("outboundNames.Node Select");
        let fall_back = self.core.ts("outboundNames.Fall Back");
        let head = vec![
            obj! { "action" => "sniff" },
            obj! { "protocol" => "dns", "action" => "hijack-dns" },
            obj! { "clash_mode" => "direct", "outbound" => "DIRECT" },
            obj! { "clash_mode" => "global", "outbound" => node_select },
        ];
        let rules_value = self.route().get("rules").clone();
        let route = self.route_mut("rules")?;
        match route.get_mut("rules") {
            Some(Value::Array(list)) => {
                list.splice(0..0, head);
            }
            _ => {
                return Err(match rules_value {
                    Value::Undefined | Value::Null => JsError::read_prop(&rules_value, "unshift"),
                    _ => JsError::not_function("this.config.route.rules.unshift"),
                });
            }
        }
        route.set("auto_detect_interface", Value::Bool(true));
        route.set("final", Value::String(fall_back));

        if self.enable_clash_ui || self.external_controller.is_some() || self.external_ui_download_url.is_some() {
            if !self.config().get("experimental").truthy() {
                set_prop(&mut self.core.config, "experimental", Value::Object(Object::new()))?;
            }
            let existing =
                self.config().get("experimental").get("clash_api").clone().or_falsy(|| Value::Object(Object::new()));
            let controller = self
                .external_controller
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Undefined)
                .or_falsy(|| existing.get("external_controller").clone())
                .or_falsy(|| Value::str("0.0.0.0:9090"));
            let ui_url = self
                .external_ui_download_url
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Undefined)
                .or_falsy(|| existing.get("external_ui_download_url").clone())
                .or_falsy(|| {
                    Value::str(
                        "https://gh-proxy.com/https://github.com/Zephyruso/zashboard/archive/refs/heads/gh-pages.zip",
                    )
                });
            let ui = existing.get("external_ui").clone().or_falsy(|| Value::str("./ui"));
            let secret = existing.get("secret").clone().or_nullish(|| Value::str(""));
            let detour = existing.get("external_ui_download_detour").clone().or_falsy(|| Value::str("DIRECT"));
            let mode = existing.get("default_mode").clone().or_falsy(|| Value::str("rule"));
            let mut api = existing.spread();
            api.set("external_controller", controller);
            api.set("external_ui", ui);
            api.set("external_ui_download_url", ui_url);
            api.set("external_ui_download_detour", detour);
            api.set("secret", secret);
            api.set("default_mode", mode);
            let mut experimental = self.config().get("experimental").clone();
            set_prop(&mut experimental, "clash_api", Value::Object(api))?;
            set_prop(&mut self.core.config, "experimental", experimental)?;
        }
        Ok(self.core.config.clone())
    }

    /// `await builder.build()`
    pub async fn build(&mut self, fetcher: &dyn Fetcher) -> JsResult<Value> {
        prepare(self, fetcher).await?;
        self.format_config()
    }

    pub fn subscription_userinfo(&self) -> Option<String> {
        self.core.subscription_userinfo.clone()
    }
}

impl ConfigBuilder for SingboxBuilder {
    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn is_compatible_provider_format(&self, format: Format) -> bool {
        self.singbox_version != "1.11" && format == Format::Singbox
    }

    fn get_proxies(&self) -> JsResult<Vec<Value>> {
        let list = expect_array(self.outbounds(), "this.config.outbounds", "filter")?;
        Ok(list.iter().filter(|o| !o.get("server").is_nullish()).cloned().collect())
    }

    fn get_proxy_name(&self, proxy: &Value) -> JsResult<Value> {
        Ok(crate::js::prop(proxy, "tag")?.clone())
    }

    fn convert_proxy(&self, proxy: &Value) -> JsResult<Value> {
        convert_proxy(proxy)
    }

    fn add_custom_items(&mut self, items: Vec<Value>) -> JsResult<()> {
        let get_name = |item: &Value| -> JsResult<Value> { Ok(item.get("tag").clone()) };
        let set_name = |mut item: Value, name: &str| -> Value {
            let _ = set_prop(&mut item, "tag", Value::str(name));
            item
        };
        let same_key = |item: &Value| -> Option<String> { Some(stringify_without(item, "tag")) };
        let mut index: Option<DedupIndex> = None;
        for item in items.into_iter().filter(|i| !i.is_nullish()) {
            if !item.get("tag").truthy() {
                continue;
            }
            let converted = self.convert_proxy(&item)?;
            if !converted.truthy() {
                continue;
            }
            self.ensure_outbounds()?;
            if !self.outbounds().is_array() {
                return Err(JsError::error("addProxyWithDedup expects the target collection to be an array"));
            }
            let list = self.outbounds_mut("push")?;
            if index.is_none() {
                index = Some(DedupIndex::new(list, &get_name, &same_key, Some(("tag", "existing")))?);
            }
            index.as_mut().unwrap().add(list, converted, &get_name, &set_name, &same_key)?;
        }
        Ok(())
    }

    fn add_auto_select_group(&mut self, proxy_list: &[Value]) -> JsResult<()> {
        if !self.core.include_auto_select {
            return Ok(());
        }
        self.ensure_outbounds()?;
        let tag = self.core.t.t("outboundNames.Auto Select");
        if self.has_outbound_tag(&tag)? {
            return Ok(());
        }
        let provider_tags = self.all_provider_tags()?;
        let members = unique_names(proxy_list);
        if members.is_empty() && provider_tags.is_empty() {
            return Ok(());
        }
        let mut group = Object::new();
        group.set("type", Value::str("urltest"));
        group.set("tag", tag);
        group.set("outbounds", Value::from(members));
        if !provider_tags.is_empty() {
            group.set("providers", Value::array(provider_tags));
        }
        self.outbounds_mut("unshift")?.insert(0, Value::Object(group));
        Ok(())
    }

    fn add_node_select_group(&mut self, proxy_list: &[Value]) -> JsResult<()> {
        self.ensure_outbounds()?;
        let tag = self.core.t.t("outboundNames.Node Select");
        if self.has_outbound_tag(&tag)? {
            return Ok(());
        }
        let include = self.core.include_auto_select && self.has_auto_select_candidates(proxy_list)?;
        let members = build_node_select_members(&self.member_options(proxy_list, include));
        let mut group = Object::new();
        group.set("type", Value::str("selector"));
        group.set("tag", tag);
        group.set("outbounds", Value::from(members));
        let provider_tags = self.all_provider_tags()?;
        if !provider_tags.is_empty() {
            group.set("providers", Value::array(provider_tags));
        }
        self.outbounds_mut("unshift")?.insert(0, Value::Object(group));
        Ok(())
    }

    fn add_country_groups(&mut self) -> JsResult<()> {
        let proxies = self.get_proxies()?;
        let mut names = Vec::with_capacity(proxies.len());
        for p in &proxies {
            names.push(self.get_proxy_name(p)?);
        }
        let groups = group_proxies_by_country(&names, |n| n.clone());
        let mut existing = ValueSet::new();
        for o in self.outbounds_or_empty()? {
            let n = normalize_group_name(o.get("tag"));
            if n.truthy() {
                existing.add(n);
            }
        }
        let manual_names: Vec<Value> = proxies.iter().map(|p| p.get("tag").clone()).filter(Value::truthy).collect();
        let manual_group_name =
            if manual_names.is_empty() { None } else { Some(self.core.ts("outboundNames.Manual Switch")) };
        if let Some(name) = &manual_group_name {
            let norm = normalize_group_name(&Value::str(name));
            if !existing.has(&norm) {
                self.outbounds_mut("push")?.push(obj! {
                    "type" => "selector",
                    "tag" => name.clone(),
                    "outbounds" => Value::array(manual_names.clone()),
                });
                existing.add(norm);
            }
        }
        let mut countries: Vec<&String> = groups.keys().collect();
        countries.sort();
        let mut country_group_names = Vec::new();
        let include = self.core.include_auto_select && {
            let list = self.get_proxy_list()?;
            self.has_auto_select_candidates(&list)?
        };
        for country in countries {
            let g = &groups[country];
            if g.proxies.is_empty() {
                continue;
            }
            let group_name = format!("{} {}", g.country.emoji, g.country.name);
            let norm = normalize_group_name(&Value::str(&group_name));
            if !existing.has(&norm) {
                self.outbounds_mut("push")?.push(obj! {
                    "tag" => group_name.clone(),
                    "type" => "urltest",
                    "outbounds" => Value::from(g.proxies.clone()),
                });
                existing.add(norm);
            }
            country_group_names.push(group_name);
        }
        let node_select = normalize_group_name(&self.core.t.t("outboundNames.Node Select"));
        let rebuilt = build_node_select_members(&MemberOptions {
            proxy_list: &[],
            t: &self.core.t,
            group_by_country: true,
            manual_group_name: manual_group_name.as_deref(),
            country_group_names: &country_group_names,
            include_auto_select: include,
            include_reject: false,
        });
        let list = self.outbounds_mut("find")?;
        if let Some(group) = list.iter_mut().find(|o| strict_equals(&normalize_group_name(o.get("tag")), &node_select))
            && group.get("outbounds").is_array()
        {
            set_prop(group, "outbounds", Value::from(rebuilt))?;
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
            let ob = Value::str(outbound);
            if is_reject_action_rule(&ob) {
                continue;
            }
            let mut members = self.selector_members(proxy_list)?;
            let tag = self.core.t.outbound(outbound);
            if self.has_outbound_tag(&Value::str(&tag))? {
                continue;
            }
            if is_direct_default_rule(&ob) {
                let mut reordered = vec!["DIRECT".to_string()];
                reordered.extend(members.into_iter().filter(|m| m != "DIRECT"));
                members = reordered;
            }
            self.outbounds_mut("push")?
                .push(obj! { "type" => "selector", "tag" => tag, "outbounds" => Value::from(members) });
        }
        Ok(())
    }

    fn add_custom_rule_groups(&mut self, proxy_list: &[Value]) -> JsResult<()> {
        let rules = self.core.custom_rules.clone();
        for rule in &rules {
            let include = self.core.include_auto_select && self.has_auto_select_candidates(proxy_list)?;
            let members = build_custom_rule_members(&self.member_options(proxy_list, include));
            let name = crate::js::prop(rule, "name")?.clone();
            if self.has_outbound_tag(&name)? {
                continue;
            }
            self.outbounds_mut("push")?
                .push(obj! { "type" => "selector", "tag" => name, "outbounds" => Value::from(members) });
        }
        Ok(())
    }

    fn add_fall_back_group(&mut self, proxy_list: &[Value]) -> JsResult<()> {
        let members = self.selector_members(proxy_list)?;
        let tag = self.core.t.t("outboundNames.Fall Back");
        if self.has_outbound_tag(&tag)? {
            return Ok(());
        }
        self.outbounds_mut("push")?
            .push(obj! { "type" => "selector", "tag" => tag, "outbounds" => Value::from(members) });
        Ok(())
    }

    fn merge_user_proxy_groups(&mut self, user_groups: &[Value]) -> JsResult<()> {
        let proxy_list = self.get_proxy_list()?;
        let all_providers = ValueSet::from_values(&self.all_provider_tags()?);
        let mut valid_refs = ValueSet::new();
        valid_refs.add(Value::str("DIRECT"));
        valid_refs.add(Value::str("direct"));
        for n in &proxy_list {
            valid_refs.add(n.clone());
        }
        for o in self.outbounds_or_empty()? {
            if o.is_nullish() {
                return Err(JsError::read_prop(&o, "type"));
            }
            if matches!(o.get("type").as_str(), Some("selector" | "urltest")) {
                let n = normalize_group_name(o.get("tag"));
                if n.truthy() {
                    valid_refs.add(n);
                }
            }
        }
        for user_group in user_groups {
            if !user_group.get("name").truthy() {
                continue;
            }
            let name = normalize_group_name(user_group.get("name"));
            let existing_index = self
                .outbounds_or_empty()?
                .iter()
                .position(|o| strict_equals(&normalize_group_name(o.get("tag")), &name));
            if let Some(idx) = existing_index {
                let list = self.outbounds_mut("findIndex")?;
                let existing = &mut list[idx];
                if let Value::Array(uses) = user_group.get("use")
                    && !uses.is_empty()
                {
                    let valid: Vec<Value> = uses.iter().filter(|p| all_providers.has(p)).cloned().collect();
                    let mut merged = spread_iterable(existing.get("providers"), "(existing.providers || [])")?;
                    merged.extend(valid);
                    set_prop(existing, "providers", Value::array(dedupe(merged)))?;
                }
                if let Value::Array(proxies) = user_group.get("proxies")
                    && !proxies.is_empty()
                {
                    let valid: Vec<Value> = proxies.iter().filter(|p| valid_refs.has(p)).cloned().collect();
                    let mut merged = spread_iterable(existing.get("outbounds"), "(existing.outbounds || [])")?;
                    merged.extend(valid);
                    set_prop(existing, "outbounds", Value::array(dedupe(merged)))?;
                }
                if user_group.get("url").truthy() {
                    set_prop(existing, "url", user_group.get("url").clone())?;
                }
                if let Value::Number(n) = user_group.get("interval") {
                    set_prop(existing, "interval", Value::String(format!("{}s", js_number_to_string(*n))))?;
                }
            } else {
                let mut out = Object::new();
                out.set(
                    "type",
                    Value::str(if user_group.get("type").as_str() == Some("url-test") {
                        "urltest"
                    } else {
                        "selector"
                    }),
                );
                out.set("tag", user_group.get("name").clone());
                if let Value::Array(proxies) = user_group.get("proxies") {
                    out.set("outbounds", Value::Array(proxies.iter().filter(|p| valid_refs.has(p)).cloned().collect()));
                }
                if let Value::Array(uses) = user_group.get("use") {
                    let valid: Vec<Value> = uses.iter().filter(|p| all_providers.has(p)).cloned().collect();
                    if !valid.is_empty() {
                        out.set("providers", Value::array(valid));
                    }
                }
                let non_empty = |k: &str| out.get(k).and_then(Value::length).is_some_and(|l| l > 0);
                if non_empty("outbounds") || non_empty("providers") {
                    self.outbounds_mut("push")?.push(Value::Object(out));
                }
            }
        }
        Ok(())
    }
}

/// `[...(value || [])]`: strings spread into code points, and any other truthy
/// non-array throws.
pub(crate) fn spread_iterable(v: &Value, expr: &str) -> JsResult<Vec<Value>> {
    match v {
        _ if !v.truthy() => Ok(Vec::new()),
        Value::Array(items) => Ok(items.to_vec()),
        Value::String(s) => Ok(s.chars().map(|c| Value::String(c.to_string())).collect()),
        _ => Err(JsError::not_iterable(expr)),
    }
}
