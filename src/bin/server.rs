use clap::Parser;
use resource_monitor::aggregator::{Aggregator, AggregatorConfig};
use resource_monitor::console;
use resource_monitor::metrics::{MetricsSnapshot, RpcMetricsSnapshot};
use resource_monitor::runtime;
use resource_monitor::storage::MetricsBuffer;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::info;

#[derive(Debug, Parser)]
#[command(
    name = "resource_monitor-server",
    about = "Resource Monitor RPC server (collector)"
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

    /// Also show console output
    #[arg(long, default_value_t = false)]
    console: bool,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    runtime::init_tracing();
    let args = Args::parse();
    info!(
        "Starting server: interval={}ms, history={}, rpc_addr={}, console={}",
        args.interval_ms, args.history, args.rpc_addr, args.console
    );

    let buffer = Arc::new(MetricsBuffer::new(args.history));
    let cancel = CancellationToken::new();

    // Создаем канал для RPC (RpcMetricsSnapshot)
    let (rpc_stream_tx, _) = tokio::sync::broadcast::channel::<RpcMetricsSnapshot>(256);

    // Создаем канал для внутреннего использования (MetricsSnapshot)
    let (internal_stream_tx, mut internal_stream_rx) =
        tokio::sync::broadcast::channel::<MetricsSnapshot>(256);

    // Подписываемся на события и сохраняем в буфер, а также отправляем во внутренний канал
    let _storage_activity = resource_monitor::bus::register_storage_subscriber_with_channel(
        buffer.clone(),
        internal_stream_tx.clone(),
    );

    let agg = Aggregator::new(AggregatorConfig::new(std::time::Duration::from_millis(
        args.interval_ms,
    )));
    let agg_cancel = cancel.clone();
    let agg_handle = tokio::spawn(async move { agg.run(agg_cancel).await });

    let rpc_cancel = cancel.clone();
    let rpc_buffer = buffer.clone();
    let rpc_addr = args.rpc_addr;

    // Клонируем для RPC сервера
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

    // Конвертер из внутреннего формата в RPC формат
    let rpc_stream_tx_for_converter = rpc_stream_tx.clone();
    let converter_handle = tokio::spawn(async move {
        while let Ok(snapshot) = internal_stream_rx.recv().await {
            let rpc_snapshot = snapshot.to_rpc_format();
            if let Err(e) = rpc_stream_tx_for_converter.send(rpc_snapshot) {
                tracing::warn!("Failed to send to RPC channel: {}", e);
            }
        }
        info!("Converter stopped");
    });

    let console_handle = if args.console {
        let console_cancel = cancel.clone();
        let console_buffer = buffer.clone();
        let interval = std::time::Duration::from_millis(args.interval_ms);
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

    let rpc_result = tokio::time::timeout(shutdown_timeout, rpc_handle).await;
    let converter_result = tokio::time::timeout(shutdown_timeout, converter_handle).await;
    let agg_result = tokio::time::timeout(shutdown_timeout, agg_handle).await;

    if let Some(handle) = console_handle {
        let console_result = tokio::time::timeout(shutdown_timeout, handle).await;
        if console_result.is_err() {
            info!("Console shutdown timeout");
        }
    }

    if rpc_result.is_err() {
        info!("RPC server shutdown timeout");
    }
    if converter_result.is_err() {
        info!("Converter shutdown timeout");
    }
    if agg_result.is_err() {
        info!("Aggregator shutdown timeout");
    }
    
    info!("Server stopped");
}