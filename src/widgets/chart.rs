use crate::channel::{ChannelContext, ChannelEvent, ChannelValue};
use crate::config::WidgetConfig;
use futures::StreamExt;
use maud::{html, Markup, PreEscaped};
use plotters::prelude::*;
use std::sync::Arc;

// ─── Chart colours (dark-theme friendly) ────────────────────────────────────

const SERIES_COLORS: &[RGBColor] = &[
    RGBColor(0, 204, 102),   // green  (--alarm-none)
    RGBColor(100, 180, 255), // blue
    RGBColor(255, 165, 0),   // orange
    RGBColor(200, 100, 255), // purple
    RGBColor(255, 100, 100), // red
    RGBColor(0, 200, 200),   // cyan
];

const CHART_BG: RGBColor = RGBColor(30, 30, 36);
const CHART_GRID: RGBColor = RGBColor(58, 58, 66);
const CHART_TEXT: RGBColor = RGBColor(160, 160, 170);
const CHART_WIDTH: u32 = 640;
const CHART_HEIGHT: u32 = 300;

// ─── Rendering helpers ───────────────────────────────────────────────────────

fn y_range(series: &[(&str, &[f64])]) -> (f64, f64) {
    let (y_min, y_max) = series
        .iter()
        .flat_map(|(_, d)| d.iter())
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| {
            (lo.min(v), hi.max(v))
        });
    let margin = (y_max - y_min).abs() * 0.1 + 0.001;
    (y_min - margin, y_max + margin)
}

fn x_max(series: &[(&str, &[f64])]) -> f64 {
    series.iter().map(|(_, d)| d.len()).max().unwrap_or(1) as f64
}

