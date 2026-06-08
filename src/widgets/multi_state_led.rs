//! `MultiStateLed` widget — multi-state polygon indicator for valve feedback.
//!
//! Reads a single holding register and maps its value to three visual states:
//! - **open**    (green)  — register = 1 (or 0 when `invert = true`)
//! - **closed**  (red)    — register = 0 (or 1 when `invert = true`)
//! - **pending** (grey)   — disconnected / Modbus error
//!
//! The polygon shape is configurable via `widget.polygon_points`.
//! mycela defaults to a plain rectangle; downstream apps supply a bowtie:
//! `"0,0 0,18 15,9 30,18 30,0 15,9"` (30 × 18 px).

use maud::{html, Markup};
use std::sync::Arc;
use futures::StreamExt;
use crate::channel::{ChannelContext, ChannelEvent, ChannelValue};
use crate::config::WidgetConfig;

/// Default polygon for mycela's generic square indicator (30 × 18).
const DEFAULT_POINTS: &str = "0,0 30,0 30,18 0,18";

/// Compute the bounding box of a polygon points string (e.g. `"0,0 30,18 …"`).
/// Returns `(width, height)` as the ceiling of the max x and max y values.
/// Falls back to `(30, 18)` if the string cannot be parsed.
fn polygon_dimensions(points: &str) -> (u32, u32) {
    let mut max_x: f64 = 0.0;
    let mut max_y: f64 = 0.0;
    for pair in points.split_whitespace() {
        let mut parts = pair.splitn(2, ',');
        if let (Some(xs), Some(ys)) = (parts.next(), parts.next()) {
            if let (Ok(x), Ok(y)) = (xs.parse::<f64>(), ys.parse::<f64>()) {
                if x > max_x { max_x = x; }
                if y > max_y { max_y = y; }
            }
        }
    }
    let w = if max_x > 0.0 { max_x.ceil() as u32 } else { 30 };
    let h = if max_y > 0.0 { max_y.ceil() as u32 } else { 18 };
    (w, h)
}

pub struct MultiStateLed {
    config: WidgetConfig,
}

impl MultiStateLed {
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

        tokio::spawn(MultiStateLed::run_monitor_async(config.clone(), ctx, tx));

        async_stream::stream! {
            yield Ok(axum::response::sse::Event::default().data(
                render_inner_pending(&config).into_string()
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
                ChannelEvent::Value(cv)          => render_inner_connected(&config, &cv).into_string(),
                ChannelEvent::Disconnected(_)
                | ChannelEvent::Error(_)         => render_inner_pending(&config).into_string(),
                ChannelEvent::Connected          => continue,
            };
            if tx.send(html).is_err() { break; }
        }
    }
}

// ─── State helpers ─────────────────────────────────────────────────────────────

fn state_class(raw_value: f64, invert: bool) -> &'static str {
    if raw_value != 0.0 && raw_value != 1.0 {
        return "vs-pending";
    }

    let is_open = (raw_value != 0.0) ^ invert;
    if is_open { "vs-open" } else { "vs-closed" }
}

fn state_label(cls: &str) -> &'static str {
    match cls {
        "vs-open"   => "OPEN",
        "vs-closed" => "CLOSED",
        _           => "--",
    }
}

// ─── Render functions (pub so pid_screen.rs can call render_valve_state) ───────

pub fn render_inner_connected(config: &WidgetConfig, cv: &ChannelValue) -> Markup {
    let cls = state_class(cv.raw_value, config.invert.unwrap_or(false));
    let icon: Option<&str> = match cv.alarm_severity {
        1 => Some(super::MINOR_ALARM_SVG),
        2 => Some(super::MAJOR_ALARM_SVG),
        3 => Some(super::INVALID_SVG),
        _ => None,
    };
    render_polygon_html(config, cls, icon, &super::build_led_tooltip(config, cv))
}

pub fn render_inner_pending(config: &WidgetConfig) -> Markup {
    let tooltip = super::build_disconnected_tooltip(config);
    render_polygon_html(config, "vs-pending", Some(super::OFFLINE_SVG), &tooltip)
}

fn render_polygon_html(config: &WidgetConfig, state_cls: &str, icon: Option<&str>, tooltip: &str) -> Markup {
    let points = config.polygon_points.as_deref().unwrap_or(DEFAULT_POINTS);
    let (w, h) = polygon_dimensions(points);
    let view_box = format!("0 0 {} {}", w, h);
    let flex_dir = match config.label_position.as_deref() {
        Some("top")    => "column",
        Some("bottom") => "column-reverse",
        Some("right")  => "row-reverse",
        _              => "row",   // "left" or unset → default
    };
    let state_pos = config.label_position.as_deref().unwrap_or("bottom");
    let wrapper_style = format!("flex-direction:{};", flex_dir);
    html! {
        div class="widget-inner vs-widget" style=(wrapper_style) data-state-pos=(state_pos) {
            svg class="vs-polygon" width=(w) height=(h) viewBox=(view_box)
                xmlns="http://www.w3.org/2000/svg" {
                polygon class=(state_cls) points=(points) {}
            }
            span class={"vs-state " (state_cls) "-text"} {
                (state_label(state_cls))
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

/// Full outer div wrapper with SSE swap binding.
/// Used inside a container that has `hx-sse="connect:…"`.
pub fn render_multi_state_led(widget: &WidgetConfig) -> Markup {
    html! {
        div style=[super::widget_container_style(widget)]
            data-widget-id=(widget.id)
            data-ch=(widget.channel_address())
            hx-sse=(format!("swap:{}", widget.id)) {
            (render_inner_pending(widget))
        }
    }
}
