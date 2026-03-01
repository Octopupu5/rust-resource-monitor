use resource_monitor::api::{router, AppState};
use resource_monitor::db::MetricsDb;
use resource_monitor::metrics::{
    CpuMetrics, DiskMetrics, MemoryMetrics, MetricsSnapshot, NetworkMetrics,
};
use resource_monitor::storage::MetricsBuffer;
use std::sync::Arc;
use tempfile::tempdir;
use tokio_util::sync::CancellationToken;
use tower::util::ServiceExt;

#[tokio::test]
async fn history_initially_empty() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let db = Arc::new(MetricsDb::new(&db_path).unwrap());

    let buffer = Arc::new(MetricsBuffer::new(10));
    let (stream_tx, _stream_rx) = tokio::sync::broadcast::channel(8);
    let app = router(AppState {
        buffer,
        db,
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
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let db = Arc::new(MetricsDb::new(&db_path).unwrap());

    let buffer = Arc::new(MetricsBuffer::new(10));
    let (stream_tx, _stream_rx) = tokio::sync::broadcast::channel(8);
    let app = router(AppState {
        buffer,
        db,
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
async fn range_filters_by_timestamps() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let db = Arc::new(MetricsDb::new(&db_path).unwrap());

    db.insert(&sample_snapshot(1000)).unwrap();
    db.insert(&sample_snapshot(2000)).unwrap();
    db.insert(&sample_snapshot(3000)).unwrap();

    let buffer = Arc::new(MetricsBuffer::new(10));
    let (stream_tx, _stream_rx) = tokio::sync::broadcast::channel(8);
    let app = router(AppState {
        buffer,
        db,
        stream_tx,
        shutdown: CancellationToken::new(),
    });

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/range?from_ts=1500&to_ts=2500")
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
async fn latest_returns_last_snapshot() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let db = Arc::new(MetricsDb::new(&db_path).unwrap());

    db.insert(&sample_snapshot(1000)).unwrap();
    db.insert(&sample_snapshot(2000)).unwrap();

    let buffer = Arc::new(MetricsBuffer::new(10));
    let (stream_tx, _stream_rx) = tokio::sync::broadcast::channel(8);
    let app = router(AppState {
        buffer,
        db,
        stream_tx,
        shutdown: CancellationToken::new(),
    });

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/latest")
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
    assert_eq!(json["timestamp_ms"].as_u64().unwrap(), 2000);
}

#[tokio::test]
async fn stream_is_event_stream() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let db = Arc::new(MetricsDb::new(&db_path).unwrap());

    let buffer = Arc::new(MetricsBuffer::new(10));
    let (stream_tx, _stream_rx) = tokio::sync::broadcast::channel(8);
    let app = router(AppState {
        buffer,
        db,
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

#[tokio::test]
async fn range_with_limit() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let db = Arc::new(MetricsDb::new(&db_path).unwrap());

    db.insert(&sample_snapshot(1000)).unwrap();
    db.insert(&sample_snapshot(2000)).unwrap();
    db.insert(&sample_snapshot(3000)).unwrap();
    db.insert(&sample_snapshot(4000)).unwrap();

    let buffer = Arc::new(MetricsBuffer::new(10));
    let (stream_tx, _stream_rx) = tokio::sync::broadcast::channel(8);
    let app = router(AppState {
        buffer,
        db,
        stream_tx,
        shutdown: CancellationToken::new(),
    });

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/range?from_ts=1000&to_ts=4000&limit=2")
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
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["timestamp_ms"].as_u64().unwrap(), 4000);
    assert_eq!(arr[1]["timestamp_ms"].as_u64().unwrap(), 3000);
}

#[tokio::test]
async fn history_with_since() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let db = Arc::new(MetricsDb::new(&db_path).unwrap());

    db.insert(&sample_snapshot(1000)).unwrap();
    db.insert(&sample_snapshot(2000)).unwrap();
    db.insert(&sample_snapshot(3000)).unwrap();

    let buffer = Arc::new(MetricsBuffer::new(10));
    let (stream_tx, _stream_rx) = tokio::sync::broadcast::channel(8);
    let app = router(AppState {
        buffer,
        db,
        stream_tx,
        shutdown: CancellationToken::new(),
    });

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/history?since_ts=2000&limit=2")
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
    assert_eq!(arr.len(), 2);

    assert_eq!(arr[0]["timestamp_ms"].as_u64().unwrap(), 3000);
    assert_eq!(arr[1]["timestamp_ms"].as_u64().unwrap(), 2000);
}

use resource_monitor::metrics::RpcMetricsSnapshot;

#[tokio::test]
async fn latest_no_data_returns_404() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let db = Arc::new(MetricsDb::new(&db_path).unwrap());
    let buffer = Arc::new(MetricsBuffer::new(10));
    let (stream_tx, _stream_rx) = tokio::sync::broadcast::channel(8);
    let app = router(AppState {
        buffer,
        db,
        stream_tx,
        shutdown: CancellationToken::new(),
    });

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/latest")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 404);
}

#[tokio::test]
async fn latest_prefers_buffer_over_db() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let db = Arc::new(MetricsDb::new(&db_path).unwrap());

    db.insert(&sample_snapshot(1000)).unwrap();

    let buffer = Arc::new(MetricsBuffer::new(10));
    buffer.push(sample_snapshot(2000));

    let (stream_tx, _stream_rx) = tokio::sync::broadcast::channel(8);
    let app = router(AppState {
        buffer,
        db,
        stream_tx,
        shutdown: CancellationToken::new(),
    });

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/latest")
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
    assert_eq!(json["timestamp_ms"].as_u64().unwrap(), 2000);
}

