use maud::{html, Markup};
use std::sync::Arc;
use futures::StreamExt;
use crate::channel::{ChannelContext, ChannelEvent, ChannelValue};
use crate::config::WidgetConfig;

pub struct Button {
    config: WidgetConfig,
}

impl Button {
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
                ChannelEvent::Value(cv)          => render_inner_connected(&config, &cv).into_string(),
                ChannelEvent::Disconnected(_)
                | ChannelEvent::Error(_)         => render_inner_disconnected(&config).into_string(),
                ChannelEvent::Connected          => continue,
            };
            if tx.send(html).is_err() { break; }
        }
    }
}

pub fn render_inner_connected(config: &WidgetConfig, cv: &ChannelValue) -> Markup {
    render_button_html(config, false, &super::build_tooltip(config, cv))
}

pub fn render_inner_disconnected(config: &WidgetConfig) -> Markup {
    let tooltip = super::build_disconnected_tooltip(config);
    render_button_html(config, true, &tooltip)
}

fn render_button_html(
    config: &WidgetConfig,
    disabled: bool,
    tooltip: &str,
) -> Markup {
    html! {
        div class="widget-inner" {
            @if !tooltip.is_empty() {
                (super::render_info_btn(tooltip))
            }
            button class={
                    "widget-button"
                    @if let Some(c) = &config.color { " widget-button--" (c) }
                }
                disabled[disabled]
                hx-post={"/api/widget/" (config.id) "/set"}
                hx-vals=(format!(r#"{{"value": "{}"}}"#, config.write_value.unwrap_or(1)))
                hx-target="next .status"
                hx-swap="innerHTML" {
                (config.label)
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

pub fn render_button(widget: &WidgetConfig) -> Markup {
    html! {
        div style=[super::widget_container_style(widget)]
            data-widget-id=(widget.id)
            data-ch=(widget.channel_address())
            hx-sse=(format!("swap:{}", widget.id)) {
            (render_inner_disconnected(widget))
        }
    }
}
