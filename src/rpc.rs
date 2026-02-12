use crate::metrics::MetricsSnapshot;
use crate::storage::MetricsBuffer;
use futures::StreamExt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tarpc::context;
use tarpc::server;
use tarpc::server::Channel;
use tokio::sync::broadcast;
use tokio::time::MissedTickBehavior;
use tokio_serde::formats::Json;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

#[tarpc::service]
pub trait MetricsRpc {
    async fn latest() -> Option<MetricsSnapshot>;
    async fn history(limit: Option<usize>, since_ms: Option<u64>) -> Vec<MetricsSnapshot>;
    async fn next_after(since_ms: u64, timeout_ms: u64) -> Option<MetricsSnapshot>;
}

#[derive(Clone)]
pub struct MetricsRpcServer {
    buffer: Arc<MetricsBuffer>,
    stream_tx: broadcast::Sender<MetricsSnapshot>,
}

impl MetricsRpcServer {
    pub fn new(buffer: Arc<MetricsBuffer>, stream_tx: broadcast::Sender<MetricsSnapshot>) -> Self {
        Self { buffer, stream_tx }
    }
}

impl MetricsRpc for MetricsRpcServer {
    async fn latest(self, _ctx: context::Context) -> Option<MetricsSnapshot> {
        self.buffer.latest()
    }

    async fn history(
        self,
        _ctx: context::Context,
        limit: Option<usize>,
        since_ms: Option<u64>,
    ) -> Vec<MetricsSnapshot> {
        let mut v = self.buffer.history(None);
        if let Some(since_ms) = since_ms {
            v.retain(|s| s.timestamp_ms >= since_ms as u128);
        }
        if let Some(limit) = limit {
            let len = v.len();
            let take = limit.min(len);
            v = v.into_iter().skip(len - take).collect();
        }
        v
    }

    async fn next_after(
        self,
        ctx: context::Context,
        since_ms: u64,
        timeout_ms: u64,
    ) -> Option<MetricsSnapshot> {
        let deadline = ctx.deadline;
        let now = std::time::SystemTime::now();
        let until_deadline = match deadline.duration_since(now) {
            Ok(d) => d,
            Err(_) => Duration::ZERO,
        };
        if until_deadline.is_zero() {
            return None;
        }

        let want_timeout = Duration::from_millis(timeout_ms.max(1));
        let wait = want_timeout.min(until_deadline);
        if wait.is_zero() {
            return None;
        }

        // Fast path: if we already have a newer snapshot in memory, return it immediately.
        if let Some(latest) = self.buffer.latest() {
            if latest.timestamp_ms > since_ms as u128 {
                return Some(latest);
            }
        }

        let mut rx = self.stream_tx.subscribe();
        let fut = async move {
            loop {
                match rx.recv().await {
                    Ok(snap) => {
                        if snap.timestamp_ms > since_ms as u128 {
                            return Some(snap);
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        // Client fell behind; keep waiting for a new snapshot.
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        return None;
                    }
                }
            }
        };

        (tokio::time::timeout(wait, fut).await).unwrap_or_default()
    }
}

pub async fn run_rpc_server(
    buffer: Arc<MetricsBuffer>,
    stream_tx: broadcast::Sender<MetricsSnapshot>,
    addr: SocketAddr,
    cancel: CancellationToken,
) {
    info!("RPC server listening on {}", addr);

    let listener = match tarpc::serde_transport::tcp::listen(addr, Json::default).await {
        Ok(l) => l,
        Err(e) => {
            error!("Failed to bind RPC listener {}: {}", addr, e);
            return;
        }
    };

    let server_impl = MetricsRpcServer::new(buffer, stream_tx);
    let mut incoming = listener;

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                break;
            }
            next = incoming.next() => {
                let Some(next) = next else { break; };
                let transport = match next {
                    Ok(t) => t,
                    Err(e) => {
                        error!("RPC accept error: {}", e);
                        continue;
                    }
                };
                let server_impl = server_impl.clone();
                tokio::spawn(async move {
                    let channel = server::BaseChannel::with_defaults(transport);
                    channel
                        .execute(server_impl.serve())
                        .for_each(|fut| async move {
                            fut.await;
                        })
                        .await;
                });
            }
        }
    }
}

pub async fn run_rpc_client_poller(
    addr: SocketAddr,
    interval: Duration,
    cancel: CancellationToken,
    on_snapshot: impl Fn(MetricsSnapshot) + Send + Sync + 'static,
) {
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let on_snapshot = Arc::new(on_snapshot);
    let mut client: Option<MetricsRpcClient> = None;

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                break;
            }
            _ = ticker.tick() => {}
        }

        if client.is_none() {
            match tarpc::serde_transport::tcp::connect(addr, Json::default).await {
                Ok(transport) => {
                    client = Some(
                        MetricsRpcClient::new(tarpc::client::Config::default(), transport).spawn(),
                    );
                    info!("RPC client connected to {}", addr);
                }
                Err(e) => {
                    error!("RPC connect error to {}: {}", addr, e);
                    continue;
                }
            }
        }

        let Some(c) = &client else {
            continue;
        };
        let ctx = context::current();
        match c.latest(ctx).await {
            Ok(Some(snap)) => {
                (on_snapshot)(snap);
            }
            Ok(None) => {
                warn!("RPC latest returned no data");
            }
            Err(e) => {
                error!("RPC latest error: {}", e);
                client = None;
            }
        }
    }
}

pub async fn run_rpc_client_streamer(
    addr: SocketAddr,
    cancel: CancellationToken,
    on_snapshot: impl Fn(MetricsSnapshot) + Send + Sync + 'static,
) {
    let on_snapshot = Arc::new(on_snapshot);
    let mut client: Option<MetricsRpcClient> = None;
    let mut since_ms: u64 = 0;

    loop {
        if cancel.is_cancelled() {
            break;
        }

        if client.is_none() {
            match tarpc::serde_transport::tcp::connect(addr, Json::default).await {
                Ok(transport) => {
                    client = Some(
                        MetricsRpcClient::new(tarpc::client::Config::default(), transport).spawn(),
                    );
                    info!("RPC client connected to {}", addr);
                }
                Err(e) => {
                    error!("RPC connect error to {}: {}", addr, e);
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    continue;
                }
            }
        }

        let Some(c) = &client else {
            continue;
        };
        let mut ctx = context::current();
        // Ensure the request deadline is longer than our long-poll timeout.
        let long_poll_ms: u64 = 30_000;
        ctx.deadline = std::time::SystemTime::now() + Duration::from_millis(long_poll_ms + 1_000);

        match c.next_after(ctx, since_ms, long_poll_ms).await {
            Ok(Some(snap)) => {
                // Keep moving forward even if remote clock is weird.
                let ts = snap.timestamp_ms;
                since_ms = ts.try_into().unwrap_or(u64::MAX);
                (on_snapshot)(snap);
            }
            Ok(None) => {
                // Timeout/no data; keep the connection and try again.
                continue;
            }
            Err(e) => {
                error!("RPC next_after error: {}", e);
                client = None;
            }
        }
    }
}
