//! Translations (zh-CN / en-US / fa / ru) and the `t(key)` lookup semantics.

use std::sync::OnceLock;

use crate::js::{Value, json};

static TRANSLATIONS_JSON: &str = include_str!("../assets/i18n.json");

pub fn translations() -> &'static Value {
    static PARSED: OnceLock<Value> = OnceLock::new();
    PARSED.get_or_init(|| json::parse(TRANSLATIONS_JSON).expect("assets/i18n.json must be valid JSON"))
}

/// `resolveLanguage(lang)`
pub fn resolve_language(lang: Option<&str>) -> &'static str {
    let table = translations().as_object().expect("translations object");
    if let Some(l) = lang
        && let Some(key) = table.keys().into_iter().find(|k| k.as_str() == l)
    {
        // Map to the static key so the translator can be stored cheaply.
        return match key.as_str() {
            "zh-CN" => "zh-CN",
            "en-US" => "en-US",
            "fa" => "fa",
            "ru" => "ru",
            _ => "zh-CN",
        };
    }
    match lang {
        Some(l) if l.starts_with("en") => "en-US",
        Some(l) if l.starts_with("fa") => "fa",
        Some(l) if l.starts_with("ru") => "ru",
        _ => "zh-CN",
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Translator {
    pub lang: &'static str,
}

impl Translator {
    /// `createTranslator(lang)`
    pub fn new(lang: Option<&str>) -> Self {
        Translator { lang: resolve_language(lang) }
    }

    /// `t(key)`: walks dotted keys; unknown `outboundNames.*` keys fall back to
    /// the segment after the first dot, other unknown keys to the key itself.
    pub fn t(&self, key: &str) -> Value {
        let mut value = translations().get(self.lang);
        for k in key.split('.') {
            value = value.get(k);
            if value.is_undefined() {
                if key.starts_with("outboundNames.") {
                    return Value::String(key.split('.').nth(1).unwrap_or("").to_string());
                }
                return Value::String(key.to_string());
            }
        }
        value.clone()
    }

    /// `t(key)` coerced to a string (template literal semantics).
    pub fn ts(&self, key: &str) -> String {
        self.t(key).to_js_string()
    }

    /// `t('outboundNames.' + name)`
    pub fn outbound(&self, name: &str) -> String {
        self.ts(&format!("outboundNames.{}", name))
    }
}

pub const APP_NAME: &str = "Sublink Worker";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const GITHUB_REPO: &str = "https://github.com/7Sageer/sublink-worker";
pub const GITHUB_API_RELEASES: &str = "https://api.github.com/repos/7Sageer/sublink-worker/releases/latest";
pub const DOCS_URL: &str = "https://sublink.works";
pub const APP_KEYWORDS: &str = "clash, singbox, surge, subscription, converter, sublink";

/// `APP_SUBTITLE[lang] || APP_SUBTITLE['zh-CN']`
pub fn app_subtitle(lang: &str) -> &'static str {
    match lang {
        "en-US" => "Efficiently Aggregate and Manage Your Proxy Nodes",
        "fa" => "تجمیع و مدیریت کارآمد نودهای پروکسی شما",
        "ru" => "Эффективная агрегация и управление вашими прокси-узлами",
        _ => "高效聚合与管理您的代理节点",
    }
}
