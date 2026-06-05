use maud::{html, Markup};
use std::sync::Arc;
use futures::StreamExt;
use tokio::time::{Duration, Instant};
use crate::channel::{ChannelContext, ChannelEvent, ChannelValue};
use crate::config::WidgetConfig;

pub struct ToggleButton {
    config: WidgetConfig,
}

impl ToggleButton {
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

}

pub fn render_inner_connected(config: &WidgetConfig, cv: &ChannelValue) -> Markup {
    let is_on = cv.raw_value > 0.5;
    let next_val = if is_on { "0" } else { "1" };
    render_toggle_html(config, is_on, next_val, false, &super::build_tooltip(config, cv), None)
}

pub fn render_inner_disconnected(config: &WidgetConfig) -> Markup {
    render_toggle_html(config, false, "0", true, "", None)
}

fn render_toggle_html(
    config: &WidgetConfig,
    is_on: bool,
    next_val: &str,
    disabled: bool,
    tooltip: &str,
    countdown_secs: Option<u64>,
) -> Markup {
    let btn_class = if is_on {
        "widget-button widget-toggle-btn widget-toggle-btn--on"
    } else {
        "widget-button widget-toggle-btn widget-toggle-btn--off"
    };
    let state_label = if is_on { "ON" } else { "OFF" };

    html! {
        div class="widget-inner" {
            @if !tooltip.is_empty() {
                div class="button-label-row" style="display:flex;align-items:center;gap:0.4rem;margin-bottom:0.5rem;" {
                    span class="widget-label" { (config.label) }
                    (super::render_info_btn(tooltip))
                }
            }
            button class=(btn_class)
                disabled[disabled]
                hx-post={"/api/widget/" (config.id) "/set"}
                hx-vals=(format!(r#"{{"value": "{}"}}"#, next_val))
                hx-target="next .status"
                hx-swap="innerHTML" {
                    span class="widget-toggle-btn-label" { (config.label) ": " (state_label) }
                    @if let Some(seconds) = countdown_secs {
                        span class="widget-toggle-btn-countdown" {
                            (format!("{}s", seconds))
                        }
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

pub fn render_toggle_button(widget: &WidgetConfig) -> Markup {
    html! {
        div style=[super::widget_container_style(widget)]
            data-widget-id=(widget.id)
            data-ch=(widget.channel_address())
            hx-sse=(format!("swap:{}", widget.id)) {
            (render_inner_disconnected(widget))
        }
    }
}

fn render_inner_connected_with_countdown(
    config: &WidgetConfig,
    cv: &ChannelValue,
    countdown_secs: Option<u64>,
) -> Markup {
    let is_on = cv.raw_value > 0.5;
    let next_val = if is_on { "0" } else { "1" };
    render_toggle_html(
        config,
        is_on,
        next_val,
        false,
        &super::build_tooltip(config, cv),
        countdown_secs,
    )
}

impl ToggleButton {
    async fn next_channel_event(
        stream: &mut (impl tokio_stream::Stream<Item = ChannelEvent> + Unpin),
        deadline: Option<Instant>,
    ) -> NextEvent {
        match deadline {
            Some(deadline) => {
                tokio::select! {
                    event = stream.next() => NextEvent::Channel(event),
                    _ = tokio::time::sleep_until(deadline) => NextEvent::Tick,
                }
            }
            None => NextEvent::Channel(stream.next().await),
        }
    }
}

enum NextEvent {
    Channel(Option<ChannelEvent>),
    Tick,
}

impl ToggleButton {
    pub(crate) async fn run_monitor_async(
        config: Arc<WidgetConfig>,
        ctx: Arc<ChannelContext>,
        tx: tokio::sync::mpsc::UnboundedSender<String>,
    ) {
        let mut stream = crate::channel::channel_stream(config.clone(), ctx);
        let mut countdown_end: Option<Instant> = None;
        let mut last_value: Option<ChannelValue> = None;

        loop {
            let next_tick = countdown_end.map(|_| Instant::now() + Duration::from_secs(1));

            match Self::next_channel_event(&mut stream, next_tick).await {
                NextEvent::Channel(None) => break,
                NextEvent::Channel(Some(ChannelEvent::Connected)) => continue,
                NextEvent::Channel(Some(ChannelEvent::Disconnected(_)))
                | NextEvent::Channel(Some(ChannelEvent::Error(_))) => {
                    countdown_end = None;
                    last_value = None;
                    if tx
                        .send(render_inner_disconnected(&config).into_string())
                        .is_err()
                    {
                        break;
                    }
                }
                NextEvent::Channel(Some(ChannelEvent::Value(cv))) => {
                    let is_on = cv.raw_value > 0.5;
                    if is_on {
                        if let Some(timeout_ms) = config.reset_timeout.filter(|ms| *ms > 0) {
                            countdown_end = Some(Instant::now() + Duration::from_millis(timeout_ms));
                        } else {
                            countdown_end = None;
                        }
                    } else {
                        countdown_end = None;
                    }

                    let now = Instant::now();
                    let countdown_secs = countdown_end
                        .and_then(|end| end.checked_duration_since(now))
                        .map(|d| d.as_secs().max(1));

                    last_value = Some(cv.clone());
                    if tx
                        .send(
                            render_inner_connected_with_countdown(&config, &cv, countdown_secs)
                                .into_string(),
                        )
                        .is_err()
                    {
                        break;
                    }
                }
                NextEvent::Tick => {
                    if let (Some(end), Some(cv)) = (countdown_end, last_value.as_ref()) {
                        let now = Instant::now();
                        let countdown_secs = end
                            .checked_duration_since(now)
                            .map(|d| d.as_secs().max(1));

                        if countdown_secs.is_none() {
                            countdown_end = None;
                        }

                        if tx
                            .send(
                                render_inner_connected_with_countdown(&config, cv, countdown_secs)
                                    .into_string(),
                            )
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
        }
    }
}