/// Render multiple named line series with optional axis labels.
fn render_line_chart(series: &[(&str, &[f64])], x_label: &str, y_label: &str) -> String {
    let mut svg_buf = String::new();

    let n = series.len();

    if n != 1 {
        // ── Multi-series: overlaid traces, per-series coloured Y-axis columns ──
        // Each series is normalised to 0..1 for display so different-magnitude
        // PVs are all visible. The left margin holds N coloured Y-axis columns,
        // each showing the actual value scale for that series.
        const Y_COL_W: u32 = 56; // pixels per Y-axis column
        const N_TICKS: usize = 5;
        let left_pad: u32 = Y_COL_W * n.max(1) as u32;
        let top_pad: u32 = 10;
        let right_pad: u32 = 12;
        let x_label_area: u32 = if x_label.is_empty() { 22 } else { 36 };

        {
            let root = SVGBackend::with_string(&mut svg_buf, (CHART_WIDTH, CHART_HEIGHT))
                .into_drawing_area();
            root.fill(&CHART_BG).ok();

            // Per-series actual value ranges (used for axis label annotation)
            let ranges: Vec<(f64, f64)> = series
                .iter()
                .map(|(_, data)| {
                    if data.is_empty() {
                        return (0.0, 1.0);
                    }
                    let lo = data.iter().cloned().fold(f64::INFINITY, f64::min);
                    let hi = data.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                    let m = (hi - lo).abs() * 0.12 + 0.001;
                    (lo - m, hi + m)
                })
                .collect();

            let xm = x_max(series).max(1.0);

            // Chart area occupies the right portion; left_pad pixels reserved for Y columns.
            // DrawingArea::margin(top, bottom, left, right) — all u32.
            let chart_area = root.margin(top_pad, 4u32, left_pad, right_pad);

            let mut chart = ChartBuilder::on(&chart_area)
                .y_label_area_size(0u32)
                .x_label_area_size(x_label_area)
                .build_cartesian_2d(0f64..xm, 0f64..1f64)
                .unwrap();

            {
                let mut mesh = chart.configure_mesh();
                mesh.x_labels(5)
                    .y_labels(0)
                    .light_line_style(CHART_GRID.mix(0.2))
                    .axis_style(CHART_GRID)
                    .label_style(("sans-serif", 10).into_font().color(&CHART_TEXT))
                    .x_label_formatter(&|v| format!("{:.0}", v));
                if !x_label.is_empty() {
                    mesh.x_desc(x_label);
                }
                mesh.draw().ok();
            }

            // Draw each series normalised to 0..1
            for (idx, (_, data)) in series.iter().enumerate() {
                if data.is_empty() {
                    continue;
                }
                let color = SERIES_COLORS[idx % SERIES_COLORS.len()];
                let (lo, hi) = ranges[idx];
                let scale = (hi - lo).max(f64::EPSILON);
                let pts: Vec<(f64, f64)> = data
                    .iter()
                    .enumerate()
                    .map(|(i, &y)| (i as f64, (y - lo) / scale))
                    .collect();
                chart
                    .draw_series(LineSeries::new(pts, color.stroke_width(2)))
                    .ok();
            }

            // Compute plot-area pixel bounds from normalised data coordinates.
            // DrawingArea uses Rc<RefCell> internally so root.draw() is valid
            // while chart is still in scope.
            let (_, y_top) = chart.backend_coord(&(0.0, 1.0_f64));
            let (_, y_bot) = chart.backend_coord(&(0.0, 0.0_f64));

            // Draw per-series Y-axis columns in the left margin.
            // Series 0 is rightmost (closest to the chart), series n-1 is leftmost.
            for (idx, (name, data)) in series.iter().enumerate() {
                let color = SERIES_COLORS[idx % SERIES_COLORS.len()];
                let (lo, hi) = ranges[idx];
                let scale = (hi - lo).max(f64::EPSILON);

                let col_right = (left_pad as i32) - (idx as i32) * (Y_COL_W as i32);
                let col_left = col_right - Y_COL_W as i32;

                // Vertical axis line
                root.draw(&PathElement::new(
                    vec![(col_right - 1, y_top), (col_right - 1, y_bot)],
                    color.stroke_width(1),
                ))
                .ok();

                // Shortened display name — take the penultimate colon-segment
                // e.g. "demo:pressure:sensor" → "pressure"
                let parts: Vec<&str> = name.split(':').collect();
                let disp = if parts.len() >= 2 {
                    parts[parts.len() - 2]
                } else {
                    name
                };
                root.draw(&Text::new(
                    disp.to_string(),
                    (col_left + 2, y_top - 2),
                    ("sans-serif", 9).into_font().color(&color),
                ))
                .ok();

                if data.is_empty() {
                    continue;
                }

                // Tick marks and value labels
                for t in 0..=N_TICKS {
                    let norm = t as f64 / N_TICKS as f64;
                    let actual = lo + norm * scale;
                    let (_, py) = chart.backend_coord(&(0.0, norm));

                    // Short tick mark on the right edge of the column
                    root.draw(&PathElement::new(
                        vec![(col_right - 6, py), (col_right - 1, py)],
                        color.stroke_width(1),
                    ))
                    .ok();

                    // Value label, left-aligned inside column
                    root.draw(&Text::new(
                        format!("{:.1}", actual),
                        (col_left + 2, py - 6),
                        ("sans-serif", 9).into_font().color(&color),
                    ))
                    .ok();
                }
            }

            root.present().ok();
        }
        return svg_buf;
    }

    // ── Single-series: original rendering ───────────────────────────────────
    {
        let root =
            SVGBackend::with_string(&mut svg_buf, (CHART_WIDTH, CHART_HEIGHT)).into_drawing_area();
        root.fill(&CHART_BG).ok();

        let (y_lo, y_hi) = y_range(series);
        let xm = x_max(series);

        let x_label_area: u32 = if x_label.is_empty() { 20 } else { 35 };
        let y_label_area: u32 = if y_label.is_empty() { 45 } else { 60 };

        let mut chart = ChartBuilder::on(&root)
            .margin(8)
            .x_label_area_size(x_label_area)
            .y_label_area_size(y_label_area)
            .build_cartesian_2d(0f64..xm, y_lo..y_hi)
            .unwrap();

        let mut mesh = chart.configure_mesh();
        mesh.x_labels(5)
            .y_labels(5)
            .light_line_style(CHART_GRID.mix(0.3))
            .bold_line_style(CHART_GRID.mix(0.5))
            .axis_style(CHART_GRID)
            .label_style(("sans-serif", 11).into_font().color(&CHART_TEXT))
            .y_label_formatter(&|v| format!("{:.1}", v))
            .x_label_formatter(&|_| String::new());
        if !x_label.is_empty() {
            mesh.x_desc(x_label);
        }
        if !y_label.is_empty() {
            mesh.y_desc(y_label);
        }
        mesh.draw().ok();

        let color = SERIES_COLORS[0];
        let pts: Vec<(f64, f64)> = series[0]
            .1
            .iter()
            .enumerate()
            .map(|(i, &y)| (i as f64, y))
            .collect();
        chart
            .draw_series(LineSeries::new(pts, color.stroke_width(2)))
            .ok();

        root.present().ok();
    }
    svg_buf
}

