# Resource Monitor

### A small system monitor written in Rust.

What it does:
- Collects CPU, memory, disk, network, GPU, battery metrics
- Stores history in SQLite
- Shows live charts in a web UI
- Exposes HTTP endpoints for latest data, history, and time range queries
- Supports RPC for service/console use cases

How it is built:
- Collector: gathers raw metrics on a timer
- In-memory buffer: keeps recent points for fast access
- Database: stores long-term history
- API server: serves JSON + live stream
- Web client: displays charts and lets you move through time

Run:
1) Start server
   ```
   cargo run --bin server -- --rpc-addr 127.0.0.1:50051 --bind 127.0.0.1 --port 9000
   ```

2) Start client
   ```
   cargo run --bin client -- --api-url http://127.0.0.1:9000 --rpc-addr 127.0.0.1:50051 --bind 127.0.0.1 --port 8080
   ```
   
3) Open ``http://127.0.0.1:8080``
