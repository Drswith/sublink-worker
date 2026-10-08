//! Port of js-yaml 4.3.0 `dumper.js` with default options (indent 2,
//! lineWidth 80, single-quote preference, compat mode on).
//!
//! Objects/arrays referenced more than once are emitted with `&ref_N` /
//! `*ref_N` exactly like js-yaml (identity comes from shared `Value` storage).

use super::load::is_implicitly_typed;
use crate::js::date::to_iso_string;
use crate::js::number::{is_negative_zero, js_number_to_string};
use crate::js::{JsError, Value};

const INDENT: usize = 2;
const LINE_WIDTH: isize = 80;

const CHAR_TAB: u32 = 0x09;
const CHAR_LINE_FEED: u32 = 0x0A;
const CHAR_SPACE: u32 = 0x20;
const CHAR_BOM: u32 = 0xFEFF;

const STYLE_PLAIN: u8 = 1;
const STYLE_SINGLE: u8 = 2;
const STYLE_LITERAL: u8 = 3;
const STYLE_FOLDED: u8 = 4;
const STYLE_DOUBLE: u8 = 5;

const DEPRECATED_BOOLEANS_SYNTAX: [&str; 16] =
    ["y", "Y", "yes", "Yes", "YES", "on", "On", "ON", "n", "N", "no", "No", "NO", "off", "Off", "OFF"];

fn escape_sequence(c: u32) -> Option<&'static str> {
    Some(match c {
        0x00 => "\\0",
        0x07 => "\\a",
        0x08 => "\\b",
        0x09 => "\\t",
        0x0A => "\\n",
        0x0B => "\\v",
        0x0C => "\\f",
        0x0D => "\\r",
        0x1B => "\\e",
        0x22 => "\\\"",
        0x5C => "\\\\",
        0x85 => "\\N",
        0xA0 => "\\_",
        0x2028 => "\\L",
        0x2029 => "\\P",
        _ => return None,
    })
}

/// `/^[-+]?[0-9_]+(?::[0-9_]+)+(?:\.[0-9_]*)?$/`
fn is_deprecated_base60(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
        i += 1;
    }
    let digit = |c: u8| c.is_ascii_digit() || c == b'_';
    let start = i;
    while i < b.len() && digit(b[i]) {
        i += 1;
    }
    if i == start {
        return false;
    }
    let mut groups = 0;
    while i < b.len() && b[i] == b':' {
        let gs = i + 1;
        let mut j = gs;
        while j < b.len() && digit(b[j]) {
            j += 1;
        }
        if j == gs {
            break;
        }
        groups += 1;
        i = j;
    }
    if groups == 0 {
        return false;
    }
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && digit(b[i]) {
            i += 1;
        }
    }
    i == b.len()
}

fn encode_hex(c: u32) -> String {
    let s = format!("{:X}", c);
    if c <= 0xFF {
        format!("\\x{}{}", "0".repeat(2 - s.len()), s)
    } else if c <= 0xFFFF {
        format!("\\u{}{}", "0".repeat(4 - s.len()), s)
    } else {
        format!("\\U{}{}", "0".repeat(8 - s.len()), s)
    }
}

fn generate_next_line(level: usize) -> String {
    format!("\n{}", " ".repeat(INDENT * level))
}

fn is_whitespace(c: u32) -> bool {
    c == CHAR_SPACE || c == CHAR_TAB
}

fn is_printable(c: u32) -> bool {
    (0x20..=0x7E).contains(&c)
        || ((0xA1..=0xD7FF).contains(&c) && c != 0x2028 && c != 0x2029)
        || ((0xE000..=0xFFFD).contains(&c) && c != CHAR_BOM)
        || (0x10000..=0x10FFFF).contains(&c)
}

fn is_ns_char_or_whitespace(c: u32) -> bool {
    is_printable(c) && c != CHAR_BOM && c != 0x0D && c != CHAR_LINE_FEED
}

