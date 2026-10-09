//! Rule definitions, rule generators and platform base configs.

pub mod subconverter;

pub use subconverter::generate_subconverter_config;

use std::sync::OnceLock;

use crate::js::string::js_trim;
use crate::js::{JsError, JsResult, Object, Value, includes, json, same_value_zero};
use crate::obj;

pub const SITE_RULE_SET_BASE_URL: &str =
    "https://gh-proxy.com/https://github.com/MetaCubeX/meta-rules-dat/raw/refs/heads/sing/geo/geosite/";
pub const IP_RULE_SET_BASE_URL: &str =
    "https://gh-proxy.com/https://github.com/MetaCubeX/meta-rules-dat/raw/refs/heads/sing/geo/geoip/";
pub const CLASH_SITE_RULE_SET_BASE_URL: &str =
    "https://gh-proxy.com/https://github.com/MetaCubeX/meta-rules-dat/raw/refs/heads/meta/geo/geosite/";
pub const CLASH_IP_RULE_SET_BASE_URL: &str =
    "https://gh-proxy.com/https://github.com/MetaCubeX/meta-rules-dat/raw/refs/heads/meta/geo/geoip/";
pub const SURGE_SITE_RULE_SET_BASEURL: &str =
    "https://gh-proxy.com/https://github.com/NSZA156/surge-geox-rules/raw/refs/heads/release/geo/geosite/";
pub const SURGE_IP_RULE_SET_BASEURL: &str =
    "https://gh-proxy.com/https://github.com/NSZA156/surge-geox-rules/raw/refs/heads/release/geo/geoip/";

pub struct UnifiedRule {
    pub name: &'static str,
    pub site_rules: &'static [&'static str],
    pub ip_rules: &'static [&'static str],
}

pub static UNIFIED_RULES: &[UnifiedRule] = &[
    UnifiedRule { name: "Ad Block", site_rules: &["category-ads-all"], ip_rules: &[] },
    UnifiedRule { name: "AI Services", site_rules: &["category-ai-!cn"], ip_rules: &[] },
    UnifiedRule { name: "Bilibili", site_rules: &["bilibili"], ip_rules: &[] },
    UnifiedRule { name: "Youtube", site_rules: &["youtube"], ip_rules: &[] },
    UnifiedRule { name: "Google", site_rules: &["google"], ip_rules: &["google"] },
    UnifiedRule { name: "Private", site_rules: &[], ip_rules: &["private"] },
    UnifiedRule { name: "Location:CN", site_rules: &["geolocation-cn", "cn"], ip_rules: &["cn"] },
    UnifiedRule { name: "Telegram", site_rules: &[], ip_rules: &["telegram"] },
    UnifiedRule { name: "Github", site_rules: &["github", "gitlab"], ip_rules: &[] },
    UnifiedRule { name: "Microsoft", site_rules: &["microsoft"], ip_rules: &[] },
    UnifiedRule { name: "Apple", site_rules: &["apple"], ip_rules: &[] },
    UnifiedRule {
        name: "Social Media",
        site_rules: &["facebook", "instagram", "twitter", "tiktok", "linkedin"],
        ip_rules: &[],
    },
    UnifiedRule {
        name: "Streaming",
        site_rules: &["netflix", "hulu", "disney", "hbo", "amazon", "bahamut"],
        ip_rules: &[],
    },
    UnifiedRule { name: "Gaming", site_rules: &["steam", "epicgames", "ea", "ubisoft", "blizzard"], ip_rules: &[] },
    UnifiedRule {
        name: "Education",
        site_rules: &["coursera", "edx", "udemy", "khanacademy", "category-scholar-!cn"],
        ip_rules: &[],
    },
    UnifiedRule { name: "Financial", site_rules: &["paypal", "visa", "mastercard", "stripe", "wise"], ip_rules: &[] },
    UnifiedRule {
        name: "Cloud Services",
        site_rules: &["aws", "azure", "digitalocean", "heroku", "dropbox"],
        ip_rules: &[],
    },
    UnifiedRule { name: "Non-China", site_rules: &["geolocation-!cn"], ip_rules: &[] },
];

