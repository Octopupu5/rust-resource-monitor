use crate::metrics::MetricsSnapshot;
use std::collections::VecDeque;
use std::sync::RwLock;

pub struct MetricsBuffer {
    capacity: usize,
    inner: RwLock<VecDeque<MetricsSnapshot>>,
}

impl MetricsBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            inner: RwLock::new(VecDeque::with_capacity(capacity)),
        }
    }

    pub fn push(&self, snapshot: MetricsSnapshot) {
        let mut guard = match self.inner.write() {
            Ok(g) => g,
            Err(poisoned) => {
                // Continue with the inner value even if poisoned.
                poisoned.into_inner()
            }
        };
        if guard.len() >= self.capacity {
            // Trim oldest to make room.
            guard.pop_front();
        }
        guard.push_back(snapshot);
    }

    pub fn latest(&self) -> Option<MetricsSnapshot> {
        let guard = match self.inner.read() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.back().cloned()
    }

    pub fn history(&self, limit: Option<usize>) -> Vec<MetricsSnapshot> {
        let guard = match self.inner.read() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let len = guard.len();
        let take = limit.unwrap_or(len).min(len);
        guard.iter().skip(len - take).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::{CpuMetrics, DiskMetrics, MemoryMetrics, MetricsSnapshot, NetworkMetrics};

    fn sample(i: u128) -> MetricsSnapshot {
        MetricsSnapshot {
            timestamp_ms: i,
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
        }
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
}
