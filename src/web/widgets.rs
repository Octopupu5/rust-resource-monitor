#[derive(Clone, Debug, PartialEq)]
pub enum WidgetPosition {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    FullWidth,
}

#[derive(Clone, Debug)]
pub struct Widget {
    pub id: String,
    pub title: String,
    pub width: usize,
    pub position: WidgetPosition,
    pub chart_type: ChartType,
    pub data_series: Vec<DataSeries>,
}

#[derive(Clone, Debug)]
pub enum ChartType {
    Line,
    Area,
    Bar,
    Gauge,
}

#[derive(Clone, Debug)]
pub struct DataSeries {
    pub key: String,
    pub label: String,
    pub color: String,
    pub unit: String,
    pub min: Option<f32>,
    pub max: Option<f32>,
    pub line_width: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct StatCard {
    pub id: String,
    pub label: String,
    pub data_key: String,
    pub format: ValueFormat,
}

#[derive(Clone, Debug)]
pub enum ValueFormat {
    Percentage,
    Bytes,
    BytesPerSecond,
    Float(usize),
    Integer,
}

pub struct WidgetRegistry {
    widgets: Vec<Widget>,
    stat_cards: Vec<StatCard>,
}

impl WidgetRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            widgets: Vec::new(),
            stat_cards: Vec::new(),
        };

        registry.register_cpu_widgets();
        registry.register_memory_widgets();
        registry.register_network_widgets();
        registry.register_disk_widgets();
        registry.register_stat_cards();

        registry
    }

    fn register_cpu_widgets(&mut self) {
        self.widgets.push(Widget {
            id: "cpu".to_string(),
            title: "CPU total (%)".to_string(),
            width: 1,
            position: WidgetPosition::TopLeft,
            chart_type: ChartType::Line,
            data_series: vec![DataSeries {
                key: "cpu.total_usage_pct".to_string(),
                label: "CPU".to_string(),
                color: "#c44".to_string(),
                unit: "%".to_string(),
                min: Some(0.0),
                max: Some(100.0),
                line_width: Some(2),
            }],
        });

        self.widgets.push(Widget {
            id: "load".to_string(),
            title: "CPU Load Average".to_string(),
            width: 1,
            position: WidgetPosition::TopLeft,
            chart_type: ChartType::Line,
            data_series: vec![
                DataSeries {
                    key: "cpu.load_avg_1".to_string(),
                    label: "1m".to_string(),
                    color: "#e879f9".to_string(),
                    unit: "".to_string(),
                    min: Some(0.0),
                    max: None,
                    line_width: Some(2),
                },
                DataSeries {
                    key: "cpu.load_avg_5".to_string(),
                    label: "5m".to_string(),
                    color: "#a78bfa".to_string(),
                    unit: "".to_string(),
                    min: Some(0.0),
                    max: None,
                    line_width: Some(2),
                },
                DataSeries {
                    key: "cpu.load_avg_15".to_string(),
                    label: "15m".to_string(),
                    color: "#60a5fa".to_string(),
                    unit: "".to_string(),
                    min: Some(0.0),
                    max: None,
                    line_width: Some(2),
                },
            ],
        });

        self.widgets.push(Widget {
            id: "cores".to_string(),
            title: "Per-core CPU (%)".to_string(),
            width: 1,
            position: WidgetPosition::TopRight,
            chart_type: ChartType::Line,
            data_series: vec![],
        });
    }

    fn register_memory_widgets(&mut self) {
        self.widgets.push(Widget {
            id: "mem".to_string(),
            title: "Memory used (%)".to_string(),
            width: 1,
            position: WidgetPosition::TopRight,
            chart_type: ChartType::Line,
            data_series: vec![DataSeries {
                key: "memory.used_pct".to_string(),
                label: "Memory".to_string(),
                color: "#f59e0b".to_string(),
                unit: "%".to_string(),
                min: Some(0.0),
                max: Some(100.0),
                line_width: Some(2),
            }],
        });

        self.widgets.push(Widget {
            id: "swap".to_string(),
            title: "Swap used (%)".to_string(),
            width: 1,
            position: WidgetPosition::BottomLeft,
            chart_type: ChartType::Line,
            data_series: vec![DataSeries {
                key: "memory.swap_used_pct".to_string(),
                label: "Swap".to_string(),
                color: "#818cf8".to_string(),
                unit: "%".to_string(),
                min: Some(0.0),
                max: Some(100.0),
                line_width: Some(2),
            }],
        });
    }

    fn register_network_widgets(&mut self) {
        self.widgets.push(Widget {
            id: "net".to_string(),
            title: "Network (B/s)".to_string(),
            width: 1,
            position: WidgetPosition::TopRight,
            chart_type: ChartType::Line,
            data_series: vec![
                DataSeries {
                    key: "network.rx_bytes_per_sec".to_string(),
                    label: "RX".to_string(),
                    color: "#0b6".to_string(),
                    unit: "B/s".to_string(),
                    min: Some(0.0),
                    max: None,
                    line_width: Some(2),
                },
                DataSeries {
                    key: "network.tx_bytes_per_sec".to_string(),
                    label: "TX".to_string(),
                    color: "#06b".to_string(),
                    unit: "B/s".to_string(),
                    min: Some(0.0),
                    max: None,
                    line_width: Some(2),
                },
            ],
        });
    }

    fn register_disk_widgets(&mut self) {
        self.widgets.push(Widget {
            id: "disk".to_string(),
            title: "Disk space used (%)".to_string(),
            width: 1,
            position: WidgetPosition::BottomRight,
            chart_type: ChartType::Line,
            data_series: vec![DataSeries {
                key: "disk.used_pct".to_string(),
                label: "Disk".to_string(),
                color: "#34d399".to_string(),
                unit: "%".to_string(),
                min: Some(0.0),
                max: Some(100.0),
                line_width: Some(2),
            }],
        });
    }

    fn register_stat_cards(&mut self) {
        self.stat_cards = vec![
            StatCard {
                id: "cpu".to_string(),
                label: "CPU".to_string(),
                data_key: "cpu.total_usage_pct".to_string(),
                format: ValueFormat::Percentage,
            },
            StatCard {
                id: "mem".to_string(),
                label: "Memory".to_string(),
                data_key: "memory.used_pct".to_string(),
                format: ValueFormat::Percentage,
            },
            StatCard {
                id: "la1".to_string(),
                label: "Load 1m".to_string(),
                data_key: "cpu.load_avg_1".to_string(),
                format: ValueFormat::Float(2),
            },
            StatCard {
                id: "la5".to_string(),
                label: "Load 5m".to_string(),
                data_key: "cpu.load_avg_5".to_string(),
                format: ValueFormat::Float(2),
            },
            StatCard {
                id: "la15".to_string(),
                label: "Load 15m".to_string(),
                data_key: "cpu.load_avg_15".to_string(),
                format: ValueFormat::Float(2),
            },
            StatCard {
                id: "rx".to_string(),
                label: "Net RX".to_string(),
                data_key: "network.rx_bytes_per_sec".to_string(),
                format: ValueFormat::BytesPerSecond,
            },
            StatCard {
                id: "tx".to_string(),
                label: "Net TX".to_string(),
                data_key: "network.tx_bytes_per_sec".to_string(),
                format: ValueFormat::BytesPerSecond,
            },
            StatCard {
                id: "disk".to_string(),
                label: "Disk used".to_string(),
                data_key: "disk.used_pct".to_string(),
                format: ValueFormat::Percentage,
            },
            StatCard {
                id: "swap".to_string(),
                label: "Swap".to_string(),
                data_key: "memory.swap_used_pct".to_string(),
                format: ValueFormat::Percentage,
            },
        ];
    }

    pub fn get_widgets(&self) -> Vec<&Widget> {
        self.widgets.iter().collect()
    }

    pub fn get_stat_cards(&self) -> &[StatCard] {
        &self.stat_cards
    }

    pub fn get_widget(&self, id: &str) -> Option<&Widget> {
        self.widgets.iter().find(|w| w.id == id)
    }

    pub fn get_widgets_by_position(&self, position: WidgetPosition) -> Vec<&Widget> {
        self.widgets
            .iter()
            .filter(|w| w.position == position)
            .collect()
    }
}

