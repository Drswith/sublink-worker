//! Shared group-building helpers (`builders/helpers/*.js`).

use std::collections::HashSet;

use crate::config::Rule;
use crate::i18n::Translator;
use crate::js::string::{is_js_whitespace, js_trim};
use crate::js::{JsError, JsResult, Object, Value, json};

/// `uniqueNames(names)`: trimmed, non-empty, de-duplicated strings.
pub fn unique_names(names: &[Value]) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for name in names {
        let Value::String(s) = name else { continue };
        let n = js_trim(s);
        if n.is_empty() || !seen.insert(n.to_string()) {
            continue;
        }
        out.push(n.to_string());
    }
    out
}

pub fn strings(v: &[String]) -> Vec<Value> {
    v.iter().map(|s| Value::String(s.clone())).collect()
}

/// `withDirectReject(options, { includeReject })`
pub fn with_direct_reject(options: Vec<Value>, include_reject: bool) -> Vec<String> {
    let mut all = options;
    all.push(Value::str("DIRECT"));
    if include_reject {
        all.push(Value::str("REJECT"));
    }
    unique_names(&all)
}

pub struct MemberOptions<'a> {
    pub proxy_list: &'a [Value],
    pub t: &'a Translator,
    pub group_by_country: bool,
    pub manual_group_name: Option<&'a str>,
    pub country_group_names: &'a [String],
    pub include_auto_select: bool,
    pub include_reject: bool,
}

pub fn build_node_select_members(o: &MemberOptions) -> Vec<String> {
    let auto_name = o.t.t("outboundNames.Auto Select");
    let mut base: Vec<Value> = Vec::new();
    if o.include_auto_select {
        base.push(auto_name);
    }
    if o.group_by_country {
        if let Some(m) = o.manual_group_name {
            base.push(Value::str(m));
        }
        base.extend(strings(o.country_group_names));
    } else {
        base.extend(o.proxy_list.iter().cloned());
    }
    with_direct_reject(base, o.include_reject)
}

pub fn build_selector_members(o: &MemberOptions) -> Vec<String> {
    let mut base: Vec<Value> = vec![o.t.t("outboundNames.Node Select")];
    if o.group_by_country {
        if o.include_auto_select {
            base.push(o.t.t("outboundNames.Auto Select"));
        }
        if let Some(m) = o.manual_group_name {
            base.push(Value::str(m));
        }
        base.extend(strings(o.country_group_names));
    } else {
        base.extend(o.proxy_list.iter().cloned());
    }
    with_direct_reject(base, o.include_reject)
}

pub fn build_custom_rule_members(o: &MemberOptions) -> Vec<String> {
    let mut base: Vec<Value> = vec![o.t.t("outboundNames.Node Select")];
    if o.include_auto_select {
        base.push(o.t.t("outboundNames.Auto Select"));
    }
    if let Some(m) = o.manual_group_name {
        base.push(Value::str(m));
    }
    base.extend(o.proxy_list.iter().cloned());
    with_direct_reject(base, o.include_reject)
}

/// `normalizeGroupName(name)`: collapses whitespace runs (incl. Unicode spaces
/// and U+200B) into one space and trims; non-strings are returned unchanged.
pub fn normalize_group_name(name: &Value) -> Value {
    let Value::String(s) = name else { return name.clone() };
    let mut out = String::with_capacity(s.len());
    let mut in_space = false;
    for c in s.chars() {
        if is_js_whitespace(c) || c == '\u{200B}' {
            if !in_space {
                out.push(' ');
                in_space = true;
            }
        } else {
            out.push(c);
            in_space = false;
        }
    }
    Value::String(js_trim(&out).to_string())
}