fn is_plain_safe(c: u32, prev: Option<u32>, inblock: bool) -> bool {
    let c_is_ns_char_or_ws = is_ns_char_or_whitespace(c);
    let c_is_ns_char = c_is_ns_char_or_ws && !is_whitespace(c);
    let prev_is_colon = prev == Some(0x3A);
    ((if inblock { c_is_ns_char_or_ws } else { c_is_ns_char_or_ws && !matches!(c, 0x2C | 0x5B | 0x5D | 0x7B | 0x7D) })
        && c != 0x23
        && !(prev_is_colon && !c_is_ns_char))
        || (prev.is_some_and(|p| is_ns_char_or_whitespace(p) && !is_whitespace(p)) && c == 0x23)
        || (prev_is_colon && c_is_ns_char)
}

fn is_plain_safe_first(c: u32) -> bool {
    is_printable(c)
        && c != CHAR_BOM
        && !is_whitespace(c)
        && !matches!(
            c,
            0x2D | 0x3F
                | 0x3A
                | 0x2C
                | 0x5B
                | 0x5D
                | 0x7B
                | 0x7D
                | 0x23
                | 0x26
                | 0x2A
                | 0x21
                | 0x7C
                | 0x3D
                | 0x3E
                | 0x27
                | 0x22
                | 0x25
                | 0x40
                | 0x60
        )
}

fn is_plain_safe_last(c: u32) -> bool {
    !is_whitespace(c) && c != 0x3A
}

fn code_point_at(s: &[u16], pos: usize) -> u32 {
    let first = s[pos] as u32;
    if (0xD800..=0xDBFF).contains(&first) && pos + 1 < s.len() {
        let second = s[pos + 1] as u32;
        if (0xDC00..=0xDFFF).contains(&second) {
            return (first - 0xD800) * 0x400 + second - 0xDC00 + 0x10000;
        }
    }
    first
}

/// `/^\n* /`
fn need_indent_indicator(s: &[u16]) -> bool {
    let mut i = 0;
    while i < s.len() && s[i] == 0x0A {
        i += 1;
    }
    s.get(i) == Some(&0x20)
}

fn choose_scalar_style(s: &[u16], single_line_only: bool, line_width: isize, inblock: bool) -> u8 {
    let mut has_line_break = false;
    let mut has_foldable_line = false;
    let should_track_width = line_width != -1;
    let mut previous_line_break: isize = -1;
    let mut plain = is_plain_safe_first(code_point_at(s, 0)) && is_plain_safe_last(code_point_at(s, s.len() - 1));
    let mut prev: Option<u32> = None;
    let mut i = 0usize;
    if single_line_only {
        while i < s.len() {
            let c = code_point_at(s, i);
            if !is_printable(c) {
                return STYLE_DOUBLE;
            }
            plain = plain && is_plain_safe(c, prev, inblock);
            prev = Some(c);
            i += if c >= 0x10000 { 2 } else { 1 };
        }
    } else {
        while i < s.len() {
            let c = code_point_at(s, i);
            if c == CHAR_LINE_FEED {
                has_line_break = true;
                if should_track_width {
                    has_foldable_line = has_foldable_line
                        || (i as isize - previous_line_break - 1 > line_width
                            && s.get((previous_line_break + 1) as usize) != Some(&0x20));
                    previous_line_break = i as isize;
                }
            } else if !is_printable(c) {
                return STYLE_DOUBLE;
            }
            plain = plain && is_plain_safe(c, prev, inblock);
            prev = Some(c);
            i += if c >= 0x10000 { 2 } else { 1 };
        }
        has_foldable_line = has_foldable_line
            || (should_track_width
                && (i as isize - previous_line_break - 1 > line_width
                    && s.get((previous_line_break + 1) as usize) != Some(&0x20)));
    }
    if !has_line_break && !has_foldable_line {
        let text = String::from_utf16_lossy(s);
        if plain && !is_implicitly_typed(&text) {
            return STYLE_PLAIN;
        }
        return STYLE_SINGLE;
    }
    if INDENT > 9 && need_indent_indicator(s) {
        return STYLE_DOUBLE;
    }
    if has_foldable_line { STYLE_FOLDED } else { STYLE_LITERAL }
}

