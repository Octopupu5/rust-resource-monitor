use futures::StreamExt;
use resource_monitor::metrics::{CpuMetrics, MemoryMetrics, MetricsSnapshot, NetworkMetrics};
use resource_monitor::rpc::{MetricsRpc, MetricsRpcClient, MetricsRpcServer};
use resource_monitor::storage::MetricsBuffer;
use std::sync::Arc;
use std::time::Duration;
use tarpc::context;
use tarpc::server::{self, Channel};
use tokio::sync::broadcast;

#[tokio::test]
async fn next_after_returns_next_snapshot() {
    let buffer = Arc::new(MetricsBuffer::new(10));
    let (stream_tx, _stream_rx) = broadcast::channel(8);

    let server_impl = MetricsRpcServer::new(buffer.clone(), stream_tx.clone());
    let (client_transport, server_transport) = tarpc::transport::channel::unbounded();

    tokio::spawn(
        server::BaseChannel::with_defaults(server_transport)
            .execute(server_impl.serve())
            .for_each(|fut| async move {
                tokio::spawn(fut);
            }),
    );

    let client = MetricsRpcClient::new(tarpc::client::Config::default(), client_transport).spawn();

    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let snap = sample_snapshot(2000);
        buffer.push(snap.clone());
        let _ = stream_tx.send(snap);
    });

    let mut ctx = context::current();
    ctx.deadline = std::time::SystemTime::now() + Duration::from_secs(2);
    let res = client.next_after(ctx, 0, 1_000).await.unwrap();
    assert!(res.is_some());
    assert_eq!(res.unwrap().timestamp_ms, 2000);
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
        },
        memory: MemoryMetrics {
            total_bytes: 100,
            used_bytes: 50,
            available_bytes: 50,
        },
        network: NetworkMetrics {
            rx_bytes_total: 1000,
            tx_bytes_total: 2000,
            rx_bytes_per_sec: 10.0,
            tx_bytes_per_sec: 20.0,
        },
    }
}
