//! `SurgeConfigBuilder`

use super::helpers::{
    DedupIndex, MemberOptions, build_custom_rule_members, build_node_select_members, build_selector_members,
    unique_names,
};
use super::{BuildOptions, ConfigBuilder, Core, prepare};
use crate::config::{
    SURGE_IP_RULE_SET_BASEURL, SURGE_SITE_RULE_SET_BASEURL, generate_rules, is_direct_default_rule, surge_config,
};
use crate::fetch::Fetcher;
use crate::js::string::{js_trim, js_trim_start};
use crate::js::{JsError, JsResult, Value, set_prop};
use crate::utils::group_proxies_by_country;

pub struct SurgeBuilder {
    pub core: Core,
    pub subscription_url: Option<String>,
}

fn join_alpn(alpn: &Value) -> JsResult<String> {
    match alpn {
        Value::Array(items) => Ok(Value::Array(items.clone()).to_js_string()),
        _ => Err(JsError::not_function("proxy.tls.alpn.join")),
    }
}

/// `${value}`
fn s(v: &Value) -> String {
    v.to_js_string()
}

/// `SurgeConfigBuilder.prototype.convertProxy` (stateless).
pub fn convert_proxy(proxy: &Value) -> JsResult<Value> {
    let tag = s(proxy.get("tag"));
    let server = s(proxy.get("server"));
    let port = s(proxy.get("server_port"));
    let tls = proxy.get("tls");
    let transport = proxy.get("transport");
    let ws_or_grpc = |line: &mut String| -> JsResult<()> {
        match transport.get("type").as_str() {
            Some("ws") => {
                line.push_str(&format!(", ws=true, ws-path={}", s(transport.get("path"))));
                if transport.get("headers").truthy() {
                    line.push_str(&format!(", ws-headers=Host:{}", s(transport.get("headers").get("host"))));
                }
            }
            Some("grpc") => line.push_str(&format!(", grpc-service-name={}", s(transport.get("service_name")))),
            _ => {}
        }
        Ok(())
    };
    let line = match proxy.get("type").as_str() {
        Some("shadowsocks") => format!(
            "{} = ss, {}, {}, encrypt-method={}, password={}",
            tag,
            server,
            port,
            s(proxy.get("method")),
            s(proxy.get("password"))
        ),
        Some("vmess") => {
            let mut line = format!("{} = vmess, {}, {}, username={}", tag, server, port, s(proxy.get("uuid")));
            if crate::js::loose_equals(proxy.get("alter_id"), &Value::Number(0.0)) {
                line.push_str(", vmess-aead=true");
            }
            if tls.get("enabled").truthy() {
                line.push_str(", tls=true");
                if tls.get("server_name").truthy() {
                    line.push_str(&format!(", sni={}", s(tls.get("server_name"))));
                }
                if tls.get("insecure").truthy() {
                    line.push_str(", skip-cert-verify=true");
                }
                if tls.get("alpn").truthy() {
                    line.push_str(&format!(", alpn={}", join_alpn(tls.get("alpn"))?));
                }
            }
            ws_or_grpc(&mut line)?;
            line
        }
        Some("trojan") => {
            let mut line = format!("{} = trojan, {}, {}, password={}", tag, server, port, s(proxy.get("password")));
            if tls.get("server_name").truthy() {
                line.push_str(&format!(", sni={}", s(tls.get("server_name"))));
            }
            if tls.get("insecure").truthy() {
                line.push_str(", skip-cert-verify=true");
            }
            if tls.get("alpn").truthy() {
                line.push_str(&format!(", alpn={}", join_alpn(tls.get("alpn"))?));
            }
            ws_or_grpc(&mut line)?;
            line
        }
        Some("hysteria2") => {
            let mut line = format!("{} = hysteria2, {}, {}, password={}", tag, server, port, s(proxy.get("password")));
            if tls.get("server_name").truthy() {
                line.push_str(&format!(", sni={}", s(tls.get("server_name"))));
            }
            if tls.get("insecure").truthy() {
                line.push_str(", skip-cert-verify=true");
            }
            if tls.get("alpn").truthy() {
                line.push_str(&format!(", alpn={}", join_alpn(tls.get("alpn"))?));
            }
            line
        }
        Some("tuic") => {
            let mut line = format!(
                "{} = tuic, {}, {}, password={}, uuid={}",
                tag,
                server,
                port,
                s(proxy.get("password")),
                s(proxy.get("uuid"))
            );
            if tls.get("server_name").truthy() {
                line.push_str(&format!(", sni={}", s(tls.get("server_name"))));
            }
            if tls.get("alpn").truthy() {
                line.push_str(&format!(", alpn={}", join_alpn(tls.get("alpn"))?));
            }
            if tls.get("insecure").truthy() {
                line.push_str(", skip-cert-verify=true");
            }
            if proxy.get("congestion_control").truthy() {
                line.push_str(&format!(", congestion-controller={}", s(proxy.get("congestion_control"))));
            }
            if proxy.get("udp_relay_mode").truthy() {
                line.push_str(&format!(", udp-relay-mode={}", s(proxy.get("udp_relay_mode"))));
            }
            line
        }
        _ => format!("# {} - Unsupported proxy type: {}", tag, s(proxy.get("type"))),
    };
    Ok(Value::String(line))
}

