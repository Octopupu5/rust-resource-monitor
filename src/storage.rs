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
