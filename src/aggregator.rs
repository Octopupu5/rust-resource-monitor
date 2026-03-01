use crate::bus::publish_snapshot;
use crate::metrics::{
    now_timestamp_ms, BatteryMetrics, CpuMetrics, DiskMetrics, GpuMetrics, MemoryMetrics,
    MetricsSnapshot, NetworkMetrics,
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

            let battery_metrics = get_battery_metrics();
            let gpu_metrics = get_gpu_metrics();

            if let Some(battery) = &battery_metrics {
                debug!(
                    "Battery: {}% ({}), Power: {}W, Time to empty: {:?}",
                    battery.percentage, battery.state, battery.power_now, battery.time_to_empty
                );
            }
            if let Some(gpu) = &gpu_metrics {
                debug!(
                    "GPU: {} util={}%, mem={}/{} unified={}",
                    gpu.name,
                    gpu.gpu_utilization_pct,
                    gpu.vram_used_bytes,
                    gpu.vram_total_bytes,
                    gpu.is_unified_memory
                );
            }

            let per_core: Vec<f32> = sys.cpus().iter().map(|c| c.cpu_usage()).collect();
            let total_pct = if per_core.is_empty() {
                0.0
            } else {
                per_core.iter().sum::<f32>() / per_core.len() as f32
            };

            let la = System::load_average();

            let total_mem_bytes = sys.total_memory();
            let used_mem_bytes = sys.used_memory();
            let avail_mem_bytes = sys.available_memory();
            let swap_total_bytes = sys.total_swap();
            let swap_used_bytes = sys.used_swap();

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
                gpu: gpu_metrics,
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

fn get_gpu_metrics() -> Option<GpuMetrics> {
    try_nvidia_smi().or_else(try_macos_ioreg)
}

fn try_nvidia_smi() -> Option<GpuMetrics> {
    let output = std::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,utilization.gpu,memory.total,memory.used,temperature.gpu",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let parts: Vec<&str> = text.trim().split(", ").collect();
    if parts.len() < 5 {
        return None;
    }

    Some(GpuMetrics {
        name: parts[0].to_string(),
        gpu_utilization_pct: parts[1].trim().parse().ok()?,
        vram_total_bytes: parts[2].trim().parse::<u64>().ok()? * 1024 * 1024,
        vram_used_bytes: parts[3].trim().parse::<u64>().ok()? * 1024 * 1024,
        temperature_celsius: parts[4].trim().parse().ok(),
        is_unified_memory: false,
    })
}

fn try_macos_ioreg() -> Option<GpuMetrics> {
    let output = std::process::Command::new("ioreg")
        .args(["-r", "-d", "1", "-c", "IOAccelerator"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    if text.is_empty() {
        return None;
    }

    let utilization = parse_ioreg_int(&text, "Device Utilization %")
        .or_else(|| parse_ioreg_int(&text, "GPU Activity(%)"))
        .unwrap_or(0) as f32;

    let vram_used = parse_ioreg_int(&text, "In Use System Memory").unwrap_or(0) as u64;
    let vram_total = parse_ioreg_int(&text, "Alloc system memory")
        .or_else(|| parse_ioreg_int(&text, "Allocated System Memory"))
        .map(|v| v as u64)
        .or_else(|| {
            let total = parse_ioreg_int(&text, "VRAM,totalMB").unwrap_or(0) as u64;
            if total > 0 {
                Some(total * 1024 * 1024)
            } else {
                None
            }
        })
        .unwrap_or(0);

    if vram_used == 0 && vram_total == 0 && utilization == 0.0 {
        return None;
    }

    let name = parse_ioreg_string(&text, "model")
        .or_else(detect_macos_gpu_name)
        .unwrap_or_else(|| "Apple GPU".to_string());

    Some(GpuMetrics {
        name,
        gpu_utilization_pct: utilization,
        vram_total_bytes: vram_total,
        vram_used_bytes: vram_used,
        temperature_celsius: None,
        is_unified_memory: true,
    })
}

fn parse_ioreg_int(text: &str, key: &str) -> Option<i64> {
    let search = format!("\"{}\"", key);
    for line in text.lines() {
        if !line.contains(&search) {
            continue;
        }
        if let Some(pos) = line.find(&search) {
            let after = &line[pos + search.len()..];
            let after = after.trim_start().trim_start_matches('=').trim_start();
            let num_str: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !num_str.is_empty() {
                return num_str.parse().ok();
            }
        }
    }
    None
}

fn parse_ioreg_string(text: &str, key: &str) -> Option<String> {
    let search = format!("\"{}\"", key);
    for line in text.lines() {
        if !line.contains(&search) {
            continue;
        }
        if let Some(pos) = line.find(&search) {
            let after = &line[pos + search.len()..];
            let after = after.trim_start().trim_start_matches('=').trim_start();
            if let Some(inner) = after.strip_prefix('"') {
                if let Some(end) = inner.find('"') {
                    return Some(inner[..end].to_string());
                }
            }
        }
    }
    None
}

fn detect_macos_gpu_name() -> Option<String> {
    let output = std::process::Command::new("system_profiler")
        .args(["SPDisplaysDataType", "-json"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let displays = json.get("SPDisplaysDataType")?.as_array()?;
    let first = displays.first()?;
    first
        .get("sppci_model")
        .and_then(|v| v.as_str())
        .map(String::from)
}
