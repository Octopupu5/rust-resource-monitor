use clap::Parser;
use resource_monitor::aggregator::{Aggregator, AggregatorConfig};
use resource_monitor::api::{api_only_router, AppState};
use resource_monitor::console;
use resource_monitor::db::MetricsDb;
use resource_monitor::metrics::{MetricsSnapshot, RpcMetricsSnapshot};
use resource_monitor::runtime;
use resource_monitor::storage::MetricsBuffer;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

#[derive(Debug, Parser)]
#[command(
    name = "resource_monitor-server",
    about = "Resource Monitor server (collector + HTTP API + RPC)"
)]
struct Args {
    /// Polling interval in milliseconds
    #[arg(long, default_value_t = 1000)]
    interval_ms: u64,

    /// History depth (number of snapshots kept in memory)
    #[arg(long, default_value_t = 3600)]
    history: usize,

    /// RPC bind address
    #[arg(long, default_value = "127.0.0.1:50051")]
    rpc_addr: SocketAddr,

    /// HTTP bind address
    #[arg(long, default_value = "127.0.0.1")]
    bind: IpAddr,

    /// HTTP API server port
    #[arg(long, default_value_t = 9000)]
    port: u16,

    /// Disable HTTP API server
    #[arg(long, default_value_t = false)]
    no_http: bool,

    /// Also show console output
    #[arg(long, default_value_t = false)]
    console: bool,

    /// Path to SQLite database file
    #[arg(long, default_value = "metrics.db")]
    db_path: PathBuf,

    /// Automatically cleanup old records after N hours (0 to disable)
    #[arg(long, default_value_t = 168)] // 7 days
    db_cleanup_hours: u64,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    runtime::init_tracing();
    let args = Args::parse();
    info!(
        "Starting server: interval={}ms, history={}, rpc={}, http={}:{}, http_enabled={}, console={}, db={}",
        args.interval_ms,
        args.history,
        args.rpc_addr,
        args.bind,
        args.port,
        !args.no_http,
        args.console,
        args.db_path.display()
    );

    // Инициализируем базу данных
    let db = match MetricsDb::new(&args.db_path) {
        Ok(db) => Arc::new(db),
        Err(e) => {
            error!("Failed to initialize database: {}", e);
            return;
        }
    };

    // Запускаем cleanup если нужно
    if args.db_cleanup_hours > 0 {
        let db_clone = db.clone();
        let cleanup_interval = Duration::from_secs(3600);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(cleanup_interval);
            loop {
                interval.tick().await;
                if let Err(e) = db_clone.cleanup_old(args.db_cleanup_hours) {
                    error!("Database cleanup failed: {}", e);
                }
            }
        });
    }

    let buffer = Arc::new(MetricsBuffer::new(args.history));
    let cancel = CancellationToken::new();

    let (rpc_stream_tx, _) = tokio::sync::broadcast::channel::<RpcMetricsSnapshot>(256);
    let (internal_stream_tx, _) = tokio::sync::broadcast::channel::<MetricsSnapshot>(256);

    let _storage_activity = resource_monitor::bus::register_storage_subscriber_with_channel(
        buffer.clone(),
        internal_stream_tx.clone(),
    );

    let agg = Aggregator::new(AggregatorConfig::new(Duration::from_millis(
        args.interval_ms,
    )));
    let agg_cancel = cancel.clone();
    let agg_handle = tokio::spawn(async move { agg.run(agg_cancel).await });

    let db_rx = internal_stream_tx.subscribe();
    let db_writer_handle = {
        let db = db.clone();
        tokio::spawn(async move {
            let mut rx = db_rx;
            while let Ok(snapshot) = rx.recv().await {
                if let Err(e) = db.insert(&snapshot) {
                    error!("Failed to insert snapshot into database: {}", e);
                }
            }
            info!("Database writer stopped");
        })
    };

    let converter_rx = internal_stream_tx.subscribe();
    let rpc_stream_tx_for_converter = rpc_stream_tx.clone();
    let converter_handle = tokio::spawn(async move {
        let mut rx = converter_rx;
        while let Ok(snapshot) = rx.recv().await {
            let rpc_snapshot = snapshot.to_rpc_format();
            if let Err(e) = rpc_stream_tx_for_converter.send(rpc_snapshot) {
                tracing::warn!("Failed to send to broadcast channel: {}", e);
            }
        }
        info!("Converter stopped");
    });

    let rpc_cancel = cancel.clone();
    let rpc_buffer = buffer.clone();
    let rpc_addr = args.rpc_addr;
    let rpc_stream_tx_for_server = rpc_stream_tx.clone();
    let rpc_handle = tokio::spawn(async move {
        resource_monitor::rpc::run_rpc_server(
            rpc_buffer,
            rpc_stream_tx_for_server,
            rpc_addr,
            rpc_cancel,
        )
        .await;
    });

    let web_handle = if !args.no_http {
        let state = AppState {
            buffer: buffer.clone(),
            db: db.clone(),
            stream_tx: rpc_stream_tx.clone(),
            shutdown: cancel.clone(),
        };
        let app = api_only_router(state);
        let addr = SocketAddr::from((args.bind, args.port));
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                error!("Failed to bind HTTP {}: {}", addr, e);
                cancel.cancel();
                return;
            }
        };
        info!(
            "HTTP API listening on http://{}",
            listener.local_addr().unwrap_or(addr)
        );
        let shutdown = cancel.clone();
        Some(tokio::spawn(async move {
            let res = axum::serve(listener, app)
                .with_graceful_shutdown(async move { shutdown.cancelled().await })
                .await;
            if let Err(e) = res {
                error!("HTTP server error: {}", e);
            }
        }))
    } else {
        None
    };

    let console_handle = if args.console {
        let console_cancel = cancel.clone();
        let console_buffer = buffer.clone();
        let interval = Duration::from_millis(args.interval_ms);
        Some(tokio::spawn(async move {
            console::run_console(console_buffer, interval, console_cancel).await;
            info!("Console stopped");
        }))
    } else {
        None
    };

    runtime::shutdown_signal().await;
    info!("Shutdown signal received, stopping server...");
    cancel.cancel();

    let shutdown_timeout = Duration::from_secs(3);

    if let Some(h) = web_handle {
        if tokio::time::timeout(shutdown_timeout, h).await.is_err() {
            info!("HTTP API shutdown timeout");
        }
    }
    if tokio::time::timeout(shutdown_timeout, rpc_handle)
        .await
        .is_err()
    {
        info!("RPC server shutdown timeout");
    }
    if tokio::time::timeout(shutdown_timeout, converter_handle)
        .await
        .is_err()
    {
        info!("Converter shutdown timeout");
    }
    if tokio::time::timeout(shutdown_timeout, agg_handle)
        .await
        .is_err()
    {
        info!("Aggregator shutdown timeout");
    }
    if let Some(h) = console_handle {
        if tokio::time::timeout(shutdown_timeout, h).await.is_err() {
            info!("Console shutdown timeout");
        }
    }

    if tokio::time::timeout(Duration::from_secs(2), db_writer_handle)
        .await
        .is_err()
    {
        info!("Database writer shutdown timeout");
    }

    info!("Server stopped");
}
