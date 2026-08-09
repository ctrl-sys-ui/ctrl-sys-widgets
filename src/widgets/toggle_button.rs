use crate::channel::{ChannelContext, ChannelEvent, ChannelValue};
use crate::config::WidgetConfig;
use futures::StreamExt;
use maud::{html, Markup};
use std::sync::Arc;
use tokio::sync::watch;
use tokio::time::{Duration, Instant};

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

        tokio::spawn(Self::run_monitor_async(config.clone(), ctx.clone(), tx));

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
    let countdown_secs = (cv.raw_value > 0.5)
        .then(|| config.reset_timeout.filter(|timeout_ms| *timeout_ms > 0))
        .flatten()
        .map(|timeout_ms| timeout_ms.div_ceil(1000));
    render_inner_connected_with_countdown(config, cv, countdown_secs, true)
}

pub fn render_inner_disconnected(config: &WidgetConfig) -> Markup {
    let tooltip = super::tooltips::build_disconnected_tooltip(config);
    render_toggle_html(config, false, "1", true, &tooltip, None)
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
        div class="widget-inner" data-widget-enabled=(if disabled { "false" } else { "true" }) {
            @if !tooltip.is_empty() {
                div class="button-label-row" style="display:flex;align-items:center;gap:0.4rem;margin-bottom:0.5rem;" {
                    span class="widget-label" { (config.label) }
                    (super::tooltips::render_tooltip_info_btn(tooltip))
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
            data-widget-enabled="false"
            hx-sse=(format!("swap:{}", widget.id)) {
            (render_inner_disconnected(widget))
        }
    }
}

fn render_inner_connected_with_countdown(
    config: &WidgetConfig,
    cv: &ChannelValue,
    countdown_secs: Option<u64>,
    enabled: bool,
) -> Markup {
    let is_on = cv.raw_value > 0.5;
    let next_val = if is_on { "0" } else { "1" };
    render_toggle_html(
        config,
        is_on,
        next_val,
        !enabled,
        &super::tooltips::build_button_tooltip(config, cv),
        countdown_secs,
    )
}

impl ToggleButton {
    async fn next_channel_event(
        stream: &mut (impl tokio_stream::Stream<Item = ChannelEvent> + Unpin),
        deadline: Option<Instant>,
        enabled_rx: &mut watch::Receiver<bool>,
    ) -> NextEvent {
        match deadline {
            Some(deadline) => {
                tokio::select! {
                    event = stream.next() => NextEvent::Channel(event),
                    _ = tokio::time::sleep_until(deadline) => NextEvent::Tick,
                    Ok(()) = enabled_rx.changed() => NextEvent::EnabledChanged,
                }
            }
            None => {
                tokio::select! {
                    event = stream.next() => NextEvent::Channel(event),
                    Ok(()) = enabled_rx.changed() => NextEvent::EnabledChanged,
                }
            }
        }
    }
}

enum NextEvent {
    Channel(Option<ChannelEvent>),
    Tick,
    EnabledChanged,
}

fn publish_countdown_secs(ctx: &ChannelContext, toggle_widget_id: &str, secs: u64) {
    let mirror_widget_id = format!("{}_reset_countdown", toggle_widget_id);
    let cv = ChannelValue {
        raw_value: secs as f64,
        value_str: secs.to_string(),
        precision: 0,
        ..ChannelValue::default()
    };
    ctx.publish_widget_value(&mirror_widget_id, cv);
}

impl ToggleButton {
    pub(crate) async fn run_monitor_async(
        config: Arc<WidgetConfig>,
        ctx: Arc<ChannelContext>,
        tx: tokio::sync::mpsc::UnboundedSender<String>,
    ) {
        // Clone ctx so it remains available for reset writes after the stream takes ownership.
        let ctx_clone = ctx.clone();
        let widget_id = config.id.clone();
        let mut enabled_rx = ctx.subscribe_widget_enabled(&config.id);
        let mut stream = crate::channel::channel_stream(config.clone(), ctx.clone());
        let mut countdown_end: Option<Instant> = None;
        let mut current_value: ChannelValue = ChannelValue::default();
        let mut is_connected = false;
        let mut enabled = *enabled_rx.borrow();
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

        let render_and_send = |tx: &tokio::sync::mpsc::UnboundedSender<String>,
                                last_html: &mut String,
                                config: &Arc<WidgetConfig>,
                                current_value: &ChannelValue,
                                connected: bool,
                                enabled: bool,
                                countdown_end: Option<Instant>| {
            let countdown_secs = countdown_end
                .and_then(|end| end.checked_duration_since(Instant::now()))
                .map(|d| d.as_secs().max(1));
            publish_countdown_secs(&ctx, &config.id, countdown_secs.unwrap_or(0));
            let html = if connected {
                render_inner_connected_with_countdown(config, current_value, countdown_secs, enabled)
                    .into_string()
            } else {
                render_inner_disconnected(config).into_string()
            };
            send_if_changed(tx, last_html, html)
        };

        if !render_and_send(
            &tx,
            &mut last_html,
            &config,
            &current_value,
            is_connected,
            enabled,
            countdown_end,
        ) {
            return;
        }

        loop {
            let next_tick = countdown_end.map(|_| Instant::now() + Duration::from_secs(1));

            match Self::next_channel_event(
                &mut stream,
                next_tick,
                &mut enabled_rx,
            )
            .await
            {
                NextEvent::Channel(None) => break,
                NextEvent::Channel(Some(ChannelEvent::Connected)) => {
                    is_connected = true;
                    ctx_clone.set_widget_connected(&widget_id, true);
                    continue;
                }
                NextEvent::Channel(Some(ChannelEvent::Disconnected(_)))
                | NextEvent::Channel(Some(ChannelEvent::Error(_))) => {
                    is_connected = false;
                    ctx_clone.set_widget_connected(&widget_id, false);
                    // Invalidate stale cached value on disconnect.
                    current_value = ChannelValue::default();
                    ctx_clone.publish_widget_value(&widget_id, current_value.clone());
                    countdown_end = None;
                    publish_countdown_secs(&ctx, &config.id, 0);
                    if !render_and_send(
                        &tx,
                        &mut last_html,
                        &config,
                        &current_value,
                        is_connected,
                        enabled,
                        countdown_end,
                    ) {
                        break;
                    }
                }
                NextEvent::Channel(Some(ChannelEvent::Value(cv))) => {
                    is_connected = true;
                    ctx_clone.set_widget_connected(&widget_id, true);
                    ctx_clone.publish_widget_value(&widget_id, cv.clone());
                    let is_on = cv.raw_value > 0.5;
                    if is_on {
                        if let Some(timeout_ms) = config.reset_timeout.filter(|ms| *ms > 0) {
                            countdown_end =
                                Some(Instant::now() + Duration::from_millis(timeout_ms));
                        } else {
                            countdown_end = None;
                        }
                    } else {
                        countdown_end = None;
                    }

                    current_value = cv.clone();
                    enabled = *enabled_rx.borrow();
                    if !render_and_send(
                        &tx,
                        &mut last_html,
                        &config,
                        &current_value,
                        is_connected,
                        enabled,
                        countdown_end,
                    ) {
                        ctx_clone.set_widget_connected(&widget_id, false);
                        break;
                    }
                }
                NextEvent::EnabledChanged => {
                    enabled = *enabled_rx.borrow();
                    if !render_and_send(
                        &tx,
                        &mut last_html,
                        &config,
                        &current_value,
                        is_connected,
                        enabled,
                        countdown_end,
                    ) {
                        ctx_clone.set_widget_connected(&widget_id, false);
                        break;
                    }
                }
                NextEvent::Tick => {
                    if let Some(end) = countdown_end {
                        if end.checked_duration_since(Instant::now()).is_none() {
                            // Countdown has expired.  Write reset_default to the channel so
                            // the PV/register is actually reset.  
                            // This fires unconditionally
                            // — whether the ON state came from a button click or an external
                            // channel event — so the button always resets after the timeout.

                            // Check if value is already at reset_default; if so, don't write again.
                            if Some(current_value.raw_value) == config.reset_default {
                                tracing::info!(
                                    "[{}] toggle countdown expired — already at reset_default={:?}, not writing",
                                    config.id,
                                    config.reset_default
                                );
                            } else {
                                tracing::info!(
                                    "[{}] toggle countdown expired — writing reset_default={:?}",
                                    config.id,
                                    config.reset_default
                                );
                                
                                publish_countdown_secs(&ctx, &config.id, 0);
                                let reset_value = config.reset_default.unwrap_or(0.0).round() as i64;
                                let reset_value_str = reset_value.to_string();
                                let config_write = config.clone();
                                let ctx_write = ctx.clone();
                                tokio::spawn(async move {
                                    tracing::info!(
                                        "[{}] toggle countdown expired — writing reset_default={}",
                                        config_write.id,
                                        reset_value_str
                                    );
                                    let _ = crate::widgets::write_channel(
                                        (*config_write).clone(),
                                        reset_value_str,
                                        ctx_write,
                                    )
                                    .await;
                                });
                            }
                            countdown_end = None;
                            if !render_and_send(
                                &tx,
                                &mut last_html,
                                &config,
                                &current_value,
                                is_connected,
                                enabled,
                                countdown_end,
                            ) {
                                ctx_clone.set_widget_connected(&widget_id, false);
                                break;
                            }
                        } else {
                            enabled = *enabled_rx.borrow();
                            if !render_and_send(
                                &tx,
                                &mut last_html,
                                &config,
                                &current_value,
                                is_connected,
                                enabled,
                                countdown_end,
                            ) {
                                ctx_clone.set_widget_connected(&widget_id, false);
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
}
