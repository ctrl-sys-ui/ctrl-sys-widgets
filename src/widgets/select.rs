use maud::{html, Markup};
use std::sync::Arc;
use futures::StreamExt;
use crate::channel::{ChannelContext, ChannelEvent, ChannelValue};
use crate::config::WidgetConfig;

pub struct Select {
    config: WidgetConfig,
}

impl Select {
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
        let mut enabled_rx = ctx.subscribe_widget_enabled(&config.id);
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
        let mut last_html = String::new();
        while let Some(event) = stream.next().await {
            let html = match event {
                ChannelEvent::Value(cv)         => {
                    ctx_clone.set_widget_connected(&widget_id, true);
                    last_value = Some(cv.clone());
                    render_inner_connected(&config, &cv, *enabled_rx.borrow()).into_string()
                }
                ChannelEvent::Disconnected(_)
                | ChannelEvent::Error(_)        => {
                    ctx_clone.set_widget_connected(&widget_id, false);
                    render_inner_disconnected_with_last(&config, last_value.as_ref()).into_string()
                }
                ChannelEvent::Connected         => {
                    ctx_clone.set_widget_connected(&widget_id, true);
                    continue;
                }
            };
            if html != last_html {
                last_html = html.clone();
                if tx.send(html).is_err() {
                    ctx_clone.set_widget_connected(&widget_id, false);
                    break;
                }
            }

            while enabled_rx.has_changed().unwrap_or(false) {
                if enabled_rx.changed().await.is_err() {
                    break;
                }
                let html = if ctx_clone.is_widget_connected(&widget_id) {
                    match &last_value {
                        Some(cv) => render_inner_connected(&config, cv, *enabled_rx.borrow()).into_string(),
                        None => render_inner_disconnected(&config).into_string(),
                    }
                } else {
                    render_inner_disconnected_with_last(&config, last_value.as_ref()).into_string()
                };
                if html != last_html {
                    last_html = html.clone();
                    if tx.send(html).is_err() {
                        ctx_clone.set_widget_connected(&widget_id, false);
                        break;
                    }
                }
            }
        }
    }
}

pub fn render_inner_connected(config: &WidgetConfig, cv: &ChannelValue, enabled: bool) -> Markup {
    let alarm_class    = super::alarm_severity_class(cv.alarm_severity);
    let icon: Option<&str> = match cv.alarm_severity {
        1 => Some(super::MINOR_ALARM_SVG),
        2 => Some(super::MAJOR_ALARM_SVG),
        3 => Some(super::INVALID_SVG),
        _ => None,
    };
    let current_index = cv.enum_index as usize;
    let choices = &cv.enum_choices;
    let tooltip = super::tooltips::build_enum_tooltip(config, cv);
    let display_text = choices.get(current_index).map(|s| s.trim().to_string())
        .unwrap_or_else(|| current_index.to_string());

    html! {
        div class="widget-inner" data-widget-enabled=(if enabled { "true" } else { "false" }) {
            div class="select-with-icon-container" {
                @if let Some(src) = icon {
                    img class="select-icon" src=(src) alt="status";
                }
                div class="select-wrapper" {
                    select class=(format!("widget-select {}", alarm_class))
                        name="value"
                        disabled[!enabled]
                        hx-post={"/api/widget/" (config.id) "/set"}
                        hx-trigger="change"
                        hx-target="next .status"
                        hx-swap="innerHTML" {
                        @for (idx, choice) in choices.iter().enumerate() {
                            option value=(idx) selected[idx == current_index] { (choice.trim()) }
                        }
                        @if choices.is_empty() {
                            option value=(current_index) selected { (current_index) }
                        }
                    }
                    span class="status" {}
                    span class="select-display-text" { (display_text) }
                }
            }
            label class="widget-label" {
                (config.label)
                @if !tooltip.is_empty() {
                    (super::tooltips::render_tooltip_info_btn(&tooltip))
                }
            }
        }
    }
}

pub fn render_inner_disconnected(config: &WidgetConfig) -> Markup {
    render_inner_disconnected_with_last(config, None)
}

fn render_inner_disconnected_with_last(config: &WidgetConfig, last: Option<&ChannelValue>) -> Markup {
    let (choices, current_index, display_text) = match last {
        Some(cv) => {
            let idx = cv.enum_index.max(0) as usize;
            let display = cv
                .enum_choices
                .get(idx)
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|| idx.to_string());
            (cv.enum_choices.clone(), idx, display)
        }
        None => (Vec::new(), 0usize, "--".to_string()),
    };

    html! {
        div class="widget-inner" data-widget-enabled="false" {
            label class="widget-label" { (config.label) }
            div class="select-with-icon-container" {
                img class="select-icon" src=(super::OFFLINE_SVG) alt="offline";
                div class="select-wrapper" {
                    select class="widget-select alarm-disconnected" disabled {
                        @if choices.is_empty() {
                            option value="0" selected { "--" }
                        } @else {
                            @for (idx, choice) in choices.iter().enumerate() {
                                option value=(idx) selected[idx == current_index] { (choice.trim()) }
                            }
                        }
                    }
                    span class="select-display-text" { (display_text) }
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

/// Render the outer SSE shell for a select widget.
pub fn render_select(widget: &WidgetConfig) -> Markup {
    html! {
        div style=[super::widget_container_style(widget)]
            data-widget-id=(widget.id)
            data-ch=(widget.channel_address())
            data-widget-enabled="false"
            hx-sse=(format!("swap:{}", widget.id)) {
            (render_inner_disconnected(widget))
        }
    }
}