/// Rules that default to DIRECT instead of Node Select.
pub fn is_direct_default_rule(outbound: &Value) -> bool {
    matches!(outbound.as_str(), Some("Private") | Some("Location:CN"))
}

pub fn is_reject_action_rule(outbound: &Value) -> bool {
    outbound.as_str() == Some("Ad Block")
}

const MINIMAL: &[&str] = &["Location:CN", "Private", "Non-China"];
const BALANCED: &[&str] =
    &["Location:CN", "Private", "Non-China", "Github", "Google", "Youtube", "AI Services", "Telegram"];

/// `PREDEFINED_RULE_SETS[name]`
pub fn predefined_rule_set(name: &str) -> Option<Value> {
    match name {
        "minimal" => Some(Value::from(MINIMAL.to_vec())),
        "balanced" => Some(Value::from(BALANCED.to_vec())),
        "comprehensive" => Some(Value::from(UNIFIED_RULES.iter().map(|r| r.name).collect::<Vec<_>>())),
        _ => None,
    }
}

/// The `PREDEFINED_RULE_SETS` object as JSON (embedded in the web page).
pub fn predefined_rule_sets_value() -> Value {
    obj! {
        "minimal" => predefined_rule_set("minimal").unwrap(),
        "balanced" => predefined_rule_set("balanced").unwrap(),
        "comprehensive" => predefined_rule_set("comprehensive").unwrap(),
    }
}

/// `getOutbounds(selectedRuleNames)`
pub fn get_outbounds(selected: &Value) -> Vec<String> {
    let Value::Array(_) = selected else { return Vec::new() };
    UNIFIED_RULES.iter().filter(|r| includes(selected, &Value::str(r.name))).map(|r| r.name.to_string()).collect()
}

/// A generated routing rule (custom rules carry the extra match fields).
#[derive(Clone, Debug, Default)]
pub struct Rule {
    pub site_rules: Vec<String>,
    pub ip_rules: Vec<String>,
    pub domain_suffix: Option<Vec<String>>,
    pub domain_keyword: Option<Vec<String>>,
    pub ip_cidr: Option<Vec<String>>,
    pub src_ip_cidr: Option<Vec<String>>,
    pub protocol: Option<Vec<String>>,
    pub outbound: Value,
}

impl Rule {
    /// `${rule.outbound}` as used when building `outboundNames.*` keys.
    pub fn outbound_str(&self) -> String {
        self.outbound.to_js_string()
    }
}

fn to_string_array(value: &Value) -> Vec<String> {
    match value {
        Value::Array(items) => {
            items.iter().filter_map(|x| x.as_str()).map(|s| js_trim(s).to_string()).filter(|s| !s.is_empty()).collect()
        }
        Value::String(s) => s.split(',').map(|x| js_trim(x).to_string()).filter(|s| !s.is_empty()).collect(),
        _ => Vec::new(),
    }
}

/// `/^[A-Za-z0-9_-]+(?:\.[A-Za-z0-9_-]+)*$/` and no `..`
fn is_safe_rule_id(v: &str) -> bool {
    let seg_ok = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    v.split('.').all(seg_ok) && !v.contains("..")
}

fn sanitize_rule_ids(values: &Value) -> Vec<String> {
    to_string_array(values).into_iter().filter(|v| is_safe_rule_id(v)).collect()
}

/// Normalizes `selectedRules` the way every generator does (preset names,
/// empty fallback to `minimal`).
fn resolve_selected(selected: &Value) -> Value {
    let mut s = selected.clone();
    if let Value::String(name) = &s
        && let Some(set) = predefined_rule_set(name)
    {
        s = set;
    }
    let empty = !s.truthy() || matches!(s.length_prop(), Value::Number(n) if n == 0.0);
    if empty { predefined_rule_set("minimal").unwrap() } else { s }
}

fn selected_includes(selected: &Value, name: &str) -> JsResult<bool> {
    match selected {
        Value::Array(_) | Value::String(_) => Ok(includes(selected, &Value::str(name))),
        _ => Err(JsError::not_function("selectedRules.includes")),
    }
}

