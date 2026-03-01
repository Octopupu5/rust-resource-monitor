use resource_monitor::metrics::{
    CpuMetrics, DiskMetrics, MemoryMetrics, MetricsSnapshot, NetworkMetrics,
};
use resource_monitor::storage::MetricsBuffer;

fn sample(i: u128) -> MetricsSnapshot {
    MetricsSnapshot {
        timestamp_ms: i,
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

#[test]
fn latest_on_empty_buffer_returns_none() {
    let buf = MetricsBuffer::new(5);
    assert!(buf.latest().is_none());
}

#[test]
fn history_on_empty_buffer_returns_empty() {
    let buf = MetricsBuffer::new(5);
    assert!(buf.history(None).is_empty());
    assert!(buf.history(Some(10)).is_empty());
}

#[test]
fn push_and_latest_returns_last() {
    let buf = MetricsBuffer::new(5);
    buf.push(sample(100));
    buf.push(sample(200));
    assert_eq!(buf.latest().unwrap().timestamp_ms, 200);
}

#[test]
fn history_returns_all_when_no_limit() {
    let buf = MetricsBuffer::new(10);
    for i in 1..=5 {
        buf.push(sample(i));
    }
    let hist = buf.history(None);
    assert_eq!(hist.len(), 5);
    assert_eq!(hist[0].timestamp_ms, 1);
    assert_eq!(hist[4].timestamp_ms, 5);
}

#[test]
fn history_respects_limit() {
    let buf = MetricsBuffer::new(10);
    for i in 1..=5 {
        buf.push(sample(i));
    }
    let hist = buf.history(Some(3));
    assert_eq!(hist.len(), 3);
    assert_eq!(hist[0].timestamp_ms, 3);
    assert_eq!(hist[2].timestamp_ms, 5);
}

#[test]
fn history_limit_larger_than_len_returns_all() {
    let buf = MetricsBuffer::new(10);
    buf.push(sample(1));
    buf.push(sample(2));
    let hist = buf.history(Some(100));
    assert_eq!(hist.len(), 2);
}

#[test]
fn history_limit_zero_returns_empty() {
    let buf = MetricsBuffer::new(10);
    buf.push(sample(1));
    buf.push(sample(2));
    let hist = buf.history(Some(0));
    assert!(hist.is_empty());
}

#[test]
fn trims_to_capacity() {
    let buf = MetricsBuffer::new(3);
    buf.push(sample(1));
    buf.push(sample(2));
    buf.push(sample(3));
    buf.push(sample(4));
    let hist = buf.history(None);
    assert_eq!(hist.len(), 3);
    assert_eq!(hist[0].timestamp_ms, 2);
    assert_eq!(hist[2].timestamp_ms, 4);
    assert_eq!(buf.latest().unwrap().timestamp_ms, 4);
}

#[test]
fn capacity_one_keeps_only_last() {
    let buf = MetricsBuffer::new(1);
    buf.push(sample(10));
    buf.push(sample(20));
    buf.push(sample(30));
    assert_eq!(buf.latest().unwrap().timestamp_ms, 30);
    assert_eq!(buf.history(None).len(), 1);
}
