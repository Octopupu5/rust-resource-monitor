use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RpcMetricsSnapshot {
    pub timestamp_ms: u128,
    pub data: Vec<MetricSeries>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetricSeries {
    pub name: String,
    pub beautiful_name: String,
    pub series: Vec<f32>,
    pub legend: Vec<MetricLegend>,
    pub format: DisplayFormat,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetricLegend {
    pub name: String,
    pub color: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "params")]
pub enum DisplayFormat {
    Percentage { decimals: usize },
    Bytes { suffix: String },
    Float { decimals: usize },
    Integer,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BatteryMetrics {
    pub percentage: f32,
    pub voltage: f32,
    pub temperature: Option<f32>,
    pub energy_full: u64,
    pub energy_now: u64,
    pub power_now: f32,
    pub time_to_empty: Option<u64>,
    pub time_to_full: Option<u64>,
    pub state: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CpuMetrics {
    pub total_usage_pct: f32,
    pub per_core_usage_pct: Vec<f32>,
    pub load_avg_1: f32,
    pub load_avg_5: f32,
    pub load_avg_15: f32,
    pub temperature_celsius: Option<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryMetrics {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NetworkMetrics {
    pub rx_bytes_total: u64,
    pub tx_bytes_total: u64,
    pub rx_bytes_per_sec: f32,
    pub tx_bytes_per_sec: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiskMetrics {
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub used_pct: f32,
}

// Внутренняя структура для хранения в буфере
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    pub timestamp_ms: u128,
    pub cpu: CpuMetrics,
    pub memory: MemoryMetrics,
    pub network: NetworkMetrics,
    pub disk: DiskMetrics,
    pub battery: Option<BatteryMetrics>,
}

impl MetricsSnapshot {
    pub fn to_rpc_format(&self) -> RpcMetricsSnapshot {
        let total_mem_bytes = self.memory.total_bytes;
        let used_mem_bytes = self.memory.used_bytes;
        let mem_used_pct = if total_mem_bytes > 0 {
            (used_mem_bytes as f32 / total_mem_bytes as f32) * 100.0
        } else {
            0.0
        };

        let swap_total = self.memory.swap_total_bytes;
        let swap_used = self.memory.swap_used_bytes;
        let swap_used_pct = if swap_total > 0 {
            (swap_used as f32 / swap_total as f32) * 100.0
        } else {
            0.0
        };

        let mut data = vec![
            // CPU total
            MetricSeries {
                name: "cpu_total".to_string(),
                beautiful_name: "CPU total (%)".to_string(),
                series: vec![self.cpu.total_usage_pct],
                legend: vec![MetricLegend {
                    name: "CPU".to_string(),
                    color: "#c44".to_string(),
                }],
                format: DisplayFormat::Percentage { decimals: 1 },
            },
            // CPU per core
            MetricSeries {
                name: "cpu_cores".to_string(),
                beautiful_name: "CPU per core (%)".to_string(),
                series: self.cpu.per_core_usage_pct.clone(),
                legend: self
                    .cpu
                    .per_core_usage_pct
                    .iter()
                    .enumerate()
                    .map(|(i, _)| {
                        let hue = (i as f32 / self.cpu.per_core_usage_pct.len() as f32) * 360.0;
                        MetricLegend {
                            name: format!("C{}", i),
                            color: format!("hsl({}, 80%, 60%)", hue),
                        }
                    })
                    .collect(),
                format: DisplayFormat::Percentage { decimals: 1 },
            },
            // Load average
            MetricSeries {
                name: "load_avg".to_string(),
                beautiful_name: "CPU Load Average".to_string(),
                series: vec![
                    self.cpu.load_avg_1,
                    self.cpu.load_avg_5,
                    self.cpu.load_avg_15,
                ],
                legend: vec![
                    MetricLegend {
                        name: "1m".to_string(),
                        color: "#e879f9".to_string(),
                    },
                    MetricLegend {
                        name: "5m".to_string(),
                        color: "#a78bfa".to_string(),
                    },
                    MetricLegend {
                        name: "15m".to_string(),
                        color: "#60a5fa".to_string(),
                    },
                ],
                format: DisplayFormat::Float { decimals: 2 },
            },
            // Memory used
            MetricSeries {
                name: "memory".to_string(),
                beautiful_name: "Memory used (%)".to_string(),
                series: vec![mem_used_pct],
                legend: vec![MetricLegend {
                    name: "Memory".to_string(),
                    color: "#f59e0b".to_string(),
                }],
                format: DisplayFormat::Percentage { decimals: 1 },
            },
            // Swap used
            MetricSeries {
                name: "swap".to_string(),
                beautiful_name: "Swap used (%)".to_string(),
                series: vec![swap_used_pct],
                legend: vec![MetricLegend {
                    name: "Swap".to_string(),
                    color: "#818cf8".to_string(),
                }],
                format: DisplayFormat::Percentage { decimals: 1 },
            },
            // Network RX/TX
            MetricSeries {
                name: "network".to_string(),
                beautiful_name: "Network".to_string(),
                series: vec![self.network.rx_bytes_per_sec, self.network.tx_bytes_per_sec],
                legend: vec![
                    MetricLegend {
                        name: "RX".to_string(),
                        color: "#0b6".to_string(),
                    },
                    MetricLegend {
                        name: "TX".to_string(),
                        color: "#06b".to_string(),
                    },
                ],
                format: DisplayFormat::Bytes {
                    suffix: "B/s".to_string(),
                },
            },
            // Disk used
            MetricSeries {
                name: "disk".to_string(),
                beautiful_name: "Disk used (%)".to_string(),
                series: vec![self.disk.used_pct],
                legend: vec![MetricLegend {
                    name: "Disk".to_string(),
                    color: "#34d399".to_string(),
                }],
                format: DisplayFormat::Percentage { decimals: 1 },
            },
        ];

        // Добавляем батарею если доступна
        if let Some(battery) = &self.battery {
            data.extend(vec![
                // Battery percentage
                MetricSeries {
                    name: "battery".to_string(),
                    beautiful_name: "Battery Level".to_string(),
                    series: vec![battery.percentage],
                    legend: vec![MetricLegend {
                        name: battery.state.clone(),
                        color: get_battery_color(battery.percentage),
                    }],
                    format: DisplayFormat::Percentage { decimals: 1 },
                },
                // Battery power
                MetricSeries {
                    name: "battery_power".to_string(),
                    beautiful_name: "Battery Power".to_string(),
                    series: vec![battery.power_now],
                    legend: vec![MetricLegend {
                        name: "Power".to_string(),
                        color: "#fbbf24".to_string(),
                    }],
                    format: DisplayFormat::Float { decimals: 2 },
                },
            ]);
        }

        RpcMetricsSnapshot {
            timestamp_ms: self.timestamp_ms,
            data,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
}

pub fn now_timestamp_ms() -> u128 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(dur) => dur.as_millis(),
        Err(err) => {
            tracing::error!("SystemTime before UNIX_EPOCH: {}", err);
            0
        }
    }
}

fn get_battery_color(percentage: f32) -> String {
    if percentage >= 90.0 {
        "#4ade80".to_string() // Зеленый
    } else if percentage >= 60.0 {
        "#22c55e".to_string() // Светло-зеленый
    } else if percentage >= 30.0 {
        "#fbbf24".to_string() // Желтый
    } else if percentage >= 10.0 {
        "#f97316".to_string() // Оранжевый
    } else {
        "#ef4444".to_string() // Красный
    }
}
