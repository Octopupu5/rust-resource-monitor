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

  <!-- Stat cards будут созданы динамически на основе данных -->
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

  <!-- Графики будут созданы динамически на основе данных из snapshot -->
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
