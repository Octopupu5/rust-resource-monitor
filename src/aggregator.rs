use crate::bus::publish_snapshot;
use crate::metrics::{
    now_timestamp_ms, BatteryMetrics, CpuMetrics, DiskMetrics, MemoryMetrics, MetricsSnapshot,
    NetworkMetrics,
};
use battery::{Manager, State};
use std::time::{Duration, Instant};
use sysinfo::{CpuRefreshKind, Disks, MemoryRefreshKind, Networks, RefreshKind, System};
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

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
        let mut networks = Networks::new_with_refreshed_list();
        let mut disks = Disks::new_with_refreshed_list();
        let mut sys = System::new_with_specifics(
            RefreshKind::everything()
                .with_cpu(CpuRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything()),
        );

        sys.refresh_all();
        networks.refresh(true);
        disks.refresh(true);

        let mut last_time = Instant::now();
        let mut last_rx_total: u64 = sum_network_rx(&networks);
        let mut last_tx_total: u64 = sum_network_tx(&networks);

        info!(
            "Aggregator started with interval {:?}",
            self.config.interval
        );

        let mut ticker = tokio::time::interval(self.config.interval);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut is_first = true;

        loop {
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

            sys.refresh_all();
            networks.refresh(false);
            disks.refresh(false);

            // Получаем данные о батарее - инициализируем менеджер каждый раз
            // или сохраняем в локальную переменную внутри цикла
            let battery_metrics = get_battery_metrics();

            // Логируем данные о батарее для отладки
            if let Some(battery) = &battery_metrics {
                debug!(
                    "Battery: {}% ({}), Power: {}W, Time to empty: {:?}",
                    battery.percentage, battery.state, battery.power_now, battery.time_to_empty
                );
            }

            let per_core: Vec<f32> = sys.cpus().iter().map(|c| c.cpu_usage()).collect();
            let total_pct = if per_core.is_empty() {
                0.0
            } else {
                per_core.iter().sum::<f32>() / per_core.len() as f32
            };

            let la = System::load_average();

            let total_mem_bytes = sys.total_memory().saturating_mul(1024);
            let used_mem_bytes = sys.used_memory().saturating_mul(1024);
            let avail_mem_bytes = sys.available_memory().saturating_mul(1024);
            let swap_total_bytes = sys.total_swap().saturating_mul(1024);
            let swap_used_bytes = sys.used_swap().saturating_mul(1024);

            let rx_total = sum_network_rx(&networks);
            let tx_total = sum_network_tx(&networks);
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

            let disk_total = sum_disk_total(&disks);
            let disk_avail = sum_disk_avail(&disks);
            let disk_used_pct = if disk_total == 0 {
                0.0
            } else {
                (disk_total.saturating_sub(disk_avail)) as f32 / disk_total as f32 * 100.0
            };

            let snapshot = MetricsSnapshot {
                timestamp_ms: now_timestamp_ms(),
                cpu: CpuMetrics {
                    total_usage_pct: total_pct,
                    per_core_usage_pct: per_core,
                    load_avg_1: la.one as f32,
                    load_avg_5: la.five as f32,
                    load_avg_15: la.fifteen as f32,
                    temperature_celsius: None,
                },
                memory: MemoryMetrics {
                    total_bytes: total_mem_bytes,
                    used_bytes: used_mem_bytes,
                    available_bytes: avail_mem_bytes,
                    swap_total_bytes,
                    swap_used_bytes,
                },
                network: NetworkMetrics {
                    rx_bytes_total: rx_total,
                    tx_bytes_total: tx_total,
                    rx_bytes_per_sec: rx_rate,
                    tx_bytes_per_sec: tx_rate,
                },
                disk: DiskMetrics {
                    total_bytes: disk_total,
                    available_bytes: disk_avail,
                    used_pct: disk_used_pct,
                },
                battery: battery_metrics,
            };

            publish_snapshot(snapshot);

            last_time = now;
            last_rx_total = rx_total;
            last_tx_total = tx_total;
            is_first = false;
        }
    }
}

fn get_battery_metrics() -> Option<BatteryMetrics> {
    let manager = match Manager::new() {
        Ok(m) => m,
        Err(e) => {
            debug!("Failed to initialize battery manager: {}", e);
            return None;
        }
    };

    let mut batteries = match manager.batteries() {
        Ok(b) => b,
        Err(e) => {
            debug!("Failed to get batteries: {}", e);
            return None;
        }
    };

    if let Some(Ok(battery)) = batteries.next() {
        let state = match battery.state() {
            State::Charging => "Charging",
            State::Discharging => "Discharging",
            State::Empty => "Empty",
            State::Full => "Full",
            _ => "Unknown",
        };

        let time_to_empty = battery.time_to_empty().map(|t| {
            let seconds = t.get::<battery::units::time::second>();
            seconds.round() as u64
        });

        let time_to_full = battery.time_to_full().map(|t| {
            let seconds = t.get::<battery::units::time::second>();
            seconds.round() as u64
        });

        Some(BatteryMetrics {
            percentage: battery.state_of_charge().value * 100.0,
            voltage: battery.voltage().value,
            temperature: battery.temperature().map(|t| t.value),
            energy_full: battery.energy_full().value as u64,
            energy_now: battery.energy().value as u64,
            power_now: battery.energy_rate().value,
            time_to_empty,
            time_to_full,
            state: state.to_string(),
        })
    } else {
        debug!("No batteries found");
        None
    }
}

fn sum_network_rx(networks: &Networks) -> u64 {
    networks
        .iter()
        .fold(0, |acc, (_, data)| acc + data.total_received())
}

fn sum_network_tx(networks: &Networks) -> u64 {
    networks
        .iter()
        .fold(0, |acc, (_, data)| acc + data.total_transmitted())
}

fn sum_disk_total(disks: &Disks) -> u64 {
    disks.iter().fold(0, |acc, disk| acc + disk.total_space())
}

fn sum_disk_avail(disks: &Disks) -> u64 {
    disks
        .iter()
        .fold(0, |acc, disk| acc + disk.available_space())
}