/// Hashable identity for SameValueZero set membership.
pub fn key_of(v: &Value) -> String {
    match v {
        Value::String(s) => format!("s:{}", s),
        Value::Number(n) if *n == 0.0 => "n:0".into(),
        Value::Number(n) => format!("n:{}", crate::js::number::js_number_to_string(*n)),
        Value::Bool(b) => format!("b:{}", b),
        Value::Undefined => "u".into(),
        Value::Null => "null".into(),
        // Objects are only equal to themselves; copies never compare equal.
        _ => {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            format!("o:{}", NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
        }
    }
}

/// An insertion-ordered set with SameValueZero semantics (`new Set(...)`).
#[derive(Default, Clone)]
pub struct ValueSet {
    keys: HashSet<String>,
    items: Vec<Value>,
}

impl ValueSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_values<'a>(values: impl IntoIterator<Item = &'a Value>) -> Self {
        let mut s = Self::new();
        for v in values {
            s.add(v.clone());
        }
        s
    }

    pub fn add(&mut self, v: Value) {
        if self.keys.insert(key_of(&v)) {
            self.items.push(v);
        }
    }

    pub fn has(&self, v: &Value) -> bool {
        self.keys.contains(&key_of(v))
    }

    pub fn values(&self) -> &[Value] {
        &self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// `[...new Set(values)]`
pub fn dedupe(values: impl IntoIterator<Item = Value>) -> Vec<Value> {
    let mut set = ValueSet::new();
    for v in values {
        set.add(v);
    }
    set.items
}

/// `JSON.stringify` of the object without one key (`const { key, ...rest } = item`).
pub fn stringify_without(item: &Value, key: &str) -> String {
    let mut rest = item.spread();
    rest.remove(key);
    json::stringify(&Value::Object(rest)).unwrap_or_default()
}

/// Incremental state for `addProxyWithDedup` over one collection.
///
/// The original rescans the whole collection (stringifying every entry) for
/// each insert; caching the per-item keys keeps the same decisions in O(n).
pub struct DedupIndex {
    same_keys: HashSet<String>,
    names: ValueSet,
}

impl DedupIndex {
    pub fn new(
        collection: &[Value],
        get_name: &dyn Fn(&Value) -> JsResult<Value>,
        same_key: &dyn Fn(&Value) -> Option<String>,
    ) -> JsResult<Self> {
        let mut idx = DedupIndex { same_keys: HashSet::new(), names: ValueSet::new() };
        for item in collection {
            idx.record(item, get_name, same_key)?;
        }
        Ok(idx)
    }

    fn record(
        &mut self,
        item: &Value,
        get_name: &dyn Fn(&Value) -> JsResult<Value>,
        same_key: &dyn Fn(&Value) -> Option<String>,
    ) -> JsResult<()> {
        if let Some(k) = same_key(item) {
            self.same_keys.insert(k);
        }
        let name = get_name(item)?;
        self.names.add(if name.truthy() { name } else { Value::str("") });
        Ok(())
    }

    /// `addProxyWithDedup(collection, proxy, { getName, setName, isSame })`
    pub fn add(
        &mut self,
        collection: &mut Vec<Value>,
        proxy: Value,
        get_name: &dyn Fn(&Value) -> JsResult<Value>,
        set_name: &dyn Fn(Value, &str) -> Value,
        same_key: &dyn Fn(&Value) -> Option<String>,
    ) -> JsResult<()> {
        if !proxy.truthy() {
            return Ok(());
        }
        let mut candidate = proxy;
        let target_name = {
            let n = get_name(&candidate)?;
            if n.truthy() { n } else { Value::str("") }
        };
        if let Some(k) = same_key(&candidate)
            && self.same_keys.contains(&k)
        {
            return Ok(());
        }
        if self.names.has(&target_name) && target_name.truthy() {
            let base = target_name.to_js_string();
            let mut suffix = 2;
            while self.names.has(&Value::String(format!("{} {}", base, suffix))) {
                suffix += 1;
            }
            candidate = set_name(candidate, &format!("{} {}", base, suffix));
        }
        self.record(&candidate, get_name, same_key)?;
        collection.push(candidate);
        Ok(())
    }
}

/// `addProxyWithDedup` with the default name/identity callbacks.
pub fn add_proxy_with_dedup(collection: &mut Vec<Value>, proxy: Value) -> JsResult<()> {
    let get_name = |item: &Value| -> JsResult<Value> {
        Ok(item.get("name").clone().or_falsy(|| item.get("tag").clone()).or_falsy(|| Value::str("")))
    };
    let set_name = |mut item: Value, name: &str| -> Value {
        if let Some(o) = item.as_object_mut() {
            if o.contains_key("name") {
                o.set("name", Value::str(name));
            } else if o.contains_key("tag") {
                o.set("tag", Value::str(name));
            }
        }
        item
    };
    let same_key = |item: &Value| -> Option<String> { json::stringify(item) };
    let mut idx = DedupIndex::new(collection, &get_name, &same_key)?;
    idx.add(collection, proxy, &get_name, &set_name, &same_key)
}

/// `emitClashRules(rules, translator)`
pub fn emit_clash_rules(rules: &[Rule], t: &Translator) -> Vec<String> {
    let mut out = Vec::new();
    let name = |r: &Rule| t.outbound(&r.outbound_str());
    for r in rules.iter().filter(|r| r.src_ip_cidr.as_ref().is_some_and(|v| !v.is_empty())) {
        for cidr in r.src_ip_cidr.as_ref().unwrap() {
            if !cidr.is_empty() {
                out.push(format!("SRC-IP-CIDR,{},{}", cidr, name(r)));
            }
        }
    }
    for r in rules.iter().filter(|r| r.domain_suffix.as_ref().is_some_and(|v| !v.is_empty())) {
        for s in r.domain_suffix.as_ref().unwrap() {
            out.push(format!("DOMAIN-SUFFIX,{},{}", s, name(r)));
        }
    }
    for r in rules.iter().filter(|r| r.domain_keyword.as_ref().is_some_and(|v| !v.is_empty())) {
        for k in r.domain_keyword.as_ref().unwrap() {
            out.push(format!("DOMAIN-KEYWORD,{},{}", k, name(r)));
        }
    }
    for r in rules.iter().filter(|r| r.site_rules.first().is_some_and(|s| !s.is_empty())) {
        for site in &r.site_rules {
            out.push(format!("RULE-SET,{},{}", site, name(r)));
        }
    }
    for r in rules.iter().filter(|r| r.ip_rules.first().is_some_and(|s| !s.is_empty())) {
        for ip in &r.ip_rules {
            out.push(format!("RULE-SET,{}-ip,{},no-resolve", ip, name(r)));
        }
    }
    for r in rules.iter().filter(|r| r.ip_cidr.as_ref().is_some_and(|v| !v.is_empty())) {
        for cidr in r.ip_cidr.as_ref().unwrap() {
            out.push(format!("IP-CIDR,{},{},no-resolve", cidr, name(r)));
        }
    }
    out
}

fn trim_value(v: &Value) -> Value {
    match v {
        Value::String(s) => Value::str(js_trim(s)),
        other => other.clone(),
    }
}

/// `sanitizeClashProxyGroups(config)`
pub fn sanitize_clash_proxy_groups(config: &mut Value) -> JsResult<()> {
    let groups = config.get("proxy-groups").clone().or_falsy(|| Value::array(Vec::new()));
    let Value::Array(groups) = groups else { return Ok(()) };
    if groups.is_empty() {
        return Ok(());
    }
    let proxies = config.get("proxies").clone().or_falsy(|| Value::array(Vec::new()));
    let Value::Array(proxies) = proxies else {
        return Err(JsError::not_function("(config.proxies || []).map"));
    };
    let mut valid = ValueSet::new();
    valid.add(Value::str("DIRECT"));
    valid.add(Value::str("REJECT"));
    for p in &proxies {
        let n = trim_value(p.get("name"));
        if n.truthy() {
            valid.add(n);
        }
    }
    for g in &groups {
        let n = trim_value(g.get("name"));
        if n.truthy() {
            valid.add(n);
        }
    }
    let rebuilt: Vec<Value> = groups
        .iter()
        .cloned()
        .map(|group| {
            let Some(items) = group.get("proxies").as_array().cloned().filter(|_| group.truthy()) else {
                return group;
            };
            let deduped = dedupe(items.iter().map(trim_value).filter(Value::is_string));
            let uses_providers = group.get("use").as_array().is_some_and(|u| !u.is_empty());
            let kept: Vec<Value> = if uses_providers {
                deduped
            } else {
                deduped.into_iter().filter(|x| valid.has(&trim_value(x))).collect()
            };
            let mut obj = group.spread();
            obj.set("proxies", Value::array(kept));
            Value::Object(obj)
        })
        .collect();
    crate::js::set_prop(config, "proxy-groups", Value::array(rebuilt))
}

/// `{ ...a, ...b }` for two objects.
pub fn merge_objects(a: &Value, b: &Value) -> Object {
    let mut out = a.spread();
    for (k, v) in b.own_entries() {
        out.set(k, v);
    }
    out
}