/// Members of `new Set(selectedRules)`.
fn selected_set(selected: &Value) -> JsResult<Vec<Value>> {
    match selected {
        Value::Array(items) => Ok(items.to_vec()),
        Value::String(s) => Ok(s.chars().map(|c| Value::String(c.to_string())).collect()),
        other => Err(JsError::type_error(format!(
            "{} {} is not iterable (cannot read property Symbol(Symbol.iterator))",
            other.typeof_(),
            other.to_js_string()
        ))),
    }
}

fn custom_rule_field<'a>(rule: &'a Value, key: &str) -> JsResult<&'a Value> {
    crate::js::prop(rule, key)
}

/// `generateRules(selectedRules, customRules)`. Note: reverses `custom_rules`
/// in place exactly like the original `customRules.reverse()`.
pub fn generate_rules(selected: &Value, custom_rules: &mut [Value]) -> JsResult<Vec<Rule>> {
    let selected = resolve_selected(selected);
    let mut rules = Vec::new();
    for rule in UNIFIED_RULES {
        if selected_includes(&selected, rule.name)? {
            rules.push(Rule {
                site_rules: rule.site_rules.iter().map(|s| s.to_string()).collect(),
                ip_rules: rule.ip_rules.iter().map(|s| s.to_string()).collect(),
                domain_suffix: None,
                domain_keyword: None,
                ip_cidr: None,
                src_ip_cidr: None,
                protocol: None,
                outbound: Value::str(rule.name),
            });
        }
    }
    custom_rules.reverse();
    let mut custom = Vec::with_capacity(custom_rules.len());
    for rule in custom_rules.iter() {
        custom.push(Rule {
            site_rules: sanitize_rule_ids(custom_rule_field(rule, "site")?),
            ip_rules: sanitize_rule_ids(custom_rule_field(rule, "ip")?),
            domain_suffix: Some(to_string_array(custom_rule_field(rule, "domain_suffix")?)),
            domain_keyword: Some(to_string_array(custom_rule_field(rule, "domain_keyword")?)),
            ip_cidr: Some(to_string_array(custom_rule_field(rule, "ip_cidr")?)),
            src_ip_cidr: Some(to_string_array(custom_rule_field(rule, "src_ip_cidr")?)),
            protocol: Some(to_string_array(custom_rule_field(rule, "protocol")?)),
            outbound: custom_rule_field(rule, "name")?.clone(),
        });
    }
    // each custom rule is unshifted in turn, so the final order is reversed
    custom.reverse();
    custom.extend(rules);
    Ok(custom)
}

fn collect_selected_rule_sets(selected: &Value) -> JsResult<(Vec<String>, Vec<String>)> {
    let set = selected_set(selected)?;
    let mut site: Vec<String> = Vec::new();
    let mut ip: Vec<String> = Vec::new();
    for rule in UNIFIED_RULES {
        if set.iter().any(|v| same_value_zero(v, &Value::str(rule.name))) {
            for s in rule.site_rules {
                if !site.iter().any(|x| x == s) {
                    site.push(s.to_string());
                }
            }
            for s in rule.ip_rules {
                if !ip.iter().any(|x| x == s) {
                    ip.push(s.to_string());
                }
            }
        }
    }
    Ok((site, ip))
}

fn remote_rule_set(tag: String, url: String) -> Value {
    obj! { "tag" => tag, "type" => "remote", "format" => "binary", "url" => url }
}

/// `generateRuleSets(selectedRules, customRules)` → (site_rule_sets, ip_rule_sets)
pub fn generate_rule_sets(selected: &Value, custom_rules: &[Value]) -> JsResult<(Vec<Value>, Vec<Value>)> {
    let selected = resolve_selected(selected);
    let (site, ip) = collect_selected_rule_sets(&selected)?;
    let mut site_sets: Vec<Value> =
        site.iter().map(|r| remote_rule_set(r.clone(), format!("{}{}.srs", SITE_RULE_SET_BASE_URL, r))).collect();
    let mut ip_sets: Vec<Value> =
        ip.iter().map(|r| remote_rule_set(format!("{}-ip", r), format!("{}{}.srs", IP_RULE_SET_BASE_URL, r))).collect();
    if !selected_includes(&selected, "Non-China")? {
        site_sets
            .push(remote_rule_set("geolocation-!cn".into(), format!("{}geolocation-!cn.srs", SITE_RULE_SET_BASE_URL)));
    }
    for rule in custom_rules {
        for s in sanitize_rule_ids(custom_rule_field(rule, "site")?) {
            site_sets.push(remote_rule_set(s.clone(), format!("{}{}.srs", SITE_RULE_SET_BASE_URL, s)));
        }
        for s in sanitize_rule_ids(custom_rule_field(rule, "ip")?) {
            ip_sets.push(remote_rule_set(format!("{}-ip", s), format!("{}{}.srs", IP_RULE_SET_BASE_URL, s)));
        }
    }
    Ok((site_sets, ip_sets))
}

