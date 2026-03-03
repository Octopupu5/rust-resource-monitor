pub fn render_index() -> String {
    format!(
        r#"<!doctype html>
<html>
<head>
  <meta charset="utf-8"/>
  <title>Resource Monitor</title>
  <style>
    {}
  </style>
</head>
<body>
  <h1>Resource Monitor</h1>

  <!-- Stat cards are created dynamically from snapshot data -->
  <div class="stat-grid" id="stat-cards"></div>

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
      <a href="/api/latest">/api/latest</a>
      <span class="label">|</span>
      <a href="/api/history?limit=60">/api/history</a>
      <span class="label">|</span>
      <a href="/api/range?from_ts=0&to_ts=9999999999999">/api/range</a>
      <span class="label">|</span>
      <a href="/api/health">/api/health</a>
      <span class="label">|</span>
      <a href="/api/stream">/api/stream</a>
    </div>
    <div class="controls">
      <span class="label" id="range-label">Last 3 minutes</span>
    </div>
    <div class="controls widget-menu-toggle">
      <button id="widget-menu-btn" type="button">Widgets</button>
      <div id="widget-menu" class="widget-menu"></div>
    </div>
  </div>

  <div style="margin-bottom:16px;">
    <div class="controls" style="gap: 12px; margin-bottom: 8px;">
      <span class="label">Window (minutes)</span>
      <input id="win-slider" type="range" min="1" max="60" value="3" step="1" style="width: 240px;">
      <span class="label" id="win-slider-label">3m</span>

      <span class="label" style="margin-left:16px;">Timeline (end)</span>
      <input id="end-slider" type="range" min="0" max="0" value="0" step="1" style="width: 420px;">
      <span class="label" id="end-slider-label">live</span>
      <button id="live-btn" type="button">Live</button>
    </div>
    <canvas id="timeline" width="1120" height="80" style="width:100%; height:80px; border: none; border-radius: 0;"></canvas>
    <div style="display: flex; justify-content: center; align-items: center; gap: 24px; margin-top: 4px; flex-wrap: wrap;">
      <span style="display: inline-flex; align-items: center; gap: 4px; font-size: 11px; color: #9ca3af; font-family: ui-monospace, monospace;">
        <span style="width: 10px; height: 10px; background: #3b82f6; border-radius: 2px; display: inline-block;"></span> Current view
      </span>
      <span style="display: inline-flex; align-items: center; gap: 4px; font-size: 11px; color: #9ca3af; font-family: ui-monospace, monospace;">
        <span style="width: 10px; height: 10px; background: #ec4899; border-radius: 2px; display: inline-block;"></span> Selection
      </span>
      <span style="font-size: 11px; color: #6b7280; font-family: ui-monospace, monospace;">|</span>
      <span class="label" id="brush-label" style="font-size: 11px;">Drag outside to select · Drag inside to move · Drag edges to resize · Double-click &rarr; live</span>
      <span style="font-size: 11px; color: #6b7280; font-family: ui-monospace, monospace;">|</span>
      <span class="label" style="font-size: 11px;">Hover charts to see exact values</span>
    </div>
  </div>

  <!-- Charts are created dynamically from snapshot data -->
  <div id="charts-container" class="widgets-grid"></div>

  <h3 style="margin-top:20px;">Latest snapshot</h3>
  <pre id="latest">Loading...</pre>
  <div id="tooltip"></div>

  <script>
    {}
  </script>
</body>
</html>"#,
        include_str!("static/styles.css"),
        include_str!("static/widgets.js")
    )
}
