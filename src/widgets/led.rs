use maud::{html, Markup};
use std::sync::Arc;
use futures::StreamExt;
use crate::channel::{ChannelContext, ChannelEvent, ChannelValue};
use crate::config::WidgetConfig;

pub struct Led {
    config: WidgetConfig,
}

impl Led {
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
        let ctx_clone = ctx.clone();
        let widget_id = config.id.clone();
        let ctx_publish = ctx_clone.clone();
        let widget_id_publish = widget_id.clone();
        let mut stream = crate::channel::channel_stream(config.clone(), ctx)
            .inspect(move |e| {
                if let ChannelEvent::Value(cv) = e {
                    ctx_publish.publish_widget_value(&widget_id_publish, cv.clone());
                }
            });
        let mut last_value: Option<ChannelValue> = None;
        while let Some(event) = stream.next().await {
            let html = match event {
                ChannelEvent::Value(cv)          => {
                    last_value = Some(cv.clone());
                    render_inner_connected(&config, &cv).into_string()
                }
                ChannelEvent::Disconnected(_)
                | ChannelEvent::Error(_)         => {
                    render_inner_disconnected_with_last(&config, last_value.as_ref()).into_string()
                }
                ChannelEvent::Connected          => continue,
            };
            if tx.send(html).is_err() { break; }
        }
    }
}

pub fn render_inner_connected(config: &WidgetConfig, cv: &ChannelValue) -> Markup {
    let icon: Option<&str> = match cv.alarm_severity {
        1 => Some(super::MINOR_ALARM_SVG),
        2 => Some(super::MAJOR_ALARM_SVG),
        3 => Some(super::INVALID_SVG),
        _ => None,
    };
    let invert = config.invert.unwrap_or(false);
    let is_on = if cv.raw_value == 0.0 {
        false ^ invert
    } else if cv.raw_value == 1.0 {
        true ^ invert
    } else {
        cv.raw_value > 0.5
    };
    render_led_html(config, is_on, icon, false, &super::tooltips::build_led_tooltip(config, cv))
}

pub fn render_inner_disconnected(config: &WidgetConfig) -> Markup {
    render_inner_disconnected_with_last(config, None)
}

pub fn render_inner_disconnected_with_last(config: &WidgetConfig, last_value: Option<&ChannelValue>) -> Markup {
    let invert = config.invert.unwrap_or(false);
    let is_on = match last_value {
        Some(cv) if cv.raw_value == 0.0 => false ^ invert,
        Some(cv) if cv.raw_value == 1.0 => true ^ invert,
        Some(cv) => cv.raw_value > 0.5,
        None => false,
    };
    let tooltip = super::tooltips::build_disconnected_tooltip(config);
    render_led_html(config, is_on, Some(super::OFFLINE_SVG), true, &tooltip)
}

fn render_led_html(
    config: &WidgetConfig,
    is_on: bool,
    icon: Option<&str>,
    _disconnected: bool,
    tooltip: &str,
) -> Markup {
    let led_state = if is_on { "led-on" } else { "led-off" };
    html! {
        div class="widget-inner" {
            div class="led-container" {
                div class={"led-indicator " (led_state)} {
                    span class="led-light" {}
                }
                span class="led-status" {
                    @if is_on { "ON" }
                    @else { "OFF" }
                }
            }
            label class="widget-label" {
                (config.label)
                @if let Some(src) = icon {
                    img class="widget-status-icon" src=(src) alt="status";
                }
                @if !tooltip.is_empty() {
                    (super::tooltips::render_tooltip_info_btn(tooltip))
                }
            }
        }
    }
}

pub fn render_led(widget: &WidgetConfig) -> Markup {
    html! {
        div style=[super::widget_container_style(widget)]
            data-widget-id=(widget.id)
            data-ch=(widget.channel_address())
            hx-sse=(format!("swap:{}", widget.id)) {
            (render_inner_disconnected(widget))
        }
    }
}