/// Render a histogram (value distribution) from a data array.
fn render_histogram(data: &[f64], x_label: &str, y_label: &str) -> String {
    if data.is_empty() {
        return render_line_chart(&[], x_label, y_label);
    }
    let mut svg_buf = String::new();
    {
        let root =
            SVGBackend::with_string(&mut svg_buf, (CHART_WIDTH, CHART_HEIGHT)).into_drawing_area();
        root.fill(&CHART_BG).ok();

        let d_min = data.iter().cloned().fold(f64::INFINITY, f64::min);
        let d_max = data.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let range = (d_max - d_min).max(0.001);

        const NUM_BINS: usize = 20;
        let mut bins = vec![0u32; NUM_BINS];
        for &v in data {
            let bin = (((v - d_min) / range) * NUM_BINS as f64) as usize;
            let bin = bin.min(NUM_BINS - 1);
            bins[bin] += 1;
        }
        let max_count = *bins.iter().max().unwrap_or(&1) as f64;

        let x_label_area: u32 = if x_label.is_empty() { 20 } else { 35 };
        let y_label_area: u32 = if y_label.is_empty() { 45 } else { 60 };

        let mut chart = ChartBuilder::on(&root)
            .margin(8)
            .x_label_area_size(x_label_area)
            .y_label_area_size(y_label_area)
            .build_cartesian_2d(
                (d_min..d_max).step(range / NUM_BINS as f64),
                0f64..max_count * 1.1,
            )
            .unwrap();

        let mut mesh = chart.configure_mesh();
        mesh.x_labels(5)
            .y_labels(5)
            .light_line_style(CHART_GRID.mix(0.3))
            .bold_line_style(CHART_GRID.mix(0.5))
            .axis_style(CHART_GRID)
            .label_style(("sans-serif", 11).into_font().color(&CHART_TEXT))
            .y_label_formatter(&|v| format!("{:.0}", v))
            .x_label_formatter(&|v| format!("{:.1}", v));
        if !x_label.is_empty() {
            mesh.x_desc(x_label);
        }
        if !y_label.is_empty() {
            mesh.y_desc(y_label);
        }
        mesh.draw().ok();

        let bar_color = SERIES_COLORS[0].mix(0.75);
        let bin_width = range / NUM_BINS as f64;
        chart
            .draw_series(bins.iter().enumerate().map(|(i, &count)| {
                let x0 = d_min + i as f64 * bin_width;
                let x1 = x0 + bin_width * 0.9;
                Rectangle::new([(x0, 0f64), (x1, count as f64)], bar_color.filled())
            }))
            .ok();

        root.present().ok();
    }
    svg_buf
}

