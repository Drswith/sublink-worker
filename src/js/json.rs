//! `JSON.parse` / `JSON.stringify` with V8-compatible behavior, including the
//! exact SyntaxError messages (they are returned to API clients verbatim).

use super::JsError;
use super::date::to_iso_string;
use super::number::js_number_to_string;
use super::string::from_utf16;
use super::value::{Object, Value};

// ---------------------------------------------------------------------------
// JSON.stringify
// ---------------------------------------------------------------------------

pub fn quote_json_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn write_value(v: &Value, out: &mut String) -> bool {
    match v {
        Value::Undefined => return false,
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if n.is_finite() {
                out.push_str(&js_number_to_string(*n));
            } else {
                out.push_str("null");
            }
        }
        Value::String(s) => quote_json_string(s, out),
        Value::Date(t) => match to_iso_string(*t) {
            Some(iso) => quote_json_string(&iso, out),
            None => out.push_str("null"),
        },
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                if !write_value(item, out) {
                    out.push_str("null");
                }
            }
            out.push(']');
        }
        Value::Object(obj) => {
            out.push('{');
            let mut first = true;
            for (k, val) in obj.entries() {
                if val.is_undefined() {
                    continue;
                }
                if !first {
                    out.push(',');
                }
                first = false;
                quote_json_string(k, out);
                out.push(':');
                write_value(val, out);
            }
            out.push('}');
        }
    }
    true
}

/// `JSON.stringify(v)`; `None` when the result is `undefined`.
pub fn stringify(v: &Value) -> Option<String> {
    let mut out = String::new();
    if write_value(v, &mut out) { Some(out) } else { None }
}

/// `JSON.stringify(v)` coerced into a string the way template literals would
/// (`"undefined"` for undefined input).
pub fn stringify_or_undefined(v: &Value) -> String {
    stringify(v).unwrap_or_else(|| "undefined".into())
}

// ---------------------------------------------------------------------------
// JSON.parse
// ---------------------------------------------------------------------------

const END: i32 = -1;
const MAX_CONTEXT: usize = 10;
const MIN_LEN_FOR_CONTEXT: usize = MAX_CONTEXT * 2 + 1;

#[derive(Clone, Copy, PartialEq)]
enum Token {
    String,
    Number,
    LBrace,
    RBrace,
    LBrack,
    RBrack,
    True,
    False,
    Null,
    Whitespace,
    Colon,
    Comma,
    Illegal,
    Eos,
}

fn token_for(c: i32) -> Token {
    if c == END {
        return Token::Eos;
    }
    match c as u32 {
        0x22 => Token::String,
        0x2D | 0x30..=0x39 => Token::Number,
        0x7B => Token::LBrace,
        0x7D => Token::RBrace,
        0x5B => Token::LBrack,
        0x5D => Token::RBrack,
        0x74 => Token::True,
        0x66 => Token::False,
        0x6E => Token::Null,
        0x20 | 0x09 | 0x0A | 0x0D => Token::Whitespace,
        0x3A => Token::Colon,
        0x2C => Token::Comma,
        _ => Token::Illegal,
    }
}

enum Msg {
    ExpectedPropNameOrRBrace,
    ExpectedDoubleQuotedPropertyName,
    ExpectedCommaOrRBrace,
    ExpectedCommaOrRBrack,
    ExpectedColonAfterPropertyName,
    UnterminatedString,
    BadControlCharacter,
    BadUnicodeEscape,
    BadEscapedCharacter,
    NoNumberAfterMinusSign,
    ExponentPartMissingNumber,
    UnterminatedFractionalNumber,
    UnexpectedNonWhiteSpaceCharacter,
}

