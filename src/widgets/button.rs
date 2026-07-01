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
        let mut enabled_rx = ctx.subscribe_widget_enabled(&config.id);
        let ctx_clone = ctx.clone();
        let widget_id = config.id.clone();
        let mut stream = crate::channel::channel_stream(config.clone(), ctx);
        let mut last_cv: Option<ChannelValue> = None;
        let mut last_html = String::new();

        let send_if_changed = |tx: &tokio::sync::mpsc::UnboundedSender<String>,
                               last_html: &mut String,
                               html: String| {
            if *last_html != html {
                *last_html = html.clone();
                tx.send(html).is_ok()
            } else {
                true
            }
        };

        loop {
            tokio::select! {
                maybe_event = stream.next() => {
                    let Some(event) = maybe_event else { break; };
                    let html = match event {
                        ChannelEvent::Value(cv) => {
                            ctx_clone.publish_widget_value(&widget_id, cv.clone());
                            let enabled = *enabled_rx.borrow();
                            let html = render_inner_connected(&config, &cv, enabled).into_string();
                            last_cv = Some(cv);
                            html
                        }
                        ChannelEvent::Disconnected(_) | ChannelEvent::Error(_) => {
                            last_cv = None;
                            render_inner_disconnected(&config).into_string()
                        }
                        ChannelEvent::Connected => continue,
                    };
                    if !send_if_changed(&tx, &mut last_html, html) { break; }
                }
                Ok(()) = enabled_rx.changed() => {
                    let enabled = *enabled_rx.borrow();
                    let html = match &last_cv {
                        Some(cv) => render_inner_connected(&config, cv, enabled).into_string(),
                        None => render_inner_disconnected(&config).into_string(),
                    };
                    if !send_if_changed(&tx, &mut last_html, html) { break; }
                }
            }
        }
    }
}

pub fn render_inner_connected(config: &WidgetConfig, cv: &ChannelValue, enabled: bool) -> Markup {
    render_button_html(config, !enabled, &super::tooltips::build_button_tooltip(config, cv))
}

pub fn render_inner_disconnected(config: &WidgetConfig) -> Markup {
    let tooltip = super::tooltips::build_disconnected_tooltip(config);
    render_button_html(config, true, &tooltip)
}

fn render_button_html(
    config: &WidgetConfig,
    disabled: bool,
    tooltip: &str,
) -> Markup {
    html! {
        @let val = config.write_value.unwrap_or(1.0) as i64;
        div class="widget-inner" {
            @if !tooltip.is_empty() {
                (super::tooltips::render_tooltip_info_btn(tooltip))
            }
            button class={
                    "widget-button"
                    @if let Some(c) = &config.color { " widget-button--" (c) }
                }
                disabled[disabled]
                hx-post={"/api/widget/" (config.id) "/set"}
                hx-vals=(format!(r#"{{"value": "{}"}}"#, val))
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
