use futures::StreamExt;
use resource_monitor::metrics::{
    CpuMetrics, DiskMetrics, MemoryMetrics, MetricsSnapshot, NetworkMetrics, RpcMetricsSnapshot,
};
use resource_monitor::rpc::{MetricsRpc, MetricsRpcClient, MetricsRpcServer};
use resource_monitor::storage::MetricsBuffer;
use std::sync::Arc;
use std::time::Duration;
use tarpc::context;
use tarpc::server::{self, Channel};
use tokio::sync::broadcast;

fn spawn_rpc_pair(
    buffer: Arc<MetricsBuffer>,
    stream_tx: broadcast::Sender<RpcMetricsSnapshot>,
) -> MetricsRpcClient {
    let server_impl = MetricsRpcServer::new(buffer, stream_tx);
    let (client_transport, server_transport) = tarpc::transport::channel::unbounded();

    tokio::spawn(
        server::BaseChannel::with_defaults(server_transport)
            .execute(server_impl.serve())
            .for_each(|fut| async move {
                tokio::spawn(fut);
            }),
    );

    MetricsRpcClient::new(tarpc::client::Config::default(), client_transport).spawn()
}

#[tokio::test]
async fn rpc_latest_empty_returns_none() {
    let buffer = Arc::new(MetricsBuffer::new(10));
    let (stream_tx, _) = broadcast::channel::<RpcMetricsSnapshot>(8);
    let client = spawn_rpc_pair(buffer, stream_tx);

    let ctx = context::current();
    let res = client.latest(ctx).await.unwrap();
    assert!(res.is_none());
}

#[tokio::test]
async fn rpc_latest_returns_most_recent() {
    let buffer = Arc::new(MetricsBuffer::new(10));
    buffer.push(sample_snapshot(1000));
    buffer.push(sample_snapshot(2000));
    let (stream_tx, _) = broadcast::channel::<RpcMetricsSnapshot>(8);
    let client = spawn_rpc_pair(buffer, stream_tx);

    let ctx = context::current();
    let res = client.latest(ctx).await.unwrap().unwrap();
    assert_eq!(res.timestamp_ms, 2000);
    assert!(!res.data.is_empty());
}

#[tokio::test]
async fn rpc_history_empty_returns_empty() {
    let buffer = Arc::new(MetricsBuffer::new(10));
    let (stream_tx, _) = broadcast::channel::<RpcMetricsSnapshot>(8);
    let client = spawn_rpc_pair(buffer, stream_tx);

    let ctx = context::current();
    let res = client.history(ctx, None, None).await.unwrap();
    assert!(res.is_empty());
}

#[tokio::test]
async fn rpc_history_returns_all() {
    let buffer = Arc::new(MetricsBuffer::new(10));
    buffer.push(sample_snapshot(1000));
    buffer.push(sample_snapshot(2000));
    buffer.push(sample_snapshot(3000));
    let (stream_tx, _) = broadcast::channel::<RpcMetricsSnapshot>(8);
    let client = spawn_rpc_pair(buffer, stream_tx);

    let ctx = context::current();
    let res = client.history(ctx, None, None).await.unwrap();
    assert_eq!(res.len(), 3);
}

#[tokio::test]
async fn rpc_history_with_limit() {
    let buffer = Arc::new(MetricsBuffer::new(10));
    for ts in [1000, 2000, 3000, 4000] {
        buffer.push(sample_snapshot(ts));
    }
    let (stream_tx, _) = broadcast::channel::<RpcMetricsSnapshot>(8);
    let client = spawn_rpc_pair(buffer, stream_tx);

    let ctx = context::current();
    let res = client.history(ctx, Some(2), None).await.unwrap();
    assert_eq!(res.len(), 2);
    assert_eq!(res[0].timestamp_ms, 3000);
    assert_eq!(res[1].timestamp_ms, 4000);
}

