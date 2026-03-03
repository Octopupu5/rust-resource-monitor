use axum::body::Body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use clap::Parser;
use futures::StreamExt;
use resource_monitor::console;
use resource_monitor::metrics::RpcMetricsSnapshot;
use resource_monitor::runtime;
use resource_monitor::web;
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

#[derive(Debug, Parser)]
#[command(
    name = "resource_monitor-client",
    about = "Resource Monitor web client (serves UI + proxies API to server)"
)]
struct Args {
    /// Server HTTP API URL (backend)
    #[arg(long, default_value = "http://127.0.0.1:9000")]
    api_url: String,

    /// RPC server address (for console mode)
    #[arg(long, default_value = "127.0.0.1:50051")]
    rpc_addr: SocketAddr,

    /// HTTP bind address for this client
    #[arg(long, default_value = "127.0.0.1")]
    bind: IpAddr,

    /// HTTP port for this client
    #[arg(long, default_value_t = 8080)]
    port: u16,

    /// Also show console output (via RPC)
    #[arg(long, default_value_t = false)]
    console: bool,
}

#[derive(Clone)]
struct ProxyState {
    api_url: String,
    http: reqwest::Client,
    rpc_latest: Arc<RwLock<Option<RpcMetricsSnapshot>>>,
    rpc_stream_tx: broadcast::Sender<RpcMetricsSnapshot>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    runtime::init_tracing();
    let args = Args::parse();
    info!(
        "Starting client: api_url={}, bind={}:{}, console={}",
        args.api_url, args.bind, args.port, args.console
    );

    let cancel = CancellationToken::new();
    let rpc_latest: Arc<RwLock<Option<RpcMetricsSnapshot>>> = Arc::new(RwLock::new(None));
    let (rpc_stream_tx, _) = broadcast::channel::<RpcMetricsSnapshot>(256);

    let rpc_cancel = cancel.clone();
    let rpc_addr = args.rpc_addr;
    let rpc_latest_for_stream = rpc_latest.clone();
    let rpc_stream_tx_for_stream = rpc_stream_tx.clone();
    let rpc_handle = tokio::spawn(async move {
        resource_monitor::rpc::run_rpc_client_streamer(rpc_addr, rpc_cancel, move |snap| {
            let mut guard = rpc_latest_for_stream
                .write()
                .unwrap_or_else(|p| p.into_inner());
            *guard = Some(snap.clone());
            if let Err(e) = rpc_stream_tx_for_stream.send(snap) {
                warn!("Failed to forward RPC snapshot to web stream: {}", e);
            }
        })
        .await;
    });

    let proxy_state = ProxyState {
        api_url: args.api_url.trim_end_matches('/').to_string(),
        http: reqwest::Client::new(),
        rpc_latest: rpc_latest.clone(),
        rpc_stream_tx: rpc_stream_tx.clone(),
    };

    let app = Router::new()
        .route("/", get(index))
        .route("/api/health", get(proxy_health))
        .route("/api/latest", get(proxy_latest))
        .route("/api/metrics", get(proxy_latest))
        .route("/api/range", get(proxy_range))
        .route("/api/history", get(proxy_history))
        .route("/api/stream", get(proxy_stream))
        .with_state(proxy_state);

    let addr = SocketAddr::from((args.bind, args.port));
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            error!("Failed to bind {}: {}", addr, e);
            return;
        }
    };
    info!(
        "Web UI available at http://{}",
        listener.local_addr().unwrap_or(addr)
    );

    let web_shutdown = cancel.clone();
    let web_handle = tokio::spawn(async move {
        let res = axum::serve(listener, app)
            .with_graceful_shutdown(async move { web_shutdown.cancelled().await })
            .await;
        if let Err(e) = res {
            error!("Client HTTP error: {}", e);
        }
    });

    // Console via RPC (optional)
    let console_handle = if args.console {
        let console_cancel = cancel.clone();
        let latest = rpc_latest.clone();
        Some(tokio::spawn(async move {
            console::run_rpc_console(latest, Duration::from_millis(1000), console_cancel).await;
        }))
    } else {
        None
    };

    runtime::shutdown_signal().await;
    info!("Shutdown signal received, stopping client...");
    cancel.cancel();

    let shutdown_timeout = Duration::from_secs(2);

    if tokio::time::timeout(shutdown_timeout, web_handle)
        .await
        .is_err()
    {
        info!("Web shutdown timeout");
    }
    if let Some(h) = console_handle {
        if tokio::time::timeout(shutdown_timeout, h).await.is_err() {
            info!("Console shutdown timeout");
        }
    }
    if tokio::time::timeout(shutdown_timeout, rpc_handle)
        .await
        .is_err()
    {
        info!("RPC stream shutdown timeout");
    }

    info!("Client stopped");
}

async fn index() -> impl IntoResponse {
    web::index().await
}

async fn proxy_health(State(st): State<ProxyState>) -> Response {
    proxy_get(&st, "/api/health", "").await
}

async fn proxy_latest(
    State(st): State<ProxyState>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
) -> Response {
    let latest = st
        .rpc_latest
        .read()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    if let Some(snapshot) = latest {
        return (StatusCode::OK, axum::Json(snapshot)).into_response();
    }

    let qs = query.map(|q| format!("?{}", q)).unwrap_or_default();
    proxy_get(&st, "/api/latest", &qs).await
}

async fn proxy_range(
    State(st): State<ProxyState>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
) -> Response {
    let qs = query.map(|q| format!("?{}", q)).unwrap_or_default();
    proxy_get(&st, "/api/range", &qs).await
}

async fn proxy_history(
    State(st): State<ProxyState>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
) -> Response {
    let qs = query.map(|q| format!("?{}", q)).unwrap_or_default();
    proxy_get(&st, "/api/history", &qs).await
}

async fn proxy_get(st: &ProxyState, path: &str, query: &str) -> Response {
    let url = format!("{}{}{}", st.api_url, path, query);
    match st.http.get(&url).send().await {
        Ok(resp) => {
            let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::OK);
            let content_type = resp
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("application/json")
                .to_string();
            match resp.bytes().await {
                Ok(body) => Response::builder()
                    .status(status)
                    .header("content-type", content_type)
                    .body(Body::from(body))
                    .unwrap_or_else(|_| {
                        (StatusCode::INTERNAL_SERVER_ERROR, "response build error").into_response()
                    }),
                Err(e) => (StatusCode::BAD_GATEWAY, format!("read error: {e}")).into_response(),
            }
        }
        Err(e) => (StatusCode::BAD_GATEWAY, format!("proxy error: {e}")).into_response(),
    }
}

async fn proxy_stream(
    State(st): State<ProxyState>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let rx = st.rpc_stream_tx.subscribe();
    let stream = BroadcastStream::new(rx).map(|msg| match msg {
        Ok(snapshot) => match serde_json::to_string(&snapshot) {
            Ok(json) => Ok(Event::default().data(json)),
            Err(e) => Ok(Event::default()
                .event("error")
                .data(format!("serialize_error: {e}"))),
        },
        Err(e) => Ok(Event::default()
            .event("error")
            .data(format!("stream_error: {e}"))),
    });

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(10))
            .text("keep-alive"),
    )
}