fn clash_provider(behavior: &str, format: &str, url: String, path: String) -> Value {
    obj! {
        "type" => "http",
        "format" => format,
        "behavior" => behavior,
        "url" => url,
        "path" => path,
        "interval" => 86400,
    }
}

/// `generateClashRuleSets(selectedRules, customRules, useMrs)` →
/// (site_rule_providers, ip_rule_providers)
pub fn generate_clash_rule_sets(selected: &Value, custom_rules: &[Value], use_mrs: bool) -> JsResult<(Object, Object)> {
    let selected = resolve_selected(selected);
    let format = if use_mrs { "mrs" } else { "yaml" };
    let ext = if use_mrs { ".mrs" } else { ".yaml" };
    let (site, ip) = collect_selected_rule_sets(&selected)?;
    let mut site_providers = Object::new();
    let mut ip_providers = Object::new();
    for r in &site {
        site_providers.set(
            r.clone(),
            clash_provider(
                "domain",
                format,
                format!("{}{}{}", CLASH_SITE_RULE_SET_BASE_URL, r, ext),
                format!("./ruleset/{}{}", r, ext),
            ),
        );
    }
    for r in &ip {
        ip_providers.set(
            format!("{}-ip", r),
            clash_provider(
                "ipcidr",
                format,
                format!("{}{}{}", CLASH_IP_RULE_SET_BASE_URL, r, ext),
                format!("./ruleset/{}-ip{}", r, ext),
            ),
        );
    }
    if !selected_includes(&selected, "Non-China")? {
        site_providers.set(
            "geolocation-!cn",
            clash_provider(
                "domain",
                format,
                format!("{}geolocation-!cn{}", CLASH_SITE_RULE_SET_BASE_URL, ext),
                format!("./ruleset/geolocation-!cn{}", ext),
            ),
        );
    }
    for rule in custom_rules {
        for s in sanitize_rule_ids(custom_rule_field(rule, "site")?) {
            site_providers.set(
                s.clone(),
                clash_provider(
                    "domain",
                    format,
                    format!("{}{}{}", CLASH_SITE_RULE_SET_BASE_URL, s, ext),
                    format!("./ruleset/{}{}", s, ext),
                ),
            );
        }
        for s in sanitize_rule_ids(custom_rule_field(rule, "ip")?) {
            ip_providers.set(
                format!("{}-ip", s),
                clash_provider(
                    "ipcidr",
                    format,
                    format!("{}{}{}", CLASH_IP_RULE_SET_BASE_URL, s, ext),
                    format!("./ruleset/{}-ip{}", s, ext),
                ),
            );
        }
    }
    Ok((site_providers, ip_providers))
}

fn parsed(cell: &'static OnceLock<Value>, text: &'static str) -> &'static Value {
    cell.get_or_init(|| json::parse(text).expect("embedded base config must be valid JSON"))
}

pub fn sing_box_config() -> &'static Value {
    static CELL: OnceLock<Value> = OnceLock::new();
    parsed(&CELL, include_str!("../../assets/base/singbox.json"))
}

pub fn sing_box_config_v1_11() -> &'static Value {
    static CELL: OnceLock<Value> = OnceLock::new();
    parsed(&CELL, include_str!("../../assets/base/singbox-1.11.json"))
}

pub fn clash_config() -> &'static Value {
    static CELL: OnceLock<Value> = OnceLock::new();
    parsed(&CELL, include_str!("../../assets/base/clash.json"))
}

pub fn surge_config() -> &'static Value {
    static CELL: OnceLock<Value> = OnceLock::new();
    parsed(&CELL, include_str!("../../assets/base/surge.json"))
}
