//! HttpFetcher ends the same way Node's fetch did for the raw upstream responses
//! recorded in fixtures/fetch_cases.json (content codings, truncated or broken
//! bodies, redirects, credentials, User-Agent values).

use std::collections::HashMap;
use std::sync::Arc;

use sublink::fetch::{Fetcher, HttpFetcher};
use sublink::js::base64::{base64_to_binary, encode_base64};
use sublink::js::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

/// Serves each route's raw response, like the generator's server.
async fn serve(routes: Arc<HashMap<String, Vec<u8>>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let host = listener.local_addr().unwrap().to_string();
    let fill = {
        let host = host.clone();
        move |raw: &[u8]| -> Vec<u8> {
            latin1(raw)
                .replace("{{CREDS}}", &format!("http://u:p@{host}"))
                .replace("{{HOST}}", &host)
                .chars()
                .map(|c| c as u8)
                .collect()
        }
    };
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = listener.accept().await.unwrap();
            let routes = routes.clone();
            let fill = fill.clone();
            tokio::spawn(async move {
                let mut data = Vec::new();
                let mut buf = [0u8; 4096];
                while !data.windows(4).any(|w| w == b"\r\n\r\n") {
                    match sock.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => data.extend_from_slice(&buf[..n]),
                    }
                }
                let head = latin1(&data);
                let path = head.split(' ').nth(1).unwrap_or("").to_string();
                let out = if path == "/ua" {
                    let ua = head
                        .split("\r\n")
                        .find(|l| l.to_ascii_lowercase().starts_with("user-agent:"))
                        .map(|l| l[11..].chars().map(|c| format!("{:02x}", c as u32)).collect::<String>())
                        .unwrap_or_else(|| "<none>".into());
                    let body = format!("trojan://p@ua.example.com:443#UA[{ua}]\n");
                    format!("HTTP/1.1 200 X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
                        .into_bytes()
                } else if let Some(raw) = routes.get(&path) {
                    fill(raw)
                } else {
                    b"HTTP/1.1 404 X\r\nContent-Length: 2\r\nConnection: close\r\n\r\nnf".to_vec()
                };
                let _ = sock.write_all(&out).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    host
}

#[tokio::test]
async fn http_fetcher_matches_node_fetch() {
    let fixture = json::parse(include_str!("fixtures/fetch_cases.json")).unwrap();
    let routes: HashMap<String, Vec<u8>> = fixture
        .get("routes")
        .own_entries()
        .into_iter()
        .map(|(path, raw)| (path, base64_to_binary(raw.as_str().unwrap()).chars().map(|c| c as u8).collect()))
        .collect();
    let host = serve(Arc::new(routes)).await;
    let fetcher = HttpFetcher::new().unwrap();

    let mut failures = Vec::new();
    for case in fixture.get("cases").as_array().unwrap() {
        let path = case.get("path").as_str().unwrap();
        let credentials = if case.get("credentials").truthy() { "u:p@" } else { "" };
        let url = format!("http://{credentials}{host}{path}");
        // fetchSubscriptionWithFormat only sets a non-empty User-Agent.
        let ua = case.get("ua").as_str().filter(|ua| !ua.is_empty());
        let got = match fetcher.get(&url, ua).await {
            Err(_) => "error".to_string(),
            Ok(res) => {
                let ui = res.header("subscription-userinfo").map(Value::String).unwrap_or(Value::Null);
                match res.body_error {
                    Some(_) => format!("body-error {} {}", res.status, json::stringify(&ui).unwrap()),
                    None => {
                        format!("ok {} {} {}", res.status, json::stringify(&ui).unwrap(), encode_base64(&res.text()))
                    }
                }
            }
        };
        let o = case.get("outcome");
        let want = match o.get("kind").as_str().unwrap() {
            "error" => "error".to_string(),
            "body-error" => {
                format!("body-error {} {}", o.get("status").to_js_string(), json::stringify(o.get("ui")).unwrap())
            }
            _ => format!(
                "ok {} {} {}",
                o.get("status").to_js_string(),
                json::stringify(o.get("ui")).unwrap(),
                encode_base64(o.get("text").as_str().unwrap())
            ),
        };
        if got != want {
            failures.push(format!("{path} ua={ua:?}: got {got}, want {want}"));
        }
    }
    assert!(failures.is_empty(), "{} cases differ:\n{}", failures.len(), failures.join("\n"));
}
