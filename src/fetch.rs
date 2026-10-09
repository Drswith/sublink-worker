//! Outbound HTTP used to download remote subscriptions.
//!
//! Abstracted behind a trait so tests can serve canned responses, the way the
//! original test-suite stubbed `globalThis.fetch`. `HttpFetcher` follows
//! redirects and decodes bodies itself so that edge cases (odd
//! `Content-Encoding` values, truncated streams, unusual `Location` headers)
//! end the same way they did under Node's fetch (undici).

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use flate2::{Decompress, FlushDecompress, Status};
use reqwest::header::HeaderValue;
use url::Url;

use crate::js::base64::utf8_decode;
use crate::js::string::js_trim;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Clone, Debug, Default)]
pub struct FetchResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// The headers arrived but the body could not be read or decoded: the
    /// original saw `response.headers`, then `response.text()` rejected.
    pub body_error: Option<String>,
}

impl FetchResponse {
    /// `response.ok`
    pub fn ok(&self) -> bool {
        (200..=299).contains(&self.status)
    }

    /// `response.headers.get(name)` (case-insensitive, repeated values joined).
    pub fn header(&self, name: &str) -> Option<String> {
        let values: Vec<&str> =
            self.headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str()).collect();
        if values.is_empty() { None } else { Some(values.join(", ")) }
    }

    /// `response.text()`: UTF-8 with BOM stripping and replacement characters.
    pub fn text(&self) -> String {
        utf8_decode(&self.body)
    }
}

pub trait Fetcher: Send + Sync {
    /// GET `url`, sending `User-Agent` when provided.
    fn get<'a>(&'a self, url: &'a str, user_agent: Option<&'a str>) -> BoxFuture<'a, Result<FetchResponse, String>>;
}

const REDIRECT_STATUSES: [u16; 5] = [301, 302, 303, 307, 308];
const NULL_BODY_STATUSES: [u16; 4] = [101, 204, 205, 304];
const MAX_REDIRECTS: usize = 20;

/// reqwest-backed fetcher mirroring Node's fetch: same default request
/// headers, redirect rules and content decoding; proxy environment honored.
pub struct HttpFetcher {
    client: reqwest::Client,
}

impl HttpFetcher {
    pub fn new() -> Result<Self, String> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(300))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(HttpFetcher { client })
    }
}

impl Fetcher for HttpFetcher {
    fn get<'a>(&'a self, url: &'a str, user_agent: Option<&'a str>) -> BoxFuture<'a, Result<FetchResponse, String>> {
        Box::pin(async move {
            // Errors end up in logs, and subscription URLs often embed credentials.
            let mut url = Url::parse(url).map_err(|_| "TypeError: Failed to parse URL".to_string())?;
            if has_credentials(&url) {
                return Err("TypeError: Request cannot be constructed from a URL that includes credentials".into());
            }
            let user_agent = match user_agent {
                Some(ua) => user_agent_header(ua)?,
                None => HeaderValue::from_static("node"),
            };
            let mut redirects = 0;
            loop {
                let response = self
                    .client
                    .get(url.clone())
                    .header("accept", "*/*")
                    .header("accept-language", "*")
                    .header("sec-fetch-mode", "cors")
                    .header("user-agent", user_agent.clone())
                    .header("accept-encoding", "gzip, deflate")
                    .send()
                    .await
                    .map_err(|e| format!("fetch failed: {}", e.without_url()))?;
                let status = response.status().as_u16();
                if REDIRECT_STATUSES.contains(&status)
                    && let Some(next) = location(response.headers(), &url)?
                {
                    if redirects == MAX_REDIRECTS {
                        return Err("fetch failed: redirect count exceeded".into());
                    }
                    redirects += 1;
                    url = next;
                    continue;
                }
                let headers: Vec<(String, String)> =
                    response.headers().iter().map(|(k, v)| (k.as_str().to_string(), latin1(v.as_bytes()))).collect();
                let encoding = joined(response.headers(), "content-encoding").map(|v| latin1(&v));
                let (body, body_error) = match response.bytes().await {
                    Ok(raw) => match decode_body(encoding.as_deref(), status, raw.to_vec()) {
                        Ok(body) => (body, None),
                        Err(e) => (Vec::new(), Some(format!("TypeError: terminated: {e}"))),
                    },
                    Err(e) => (Vec::new(), Some(format!("TypeError: terminated: {}", e.without_url()))),
                };
                return Ok(FetchResponse { status, headers, body, body_error });
            }
        })
    }
}

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

/// Header values joined the way `headers.get()` does.
fn joined(headers: &reqwest::header::HeaderMap, name: &str) -> Option<Vec<u8>> {
    let values: Vec<&[u8]> = headers.get_all(name).iter().map(HeaderValue::as_bytes).collect();
    if values.is_empty() { None } else { Some(values.join(&b", "[..])) }
}

fn has_credentials(url: &Url) -> bool {
    !url.username().is_empty() || url.password().is_some()
}

fn is_http_whitespace(b: u8) -> bool {
    matches!(b, b'\t' | b'\n' | b'\r' | b' ')
}

/// `headers.set('User-Agent', ua)`: the value must be a ByteString (sent as
/// Latin-1 bytes, not UTF-8) and is trimmed of HTTP whitespace.
fn user_agent_header(ua: &str) -> Result<HeaderValue, String> {
    let bytes = ua
        .chars()
        .map(|c| u8::try_from(c as u32))
        .collect::<Result<Vec<u8>, _>>()
        .map_err(|_| "TypeError: Cannot convert argument to a ByteString".to_string())?;
    let start = bytes.iter().position(|&b| !is_http_whitespace(b)).unwrap_or(bytes.len());
    let end = bytes.iter().rposition(|&b| !is_http_whitespace(b)).map_or(start, |i| i + 1);
    HeaderValue::from_bytes(&bytes[start..end]).map_err(|_| "TypeError: Invalid header value".to_string())
}