fn indent_string(s: &[u16], spaces: usize) -> Vec<u16> {
    let ind: Vec<u16> = vec![0x20; spaces];
    let mut out = Vec::with_capacity(s.len());
    let mut position = 0;
    while position < s.len() {
        let next = s[position..].iter().position(|&c| c == 0x0A).map(|p| p + position);
        let line = match next {
            None => {
                let l = &s[position..];
                position = s.len();
                l
            }
            Some(n) => {
                let l = &s[position..=n];
                position = n + 1;
                l
            }
        };
        if !line.is_empty() && line != [0x0A] {
            out.extend_from_slice(&ind);
        }
        out.extend_from_slice(line);
    }
    out
}

fn block_header(s: &[u16]) -> String {
    let indent_indicator = if need_indent_indicator(s) { INDENT.to_string() } else { String::new() };
    let clip = s.last() == Some(&0x0A);
    let keep = clip && ((s.len() >= 2 && s[s.len() - 2] == 0x0A) || s == [0x0A]);
    let chomp = if keep {
        "+"
    } else if clip {
        ""
    } else {
        "-"
    };
    format!("{}{}\n", indent_indicator, chomp)
}

fn drop_ending_newline(mut s: Vec<u16>) -> Vec<u16> {
    if s.last() == Some(&0x0A) {
        s.pop();
    }
    s
}

fn fold_line(line: &[u16], width: isize) -> Vec<u16> {
    if line.is_empty() || line[0] == 0x20 {
        return line.to_vec();
    }
    // matches of / [^ ]/g
    let mut start: usize = 0;
    let mut curr: usize = 0;
    let mut result: Vec<u16> = Vec::new();
    let mut idx = 0;
    while idx + 1 < line.len() {
        if line[idx] == 0x20 && line[idx + 1] != 0x20 {
            let next = idx;
            if next as isize - start as isize > width {
                let end = if curr > start { curr } else { next };
                result.push(0x0A);
                result.extend_from_slice(&line[start..end]);
                start = end + 1;
            }
            curr = next;
            idx += 2;
        } else {
            idx += 1;
        }
    }
    result.push(0x0A);
    if line.len() as isize - start as isize > width && curr > start {
        result.extend_from_slice(&line[start..curr]);
        result.push(0x0A);
        result.extend_from_slice(&line[curr + 1..]);
    } else {
        result.extend_from_slice(&line[start..]);
    }
    result[1..].to_vec()
}

fn fold_string(s: &[u16], width: isize) -> Vec<u16> {
    let first_lf = s.iter().position(|&c| c == 0x0A).unwrap_or(s.len());
    let mut result = fold_line(&s[..first_lf], width);
    let mut prev_more_indented = s.first() == Some(&0x0A) || s.first() == Some(&0x20);
    // /(\n+)([^\n]*)/g starting at first_lf
    let mut pos = first_lf;
    while pos < s.len() {
        let prefix_start = pos;
        while pos < s.len() && s[pos] == 0x0A {
            pos += 1;
        }
        if pos == prefix_start {
            break;
        }
        let prefix = &s[prefix_start..pos];
        let line_start = pos;
        while pos < s.len() && s[pos] != 0x0A {
            pos += 1;
        }
        let line = &s[line_start..pos];
        let more_indented = line.first() == Some(&0x20);
        result.extend_from_slice(prefix);
        if !prev_more_indented && !more_indented && !line.is_empty() {
            result.push(0x0A);
        }
        result.extend(fold_line(line, width));
        prev_more_indented = more_indented;
    }
    result
}

fn escape_string(s: &[u16]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < s.len() {
        let c = code_point_at(s, i);
        let step = if c >= 0x10000 { 2 } else { 1 };
        match escape_sequence(c) {
            None if is_printable(c) => out.push_str(&String::from_utf16_lossy(&s[i..i + step])),
            Some(seq) => out.push_str(seq),
            None => out.push_str(&encode_hex(c)),
        }
        i += step;
    }
    out
}

