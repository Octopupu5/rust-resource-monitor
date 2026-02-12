use crate::metrics::{ErrorResponse, MetricsSnapshot};
use crate::storage::MetricsBuffer;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

#[derive(Clone)]
pub struct AppState {
    pub buffer: Arc<MetricsBuffer>,
    pub stream_tx: broadcast::Sender<MetricsSnapshot>,
}

#[derive(Deserialize)]
pub struct HistoryQuery {
    pub limit: Option<usize>,
    pub since_ms: Option<u64>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/health", get(health))
        .route("/api/metrics", get(get_latest))
        .route("/api/history", get(get_history))
        .route("/api/stream", get(stream))
        .with_state(state)
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

async fn health() -> impl IntoResponse {
    (StatusCode::OK, Json(HealthResponse { status: "ok" })).into_response()
}

async fn index() -> impl IntoResponse {
    // Minimal page to quickly visualize responses; can be replaced later by full UI.
    Html(
        r#"<!doctype html>
<html>
<head><meta charset="utf-8"/><title>Resource Monitor</title></head>
<body>
  <h1>Resource Monitor</h1>
  <p>
    <a href="/api/metrics">/api/metrics</a> |
    <a href="/api/history?limit=60">/api/history</a> |
    <a href="/api/health">/api/health</a>
  </p>
  <div style="display:flex; gap:24px; flex-wrap:wrap;">
    <div>
      <h3>CPU total (%)</h3>
      <canvas id="cpu" width="520" height="180" style="border:1px solid #ddd;"></canvas>
    </div>
    <div>
      <h3>Memory used (%)</h3>
      <canvas id="mem" width="520" height="180" style="border:1px solid #ddd;"></canvas>
    </div>
    <div>
      <h3>Network (B/s)</h3>
      <canvas id="net" width="520" height="180" style="border:1px solid #ddd;"></canvas>
      <div style="font-family: monospace; font-size: 12px; margin-top: 6px;">
        <span style="color:#0b6;">RX</span> |
        <span style="color:#06b;">TX</span>
      </div>
    </div>
  </div>

  <h3>Latest snapshot</h3>
  <pre id="latest">Loading...</pre>
  <script>
    function clamp(x, lo, hi) { return Math.max(lo, Math.min(hi, x)); }

    function drawLineChart(canvas, series, options) {
      const ctx = canvas.getContext('2d');
      const w = canvas.width, h = canvas.height;
      ctx.clearRect(0, 0, w, h);

      // Background grid
      ctx.strokeStyle = '#f0f0f0';
      ctx.lineWidth = 1;
      for (let i = 0; i <= 10; i++) {
        const y = (h * i) / 10;
        ctx.beginPath();
        ctx.moveTo(0, y);
        ctx.lineTo(w, y);
        ctx.stroke();
      }

      const xs = options.xs;
      const minX = Math.min(...xs);
      const maxX = Math.max(...xs);
      const minY = options.minY;
      const maxY = options.maxY;

      function xToPx(x) {
        if (maxX === minX) return 0;
        return (x - minX) / (maxX - minX) * (w - 10) + 5;
      }
      function yToPx(y) {
        const t = (y - minY) / (maxY - minY);
        return (1 - clamp(t, 0, 1)) * (h - 10) + 5;
      }

      // Axes labels
      ctx.fillStyle = '#666';
      ctx.font = '12px monospace';
      ctx.fillText(String(maxY.toFixed(1)), 6, 14);
      ctx.fillText(String(minY.toFixed(1)), 6, h - 6);

      for (const s of series) {
        ctx.strokeStyle = s.color;
        ctx.lineWidth = 2;
        ctx.beginPath();
        for (let i = 0; i < xs.length; i++) {
          const px = xToPx(xs[i]);
          const py = yToPx(s.ys[i]);
          if (i === 0) ctx.moveTo(px, py);
          else ctx.lineTo(px, py);
        }
        ctx.stroke();
      }
    }

    let series = {
      xs: [],
      cpu: [],
      mem: [],
      rx: [],
      tx: [],
      lastTs: 0,
    };

    function pushPoint(p) {
      const ts = p.timestamp_ms;
      if (typeof ts !== 'number') return;
      if (ts <= series.lastTs) return;
      series.lastTs = ts;

      series.xs.push(ts);
      series.cpu.push(p.cpu.total_usage_pct);
      const total = (p.memory.total_bytes || 0);
      const used = (p.memory.used_bytes || 0);
      series.mem.push(total === 0 ? 0 : used / total * 100);
      series.rx.push(p.network.rx_bytes_per_sec);
      series.tx.push(p.network.tx_bytes_per_sec);

      const maxLen = 120;
      for (const k of ['xs','cpu','mem','rx','tx']) {
        if (series[k].length > maxLen) series[k].splice(0, series[k].length - maxLen);
      }
    }

    function redraw() {
      if (series.xs.length === 0) return;
      drawLineChart(document.getElementById('cpu'), [{ ys: series.cpu, color: '#c44' }], {
        xs: series.xs, minY: 0, maxY: 100
      });
      drawLineChart(document.getElementById('mem'), [{ ys: series.mem, color: '#444' }], {
        xs: series.xs, minY: 0, maxY: 100
      });
      const maxNet = Math.max(1, ...series.rx, ...series.tx);
      drawLineChart(document.getElementById('net'), [
        { ys: series.rx, color: '#0b6' },
        { ys: series.tx, color: '#06b' },
      ], {
        xs: series.xs, minY: 0, maxY: maxNet * 1.1
      });
    }

    async function bootstrapHistory() {
      try {
        const resLatest = await fetch('/api/metrics');
        if (resLatest.status === 404) {
          document.getElementById('latest').textContent = 'Waiting for first sample...';
          return;
        }
        if (!resLatest.ok) throw new Error('HTTP ' + resLatest.status);
        const latest = await resLatest.json();
        document.getElementById('latest').textContent = JSON.stringify(latest, null, 2);

        const resHist = await fetch('/api/history?limit=120');
        if (!resHist.ok) throw new Error('HTTP ' + resHist.status);
        const hist = await resHist.json();
        if (!Array.isArray(hist) || hist.length === 0) return;

        for (const p of hist) pushPoint(p);
        redraw();
      } catch (e) {
        document.getElementById('latest').textContent = 'Error: ' + e;
      }
    }

    function startStream() {
      const es = new EventSource('/api/stream');
      es.onmessage = (ev) => {
        try {
          const p = JSON.parse(ev.data);
          document.getElementById('latest').textContent = JSON.stringify(p, null, 2);
          pushPoint(p);
          redraw();
        } catch (e) {
          // Ignore malformed events.
        }
      };
      es.onerror = () => {
        // Fall back: keep trying, browser will reconnect automatically.
      };
    }

    bootstrapHistory();
    startStream();
  </script>
</body>
</html>"#,
    )
}

async fn get_latest(State(state): State<AppState>) -> impl IntoResponse {
    match state.buffer.latest() {
        Some(snap) => (StatusCode::OK, Json(snap)).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "no data yet".to_string(),
            }),
        )
            .into_response(),
    }
}

async fn get_history(
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<HistoryQuery>,
) -> impl IntoResponse {
    let limit = query.limit;
    let since_ms = query.since_ms;

    let history: Vec<MetricsSnapshot> = state
        .buffer
        .history(None)
        .into_iter()
        .filter(|s| {
            since_ms
                .map(|ts| s.timestamp_ms >= ts as u128)
                .unwrap_or(true)
        })
        .collect();

    let history = if let Some(limit) = limit {
        let len = history.len();
        let take = limit.min(len);
        history.into_iter().skip(len - take).collect()
    } else {
        history
    };

    (StatusCode::OK, Json(history)).into_response()
}

async fn stream(
    State(state): State<AppState>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let rx = state.stream_tx.subscribe();
    let stream = BroadcastStream::new(rx).map(|msg| match msg {
        Ok(snapshot) => match serde_json::to_string(&snapshot) {
            Ok(json) => Ok(Event::default().data(json)),
            Err(e) => Ok(Event::default()
                .event("error")
                .data(format!("serialize_error: {e}"))),
        },
        Err(e) => Ok(Event::default()
            .event("error")
            .data(format!("stream_error: {e}"))),
    });

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(10))
            .text("keep-alive"),
    )
}