#[tokio::test]
async fn rpc_history_with_since() {
    let buffer = Arc::new(MetricsBuffer::new(10));
    for ts in [1000, 2000, 3000, 4000] {
        buffer.push(sample_snapshot(ts));
    }
    let (stream_tx, _) = broadcast::channel::<RpcMetricsSnapshot>(8);
    let client = spawn_rpc_pair(buffer, stream_tx);

    let ctx = context::current();
    let res = client.history(ctx, None, Some(2500)).await.unwrap();
    assert_eq!(res.len(), 2);
    assert_eq!(res[0].timestamp_ms, 3000);
    assert_eq!(res[1].timestamp_ms, 4000);
}

#[tokio::test]
async fn rpc_history_with_since_and_limit() {
    let buffer = Arc::new(MetricsBuffer::new(10));
    for ts in [1000, 2000, 3000, 4000, 5000] {
        buffer.push(sample_snapshot(ts));
    }
    let (stream_tx, _) = broadcast::channel::<RpcMetricsSnapshot>(8);
    let client = spawn_rpc_pair(buffer, stream_tx);

    let ctx = context::current();
    let res = client.history(ctx, Some(2), Some(2000)).await.unwrap();
    assert_eq!(res.len(), 2);
    assert_eq!(res[0].timestamp_ms, 4000);
    assert_eq!(res[1].timestamp_ms, 5000);
}

#[tokio::test]
async fn next_after_returns_next_snapshot() {
    let buffer = Arc::new(MetricsBuffer::new(10));
    let (stream_tx, _) = broadcast::channel::<RpcMetricsSnapshot>(8);
    let stream_tx_clone = stream_tx.clone();
    let client = spawn_rpc_pair(buffer.clone(), stream_tx);

    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let snap = sample_snapshot(2000);
        buffer.push(snap.clone());
        let _ = stream_tx_clone.send(snap.to_rpc_format());
    });

    let mut ctx = context::current();
    ctx.deadline = std::time::SystemTime::now() + Duration::from_secs(2);
    let res = client.next_after(ctx, 0, 1_000).await.unwrap();
    assert!(res.is_some());
    assert_eq!(res.unwrap().timestamp_ms, 2000);
}

#[tokio::test]
async fn next_after_returns_existing_if_newer() {
    let buffer = Arc::new(MetricsBuffer::new(10));
    buffer.push(sample_snapshot(5000));
    let (stream_tx, _) = broadcast::channel::<RpcMetricsSnapshot>(8);
    let client = spawn_rpc_pair(buffer, stream_tx);

    let mut ctx = context::current();
    ctx.deadline = std::time::SystemTime::now() + Duration::from_secs(2);
    let res = client.next_after(ctx, 1000, 500).await.unwrap();
    assert!(res.is_some());
    assert_eq!(res.unwrap().timestamp_ms, 5000);
}

#[tokio::test]
async fn next_after_timeout_returns_none() {
    let buffer = Arc::new(MetricsBuffer::new(10));
    let (stream_tx, _) = broadcast::channel::<RpcMetricsSnapshot>(8);
    let client = spawn_rpc_pair(buffer, stream_tx);

    let mut ctx = context::current();
    ctx.deadline = std::time::SystemTime::now() + Duration::from_secs(2);
    let res = client.next_after(ctx, 0, 100).await.unwrap();
    assert!(res.is_none());
}

fn sample_snapshot(ts: u128) -> MetricsSnapshot {
    MetricsSnapshot {
        timestamp_ms: ts,
        cpu: CpuMetrics {
            total_usage_pct: 10.0,
            per_core_usage_pct: vec![10.0, 20.0],
            load_avg_1: 0.1,
            load_avg_5: 0.2,
            load_avg_15: 0.3,
            temperature_celsius: Some(50.0),
        },
        memory: MemoryMetrics {
            total_bytes: 100,
            used_bytes: 50,
            available_bytes: 50,
            swap_total_bytes: 4096,
            swap_used_bytes: 1024,
        },
        network: NetworkMetrics {
            rx_bytes_total: 1000,
            tx_bytes_total: 2000,
            rx_bytes_per_sec: 10.0,
            tx_bytes_per_sec: 20.0,
        },
        disk: DiskMetrics {
            total_bytes: 500_000_000_000,
            available_bytes: 200_000_000_000,
            used_pct: 60.0,
        },
        battery: None,
        gpu: None,
    }
}
