use clap::{Parser, ValueEnum};
use std::net::IpAddr;
use std::net::SocketAddr;
use std::time::Duration;

#[derive(Clone, Debug, ValueEnum)]
pub enum Mode {
    Console,
    Web,
    Both,
}

#[derive(Clone, Debug, ValueEnum)]
pub enum RpcMode {
    None,
    Server,
    Client,
}

#[derive(Clone, Debug, Parser)]
#[command(
    name = "resource_monitor",
    about = "Lightweight system resource monitor"
)]
pub struct Config {
    /// Polling interval in milliseconds
    #[arg(long, default_value_t = 1000)]
    pub interval_ms: u64,

    /// Output mode (console/web/both)
    #[arg(long, value_enum, default_value_t = Mode::Web)]
    pub mode: Mode,

    /// Bind address for HTTP server
    #[arg(long, default_value = "127.0.0.1")]
    pub bind: IpAddr,

    /// HTTP server port
    #[arg(long, default_value_t = 8080)]
    pub port: u16,

    /// RPC mode (none/server/client)
    #[arg(long, value_enum, default_value_t = RpcMode::None)]
    pub rpc: RpcMode,

    /// RPC address (server bind or client target)
    #[arg(long, default_value = "127.0.0.1:50051")]
    pub rpc_addr: SocketAddr,

    /// History depth (number of snapshots kept in memory)
    #[arg(long, default_value_t = 3600)]
    pub history: usize,
}

impl Config {
    pub fn interval(&self) -> Duration {
        Duration::from_millis(self.interval_ms)
    }

    pub fn web_enabled(&self) -> bool {
        matches!(self.mode, Mode::Web | Mode::Both)
    }

    pub fn console_enabled(&self) -> bool {
        matches!(self.mode, Mode::Console | Mode::Both)
    }
}
