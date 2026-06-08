use maud::{html, Markup};
use std::sync::Arc;
use futures::StreamExt;
use crate::channel::{ChannelContext, ChannelEvent, ChannelValue};
use crate::config::WidgetConfig;

pub struct TextUpdate {
    config: WidgetConfig,
}

impl TextUpdate {
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
        let ctx_clone = ctx.clone();
        let widget_id = config.id.clone();
        let mut stream = crate::channel::channel_stream(config.clone(), ctx)
            .inspect(move |e| {
                if let ChannelEvent::Value(cv) = e {
                    ctx_clone.publish_widget_value(&widget_id, cv.clone());
                }
            });
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
        1 => Some(super::MINOR_ALARM_SVG),
        2 => Some(super::MAJOR_ALARM_SVG),
        3 => Some(super::INVALID_SVG),
        _ => None,
    };
    let tooltip = super::build_tooltip(config, cv);
    render_display_html(config, &cv.value_str, &cv.units, &format!("text-update {}", alarm_class), icon, &tooltip)
}

pub fn render_inner_disconnected(config: &WidgetConfig, _reason: &str) -> Markup {
    let tooltip = super::build_disconnected_tooltip(config);
    render_display_html(config, "--", "", "text-update alarm-disconnected", Some(super::OFFLINE_SVG), &tooltip)
}

fn render_display_html(
    config: &WidgetConfig,
    current_value: &str,
    units: &str,
    alarm_class: &str,
    icon: Option<&str>,
    tooltip: &str,
) -> Markup {
    let input_type = if matches!(config.data_type.as_deref(), Some("string") | None) { "text" } else { "number" };
    html! {
        div class="widget-inner" {
            div class="text-update-with-icon-container" {
                input type=(input_type)
                    class=(alarm_class)
                    name="value"
                    value=(current_value)
                    readonly;
                @if !units.is_empty() {
                    span class="units-overlay" { (units) }
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

pub fn render_text_update(widget: &WidgetConfig) -> Markup {
    html! {
        div style=[super::widget_container_style(widget)]
            data-widget-id=(widget.id)
            data-ch=(widget.channel_address())
            hx-sse=(format!("swap:{}", widget.id)) {
            (render_inner_disconnected(widget, "Connecting..."))
        }
    }
}