/// Render a scatter plot. Requires two equal-length arrays (x, y).
fn render_scatter(x_data: &[f64], y_data: &[f64], x_label: &str, y_label: &str) -> String {
    if x_data.is_empty() || y_data.is_empty() {
        return render_line_chart(&[], x_label, y_label);
    }
    let n = x_data.len().min(y_data.len());
    let mut svg_buf = String::new();
    {
        let root =
            SVGBackend::with_string(&mut svg_buf, (CHART_WIDTH, CHART_HEIGHT)).into_drawing_area();
        root.fill(&CHART_BG).ok();

        let xmin = x_data[..n].iter().cloned().fold(f64::INFINITY, f64::min);
        let xmax = x_data[..n]
            .iter()
            .cloned()
            .fold(f64::NEG_INFINITY, f64::max);
        let ymin = y_data[..n].iter().cloned().fold(f64::INFINITY, f64::min);
        let ymax = y_data[..n]
            .iter()
            .cloned()
            .fold(f64::NEG_INFINITY, f64::max);

        let xm = (xmax - xmin).abs() * 0.05 + 0.001;
        let ym = (ymax - ymin).abs() * 0.05 + 0.001;

        let x_label_area: u32 = if x_label.is_empty() { 20 } else { 35 };
        let y_label_area: u32 = if y_label.is_empty() { 45 } else { 60 };

        let mut chart = ChartBuilder::on(&root)
            .margin(8)
            .x_label_area_size(x_label_area)
            .y_label_area_size(y_label_area)
            .build_cartesian_2d((xmin - xm)..(xmax + xm), (ymin - ym)..(ymax + ym))
            .unwrap();

        let mut mesh = chart.configure_mesh();
        mesh.x_labels(5)
            .y_labels(5)
            .light_line_style(CHART_GRID.mix(0.3))
            .bold_line_style(CHART_GRID.mix(0.5))
            .axis_style(CHART_GRID)
            .label_style(("sans-serif", 11).into_font().color(&CHART_TEXT))
            .x_label_formatter(&|v| format!("{:.1}", v))
            .y_label_formatter(&|v| format!("{:.1}", v));
        if !x_label.is_empty() {
            mesh.x_desc(x_label);
        }
        if !y_label.is_empty() {
            mesh.y_desc(y_label);
        }
        mesh.draw().ok();

        let dot_color = SERIES_COLORS[1];
        chart
            .draw_series(
                x_data[..n]
                    .iter()
                    .zip(y_data[..n].iter())
                    .map(|(&x, &y)| Circle::new((x, y), 3, dot_color.filled())),
            )
            .ok();

        root.present().ok();
    }
    svg_buf
}

/// Render a scatter+histogram: scatter in large top area, x-histogram at bottom.
/// Expects x_data and y_data.
fn render_scatter_histogram(
    x_data: &[f64],
    y_data: &[f64],
    x_label: &str,
    y_label: &str,
) -> String {
    if x_data.is_empty() || y_data.is_empty() {
        return render_line_chart(&[], x_label, y_label);
    }
    let n = x_data.len().min(y_data.len());

    // Render scatter (top 65%) and x-histogram (bottom 35%) as separate SVG areas.
    const W: u32 = CHART_WIDTH;
    const H: u32 = CHART_HEIGHT + 160; // taller for the split layout (scatter + histogram row)

    let mut svg_buf = String::new();
    {
        let root = SVGBackend::with_string(&mut svg_buf, (W, H)).into_drawing_area();
        root.fill(&CHART_BG).ok();

        let scatter_h = (H as f64 * 0.62) as u32;
        let hist_h = H - scatter_h - 6;

        let (scatter_area, rest) = root.split_vertically(scatter_h);
        let (_, hist_area) = rest.split_vertically(6); // 6px gap

        // --- scatter ---
        let xmin = x_data[..n].iter().cloned().fold(f64::INFINITY, f64::min);
        let xmax = x_data[..n]
            .iter()
            .cloned()
            .fold(f64::NEG_INFINITY, f64::max);
        let ymin = y_data[..n].iter().cloned().fold(f64::INFINITY, f64::min);
        let ymax = y_data[..n]
            .iter()
            .cloned()
            .fold(f64::NEG_INFINITY, f64::max);
        let xm = (xmax - xmin).abs() * 0.05 + 0.001;
        let ym = (ymax - ymin).abs() * 0.05 + 0.001;

        let y_label_area: u32 = if y_label.is_empty() { 45 } else { 60 };

        let mut sc = ChartBuilder::on(&scatter_area)
            .margin(6)
            .x_label_area_size(0)
            .y_label_area_size(y_label_area)
            .build_cartesian_2d((xmin - xm)..(xmax + xm), (ymin - ym)..(ymax + ym))
            .unwrap();
        let mut mesh = sc.configure_mesh();
        mesh.x_labels(0)
            .y_labels(4)
            .light_line_style(CHART_GRID.mix(0.3))
            .bold_line_style(CHART_GRID.mix(0.5))
            .axis_style(CHART_GRID)
            .label_style(("sans-serif", 11).into_font().color(&CHART_TEXT))
            .y_label_formatter(&|v| format!("{:.1}", v));
        if !y_label.is_empty() {
            mesh.y_desc(y_label);
        }
        mesh.draw().ok();
        sc.draw_series(
            x_data[..n]
                .iter()
                .zip(y_data[..n].iter())
                .map(|(&x, &y)| Circle::new((x, y), 3, SERIES_COLORS[1].filled())),
        )
        .ok();

        // --- x-histogram ---
        let range = (xmax - xmin).max(0.001);
        const NUM_BINS: usize = 20;
        let mut bins = vec![0u32; NUM_BINS];
        for &v in &x_data[..n] {
            let bin = (((v - xmin) / range) * NUM_BINS as f64) as usize;
            bins[bin.min(NUM_BINS - 1)] += 1;
        }
        let max_count = *bins.iter().max().unwrap_or(&1) as f64;

        let x_label_area: u32 = if x_label.is_empty() { 20 } else { 35 };

        let _ = hist_h; // used implicitly by split_vertically
        let mut hc = ChartBuilder::on(&hist_area)
            .margin_left(y_label_area as i32)
            .margin_right(6)
            .margin_bottom(6)
            .x_label_area_size(x_label_area)
            .y_label_area_size(0)
            .build_cartesian_2d(
                (xmin..xmax).step(range / NUM_BINS as f64),
                0f64..max_count * 1.1,
            )
            .unwrap();
        let mut mesh = hc.configure_mesh();
        mesh.x_labels(5)
            .y_labels(0)
            .light_line_style(CHART_GRID.mix(0.3))
            .axis_style(CHART_GRID)
            .label_style(("sans-serif", 11).into_font().color(&CHART_TEXT))
            .x_label_formatter(&|v| format!("{:.1}", v));
        if !x_label.is_empty() {
            mesh.x_desc(x_label);
        }
        mesh.draw().ok();

        let bin_width = range / NUM_BINS as f64;
        hc.draw_series(bins.iter().enumerate().map(|(i, &count)| {
            let x0 = xmin + i as f64 * bin_width;
            let x1 = x0 + bin_width * 0.9;
            Rectangle::new(
                [(x0, 0f64), (x1, count as f64)],
                SERIES_COLORS[0].mix(0.75).filled(),
            )
        }))
        .ok();

        root.present().ok();
    }
    svg_buf
}

