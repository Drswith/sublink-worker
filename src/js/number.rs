//! ECMAScript number conversions (`Number()`, `parseInt`, `parseFloat`, `String(n)`).

use super::string::{is_js_whitespace, js_trim, js_trim_start};

/// Number::toString (also used by JSON.stringify for finite numbers).
pub fn js_number_to_string(n: f64) -> String {
    let mut buf = ryu_js::Buffer::new();
    buf.format(n).to_string()
}

fn is_decimal_digit(c: char) -> bool {
    c.is_ascii_digit()
}

/// Length (in chars) of the longest prefix of `s` matching StrUnsignedDecimalLiteral.
fn unsigned_decimal_prefix_len(s: &[char]) -> usize {
    let mut i = 0;
    let int_start = i;
    while i < s.len() && is_decimal_digit(s[i]) {
        i += 1;
    }
    let int_digits = i - int_start;
    let mut frac_digits = 0;
    if i < s.len() && s[i] == '.' {
        let mut j = i + 1;
        while j < s.len() && is_decimal_digit(s[j]) {
            j += 1;
        }
        frac_digits = j - (i + 1);
        if int_digits > 0 || frac_digits > 0 {
            i = j;
        }
    }
    if int_digits == 0 && frac_digits == 0 {
        return 0;
    }
    if i < s.len() && (s[i] == 'e' || s[i] == 'E') {
        let mut j = i + 1;
        if j < s.len() && (s[j] == '+' || s[j] == '-') {
            j += 1;
        }
        let exp_start = j;
        while j < s.len() && is_decimal_digit(s[j]) {
            j += 1;
        }
        if j > exp_start {
            i = j;
        }
    }
    i
}

fn parse_decimal(text: &str) -> f64 {
    text.parse::<f64>().unwrap_or(f64::NAN)
}

/// StringToNumber (`Number("...")`).
pub fn string_to_number(input: &str) -> f64 {
    let trimmed = js_trim(input);
    if trimmed.is_empty() {
        return 0.0;
    }
    let lower_prefix = trimmed.get(..2).map(|p| p.to_ascii_lowercase());
    if let Some(prefix) = lower_prefix.as_deref() {
        let radix = match prefix {
            "0x" => Some(16),
            "0o" => Some(8),
            "0b" => Some(2),
            _ => None,
        };
        if let Some(radix) = radix {
            let digits = &trimmed[2..];
            if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
                return f64::NAN;
            }
            return digits.chars().fold(0.0, |acc, c| acc * radix as f64 + c.to_digit(radix).unwrap() as f64);
        }
    }
    let chars: Vec<char> = trimmed.chars().collect();
    let (sign, rest) = match chars.first() {
        Some('+') => (1.0, &chars[1..]),
        Some('-') => (-1.0, &chars[1..]),
        _ => (1.0, &chars[..]),
    };
    let rest_str: String = rest.iter().collect();
    if rest_str == "Infinity" {
        return sign * f64::INFINITY;
    }
    let len = unsigned_decimal_prefix_len(rest);
    if len == 0 || len != rest.len() {
        return f64::NAN;
    }
    sign * parse_decimal(&rest_str)
}

/// `parseFloat(string)`
pub fn parse_float(input: &str) -> f64 {
    let s = js_trim_start(input);
    let chars: Vec<char> = s.chars().collect();
    let (sign, rest) = match chars.first() {
        Some('+') => (1.0, &chars[1..]),
        Some('-') => (-1.0, &chars[1..]),
        _ => (1.0, &chars[..]),
    };
    let rest_str: String = rest.iter().collect();
    if rest_str.starts_with("Infinity") {
        return sign * f64::INFINITY;
    }
    let len = unsigned_decimal_prefix_len(rest);
    if len == 0 {
        return f64::NAN;
    }
    let text: String = rest[..len].iter().collect();
    sign * parse_decimal(&text)
}

/// `parseInt(string, radix)` where `radix` 0 means "auto" (10, or 16 with 0x).
pub fn parse_int(input: &str, radix: u32) -> f64 {
    let s = js_trim_start(input);
    let mut chars: &str = s;
    let mut sign = 1.0;
    if let Some(rest) = chars.strip_prefix('-') {
        sign = -1.0;
        chars = rest;
    } else if let Some(rest) = chars.strip_prefix('+') {
        chars = rest;
    }
    let mut r = radix;
    let mut strip_prefix = true;
    if r != 0 {
        if !(2..=36).contains(&r) {
            return f64::NAN;
        }
        if r != 16 {
            strip_prefix = false;
        }
    } else {
        r = 10;
    }
    if strip_prefix && (chars.starts_with("0x") || chars.starts_with("0X")) {
        chars = &chars[2..];
        r = 16;
    }
    let end = chars.char_indices().find(|(_, c)| !c.is_digit(r)).map(|(i, _)| i).unwrap_or(chars.len());
    let digits = &chars[..end];
    if digits.is_empty() {
        return f64::NAN;
    }
    let magnitude = if r == 10 {
        parse_decimal(digits)
    } else {
        digits.chars().fold(0.0, |acc, c| acc * r as f64 + c.to_digit(r).unwrap() as f64)
    };
    if magnitude == 0.0 {
        return if sign < 0.0 { -0.0 } else { 0.0 };
    }
    sign * magnitude
}

/// `Number.isInteger`-like check used by js-yaml (`n % 1 === 0`).
pub fn is_integral(n: f64) -> bool {
    n.is_finite() && n.fract() == 0.0
}

pub fn is_negative_zero(n: f64) -> bool {
    n == 0.0 && n.is_sign_negative()
}

/// Any JS whitespace or line terminator (used by `\s`).
pub fn is_js_space(c: char) -> bool {
    is_js_whitespace(c)
}
