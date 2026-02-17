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
use tokio_util::sync::CancellationToken;
use futures::StreamExt;

#[derive(Clone)]
pub struct AppState {
    pub buffer: Arc<MetricsBuffer>,
    pub stream_tx: broadcast::Sender<MetricsSnapshot>,
    pub shutdown: CancellationToken,
}

#[derive(Deserialize)]
pub struct HistoryQuery {
    pub limit: Option<usize>,
    pub since_ms: Option<u64>,
    pub until_ms: Option<u64>,
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
<head>
  <meta charset="utf-8"/>
  <title>Resource Monitor</title>
  <style>
    :root {
      --bg: #0b0f19;
      --panel: #0f1626;
      --border: #2a3550;
      --grid: #1c2740;
      --text: #e5e7eb;
      --muted: #9ca3af;
      --btn: #111a2d;
      --btn-active: #1b2a4a;
    }
    body { background: var(--bg); color: var(--text); font-family: ui-sans-serif, system-ui, -apple-system, Segoe UI, Roboto, Arial; margin: 24px; }
    a { color: #93c5fd; text-decoration: none; }
    a:hover { text-decoration: underline; }
    .topbar { display: flex; gap: 12px; align-items: center; flex-wrap: wrap; margin-bottom: 16px; }
    .controls { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; }
    .controls .label { color: var(--muted); font-size: 12px; font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace; }
    button { background: var(--btn); color: var(--text); border: 1px solid var(--border); border-radius: 8px; padding: 6px 10px; cursor: pointer; }
    button.active { background: var(--btn-active); border-color: #3b82f6; }
    .panel { background: var(--panel); border: 1px solid var(--border); border-radius: 12px; padding: 12px; }
    canvas { background: var(--panel); border: 1px solid var(--border); border-radius: 10px; }
    .chart { position: relative; }
    /* Overlay canvases must be transparent, otherwise they hide the chart below. */
    .chart canvas.overlay { position: absolute; left: 0; top: 0; pointer-events: none; background: transparent; border: none; }
    #tooltip { position: fixed; z-index: 1000; background: rgba(15, 22, 38, 0.95); border: 1px solid var(--border); border-radius: 10px; padding: 8px 10px; font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace; font-size: 12px; color: var(--text); display: none; max-width: 340px; }
    pre { background: var(--panel); border: 1px solid var(--border); border-radius: 12px; padding: 12px; overflow: auto; }
    h1 { margin: 0 0 8px 0; }
    h3 { margin: 0 0 10px 0; }
  </style>
</head>
<body>
  <h1>Resource Monitor</h1>
  <div class="topbar">
    <div class="controls">
      <span class="label">Time range</span>
      <button data-win="60000">1m</button>
      <button data-win="180000" class="active">3m</button>
      <button data-win="300000">5m</button>
      <button data-win="900000">15m</button>
      <button data-win="3600000">1h</button>
      <button data-win="0">All</button>
    </div>
    <div class="controls">
      <span class="label">Links</span>
      <a href="/api/metrics">/api/metrics</a>
      <span class="label">|</span>
      <a href="/api/history?limit=60">/api/history</a>
      <span class="label">|</span>
      <a href="/api/health">/api/health</a>
      <span class="label">|</span>
      <a href="/api/stream">/api/stream</a>
    </div>
    <div class="controls">
      <span class="label" id="range-label">Last 3 minutes</span>
    </div>
  </div>

  <div class="panel" style="margin-bottom:16px;">
    <div class="controls" style="gap: 12px;">
      <span class="label">Window (minutes)</span>
      <input id="win-slider" type="range" min="1" max="60" value="3" step="1" style="width: 240px;">
      <span class="label" id="win-slider-label">3m</span>

      <span class="label" style="margin-left:16px;">Timeline (end)</span>
      <input id="end-slider" type="range" min="0" max="0" value="0" step="1" style="width: 420px;">
      <span class="label" id="end-slider-label">live</span>
      <button id="live-btn" type="button">Live</button>
    </div>
    <div style="margin-top:10px;">
      <canvas id="timeline" width="1120" height="64" style="width:100%; height:64px;"></canvas>
      <div class="controls" style="justify-content: space-between; margin-top: 8px; width: 100%;">
        <span class="label" id="brush-label">Drag on the timeline to select a time range</span>
        <span class="label">Hover charts to see exact values</span>
      </div>
    </div>
  </div>

  <div style="display:flex; gap:24px; flex-wrap:wrap;">
    <div class="panel">
      <h3>CPU total (%)</h3>
      <div class="chart">
        <canvas id="cpu" width="520" height="180"></canvas>
        <canvas id="cpu-ov" class="overlay" width="520" height="180"></canvas>
      </div>
    </div>
    <div class="panel">
      <h3>Memory used (%)</h3>
      <div class="chart">
        <canvas id="mem" width="520" height="180"></canvas>
        <canvas id="mem-ov" class="overlay" width="520" height="180"></canvas>
      </div>
    </div>
    <div class="panel">
      <h3>Network (B/s)</h3>
      <div class="chart">
        <canvas id="net" width="520" height="180"></canvas>
        <canvas id="net-ov" class="overlay" width="520" height="180"></canvas>
      </div>
      <div style="font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', 'Courier New', monospace; font-size: 12px; margin-top: 6px;">
        <span style="color:#0b6;">RX</span> |
        <span style="color:#06b;">TX</span>
      </div>
    </div>
  </div>

  <h3>Latest snapshot</h3>
  <pre id="latest">Loading...</pre>
  <div id="tooltip"></div>
  <script>
    function clamp(x, lo, hi) {
      if (!Number.isFinite(x)) return lo;
      return Math.max(lo, Math.min(hi, x));
    }

    function drawLineChart(canvas, series, options) {
      const ctx = canvas.getContext('2d');
      const w = canvas.width, h = canvas.height;
      ctx.clearRect(0, 0, w, h);

      // Background
      ctx.fillStyle = '#0f1626';
      ctx.fillRect(0, 0, w, h);

      // Grid
      ctx.strokeStyle = '#1c2740';
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
      // Reserve space for axis labels to avoid overlaps (e.g. minY label vs time labels).
      const leftPad = 54;
      const rightPad = 10;
      const topPad = 10;
      const bottomPad = 24;

      // Save metadata for mouse drag selection (zoom).
      canvas.__meta = { minX, maxX, minY, maxY, w, h, leftPad, rightPad, topPad, bottomPad };

      function xToPx(x) {
        if (maxX === minX) return 0;
        return (x - minX) / (maxX - minX) * (w - leftPad - rightPad) + leftPad;
      }
      function yToPx(y) {
        const t = (y - minY) / (maxY - minY);
        return (1 - clamp(t, 0, 1)) * (h - topPad - bottomPad) + topPad;
      }

      // Axes labels / ticks
      ctx.fillStyle = '#9ca3af';
      ctx.font = '12px ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace';
      ctx.textAlign = 'left';
      ctx.textBaseline = 'middle';

      // Y-axis tick labels (more values than only min/max).
      const yTicks = 5;
      const yDecimals = (Math.abs(maxY - minY) >= 20 || maxY >= 20) ? 0 : 1;
      for (let i = 0; i < yTicks; i++) {
        const v = minY + (maxY - minY) * (i / (yTicks - 1));
        const py = yToPx(v);
        ctx.fillText(String(v.toFixed(yDecimals)), 6, py);
      }

      // Time labels (x-axis) - more tick marks.
      if (Number.isFinite(minX) && Number.isFinite(maxX)) {
        ctx.textBaseline = 'alphabetic';
        const xTicks = Math.min(7, Math.max(3, Math.floor((w - leftPad - rightPad) / 120) + 1));
        for (let i = 0; i < xTicks; i++) {
          const ts = minX + (maxX - minX) * (i / (xTicks - 1));
          const px = xToPx(ts);
          const txt = new Date(ts).toLocaleTimeString();
          const tw = ctx.measureText(txt).width;
          const x = clamp(px - tw / 2, leftPad, w - rightPad - tw);
          ctx.fillText(txt, x, h - 6);
        }
      }

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

    function windowLabel(ms) {
      if (ms === 0) return 'All (buffer)';
      const min = Math.round(ms / 60000);
      return `Last ${min} minute${min === 1 ? '' : 's'}`;
    }

    let windowMs = 180000; // Default: 3 minutes
    let followLive = true;
    // Absolute end timestamp used when paused. This avoids "jumping" when the buffer start changes.
    let pausedEndTs = null;

    // Keep full in-browser buffer; chart view is a slice of it based on sliders.
    let data = { xs: [], cpu: [], mem: [], rx: [], tx: [] };
    const tooltip = document.getElementById('tooltip');
    const overlays = {
      cpu: document.getElementById('cpu-ov'),
      mem: document.getElementById('mem-ov'),
      net: document.getElementById('net-ov'),
    };
    let lastView = null;

    function resetData() {
      data = { xs: [], cpu: [], mem: [], rx: [], tx: [] };
    }

    function pushDataPoint(p) {
      const ts = p.timestamp_ms;
      if (typeof ts !== 'number') return;
      const last = data.xs.length ? data.xs[data.xs.length - 1] : 0;
      if (ts <= last) return;

      data.xs.push(ts);
      data.cpu.push(p.cpu.total_usage_pct);
      const total = (p.memory.total_bytes || 0);
      const used = (p.memory.used_bytes || 0);
      data.mem.push(total === 0 ? 0 : used / total * 100);
      data.rx.push(p.network.rx_bytes_per_sec);
      data.tx.push(p.network.tx_bytes_per_sec);

      // Hard cap to avoid unbounded growth in the browser.
      const maxLen = 20000;
      if (data.xs.length > maxLen) {
        const drop = data.xs.length - maxLen;
        for (const k of ['xs','cpu','mem','rx','tx']) data[k].splice(0, drop);
      }
      updateEndSliderMax();
    }

    function lowerBound(arr, x) {
      let lo = 0, hi = arr.length;
      while (lo < hi) {
        const mid = (lo + hi) >> 1;
        if (arr[mid] < x) lo = mid + 1;
        else hi = mid;
      }
      return lo;
    }

    function currentViewRange() {
      if (data.xs.length === 0) return null;
      const startTs = data.xs[0];
      const endTs = data.xs[data.xs.length - 1];

      const viewEnd = followLive ? endTs : (pausedEndTs ?? endTs);
      const clampedEnd = clamp(viewEnd, startTs, endTs);
      const viewStart = windowMs === 0 ? startTs : Math.max(startTs, clampedEnd - windowMs);
      return { startTs, endTs, viewStart, viewEnd: clampedEnd };
    }

    function viewSeries() {
      const r = currentViewRange();
      if (!r) return null;
      const i0 = lowerBound(data.xs, r.viewStart);
      const i1 = lowerBound(data.xs, r.viewEnd + 1);
      return {
        xs: data.xs.slice(i0, i1),
        cpu: data.cpu.slice(i0, i1),
        mem: data.mem.slice(i0, i1),
        rx: data.rx.slice(i0, i1),
        tx: data.tx.slice(i0, i1),
        range: r,
      };
    }

    function redraw() {
      const s = viewSeries();
      if (!s || s.xs.length === 0) return;
      lastView = s;

      // Clear hover overlays and tooltip on redraw.
      for (const ov of Object.values(overlays)) {
        const ctx = ov.getContext('2d');
        ctx.clearRect(0, 0, ov.width, ov.height);
      }
      tooltip.style.display = 'none';

      drawLineChart(document.getElementById('cpu'), [{ ys: s.cpu, color: '#c44' }], {
        xs: s.xs, minY: 0, maxY: 100
      });
      // Memory line color is intentionally vivid for readability on dark background.
      drawLineChart(document.getElementById('mem'), [{ ys: s.mem, color: '#f59e0b' }], {
        xs: s.xs, minY: 0, maxY: 100
      });
      const maxNet = Math.max(1, ...s.rx, ...s.tx);
      drawLineChart(document.getElementById('net'), [
        { ys: s.rx, color: '#0b6' },
        { ys: s.tx, color: '#06b' },
      ], {
        xs: s.xs, minY: 0, maxY: maxNet * 1.1
      });

      updateRangeLabel(s.range, s.xs.length);
      drawTimeline();
    }

    function fmtTime(ms) {
      const d = new Date(ms);
      return d.toLocaleTimeString();
    }

    function updateRangeLabel(r, points) {
      const label = document.getElementById('range-label');
      if (windowMs === 0) {
        label.textContent = `All (buffer): ${fmtTime(r.startTs)} - ${fmtTime(r.endTs)} | points=${points}`;
      } else {
        label.textContent = `${fmtTime(r.viewStart)} - ${fmtTime(r.viewEnd)} | window=${Math.round(windowMs/60000)}m | points=${points}`;
      }
    }

    function updateEndSliderMax() {
      const endSlider = document.getElementById('end-slider');
      const endLabel = document.getElementById('end-slider-label');
      if (data.xs.length < 2) {
        endSlider.max = 0;
        endSlider.value = 0;
        endLabel.textContent = 'live';
        return;
      }
      const startTs = data.xs[0];
      const endTs = data.xs[data.xs.length - 1];
      const spanSec = Math.max(0, Math.floor((endTs - startTs) / 1000));
      endSlider.max = String(spanSec);
      if (followLive) {
        endSlider.value = String(spanSec);
      } else {
        const targetEnd = pausedEndTs ?? endTs;
        const endSec = clamp(Math.floor((targetEnd - startTs) / 1000), 0, spanSec);
        endSlider.value = String(endSec);
      }
      endLabel.textContent = followLive ? 'live' : `paused (t-${spanSec - Number(endSlider.value || 0)}s)`;
    }

    function tsFromCanvasX(canvas, xPx) {
      const m = canvas.__meta;
      if (!m) return null;
      const { minX, maxX, w } = m;
      const leftPad = m.leftPad ?? m.pad;
      const rightPad = m.rightPad ?? m.pad;
      if (maxX === minX) return null;
      const x = clamp(xPx, leftPad, w - rightPad);
      const t = (x - leftPad) / (w - leftPad - rightPad);
      return minX + t * (maxX - minX);
    }

    function xToPxFromMeta(m, x) {
      if (!m || m.maxX === m.minX) return m ? (m.leftPad ?? m.pad) : 0;
      const leftPad = m.leftPad ?? m.pad;
      const rightPad = m.rightPad ?? m.pad;
      return (x - m.minX) / (m.maxX - m.minX) * (m.w - leftPad - rightPad) + leftPad;
    }

    function yToPxFromMeta(m, y) {
      const topPad = m.topPad ?? m.pad;
      const bottomPad = m.bottomPad ?? m.pad;
      const t = (y - m.minY) / (m.maxY - m.minY);
      return (1 - clamp(t, 0, 1)) * (m.h - topPad - bottomPad) + topPad;
    }

    function resizeCanvasToDisplaySize(canvas) {
      const dpr = window.devicePixelRatio || 1;
      const rect = canvas.getBoundingClientRect();
      const w = Math.max(1, Math.floor(rect.width * dpr));
      const h = Math.max(1, Math.floor(rect.height * dpr));
      if (canvas.width !== w || canvas.height !== h) {
        canvas.width = w;
        canvas.height = h;
      }
      return { dpr, rect };
    }

    function drawTimeline() {
      const tl = document.getElementById('timeline');
      if (!tl) return;
      const { dpr } = resizeCanvasToDisplaySize(tl);
      const ctx = tl.getContext('2d');
      const w = tl.width, h = tl.height;
      ctx.clearRect(0, 0, w, h);

      // Background.
      ctx.fillStyle = '#0f1626';
      ctx.fillRect(0, 0, w, h);

      if (data.xs.length < 2) {
        document.getElementById('brush-label').textContent = 'Waiting for data...';
        return;
      }

      const minX = data.xs[0];
      const maxX = data.xs[data.xs.length - 1];
      const pad = Math.round(6 * dpr);
      const minY = 0;
      const maxY = 100;
      tl.__meta = { minX, maxX, minY, maxY, w, h, pad };

      // Grid.
      ctx.strokeStyle = '#1c2740';
      ctx.lineWidth = 1;
      for (let i = 0; i <= 4; i++) {
        const y = (h * i) / 4;
        ctx.beginPath();
        ctx.moveTo(0, y);
        ctx.lineTo(w, y);
        ctx.stroke();
      }

      // Mini CPU line as context.
      ctx.strokeStyle = 'rgba(196, 68, 68, 0.8)';
      ctx.lineWidth = 1;
      const n = data.xs.length;
      const step = Math.max(1, Math.floor(n / 600));
      ctx.beginPath();
      for (let i = 0; i < n; i += step) {
        const px = xToPxFromMeta(tl.__meta, data.xs[i]);
        const py = yToPxFromMeta(tl.__meta, data.cpu[i] || 0);
        if (i === 0) ctx.moveTo(px, py);
        else ctx.lineTo(px, py);
      }
      ctx.stroke();

      const r = currentViewRange();
      if (!r) return;
      const x0 = xToPxFromMeta(tl.__meta, r.viewStart);
      const x1 = xToPxFromMeta(tl.__meta, r.viewEnd);

      // Selection brush.
      ctx.save();
      ctx.fillStyle = 'rgba(59, 130, 246, 0.20)';
      ctx.strokeStyle = 'rgba(59, 130, 246, 0.85)';
      ctx.lineWidth = Math.max(1, Math.round(1 * dpr));
      ctx.fillRect(x0, 0, Math.max(1, x1 - x0), h);
      ctx.beginPath();
      ctx.moveTo(x0, 0); ctx.lineTo(x0, h);
      ctx.moveTo(x1, 0); ctx.lineTo(x1, h);
      ctx.stroke();
      // Handles.
      ctx.fillStyle = 'rgba(59, 130, 246, 0.85)';
      ctx.fillRect(x0 - 2 * dpr, 0, 4 * dpr, h);
      ctx.fillRect(x1 - 2 * dpr, 0, 4 * dpr, h);
      ctx.restore();

      // Time tick marks (bottom).
      ctx.save();
      ctx.fillStyle = '#9ca3af';
      ctx.font = `${Math.max(10, Math.floor(11 * dpr))}px ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace`;
      const ticks = 6;
      for (let i = 0; i < ticks; i++) {
        const ts = minX + (maxX - minX) * (i / (ticks - 1));
        const x = xToPxFromMeta(tl.__meta, ts);
        ctx.strokeStyle = 'rgba(28, 39, 64, 0.8)';
        ctx.beginPath();
        ctx.moveTo(x, h - Math.round(14 * dpr));
        ctx.lineTo(x, h);
        ctx.stroke();
        const txt = fmtTime(ts);
        const tw = ctx.measureText(txt).width;
        ctx.fillText(txt, clamp(x - tw / 2, pad, w - pad - tw), h - Math.round(2 * dpr));
      }
      ctx.restore();

      // Boundary labels.
      ctx.save();
      ctx.fillStyle = '#e5e7eb';
      ctx.font = `${Math.max(10, Math.floor(11 * dpr))}px ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace`;
      ctx.fillText(fmtTime(r.viewStart), clamp(x0 + 4 * dpr, pad, w - pad), Math.round(14 * dpr));
      const endTxt = fmtTime(r.viewEnd);
      const endTw = ctx.measureText(endTxt).width;
      ctx.fillText(endTxt, clamp(x1 - endTw - 4 * dpr, pad, w - pad - endTw), Math.round(28 * dpr));
      ctx.restore();

      const isLive = followLive;
      document.getElementById('brush-label').textContent =
        `${fmtTime(r.viewStart)} - ${fmtTime(r.viewEnd)}${isLive ? ' (live)' : ''}`;
    }

    function installTimelineBrush() {
      const tl = document.getElementById('timeline');
      if (!tl) return;
      let dragging = false;
      let mode = 'new'; // 'new' | 'move'
      let startX = 0;
      let curX = 0;
      let moveOffset = 0;
      let selWidth = 0;

      function pxFromEvent(e) {
        const rect = tl.getBoundingClientRect();
        const dpr = window.devicePixelRatio || 1;
        return (e.clientX - rect.left) * dpr;
      }

      function selectionPx() {
        const r = currentViewRange();
        const m = tl.__meta;
        if (!r || !m) return null;
        const x0 = xToPxFromMeta(m, r.viewStart);
        const x1 = xToPxFromMeta(m, r.viewEnd);
        return { x0, x1 };
      }

      function redrawWithOverlay() {
        drawTimeline();
        if (!dragging) return;
        const ctx = tl.getContext('2d');
        const x0 = mode === 'move' ? (curX - moveOffset) : Math.min(startX, curX);
        const x1 = mode === 'move' ? (x0 + selWidth) : Math.max(startX, curX);
        ctx.save();
        ctx.fillStyle = 'rgba(147, 197, 253, 0.10)';
        ctx.strokeStyle = 'rgba(147, 197, 253, 0.85)';
        ctx.lineWidth = 1;
        ctx.fillRect(x0, 0, Math.max(1, x1 - x0), tl.height);
        ctx.beginPath();
        ctx.moveTo(x0, 0); ctx.lineTo(x0, tl.height);
        ctx.moveTo(x1, 0); ctx.lineTo(x1, tl.height);
        ctx.stroke();
        ctx.restore();
      }

      tl.addEventListener('mousedown', (e) => {
        if (!tl.__meta) return;
        dragging = true;
        const x = pxFromEvent(e);
        const sel = selectionPx();
        if (sel && x >= sel.x0 && x <= sel.x1) {
          mode = 'move';
          moveOffset = x - sel.x0;
          selWidth = sel.x1 - sel.x0;
          curX = x;
        } else {
          mode = 'new';
          startX = x;
          curX = x;
        }
        redrawWithOverlay();
      });

      window.addEventListener('mousemove', (e) => {
        if (!dragging) return;
        curX = pxFromEvent(e);
        redrawWithOverlay();
      });

      window.addEventListener('mouseup', async () => {
        if (!dragging) return;
        dragging = false;
        const m = tl.__meta;
        if (!m || data.xs.length < 2) {
          drawTimeline();
          return;
        }

        let x0;
        let x1;
        if (mode === 'move') {
          x0 = curX - moveOffset;
          x1 = x0 + selWidth;
        } else {
          x0 = Math.min(startX, curX);
          x1 = Math.max(startX, curX);
        }

        // Minimal width.
        if (Math.abs(x1 - x0) < 6) {
          drawTimeline();
          return;
        }

        const t0 = tsFromCanvasX(tl, x0);
        const t1 = tsFromCanvasX(tl, x1);
        if (t0 === null || t1 === null) {
          drawTimeline();
          return;
        }

        const viewStart = Math.min(t0, t1);
        const viewEnd = Math.max(t0, t1);
        const selectedMs = Math.max(1000, Math.floor(viewEnd - viewStart));
        windowMs = selectedMs;
        followLive = false;
        pausedEndTs = Math.floor(viewEnd);

        // Update window slider label (slider itself is minutes, but we keep exact ms).
        const winSlider = document.getElementById('win-slider');
        const winLabel = document.getElementById('win-slider-label');
        const minApprox = clamp(Math.round(selectedMs / 60000), 1, 60);
        winSlider.value = String(minApprox);
        winLabel.textContent = `${minApprox}m*`;

        // Set end slider to selection end (pause live).
        const startTs = data.xs[0];
        const spanSec = Math.max(0, Math.floor((data.xs[data.xs.length - 1] - startTs) / 1000));
        const endSec = clamp(Math.floor((viewEnd - startTs) / 1000), 0, spanSec);
        const endSlider = document.getElementById('end-slider');
        endSlider.value = String(endSec);
        updateEndSliderMax();

        // Refetch to make sure history around the selected range exists locally.
        const margin = 10_000;
        const qs = `?since_ms=${Math.max(0, Math.floor(viewStart - margin))}&until_ms=${Math.floor(viewEnd + margin)}&limit=50000`;
        const res = await fetch('/api/history' + qs);
        if (res.ok) {
          const hist = await res.json();
          resetData();
          if (Array.isArray(hist) && hist.length > 0) for (const p of hist) pushDataPoint(p);
          updateEndSliderMax();
        }
        redraw();
      });
    }

    function installHoverTooltip(baseCanvas, overlayCanvas, seriesSpec) {
      function clear() {
        const ctx = overlayCanvas.getContext('2d');
        ctx.clearRect(0, 0, overlayCanvas.width, overlayCanvas.height);
        tooltip.style.display = 'none';
      }

      baseCanvas.addEventListener('mouseleave', clear);
      baseCanvas.addEventListener('mousemove', (e) => {
        if (!lastView || !baseCanvas.__meta || lastView.xs.length === 0) return;
        const rect = baseCanvas.getBoundingClientRect();
        const x = e.clientX - rect.left;
        const ts = tsFromCanvasX(baseCanvas, x);
        if (ts === null) return;
        const xs = lastView.xs;
        let i = lowerBound(xs, ts);
        if (i >= xs.length) i = xs.length - 1;
        if (i > 0) {
          const prev = xs[i - 1];
          const cur = xs[i];
          if (Math.abs(ts - prev) < Math.abs(cur - ts)) i = i - 1;
        }

        const meta = baseCanvas.__meta;
        const xPx = xToPxFromMeta(meta, xs[i]);

        // Draw overlay (crosshair + points).
        const ctx = overlayCanvas.getContext('2d');
        ctx.clearRect(0, 0, overlayCanvas.width, overlayCanvas.height);
        ctx.save();
        ctx.strokeStyle = 'rgba(156, 163, 175, 0.55)';
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.moveTo(xPx, 0);
        ctx.lineTo(xPx, overlayCanvas.height);
        ctx.stroke();

        const rows = [];
        rows.push(`<div style="color:#9ca3af;">${fmtTime(xs[i])}</div>`);

        for (const spec of seriesSpec) {
          const v = spec.value(i);
          const yPx = yToPxFromMeta(meta, v);
          ctx.fillStyle = spec.color;
          ctx.beginPath();
          ctx.arc(xPx, yPx, 3, 0, Math.PI * 2);
          ctx.fill();
          rows.push(`<div><span style="color:${spec.color};">${spec.label}</span>: ${spec.fmt(v)}</div>`);
        }
        ctx.restore();

        // Tooltip.
        tooltip.innerHTML = rows.join('');
        tooltip.style.display = 'block';
        const pad = 12;
        const tw = tooltip.offsetWidth;
        const th = tooltip.offsetHeight;
        let left = e.clientX + pad;
        let top = e.clientY + pad;
        if (left + tw > window.innerWidth - 8) left = e.clientX - tw - pad;
        if (top + th > window.innerHeight - 8) top = e.clientY - th - pad;
        tooltip.style.left = `${Math.max(8, left)}px`;
        tooltip.style.top = `${Math.max(8, top)}px`;
      });
    }

    function installDragZoom(canvas) {
      let dragging = false;
      let startX = 0;
      let curX = 0;

      function drawOverlay() {
        if (!dragging) return;
        const ctx = canvas.getContext('2d');
        const x0 = Math.min(startX, curX);
        const x1 = Math.max(startX, curX);
        ctx.save();
        ctx.fillStyle = 'rgba(59, 130, 246, 0.18)';
        ctx.strokeStyle = 'rgba(59, 130, 246, 0.7)';
        ctx.lineWidth = 1;
        ctx.fillRect(x0, 0, Math.max(1, x1 - x0), canvas.height);
        ctx.beginPath();
        ctx.moveTo(x0, 0); ctx.lineTo(x0, canvas.height);
        ctx.moveTo(x1, 0); ctx.lineTo(x1, canvas.height);
        ctx.stroke();
        ctx.restore();
      }

      function redrawWithOverlay() {
        redraw();
        drawOverlay();
      }

      canvas.addEventListener('mousedown', (e) => {
        if (!canvas.__meta) return;
        dragging = true;
        const r = canvas.getBoundingClientRect();
        startX = e.clientX - r.left;
        curX = startX;
        redrawWithOverlay();
      });

      window.addEventListener('mousemove', (e) => {
        if (!dragging) return;
        const r = canvas.getBoundingClientRect();
        curX = e.clientX - r.left;
        redrawWithOverlay();
      });

      window.addEventListener('mouseup', async () => {
        if (!dragging) return;
        dragging = false;

        const x0 = Math.min(startX, curX);
        const x1 = Math.max(startX, curX);
        // Minimal selection width.
        if (Math.abs(x1 - x0) < 4) {
          redraw();
          return;
        }

        const t0 = tsFromCanvasX(canvas, x0);
        const t1 = tsFromCanvasX(canvas, x1);
        if (t0 === null || t1 === null) {
          redraw();
          return;
        }

        const viewStart = Math.min(t0, t1);
        const viewEnd = Math.max(t0, t1);
        const selectedMs = Math.max(1000, Math.floor(viewEnd - viewStart));

        // Ensure we have enough history locally for a smooth scrub/zoom.
        windowMs = selectedMs;
        followLive = false;
        pausedEndTs = Math.floor(viewEnd);

        // Update window slider label (slider itself is minutes, but we keep exact ms).
        const winSlider = document.getElementById('win-slider');
        const winLabel = document.getElementById('win-slider-label');
        const minApprox = clamp(Math.round(selectedMs / 60000), 1, 60);
        winSlider.value = String(minApprox);
        winLabel.textContent = `${minApprox}m*`;

        // Move end slider to the selected end (pause live).
        if (data.xs.length >= 2) {
          const startTs = data.xs[0];
          const spanSec = Math.max(0, Math.floor((data.xs[data.xs.length - 1] - startTs) / 1000));
          const endSec = clamp(Math.floor((viewEnd - startTs) / 1000), 0, spanSec);
          const endSlider = document.getElementById('end-slider');
          endSlider.value = String(endSec);
          updateEndSliderMax();
        }

        // Pull more history if selection refers to older data than we currently keep.
        await refetchForCurrentView();
        redraw();
      });

      canvas.addEventListener('dblclick', async () => {
        // Reset to a sensible default: live + 3 minutes.
        windowMs = 180000;
        followLive = true;
        pausedEndTs = null;
        const winSlider = document.getElementById('win-slider');
        const winLabel = document.getElementById('win-slider-label');
        winSlider.value = '3';
        winLabel.textContent = '3m';
        await refetchForCurrentView();
        redraw();
      });
    }

    async function refetchForCurrentView() {
      try {
        // Add a small margin to reduce refetches when scrubbing.
        const marginMs = 10_000;
        let since = null;
        let until = null;
        if (windowMs !== 0) {
          const end = followLive ? Date.now() : (pausedEndTs ?? Date.now());
          since = Math.max(0, Math.floor(end - windowMs - marginMs));
          until = Math.max(0, Math.floor(end + marginMs));
        }
        const qs = since === null
          ? '?limit=10000'
          : `?since_ms=${since}&until_ms=${until}&limit=10000`;
        const resHist = await fetch('/api/history' + qs);
        if (!resHist.ok) throw new Error('HTTP ' + resHist.status);
        const hist = await resHist.json();
        resetData();
        if (Array.isArray(hist) && hist.length > 0) for (const p of hist) pushDataPoint(p);
        updateEndSliderMax();
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
          pushDataPoint(p);
          redraw();
        } catch (e) {
          // Ignore malformed events.
        }
      };
      es.onerror = () => {
        // Fall back: keep trying, browser will reconnect automatically.
      };
    }

    function initWindowControls() {
      const buttons = Array.from(document.querySelectorAll('button[data-win]'));
      function applyActive(ms) {
        for (const b of buttons) b.classList.toggle('active', Number(b.dataset.win) === ms);
        document.getElementById('range-label').textContent = windowLabel(ms);
      }
      for (const b of buttons) {
        b.addEventListener('click', async () => {
          const ms = Number(b.dataset.win);
          if (!Number.isFinite(ms)) return;
          windowMs = ms;
          applyActive(ms);
          // Sync slider for window size.
          const winSlider = document.getElementById('win-slider');
          const winLabel = document.getElementById('win-slider-label');
          if (ms === 0) {
            winSlider.value = '60';
            winLabel.textContent = 'All';
          } else {
            const min = Math.max(1, Math.round(ms / 60000));
            winSlider.value = String(min);
            winLabel.textContent = `${min}m`;
          }
          await refetchForCurrentView();
        });
      }
      applyActive(windowMs);
    }

    function initSliders() {
      const winSlider = document.getElementById('win-slider');
      const winLabel = document.getElementById('win-slider-label');
      winSlider.addEventListener('input', () => {
        const min = Number(winSlider.value);
        windowMs = min * 60000;
        winLabel.textContent = `${min}m`;
      });
      winSlider.addEventListener('change', async () => {
        // When slider changes, drop active button highlight unless it matches an existing preset.
        const buttons = Array.from(document.querySelectorAll('button[data-win]'));
        for (const b of buttons) b.classList.remove('active');
        await refetchForCurrentView();
      });

      const endSlider = document.getElementById('end-slider');
      endSlider.addEventListener('input', () => {
        followLive = Number(endSlider.value || 0) === Number(endSlider.max || 0);
        if (!followLive && data.xs.length > 0) {
          const startTs = data.xs[0];
          pausedEndTs = Math.floor(startTs + Number(endSlider.value || 0) * 1000);
        } else if (followLive) {
          pausedEndTs = null;
        }
        updateEndSliderMax();
        redraw();
      });

      const liveBtn = document.getElementById('live-btn');
      liveBtn.addEventListener('click', async () => {
        followLive = true;
        pausedEndTs = null;
        updateEndSliderMax();
        await refetchForCurrentView();
        redraw();
      });
    }

    initWindowControls();
    initSliders();
    refetchForCurrentView();
    startStream();

    // Grafana-like brush on the timeline.
    installTimelineBrush();

    // Hover tooltips on charts.
    installHoverTooltip(document.getElementById('cpu'), document.getElementById('cpu-ov'), [
      { label: 'CPU', color: '#c44', value: (i) => lastView.cpu[i], fmt: (v) => `${v.toFixed(1)}%` },
    ]);
    installHoverTooltip(document.getElementById('mem'), document.getElementById('mem-ov'), [
      { label: 'Memory', color: '#f59e0b', value: (i) => lastView.mem[i], fmt: (v) => `${v.toFixed(1)}%` },
    ]);
    installHoverTooltip(document.getElementById('net'), document.getElementById('net-ov'), [
      { label: 'RX', color: '#0b6', value: (i) => lastView.rx[i], fmt: (v) => `${v.toFixed(0)} B/s` },
      { label: 'TX', color: '#06b', value: (i) => lastView.tx[i], fmt: (v) => `${v.toFixed(0)} B/s` },
    ]);

    // Drag-to-zoom on charts.
    installDragZoom(document.getElementById('cpu'));
    installDragZoom(document.getElementById('mem'));
    installDragZoom(document.getElementById('net'));
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
    let until_ms = query.until_ms;

    let history: Vec<MetricsSnapshot> = state
        .buffer
        .history(None)
        .into_iter()
        .filter(|s| {
            since_ms
                .map(|ts| s.timestamp_ms >= ts as u128)
                .unwrap_or(true)
        })
        .filter(|s| {
            until_ms
                .map(|ts| s.timestamp_ms <= ts as u128)
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
    let shutdown = state.shutdown.clone();
    let stream = BroadcastStream::new(rx)
        .take_until(async move { shutdown.cancelled().await })
        .map(|msg| match msg {
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
