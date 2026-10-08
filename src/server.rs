//! hyper glue reproducing what the Node entry did around `app.fetch()`.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::net::TcpListener;

use crate::app::App;
use crate::hono::Request;

/// Node keeps only the first value of these when a client repeats them.
const SINGLE_VALUE_HEADERS: &[&str] = &[
    "age",
    "authorization",
    "content-length",
    "content-type",
    "etag",
    "expires",
    "from",
    "host",
    "if-modified-since",
    "if-unmodified-since",
    "last-modified",
    "location",
    "max-forwards",
    "proxy-authorization",
    "referer",
    "retry-after",
    "server",
    "user-agent",
];

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

fn header_pairs(map: &hyper::HeaderMap) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for name in map.keys() {
        let key = name.as_str();
        let mut values = map.get_all(name).iter().map(|v| latin1(v.as_bytes()));
        if SINGLE_VALUE_HEADERS.contains(&key) {
            out.extend(values.next().map(|v| (key.to_string(), v)));
        } else if key == "cookie" {
            out.push((key.to_string(), values.collect::<Vec<_>>().join("; ")));
        } else {
            out.extend(values.map(|v| (key.to_string(), v)));
        }
    }
    out
}

fn first_header(headers: &[(String, String)], name: &str) -> Option<String> {
    let values: Vec<&str> = headers.iter().filter(|(k, _)| k == name).map(|(_, v)| v.as_str()).collect();
    (!values.is_empty()).then(|| values.join(", ")).filter(|v| !v.is_empty())
}

async fn to_request(req: hyper::Request<Incoming>) -> Result<Request, String> {
    let headers = header_pairs(req.headers());
    let protocol = first_header(&headers, "x-forwarded-proto").unwrap_or_else(|| "http".into());
    let host = first_header(&headers, "host").unwrap_or_else(|| "localhost".into());
    let url = format!("{protocol}://{host}{}", req.uri());
    let method = req.method().as_str().to_string();
    let mut request = Request::new(&method, &url)?;
    request.headers = headers;
    if method != "GET" && method != "HEAD" {
        request.body = req.into_body().collect().await.map_err(|e| e.to_string())?.to_bytes().to_vec();
    }
    Ok(request)
}

fn plain_error() -> hyper::Response<Full<Bytes>> {
    let mut res = hyper::Response::new(Full::new(Bytes::from_static(b"Internal Server Error")));
    *res.status_mut() = hyper::StatusCode::INTERNAL_SERVER_ERROR;
    res
}

fn to_hyper(res: crate::hono::Response) -> Result<hyper::Response<Full<Bytes>>, String> {
    let mut builder = hyper::Response::builder().status(res.status);
    for (name, value) in &res.headers {
        let bytes: Vec<u8> = value
            .chars()
            .map(|c| u8::try_from(c as u32).map_err(|_| format!("Invalid character in header {name}")))
            .collect::<Result<_, _>>()?;
        builder =
            builder.header(name.as_str(), hyper::header::HeaderValue::from_bytes(&bytes).map_err(|e| e.to_string())?);
    }
    builder.body(Full::new(Bytes::from(res.body))).map_err(|e| e.to_string())
}

async fn respond(app: Arc<App>, req: hyper::Request<Incoming>) -> Result<hyper::Response<Full<Bytes>>, Infallible> {
    let converted = match to_request(req).await {
        Ok(request) => to_hyper(app.handle(&request).await),
        Err(e) => Err(e),
    };
    Ok(converted.unwrap_or_else(|e| {
        eprintln!("Node server error {e}");
        plain_error()
    }))
}

/// Accepts connections until `shutdown` resolves.
pub async fn serve(app: Arc<App>, listener: TcpListener, shutdown: impl std::future::Future<Output = ()>) {
    tokio::pin!(shutdown);
    loop {
        let stream = tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => stream,
                Err(e) => {
                    eprintln!("accept failed: {e}");
                    continue;
                }
            },
            () = &mut shutdown => return,
        };
        let app = app.clone();
        tokio::spawn(async move {
            let service = service_fn(move |req| respond(app.clone(), req));
            let mut http = http1::Builder::new();
            // Node's headersTimeout default.
            http.timer(TokioTimer::new()).header_read_timeout(Duration::from_secs(60));
            let _ = http.serve_connection(TokioIo::new(stream), service).await;
        });
    }
}
