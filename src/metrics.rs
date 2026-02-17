use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CpuMetrics {
    pub total_usage_pct: f32,
    pub per_core_usage_pct: Vec<f32>,
    pub load_avg_1: f32,
    pub load_avg_5: f32,
    pub load_avg_15: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryMetrics {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NetworkMetrics {
    pub rx_bytes_total: u64,
    pub tx_bytes_total: u64,
    pub rx_bytes_per_sec: f32,
    pub tx_bytes_per_sec: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    pub timestamp_ms: u128,
    pub cpu: CpuMetrics,
    pub memory: MemoryMetrics,
    pub network: NetworkMetrics,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
}

pub fn now_timestamp_ms() -> u128 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(dur) => dur.as_millis(),
        Err(err) => {
            // System clock is before UNIX_EPOCH; return 0 and let caller decide what to do.
            tracing::error!("SystemTime before UNIX_EPOCH: {}", err);
            0
        }
    }
}