/// `proxy.split('=')[0].trim()`
fn proxy_name(proxy: &Value) -> JsResult<String> {
    match proxy {
        Value::String(text) => Ok(js_trim(text.split('=').next().unwrap_or("")).to_string()),
        Value::Undefined | Value::Null => Err(JsError::read_prop(proxy, "split")),
        _ => Err(JsError::not_function("proxy.split")),
    }
}

impl SurgeBuilder {
    pub fn new(opts: &BuildOptions) -> Self {
        let base = if opts.base_config.is_nullish() { surge_config().clone() } else { opts.base_config.clone() };
        SurgeBuilder { core: Core::new(opts, &base), subscription_url: None }
    }

    /// `setSubscriptionUrl(url)`
    pub fn set_subscription_url(&mut self, url: &str) {
        self.subscription_url = Some(url.to_string());
    }

    fn config(&self) -> &Value {
        &self.core.config
    }

    /// `getValidProxies()`
    fn valid_proxies(&self) -> JsResult<Vec<Value>> {
        Ok(self
            .get_proxies()?
            .into_iter()
            .filter(|p| matches!(p, Value::String(s) if !js_trim_start(s).starts_with('#')))
            .collect())
    }

    fn group_name_of(&self, group: &Value) -> JsResult<Value> {
        match group {
            Value::String(_) => Ok(Value::String(proxy_name(group)?)),
            Value::Object(_) | Value::Array(_) | Value::Date(_) => Ok(group.get("name").clone()),
            _ => Ok(Value::Undefined),
        }
    }

    fn trimmed_name(v: &Value) -> JsResult<Value> {
        match v {
            Value::String(s) => Ok(Value::str(js_trim(s))),
            Value::Undefined | Value::Null => Ok(Value::Undefined),
            _ => Err(JsError::not_function("this.getGroupName(...)?.trim")),
        }
    }

    fn groups_or_empty(&self) -> Vec<Value> {
        match self.config().get("proxy-groups") {
            Value::Array(list) => list.to_vec(),
            _ => Vec::new(),
        }
    }

