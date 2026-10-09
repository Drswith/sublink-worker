//! Outbound HTTP used to download remote subscriptions.
//!
//! Abstracted behind a trait so tests can serve canned responses, the way the
//! original test-suite stubbed `globalThis.fetch`.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use crate::js::base64::utf8_decode;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Clone, Debug, Default)]
pub struct FetchResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
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

/// reqwest-backed fetcher mirroring Node's fetch defaults (redirects followed,
/// compressed bodies decoded, proxy environment honored).
pub struct HttpFetcher {
    client: reqwest::Client,
}

impl HttpFetcher {
    pub fn new() -> Result<Self, String> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::limited(20))
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
            let parsed = url::Url::parse(url).map_err(|_| "TypeError: Failed to parse URL".to_string())?;
            let mut req = self
                .client
                .get(parsed)
                .header("accept", "*/*")
                .header("accept-language", "*")
                .header("sec-fetch-mode", "cors");
            req = req.header("user-agent", user_agent.unwrap_or("node"));
            let resp = req.send().await.map_err(|e| format!("fetch failed: {}", e.without_url()))?;
            let status = resp.status().as_u16();
            let headers = resp
                .headers()
                .iter()
                .map(|(k, v)| (k.as_str().to_string(), v.as_bytes().iter().map(|&b| b as char).collect()))
                .collect();
            let body = resp.bytes().await.map_err(|e| format!("fetch failed: {}", e.without_url()))?.to_vec();
            Ok(FetchResponse { status, headers, body })
        })
    }
}
