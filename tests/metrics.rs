use resource_monitor::metrics::*;

fn base_snapshot() -> MetricsSnapshot {
    MetricsSnapshot {
        timestamp_ms: 1700000000000,
        cpu: CpuMetrics {
            total_usage_pct: 45.5,
            per_core_usage_pct: vec![30.0, 60.0, 40.0, 50.0],
            load_avg_1: 1.5,
            load_avg_5: 1.2,
            load_avg_15: 0.8,
            temperature_celsius: None,
        },
        memory: MemoryMetrics {
            total_bytes: 16_000_000_000,
            used_bytes: 8_000_000_000,
            available_bytes: 8_000_000_000,
            swap_total_bytes: 4_000_000_000,
            swap_used_bytes: 1_000_000_000,
        },
        network: NetworkMetrics {
            rx_bytes_total: 1_000_000,
            tx_bytes_total: 500_000,
            rx_bytes_per_sec: 50_000.0,
            tx_bytes_per_sec: 10_000.0,
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
fn to_rpc_format_has_base_series() {
    let snap = base_snapshot();
    let rpc = snap.to_rpc_format();

    assert_eq!(rpc.timestamp_ms, snap.timestamp_ms);
    assert_eq!(rpc.data.len(), 7);

    let names: Vec<&str> = rpc.data.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"cpu_total"));
    assert!(names.contains(&"cpu_cores"));
    assert!(names.contains(&"load_avg"));
    assert!(names.contains(&"memory"));
    assert!(names.contains(&"swap"));
    assert!(names.contains(&"network"));
    assert!(names.contains(&"disk"));
}

#[test]
fn to_rpc_format_cpu_values() {
    let snap = base_snapshot();
    let rpc = snap.to_rpc_format();

    let cpu_total = rpc.data.iter().find(|s| s.name == "cpu_total").unwrap();
    assert_eq!(cpu_total.series, vec![45.5]);
    assert_eq!(cpu_total.warn, Some(70.0));
    assert_eq!(cpu_total.crit, Some(90.0));

    let cpu_cores = rpc.data.iter().find(|s| s.name == "cpu_cores").unwrap();
    assert_eq!(cpu_cores.series, vec![30.0, 60.0, 40.0, 50.0]);
    assert_eq!(cpu_cores.legend.len(), 4);
    assert_eq!(cpu_cores.legend[0].name, "C0");
    assert_eq!(cpu_cores.legend[3].name, "C3");
}

#[test]
fn to_rpc_format_memory_percentage() {
    let snap = base_snapshot();
    let rpc = snap.to_rpc_format();

    let mem = rpc.data.iter().find(|s| s.name == "memory").unwrap();
    assert!((mem.series[0] - 50.0).abs() < 0.01);

    let swap = rpc.data.iter().find(|s| s.name == "swap").unwrap();
    assert!((swap.series[0] - 25.0).abs() < 0.01);
}

#[test]
fn to_rpc_format_zero_total_memory() {
    let mut snap = base_snapshot();
    snap.memory.total_bytes = 0;
    snap.memory.swap_total_bytes = 0;
    let rpc = snap.to_rpc_format();

    let mem = rpc.data.iter().find(|s| s.name == "memory").unwrap();
    assert_eq!(mem.series[0], 0.0);

    let swap = rpc.data.iter().find(|s| s.name == "swap").unwrap();
    assert_eq!(swap.series[0], 0.0);
}

#[test]
fn to_rpc_format_network_values() {
    let snap = base_snapshot();
    let rpc = snap.to_rpc_format();

    let net = rpc.data.iter().find(|s| s.name == "network").unwrap();
    assert_eq!(net.series, vec![50_000.0, 10_000.0]);
    assert_eq!(net.legend.len(), 2);
    assert_eq!(net.legend[0].name, "RX");
    assert_eq!(net.legend[1].name, "TX");
}

#[test]
fn to_rpc_format_with_gpu() {
    let mut snap = base_snapshot();
    snap.gpu = Some(GpuMetrics {
        name: "Test GPU".to_string(),
        gpu_utilization_pct: 75.0,
        vram_total_bytes: 8_000_000_000,
        vram_used_bytes: 4_000_000_000,
        temperature_celsius: Some(65.0),
        is_unified_memory: false,
    });
    let rpc = snap.to_rpc_format();

    assert_eq!(rpc.data.len(), 9);

    let gpu_util = rpc.data.iter().find(|s| s.name == "gpu_util").unwrap();
    assert_eq!(gpu_util.series, vec![75.0]);
    assert!(gpu_util.beautiful_name.contains("Test GPU"));
    assert_eq!(gpu_util.legend[0].comment.as_deref(), Some("65 \u{b0}C"));

    let gpu_mem = rpc.data.iter().find(|s| s.name == "gpu_mem").unwrap();
    assert!((gpu_mem.series[0] - 50.0).abs() < 0.01);
    assert!(gpu_mem.beautiful_name.contains("VRAM"));
}

#[test]
fn to_rpc_format_with_unified_gpu() {
    let mut snap = base_snapshot();
    snap.gpu = Some(GpuMetrics {
        name: "Apple M2".to_string(),
        gpu_utilization_pct: 20.0,
        vram_total_bytes: 16_000_000_000,
        vram_used_bytes: 2_000_000_000,
        temperature_celsius: None,
        is_unified_memory: true,
    });
    let rpc = snap.to_rpc_format();

    let gpu_mem = rpc.data.iter().find(|s| s.name == "gpu_mem").unwrap();
    assert!(gpu_mem.beautiful_name.contains("Unified"));
    assert_eq!(gpu_mem.legend[0].name, "Unified");

    let gpu_util = rpc.data.iter().find(|s| s.name == "gpu_util").unwrap();
    assert!(gpu_util.legend[0].comment.is_none());
}

#[test]
fn to_rpc_format_gpu_zero_vram() {
    let mut snap = base_snapshot();
    snap.gpu = Some(GpuMetrics {
        name: "GPU".to_string(),
        gpu_utilization_pct: 10.0,
        vram_total_bytes: 0,
        vram_used_bytes: 0,
        temperature_celsius: None,
        is_unified_memory: false,
    });
    let rpc = snap.to_rpc_format();

    let gpu_mem = rpc.data.iter().find(|s| s.name == "gpu_mem").unwrap();
    assert_eq!(gpu_mem.series[0], 0.0);
}

#[test]
fn to_rpc_format_with_battery() {
    let mut snap = base_snapshot();
    snap.battery = Some(BatteryMetrics {
        percentage: 75.0,
        voltage: 12.6,
        temperature: Some(35.0),
        energy_full: 80,
        energy_now: 60,
        power_now: 15.5,
        time_to_empty: Some(7200),
        time_to_full: None,
        state: "Discharging".to_string(),
    });
    let rpc = snap.to_rpc_format();

    assert_eq!(rpc.data.len(), 9);

    let bat = rpc.data.iter().find(|s| s.name == "battery").unwrap();
    assert_eq!(bat.series, vec![75.0]);
    assert_eq!(bat.legend[0].comment.as_deref(), Some("Discharging"));
    assert_eq!(bat.warn, Some(30.0));
    assert_eq!(bat.crit, Some(10.0));

    let bat_power = rpc.data.iter().find(|s| s.name == "battery_power").unwrap();
    assert_eq!(bat_power.series, vec![15.5]);
}

#[test]
fn to_rpc_format_with_gpu_and_battery() {
    let mut snap = base_snapshot();
    snap.gpu = Some(GpuMetrics {
        name: "GPU".to_string(),
        gpu_utilization_pct: 50.0,
        vram_total_bytes: 4_000_000_000,
        vram_used_bytes: 2_000_000_000,
        temperature_celsius: None,
        is_unified_memory: false,
    });
    snap.battery = Some(BatteryMetrics {
        percentage: 50.0,
        voltage: 12.0,
        temperature: None,
        energy_full: 80,
        energy_now: 40,
        power_now: 10.0,
        time_to_empty: None,
        time_to_full: None,
        state: "Full".to_string(),
    });
    let rpc = snap.to_rpc_format();
    assert_eq!(rpc.data.len(), 11);
}

#[test]
fn now_timestamp_ms_is_reasonable() {
    let ts = now_timestamp_ms();
    assert!(ts > 1_700_000_000_000);
    assert!(ts < 2_000_000_000_000);
}

#[test]
fn snapshot_json_roundtrip() {
    let snap = base_snapshot();
    let json = serde_json::to_string(&snap).unwrap();
    let deserialized: MetricsSnapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.timestamp_ms, snap.timestamp_ms);
    assert_eq!(deserialized.cpu.total_usage_pct, snap.cpu.total_usage_pct);
    assert_eq!(deserialized.memory.total_bytes, snap.memory.total_bytes);
}