impl Msg {
    fn text(&self) -> &'static str {
        match self {
            Msg::ExpectedPropNameOrRBrace => "Expected property name or '}' in JSON",
            Msg::ExpectedDoubleQuotedPropertyName => "Expected double-quoted property name in JSON",
            Msg::ExpectedCommaOrRBrace => "Expected ',' or '}' after property value in JSON",
            Msg::ExpectedCommaOrRBrack => "Expected ',' or ']' after array element in JSON",
            Msg::ExpectedColonAfterPropertyName => "Expected ':' after property name in JSON",
            Msg::UnterminatedString => "Unterminated string in JSON",
            Msg::BadControlCharacter => "Bad control character in string literal in JSON",
            Msg::BadUnicodeEscape => "Bad Unicode escape in JSON",
            Msg::BadEscapedCharacter => "Bad escaped character in JSON",
            Msg::NoNumberAfterMinusSign => "No number after minus sign in JSON",
            Msg::ExponentPartMissingNumber => "Exponent part is missing a number in JSON",
            Msg::UnterminatedFractionalNumber => "Unterminated fractional number in JSON",
            Msg::UnexpectedNonWhiteSpaceCharacter => "Unexpected non-whitespace character after JSON",
        }
    }
}

struct Parser<'a> {
    src: &'a [u16],
    pos: usize,
}

type PResult<T> = Result<T, JsError>;

impl<'a> Parser<'a> {
    fn cur(&self) -> i32 {
        self.src.get(self.pos).map(|&c| c as i32).unwrap_or(END)
    }

    fn next(&mut self) -> i32 {
        self.pos += 1;
        self.cur()
    }