    fn has_proxy_group(&self, name: &Value) -> JsResult<bool> {
        let target = match name {
            Value::String(s) => Value::str(js_trim(s)),
            other => other.clone(),
        };
        if !target.truthy() {
            return Ok(false);
        }
        for group in self.groups_or_empty() {
            let existing = match &group {
                Value::String(_) => Value::String(proxy_name(&group)?),
                Value::Object(_) | Value::Array(_) => {
                    let n = group.get("name").clone().or_falsy(|| Value::str(""));
                    match n {
                        Value::String(s) => Value::str(js_trim(&s)),
                        _ => return Err(JsError::not_function("(group.name || '').trim")),
                    }
                }
                _ => Value::Undefined,
            };
            if crate::js::strict_equals(&existing, &target) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn create_proxy_group(name: &str, ty: &str, options: &[Value], extra: &str) -> Value {
        let sanitized = unique_names(options);
        let part = if sanitized.is_empty() { String::new() } else { format!(", {}", sanitized.join(", ")) };
        Value::String(format!("{} = {}{}{}", name, ty, part, extra))
    }

    fn groups_push(&mut self, group: Value) -> JsResult<()> {
        let current = self.config().get("proxy-groups").clone();
        match self.core.config.as_object_mut().and_then(|c| c.get_mut("proxy-groups")) {
            Some(Value::Array(list)) => {
                list.push(group);
                Ok(())
            }
            _ if current.is_nullish() => Err(JsError::read_prop(&current, "push")),
            _ => Err(JsError::not_function("this.config['proxy-groups'].push")),
        }
    }

    fn member_options<'a>(&'a self, proxy_list: &'a [Value], group_by_country: bool) -> MemberOptions<'a> {
        MemberOptions {
            proxy_list,
            t: &self.core.t,
            group_by_country,
            manual_group_name: self.core.manual_group_name.as_deref(),
            country_group_names: &self.core.country_group_names,
            include_auto_select: self.core.include_auto_select,
            include_reject: true,
        }
    }

    fn convert_object_group(group: &Value) -> Value {
        if !group.truthy() || !group.get("name").truthy() || !group.get("type").truthy() {
            return Value::Null;
        }
        let ty = if group.get("type").as_str() == Some("url-test") { "url-test" } else { "select" };
        let proxies: Vec<Value> = group.get("proxies").as_array().cloned().unwrap_or_default();
        let mut out = format!("{} = {}", s(group.get("name")), ty);
        if !proxies.is_empty() {
            out.push_str(&format!(
                ", {}",
                proxies
                    .iter()
                    .map(|p| if p.is_nullish() { String::new() } else { s(p) })
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if ty == "url-test" {
            if group.get("url").truthy() {
                out.push_str(&format!(", url={}", s(group.get("url"))));
            } else {
                out.push_str(", url=http://www.gstatic.com/generate_204");
            }
            if group.get("interval").truthy() {
                out.push_str(&format!(", interval={}", s(group.get("interval"))));
            } else {
                out.push_str(", interval=300");
            }
        }
        Value::String(out)
    }

    fn entries_lines(v: &Value, out: &mut Vec<String>) {
        for (k, val) in v.own_entries() {
            out.push(format!("{} = {}", k, s(&val)));
        }
    }

    /// `formatConfig()` → Surge INI text.
    pub fn format_config(&mut self) -> JsResult<String> {
        let rules = generate_rules(&self.core.selected_rules, &mut self.core.custom_rules)?;
        let t = self.core.t;
        let mut out: Vec<String> = Vec::new();
        if let Some(url) = &self.subscription_url {
            out.push(format!("#!MANAGED-CONFIG {} interval=43200 strict=false", url));
            out.push(String::new());
        }
        out.push("[General]".into());
        if self.config().get("general").truthy() {
            Self::entries_lines(self.config().get("general"), &mut out);
        }
        if self.config().get("replica").truthy() {
            out.push("\n[Replica]".into());
            Self::entries_lines(self.config().get("replica"), &mut out);
        }
        out.push("\n[Proxy]".into());
        out.push("DIRECT = direct".into());
        let proxies = self.config().get("proxies");
        if proxies.truthy() {
            match proxies {
                Value::Array(list) => {
                    out.extend(list.iter().map(|p| if p.is_nullish() { String::new() } else { s(p) }));
                }
                Value::String(text) => out.extend(text.chars().map(|c| c.to_string())),
                _ => return Err(JsError::not_iterable("this.config.proxies")),
            }
        }
        out.push("\n[Proxy Group]".into());
        let groups = self.config().get("proxy-groups");
        if groups.truthy() {
            let Value::Array(list) = groups else {
                return Err(JsError::not_function("this.config['proxy-groups'].map"));
            };
            for group in list.iter() {
                let line = match group {
                    Value::String(_) => group.clone(),
                    g if g.truthy() && g.is_object_like() => Self::convert_object_group(g),
                    _ => Value::Null,
                };
                if !line.is_nullish() {
                    out.push(s(&line));
                }
            }
        }
        out.push("\n[Rule]".into());

        let name = |r: &crate::config::Rule| t.outbound(&r.outbound_str());
        for rule in rules.iter().filter(|r| r.src_ip_cidr.as_ref().is_some_and(|v| !v.is_empty())) {
            for cidr in rule.src_ip_cidr.as_ref().unwrap() {
                let value = js_trim(cidr);
                if value.is_empty() {
                    continue;
                }
                let safe: String = value.chars().filter(|c| *c != '\r' && *c != '\n').collect();
                let safe = js_trim(&safe);
                if safe.is_empty() {
                    continue;
                }
                if let Some(ip) = safe.strip_suffix("/32") {
                    out.push(format!("SRC-IP,{},{}", ip, name(rule)));
                } else if !safe.contains('/') {
                    out.push(format!("SRC-IP,{},{}", safe, name(rule)));
                } else {
                    out.push(format!("# SRC-IP-CIDR not supported by Surge, skipped: {}", safe));
                }
            }
        }
        for rule in rules.iter() {
            for suffix in rule.domain_suffix.iter().flatten() {
                out.push(format!("DOMAIN-SUFFIX,{},{}", suffix, name(rule)));
            }
        }
        for rule in rules.iter() {
            for keyword in rule.domain_keyword.iter().flatten() {
                out.push(format!("DOMAIN-KEYWORD,{},{}", keyword, name(rule)));
            }
        }
        for rule in rules.iter().filter(|r| r.site_rules.first().map(String::as_str) != Some("")) {
            for site in &rule.site_rules {
                out.push(format!("RULE-SET,{}{}.conf,{}", SURGE_SITE_RULE_SET_BASEURL, site, name(rule)));
            }
        }
        for rule in rules.iter().filter(|r| r.ip_rules.first().map(String::as_str) != Some("")) {
            for ip in &rule.ip_rules {
                out.push(format!("RULE-SET,{}{}.txt,{},no-resolve", SURGE_IP_RULE_SET_BASEURL, ip, name(rule)));
            }
        }
        for rule in rules.iter() {
            for cidr in rule.ip_cidr.iter().flatten() {
                out.push(format!("IP-CIDR,{},{},no-resolve", cidr, name(rule)));
            }
        }
        out.push(format!("FINAL,{}", t.ts("outboundNames.Fall Back")));
        Ok(out.join("\n"))
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

impl ConfigBuilder for SurgeBuilder {
    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn get_proxies(&self) -> JsResult<Vec<Value>> {
        match self.config().get("proxies") {
            Value::Array(list) => Ok(list.to_vec()),
            v if !v.truthy() => Ok(Vec::new()),
            _ => Err(JsError::not_function("this.getProxies().filter")),
        }
    }

    fn get_proxy_name(&self, proxy: &Value) -> JsResult<Value> {
        Ok(Value::String(proxy_name(proxy)?))
    }

    /// Unsupported proxy comments are excluded from groups (issue #299).
    fn get_proxy_list(&self) -> JsResult<Vec<Value>> {
        self.valid_proxies()?.iter().map(|p| self.get_proxy_name(p)).collect()
    }

    fn convert_proxy(&self, proxy: &Value) -> JsResult<Value> {
        convert_proxy(proxy)
    }

    fn add_custom_items(&mut self, items: Vec<Value>) -> JsResult<()> {
        let get_name = |item: &Value| -> JsResult<Value> { Ok(Value::String(proxy_name(item)?)) };
        let set_name = |item: Value, name: &str| -> Value {
            match &item {
                Value::String(text) => match text.find('=') {
                    Some(pos) if pos > 0 => Value::String(format!("{}{}", name, &text[pos..])),
                    _ => item,
                },
                _ => item,
            }
        };
        let same_key = |item: &Value| -> Option<String> {
            match item {
                Value::String(text) => Some(match text.find('=') {
                    Some(pos) => text[pos..].to_string(),
                    None => text.clone(),
                }),
                _ => None,
            }
        };
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
                index = Some(DedupIndex::new(list, &get_name, &same_key, None)?);
            }
            index.as_mut().unwrap().add(list, converted, &get_name, &set_name, &same_key)?;
        }
        Ok(())
    }

    fn add_auto_select_group(&mut self, proxy_list: &[Value]) -> JsResult<()> {
        if !self.core.include_auto_select {
            return Ok(());
        }
        if !self.config().get("proxy-groups").truthy() {
            set_prop(&mut self.core.config, "proxy-groups", Value::array(Vec::new()))?;
        }
        let name = self.core.ts("outboundNames.Auto Select");
        if self.has_proxy_group(&Value::str(&name))? {
            return Ok(());
        }
        let options: Vec<Value> = unique_names(proxy_list).into_iter().map(Value::String).collect();
        let group = Self::create_proxy_group(
            &name,
            "url-test",
            &options,
            ", url=http://www.gstatic.com/generate_204, interval=300",
        );
        self.groups_push(group)
    }

    fn add_node_select_group(&mut self, proxy_list: &[Value]) -> JsResult<()> {
        let options = build_node_select_members(&self.member_options(proxy_list, false));
        let name = self.core.ts("outboundNames.Node Select");
        if self.has_proxy_group(&Value::str(&name))? {
            return Ok(());
        }
        let group = Self::create_proxy_group(&name, "select", &super::helpers::strings(&options), "");
        self.groups_push(group)
    }

    fn add_country_groups(&mut self) -> JsResult<()> {
        let proxies = self.valid_proxies()?;
        let mut names = Vec::with_capacity(proxies.len());
        for p in &proxies {
            names.push(self.get_proxy_name(p)?);
        }
        let groups = group_proxies_by_country(&names, |n| n.clone());
        let mut existing: Vec<Value> = Vec::new();
        for g in self.groups_or_empty() {
            let n = Self::trimmed_name(&self.group_name_of(&g)?)?;
            if n.truthy() {
                existing.push(n);
            }
        }
        let has = |list: &Vec<Value>, v: &str| list.iter().any(|x| x.as_str() == Some(v));
        let manual_names: Vec<Value> = names.iter().filter(|n| n.truthy()).cloned().collect();
        let manual_group_name =
            if manual_names.is_empty() { None } else { Some(self.core.ts("outboundNames.Manual Switch")) };
        if let Some(name) = &manual_group_name {
            let norm = js_trim(name).to_string();
            if !has(&existing, &norm) {
                let options: Vec<Value> = unique_names(&manual_names).into_iter().map(Value::String).collect();
                self.groups_push(Self::create_proxy_group(name, "select", &options, ""))?;
                existing.push(Value::String(norm));
            }
        }
        let mut countries: Vec<&String> = groups.keys().collect();
        countries.sort();
        let mut country_group_names = Vec::new();
        for country in countries {
            let g = &groups[country];
            let group_name = format!("{} {}", g.country.emoji, g.country.name);
            country_group_names.push(group_name.clone());
            let norm = js_trim(&group_name).to_string();
            if !has(&existing, &norm) {
                let options: Vec<Value> = g.proxies.iter().map(Value::str).collect();
                self.groups_push(Self::create_proxy_group(
                    &group_name,
                    "url-test",
                    &options,
                    ", url=https://www.gstatic.com/generate_204, interval=300",
                ))?;
                existing.push(Value::String(norm));
            }
        }
        let node_select = self.core.ts("outboundNames.Node Select");
        let groups_now = match self.config().get("proxy-groups") {
            Value::Array(list) => list.to_vec(),
            other => return Err(JsError::read_prop(other, "findIndex")),
        };
        let mut index = None;
        for (i, g) in groups_now.iter().enumerate() {
            if self.group_name_of(g)?.as_str() == Some(node_select.as_str()) {
                index = Some(i);
                break;
            }
        }
        if let Some(i) = index {
            let options = build_node_select_members(&MemberOptions {
                proxy_list: &[],
                t: &self.core.t,
                group_by_country: true,
                manual_group_name: manual_group_name.as_deref(),
                country_group_names: &country_group_names,
                include_auto_select: self.core.include_auto_select,
                include_reject: true,
            });
            let group = Self::create_proxy_group(&node_select, "select", &super::helpers::strings(&options), "");
            if let Some(Value::Array(list)) = self.core.config.as_object_mut().and_then(|c| c.get_mut("proxy-groups")) {
                list[i] = group;
            }
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
            let mut options = build_selector_members(&self.member_options(proxy_list, self.core.group_by_country));
            let name = self.core.t.outbound(outbound);
            if self.has_proxy_group(&Value::str(&name))? {
                continue;
            }
            if is_direct_default_rule(&Value::str(outbound)) {
                let mut reordered = vec!["DIRECT".to_string()];
                reordered.extend(options.into_iter().filter(|o| o != "DIRECT"));
                options = reordered;
            }
            let group = Self::create_proxy_group(&name, "select", &super::helpers::strings(&options), "");
            self.groups_push(group)?;
        }
        Ok(())
    }

    fn add_custom_rule_groups(&mut self, proxy_list: &[Value]) -> JsResult<()> {
        let rules = self.core.custom_rules.clone();
        for rule in &rules {
            let name = crate::js::prop(rule, "name")?.clone();
            if self.has_proxy_group(&name)? {
                continue;
            }
            let options = build_custom_rule_members(&self.member_options(proxy_list, self.core.group_by_country));
            let group =
                Self::create_proxy_group(&name.to_js_string(), "select", &super::helpers::strings(&options), "");
            self.groups_push(group)?;
        }
        Ok(())
    }

    fn add_fall_back_group(&mut self, proxy_list: &[Value]) -> JsResult<()> {
        let options = build_selector_members(&self.member_options(proxy_list, self.core.group_by_country));
        let name = self.core.ts("outboundNames.Fall Back");
        if self.has_proxy_group(&Value::str(&name))? {
            return Ok(());
        }
        let group = Self::create_proxy_group(&name, "select", &super::helpers::strings(&options), "");
        self.groups_push(group)
    }
}
