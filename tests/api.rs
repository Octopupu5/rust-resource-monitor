use resource_monitor::api::{router, AppState};
use resource_monitor::metrics::{CpuMetrics, MemoryMetrics, MetricsSnapshot, NetworkMetrics};
use resource_monitor::storage::MetricsBuffer;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tower::util::ServiceExt;

#[tokio::test]
async fn history_initially_empty() {
    let buffer = Arc::new(MetricsBuffer::new(10));
    let (stream_tx, _stream_rx) = tokio::sync::broadcast::channel(8);
    let app = router(AppState {
        buffer,
        stream_tx,
        shutdown: CancellationToken::new(),
    });
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/history")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json.is_array());
    assert_eq!(json.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn health_ok() {
    let buffer = Arc::new(MetricsBuffer::new(10));
    let (stream_tx, _stream_rx) = tokio::sync::broadcast::channel(8);
    let app = router(AppState {
        buffer,
        stream_tx,
        shutdown: CancellationToken::new(),
    });
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/health")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn history_filters_by_since_ms() {
    let buffer = Arc::new(MetricsBuffer::new(10));
    buffer.push(sample_snapshot(1000));
    buffer.push(sample_snapshot(2000));

    let (stream_tx, _stream_rx) = tokio::sync::broadcast::channel(8);
    let app = router(AppState {
        buffer,
        stream_tx,
        shutdown: CancellationToken::new(),
    });
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/history?since_ms=1500")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let arr = json.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["timestamp_ms"].as_u64().unwrap(), 2000);
}

#[tokio::test]
async fn stream_is_event_stream() {
    let buffer = Arc::new(MetricsBuffer::new(10));
    let (stream_tx, _stream_rx) = tokio::sync::broadcast::channel(8);
    let app = router(AppState {
        buffer,
        stream_tx,
        shutdown: CancellationToken::new(),
    });
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/stream")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let ct = response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(ct.starts_with("text/event-stream"));
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
