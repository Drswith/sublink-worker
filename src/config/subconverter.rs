//! Subconverter external config (INI) generator.

use super::{generate_rules, is_direct_default_rule};
use crate::i18n::Translator;
use crate::js::{JsResult, Value};
use crate::utils::COUNTRY_DATA;

const SPEED_TEST_URL: &str = "http://www.gstatic.com/generate_204";

fn escape_regex(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if "-/\\^$*+?.()|[]{}".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn country_group_refs(names: &[String]) -> String {
    names.iter().map(|n| format!("[]{}", n)).collect::<Vec<_>>().join("`")
}

pub struct SubconverterOptions<'a> {
    pub selected_rules: Value,
    pub custom_rules: &'a mut Vec<Value>,
    pub lang: Option<&'a str>,
    pub include_auto_select: bool,
    pub group_by_country: bool,
}

/// `generateSubconverterConfig(options)`
pub fn generate_subconverter_config(opts: SubconverterOptions<'_>) -> JsResult<String> {
    let t = Translator::new(opts.lang);
    let rules = generate_rules(&opts.selected_rules, opts.custom_rules)?;
    let include_auto_select = opts.include_auto_select;
    let group_by_country = opts.group_by_country;
    let mut lines: Vec<String> = vec!["[custom]".into()];

    for rule in &rules {
        let group = t.outbound(&rule.outbound_str());
        for cidr in rule.src_ip_cidr.iter().flatten() {
            if !cidr.is_empty() {
                lines.push(format!("ruleset={},[]SRC-IP-CIDR,{}", group, cidr));
            }
        }
    }
    for rule in &rules {
        let group = t.outbound(&rule.outbound_str());
        for suffix in rule.domain_suffix.iter().flatten() {
            if !suffix.is_empty() {
                lines.push(format!("ruleset={},[]DOMAIN-SUFFIX,{}", group, suffix));
            }
        }
        for keyword in rule.domain_keyword.iter().flatten() {
            if !keyword.is_empty() {
                lines.push(format!("ruleset={},[]DOMAIN-KEYWORD,{}", group, keyword));
            }
        }
        for site in &rule.site_rules {
            if !site.is_empty() {
                lines.push(format!("ruleset={},[]GEOSITE,{}", group, site));
            }
        }
    }
    for rule in &rules {
        let group = t.outbound(&rule.outbound_str());
        for ip in &rule.ip_rules {
            if !ip.is_empty() {
                lines.push(format!("ruleset={},[]GEOIP,{}", group, ip));
            }
        }
        for cidr in rule.ip_cidr.iter().flatten() {
            if !cidr.is_empty() {
                lines.push(format!("ruleset={},[]IP-CIDR,{}", group, cidr));
            }
        }
    }

    let fall_back = t.ts("outboundNames.Fall Back");
    lines.push(format!("ruleset={},[]FINAL", fall_back));
    lines.push(String::new());

    let node_select = t.ts("outboundNames.Node Select");
    let auto_select = t.ts("outboundNames.Auto Select");
    let manual_switch = t.ts("outboundNames.Manual Switch");

    let mut country_names: Vec<String> = Vec::new();
    let mut country_lines: Vec<String> = Vec::new();
    if group_by_country {
        for country in COUNTRY_DATA {
            let name = format!("{} {}", country.emoji, country.name);
            let regex = country
                .aliases
                .iter()
                .map(|a| {
                    let escaped = escape_regex(a);
                    let ascii_words = !a.is_empty()
                        && a.chars().all(|c| c.is_ascii_alphabetic() || crate::js::string::is_js_whitespace(c));
                    if ascii_words { format!("\\b{}\\b", escaped) } else { escaped }
                })
                .collect::<Vec<_>>()
                .join("|");
            country_lines
                .push(format!("custom_proxy_group={}`url-test`(?i)({})`{}`300,,50", name, regex, SPEED_TEST_URL));
            country_names.push(name);
        }
    }

    // Selector members shared by Node Select-like groups.
    let selector_members = |with_node_select: bool| -> String {
        let prefix = if with_node_select { format!("[]{}`", node_select) } else { String::new() };
        if group_by_country {
            let refs = country_group_refs(&country_names);
            if include_auto_select {
                format!("{}[]{}`[]{}`{}`[]DIRECT", prefix, auto_select, manual_switch, refs)
            } else {
                format!("{}[]{}`{}`[]DIRECT", prefix, manual_switch, refs)
            }
        } else if include_auto_select {
            format!("{}[]{}`[]DIRECT`.*", prefix, auto_select)
        } else {
            format!("{}[]DIRECT`.*", prefix)
        }
    };

    lines.push(format!("custom_proxy_group={}`select`{}", node_select, selector_members(false)));
    if include_auto_select {
        lines.push(format!("custom_proxy_group={}`url-test`.*`{}`300,,50", auto_select, SPEED_TEST_URL));
    }
    if group_by_country {
        lines.push(format!("custom_proxy_group={}`select`.*", manual_switch));
    }
    lines.extend(country_lines);

    let mut processed: Vec<String> = vec![node_select.clone()];
    if include_auto_select {
        processed.push(auto_select.clone());
    }
    if group_by_country {
        processed.push(manual_switch.clone());
        processed.extend(country_names.iter().cloned());
    }

    for rule in &rules {
        let group = t.outbound(&rule.outbound_str());
        if processed.contains(&group) {
            continue;
        }
        processed.push(group.clone());
        if rule.outbound.as_str() == Some("Ad Block") {
            lines.push(format!("custom_proxy_group={}`select`[]REJECT`[]DIRECT", group));
        } else if is_direct_default_rule(&rule.outbound) {
            lines.push(format!("custom_proxy_group={}`select`[]DIRECT`[]{}", group, node_select));
        } else {
            lines.push(format!("custom_proxy_group={}`select`{}", group, selector_members(true)));
        }
    }

    if !processed.contains(&fall_back) {
        lines.push(format!("custom_proxy_group={}`select`{}", fall_back, selector_members(true)));
    }

    lines.push(String::new());
    lines.push("enable_rule_generator=true".into());
    lines.push("overwrite_original_rules=true".into());
    Ok(lines.join("\n"))
}
