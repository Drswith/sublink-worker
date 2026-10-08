//! Line-by-line port of js-yaml 4.3.0 `loader.js` (DEFAULT_SCHEMA).
//!
//! Clash subscriptions are YAML, and whether a document is accepted (duplicate
//! keys, tabs, anchors, implicit typing) changes the conversion result, so the
//! parser mirrors js-yaml instead of using a generic YAML crate. It operates on
//! UTF-16 code units like the original so error positions match.

use std::collections::HashMap;

use indexmap::IndexMap;

use super::snippet::make_snippet;
use crate::js::date::date_utc;
use crate::js::number::parse_int;
use crate::js::string::decode_uri_component;
use crate::js::{Object, Value};

const CONTEXT_FLOW_IN: u8 = 1;
const CONTEXT_FLOW_OUT: u8 = 2;
const CONTEXT_BLOCK_IN: u8 = 3;
const CONTEXT_BLOCK_OUT: u8 = 4;

const CHOMPING_CLIP: u8 = 1;
const CHOMPING_STRIP: u8 = 2;
const CHOMPING_KEEP: u8 = 3;

const MAX_DEPTH: usize = 100;
const MAX_TOTAL_MERGE_KEYS: usize = 10000;

#[derive(Clone, Debug)]
pub struct YamlError {
    pub reason: String,
    pub message: String,
}

impl std::fmt::Display for YamlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

type R<T> = Result<T, YamlError>;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Scalar,
    Sequence,
    Mapping,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Scalar => "scalar",
            Kind::Sequence => "sequence",
            Kind::Mapping => "mapping",
        }
    }
}

fn is_eol(c: i32) -> bool {
    c == 0x0A || c == 0x0D
}
fn is_white_space(c: i32) -> bool {
    c == 0x09 || c == 0x20
}
fn is_ws_or_eol(c: i32) -> bool {
    c == 0x09 || c == 0x20 || c == 0x0A || c == 0x0D
}
fn is_flow_indicator(c: i32) -> bool {
    c == 0x2C || c == 0x5B || c == 0x5D || c == 0x7B || c == 0x7D
}
fn from_hex_code(c: i32) -> i32 {
    if (0x30..=0x39).contains(&c) {
        return c - 0x30;
    }
    let lc = c | 0x20;
    if (0x61..=0x66).contains(&lc) {
        return lc - 0x61 + 10;
    }
    -1
}
fn escaped_hex_len(c: i32) -> u32 {
    match c {
        0x78 => 2,
        0x75 => 4,
        0x55 => 8,
        _ => 0,
    }
}
fn from_decimal_code(c: i32) -> i32 {
    if (0x30..=0x39).contains(&c) { c - 0x30 } else { -1 }
}
fn simple_escape_sequence(c: i32) -> Option<&'static [u16]> {
    Some(match c {
        0x30 => &[0x00],
        0x61 => &[0x07],
        0x62 => &[0x08],
        0x74 | 0x09 => &[0x09],
        0x6E => &[0x0A],
        0x76 => &[0x0B],
        0x66 => &[0x0C],
        0x72 => &[0x0D],
        0x65 => &[0x1B],
        0x20 => &[0x20],
        0x22 => &[0x22],
        0x2F => &[0x2F],
        0x5C => &[0x5C],
        0x4E => &[0x85],
        0x5F => &[0xA0],
        0x4C => &[0x2028],
        0x50 => &[0x2029],
        _ => return None,
    })
}

fn char_from_codepoint(c: u32) -> Vec<u16> {
    if c <= 0xFFFF {
        return vec![c as u16];
    }
    vec![(((c - 0x10000) >> 10) + 0xD800) as u16, (((c - 0x10000) & 0x3FF) + 0xDC00) as u16]
}

