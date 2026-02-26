use crate::web::widgets::WidgetRegistry;

pub fn render_index() -> String {
    let registry = WidgetRegistry::new();

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

  <!-- Stat cards row -->
  <div class="stat-grid">
    {}
  </div>

  <div class="topbar">
    {}
    {}
  </div>

  <div class="panel" style="margin-bottom:16px;">
    <div class="controls" style="gap: 12px;">
      {}
    </div>
    <div style="margin-top:10px;">
      <canvas id="timeline" width="1120" height="64" style="width:100%; height:64px;"></canvas>
      <div class="controls" style="justify-content: space-between; margin-top: 8px; width: 100%;">
        <span class="label" id="brush-label">Drag on the timeline to select a time range</span>
        <span class="label">Hover charts to see exact values</span>
      </div>
    </div>
  </div>

  <!-- Widgets grid -->
  <div class="widgets-grid">
    {}
  </div>

  <h3 style="margin-top:20px;">Latest snapshot</h3>
  <pre id="latest">Loading...</pre>
  <div id="tooltip"></div>
  
  <script>
    {}
  </script>
</body>
</html>"#,
        include_str!("static/styles.css"),
        render_stat_cards(&registry),
        render_time_range_controls(),
        render_links(),
        render_timeline_controls(),
        render_widgets_grid(&registry),
        include_str!("static/widgets.js")
    )
}

fn render_stat_cards(registry: &WidgetRegistry) -> String {
    registry.get_stat_cards().iter()
        .map(|card| {
            format!(
                r#"<div class="stat-card panel"><div class="stat-label">{}</div><div class="stat-val" id="sc-{}">—</div></div>"#,
                card.label, card.id
            )
        })
        .collect::<Vec<_>>()
        .join("")
}

fn render_time_range_controls() -> String {
    r#"<div class="controls">
      <span class="label">Time range</span>
      <button data-win="60000">1m</button>
      <button data-win="180000" class="active">3m</button>
      <button data-win="300000">5m</button>
      <button data-win="900000">15m</button>
      <button data-win="3600000">1h</button>
      <button data-win="0">All</button>
    </div>"#
        .to_string()
}

fn render_links() -> String {
    r#"<div class="controls">
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
    </div>"#
        .to_string()
}

fn render_timeline_controls() -> String {
    r#"<span class="label">Window (minutes)</span>
      <input id="win-slider" type="range" min="1" max="60" value="3" step="1" style="width: 240px;">
      <span class="label" id="win-slider-label">3m</span>

      <span class="label" style="margin-left:16px;">Timeline (end)</span>
      <input id="end-slider" type="range" min="0" max="0" value="0" step="1" style="width: 420px;">
      <span class="label" id="end-slider-label">live</span>
      <button id="live-btn" type="button">Live</button>"#
        .to_string()
}

fn render_widgets_grid(registry: &WidgetRegistry) -> String {
    let widgets = registry.get_widgets();
    let mut rows = Vec::new();
    let mut current_row = Vec::new();
    let mut row_width = 0;

    for widget in widgets {
        if row_width + widget.width > 3 {
            rows.push(render_widget_row(&current_row));
            current_row.clear();
            row_width = 0;
        }
        current_row.push(widget);
        row_width += widget.width;
    }

    if !current_row.is_empty() {
        rows.push(render_widget_row(&current_row));
    }

    rows.join("")
}

fn render_widget_row(widgets: &[&crate::web::widgets::Widget]) -> String {
    let widgets_html = widgets
        .iter()
        .map(|w| w.render_html())
        .collect::<Vec<_>>()
        .join("");

    format!(r#"<div class="widget-row">{}</div>"#, widgets_html)
}