// ─── Widget struct ──────────────────────────────────────────────────────────

// ─── Widget struct ──────────────────────────────────────────────────────────

pub struct Chart {
    config: WidgetConfig,
}

impl Chart {
    pub fn new(config: WidgetConfig) -> Self {
        Self { config }
    }

    pub fn into_sse_stream(
        self,
        ctx: Arc<ChannelContext>,
    ) -> impl tokio_stream::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>
           + Send
           + 'static {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let config = Arc::new(self.config);

        tokio::spawn(Self::run_monitor_async(config.clone(), ctx, tx));

        async_stream::stream! {
            yield Ok(axum::response::sse::Event::default().data(
                render_inner_disconnected(&config).into_string()
            ));
            let mut rx = rx;
            while let Some(html) = rx.recv().await {
                yield Ok(axum::response::sse::Event::default().data(html));
            }
        }
    }

    pub(crate) async fn run_monitor_async(
        config: Arc<WidgetConfig>,
        ctx: Arc<ChannelContext>,
        tx: tokio::sync::mpsc::UnboundedSender<String>,
    ) {
        let mut stream = crate::channel::channel_stream(config.clone(), ctx);
        while let Some(event) = stream.next().await {
            let html = match event {
                ChannelEvent::Value(cv) => render_inner_connected(&config, &cv).into_string(),
                ChannelEvent::Disconnected(_) | ChannelEvent::Error(_) => {
                    render_inner_disconnected(&config).into_string()
                }
                ChannelEvent::Connected => continue,
            };
            if tx.send(html).is_err() {
                break;
            }
        }
    }
}

// ─── Tooltip helpers ─────────────────────────────────────────────────────────