fn write_scalar(text: &str, level: usize, iskey: bool, inblock: bool) -> String {
    if text.is_empty() {
        return "''".into();
    }
    if DEPRECATED_BOOLEANS_SYNTAX.contains(&text) || is_deprecated_base60(text) {
        return format!("'{}'", text);
    }
    let s: Vec<u16> = text.encode_utf16().collect();
    let indent = INDENT * level.max(1);
    let line_width = (LINE_WIDTH.min(40)).max(LINE_WIDTH - indent as isize);
    match choose_scalar_style(&s, iskey, line_width, inblock) {
        STYLE_PLAIN => text.to_string(),
        STYLE_SINGLE => format!("'{}'", text.replace('\'', "''")),
        STYLE_LITERAL => {
            let body = drop_ending_newline(indent_string(&s, indent));
            format!("|{}{}", block_header(&s), String::from_utf16_lossy(&body))
        }
        STYLE_FOLDED => {
            let body = drop_ending_newline(indent_string(&fold_string(&s, line_width), indent));
            format!(">{}{}", block_header(&s), String::from_utf16_lossy(&body))
        }
        _ => format!("\"{}\"", escape_string(&s)),
    }
}

fn represent_float(n: f64) -> String {
    if n.is_nan() {
        return ".nan".into();
    }
    if n == f64::INFINITY {
        return ".inf".into();
    }
    if n == f64::NEG_INFINITY {
        return "-.inf".into();
    }
    if is_negative_zero(n) {
        return "-0.0".into();
    }
    let res = js_number_to_string(n);
    // /^[-+]?[0-9]+e/ -> insert a dot so it still reads as a float
    let digits = res.trim_start_matches(['-', '+']);
    let int_len = digits.bytes().take_while(u8::is_ascii_digit).count();
    if int_len > 0 && digits.as_bytes().get(int_len) == Some(&b'e') {
        return res.replacen('e', ".e", 1);
    }
    res
}

struct DumpState {
    duplicates: Vec<usize>,
    used: Vec<bool>,
}

fn identity(v: &Value) -> Option<usize> {
    match v {
        Value::Object(o) => Some(o.identity()),
        Value::Array(a) => Some(a.identity()),
        _ => None,
    }
}

/// `getDuplicateReferences`: identities seen more than once, in detection order.
fn duplicate_references(root: &Value) -> Vec<usize> {
    fn inspect(
        v: &Value,
        objects: &mut Vec<usize>,
        index: &mut std::collections::HashMap<usize, usize>,
        dups: &mut Vec<usize>,
    ) {
        let Some(id) = identity(v) else { return };
        if let Some(&i) = index.get(&id) {
            if !dups.contains(&i) {
                dups.push(i);
            }
            return;
        }
        index.insert(id, objects.len());
        objects.push(id);
        match v {
            Value::Array(items) => {
                for item in items.iter() {
                    inspect(item, objects, index, dups);
                }
            }
            Value::Object(o) => {
                for (_, val) in o.entries() {
                    inspect(val, objects, index, dups);
                }
            }
            _ => {}
        }
    }
    let mut objects = Vec::new();
    let mut index = std::collections::HashMap::new();
    let mut dups = Vec::new();
    inspect(root, &mut objects, &mut index, &mut dups);
    dups.into_iter().map(|i| objects[i]).collect()
}

/// Returns the rendered node, or None for values js-yaml skips (undefined).
fn write_node(
    st: &mut DumpState,
    level: usize,
    value: &Value,
    block: bool,
    compact: bool,
    iskey: bool,
) -> Result<Option<String>, JsError> {
    let inblock = block;
    let dup = identity(value).and_then(|id| st.duplicates.iter().position(|d| *d == id));
    let compact = compact && dup.is_none();
    if let Some(i) = dup {
        if st.used[i] {
            return Ok(Some(format!("*ref_{}", i)));
        }
        st.used[i] = true;
    }
    let anchor = |dump: String, block_form: bool| -> String {
        match dup {
            Some(i) if block_form => format!("&ref_{}{}", i, dump),
            Some(i) => format!("&ref_{} {}", i, dump),
            None => dump,
        }
    };
    Ok(Some(match value {
        Value::Undefined => return Ok(None),
        Value::Null => "null".into(),
        Value::Bool(b) => {
            if *b {
                "true".into()
            } else {
                "false".into()
            }
        }
        Value::Number(n) => {
            if n.fract() == 0.0 && n.is_finite() && !is_negative_zero(*n) {
                js_number_to_string(*n)
            } else {
                represent_float(*n)
            }
        }
        Value::Date(t) => to_iso_string(*t)
            .ok_or_else(|| JsError { kind: crate::js::ErrorKind::RangeError, message: "Invalid time value".into() })?,
        Value::String(s) => write_scalar(s, level, iskey, inblock),
        Value::Object(obj) => {
            if block && !obj.is_empty() {
                anchor(write_block_mapping(st, level, value, compact)?, true)
            } else {
                anchor(write_flow_mapping(st, level, value)?, false)
            }
        }
        Value::Array(items) => {
            if block && !items.is_empty() {
                anchor(write_block_sequence(st, level, items, compact)?, true)
            } else {
                anchor(write_flow_sequence(st, level, items)?, false)
            }
        }
    }))
}

