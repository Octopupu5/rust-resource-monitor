use resource_monitor::db::MetricsDb;
use resource_monitor::metrics::{
    CpuMetrics, DiskMetrics, MemoryMetrics, MetricsSnapshot, NetworkMetrics,
};
use tempfile::tempdir;

fn sample(ts: u128) -> MetricsSnapshot {
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

fn test_db() -> (tempfile::TempDir, MetricsDb) {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let db = MetricsDb::new(&db_path).unwrap();
    (dir, db)
}

#[test]
fn get_latest_empty_returns_none() {
    let (_dir, db) = test_db();
    assert!(db.get_latest().unwrap().is_none());
}

#[test]
fn insert_and_get_latest() {
    let (_dir, db) = test_db();
    db.insert(&sample(1000)).unwrap();
    let latest = db.get_latest().unwrap().unwrap();
    assert_eq!(latest.timestamp_ms, 1000);
}

#[test]
fn latest_returns_most_recent() {
    let (_dir, db) = test_db();
    db.insert(&sample(1000)).unwrap();
    db.insert(&sample(2000)).unwrap();
    db.insert(&sample(3000)).unwrap();
    let latest = db.get_latest().unwrap().unwrap();
    assert_eq!(latest.timestamp_ms, 3000);
}

#[test]
fn insert_duplicate_timestamp_replaces() {
    let (_dir, db) = test_db();
    db.insert(&sample(1000)).unwrap();
    db.insert(&sample(1000)).unwrap();
    let history = db.get_history(None, None).unwrap();
    assert_eq!(history.len(), 1);
}

#[test]
fn get_range_filters_correctly() {
    let (_dir, db) = test_db();
    db.insert(&sample(1000)).unwrap();
    db.insert(&sample(2000)).unwrap();
    db.insert(&sample(3000)).unwrap();

    let range = db.get_range(1500, 2500, None).unwrap();
    assert_eq!(range.len(), 1);
    assert_eq!(range[0].timestamp_ms, 2000);
}

#[test]
fn get_range_no_matches_returns_empty() {
    let (_dir, db) = test_db();
    db.insert(&sample(1000)).unwrap();
    let range = db.get_range(5000, 6000, None).unwrap();
    assert!(range.is_empty());
}

#[test]
fn get_range_with_limit() {
    let (_dir, db) = test_db();
    for ts in [1000, 2000, 3000, 4000, 5000] {
        db.insert(&sample(ts)).unwrap();
    }
    let range = db.get_range(1000, 5000, Some(2)).unwrap();
    assert_eq!(range.len(), 2);
    assert_eq!(range[0].timestamp_ms, 5000);
    assert_eq!(range[1].timestamp_ms, 4000);
}

#[test]
fn get_range_inclusive_boundaries() {
    let (_dir, db) = test_db();
    db.insert(&sample(1000)).unwrap();
    db.insert(&sample(2000)).unwrap();
    db.insert(&sample(3000)).unwrap();
    let range = db.get_range(1000, 3000, None).unwrap();
    assert_eq!(range.len(), 3);
}

#[test]
fn get_history_all_records() {
    let (_dir, db) = test_db();
    for ts in [1000, 2000, 3000] {
        db.insert(&sample(ts)).unwrap();
    }
    let history = db.get_history(None, None).unwrap();
    assert_eq!(history.len(), 3);
    assert_eq!(history[0].timestamp_ms, 3000);
    assert_eq!(history[2].timestamp_ms, 1000);
}

#[test]
fn get_history_with_limit_only() {
    let (_dir, db) = test_db();
    for ts in [1000, 2000, 3000, 4000] {
        db.insert(&sample(ts)).unwrap();
    }
    let history = db.get_history(Some(2), None).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].timestamp_ms, 4000);
    assert_eq!(history[1].timestamp_ms, 3000);
}

#[test]
fn get_history_with_since_only() {
    let (_dir, db) = test_db();
    for ts in [1000, 2000, 3000, 4000] {
        db.insert(&sample(ts)).unwrap();
    }
    let history = db.get_history(None, Some(2500)).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].timestamp_ms, 4000);
    assert_eq!(history[1].timestamp_ms, 3000);
}

#[test]
fn get_history_with_since_and_limit() {
    let (_dir, db) = test_db();
    for ts in [1000, 2000, 3000, 4000, 5000] {
        db.insert(&sample(ts)).unwrap();
    }
    let history = db.get_history(Some(2), Some(2000)).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].timestamp_ms, 5000);
    assert_eq!(history[1].timestamp_ms, 4000);
}

#[test]
fn get_history_empty_returns_empty() {
    let (_dir, db) = test_db();
    let history = db.get_history(None, None).unwrap();
    assert!(history.is_empty());
}

#[test]
fn get_stats_empty_db() {
    let (_dir, db) = test_db();
    let stats = db.get_stats().unwrap();
    assert_eq!(stats.total_records, 0);
    assert!(stats.oldest_timestamp.is_none());
    assert!(stats.newest_timestamp.is_none());
}

#[test]
fn get_stats_with_data() {
    let (_dir, db) = test_db();
    db.insert(&sample(1000)).unwrap();
    db.insert(&sample(2000)).unwrap();
    db.insert(&sample(3000)).unwrap();

    let stats = db.get_stats().unwrap();
    assert_eq!(stats.total_records, 3);
    assert_eq!(stats.oldest_timestamp, Some(1000));
    assert_eq!(stats.newest_timestamp, Some(3000));
    assert!(stats.database_size_bytes > 0);
}

#[test]
fn cleanup_old_removes_expired_records() {
    let (_dir, db) = test_db();
    let now_ms = chrono::Utc::now().timestamp_millis() as u128;
    let two_days_ago = now_ms - 2 * 24 * 3600 * 1000;
    let one_hour_ago = now_ms - 3600 * 1000;

    db.insert(&sample(two_days_ago)).unwrap();
    db.insert(&sample(one_hour_ago)).unwrap();
    db.insert(&sample(now_ms)).unwrap();

    let deleted = db.cleanup_old(24).unwrap();
    assert_eq!(deleted, 1);

    let remaining = db.get_history(None, None).unwrap();
    assert_eq!(remaining.len(), 2);
}

#[test]
fn cleanup_old_keeps_all_when_none_expired() {
    let (_dir, db) = test_db();
    let now_ms = chrono::Utc::now().timestamp_millis() as u128;
    db.insert(&sample(now_ms - 1000)).unwrap();
    db.insert(&sample(now_ms)).unwrap();

    let deleted = db.cleanup_old(24).unwrap();
    assert_eq!(deleted, 0);
    assert_eq!(db.get_history(None, None).unwrap().len(), 2);
}

#[test]
fn vacuum_succeeds() {
    let (_dir, db) = test_db();
    db.insert(&sample(1000)).unwrap();
    db.vacuum().unwrap();
    assert_eq!(db.get_latest().unwrap().unwrap().timestamp_ms, 1000);
}