/// Build a chart-specific tooltip that lists every PV and which axis/series it maps to.
fn build_chart_tooltip(config: &WidgetConfig, raw: &ChannelValue) -> String {
    use crate::config::ProtocolConfig;
    let chart_type = config.chart_type.as_deref().unwrap_or("line");
    let x_label = config.axis_label_x.as_deref().unwrap_or("").to_string();
    let y_label = config.axis_label_y.as_deref().unwrap_or("").to_string();

    let mut t = String::new();

    let protocol_label = match &config.protocol {
        Some(ProtocolConfig::Local(_)) => "Local",
        #[cfg(feature = "epics")]
        Some(ProtocolConfig::EpicsPva(_)) => "EPICS PVA",
        #[cfg(feature = "modbus")]
        Some(ProtocolConfig::ModbusTcp(_)) => "Modbus TCP",
        _ => "None",
    };
    t.push_str(&format!("ID: {}\n", config.id));
    t.push_str(&format!("Protocol: {}\n", protocol_label));

    // Chart type line
    let type_str = match chart_type {
        "histogram" => "Histogram",
        "scatter" => "Scatter",
        "scatter_histogram" => "Scatter + Histogram",
        _ => "Line",
    };
    t.push_str(&format!("Chart type: {}\n", type_str));

    // Axis labels
    if !x_label.is_empty() {
        t.push_str(&format!("X-axis: {}\n", x_label));
    }
    if !y_label.is_empty() {
        t.push_str(&format!("Y-axis: {}\n", y_label));
    }

    t.push('\n');

    // Per-PV / per-series breakdown
    match chart_type {
        "scatter" | "scatter_histogram" => {
            // pv_name → X axis, first name in pv_names → Y axis
            #[cfg(feature = "epics")]
            if let Some(epics) = config.epics_pva() {
                t.push_str(&format!("X data:  {}\n", epics.pv_name));
                if let Some(names) = &epics.pv_names {
                    if let Some(y_pv) = names.first() {
                        t.push_str(&format!("Y data:  {}\n", y_pv));
                    }
                }
            } else {
                t.push_str(&format!("Channel: {}\n", config.channel_address()));
            }
            #[cfg(not(feature = "epics"))]
            t.push_str(&format!("Channel: {}\n", config.channel_address()));
        }
        "histogram" => {
            t.push_str(&format!("Data PV: {}\n", config.channel_address()));
        }
        _ => {
            // Line chart — list all series PVs
            let series_pvs = collect_series_pvs(config);
            for (idx, pv) in series_pvs.iter().enumerate() {
                t.push_str(&format!("Series {}: {}\n", idx + 1, pv));
            }
        }
    }

    // Standard PV metadata (from the normalised ChannelValue fields)
    t.push('\n');
    if !raw.primary_meta.description.is_empty() {
        t.push_str(&format!("{}\n", raw.primary_meta.description));
    }
    if !raw.units.is_empty() {
        t.push_str(&format!("Units: {}\n", raw.units));
    }
    if raw.display_low != 0.0 || raw.display_high != 0.0 {
        t.push_str(&format!(
            "Display range: {:.2} – {:.2}\n",
            raw.display_low, raw.display_high
        ));
    }

    let sev_str = match raw.alarm_severity {
        0 => "No Alarm",
        1 => "Minor",
        2 => "Major",
        3 => "Invalid",
        _ => "Unknown",
    };
    t.push_str(&format!("Alarm: {}\n", sev_str));

    t.trim_end().to_string()
}

/// Collect all PV names for a multi-series line chart (primary + extras from EpicsPva config).
fn collect_series_pvs(config: &WidgetConfig) -> Vec<String> {
    match &config.protocol {
        #[cfg(feature = "epics")]
        Some(crate::config::ProtocolConfig::EpicsPva(e)) => e.series_pvs(),
        _ => Vec::new(),
    }
}