/// undici's `responseLocationURL` plus the checks that make a redirect fail.
fn location(headers: &reqwest::header::HeaderMap, base: &Url) -> Result<Option<Url>, String> {
    let Some(raw) = joined(headers, "location") else { return Ok(None) };
    let padded =
        raw.first().is_some_and(|&b| b == b' ' || b == b'\t') || raw.last().is_some_and(|&b| b == b' ' || b == b'\t');
    if padded || raw.iter().any(|&b| matches!(b, 0 | b'\r' | b'\n')) {
        // undici keeps the raw string and then fails assigning its `hash`.
        return Err("fetch failed: invalid Location header".into());
    }
    // Non-ASCII bytes are reinterpreted as UTF-8 (`normalizeBinaryStringToUtf8`).
    let text = String::from_utf8_lossy(&raw);
    let next = base.join(&text).map_err(|_| "fetch failed: Invalid URL".to_string())?;
    if !matches!(next.scheme(), "http" | "https") {
        return Err("fetch failed: URL scheme must be a HTTP(S) scheme".into());
    }
    if has_credentials(&next) {
        return Err("fetch failed: cross origin not allowed for request mode \"cors\"".into());
    }
    Ok(Some(next))
}

type Decoder = fn(&[u8]) -> Result<Vec<u8>, String>;

/// undici's content decoding: codings are applied last to first, and a single
/// unknown coding (including `identity` or an empty item) disables decoding.
fn decode_body(encoding: Option<&str>, status: u16, body: Vec<u8>) -> Result<Vec<u8>, String> {
    let Some(encoding) = encoding.filter(|e| !e.is_empty()) else { return Ok(body) };
    if NULL_BODY_STATUSES.contains(&status) {
        return Ok(body);
    }
    let mut decoders: Vec<Decoder> = Vec::new();
    for coding in encoding.to_lowercase().split(',').rev() {
        match js_trim(coding) {
            "gzip" | "x-gzip" => decoders.push(gunzip),
            "deflate" => decoders.push(inflate),
            "br" => decoders.push(unbrotli),
            _ => return Ok(body),
        }
    }
    decoders.into_iter().try_fold(body, |data, decode| decode(&data))
}

/// Runs `d` over `input` with `Z_SYNC_FLUSH`, the finishing flush undici uses:
/// a truncated stream ends without an error. Returns the bytes consumed and
/// whether the stream reached its end.
fn inflate_into(d: &mut Decompress, input: &[u8], out: &mut Vec<u8>) -> Result<(usize, bool), String> {
    loop {
        let consumed = d.total_in() as usize;
        if out.capacity() - out.len() < 32 * 1024 {
            out.reserve(64 * 1024);
        }
        let produced = out.len();
        let status = d.decompress_vec(&input[consumed..], out, FlushDecompress::Sync).map_err(|e| e.to_string())?;
        if status == Status::StreamEnd {
            return Ok((d.total_in() as usize, true));
        }
        if d.total_in() as usize == consumed && out.len() == produced {
            return Ok((consumed, false));
        }
    }
}

/// `deflate` per undici: zlib-wrapped when the first byte says so, raw
/// otherwise. Data after the end of the stream is ignored.
fn inflate(input: &[u8]) -> Result<Vec<u8>, String> {
    let Some(&first) = input.first() else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    inflate_into(&mut Decompress::new(first & 0x0f == 0x08), input, &mut out)?;
    Ok(out)
}

/// Node's gunzip: zlib checks each member's header and trailer, members are
/// decoded back to back, and leftover input is ignored only when it starts
/// with a zero byte (Node treats that as padding).
fn gunzip(mut input: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let (consumed, ended) = inflate_into(&mut Decompress::new_gzip(15), input, &mut out)?;
        input = &input[consumed..];
        if !ended || input.first().is_none_or(|&b| b == 0) {
            return Ok(out);
        }
    }
}

/// Brotli with `BROTLI_OPERATION_FLUSH` as the finishing flush: truncated input
/// yields what was decoded so far and trailing data is ignored.
fn unbrotli(input: &[u8]) -> Result<Vec<u8>, String> {
    use brotli_decompressor::{BrotliDecompressStream, BrotliResult, BrotliState, StandardAlloc};
    let mut state = BrotliState::new(StandardAlloc::default(), StandardAlloc::default(), StandardAlloc::default());
    let (mut available_in, mut input_offset) = (input.len(), 0);
    let mut buf = vec![0u8; 64 * 1024];
    let mut out = Vec::new();
    loop {
        let (mut available_out, mut output_offset, mut total_out) = (buf.len(), 0, 0);
        let result = BrotliDecompressStream(
            &mut available_in,
            &mut input_offset,
            input,
            &mut available_out,
            &mut output_offset,
            &mut buf,
            &mut total_out,
            &mut state,
        );
        out.extend_from_slice(&buf[..output_offset]);
        match result {
            BrotliResult::NeedsMoreOutput => continue,
            BrotliResult::ResultSuccess | BrotliResult::NeedsMoreInput => return Ok(out),
            BrotliResult::ResultFailure => return Err("brotli decoding failed".into()),
        }
    }
}
