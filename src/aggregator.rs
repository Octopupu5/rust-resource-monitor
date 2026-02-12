use crate::bus::publish_snapshot;
use crate::metrics::{
    now_timestamp_ms, CpuMetrics, MemoryMetrics, MetricsSnapshot, NetworkMetrics,
};
use std::time::{Duration, Instant};
use sysinfo::{CpuExt, CpuRefreshKind, NetworkExt, NetworksExt, RefreshKind, System, SystemExt};
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

pub struct AggregatorConfig {
    pub interval: Duration,
}

impl AggregatorConfig {
    pub fn new(interval: Duration) -> Self {
        Self { interval }
    }
}

pub struct Aggregator {
    config: AggregatorConfig,
}

impl Aggregator {
    pub fn new(config: AggregatorConfig) -> Self {
        Self { config }
    }

    pub async fn run(self, cancel: CancellationToken) {
        let refresh = RefreshKind::new()
            .with_cpu(CpuRefreshKind::everything())
            .with_memory()
            .with_components()
            .with_disks_list()
            .with_disks();
        let mut sys = System::new_with_specifics(refresh);

        // Initialize once before loop to compute deltas.
        sys.refresh_cpu();
        sys.refresh_memory();
        sys.refresh_networks();

        let mut last_time = Instant::now();
        let mut last_rx_total: u64 = sum_network_rx(&sys);
        let mut last_tx_total: u64 = sum_network_tx(&sys);

        info!(
            "Aggregator started with interval {:?}",
            self.config.interval
        );

        let mut ticker = tokio::time::interval(self.config.interval);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut is_first = true;
        loop {
            // interval() ticks immediately on the first await, which gives us a fast first sample.
            tokio::select! {
                _ = cancel.cancelled() => {
                    break;
                }
                _ = ticker.tick() => {}
            }

            let now = Instant::now();
            let elapsed = now.saturating_duration_since(last_time);
            let dt = if is_first {
                self.config.interval.as_secs_f32().max(0.001)
            } else {
                elapsed.as_secs_f32()
            };
            if dt <= 0.0 {
                warn!("Non-positive elapsed time detected, skipping sample");
                continue;
            }

            // Refresh subsets to keep overhead low.
            sys.refresh_cpu();
            sys.refresh_memory();
            sys.refresh_networks();

            let per_core: Vec<f32> = sys.cpus().iter().map(|c| c.cpu_usage()).collect();
            let total_pct = if per_core.is_empty() {
                0.0
            } else {
                per_core.iter().sum::<f32>() / per_core.len() as f32
            };
            let la = sys.load_average();

            let total_mem_bytes = sys.total_memory();
            let used_mem_bytes = sys.used_memory();
            let avail_mem_bytes = sys.available_memory();
            // sysinfo returns KiB; convert to bytes
            let total_mem_bytes = total_mem_bytes.saturating_mul(1024);
            let used_mem_bytes = used_mem_bytes.saturating_mul(1024);
            let avail_mem_bytes = avail_mem_bytes.saturating_mul(1024);

            let rx_total = sum_network_rx(&sys);
            let tx_total = sum_network_tx(&sys);
            let rx_rate = if is_first {
                0.0
            } else if rx_total >= last_rx_total {
                (rx_total - last_rx_total) as f32 / dt
            } else {
                warn!("Network RX counter decreased; possible interface reset");
                0.0
            };
            let tx_rate = if is_first {
                0.0
            } else if tx_total >= last_tx_total {
                (tx_total - last_tx_total) as f32 / dt
            } else {
                warn!("Network TX counter decreased; possible interface reset");
                0.0
            };

            let snapshot = MetricsSnapshot {
                timestamp_ms: now_timestamp_ms(),
                cpu: CpuMetrics {
                    total_usage_pct: total_pct,
                    per_core_usage_pct: per_core,
                    load_avg_1: la.one as f32,
                    load_avg_5: la.five as f32,
                    load_avg_15: la.fifteen as f32,
                },
                memory: MemoryMetrics {
                    total_bytes: total_mem_bytes,
                    used_bytes: used_mem_bytes,
                    available_bytes: avail_mem_bytes,
                },
                network: NetworkMetrics {
                    rx_bytes_total: rx_total,
                    tx_bytes_total: tx_total,
                    rx_bytes_per_sec: rx_rate,
                    tx_bytes_per_sec: tx_rate,
                },
            };

            publish_snapshot(snapshot);

            last_time = now;
            last_rx_total = rx_total;
            last_tx_total = tx_total;
            is_first = false;
        }
    }
}

fn sum_network_rx(sys: &System) -> u64 {
    sys.networks().iter().map(|(_, n)| n.total_received()).sum()
}

fn sum_network_tx(sys: &System) -> u64 {
    sys.networks()
        .iter()
        .map(|(_, n)| n.total_transmitted())
        .sum()
}