pub fn render_inner_connected(config: &WidgetConfig, cv: &ChannelValue) -> Markup {
    let alarm_class = super::alarm_severity_class(cv.alarm_severity);
    let icon: Option<&str> = match cv.alarm_severity {
        1 => Some(super::MINOR_ALARM_SVG),
        2 => Some(super::MAJOR_ALARM_SVG),
        3 => Some(super::INVALID_SVG),
        _ => None,
    };

    // Multi-series: named_series is populated by epics_channel::run_multi_monitor
    if !cv.named_series.is_empty() {
        let x_label = config.axis_label_x.as_deref().unwrap_or("");
        let y_label = config.axis_label_y.as_deref().unwrap_or("");
        let all_pvs = collect_series_pvs(config);
        let series_vecs: Vec<(&str, Vec<f64>)> = all_pvs
            .iter()
            .filter_map(|pv| {
                cv.named_series
                    .iter()
                    .find(|(n, _)| n == pv)
                    .map(|(_, v)| (pv.as_str(), v.clone()))
            })
            .collect();
        let series_refs: Vec<(&str, &[f64])> = series_vecs
            .iter()
            .map(|(n, v)| (*n, v.as_slice()))
            .collect();
        let svg_string = render_line_chart(&series_refs, x_label, y_label);
        let mut t = format!("Chart type: Line\n");
        if !x_label.is_empty() {
            t.push_str(&format!("X-axis: {}\n", x_label));
        }
        if !y_label.is_empty() {
            t.push_str(&format!("Y-axis: {}\n", y_label));
        }
        t.push('\n');
        for (idx, pv) in all_pvs.iter().enumerate() {
            t.push_str(&format!("Series {}: {}\n", idx + 1, pv));
        }
        t.push('\n');
        let meta = &cv.primary_meta;
        if !meta.description.is_empty() {
            t.push_str(&format!("{0}\n", meta.description));
        }
        if !cv.units.is_empty() {
            t.push_str(&format!("Units: {}\n", cv.units));
        }
        let sev_str = match cv.alarm_severity {
            0 => "No Alarm",
            1 => "Minor",
            2 => "Major",
            _ => "Invalid",
        };
        t.push_str(&format!("Alarm: {}\n", sev_str));
        return render_chart_html(
            config,
            None,
            &format!("chart {}", alarm_class),
            icon,
            t.trim_end(),
            &svg_string,
        );
    }

    let chart_type = config.chart_type.as_deref().unwrap_or("line");
    let x_label = config.axis_label_x.as_deref().unwrap_or("");
    let y_label = config.axis_label_y.as_deref().unwrap_or("");

    let svg_string = match chart_type {
        "histogram" => render_histogram(&cv.array_values, x_label, y_label),
        "scatter" => {
            let arr = &cv.array_values;
            if arr.len() >= 2 {
                let mid = arr.len() / 2;
                render_scatter(&arr[..mid], &arr[mid..], x_label, y_label)
            } else {
                render_scatter(&[], &[], x_label, y_label)
            }
        }
        "scatter_histogram" => {
            let arr = &cv.array_values;
            if arr.len() >= 2 {
                let mid = arr.len() / 2;
                render_scatter_histogram(&arr[..mid], &arr[mid..], x_label, y_label)
            } else {
                render_scatter_histogram(&[], &[], x_label, y_label)
            }
        }
        _ => render_line_chart(&[("value", cv.array_values.as_slice())], x_label, y_label),
    };

    let tooltip = build_chart_tooltip(config, cv);
    render_chart_html(
        config,
        None,
        &format!("chart {}", alarm_class),
        icon,
        &tooltip,
        &svg_string,
    )
}

pub fn render_inner_disconnected(config: &WidgetConfig) -> Markup {
    render_chart_html(
        config,
        None,
        "chart alarm-disconnected",
        Some(super::OFFLINE_SVG),
        "",
        "",
    )
}

fn render_chart_html(
    config: &WidgetConfig,
    _display_value: Option<String>, // kept for future scalar fallback
    _alarm_class: &str,
    icon: Option<&str>,
    tooltip: &str,
    svg_content: &str,
) -> Markup {
    html! {
        div class="widget-inner" {
            div class="chart-container" {
                @if !svg_content.is_empty() {
                    (PreEscaped(svg_content))
                } @else {
                    div class="chart-placeholder" { "Waiting for data…" }
                }
            }
            label class="widget-label" {
                (config.label)
                @if let Some(src) = icon {
                    img class="widget-status-icon" src=(src) alt="status";
                }
                @if !tooltip.is_empty() {
                    (super::render_info_btn(tooltip))
                }
            }
        }
    }
}

pub fn render_chart(widget: &WidgetConfig) -> Markup {
    html! {
        div style=[super::widget_container_style(widget)]
            data-widget-id=(widget.id)
            data-ch=(widget.channel_address())
            hx-sse=(format!("swap:{}", widget.id)) {
            (render_inner_disconnected(widget))
        }
    }
}
