//! String helpers with ECMAScript semantics (whitespace set, UTF-16 lengths,
//! URI encoding/decoding).

use super::JsError;

/// WhiteSpace or LineTerminator as defined by ECMAScript (`\s`, `trim()`).
pub fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\u{0009}'
            | '\u{000A}'
            | '\u{000B}'
            | '\u{000C}'
            | '\u{000D}'
            | '\u{0020}'
            | '\u{00A0}'
            | '\u{1680}'
            | '\u{2000}'
            ..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}'
    )
}

pub fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_whitespace)
}

pub fn js_trim_start(s: &str) -> &str {
    s.trim_start_matches(is_js_whitespace)
}

pub fn js_trim_end(s: &str) -> &str {
    s.trim_end_matches(is_js_whitespace)
}

pub fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

pub fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

pub fn from_utf16(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

/// `String.prototype.slice` on UTF-16 indices (negative indices count from the end).
pub fn js_slice(s: &str, start: isize, end: Option<isize>) -> String {
    let units = utf16(s);
    let len = units.len() as isize;
    let norm = |i: isize| if i < 0 { (len + i).max(0) } else { i.min(len) };
    let from = norm(start);
    let to = end.map(norm).unwrap_or(len);
    if from >= to {
        return String::new();
    }
    from_utf16(&units[from as usize..to as usize])
}

/// `str.split(sep)` for a non-empty literal separator.
pub fn js_split<'a>(s: &'a str, sep: &str) -> Vec<&'a str> {
    s.split(sep).collect()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn uri_error() -> JsError {
    JsError::uri("URI malformed")
}

/// Decode per ECMAScript `Decode(string, preserveEscapeSet)`.
fn decode(s: &str, preserve: &str) -> Result<String, JsError> {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let read_byte = |i: usize| -> Option<u8> {
        if i + 2 < bytes.len() && bytes[i] == b'%' {
            let hi = hex_val(bytes[i + 1])?;
            let lo = hex_val(bytes[i + 2])?;
            Some(hi * 16 + lo)
        } else {
            None
        }
    };
    while i < bytes.len() {
        if bytes[i] != b'%' {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        if i + 2 >= bytes.len() {
            return Err(uri_error());
        }
        let b = read_byte(i).ok_or_else(uri_error)?;
        if b < 0x80 {
            let c = b as char;
            if preserve.contains(c) {
                out.extend_from_slice(&bytes[i..i + 3]);
            } else {
                out.push(b);
            }
            i += 3;
            continue;
        }
        let n = if b & 0xE0 == 0xC0 {
            2
        } else if b & 0xF0 == 0xE0 {
            3
        } else if b & 0xF8 == 0xF0 {
            4
        } else {
            return Err(uri_error());
        };
        if i + 3 * n > bytes.len() {
            return Err(uri_error());
        }
        let mut octets = vec![b];
        let mut j = i + 3;
        for _ in 1..n {
            if bytes[j] != b'%' {
                return Err(uri_error());
            }
            let cb = read_byte(j).ok_or_else(uri_error)?;
            if cb & 0xC0 != 0x80 {
                return Err(uri_error());
            }
            octets.push(cb);
            j += 3;
        }
        // Rejects overlong forms, surrogates and out-of-range code points.
        if std::str::from_utf8(&octets).is_err() {
            return Err(uri_error());
        }
        out.extend_from_slice(&octets);
        i = j;
    }
    String::from_utf8(out).map_err(|_| uri_error())
}

/// `decodeURIComponent`
pub fn decode_uri_component(s: &str) -> Result<String, JsError> {
    decode(s, "")
}

/// `decodeURI`
pub fn decode_uri(s: &str) -> Result<String, JsError> {
    decode(s, ";/?:@&=+$,#")
}

fn encode(s: &str, unescaped: &dyn Fn(char) -> bool) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if unescaped(c) {
            out.push(c);
        } else {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}

fn is_uri_unreserved(c: char) -> bool {
    c.is_ascii_alphanumeric() || "-_.!~*'()".contains(c)
}

/// `encodeURIComponent`
pub fn encode_uri_component(s: &str) -> String {
    encode(s, &is_uri_unreserved)
}

/// `encodeURI`
pub fn encode_uri(s: &str) -> String {
    encode(s, &|c| is_uri_unreserved(c) || ";/?:@&=+$,#".contains(c))
}

/// Character class `[A-Za-z0-9_]` (`\w` without the `u` flag).
pub fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}
