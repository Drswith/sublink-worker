//! Ports of the hand-written Base64 helpers in the original `utils.js`.
//! `base64_to_binary` is intentionally lenient (invalid characters become odd
//! code points) because parsers depend on that exact behavior.

const CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn index_of(c: Option<char>) -> i32 {
    match c {
        Some(c) if c.is_ascii() => CHARS.iter().position(|&b| b as char == c).map(|p| p as i32).unwrap_or(-1),
        _ => -1,
    }
}

/// `base64ToBinary`: returns a "binary string" whose chars are byte values
/// (or 0xFFFC..=0xFFFF for invalid input, mirroring `String.fromCharCode`).
pub fn base64_to_binary(input: &str) -> String {
    let trimmed = input.trim_end_matches('=');
    let units: Vec<char> = trimmed.encode_utf16().map(|u| char::from_u32(u as u32).unwrap_or('\u{FFFD}')).collect();
    let mut out = String::new();
    let mut i = 0;
    while i < units.len() {
        let b0 = index_of(units.get(i).copied());
        let b1 = index_of(units.get(i + 1).copied());
        let b2 = index_of(units.get(i + 2).copied());
        let b3 = index_of(units.get(i + 3).copied());
        let byte1 = (b0 << 2) | (b1 >> 4);
        let byte2 = ((b1 & 15) << 4) | (b2 >> 2);
        let byte3 = ((b2 & 3) << 6) | b3;
        let push = |out: &mut String, v: i32| {
            let code = (v as u32) & 0xFFFF;
            out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
        };
        if b1 != -1 {
            push(&mut out, byte1);
        }
        if b2 != -1 {
            push(&mut out, byte2);
        }
        if b3 != -1 {
            push(&mut out, byte3);
        }
        i += 4;
    }
    out
}

/// TextDecoder('utf-8').decode: lossy, strips a leading BOM.
pub fn utf8_decode(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8_lossy(bytes).into_owned()
}

/// `decodeBase64`
pub fn decode_base64(input: &str) -> String {
    let binary = base64_to_binary(input);
    let bytes: Vec<u8> = binary.chars().map(|c| (c as u32 & 0xFF) as u8).collect();
    utf8_decode(&bytes)
}

/// `encodeBase64` (UTF-8 then standard padded Base64).
pub fn encode_base64(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        out.push(CHARS[(b[0] >> 2) as usize] as char);
        out.push(CHARS[(((b[0] & 3) << 4) | (b[1] >> 4)) as usize] as char);
        out.push(if chunk.len() > 1 { CHARS[(((b[1] & 15) << 2) | (b[2] >> 6)) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { CHARS[(b[2] & 63) as usize] as char } else { '=' });
    }
    out
}
