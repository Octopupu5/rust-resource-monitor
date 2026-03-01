use clap::Parser;
use resource_monitor::console;
use resource_monitor::metrics::RpcMetricsSnapshot;
use resource_monitor::runtime;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::info;

#[derive(Debug, Parser)]
#[command(
    name = "resource_monitor-client",
    about = "Resource Monitor console client (connects to server via RPC)"
)]
struct Args {
    /// RPC server address to connect to
    #[arg(long, default_value = "127.0.0.1:50051")]
    rpc_addr: SocketAddr,

    /// Console refresh interval in milliseconds
    #[arg(long, default_value_t = 1000)]
    interval_ms: u64,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    runtime::init_tracing();
    let args = Args::parse();
    info!(
        "Starting console client: rpc_addr={}, interval={}ms",
        args.rpc_addr, args.interval_ms
    );

    let latest: Arc<RwLock<Option<RpcMetricsSnapshot>>> = Arc::new(RwLock::new(None));
    let cancel = CancellationToken::new();

    let rpc_cancel = cancel.clone();
    let rpc_addr = args.rpc_addr;
    let rpc_latest = latest.clone();
    let rpc_handle = tokio::spawn(async move {
        resource_monitor::rpc::run_rpc_client_streamer(rpc_addr, rpc_cancel, move |snap| {
            let mut guard = rpc_latest.write().unwrap_or_else(|p| p.into_inner());
            *guard = Some(snap);
        })
        .await;
    });

    let console_cancel = cancel.clone();
    let console_latest = latest.clone();
    let interval = Duration::from_millis(args.interval_ms);
    let console_handle = tokio::spawn(async move {
        console::run_rpc_console(console_latest, interval, console_cancel).await;
    });

    runtime::shutdown_signal().await;
    info!("Shutdown signal received, stopping client...");
    cancel.cancel();

    let shutdown_timeout = Duration::from_secs(2);

    if tokio::time::timeout(shutdown_timeout, console_handle)
        .await
        .is_err()
    {
        info!("Console shutdown timeout");
    }

    let mut rpc_handle = rpc_handle;
    if tokio::time::timeout(shutdown_timeout, &mut rpc_handle)
        .await
        .is_err()
    {
        rpc_handle.abort();
        let _ = rpc_handle.await;
    }

    info!("Client stopped");
}