#[test]
fn rpc_snapshot_json_roundtrip() {
    let snap = base_snapshot();
    let rpc = snap.to_rpc_format();
    let json = serde_json::to_string(&rpc).unwrap();
    let deserialized: RpcMetricsSnapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.timestamp_ms, rpc.timestamp_ms);
    assert_eq!(deserialized.data.len(), rpc.data.len());
    for (orig, deser) in rpc.data.iter().zip(deserialized.data.iter()) {
        assert_eq!(orig.name, deser.name);
        assert_eq!(orig.series, deser.series);
    }
}

#[test]
fn battery_color_thresholds() {
    assert_eq!(get_battery_color(95.0), "#4ade80");
    assert_eq!(get_battery_color(90.0), "#4ade80");
    assert_eq!(get_battery_color(75.0), "#22c55e");
    assert_eq!(get_battery_color(60.0), "#22c55e");
    assert_eq!(get_battery_color(45.0), "#fbbf24");
    assert_eq!(get_battery_color(30.0), "#fbbf24");
    assert_eq!(get_battery_color(15.0), "#f97316");
    assert_eq!(get_battery_color(10.0), "#f97316");
    assert_eq!(get_battery_color(5.0), "#ef4444");
}

#[test]
fn format_bytes_short_ranges() {
    assert_eq!(format_bytes_short(500), "0 KB");
    assert_eq!(format_bytes_short(1024 * 1024), "1 MB");
    assert_eq!(format_bytes_short(1024 * 1024 * 1024), "1.0 GB");
    assert_eq!(format_bytes_short(8 * 1024 * 1024 * 1024), "8.0 GB");
}

#[test]
fn load_avg_in_rpc() {
    let snap = base_snapshot();
    let rpc = snap.to_rpc_format();
    let la = rpc.data.iter().find(|s| s.name == "load_avg").unwrap();
    assert_eq!(la.series, vec![1.5, 1.2, 0.8]);
    assert_eq!(la.legend.len(), 3);
    assert_eq!(la.legend[0].name, "1m");
    assert_eq!(la.legend[1].name, "5m");
    assert_eq!(la.legend[2].name, "15m");
}

#[test]
fn error_response_serializes() {
    let err = ErrorResponse {
        error: "test error".to_string(),
    };
    let json = serde_json::to_string(&err).unwrap();
    assert!(json.contains("test error"));
    let deser: ErrorResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(deser.error, "test error");
}