#[tokio::test]
async fn db_stats_empty() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let db = Arc::new(MetricsDb::new(&db_path).unwrap());
    let buffer = Arc::new(MetricsBuffer::new(10));
    let (stream_tx, _stream_rx) = tokio::sync::broadcast::channel(8);
    let app = router(AppState {
        buffer,
        db,
        stream_tx,
        shutdown: CancellationToken::new(),
    });

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/db/stats")
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
    assert_eq!(json["total_records"].as_u64().unwrap(), 0);
    assert!(json["oldest_timestamp"].is_null());
    assert!(json["newest_timestamp"].is_null());
}

#[tokio::test]
async fn db_stats_with_data() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let db = Arc::new(MetricsDb::new(&db_path).unwrap());

    db.insert(&sample_snapshot(1000)).unwrap();
    db.insert(&sample_snapshot(2000)).unwrap();
    db.insert(&sample_snapshot(3000)).unwrap();

    let buffer = Arc::new(MetricsBuffer::new(10));
    let (stream_tx, _stream_rx) = tokio::sync::broadcast::channel(8);
    let app = router(AppState {
        buffer,
        db,
        stream_tx,
        shutdown: CancellationToken::new(),
    });

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/db/stats")
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
    assert_eq!(json["total_records"].as_u64().unwrap(), 3);
    assert_eq!(json["oldest_timestamp"].as_u64().unwrap(), 1000);
    assert_eq!(json["newest_timestamp"].as_u64().unwrap(), 3000);
    assert!(json["database_size_bytes"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn index_returns_html() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let db = Arc::new(MetricsDb::new(&db_path).unwrap());
    let buffer = Arc::new(MetricsBuffer::new(10));
    let (stream_tx, _stream_rx) = tokio::sync::broadcast::channel(8);
    let app = router(AppState {
        buffer,
        db,
        stream_tx,
        shutdown: CancellationToken::new(),
    });

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/")
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
    assert!(ct.contains("text/html"));
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let html = String::from_utf8_lossy(&body);
    assert!(html.contains("Resource Monitor"));
}

#[tokio::test]
async fn health_response_has_ok_status() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let db = Arc::new(MetricsDb::new(&db_path).unwrap());
    let buffer = Arc::new(MetricsBuffer::new(10));
    let (stream_tx, _stream_rx) = tokio::sync::broadcast::channel(8);
    let app = router(AppState {
        buffer,
        db,
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
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"].as_str().unwrap(), "ok");
}

#[tokio::test]
async fn range_empty_result_for_future_range() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let db = Arc::new(MetricsDb::new(&db_path).unwrap());
    db.insert(&sample_snapshot(1000)).unwrap();

    let buffer = Arc::new(MetricsBuffer::new(10));
    let (stream_tx, _stream_rx) = tokio::sync::broadcast::channel(8);
    let app = router(AppState {
        buffer,
        db,
        stream_tx,
        shutdown: CancellationToken::new(),
    });

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/range?from_ts=9000&to_ts=10000")
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
    assert_eq!(json.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn latest_snapshot_has_valid_rpc_structure() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let db = Arc::new(MetricsDb::new(&db_path).unwrap());
    let buffer = Arc::new(MetricsBuffer::new(10));
    buffer.push(sample_snapshot(5000));

    let (stream_tx, _stream_rx) = tokio::sync::broadcast::channel(8);
    let app = router(AppState {
        buffer,
        db,
        stream_tx,
        shutdown: CancellationToken::new(),
    });

    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/latest")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let rpc: RpcMetricsSnapshot = serde_json::from_slice(&body).unwrap();
    assert_eq!(rpc.timestamp_ms, 5000);
    assert!(!rpc.data.is_empty());
    assert!(rpc.data.iter().any(|s| s.name == "cpu_total"));
    assert!(rpc.data.iter().any(|s| s.name == "memory"));
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