fn u16s(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn to_string(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

fn is_non_printable(seg: &[u16]) -> bool {
    for (i, &c) in seg.iter().enumerate() {
        let c = c as u32;
        if c <= 0x08
            || c == 0x0B
            || c == 0x0C
            || (0x0E..=0x1F).contains(&c)
            || (0x7F..=0x84).contains(&c)
            || (0x86..=0x9F).contains(&c)
            || c == 0xFFFE
            || c == 0xFFFF
        {
            return true;
        }
        if (0xD800..=0xDBFF).contains(&c) {
            let next = seg.get(i + 1).copied().unwrap_or(0) as u32;
            if !(0xDC00..=0xDFFF).contains(&next) {
                return true;
            }
        }
        if (0xDC00..=0xDFFF).contains(&c) {
            let prev = if i == 0 { None } else { Some(seg[i - 1] as u32) };
            if prev.is_none_or(|p| !(0xD800..=0xDBFF).contains(&p)) {
                return true;
            }
        }
    }
    false
}

fn is_tag_handle(s: &str) -> bool {
    if s == "!" || s == "!!" {
        return true;
    }
    s.len() > 2
        && s.starts_with('!')
        && s.ends_with('!')
        && s[1..s.len() - 1].chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn is_tag_uri(s: &str) -> bool {
    let chars: Vec<char> = s.chars().collect();
    if chars.is_empty() {
        return false;
    }
    let first_ok = chars[0] == '!' || !matches!(chars[0], ',' | '[' | ']' | '{' | '}');
    if !first_ok {
        return false;
    }
    let mut i = 1;
    while i < chars.len() {
        let c = chars[i];
        if c == '%' {
            if i + 2 < chars.len()
                && chars.get(i + 1).is_some_and(|h| h.is_ascii_hexdigit())
                && chars.get(i + 2).is_some_and(|h| h.is_ascii_hexdigit())
            {
                i += 3;
                continue;
            }
            return false;
        }
        let ok = c.is_ascii_alphanumeric() || "-#;/?:@&=+$,_.!~*'()[]".contains(c);
        if !ok {
            return false;
        }
        i += 1;
    }
    true
}

#[derive(Clone)]
struct Snapshot {
    position: usize,
    line: usize,
    line_start: usize,
    line_indent: isize,
    first_tab_in_line: isize,
    tag: Option<String>,
    anchor: Option<String>,
    kind: Option<Kind>,
    result: Value,
}

struct State {
    input: Vec<u16>,
    length: usize,
    position: usize,
    line: usize,
    line_start: usize,
    line_indent: isize,
    depth: usize,
    total_merge_keys: usize,
    first_tab_in_line: isize,
    documents: Vec<Value>,
    anchor_map_transactions: Vec<IndexMap<String, (bool, Option<Value>)>>,
    version: Option<String>,
    tag_map: HashMap<String, String>,
    anchor_map: HashMap<String, Value>,
    tag: Option<String>,
    anchor: Option<String>,
    kind: Option<Kind>,
    result: Value,
}

impl State {
    fn ch(&self, pos: usize) -> i32 {
        self.input.get(pos).map(|&c| c as i32).unwrap_or(-1)
    }

    fn cur(&self) -> i32 {
        self.ch(self.position)
    }

    fn adv(&mut self) -> i32 {
        self.position += 1;
        self.cur()
    }

    fn error(&self, message: &str) -> YamlError {
        let buffer = &self.input[..self.input.len().saturating_sub(1)];
        let column = self.position as isize - self.line_start as isize;
        let snippet = make_snippet(buffer, self.position, self.line, column);
        let mut full = format!("{} ({}:{})", message, self.line + 1, column + 1);
        if let Some(snippet) = snippet {
            full.push_str("\n\n");
            full.push_str(&snippet);
        }
        YamlError { reason: message.to_string(), message: full }
    }

    fn fail<T>(&self, message: &str) -> R<T> {
        Err(self.error(message))
    }

    fn result_append(&mut self, units: &[u16]) {
        if let Value::String(s) = &mut self.result {
            s.push_str(&to_string(units));
        } else {
            let mut s = self.result_as_string();
            s.push_str(&to_string(units));
            self.result = Value::String(s);
        }
    }

    fn result_as_string(&self) -> String {
        match &self.result {
            Value::String(s) => s.clone(),
            Value::Null => "null".into(),
            other => other.to_js_string(),
        }
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            position: self.position,
            line: self.line,
            line_start: self.line_start,
            line_indent: self.line_indent,
            first_tab_in_line: self.first_tab_in_line,
            tag: self.tag.clone(),
            anchor: self.anchor.clone(),
            kind: self.kind,
            result: self.result.clone(),
        }
    }

    fn restore(&mut self, s: Snapshot) {
        self.position = s.position;
        self.line = s.line;
        self.line_start = s.line_start;
        self.line_indent = s.line_indent;
        self.first_tab_in_line = s.first_tab_in_line;
        self.tag = s.tag;
        self.anchor = s.anchor;
        self.kind = s.kind;
        self.result = s.result;
    }

    fn store_anchor(&mut self, name: &str, value: Value) {
        if let Some(tx) = self.anchor_map_transactions.last_mut()
            && !tx.contains_key(name)
        {
            let existed = self.anchor_map.contains_key(name);
            let prev = self.anchor_map.get(name).cloned();
            tx.insert(name.to_string(), (existed, prev));
        }
        self.anchor_map.insert(name.to_string(), value);
    }

    fn begin_anchor_transaction(&mut self) {
        self.anchor_map_transactions.push(IndexMap::new());
    }

    fn commit_anchor_transaction(&mut self) {
        let tx = self.anchor_map_transactions.pop().unwrap_or_default();
        if let Some(parent) = self.anchor_map_transactions.last_mut() {
            for (name, entry) in tx {
                parent.entry(name).or_insert(entry);
            }
        }
    }

    fn rollback_anchor_transaction(&mut self) {
        let tx = self.anchor_map_transactions.pop().unwrap_or_default();
        for (name, (existed, prev)) in tx.into_iter().rev() {
            if existed {
                self.anchor_map.insert(name, prev.unwrap_or_default());
            } else {
                self.anchor_map.remove(&name);
            }
        }
    }

    fn capture_segment(&mut self, start: usize, end: usize, check_json: bool) -> R<()> {
        if start < end {
            let seg = self.input[start..end].to_vec();
            if check_json {
                for &c in &seg {
                    if !(c == 0x09 || c >= 0x20) {
                        return self.fail("expected valid JSON character");
                    }
                }
            } else if is_non_printable(&seg) {
                return self.fail("the stream contains non-printable characters");
            }
            self.result_append(&seg);
        }
        Ok(())
    }

    fn read_line_break(&mut self) -> R<()> {
        let ch = self.cur();
        if ch == 0x0A {
            self.position += 1;
        } else if ch == 0x0D {
            self.position += 1;
            if self.cur() == 0x0A {
                self.position += 1;
            }
        } else {
            return self.fail("a line break is expected");
        }
        self.line += 1;
        self.line_start = self.position;
        self.first_tab_in_line = -1;
        Ok(())
    }

    fn skip_separation_space(&mut self, allow_comments: bool, _check_indent: isize) -> R<usize> {
        let mut line_breaks = 0;
        let mut ch = self.cur();
        while ch != 0 {
            while is_white_space(ch) {
                if ch == 0x09 && self.first_tab_in_line == -1 {
                    self.first_tab_in_line = self.position as isize;
                }
                ch = self.adv();
            }
            if allow_comments && ch == 0x23 {
                loop {
                    ch = self.adv();
                    if ch == 0x0A || ch == 0x0D || ch == 0 {
                        break;
                    }
                }
            }
            if is_eol(ch) {
                self.read_line_break()?;
                ch = self.cur();
                line_breaks += 1;
                self.line_indent = 0;
                while ch == 0x20 {
                    self.line_indent += 1;
                    ch = self.adv();
                }
            } else {
                break;
            }
        }
        Ok(line_breaks)
    }

    fn test_document_separator(&self) -> bool {
        let mut pos = self.position;
        let ch = self.ch(pos);
        if (ch == 0x2D || ch == 0x2E) && ch == self.ch(pos + 1) && ch == self.ch(pos + 2) {
            pos += 3;
            let ch = self.ch(pos);
            if ch == 0 || is_ws_or_eol(ch) {
                return true;
            }
        }
        false
    }

    fn write_folded_lines(&mut self, count: usize) {
        if count == 1 {
            self.result_append(&[0x20]);
        } else if count > 1 {
            self.result_append(&vec![0x0A; count - 1]);
        }
    }

    fn read_plain_scalar(&mut self, node_indent: isize, within_flow_collection: bool) -> R<bool> {
        let kind = self.kind;
        let result = self.result.clone();
        let mut ch = self.cur();
        if is_ws_or_eol(ch)
            || is_flow_indicator(ch)
            || matches!(ch, 0x23 | 0x26 | 0x2A | 0x21 | 0x7C | 0x3E | 0x27 | 0x22 | 0x25 | 0x40 | 0x60)
        {
            return Ok(false);
        }
        if ch == 0x3F || ch == 0x2D {
            let following = self.ch(self.position + 1);
            if is_ws_or_eol(following) || (within_flow_collection && is_flow_indicator(following)) {
                return Ok(false);
            }
        }
        self.kind = Some(Kind::Scalar);
        self.result = Value::String(String::new());
        let mut capture_start = self.position;
        let mut capture_end = self.position;
        let mut has_pending_content = false;
        let mut line = 0;
        while ch != 0 {
            if ch == 0x3A {
                let following = self.ch(self.position + 1);
                if is_ws_or_eol(following) || (within_flow_collection && is_flow_indicator(following)) {
                    break;
                }
            } else if ch == 0x23 {
                let preceding = if self.position == 0 { -1 } else { self.ch(self.position - 1) };
                if is_ws_or_eol(preceding) {
                    break;
                }
            } else if (self.position == self.line_start && self.test_document_separator())
                || (within_flow_collection && is_flow_indicator(ch))
            {
                break;
            } else if is_eol(ch) {
                line = self.line;
                let line_start = self.line_start;
                let line_indent = self.line_indent;
                self.skip_separation_space(false, -1)?;
                if self.line_indent >= node_indent {
                    has_pending_content = true;
                    ch = self.cur();
                    continue;
                } else {
                    self.position = capture_end;
                    self.line = line;
                    self.line_start = line_start;
                    self.line_indent = line_indent;
                    break;
                }
            }
            if has_pending_content {
                self.capture_segment(capture_start, capture_end, false)?;
                self.write_folded_lines(self.line - line);
                capture_start = self.position;
                capture_end = self.position;
                has_pending_content = false;
            }
            if !is_white_space(ch) {
                capture_end = self.position + 1;
            }
            ch = self.adv();
        }
        self.capture_segment(capture_start, capture_end, false)?;
        if self.result.truthy() {
            return Ok(true);
        }
        self.kind = kind;
        self.result = result;
        Ok(false)
    }

    fn read_single_quoted_scalar(&mut self, node_indent: isize) -> R<bool> {
        if self.cur() != 0x27 {
            return Ok(false);
        }
        self.kind = Some(Kind::Scalar);
        self.result = Value::String(String::new());
        self.position += 1;
        let mut capture_start = self.position;
        let mut capture_end = self.position;
        loop {
            let ch = self.cur();
            if ch == 0 {
                break;
            }
            if ch == 0x27 {
                self.capture_segment(capture_start, self.position, true)?;
                let ch = self.adv();
                if ch == 0x27 {
                    capture_start = self.position;
                    self.position += 1;
                    capture_end = self.position;
                } else {
                    return Ok(true);
                }
            } else if is_eol(ch) {
                self.capture_segment(capture_start, capture_end, true)?;
                let n = self.skip_separation_space(false, node_indent)?;
                self.write_folded_lines(n);
                capture_start = self.position;
                capture_end = self.position;
            } else if self.position == self.line_start && self.test_document_separator() {
                return self.fail("unexpected end of the document within a single quoted scalar");
            } else {
                self.position += 1;
                if !is_white_space(ch) {
                    capture_end = self.position;
                }
            }
        }
        self.fail("unexpected end of the stream within a single quoted scalar")
    }

    fn read_double_quoted_scalar(&mut self, node_indent: isize) -> R<bool> {
        if self.cur() != 0x22 {
            return Ok(false);
        }
        self.kind = Some(Kind::Scalar);
        self.result = Value::String(String::new());
        self.position += 1;
        let mut capture_start = self.position;
        let mut capture_end;
        // Collected as UTF-16 so `\uD83D\uDE00` escape pairs recombine like in JS.
        let mut buf: Vec<u16> = Vec::new();
        loop {
            let mut ch = self.cur();
            if ch == 0 {
                break;
            }
            if ch == 0x22 {
                self.capture_into(&mut buf, capture_start, self.position)?;
                self.position += 1;
                self.result = Value::String(to_string(&buf));
                return Ok(true);
            } else if ch == 0x5C {
                self.capture_into(&mut buf, capture_start, self.position)?;
                ch = self.adv();
                if is_eol(ch) {
                    self.skip_separation_space(false, node_indent)?;
                } else if let Some(seq) = (ch < 256).then(|| simple_escape_sequence(ch)).flatten() {
                    buf.extend_from_slice(seq);
                    self.position += 1;
                } else if escaped_hex_len(ch) > 0 {
                    let mut hex_length = escaped_hex_len(ch);
                    let mut hex_result: u32 = 0;
                    while hex_length > 0 {
                        ch = self.adv();
                        let tmp = from_hex_code(ch);
                        if tmp >= 0 {
                            hex_result = (hex_result << 4).wrapping_add(tmp as u32);
                        } else {
                            return self.fail("expected hexadecimal character");
                        }
                        hex_length -= 1;
                    }
                    buf.extend(char_from_codepoint(hex_result));
                    self.position += 1;
                } else {
                    return self.fail("unknown escape sequence");
                }
                capture_start = self.position;
            } else if is_eol(ch) {
                capture_end = self.last_non_ws_end(capture_start);
                self.capture_into(&mut buf, capture_start, capture_end)?;
                let n = self.skip_separation_space(false, node_indent)?;
                if n == 1 {
                    buf.push(0x20);
                } else if n > 1 {
                    buf.extend(std::iter::repeat_n(0x0A, n - 1));
                }
                capture_start = self.position;
            } else if self.position == self.line_start && self.test_document_separator() {
                return self.fail("unexpected end of the document within a double quoted scalar");
            } else {
                self.position += 1;
            }
        }
        self.fail("unexpected end of the stream within a double quoted scalar")
    }

    /// End of the captured run (exclusive) after trimming trailing white space,
    /// matching js-yaml's `captureEnd` bookkeeping in quoted scalars.
    fn last_non_ws_end(&self, capture_start: usize) -> usize {
        let mut end = self.position;
        while end > capture_start && is_white_space(self.input[end - 1] as i32) {
            end -= 1;
        }
        end
    }

    fn capture_into(&self, buf: &mut Vec<u16>, start: usize, end: usize) -> R<()> {
        if start < end {
            for &c in &self.input[start..end] {
                if !(c == 0x09 || c >= 0x20) {
                    return self.fail("expected valid JSON character");
                }
            }
            buf.extend_from_slice(&self.input[start..end]);
        }
        Ok(())
    }

    fn read_flow_collection(&mut self, node_indent: isize) -> R<bool> {
        let tag = self.tag.clone();
        let anchor = self.anchor.clone();
        let mut ch = self.cur();
        let (terminator, is_mapping) = if ch == 0x5B {
            (0x5D, false)
        } else if ch == 0x7B {
            (0x7D, true)
        } else {
            return Ok(false);
        };
        let mut seq: Vec<Value> = Vec::new();
        let mut map = Object::new();
        let mut overridable: HashMap<String, bool> = HashMap::new();
        if let Some(a) = &self.anchor {
            let placeholder = if is_mapping { Value::Object(Object::new()) } else { Value::array(Vec::new()) };
            self.store_anchor(&a.clone(), placeholder);
        }
        ch = self.adv();
        let mut read_next = true;
        while ch != 0 {
            self.skip_separation_space(true, node_indent)?;
            ch = self.cur();
            if ch == terminator {
                self.position += 1;
                self.tag = tag;
                self.anchor = anchor.clone();
                self.kind = Some(if is_mapping { Kind::Mapping } else { Kind::Sequence });
                self.result = if is_mapping { Value::Object(map) } else { Value::array(seq) };
                if let Some(a) = &anchor {
                    let v = self.result.clone();
                    self.store_anchor(a, v);
                }
                return Ok(true);
            } else if !read_next {
                return self.fail("missed comma between flow collection entries");
            } else if ch == 0x2C {
                return self.fail("expected the node content, but found ','");
            }
            let mut is_pair = false;
            let mut is_explicit_pair = false;
            let mut value_node = Value::Null;
            if ch == 0x3F {
                let following = self.ch(self.position + 1);
                if is_ws_or_eol(following) {
                    is_pair = true;
                    is_explicit_pair = true;
                    self.position += 1;
                    self.skip_separation_space(true, node_indent)?;
                }
            }
            let line = self.line;
            let line_start = self.line_start;
            let pos = self.position;
            self.compose_node(node_indent, CONTEXT_FLOW_IN, false, true)?;
            let key_tag = self.tag.clone();
            let key_node = self.result.clone();
            self.skip_separation_space(true, node_indent)?;
            ch = self.cur();
            if (is_explicit_pair || self.line == line) && ch == 0x3A {
                is_pair = true;
                self.adv();
                self.skip_separation_space(true, node_indent)?;
                self.compose_node(node_indent, CONTEXT_FLOW_IN, false, true)?;
                value_node = self.result.clone();
            }
            if is_mapping {
                self.store_mapping_pair(
                    &mut map,
                    &mut overridable,
                    key_tag.as_deref(),
                    key_node,
                    value_node,
                    line,
                    line_start,
                    pos,
                )?;
            } else if is_pair {
                let mut single = Object::new();
                self.store_mapping_pair(
                    &mut single,
                    &mut overridable,
                    key_tag.as_deref(),
                    key_node,
                    value_node,
                    line,
                    line_start,
                    pos,
                )?;
                seq.push(Value::Object(single));
            } else {
                seq.push(key_node);
            }
            self.skip_separation_space(true, node_indent)?;
            ch = self.cur();
            if ch == 0x2C {
                read_next = true;
                ch = self.adv();
            } else {
                read_next = false;
            }
        }
        self.fail("unexpected end of the stream within a flow collection")
    }

    fn read_block_scalar(&mut self, node_indent: isize) -> R<bool> {
        let mut ch = self.cur();
        let folding = if ch == 0x7C {
            false
        } else if ch == 0x3E {
            true
        } else {
            return Ok(false);
        };
        let mut chomping = CHOMPING_CLIP;
        let mut did_read_content = false;
        let mut detected_indent = false;
        let mut text_indent = node_indent;
        let mut empty_lines: usize = 0;
        let mut at_more_indented = false;
        self.kind = Some(Kind::Scalar);
        self.result = Value::String(String::new());
        while ch != 0 {
            ch = self.adv();
            if ch == 0x2B || ch == 0x2D {
                if chomping == CHOMPING_CLIP {
                    chomping = if ch == 0x2B { CHOMPING_KEEP } else { CHOMPING_STRIP };
                } else {
                    return self.fail("repeat of a chomping mode identifier");
                }
            } else {
                let tmp = from_decimal_code(ch);
                if tmp >= 0 {
                    if tmp == 0 {
                        return self
                            .fail("bad explicit indentation width of a block scalar; it cannot be less than one");
                    } else if !detected_indent {
                        text_indent = node_indent + tmp as isize - 1;
                        detected_indent = true;
                    } else {
                        return self.fail("repeat of an indentation width identifier");
                    }
                } else {
                    break;
                }
            }
        }
        if is_white_space(ch) {
            loop {
                ch = self.adv();
                if !is_white_space(ch) {
                    break;
                }
            }
            if ch == 0x23 {
                loop {
                    ch = self.adv();
                    if is_eol(ch) || ch == 0 {
                        break;
                    }
                }
            }
        }
        while ch != 0 {
            self.read_line_break()?;
            self.line_indent = 0;
            ch = self.cur();
            while (!detected_indent || self.line_indent < text_indent) && ch == 0x20 {
                self.line_indent += 1;
                ch = self.adv();
            }
            if !detected_indent && self.line_indent > text_indent {
                text_indent = self.line_indent;
            }
            if is_eol(ch) {
                empty_lines += 1;
                continue;
            }
            if !detected_indent && text_indent == 0 {
                return self.fail("missing indentation for block scalar");
            }
            if self.line_indent < text_indent {
                if chomping == CHOMPING_KEEP {
                    let n = if did_read_content { 1 + empty_lines } else { empty_lines };
                    self.result_append(&vec![0x0A; n]);
                } else if chomping == CHOMPING_CLIP && did_read_content {
                    self.result_append(&[0x0A]);
                }
                break;
            }
            if folding {
                if is_white_space(ch) {
                    at_more_indented = true;
                    let n = if did_read_content { 1 + empty_lines } else { empty_lines };
                    self.result_append(&vec![0x0A; n]);
                } else if at_more_indented {
                    at_more_indented = false;
                    self.result_append(&vec![0x0A; empty_lines + 1]);
                } else if empty_lines == 0 {
                    if did_read_content {
                        self.result_append(&[0x20]);
                    }
                } else {
                    self.result_append(&vec![0x0A; empty_lines]);
                }
            } else {
                let n = if did_read_content { 1 + empty_lines } else { empty_lines };
                self.result_append(&vec![0x0A; n]);
            }
            did_read_content = true;
            detected_indent = true;
            empty_lines = 0;
            let capture_start = self.position;
            while !is_eol(ch) && ch != 0 {
                ch = self.adv();
            }
            self.capture_segment(capture_start, self.position, false)?;
        }
        Ok(true)
    }

    fn read_block_sequence(&mut self, node_indent: isize) -> R<bool> {
        let tag = self.tag.clone();
        let anchor = self.anchor.clone();
        let mut result: Vec<Value> = Vec::new();
        let mut detected = false;
        if self.first_tab_in_line != -1 {
            return Ok(false);
        }
        if let Some(a) = &self.anchor {
            self.store_anchor(&a.clone(), Value::array(Vec::new()));
        }
        let mut ch = self.cur();
        while ch != 0 {
            if self.first_tab_in_line != -1 {
                self.position = self.first_tab_in_line as usize;
                return self.fail("tab characters must not be used in indentation");
            }
            if ch != 0x2D {
                break;
            }
            let following = self.ch(self.position + 1);
            if !is_ws_or_eol(following) {
                break;
            }
            detected = true;
            self.position += 1;
            if self.skip_separation_space(true, -1)? > 0 && self.line_indent <= node_indent {
                result.push(Value::Null);
                ch = self.cur();
                continue;
            }
            let line = self.line;
            self.compose_node(node_indent, CONTEXT_BLOCK_IN, false, true)?;
            result.push(self.result.clone());
            self.skip_separation_space(true, -1)?;
            ch = self.cur();
            if (self.line == line || self.line_indent > node_indent) && ch != 0 {
                return self.fail("bad indentation of a sequence entry");
            } else if self.line_indent < node_indent {
                break;
            }
        }
        if detected {
            self.tag = tag;
            self.anchor = anchor.clone();
            self.kind = Some(Kind::Sequence);
            self.result = Value::array(result);
            if let Some(a) = &anchor {
                let v = self.result.clone();
                self.store_anchor(a, v);
            }
            return Ok(true);
        }
        Ok(false)
    }

    #[allow(unused_assignments)]
    fn read_block_mapping(&mut self, node_indent: isize, flow_indent: isize) -> R<bool> {
        let tag = self.tag.clone();
        let anchor = self.anchor.clone();
        let mut result = Object::new();
        let mut overridable: HashMap<String, bool> = HashMap::new();
        let mut key_tag: Option<String> = None;
        let mut key_node = Value::Null;
        let mut value_node = Value::Null;
        let mut at_explicit_key = false;
        let mut detected = false;
        let mut allow_compact = false;
        let mut key_line = 0;
        let mut key_line_start = 0;
        let mut key_pos = 0;
        if self.first_tab_in_line != -1 {
            return Ok(false);
        }
        if let Some(a) = &self.anchor {
            self.store_anchor(&a.clone(), Value::Object(Object::new()));
        }
        let mut ch = self.cur();
        while ch != 0 {
            if !at_explicit_key && self.first_tab_in_line != -1 {
                self.position = self.first_tab_in_line as usize;
                return self.fail("tab characters must not be used in indentation");
            }
            let following = self.ch(self.position + 1);
            let line = self.line;
            if (ch == 0x3F || ch == 0x3A) && is_ws_or_eol(following) {
                if ch == 0x3F {
                    if at_explicit_key {
                        self.store_mapping_pair(
                            &mut result,
                            &mut overridable,
                            key_tag.as_deref(),
                            key_node.clone(),
                            Value::Null,
                            key_line,
                            key_line_start,
                            key_pos,
                        )?;
                        key_tag = None;
                        key_node = Value::Null;
                        value_node = Value::Null;
                    }
                    detected = true;
                    at_explicit_key = true;
                    allow_compact = true;
                } else if at_explicit_key {
                    at_explicit_key = false;
                    allow_compact = true;
                } else {
                    return self.fail(
                        "incomplete explicit mapping pair; a key node is missed; or followed by a non-tabulated empty line",
                    );
                }
                self.position += 1;
                ch = following;
            } else {
                key_line = self.line;
                key_line_start = self.line_start;
                key_pos = self.position;
                if !self.compose_node(flow_indent, CONTEXT_FLOW_OUT, false, true)? {
                    break;
                }
                if self.line == line {
                    ch = self.cur();
                    while is_white_space(ch) {
                        ch = self.adv();
                    }
                    if ch == 0x3A {
                        ch = self.adv();
                        if !is_ws_or_eol(ch) {
                            return self.fail(
                                "a whitespace character is expected after the key-value separator within a block mapping",
                            );
                        }
                        if at_explicit_key {
                            self.store_mapping_pair(
                                &mut result,
                                &mut overridable,
                                key_tag.as_deref(),
                                key_node.clone(),
                                Value::Null,
                                key_line,
                                key_line_start,
                                key_pos,
                            )?;
                            key_tag = None;
                            key_node = Value::Null;
                            value_node = Value::Null;
                        }
                        detected = true;
                        at_explicit_key = false;
                        allow_compact = false;
                        key_tag = self.tag.clone();
                        key_node = self.result.clone();
                    } else if detected {
                        return self.fail("can not read an implicit mapping pair; a colon is missed");
                    } else {
                        self.tag = tag;
                        self.anchor = anchor;
                        return Ok(true);
                    }
                } else if detected {
                    return self.fail("can not read a block mapping entry; a multiline key may not be an implicit key");
                } else {
                    self.tag = tag;
                    self.anchor = anchor;
                    return Ok(true);
                }
            }
            if self.line == line || self.line_indent > node_indent {
                if at_explicit_key {
                    key_line = self.line;
                    key_line_start = self.line_start;
                    key_pos = self.position;
                }
                if self.compose_node(node_indent, CONTEXT_BLOCK_OUT, true, allow_compact)? {
                    if at_explicit_key {
                        key_node = self.result.clone();
                    } else {
                        value_node = self.result.clone();
                    }
                }
                if !at_explicit_key {
                    self.store_mapping_pair(
                        &mut result,
                        &mut overridable,
                        key_tag.as_deref(),
                        key_node.clone(),
                        value_node.clone(),
                        key_line,
                        key_line_start,
                        key_pos,
                    )?;
                    key_tag = None;
                    key_node = Value::Null;
                    value_node = Value::Null;
                }
                self.skip_separation_space(true, -1)?;
                ch = self.cur();
            }
            if (self.line == line || self.line_indent > node_indent) && ch != 0 {
                return self.fail("bad indentation of a mapping entry");
            } else if self.line_indent < node_indent {
                break;
            }
        }
        if at_explicit_key {
            self.store_mapping_pair(
                &mut result,
                &mut overridable,
                key_tag.as_deref(),
                key_node,
                Value::Null,
                key_line,
                key_line_start,
                key_pos,
            )?;
        }
        if detected {
            self.tag = tag;
            self.anchor = anchor.clone();
            self.kind = Some(Kind::Mapping);
            self.result = Value::Object(result);
            if let Some(a) = &anchor {
                let v = self.result.clone();
                self.store_anchor(a, v);
            }
        }
        Ok(detected)
    }

    #[allow(clippy::too_many_arguments)]
    fn store_mapping_pair(
        &mut self,
        result: &mut Object,
        overridable: &mut HashMap<String, bool>,
        key_tag: Option<&str>,
        key_node: Value,
        value_node: Value,
        start_line: usize,
        start_line_start: usize,
        start_pos: usize,
    ) -> R<()> {
        let key = match &key_node {
            Value::Array(items) => {
                let mut parts = Vec::with_capacity(items.len());
                for item in items {
                    if item.is_array() {
                        return self.fail("nested arrays are not supported inside keys");
                    }
                    parts.push(if item.is_plain_object() { Value::str("[object Object]") } else { item.clone() });
                }
                Value::array(parts).to_js_string()
            }
            Value::Object(_) => "[object Object]".to_string(),
            other => other.to_js_string(),
        };
        if key_tag == Some("tag:yaml.org,2002:merge") {
            if let Value::Array(sources) = &value_node {
                for source in sources {
                    self.merge_mappings(result, source, overridable)?;
                }
            } else {
                self.merge_mappings(result, &value_node, overridable)?;
            }
        } else {
            if !overridable.contains_key(&key) && result.contains_key(&key) {
                if start_line != 0 {
                    self.line = start_line;
                }
                if start_line_start != 0 {
                    self.line_start = start_line_start;
                }
                if start_pos != 0 {
                    self.position = start_pos;
                }
                return self.fail("duplicated mapping key");
            }
            result.set(key.clone(), value_node);
            overridable.remove(&key);
        }
        Ok(())
    }

    fn merge_mappings(
        &mut self,
        destination: &mut Object,
        source: &Value,
        overridable: &mut HashMap<String, bool>,
    ) -> R<()> {
        if !source.is_object_like() {
            return self.fail("cannot merge mappings; the provided source object is unacceptable");
        }
        for (key, value) in source.own_entries() {
            self.total_merge_keys += 1;
            if self.total_merge_keys > MAX_TOTAL_MERGE_KEYS {
                return self.fail(&format!("merge keys exceeded maxTotalMergeKeys ({})", MAX_TOTAL_MERGE_KEYS));
            }
            if !destination.contains_key(&key) {
                destination.set(key.clone(), value);
                overridable.insert(key, true);
            }
        }
        Ok(())
    }

    fn read_tag_property(&mut self) -> R<bool> {
        let mut ch = self.cur();
        if ch != 0x21 {
            return Ok(false);
        }
        if self.tag.is_some() {
            return self.fail("duplication of a tag property");
        }
        ch = self.adv();
        let mut is_verbatim = false;
        let mut is_named = false;
        let mut tag_handle: String;
        if ch == 0x3C {
            is_verbatim = true;
            ch = self.adv();
            tag_handle = String::new();
        } else if ch == 0x21 {
            is_named = true;
            tag_handle = "!!".into();
            ch = self.adv();
        } else {
            tag_handle = "!".into();
        }
        let mut start = self.position;
        let tag_name: String;
        if is_verbatim {
            loop {
                ch = self.adv();
                if ch == 0 || ch == 0x3E {
                    break;
                }
            }
            if self.position < self.length {
                tag_name = to_string(&self.input[start..self.position]);
                self.adv();
            } else {
                return self.fail("unexpected end of the stream within a verbatim tag");
            }
        } else {
            while ch != 0 && !is_ws_or_eol(ch) {
                if ch == 0x21 {
                    if !is_named {
                        tag_handle = to_string(&self.input[start - 1..self.position + 1]);
                        if !is_tag_handle(&tag_handle) {
                            return self.fail("named tag handle cannot contain such characters");
                        }
                        is_named = true;
                        start = self.position + 1;
                    } else {
                        return self.fail("tag suffix cannot contain exclamation marks");
                    }
                }
                ch = self.adv();
            }
            tag_name = to_string(&self.input[start..self.position]);
            if tag_name.chars().any(|c| matches!(c, ',' | '[' | ']' | '{' | '}')) {
                return self.fail("tag suffix cannot contain flow indicator characters");
            }
        }
        if !tag_name.is_empty() && !is_tag_uri(&tag_name) {
            return self.fail(&format!("tag name cannot contain such characters: {}", tag_name));
        }
        let decoded = match decode_uri_component(&tag_name) {
            Ok(d) => d,
            Err(_) => return self.fail(&format!("tag name is malformed: {}", tag_name)),
        };
        if is_verbatim {
            self.tag = Some(decoded);
        } else if let Some(prefix) = self.tag_map.get(&tag_handle) {
            self.tag = Some(format!("{}{}", prefix, decoded));
        } else if tag_handle == "!" {
            self.tag = Some(format!("!{}", decoded));
        } else if tag_handle == "!!" {
            self.tag = Some(format!("tag:yaml.org,2002:{}", decoded));
        } else {
            return self.fail(&format!("undeclared tag handle \"{}\"", tag_handle));
        }
        Ok(true)
    }

    fn read_anchor_property(&mut self) -> R<bool> {
        let mut ch = self.cur();
        if ch != 0x26 {
            return Ok(false);
        }
        if self.anchor.is_some() {
            return self.fail("duplication of an anchor property");
        }
        ch = self.adv();
        let start = self.position;
        while ch != 0 && !is_ws_or_eol(ch) && !is_flow_indicator(ch) {
            ch = self.adv();
        }
        if self.position == start {
            return self.fail("name of an anchor node must contain at least one character");
        }
        self.anchor = Some(to_string(&self.input[start..self.position]));
        Ok(true)
    }

    fn read_alias(&mut self) -> R<bool> {
        let mut ch = self.cur();
        if ch != 0x2A {
            return Ok(false);
        }
        ch = self.adv();
        let start = self.position;
        while ch != 0 && !is_ws_or_eol(ch) && !is_flow_indicator(ch) {
            ch = self.adv();
        }
        if self.position == start {
            return self.fail("name of an alias node must contain at least one character");
        }
        let alias = to_string(&self.input[start..self.position]);
        match self.anchor_map.get(&alias) {
            Some(v) => self.result = v.clone(),
            None => return self.fail(&format!("unidentified alias \"{}\"", alias)),
        }
        self.skip_separation_space(true, -1)?;
        Ok(true)
    }

    fn try_read_block_mapping_from_property(
        &mut self,
        property_start: Snapshot,
        node_indent: isize,
        flow_indent: isize,
    ) -> R<bool> {
        let fallback = self.snapshot();
        self.begin_anchor_transaction();
        self.restore(property_start);
        self.tag = None;
        self.anchor = None;
        self.kind = None;
        self.result = Value::Null;
        match self.read_block_mapping(node_indent, flow_indent) {
            Ok(true) if self.kind == Some(Kind::Mapping) => {
                self.commit_anchor_transaction();
                Ok(true)
            }
            Ok(_) => {
                self.rollback_anchor_transaction();
                self.restore(fallback);
                Ok(false)
            }
            Err(e) => Err(e),
        }
    }

    fn compose_node(
        &mut self,
        parent_indent: isize,
        node_context: u8,
        allow_to_seek: bool,
        allow_compact: bool,
    ) -> R<bool> {
        if self.depth >= MAX_DEPTH {
            return self.fail(&format!("nesting exceeded maxDepth ({})", MAX_DEPTH));
        }
        self.depth += 1;
        self.tag = None;
        self.anchor = None;
        self.kind = None;
        self.result = Value::Null;
        let mut indent_status = 1;
        let mut at_new_line = false;
        let mut has_content = false;
        let mut property_start: Option<Snapshot> = None;
        let allow_block_styles = node_context == CONTEXT_BLOCK_OUT || node_context == CONTEXT_BLOCK_IN;
        let allow_block_scalars = allow_block_styles;
        let mut allow_block_collections = allow_block_styles;
        if allow_to_seek && self.skip_separation_space(true, -1)? > 0 {
            at_new_line = true;
            indent_status = self.indent_status(parent_indent);
        }
        if indent_status == 1 {
            loop {
                let ch = self.cur();
                let property_state = self.snapshot();
                if at_new_line && ((ch == 0x21 && self.tag.is_some()) || (ch == 0x26 && self.anchor.is_some())) {
                    break;
                }
                if !self.read_tag_property()? && !self.read_anchor_property()? {
                    break;
                }
                if property_start.is_none() {
                    property_start = Some(property_state);
                }
                if self.skip_separation_space(true, -1)? > 0 {
                    at_new_line = true;
                    allow_block_collections = allow_block_styles;
                    indent_status = self.indent_status(parent_indent);
                } else {
                    allow_block_collections = false;
                }
            }
        }
        if allow_block_collections {
            allow_block_collections = at_new_line || allow_compact;
        }
        if indent_status == 1 || node_context == CONTEXT_BLOCK_OUT {
            let flow_indent = if node_context == CONTEXT_FLOW_IN || node_context == CONTEXT_FLOW_OUT {
                parent_indent
            } else {
                parent_indent + 1
            };
            let block_indent = self.position as isize - self.line_start as isize;
            if indent_status == 1 {
                if (allow_block_collections
                    && (self.read_block_sequence(block_indent)?
                        || self.read_block_mapping(block_indent, flow_indent)?))
                    || self.read_flow_collection(flow_indent)?
                {
                    has_content = true;
                } else {
                    let ch = self.cur();
                    let mut handled = false;
                    if let Some(ps) = property_start.clone()
                        && allow_block_styles
                        && !allow_block_collections
                        && ch != 0x7C
                        && ch != 0x3E
                    {
                        let indent = ps.position as isize - ps.line_start as isize;
                        if self.try_read_block_mapping_from_property(ps, indent, flow_indent)? {
                            has_content = true;
                            handled = true;
                        }
                    }
                    if !handled {
                        if (allow_block_scalars && self.read_block_scalar(flow_indent)?)
                            || self.read_single_quoted_scalar(flow_indent)?
                            || self.read_double_quoted_scalar(flow_indent)?
                        {
                            has_content = true;
                        } else if self.read_alias()? {
                            has_content = true;
                            if self.tag.is_some() || self.anchor.is_some() {
                                return self.fail("alias node should not have any properties");
                            }
                        } else if self.read_plain_scalar(flow_indent, node_context == CONTEXT_FLOW_IN)? {
                            has_content = true;
                            if self.tag.is_none() {
                                self.tag = Some("?".into());
                            }
                        }
                    }
                    if let Some(a) = self.anchor.clone() {
                        let v = self.result.clone();
                        self.store_anchor(&a, v);
                    }
                }
            } else if indent_status == 0 {
                has_content = allow_block_collections && self.read_block_sequence(block_indent)?;
            }
        }
        match self.tag.clone() {
            None => {
                if let Some(a) = self.anchor.clone() {
                    let v = self.result.clone();
                    self.store_anchor(&a, v);
                }
            }
            Some(t) if t == "?" => {
                if !self.result.is_null() && self.kind != Some(Kind::Scalar) {
                    let kind = self.kind.map(Kind::name).unwrap_or("null");
                    return self.fail(&format!(
                        "unacceptable node kind for !<?> tag; it should be \"scalar\", not \"{}\"",
                        kind
                    ));
                }
                let resolved = if self.result.is_null() {
                    Some(("tag:yaml.org,2002:null", Value::Null))
                } else {
                    resolve_implicit(&self.result)
                };
                if let Some((tag, value)) = resolved {
                    self.result = value;
                    self.tag = Some(tag.into());
                    if let Some(a) = self.anchor.clone() {
                        let v = self.result.clone();
                        self.store_anchor(&a, v);
                    }
                }
            }
            Some(t) if t == "!" => {}
            Some(t) => {
                let kind = self.kind;
                let Some(ty) = explicit_type(&t).filter(|ty| kind.is_none_or(|k| k == ty.kind)) else {
                    return self.fail(&format!("unknown tag !<{}>", t));
                };
                if !self.result.is_null() && Some(ty.kind) != kind {
                    let actual = kind.map(Kind::name).unwrap_or("null");
                    return self.fail(&format!(
                        "unacceptable node kind for !<{}> tag; it should be \"{}\", not \"{}\"",
                        t,
                        ty.kind.name(),
                        actual
                    ));
                }
                match (ty.construct)(&self.result) {
                    Some(v) => {
                        self.result = v;
                        if let Some(a) = self.anchor.clone() {
                            let v = self.result.clone();
                            self.store_anchor(&a, v);
                        }
                    }
                    None => return self.fail(&format!("cannot resolve a node with !<{}> explicit tag", t)),
                }
            }
        }
        self.depth -= 1;
        Ok(self.tag.is_some() || self.anchor.is_some() || has_content)
    }

    fn indent_status(&self, parent_indent: isize) -> i32 {
        if self.line_indent > parent_indent {
            1
        } else if self.line_indent == parent_indent {
            0
        } else {
            -1
        }
    }

    fn read_document(&mut self) -> R<()> {
        let mut has_directives = false;
        self.version = None;
        self.tag_map = HashMap::new();
        self.anchor_map = HashMap::new();
        while self.cur() != 0 {
            self.skip_separation_space(true, -1)?;
            let mut ch = self.cur();
            if self.line_indent > 0 || ch != 0x25 {
                break;
            }
            has_directives = true;
            ch = self.adv();
            let mut start = self.position;
            while ch != 0 && !is_ws_or_eol(ch) {
                ch = self.adv();
            }
            let directive_name = to_string(&self.input[start..self.position]);
            let mut args: Vec<String> = Vec::new();
            if directive_name.is_empty() {
                return self.fail("directive name must not be less than one character in length");
            }
            while ch != 0 {
                while is_white_space(ch) {
                    ch = self.adv();
                }
                if ch == 0x23 {
                    loop {
                        ch = self.adv();
                        if ch == 0 || is_eol(ch) {
                            break;
                        }
                    }
                    break;
                }
                if is_eol(ch) {
                    break;
                }
                start = self.position;
                while ch != 0 && !is_ws_or_eol(ch) {
                    ch = self.adv();
                }
                args.push(to_string(&self.input[start..self.position]));
            }
            if ch != 0 {
                self.read_line_break()?;
            }
            match directive_name.as_str() {
                "YAML" => self.handle_yaml_directive(&args)?,
                "TAG" => self.handle_tag_directive(&args)?,
                _ => {}
            }
        }
        self.skip_separation_space(true, -1)?;
        if self.line_indent == 0
            && self.cur() == 0x2D
            && self.ch(self.position + 1) == 0x2D
            && self.ch(self.position + 2) == 0x2D
        {
            self.position += 3;
            self.skip_separation_space(true, -1)?;
        } else if has_directives {
            return self.fail("directives end mark is expected");
        }
        self.compose_node(self.line_indent - 1, CONTEXT_BLOCK_OUT, false, true)?;
        self.skip_separation_space(true, -1)?;
        self.documents.push(self.result.clone());
        if self.position == self.line_start && self.test_document_separator() {
            if self.cur() == 0x2E {
                self.position += 3;
                self.skip_separation_space(true, -1)?;
            }
            return Ok(());
        }
        if self.position < self.length.saturating_sub(1) {
            return self.fail("end of the stream or a document separator is expected");
        }
        Ok(())
    }

    fn handle_yaml_directive(&mut self, args: &[String]) -> R<()> {
        if self.version.is_some() {
            return self.fail("duplication of %YAML directive");
        }
        if args.len() != 1 {
            return self.fail("YAML directive accepts exactly one argument");
        }
        let parts: Vec<&str> = args[0].split('.').collect();
        let valid = parts.len() == 2 && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
        if !valid {
            return self.fail("ill-formed argument of the YAML directive");
        }
        if parse_int(parts[0], 10) != 1.0 {
            return self.fail("unacceptable YAML version of the document");
        }
        self.version = Some(args[0].clone());
        Ok(())
    }

    fn handle_tag_directive(&mut self, args: &[String]) -> R<()> {
        if args.len() != 2 {
            return self.fail("TAG directive accepts exactly two arguments");
        }
        let handle = &args[0];
        let prefix = &args[1];
        if !is_tag_handle(handle) {
            return self.fail("ill-formed tag handle (first argument) of the TAG directive");
        }
        if self.tag_map.contains_key(handle) {
            return self.fail(&format!("there is a previously declared suffix for \"{}\" tag handle", handle));
        }
        if !is_tag_uri(prefix) {
            return self.fail("ill-formed tag prefix (second argument) of the TAG directive");
        }
        let decoded = match decode_uri_component(prefix) {
            Ok(d) => d,
            Err(_) => return self.fail(&format!("tag prefix is malformed: {}", prefix)),
        };
        self.tag_map.insert(handle.clone(), decoded);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Scalar types of the default schema
// ---------------------------------------------------------------------------

fn resolve_null(data: &str) -> bool {
    matches!(data, "~" | "null" | "Null" | "NULL")
}

fn resolve_bool(data: &str) -> Option<bool> {
    match data {
        "true" | "True" | "TRUE" => Some(true),
        "false" | "False" | "FALSE" => Some(false),
        _ => None,
    }
}

pub(crate) fn resolve_int(data: &str) -> Option<f64> {
    let b = data.as_bytes();
    if b.is_empty() {
        return None;
    }
    let mut i = 0;
    if b[0] == b'-' || b[0] == b'+' {
        i += 1;
    }
    let mut has_digits = false;
    if b.get(i) == Some(&b'0') {
        if i + 1 == b.len() {
            return Some(parse_yaml_integer(data));
        }
        let ch = b[i + 1];
        let check = |pred: &dyn Fn(u8) -> bool| -> Option<f64> {
            let digits = &b[i + 2..];
            if digits.is_empty() || !digits.iter().all(|&c| pred(c)) {
                return None;
            }
            let v = parse_yaml_integer(data);
            v.is_finite().then_some(v)
        };
        match ch {
            b'b' => return check(&|c| c == b'0' || c == b'1'),
            b'x' => return check(&|c| c.is_ascii_hexdigit()),
            b'o' => return check(&|c| (b'0'..=b'7').contains(&c)),
            _ => i += 1,
        }
        has_digits = true;
    }
    for &c in &b[i..] {
        if !c.is_ascii_digit() {
            return None;
        }
        has_digits = true;
    }
    if !has_digits {
        return None;
    }
    let v = parse_yaml_integer(data);
    v.is_finite().then_some(v)
}

fn parse_yaml_integer(data: &str) -> f64 {
    let mut value = data;
    let mut sign = 1.0;
    let first = value.chars().next();
    if first == Some('-') || first == Some('+') {
        if first == Some('-') {
            sign = -1.0;
        }
        value = &value[1..];
    }
    if value == "0" {
        return 0.0;
    }
    if let Some(rest) = value.strip_prefix('0') {
        if let Some(d) = rest.strip_prefix('b') {
            return sign * parse_int(d, 2);
        }
        if let Some(d) = rest.strip_prefix('x') {
            return sign * parse_int(d, 16);
        }
        if let Some(d) = rest.strip_prefix('o') {
            return sign * parse_int(d, 8);
        }
    }
    sign * parse_int(value, 10)
}

fn is_float_syntax(data: &str) -> bool {
    let s = data.strip_prefix(['-', '+']).unwrap_or(data);
    if matches!(s, ".inf" | ".Inf" | ".INF") {
        return true;
    }
    if data == ".nan" || data == ".NaN" || data == ".NAN" {
        return true;
    }
    let b = data.as_bytes();
    // [-+]?[0-9]+(\.[0-9]*)?([eE][-+]?[0-9]+)?
    let mut i = 0;
    if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
        i += 1;
    }
    let d0 = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let first_form = if i > d0 {
        let mut j = i;
        if j < b.len() && b[j] == b'.' {
            j += 1;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
        }
        exponent_end(b, j) == Some(b.len())
    } else {
        false
    };
    if first_form {
        return true;
    }
    // \.[0-9]+([eE][-+]?[0-9]+)?
    if b.first() == Some(&b'.') {
        let mut j = 1;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > 1 && exponent_end(b, j) == Some(b.len()) {
            return true;
        }
    }
    false
}

/// Optional exponent at `i`; returns the end index when the remainder is a
/// (possibly empty) valid exponent.
fn exponent_end(b: &[u8], i: usize) -> Option<usize> {
    if i == b.len() {
        return Some(i);
    }
    if b[i] != b'e' && b[i] != b'E' {
        return None;
    }
    let mut j = i + 1;
    if j < b.len() && (b[j] == b'-' || b[j] == b'+') {
        j += 1;
    }
    let start = j;
    while j < b.len() && b[j].is_ascii_digit() {
        j += 1;
    }
    (j > start).then_some(j)
}

fn resolve_float(data: &str) -> Option<f64> {
    if !is_float_syntax(data) {
        return None;
    }
    let value = construct_float(data);
    if value.is_finite() {
        return Some(value);
    }
    let special = matches!(
        data,
        ".inf" | ".Inf" | ".INF" | "-.inf" | "-.Inf" | "-.INF" | "+.inf" | "+.Inf" | "+.INF" | ".nan" | ".NaN" | ".NAN"
    );
    special.then_some(value)
}

fn construct_float(data: &str) -> f64 {
    let lower = data.to_lowercase();
    let sign = if lower.starts_with('-') { -1.0 } else { 1.0 };
    let value = lower.strip_prefix(['+', '-']).unwrap_or(&lower);
    if value == ".inf" {
        return sign * f64::INFINITY;
    }
    if value == ".nan" {
        return f64::NAN;
    }
    sign * crate::js::number::parse_float(value)
}

fn resolve_timestamp(data: &str) -> Option<f64> {
    let b = data.as_bytes();
    let digits = |s: &[u8]| s.iter().all(u8::is_ascii_digit);
    // YYYY-MM-DD
    if b.len() == 10 && digits(&b[0..4]) && b[4] == b'-' && digits(&b[5..7]) && b[7] == b'-' && digits(&b[8..10]) {
        let year: f64 = data[0..4].parse().ok()?;
        let month: f64 = data[5..7].parse().ok()?;
        let day: f64 = data[8..10].parse().ok()?;
        return Some(date_utc(year, month - 1.0, day, 0.0, 0.0, 0.0, 0.0));
    }
    parse_full_timestamp(data)
}

fn parse_full_timestamp(data: &str) -> Option<f64> {
    let b = data.as_bytes();
    let mut i = 0;
    let take_digits = |i: &mut usize, min: usize, max: usize| -> Option<String> {
        let start = *i;
        while *i < b.len() && *i - start < max && b[*i].is_ascii_digit() {
            *i += 1;
        }
        if *i - start < min { None } else { Some(data[start..*i].to_string()) }
    };
    let year = take_digits(&mut i, 4, 4)?;
    if b.get(i) != Some(&b'-') {
        return None;
    }
    i += 1;
    let month = take_digits(&mut i, 1, 2)?;
    if b.get(i) != Some(&b'-') {
        return None;
    }
    i += 1;
    let day = take_digits(&mut i, 1, 2)?;
    // (?:[Tt]|[ \t]+)
    match b.get(i) {
        Some(b'T') | Some(b't') => i += 1,
        Some(b' ') | Some(b'\t') => {
            while matches!(b.get(i), Some(b' ') | Some(b'\t')) {
                i += 1;
            }
        }
        _ => return None,
    }
    let hour = take_digits(&mut i, 1, 2)?;
    if b.get(i) != Some(&b':') {
        return None;
    }
    i += 1;
    let minute = take_digits(&mut i, 2, 2)?;
    if b.get(i) != Some(&b':') {
        return None;
    }
    i += 1;
    let second = take_digits(&mut i, 2, 2)?;
    let mut fraction = String::new();
    if b.get(i) == Some(&b'.') {
        i += 1;
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        fraction = data[start..i].to_string();
    }
    // (?:[ \t]*(Z|([-+])([0-9][0-9]?)(?::([0-9][0-9]))?))?$
    let mut delta = 0.0;
    if i < b.len() {
        let mut j = i;
        while matches!(b.get(j), Some(b' ') | Some(b'\t')) {
            j += 1;
        }
        match b.get(j) {
            Some(b'Z') => {
                j += 1;
            }
            Some(&s) if s == b'-' || s == b'+' => {
                j += 1;
                let tz_hour = take_digits(&mut j, 1, 2)?;
                let mut tz_minute = String::from("0");
                if b.get(j) == Some(&b':') {
                    j += 1;
                    tz_minute = take_digits(&mut j, 2, 2)?;
                }
                let d = (tz_hour.parse::<f64>().ok()? * 60.0 + tz_minute.parse::<f64>().ok()?) * 60000.0;
                delta = if s == b'-' { -d } else { d };
            }
            _ => return None,
        }
        if j != b.len() {
            return None;
        }
    }
    let ms = if fraction.is_empty() {
        0.0
    } else {
        let mut f: String = fraction.chars().take(3).collect();
        while f.len() < 3 {
            f.push('0');
        }
        f.parse::<f64>().ok()?
    };
    let tv = date_utc(
        year.parse().ok()?,
        month.parse::<f64>().ok()? - 1.0,
        day.parse().ok()?,
        hour.parse().ok()?,
        minute.parse().ok()?,
        second.parse().ok()?,
        ms,
    );
    Some(if delta != 0.0 { tv - delta } else { tv })
}

/// Implicit resolution of a plain scalar; returns the resolved tag and value.
fn resolve_implicit(result: &Value) -> Option<(&'static str, Value)> {
    let data = result.as_str()?;
    if resolve_null(data) {
        return Some(("tag:yaml.org,2002:null", Value::Null));
    }
    if let Some(b) = resolve_bool(data) {
        return Some(("tag:yaml.org,2002:bool", Value::Bool(b)));
    }
    if let Some(n) = resolve_int(data) {
        return Some(("tag:yaml.org,2002:int", Value::Number(n)));
    }
    if let Some(n) = resolve_float(data) {
        return Some(("tag:yaml.org,2002:float", Value::Number(n)));
    }
    if let Some(t) = resolve_timestamp(data) {
        return Some(("tag:yaml.org,2002:timestamp", Value::Date(t)));
    }
    if data == "<<" {
        return Some(("tag:yaml.org,2002:merge", Value::String(data.to_string())));
    }
    None
}

/// Whether a plain string would be read back as another type (dumper quoting).
pub(crate) fn is_implicitly_typed(data: &str) -> bool {
    resolve_null(data)
        || resolve_bool(data).is_some()
        || resolve_int(data).is_some()
        || resolve_float(data).is_some()
        || resolve_timestamp(data).is_some()
        || data == "<<"
}

struct ExplicitType {
    kind: Kind,
    construct: fn(&Value) -> Option<Value>,
}

fn scalar_data(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        other => Some(other.to_js_string()),
    }
}

fn explicit_type(tag: &str) -> Option<ExplicitType> {
    let name = tag.strip_prefix("tag:yaml.org,2002:")?;
    let scalar = |construct: fn(&Value) -> Option<Value>| Some(ExplicitType { kind: Kind::Scalar, construct });
    match name {
        "str" => scalar(|v| Some(Value::String(scalar_data(v).unwrap_or_default()))),
        "null" => scalar(|v| match v {
            Value::Null => Some(Value::Null),
            other => scalar_data(other).filter(|d| resolve_null(d)).map(|_| Value::Null),
        }),
        "bool" => scalar(|v| scalar_data(v).and_then(|d| resolve_bool(&d)).map(Value::Bool)),
        "int" => scalar(|v| scalar_data(v).and_then(|d| resolve_int(&d)).map(Value::Number)),
        "float" => scalar(|v| scalar_data(v).and_then(|d| resolve_float(&d)).map(Value::Number)),
        "timestamp" => scalar(|v| scalar_data(v).and_then(|d| resolve_timestamp(&d)).map(Value::Date)),
        "merge" => scalar(|v| match v {
            Value::Null => Some(Value::Null),
            other => scalar_data(other).filter(|d| d == "<<").map(Value::String),
        }),
        "binary" => scalar(|v| match v {
            Value::Null => None,
            other => construct_binary(&scalar_data(other)?),
        }),
        "seq" => Some(ExplicitType {
            kind: Kind::Sequence,
            construct: |v| Some(if v.is_null() { Value::array(Vec::new()) } else { v.clone() }),
        }),
        "omap" => Some(ExplicitType { kind: Kind::Sequence, construct: construct_omap }),
        "pairs" => Some(ExplicitType { kind: Kind::Sequence, construct: construct_pairs }),
        "map" => Some(ExplicitType {
            kind: Kind::Mapping,
            construct: |v| Some(if v.is_null() { Value::Object(Object::new()) } else { v.clone() }),
        }),
        "set" => Some(ExplicitType { kind: Kind::Mapping, construct: construct_set }),
        _ => None,
    }
}

fn construct_binary(data: &str) -> Option<Value> {
    const MAP: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=\n\r";
    let mut bitlen = 0;
    for c in data.chars() {
        match MAP.find(c) {
            Some(code) if code > 64 => continue,
            Some(_) => bitlen += 6,
            None => return None,
        }
    }
    if bitlen % 8 != 0 {
        return None;
    }
    let input: String = data.chars().filter(|c| !matches!(c, '\r' | '\n' | '=')).collect();
    let idx = |c: char| MAP.find(c).unwrap_or(0) as u32;
    let chars: Vec<char> = input.chars().collect();
    let mut bits: u32 = 0;
    let mut out: Vec<u8> = Vec::new();
    for (i, &c) in chars.iter().enumerate() {
        if i % 4 == 0 && i > 0 {
            out.push((bits >> 16) as u8);
            out.push((bits >> 8) as u8);
            out.push(bits as u8);
        }
        bits = (bits << 6) | idx(c);
    }
    match (chars.len() % 4) * 6 {
        0 => {
            out.push((bits >> 16) as u8);
            out.push((bits >> 8) as u8);
            out.push(bits as u8);
        }
        18 => {
            out.push((bits >> 10) as u8);
            out.push((bits >> 2) as u8);
        }
        12 => out.push((bits >> 4) as u8),
        _ => {}
    }
    // A Uint8Array behaves like an index-keyed object once copied.
    Some(Value::Object(out.iter().enumerate().map(|(i, b)| (i.to_string(), Value::Number(*b as f64))).collect()))
}

fn construct_omap(v: &Value) -> Option<Value> {
    let Value::Array(items) = v else {
        return if v.is_null() { Some(Value::array(Vec::new())) } else { None };
    };
    let mut seen: Vec<String> = Vec::new();
    for pair in items {
        let obj = pair.as_object()?;
        if obj.len() != 1 {
            return None;
        }
        let key = obj.keys()[0].clone();
        if seen.contains(&key) {
            return None;
        }
        seen.push(key);
    }
    Some(v.clone())
}

fn construct_pairs(v: &Value) -> Option<Value> {
    let Value::Array(items) = v else {
        return if v.is_null() { Some(Value::array(Vec::new())) } else { None };
    };
    let mut out = Vec::new();
    for pair in items {
        let obj = pair.as_object()?;
        if obj.len() != 1 {
            return None;
        }
        let (k, val) = obj.entries()[0];
        out.push(Value::array(vec![Value::String(k.clone()), val.clone()]));
    }
    Some(Value::array(out))
}

fn construct_set(v: &Value) -> Option<Value> {
    match v {
        Value::Null => Some(Value::Object(Object::new())),
        Value::Object(o) => o.entries().iter().all(|(_, val)| val.is_null()).then(|| v.clone()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------

/// `yaml.load(input)`; `Ok(Value::Undefined)` for an empty stream.
pub fn load(input: &str) -> Result<Value, YamlError> {
    let mut units = u16s(input);
    if !units.is_empty() {
        let last = *units.last().unwrap();
        if last != 0x0A && last != 0x0D {
            units.push(0x0A);
        }
        if units[0] == 0xFEFF {
            units.remove(0);
        }
    }
    let length = units.len();
    let mut state = State {
        input: units,
        length,
        position: 0,
        line: 0,
        line_start: 0,
        line_indent: 0,
        depth: 0,
        total_merge_keys: 0,
        first_tab_in_line: -1,
        documents: Vec::new(),
        anchor_map_transactions: Vec::new(),
        version: None,
        tag_map: HashMap::new(),
        anchor_map: HashMap::new(),
        tag: None,
        anchor: None,
        kind: None,
        result: Value::Null,
    };
    state.input.push(0);
    if let Some(nullpos) = state.input[..length].iter().position(|&c| c == 0) {
        state.position = nullpos;
        return Err(state.error("null byte is not allowed in input"));
    }
    while state.cur() == 0x20 {
        state.line_indent += 1;
        state.position += 1;
    }
    while state.position < state.length.saturating_sub(1) {
        state.read_document()?;
    }
    match state.documents.len() {
        0 => Ok(Value::Undefined),
        1 => Ok(state.documents.pop().unwrap()),
        _ => Err(YamlError {
            reason: "expected a single document in the stream, but found more".into(),
            message: "expected a single document in the stream, but found more".into(),
        }),
    }
}