fn write_flow_sequence(st: &mut DumpState, level: usize, items: &[Value]) -> Result<String, JsError> {
    let mut result = String::new();
    for item in items {
        let dumped = match write_node(st, level, item, false, false, false)? {
            Some(d) => Some(d),
            None if item.is_undefined() => write_node(st, level, &Value::Null, false, false, false)?,
            None => None,
        };
        if let Some(d) = dumped {
            if !result.is_empty() {
                result.push_str(", ");
            }
            result.push_str(&d);
        }
    }
    Ok(format!("[{}]", result))
}

fn write_block_sequence(st: &mut DumpState, level: usize, items: &[Value], compact: bool) -> Result<String, JsError> {
    let mut result = String::new();
    for item in items {
        let dumped = match write_node(st, level + 1, item, true, true, false)? {
            Some(d) => Some(d),
            None if item.is_undefined() => write_node(st, level + 1, &Value::Null, true, true, false)?,
            None => None,
        };
        if let Some(d) = dumped {
            if !compact || !result.is_empty() {
                result.push_str(&generate_next_line(level));
            }
            result.push_str(if d.starts_with('\n') { "-" } else { "- " });
            result.push_str(&d);
        }
    }
    Ok(if result.is_empty() { "[]".into() } else { result })
}

fn write_flow_mapping(st: &mut DumpState, level: usize, value: &Value) -> Result<String, JsError> {
    let Value::Object(obj) = value else { return Ok("{}".into()) };
    let mut result = String::new();
    for (key, val) in obj.entries() {
        let mut pair = String::new();
        if !result.is_empty() {
            pair.push_str(", ");
        }
        let Some(k) = write_node(st, level, &Value::String(key.clone()), false, false, false)? else { continue };
        if k.encode_utf16().count() > 1024 {
            pair.push_str("? ");
        }
        pair.push_str(&k);
        pair.push_str(": ");
        let Some(v) = write_node(st, level, val, false, false, false)? else { continue };
        pair.push_str(&v);
        result.push_str(&pair);
    }
    Ok(format!("{{{}}}", result))
}

fn write_block_mapping(st: &mut DumpState, level: usize, value: &Value, compact: bool) -> Result<String, JsError> {
    let Value::Object(obj) = value else { return Ok("{}".into()) };
    let mut result = String::new();
    for (key, val) in obj.entries() {
        let mut pair = String::new();
        if !compact || !result.is_empty() {
            pair.push_str(&generate_next_line(level));
        }
        let Some(k) = write_node(st, level + 1, &Value::String(key.clone()), true, true, true)? else { continue };
        let explicit_pair = k.encode_utf16().count() > 1024;
        if explicit_pair {
            pair.push_str(if k.starts_with('\n') { "?" } else { "? " });
        }
        pair.push_str(&k);
        if explicit_pair {
            pair.push_str(&generate_next_line(level));
        }
        let Some(v) = write_node(st, level + 1, val, true, explicit_pair, false)? else { continue };
        pair.push_str(if v.starts_with('\n') { ":" } else { ": " });
        pair.push_str(&v);
        result.push_str(&pair);
    }
    Ok(if result.is_empty() { "{}".into() } else { result })
}

/// `yaml.dump(value)`
pub fn dump(value: &Value) -> Result<String, JsError> {
    let duplicates = duplicate_references(value);
    let mut st = DumpState { used: vec![false; duplicates.len()], duplicates };
    Ok(match write_node(&mut st, 0, value, true, true, false)? {
        Some(s) => s + "\n",
        None => String::new(),
    })
}
