use crate::channel::{ChannelContext, ChannelEvent, ChannelValue};
use crate::config::WidgetConfig;
use futures::StreamExt;
use maud::{html, Markup};
use std::sync::Arc;

pub struct Gauge {
    config: WidgetConfig,
}

impl Gauge {
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

pub fn render_inner_connected(config: &WidgetConfig, cv: &ChannelValue) -> Markup {
    let alarm_class = super::alarm_severity_class(cv.alarm_severity);
    let icon: Option<&str> = match cv.alarm_severity {
        1 => Some(super::MINOR_ALARM_SVG),
        2 => Some(super::MAJOR_ALARM_SVG),
        3 => Some(super::INVALID_SVG),
        _ => None,
    };
    let display_value = if cv.value_str.is_empty() {
        format!("{:.prec$}", cv.raw_value, prec = cv.precision as usize)
    } else {
        cv.value_str.clone()
    };
    let min = cv.display_low;
    let max = if (cv.display_high - cv.display_low).abs() < f64::EPSILON {
        cv.display_low + 100.0
    } else {
        cv.display_high
    };
    let percentage = ((cv.raw_value - min) / (max - min) * 100.0).clamp(0.0, 100.0);
    let range = max - min;
    let to_pct = |v: f64| ((v - min) / range * 100.0).clamp(0.0, 100.0);
    let low_alarm = if cv.low_alarm_limit != 0.0 {
        Some((cv.low_alarm_limit, to_pct(cv.low_alarm_limit)))
    } else {
        None
    };
    let low_warn = if cv.low_warn_limit != 0.0 {
        Some((cv.low_warn_limit, to_pct(cv.low_warn_limit)))
    } else {
        None
    };
    let high_warn = if cv.high_warn_limit != 100.0 {
        Some((cv.high_warn_limit, to_pct(cv.high_warn_limit)))
    } else {
        None
    };
    let high_alarm = if cv.high_alarm_limit != 100.0 {
        Some((cv.high_alarm_limit, to_pct(cv.high_alarm_limit)))
    } else {
        None
    };
    let tooltip = super::build_tooltip(config, cv);
    render_gauge_html(
        config,
        &display_value,
        &cv.units,
        min,
        max,
        percentage,
        &format!("gauge {}", alarm_class),
        icon,
        low_alarm,
        low_warn,
        high_warn,
        high_alarm,
        &tooltip,
    )
}

pub fn render_inner_disconnected(config: &WidgetConfig) -> Markup {
    render_gauge_html(
        config,
        "--",
        "",
        0.0,
        100.0,
        0.0,
        "gauge alarm-disconnected",
        Some(super::OFFLINE_SVG),
        None,
        None,
        None,
        None,
        "",
    )
}

fn render_gauge_html(
    config: &WidgetConfig,
    display_value: &str,
    units: &str,
    min: f64,
    max: f64,
    percentage: f64,
    _alarm_class: &str, // reserved for future CSS alarm colouring; alarm state is shown via icon
    icon: Option<&str>,
    low_alarm: Option<(f64, f64)>,
    low_warn: Option<(f64, f64)>,
    high_warn: Option<(f64, f64)>,
    high_alarm: Option<(f64, f64)>,
    tooltip: &str,
) -> Markup {
    let has_alarm_labels =
        low_alarm.is_some() || low_warn.is_some() || high_warn.is_some() || high_alarm.is_some();
    let vertical = config.orientation.as_deref() == Some("vertical");
    let fill_style = if vertical {
        format!("height: {:.1}%", percentage)
    } else {
        format!("width: {:.1}%", percentage)
    };
    let display_class = if vertical {
        "gauge-display gauge-vertical"
    } else {
        "gauge-display"
    };

    // Graduated axis: 5 evenly-spaced ticks from min to max
    let tick_count = 5;
    let range = max - min;
    let ticks: Vec<(f64, f64)> = (0..=tick_count)
        .map(|i| {
            let frac = i as f64 / tick_count as f64;
            let val = min + frac * range;
            let pct = frac * 100.0;
            (val, pct)
        })
        .collect();

    html! {
        div class="widget-inner" {
            div class=(display_class) {
                div class="gauge-value" {
                    (display_value)
                    @if !units.is_empty() { " " (units) }
                }
                // Body row: limits | bar | axis
                div class="gauge-body" {
                    // Alarm limit labels (above bar in horizontal, left of bar in vertical)
                    @if has_alarm_labels {
                        div class="gauge-limits" {
                            @if let Some((v, p)) = low_alarm {
                                @let pos = if vertical { format!("bottom:{:.2}%", p) } else { format!("left:{:.2}%", p) };
                                span class="gauge-limit gauge-limit--low-low" style=(pos) { (format!("{:.1}", v)) }
                            }
                            @if let Some((v, p)) = low_warn {
                                @let pos = if vertical { format!("bottom:{:.2}%", p) } else { format!("left:{:.2}%", p) };
                                span class="gauge-limit gauge-limit--low" style=(pos) { (format!("{:.1}", v)) }
                            }
                            @if let Some((v, p)) = high_warn {
                                @let pos = if vertical { format!("bottom:{:.2}%", p) } else { format!("left:{:.2}%", p) };
                                span class="gauge-limit gauge-limit--high" style=(pos) { (format!("{:.1}", v)) }
                            }
                            @if let Some((v, p)) = high_alarm {
                                @let pos = if vertical { format!("bottom:{:.2}%", p) } else { format!("left:{:.2}%", p) };
                                span class="gauge-limit gauge-limit--high-high" style=(pos) { (format!("{:.1}", v)) }
                            }
                        }
                    }
                    // bar + alarm marker overlay
                    div class="gauge-bar" {
                        div class="gauge-fill" style=(fill_style) {}
                        @if let Some((_, p)) = low_alarm {
                            @let pos = if vertical { format!("bottom:{:.2}%", p) } else { format!("left:{:.2}%", p) };
                            div class="gauge-marker gauge-marker--alarm" style=(pos) {}
                        }
                        @if let Some((_, p)) = low_warn {
                            @let pos = if vertical { format!("bottom:{:.2}%", p) } else { format!("left:{:.2}%", p) };
                            div class="gauge-marker gauge-marker--warn" style=(pos) {}
                        }
                        @if let Some((_, p)) = high_warn {
                            @let pos = if vertical { format!("bottom:{:.2}%", p) } else { format!("left:{:.2}%", p) };
                            div class="gauge-marker gauge-marker--warn" style=(pos) {}
                        }
                        @if let Some((_, p)) = high_alarm {
                            @let pos = if vertical { format!("bottom:{:.2}%", p) } else { format!("left:{:.2}%", p) };
                            div class="gauge-marker gauge-marker--alarm" style=(pos) {}
                        }
                    }
                    // Graduated axis (below bar in horizontal, right of bar in vertical)
                    div class="gauge-axis" {
                        @for &(val, pct) in &ticks {
                            @let pos = if vertical { format!("bottom:{:.2}%", pct) } else { format!("left:{:.2}%", pct) };
                            span class="gauge-tick" style=(pos) {
                                span class="gauge-tick-mark" {}
                                span class="gauge-tick-label" { (format!("{:.1}", val)) }
                            }
                        }
                    }
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

pub fn render_gauge(widget: &WidgetConfig) -> Markup {
    html! {
        div style=[super::widget_container_style(widget)]
            data-widget-id=(widget.id)
            data-ch=(widget.channel_address())
            hx-sse=(format!("swap:{}", widget.id)) {
            (render_inner_disconnected(widget))
        }
    }
}
