//! Server-rendered home page.
//!
//! Templates under `templates/` mirror the original JSX components. They are
//! indented for readability; the whitespace JSX compilation would have dropped
//! is folded away after rendering so the page matches the original output.

use std::fmt::{self, Write};

use askama::Template;

use crate::config::{UNIFIED_RULES, predefined_rule_sets_value};
use crate::i18n::{APP_KEYWORDS, APP_VERSION, Translator, app_subtitle, resolve_language};
use crate::js::string::is_js_whitespace;
use crate::js::{Object, Value, json};

/// Fences around interpolated values so whitespace folding leaves them alone.
const OPEN: char = '\u{E000}';
const CLOSE: char = '\u{E001}';

static FORM_LOGIC: &str = include_str!("../assets/web/form-logic.js");

/// Keys the client script reads from `window.APP_TRANSLATIONS`.
const CLIENT_TRANSLATION_KEYS: &[&str] = &[
    "processing",
    "convert",
    "saveConfigSuccess",
    "saveConfig",
    "savingConfig",
    "configContentRequired",
    "configSaveFailed",
    "confirmClearConfig",
    "confirmClearAll",
    "errorGeneratingLinks",
    "shortenLinks",
    "shortening",
    "alreadyShortened",
    "shortenFailed",
    "customShortCode",
    "optional",
    "customShortCodePlaceholder",
    "showFullLinks",
];

const LINK_FIELDS: &[(&str, &str)] =
    &[("xray", "xrayLink"), ("singbox", "singboxLink"), ("clash", "clashLink"), ("surge", "surgeLink")];

/// Hono's HTML escaping (`&quot;`/`&#39;` rather than askama's numeric forms).
#[derive(Clone, Copy)]
pub struct HonoHtml;

impl askama::filters::Escaper for HonoHtml {
    fn write_escaped_str<W: Write>(&self, mut dest: W, string: &str) -> fmt::Result {
        dest.write_char(OPEN)?;
        for c in string.chars() {
            match c {
                '"' => dest.write_str("&quot;")?,
                '\'' => dest.write_str("&#39;")?,
                '&' => dest.write_str("&amp;")?,
                '<' => dest.write_str("&lt;")?,
                '>' => dest.write_str("&gt;")?,
                _ => dest.write_char(c)?,
            }
        }
        dest.write_char(CLOSE)
    }
}

#[derive(Template)]
#[template(path = "index.html")]
struct Body<'a> {
    tr: Translator,
    subtitle: &'a str,
    year: i64,
    version: &'a str,
    form_script: String,
    rule_names: Vec<&'static str>,
    link_fields: &'static [(&'static str, &'static str)],
}

impl Body<'_> {
    fn t(&self, key: &str) -> String {
        self.tr.ts(key)
    }

    fn outbound(&self, name: &str) -> String {
        self.tr.outbound(name)
    }
}

#[derive(Template)]
#[template(path = "layout.html")]
struct Layout<'a> {
    title: String,
    keywords: &'a str,
    body: String,
}

/// The inline bootstrap script of the form component.
fn form_script(tr: &Translator, lang: &str) -> String {
    let translations: Object = CLIENT_TRANSLATION_KEYS.iter().map(|k| (k.to_string(), tr.t(k))).collect();
    let mut lang_json = String::new();
    json::quote_json_string(lang, &mut lang_json);
    format!(
        "\n    window.APP_TRANSLATIONS = {};\n    window.PREDEFINED_RULE_SETS = {};\n    window.APP_LANG = {};\n    if (typeof __name === 'undefined') {{ var __name = function(fn) {{ return fn; }}; }}\n    ({})();\n  ",
        json::stringify(&Value::Object(translations)).unwrap_or_default(),
        json::stringify(&predefined_rule_sets_value()).unwrap_or_default(),
        lang_json,
        FORM_LOGIC.trim_end(),
    )
}

/// esbuild's JSX text rule: lines are trimmed (keeping the outer edges of the
/// first and last line), blank lines dropped, the rest joined by one space.
fn fold_jsx_text(text: &str, out: &mut String) {
    if !text.contains(['\n', '\r']) {
        out.push_str(text);
        return;
    }
    let lines: Vec<&str> = text.split(['\n', '\r']).collect();
    let last = lines.len() - 1;
    let mut first_piece = true;
    for (i, line) in lines.iter().enumerate() {
        if line.trim_matches(is_js_whitespace).is_empty() {
            continue;
        }
        let piece = match i {
            0 => line.trim_end_matches(is_js_whitespace),
            _ if i == last => line.trim_start_matches(is_js_whitespace),
            _ => line.trim_matches(is_js_whitespace),
        };
        if !first_piece {
            out.push(' ');
        }
        out.push_str(piece);
        first_piece = false;
    }
}

/// Applies JSX whitespace folding to text between tags, leaving tags,
/// `<script>` bodies and fenced values untouched, then drops the fences.
fn fold_jsx_whitespace(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while !rest.is_empty() {
        if rest.starts_with('<') {
            let end = rest.find('>').map_or(rest.len(), |i| i + 1);
            let is_script = rest.starts_with("<script");
            out.push_str(&rest[..end]);
            rest = &rest[end..];
            if is_script {
                let close = rest.find("</script>").unwrap_or(rest.len());
                out.push_str(&rest[..close]);
                rest = &rest[close..];
            }
            continue;
        }
        let end = rest.find('<').unwrap_or(rest.len());
        let mut text = &rest[..end];
        while let Some(open) = text.find(OPEN) {
            fold_jsx_text(&text[..open], &mut out);
            let close = text[open..].find(CLOSE).map_or(text.len(), |i| i + open);
            out.push_str(&text[open + OPEN.len_utf8()..close]);
            text = text.get(close + CLOSE.len_utf8()..).unwrap_or("");
        }
        fold_jsx_text(text, &mut out);
        rest = &rest[end..];
    }
    out.retain(|c| c != OPEN && c != CLOSE);
    out
}

/// `GET /` for the request's language (raw `lang` value, resolved here).
pub fn index(lang: &str) -> String {
    let tr = Translator::new(Some(lang));
    let resolved = resolve_language(Some(lang));
    let body = Body {
        tr,
        subtitle: app_subtitle(resolved),
        year: crate::js::date::current_year(),
        version: APP_VERSION,
        form_script: form_script(&tr, resolved),
        rule_names: UNIFIED_RULES.iter().map(|r| r.name).collect(),
        link_fields: LINK_FIELDS,
    };
    let body = fold_jsx_whitespace(&body.render().expect("index template renders"));
    let layout = Layout { title: tr.ts("pageTitle"), keywords: APP_KEYWORDS, body };
    let mut page = layout.render().expect("layout template renders");
    page.retain(|c| c != OPEN && c != CLOSE);
    page
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_like_esbuild() {
        let mut out = String::new();
        fold_jsx_text("\n    Hello\n    world  \n  ", &mut out);
        assert_eq!(out, "Hello world");
        out.clear();
        fold_jsx_text("  a  \n  b  ", &mut out);
        assert_eq!(out, "  a b  ");
        out.clear();
        fold_jsx_text(" ", &mut out);
        assert_eq!(out, " ");
        assert_eq!(fold_jsx_whitespace("<a>\n  <b>x</b>\n  \u{E000} y \u{E001}\n</a>"), "<a><b>x</b> y </a>");
    }
}
