use crate::storage::MetricsBuffer;
use crossterm::cursor::MoveTo;
use crossterm::style::{Color, Stylize};
use crossterm::terminal::{Clear, ClearType};
use crossterm::ExecutableCommand;
use std::io::{stdout, Write};
use std::sync::Arc;
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