    fn skip_ws(&mut self) {
        while matches!(self.cur(), 0x20 | 0x09 | 0x0A | 0x0D) {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Token {
        token_for(self.cur())
    }

    fn line_col(&self) -> (usize, usize) {
        let mut line = 1;
        let mut last_break = 0;
        let mut i = 0;
        while i < self.pos {
            if self.src[i] == 0x0D && i + 1 < self.pos && self.src[i + 1] == 0x0A {
                i += 1;
            }
            if self.src[i] == 0x0D || self.src[i] == 0x0A {
                line += 1;
                last_break = i + 1;
            }
            i += 1;
        }
        (line, 1 + i - last_break)
    }

    fn located(&self, text: &str) -> JsError {
        let (line, col) = self.line_col();
        JsError::syntax(format!("{} at position {} (line {} column {})", text, self.pos, line, col))
    }

    fn is_special_string(&self) -> bool {
        let s = from_utf16(self.src);
        matches!(s.as_str(), "[object Object]" | "undefined" | "Infinity" | "NaN")
    }

    fn unexpected(&self, token: Token, msg: Option<Msg>) -> JsError {
        if let Some(m) = msg {
            return self.located(m.text());
        }
        match token {
            Token::Eos => JsError::syntax("Unexpected end of JSON input"),
            Token::Number => self.located("Unexpected number in JSON"),
            Token::String => self.located("Unexpected string in JSON"),
            _ => {
                if self.is_special_string() {
                    return JsError::syntax(format!("\"{}\" is not valid JSON", from_utf16(self.src)));
                }
                let ch = from_utf16(&self.src[self.pos..self.pos + 1]);
                let len = self.src.len();
                let pos = self.pos;
                if len < MIN_LEN_FOR_CONTEXT {
                    return JsError::syntax(format!(
                        "Unexpected token '{}', \"{}\" is not valid JSON",
                        ch,
                        from_utf16(self.src)
                    ));
                }
                let (start, end, prefix, suffix) = if pos < MAX_CONTEXT {
                    (0, pos + MAX_CONTEXT, "", "...")
                } else if pos < len - MAX_CONTEXT {
                    (pos - MAX_CONTEXT, pos + MAX_CONTEXT, "...", "...")
                } else {
                    (pos - MAX_CONTEXT, len, "...", "")
                };
                JsError::syntax(format!(
                    "Unexpected token '{}', {}\"{}\"{} is not valid JSON",
                    ch,
                    prefix,
                    from_utf16(&self.src[start..end.min(len)]),
                    suffix
                ))
            }
        }
    }

    fn unexpected_char(&self) -> JsError {
        let c = self.cur();
        let token = if c == END {
            Token::Eos
        } else if c <= 0xFF {
            token_for(c)
        } else {
            Token::Illegal
        };
        self.unexpected(token, None)
    }

    fn check(&mut self, token: Token) -> bool {
        self.skip_ws();
        if self.peek() != token {
            return false;
        }
        self.pos += 1;
        true
    }

    fn expect_next(&mut self, token: Token, msg: Msg) -> PResult<()> {
        self.skip_ws();
        self.expect(token, msg)
    }

    fn expect(&mut self, token: Token, msg: Msg) -> PResult<()> {
        if self.peek() == token {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.unexpected(self.peek(), Some(msg)))
        }
    }

    fn scan_literal(&mut self, lit: &str) -> PResult<()> {
        let lit: Vec<u16> = lit.encode_utf16().collect();
        let n = lit.len();
        let remaining = self.src.len() - self.pos;
        if remaining >= n && self.src[self.pos + 1..self.pos + n] == lit[1..] {
            self.pos += n;
            return Ok(());
        }
        self.pos += 1;
        for i in 0..(n - 1).min(remaining - 1) {
            if lit[1 + i] as i32 != self.cur() {
                return Err(self.unexpected_char());
            }
            self.pos += 1;
        }
        Err(self.unexpected(Token::Eos, None))
    }

    fn scan_string(&mut self) -> PResult<String> {
        // cursor is just past the opening quote
        let mut buf: Vec<u16> = Vec::new();
        loop {
            let c = self.cur();
            if c == END {
                return Err(self.unexpected(Token::Illegal, Some(Msg::UnterminatedString)));
            }
            if c == 0x22 {
                self.pos += 1;
                return Ok(from_utf16(&buf));
            }
            if c == 0x5C {
                let e = self.next();
                if e == END || e > 0xFF {
                    return Err(self.unexpected_char());
                }
                match e as u8 {
                    b'"' | b'\\' | b'/' => buf.push(e as u16),
                    b'b' => buf.push(0x08),
                    b'f' => buf.push(0x0C),
                    b'n' => buf.push(0x0A),
                    b'r' => buf.push(0x0D),
                    b't' => buf.push(0x09),
                    b'u' => {
                        let mut value: u32 = 0;
                        for _ in 0..4 {
                            let h = self.next();
                            let d = if h == END { None } else { char::from_u32(h as u32).and_then(|c| c.to_digit(16)) };
                            match d {
                                Some(d) => value = value * 16 + d,
                                None => {
                                    return Err(self.unexpected(Token::Illegal, Some(Msg::BadUnicodeEscape)));
                                }
                            }
                        }
                        buf.push(value as u16);
                    }
                    _ => return Err(self.unexpected(Token::Illegal, Some(Msg::BadEscapedCharacter))),
                }
                self.pos += 1;
                continue;
            }
            if c < 0x20 {
                return Err(self.unexpected(Token::Illegal, Some(Msg::BadControlCharacter)));
            }
            buf.push(c as u16);
            self.pos += 1;
        }
    }

    fn is_number_part(c: i32) -> bool {
        matches!(c, 0x30..=0x39 | 0x2D | 0x2B | 0x2E | 0x65 | 0x45)
    }

    fn is_digit(c: i32) -> bool {
        (0x30..=0x39).contains(&c)
    }

    fn advance_to_non_decimal(&mut self) {
        while Self::is_digit(self.cur()) {
            self.pos += 1;
        }
    }

    fn parse_number(&mut self) -> PResult<f64> {
        let start = self.pos;
        let mut c = self.cur();
        let mut negative = false;
        if c == 0x2D {
            negative = true;
            c = self.next();
        }
        if c == 0x30 {
            c = self.next();
            if Self::is_number_part(c) {
                if Self::is_digit(c) {
                    return Err(self.unexpected(Token::Number, None));
                }
            } else if !negative {
                return Ok(0.0);
            }
        } else {
            let digits_start = self.pos;
            self.advance_to_non_decimal();
            if digits_start == self.pos {
                return Err(self.unexpected(Token::Illegal, Some(Msg::NoNumberAfterMinusSign)));
            }
        }
        if self.cur() == 0x2E {
            let c = self.next();
            if !Self::is_digit(c) {
                return Err(self.unexpected(Token::Illegal, Some(Msg::UnterminatedFractionalNumber)));
            }
            self.advance_to_non_decimal();
        }
        if self.cur() == 0x65 || self.cur() == 0x45 {
            let mut c = self.next();
            if c == 0x2D || c == 0x2B {
                c = self.next();
            }
            if !Self::is_digit(c) {
                return Err(self.unexpected(Token::Illegal, Some(Msg::ExponentPartMissingNumber)));
            }
            self.advance_to_non_decimal();
        }
        let text = from_utf16(&self.src[start..self.pos]);
        Ok(text.parse::<f64>().unwrap_or(f64::NAN))
    }

    fn parse_value(&mut self) -> PResult<Value> {
        self.skip_ws();
        match self.peek() {
            Token::String => {
                self.pos += 1;
                Ok(Value::String(self.scan_string()?))
            }
            Token::Number => Ok(Value::Number(self.parse_number()?)),
            Token::LBrace => {
                self.pos += 1;
                let mut obj = Object::new();
                if self.check(Token::RBrace) {
                    return Ok(Value::Object(obj));
                }
                self.expect_next(Token::String, Msg::ExpectedPropNameOrRBrace)?;
                let mut key = self.scan_string()?;
                self.expect_next(Token::Colon, Msg::ExpectedColonAfterPropertyName)?;
                loop {
                    let value = self.parse_value()?;
                    obj.set(key, value);
                    if self.check(Token::Comma) {
                        self.expect_next(Token::String, Msg::ExpectedDoubleQuotedPropertyName)?;
                        key = self.scan_string()?;
                        self.expect_next(Token::Colon, Msg::ExpectedColonAfterPropertyName)?;
                        continue;
                    }
                    self.expect(Token::RBrace, Msg::ExpectedCommaOrRBrace)?;
                    return Ok(Value::Object(obj));
                }
            }
            Token::LBrack => {
                self.pos += 1;
                let mut items = Vec::new();
                if self.check(Token::RBrack) {
                    return Ok(Value::array(items));
                }
                loop {
                    items.push(self.parse_value()?);
                    if self.check(Token::Comma) {
                        continue;
                    }
                    self.expect(Token::RBrack, Msg::ExpectedCommaOrRBrack)?;
                    return Ok(Value::array(items));
                }
            }
            Token::True => {
                self.scan_literal("true")?;
                Ok(Value::Bool(true))
            }
            Token::False => {
                self.scan_literal("false")?;
                Ok(Value::Bool(false))
            }
            Token::Null => {
                self.scan_literal("null")?;
                Ok(Value::Null)
            }
            _ => Err(self.unexpected_char()),
        }
    }
}

/// `JSON.parse(text)`
pub fn parse(text: &str) -> Result<Value, JsError> {
    let src: Vec<u16> = text.encode_utf16().collect();
    let mut p = Parser { src: &src, pos: 0 };
    let value = p.parse_value()?;
    if !p.check(Token::Eos) {
        return Err(p.unexpected(p.peek(), Some(Msg::UnexpectedNonWhiteSpaceCharacter)));
    }
    Ok(value)
}

/// `JSON.parse(String(v))` for an arbitrary value (e.g. `JSON.parse(undefined)`).
pub fn parse_value(v: &Value) -> Result<Value, JsError> {
    parse(&v.to_js_string())
}