impl Widget {
    pub fn render_html(&self) -> String {
        match self.chart_type {
            ChartType::Line => self.render_line_chart(),
            _ => self.render_line_chart(),
        }
    }

    fn render_line_chart(&self) -> String {
        let legend_extra = if self.id == "cores" {
            r#"<div id="cores-legend" style="font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', 'Courier New', monospace; font-size: 12px; margin-top: 6px; color: var(--muted);"></div>"#
        } else {
            ""
        };

        let legend = if self.data_series.is_empty() && self.id != "cores" {
            String::new()
        } else if self.id != "cores" {
            self.render_legend()
        } else {
            String::new()
        };

        // Убрали flex из class="panel"
        format!(
            r#"<div class="panel">
                <h3>{}</h3>
                <div class="chart">
                    <canvas id="{}" width="520" height="180"></canvas>
                    <canvas id="{}-ov" class="overlay" width="520" height="180"></canvas>
                </div>
                {}{}
            </div>"#,
            self.title, self.id, self.id, legend, legend_extra
        )
    }

    fn render_legend(&self) -> String {
        if self.data_series.is_empty() {
            return String::new();
        }

        let legend_items: Vec<String> = self
            .data_series
            .iter()
            .map(|s| format!(r#"<span style="color:{};">{}</span>"#, s.color, s.label))
            .collect();

        format!(
            r#"<div style="font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono', 'Courier New', monospace; font-size: 12px; margin-top: 6px;">{}</div>"#,
            legend_items.join(" | ")
        )
    }
}

impl Default for WidgetRegistry {
    fn default() -> Self {
        Self::new()
    }
}
