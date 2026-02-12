use clap::{Parser, ValueEnum};
use resource_monitor::api::{router, AppState};
use resource_monitor::console;
use resource_monitor::runtime;
use resource_monitor::storage::MetricsBuffer;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

#[derive(Clone, Debug, ValueEnum)]
enum Mode {
    Console,
    Web,
    Both,
}

#[derive(Debug, Parser)]
#[command(
    name = "resource_monitor-client",
    about = "Resource Monitor client (web/console UI)"
)]
struct Args {
    /// Output mode (console/web/both)
    #[arg(long, value_enum, default_value_t = Mode::Web)]
    mode: Mode,

    /// History depth (number of snapshots kept in memory)
    #[arg(long, default_value_t = 3600)]
    history: usize,

    /// RPC server address
    #[arg(long, default_value = "127.0.0.1:50051")]
    rpc_addr: SocketAddr,

    /// Bind address for HTTP server
    #[arg(long, default_value = "127.0.0.1")]
    bind: IpAddr,

    /// HTTP server port
    #[arg(long, default_value_t = 8080)]
    port: u16,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    runtime::init_tracing();
    let args = Args::parse();
    info!(
        "Starting client: mode={:?}, history={}, rpc_addr={}, bind={}, port={}",
        args.mode, args.history, args.rpc_addr, args.bind, args.port
    );

    let buffer = Arc::new(MetricsBuffer::new(args.history));
    let cancel = CancellationToken::new();

    let (stream_tx, _stream_rx) = tokio::sync::broadcast::channel(256);

    // Keep storage+stream subscriber alive. RPC snapshots are published into nuts.
    let _storage_activity = resource_monitor::bus::register_storage_and_stream_subscriber(
        buffer.clone(),
        stream_tx.clone(),
    );

    let rpc_cancel = cancel.clone();
    let rpc_addr = args.rpc_addr;
    let rpc_handle = tokio::spawn(async move {
        resource_monitor::rpc::run_rpc_client_streamer(rpc_addr, rpc_cancel, |snap| {
            resource_monitor::bus::publish_snapshot(snap)
        })
        .await;
    });

    let console_handle = match args.mode {
        Mode::Console | Mode::Both => {
            let console_cancel = cancel.clone();
            let console_buffer = buffer.clone();
            Some(tokio::spawn(async move {
                console::run_console(
                    console_buffer,
                    std::time::Duration::from_millis(1000),
                    console_cancel,
                )
                .await;
            }))
        }
        Mode::Web => None,
    };

    let web_handle = match args.mode {
        Mode::Web | Mode::Both => {
            let state = AppState {
                buffer: buffer.clone(),
                stream_tx: stream_tx.clone(),
            };
            let app = router(state);
            let addr = SocketAddr::from((args.bind, args.port));
            let listener = match tokio::net::TcpListener::bind(addr).await {
                Ok(l) => l,
                Err(e) => {
                    error!("Failed to bind {}: {}", addr, e);
                    cancel.cancel();
                    return;
                }
            };
            info!(
                "HTTP server listening on http://{}",
                listener.local_addr().unwrap_or(addr)
            );
            let shutdown = cancel.clone();
            Some(tokio::spawn(async move {
                let res = axum::serve(listener, app)
                    .with_graceful_shutdown(async move { shutdown.cancelled().await })
                    .await;
                if let Err(e) = res {
                    error!("Server error: {}", e);
                }
            }))
        }
        Mode::Console => None,
    };

    runtime::shutdown_signal().await;
    cancel.cancel();

    if let Some(h) = web_handle {
        let _ = h.await;
    }
    if let Some(h) = console_handle {
        let _ = h.await;
    }
    let _ = rpc_handle.await;
}
