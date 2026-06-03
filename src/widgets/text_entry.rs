use maud::{html, Markup};
use std::sync::Arc;
use futures::StreamExt;
use crate::channel::{ChannelContext, ChannelEvent, ChannelValue};
use crate::config::WidgetConfig;

pub struct TextEntry {
    config: WidgetConfig,
}

impl TextEntry {
    pub fn new(config: WidgetConfig) -> Self {
        Self { config }
    }

    /// Spawn an async channel monitor and return a live SSE event stream.
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
                render_inner_disconnected(&config, "Connecting...").into_string()
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
                ChannelEvent::Disconnected(msg)
                | ChannelEvent::Error(msg)       => render_inner_disconnected(&config, &msg).into_string(),
                ChannelEvent::Connected          => continue,
            };
            if tx.send(html).is_err() { break; }
        }
    }
}

pub fn render_inner_connected(config: &WidgetConfig, cv: &ChannelValue) -> Markup {
    let alarm_class = super::alarm_severity_class(cv.alarm_severity);
    let icon: Option<&str> = match cv.alarm_severity {
        0 => None,
        1 => Some(super::MINOR_ALARM_SVG),
        2 => Some(super::MAJOR_ALARM_SVG),
        _ => Some(super::INVALID_SVG),
    };
    let is_double  = matches!(config.data_type.as_deref(), Some("double") | Some("float") | Some("f64") | Some("f32"));
    let is_integer = matches!(config.data_type.as_deref(), Some("integer") | Some("int") | Some("i32") | Some("int32") | Some("bool"));
    let is_string  = !is_double && !is_integer;
    let precision_step = 10f64.powi(-(cv.precision as i32).max(0));
    let min_step = if is_integer {
        1.0
    } else {
        config.metadata.as_ref()
            .and_then(|m| m.control.as_ref())
            .map(|c| c.min_step)
            .filter(|&s| s > 0.0)
            .unwrap_or(precision_step)
    };
    let tooltip = super::build_tooltip(config, cv);
    render_input_html(config, &cv.value_str, &cv.units, min_step, is_string,
                      &format!("text-entry {}", alarm_class), icon, false, &tooltip)
}

pub fn render_inner_disconnected(config: &WidgetConfig, _reason: &str) -> Markup {
    let is_string = config.data_type.as_deref() == Some("string");
    render_input_html(config, "--", "", 0.01, is_string,
                      "text-entry alarm-disconnected", Some(super::OFFLINE_SVG), true, "")
}

fn render_input_html(
    config: &WidgetConfig,
    current_value: &str,
    units: &str,
    min_step: f64,
    is_string: bool,
    input_class: &str,
    icon: Option<&str>,
    disabled: bool,
    tooltip: &str,
) -> Markup {
    let input_type = if is_string { "text" } else { "number" };
    html! {
        div class="widget-inner" {
            label class="widget-label" {
                (config.label)
                @if let Some(src) = icon {
                    img class="widget-status-icon" src=(src) alt="status";
                }
                @if !tooltip.is_empty() {
                    button class="widget-info-btn" data-tooltip=(tooltip) type="button" {
                        img class="info-icon info-icon--dark"  src=(super::INFO_SVG_DARK)  alt="info";
                        img class="info-icon info-icon--light" src=(super::INFO_SVG_LIGHT) alt="info";
                    }                }
            }
            div class="text-entry-with-icon-container" {
                @if is_string {
                    input type="text"
                        class=(input_class)
                        name="value"
                        value=(current_value)
                        disabled[disabled]
                        hx-post={"/api/widget/" (config.id) "/set"}
                        hx-trigger="keyup[key=='Enter']"
                        hx-target="next .status"
                        hx-swap="innerHTML";
                } @else {
                    input type=(input_type)
                        class=(input_class)
                        name="value"
                        value=(current_value)
                        data-original-value=(current_value)
                        step=(format!("{}", min_step))
                        disabled[disabled]
                        hx-post={"/api/widget/" (config.id) "/set"}
                        hx-trigger="keyup[key=='Enter']"
                        hx-target="next .status"
                        hx-swap="innerHTML"
                        hx-on--before-request="if(isNaN(parseFloat(this.value))||!isFinite(this.value)){this.value=this.dataset.originalValue;event.preventDefault();this.parentElement.nextElementSibling.textContent='Invalid number';return false;}else{this.dataset.originalValue=this.value;this.parentElement.nextElementSibling.textContent='';return true;}";
                }
                @if !units.is_empty() {
                    span class="units-overlay" { (units) }
                }
            }
            span class="status" {}
            @if let Some(desc) = &config.description {
                @if !desc.is_empty() {
                    p class="widget-description" { (desc) }
                }
            }
        }
    }
}

/// Render the outer SSE shell for a text entry widget.
pub fn render_text_entry(widget: &WidgetConfig) -> Markup {
    html! {
        div style=[super::widget_container_style(widget)]
            data-widget-id=(widget.id)
            data-ch=(widget.channel_address())
            hx-sse=(format!("swap:{}", widget.id)) {
            (render_inner_disconnected(widget, "Connecting..."))
        }
    }
}
