use std::sync::Arc;
use std::time::Duration;

use sublink::app::App;
use sublink::fetch::HttpFetcher;
use sublink::server;
use sublink::settings::Settings;
use sublink::storage::Store;

const SWEEP_INTERVAL: Duration = Duration::from_secs(600);

fn or_exit<T>(result: Result<T, String>, what: &str) -> T {
    result.unwrap_or_else(|e| {
        eprintln!("Failed to {what}: {e}");
        std::process::exit(1);
    })
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    // PID 1 in a container ignores SIGTERM unless it is handled explicitly.
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = term => {},
    }
}

async fn bind(port: u16) -> std::io::Result<tokio::net::TcpListener> {
    // Like Node's listen(port): dual-stack when IPv6 exists, IPv4 otherwise.
    match tokio::net::TcpListener::bind(("::", port)).await {
        Ok(listener) => Ok(listener),
        Err(_) => tokio::net::TcpListener::bind(("0.0.0.0", port)).await,
    }
}

#[tokio::main]
async fn main() {
    let settings = or_exit(Settings::from_env(), "read settings");
    let store = if settings.db_path == ":memory:" {
        Store::in_memory()
    } else {
        or_exit(Store::open(&settings.db_path), "open DB_PATH")
    };
    let fetcher = or_exit(HttpFetcher::new(), "create HTTP client");

    let mut app = App::new(store.clone(), Arc::new(fetcher));
    app.config_ttl_seconds = settings.config_ttl_seconds;
    app.short_link_ttl_seconds = settings.short_link_ttl_seconds;

    tokio::spawn(async move {
        let mut tick = tokio::time::interval(SWEEP_INTERVAL);
        loop {
            tick.tick().await;
            let store = store.clone();
            match tokio::task::spawn_blocking(move || store.sweep()).await {
                Ok(Err(e)) => eprintln!("Expired key sweep failed: {e}"),
                Err(e) => eprintln!("Expired key sweep panicked: {e}"),
                Ok(Ok(_)) => {}
            }
        }
    });

    let listener = or_exit(bind(settings.port).await.map_err(|e| e.to_string()), "bind port");
    println!("Sublink worker running on http://0.0.0.0:{}", settings.port);
    server::serve(Arc::new(app), listener, shutdown_signal()).await;
}
