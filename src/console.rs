use crate::metrics::{DisplayFormat, RpcMetricsSnapshot};
use crate::storage::MetricsBuffer;
use crossterm::cursor::MoveTo;
use crossterm::style::{Color, Stylize};
use crossterm::terminal::{Clear, ClearType};
use crossterm::ExecutableCommand;
use std::io::{stdout, Write};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;
use tracing::error;

pub async fn run_console(
    buffer: Arc<MetricsBuffer>,
    interval: Duration,
    cancel: CancellationToken,
) {
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                break;
            }
            _ = ticker.tick() => {
                if let Err(e) = render_once(&buffer) {
                    error!("Console render error: {}", e);
                }
            }
        }
    }
}

fn render_once(buffer: &MetricsBuffer) -> std::io::Result<()> {
    let mut out = stdout();
    out.execute(MoveTo(0, 0))?;
    out.execute(Clear(ClearType::All))?;

    writeln!(out, "Resource Monitor (console)")?;
    writeln!(out, "Press Ctrl+C to exit.")?;
    writeln!(out)?;

    let Some(snap) = buffer.latest() else {
        writeln!(out, "Waiting for first sample...")?;
        out.flush()?;
        return Ok(());
    };

    let cpu_total = snap.cpu.total_usage_pct;
    let cpu_total_colored = color_pct(cpu_total, 50.0, 80.0);

    let mem_total = snap.memory.total_bytes;
    let mem_used = snap.memory.used_bytes;
    let mem_pct = if mem_total == 0 {
        0.0
    } else {
        (mem_used as f64 / mem_total as f64 * 100.0) as f32
    };
    let mem_pct_colored = color_pct(mem_pct, 70.0, 90.0);

    writeln!(
        out,
        "CPU total: {}   Load avg: {:.2} / {:.2} / {:.2}",
        cpu_total_colored, snap.cpu.load_avg_1, snap.cpu.load_avg_5, snap.cpu.load_avg_15
    )?;
    writeln!(
        out,
        "Memory: {} used / {} total ({})",
        format_bytes(mem_used),
        format_bytes(mem_total),
        mem_pct_colored
    )?;
    writeln!(
        out,
        "Network: RX {:.0} B/s  TX {:.0} B/s   (total RX {} / TX {})",
        snap.network.rx_bytes_per_sec,
        snap.network.tx_bytes_per_sec,
        format_bytes(snap.network.rx_bytes_total),
        format_bytes(snap.network.tx_bytes_total)
    )?;

    if let Some(gpu) = &snap.gpu {
        let mem_label = if gpu.is_unified_memory {
            "Unified"
        } else {
            "VRAM"
        };
        let vram_pct = if gpu.vram_total_bytes > 0 {
            gpu.vram_used_bytes as f32 / gpu.vram_total_bytes as f32 * 100.0
        } else {
            0.0
        };
        let gpu_colored = color_pct(gpu.gpu_utilization_pct, 50.0, 80.0);
        let mem_colored = color_pct(vram_pct, 70.0, 90.0);
        let temp_str = gpu
            .temperature_celsius
            .map(|t| format!("  {t:.0}°C"))
            .unwrap_or_default();
        writeln!(
            out,
            "GPU: {} – {} util  {} {}: {} / {}{}",
            gpu.name,
            gpu_colored,
            mem_colored,
            mem_label,
            format_bytes(gpu.vram_used_bytes),
            format_bytes(gpu.vram_total_bytes),
            temp_str
        )?;
    }

    writeln!(out)?;
    writeln!(out, "Per-core CPU usage:")?;
    for (i, pct) in snap.cpu.per_core_usage_pct.iter().enumerate() {
        let colored = color_pct(*pct, 50.0, 80.0);
        writeln!(out, "  Core {:>2}: {}", i, colored)?;
    }

    out.flush()?;
    Ok(())
}

fn color_pct(value: f32, warn: f32, crit: f32) -> String {
    let s = format!("{value:.1}%");
    if value >= crit {
        s.with(Color::Red).to_string()
    } else if value >= warn {
        s.with(Color::Yellow).to_string()
    } else {
        s.with(Color::Green).to_string()
    }
}

fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    const TB: f64 = GB * 1024.0;

    let b = bytes as f64;
    if b >= TB {
        format!("{:.2} TiB", b / TB)
    } else if b >= GB {
        format!("{:.2} GiB", b / GB)
    } else if b >= MB {
        format!("{:.2} MiB", b / MB)
    } else if b >= KB {
        format!("{:.2} KiB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}

/// Console renderer for the client binary, which receives `RpcMetricsSnapshot` via tarpc.
pub async fn run_rpc_console(
    latest: Arc<RwLock<Option<RpcMetricsSnapshot>>>,
    interval: Duration,
    cancel: CancellationToken,
) {
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = ticker.tick() => {
                if let Err(e) = render_rpc_once(&latest) {
                    error!("Console render error: {}", e);
                }
            }
        }
    }
}

fn render_rpc_once(latest: &RwLock<Option<RpcMetricsSnapshot>>) -> std::io::Result<()> {
    let mut out = stdout();
    out.execute(MoveTo(0, 0))?;
    out.execute(Clear(ClearType::All))?;

    writeln!(out, "Resource Monitor (RPC console client)")?;
    writeln!(out, "Press Ctrl+C to exit.")?;
    writeln!(out)?;

    let guard = match latest.read() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    let Some(snap) = guard.as_ref() else {
        writeln!(out, "Waiting for data from server...")?;
        out.flush()?;
        return Ok(());
    };

    for series in &snap.data {
        let values: Vec<String> = series
            .series
            .iter()
            .zip(series.legend.iter())
            .map(|(val, leg)| {
                let formatted = format_value(*val, &series.format);
                let colored = if let Some(warn) = series.warn {
                    if let Some(crit) = series.crit {
                        if crit > warn {
                            color_pct(*val, warn, crit)
                        } else {
                            color_pct_inverted(*val, warn, crit)
                        }
                    } else {
                        formatted.clone()
                    }
                } else {
                    formatted.clone()
                };
                let comment = leg
                    .comment
                    .as_ref()
                    .map(|c| format!(" ({})", c))
                    .unwrap_or_default();
                format!("{}: {}{}", leg.name, colored, comment)
            })
            .collect();

        writeln!(out, "{}: {}", series.beautiful_name, values.join("  "))?;
    }

    out.flush()?;
    Ok(())
}

fn color_pct_inverted(value: f32, warn: f32, crit: f32) -> String {
    let s = format!("{value:.1}%");
    if value <= crit {
        s.with(Color::Red).to_string()
    } else if value <= warn {
        s.with(Color::Yellow).to_string()
    } else {
        s.with(Color::Green).to_string()
    }
}

fn format_value(val: f32, fmt: &DisplayFormat) -> String {
    match fmt {
        DisplayFormat::Percentage { decimals } => format!("{:.prec$}%", val, prec = decimals),
        DisplayFormat::Float { decimals } => format!("{:.prec$}", val, prec = decimals),
        DisplayFormat::Integer => format!("{}", val as i64),
        DisplayFormat::Bytes { suffix } => {
            let b = val as f64;
            const KB: f64 = 1024.0;
            const MB: f64 = KB * 1024.0;
            const GB: f64 = MB * 1024.0;
            if b >= GB {
                format!("{:.2} Gi{}", b / GB, suffix)
            } else if b >= MB {
                format!("{:.2} Mi{}", b / MB, suffix)
            } else if b >= KB {
                format!("{:.2} Ki{}", b / KB, suffix)
            } else {
                format!("{:.0} {}", b, suffix)
            }
        }
    }
}
