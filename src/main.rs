fn main() {
    eprintln!("Use `cargo run --bin server` or `cargo run --bin client`.");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  cargo run --bin server -- --rpc-addr 127.0.0.1:50051 --interval-ms 1000 --history 3600 --console");
    eprintln!("  cargo run --bin client -- --rpc-addr 127.0.0.1:50051 --mode web --bind 127.0.0.1 --port 8080 --history 3600");
    std::process::exit(2);
}
