use crate::channel::ChannelContext;
#[cfg(feature = "modbus")]
use crate::config::ModbusTCPConfig;
use crate::config::{ActionConfig, ProtocolConfig, ScreenConfig, WidgetConfig, WidgetType};
use maud::{html, Markup, PreEscaped};
use std::sync::Arc;

#[derive(serde::Deserialize)]
pub struct WriteForm {
    pub value: String,
}

// Base64 encoded SVG icons for different alarm states (shared across all widgets)
pub const OFFLINE_SVG: &str = "data:image/svg+xml;base64,PHN2ZyB2ZXJzaW9uPSIxLjEiIHhtbG5zPSJodHRwOi8vd3d3LnczLm9yZy8yMDAwL3N2ZyIgd2lkdGg9IjI0IiBoZWlnaHQ9IjI0IiB2aWV3Qm94PSIwIDAgMjQgMjQiPjxwYXRoIGZpbGw9IiNmYTAwZmEiIHN0cm9rZT0iI2ZmZiIgc3Ryb2tlLWxpbmVqb2luPSJyb3VuZCIgc3Ryb2tlLWxpbmVjYXA9InJvdW5kIiBzdHJva2UtbWl0ZXJsaW1pdD0iNCIgc3Ryb2tlLXdpZHRoPSIxLjUiIGQ9Ik0yLjc1NyA2LjA5N2MwLTEuODQ1IDEuNDk2LTMuMzQgMy4zNC0zLjM0aDExLjgxOWMxLjg0NSAwIDMuMzQgMS40OTUgMy4zNCAzLjM0djExLjgxOWMwIDEuODQ1LTEuNDk1IDMuMzQtMy4zNCAzLjM0aC0xMS44MTljLTEuODQ1IDAtMy4zNC0xLjQ5NS0zLjM0LTMuMzR2LTExLjgxOXoiPjwvcGF0aD48cGF0aCBmaWxsPSJub25lIiBzdHJva2U9IiNmZmYiIHN0cm9rZS1saW5lam9pbj0icm91bmQiIHN0cm9rZS1saW5lY2FwPSJyb3VuZCIgc3Ryb2tlLW1pdGVybGltaXQ9IjQiIHN0cm9rZS13aWR0aD0iMS41IiBkPSJNMTcuODIgMTQuNDAyYzAuMTE2LTAuMjkzIDAuMTgtMC42MTEgMC4xOC0wLjk0NCAwLTEuMzY3LTEuMDc1LTIuNDktMi40NDgtMi42MTQtMC4yODEtMS42NjEtMS43NjQtMi45MjgtMy41NTItMi45MjgtMC4yNjggMC0wLjUyOSAwLjAyOC0wLjc4IDAuMDgyTTkuMTcyIDkuMjVjLTAuMzY5IDAuNDU0LTAuNjI0IDAuOTk5LTAuNzI1IDEuNTk1LTEuMzczIDAuMTI0LTIuNDQ4IDEuMjQ3LTIuNDQ4IDIuNjE0IDAgMS40NSAxLjIwOSAyLjYyNSAyLjcgMi42MjVoNi42YzAuMjc0IDAgMC41MzgtMC4wMzkgMC43ODctMC4xMTNNNi42IDYuNzVsMTAuOCAxMC41Ij48L3BhdGg+PC9zdmc+";

pub const MAJOR_ALARM_SVG: &str = "data:image/svg+xml;base64,PHN2ZyB3aWR0aD0iMjAiIGhlaWdodD0iMjAiIHZpZXdCb3g9IjAgMCAyMCAyMCIgeG1sbnM9Imh0dHA6Ly93d3cudzMub3JnLzIwMDAvc3ZnIj48Y2lyY2xlIGN4PSIxMCIgY3k9IjEwIiByPSI4IiBmaWxsPSIjZmYwMDAwIi8+PHRleHQgeD0iMTAiIHk9IjE0IiB0ZXh0LWFuY2hvcj0ibWlkZGxlIiBmaWxsPSJ3aGl0ZSIgZm9udC1zaXplPSIxMiIgZm9udC13ZWlnaHQ9ImJvbGQiIGZvbnQtZmFtaWx5PSJBcmlhbCI+ITwvdGV4dD48L3N2Zz4=";

pub const MINOR_ALARM_SVG: &str = "data:image/svg+xml;base64,PHN2ZyB3aWR0aD0iMjAiIGhlaWdodD0iMjAiIHZpZXdCb3g9IjAgMCAyMCAyMCIgeG1sbnM9Imh0dHA6Ly93d3cudzMub3JnLzIwMDAvc3ZnIj48cGF0aCBkPSJNMTAgMyBMMTcgMTYgTDMgMTYgWiIgZmlsbD0iI2ZmYTUwMCIvPjx0ZXh0IHg9IjEwIiB5PSIxNCIgdGV4dC1hbmNob3I9Im1pZGRsZSIgZmlsbD0id2hpdGUiIGZvbnQtc2l6ZT0iMTAiIGZvbnQtd2VpZ2h0PSJib2xkIiBmb250LWZhbWlseT0iQXJpYWwiPiE8L3RleHQ+PC9zdmc+";

pub const INVALID_SVG: &str = "data:image/svg+xml;base64,PHN2ZyB3aWR0aD0iMjAiIGhlaWdodD0iMjAiIHZpZXdCb3g9IjAgMCAyMCAyMCIgeG1sbnM9Imh0dHA6Ly93d3cudzMub3JnLzIwMDAvc3ZnIj48Y2lyY2xlIGN4PSIxMCIgY3k9IjEwIiByPSI4IiBmaWxsPSIjOTk5OTk5Ii8+PHRleHQgeD0iMTAiIHk9IjE0IiB0ZXh0LWFuY2hvcj0ibWlkZGxlIiBmaWxsPSJ3aGl0ZSIgZm9udC1zaXplPSIxMiIgZm9udC13ZWlnaHQ9ImJvbGQiIGZvbnQtZmFtaWx5PSJBcmlhbCI+PzwvdGV4dD48L3N2Zz4=";

pub const INFO_SVG_LIGHT: &str = "data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSIyNCIgaGVpZ2h0PSIyNCIgdmlld0JveD0iMCAwIDI0IDI0Ij4KICA8Y2lyY2xlIGN4PSIxMiIgY3k9IjEyIiByPSIxMCIgc3Ryb2tlPSJibGFjayIgc3Ryb2tlLXdpZHRoPSIyIiBmaWxsPSJub25lIi8+CiAgPHJlY3QgeD0iMTEiIHk9IjEwIiB3aWR0aD0iMiIgaGVpZ2h0PSI3IiBmaWxsPSJibGFjayIvPgogIDxjaXJjbGUgY3g9IjEyIiBjeT0iNyIgcj0iMSIgZmlsbD0iYmxhY2siLz4KPC9zdmc+";

pub const INFO_SVG_DARK: &str = "data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSIyNCIgaGVpZ2h0PSIyNCIgdmlld0JveD0iMCAwIDI0IDI0Ij48Y2lyY2xlIGN4PSIxMiIgY3k9IjEyIiByPSIxMCIgc3Ryb2tlPSJ3aGl0ZSIgc3Ryb2tlLXdpZHRoPSIyIiBmaWxsPSJub25lIi8+PHJlY3QgeD0iMTEiIHk9IjEwIiB3aWR0aD0iMiIgaGVpZ2h0PSI3IiBmaWxsPSJ3aGl0ZSIvPjxjaXJjbGUgY3g9IjEyIiBjeT0iNyIgcj0iMSIgZmlsbD0id2hpdGUiLz48L3N2Zz4=";

// Material Design status icons (new — do not replace the alarm icons above)
/// MD check_circle — green, 20 px — server running / PV connected OK
pub const CHECK_CIRCLE_SVG: &str = "data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjAgMCAyNCAyNCIgd2lkdGg9IjIwIiBoZWlnaHQ9IjIwIj48cGF0aCBmaWxsPSIjMDBjYzY2IiBkPSJNMTIgMkM2LjQ4IDIgMiA2LjQ4IDIgMTJzNC40OCAxMCAxMCAxMCAxMC00LjQ4IDEwLTEwUzE3LjUyIDIgMTIgMnptLTIgMTVsLTUtNSAxLjQxLTEuNDFMMTAgMTQuMTdsNy41OS03LjU5TDE5IDhsLTkgOXoiLz48L3N2Zz4=";

/// MD cancel — red, 20 px — server stopped / error
pub const CANCEL_SVG: &str = "data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjAgMCAyNCAyNCIgd2lkdGg9IjIwIiBoZWlnaHQ9IjIwIj48cGF0aCBmaWxsPSIjZmYzMzMzIiBkPSJNMTIgMkM2LjQ3IDIgMiA2LjQ3IDIgMTJzNC40NyAxMCAxMCAxMCAxMC00LjQ3IDEwLTEwUzE3LjUzIDIgMTIgMnptNSAxMy41OUwxNS41OSAxNyAxMiAxMy40MSA4LjQxIDE3IDcgMTUuNTkgMTAuNTkgMTIgNyA4LjQxIDguNDEgNyAxMiAxMC41OSAxNS41OSA3IDE3IDguNDEgMTMuNDEgMTIgMTcgMTUuNTl6Ii8+PC9zdmc+";

pub mod button;
pub mod chart;
pub mod gauge;
pub mod group;
pub mod hidden;
pub mod led;
pub mod multi_state_led;
pub mod select;
pub mod slider;
pub mod tooltips;
/// MD bolt — white fill, 16 px — button widget action indicator
// pub const BOLT_SVG: &str = "data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjAgMCAyNCAyNCIgd2lkdGg9IjE2IiBoZWlnaHQ9IjE2Ij48cGF0aCBmaWxsPSJ3aGl0ZSIgZD0iTTcgMnYxMWgzdjlsNy0xMmgtNGw0LTh6Ii8+PC9zdmc+";

// Widget type modules
pub mod text_entry;
pub mod text_update;
pub mod toggle_button;

// Re-export widget render functions
pub use button::render_button;
pub use chart::render_chart;
pub use gauge::render_gauge;
pub use group::render_group;
pub use hidden::render_hidden;
pub use led::render_led;
pub use multi_state_led::render_multi_state_led;
pub use select::render_select;
pub use slider::render_slider;
pub use text_entry::render_text_entry;
pub use text_update::render_text_update;
pub use toggle_button::render_toggle_button;

/// Recursively collect all data widgets (non-Group) from a widget tree,
/// flattening children of Group containers so they can each get SSE monitors.
pub fn collect_data_widgets(widgets: &[WidgetConfig]) -> Vec<WidgetConfig> {
    let mut result = Vec::new();
    for w in widgets {
        if w.widget_type == WidgetType::Group {
            if let Some(children) = &w.children {
                result.extend(collect_data_widgets(children));
            }
        } else {
            result.push(w.clone());
        }
    }
    result
}

///
/// Dispatch an async widget monitor based on widget type, sending rendered HTML fragments to the provided channel.
/// Used by the individual `/stream/screen/{screen_id}` SSE endpoint for each widget, and also by the multiplexed `/stream/all` endpoint.
///
/// The widget monitor runs indefinitely, sending updated HTML whenever the widget's data changes.
/// For widgets with user actions (e.g. buttons), the monitor also listens for incoming messages on its channel to receive user input and perform actions.
///
pub async fn run_widget_monitor_html_async(
    config: WidgetConfig,
    ctx: Arc<ChannelContext>,
    tx: tokio::sync::mpsc::UnboundedSender<String>,
) {
    let config = Arc::new(config);
    match config.widget_type {
        WidgetType::TextEntry => text_entry::TextEntry::run_monitor_async(config, ctx, tx).await,
        WidgetType::TextUpdate => text_update::TextUpdate::run_monitor_async(config, ctx, tx).await,
        WidgetType::Gauge => gauge::Gauge::run_monitor_async(config, ctx, tx).await,
        WidgetType::Led => led::Led::run_monitor_async(config, ctx, tx).await,
        WidgetType::Slider => slider::Slider::run_monitor_async(config, ctx, tx).await,
        WidgetType::Button => button::Button::run_monitor_async(config, ctx, tx).await,
        WidgetType::ToggleButton => {
            toggle_button::ToggleButton::run_monitor_async(config, ctx, tx).await
        }
        WidgetType::Chart => chart::Chart::run_monitor_async(config, ctx, tx).await,
        WidgetType::Select => select::Select::run_monitor_async(config, ctx, tx).await,
        WidgetType::Hidden => hidden::Hidden::run_monitor_async(config, ctx, tx).await,
        WidgetType::MultiStateLed => {
            multi_state_led::MultiStateLed::run_monitor_async(config, ctx, tx).await
        }
        WidgetType::Group => {}
    }
}

/// Dispatch an async widget monitor, tagging each HTML fragment with the widget ID.
/// Used by the multiplexed `/stream/all` SSE endpoint.
pub async fn run_widget_monitor_async(
    config: WidgetConfig,
    widget_id: String,
    ctx: Arc<ChannelContext>,
    tx: tokio::sync::mpsc::Sender<(String, String)>,
) {
    let (inner_tx, mut inner_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    // Forward task: tag each HTML fragment with the widget ID and push it to the
    // bounded dispatch channel.  When the channel is full (slow client) the frame
    // is dropped with a debug log rather than accumulating unbounded memory.
    let fwd_wid = widget_id;
    tokio::spawn(async move {
        while let Some(html) = inner_rx.recv().await {
            match tx.try_send((fwd_wid.clone(), html)) {
                Ok(()) => {}
                Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                    tracing::debug!(
                        "SSE dispatch channel full for '{}' — frame dropped",
                        fwd_wid
                    );
                }
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => break,
            }
        }
    });

    run_widget_monitor_html_async(config, ctx, inner_tx).await;
}

/// Render widget from config — each widget's outer div contains its own SSE connection.
pub fn render_widget_from_config(widget: &WidgetConfig) -> Markup {
    match widget.widget_type {
        WidgetType::TextEntry => render_text_entry(widget),
        WidgetType::TextUpdate => render_text_update(widget),
        WidgetType::Gauge => render_gauge(widget),
        WidgetType::Led => render_led(widget),
        WidgetType::Slider => render_slider(widget),
        WidgetType::Button => render_button(widget),
        WidgetType::ToggleButton => render_toggle_button(widget),
        WidgetType::Chart => render_chart(widget),
        WidgetType::Select => render_select(widget),
        WidgetType::Hidden => render_hidden(widget),
        WidgetType::MultiStateLed => render_multi_state_led(widget),
        WidgetType::Group => render_group(widget),
    }
}

/// Render a complete screen from configuration
pub fn render_screen(config: &ScreenConfig) -> Markup {
    render_screen_with_options(config, true, None, None)
}

pub fn render_screen_with_options(
    config: &ScreenConfig,
    enable_streaming: bool,
    ipc_token: Option<&str>,
    loopback_token: Option<&str>,
) -> Markup {
    let nav_base = if ipc_token.is_some() {
        Some("mycela://app")
    } else {
        None
    };
    let has_server_controls = config
        .actions
        .as_ref()
        .map(|actions| {
            actions.iter().any(|action| {
                matches!(action,
            ActionConfig::Api { path, .. } if path.starts_with("/api/server/"))
            })
        })
        .unwrap_or(false);
    let has_modbus_controls = config
        .actions
        .as_ref()
        .map(|actions| {
            actions.iter().any(|action| {
                matches!(action,
            ActionConfig::Api { path, .. } if path.starts_with("/api/modbus/"))
            })
        })
        .unwrap_or(false);

    html! {
        (maud::DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1.0";
                title { (config.title) }

                script src="/static/htmx.min.js" {}
                script src="/static/tooltip.js" {}
                script src="/static/desktop_transport.js" {}
                @if let Some(token) = ipc_token {
                    @let token_json = serde_json::to_string(token)
                        .expect("IPC session token should serialize to JSON string");
                    script { (PreEscaped(format!("window.MYCELA_IPC_TOKEN = {};", token_json))) }
                }
                @if let Some(token) = loopback_token {
                    @let token_json = serde_json::to_string(token)
                        .expect("loopback session token should serialize to JSON string");
                    script { (PreEscaped(format!("window.MYCELA_HTTP_TOKEN = {};", token_json))) }
                }
                link rel="stylesheet" href="/static/style.css";
            }
            body data-myce-screen-id=(config.id) {
                header class="screen-header" {
                    h1 { (config.title) }
                    p class="description" { (config.description) }
                    nav class="screen-actions" {
                        @if let Some(actions) = &config.actions {
                            @for action in actions {
                                (render_action(action, nav_base))
                            }
                        } @else {
                            a href="/" class="back-link" { "← Home" }
                        }
                    }
                    @if has_server_controls || has_modbus_controls {
                        div class="screen-status-strip" {
                            @if has_server_controls {
                                div id="server-status" class="warning screen-status-pill"
                                    data-myce-status-path="/api/server/status"
                                    data-myce-method="get" {
                                    span { "EPICS Server Status" }
                                }
                            }
                            @if has_modbus_controls {
                                div id="modbus-status" class="warning screen-status-pill"
                                    data-myce-status-path="/api/modbus/status"
                                    data-myce-method="get" {
                                    span { "Modbus Status" }
                                }
                            }
                        }
                        div id="screen-action-feedback" class="screen-action-feedback" {}
                    }
                }

                @let num_widgets = collect_data_widgets(&config.widgets).len();
                @let columns = if num_widgets <= 2 { num_widgets } else if num_widgets <= 4 { 2 } else if num_widgets <= 6 { 3 } else { 4 };
                @if enable_streaming {
                    main class="screen-container" hx-sse=(format!("connect:/stream/screen/{}", config.id)) {
                        div class="widget-grid" style=(format!("grid-template-columns: repeat({}, 1fr);", columns)) {
                            @for widget in &config.widgets {
                                (render_widget_from_config(widget))
                            }
                        }
                    }
                } @else {
                    main class="screen-container" {
                        div class="widget-grid" style=(format!("grid-template-columns: repeat({}, 1fr);", columns)) {
                            @for widget in &config.widgets {
                                (render_widget_from_config(widget))
                            }
                        }
                    }
                }

                footer {
                    p class="screen-footer" {
                        "Screen: " (config.id) " | "
                        span class="widget-count" { (config.widgets.len()) " widgets" }
                    }
                }
            }
        }
    }
}

fn render_action(action: &ActionConfig, nav_base: Option<&str>) -> Markup {
    let home_target = "/".to_string();
    match action {
        ActionConfig::Navigate { label, to } => {
            let target = format!("/screen/{}", to);
            html! {
                button class="nav-button" onclick=(format!("window.__MYCELA_NAVIGATE ? window.__MYCELA_NAVIGATE({}) : (window.location={});", serde_json::to_string(&target).unwrap(), serde_json::to_string(&target).unwrap())) { (label) }
            }
        }
        ActionConfig::Back { label } => html! {
            button class="nav-button" onclick=(format!("window.__MYCELA_NAVIGATE ? window.__MYCELA_NAVIGATE({}) : (window.location={});", serde_json::to_string(&home_target).unwrap(), serde_json::to_string(&home_target).unwrap())) { (label) }
        },
        ActionConfig::Popup { label, to } => {
            let target = nav_base
                .map(|base| format!("{}/screen/{}", base, to))
                .unwrap_or_else(|| format!("/screen/{}", to));
            html! {
                button class="nav-button" onclick=(format!("(function(t){{var u=t.startsWith('/') ? (window.location.origin + t) : t; window.open(u,'_blank');}})({});", serde_json::to_string(&target).unwrap())) { (label) }
            }
        }
        ActionConfig::Window { label, to } => {
            let target = nav_base
                .map(|base| format!("{}/screen/{}", base, to))
                .unwrap_or_else(|| format!("/screen/{}", to));
            html! {
                button class="nav-button" onclick=(format!("(function(t){{var u=t.startsWith('/') ? (window.location.origin + t) : t; window.open(u,'_blank','width=1200,height=800,resizable=yes,scrollbars=yes');}})({});", serde_json::to_string(&target).unwrap())) { (label) }
            }
        }
        ActionConfig::Api {
            label,
            method,
            path,
        } => match method.to_lowercase().as_str() {
            "post" => html! {
                button class="nav-button"
                    type="button"
                    data-myce-api-path=(path)
                    data-myce-method="post"
                    data-myce-target="#screen-action-feedback" {
                    (label)
                }
            },
            _ => html! {
                button class="nav-button"
                    type="button"
                    data-myce-api-path=(path)
                    data-myce-method="get"
                    data-myce-target="#screen-action-feedback" {
                    (label)
                }
            },
        },
    }
}

/// Guard for `write_channel`: returns `Some(error_markup)` when the parsed
/// value falls outside the widget's configured control limits, `None` otherwise.
/// Non-numeric strings (booleans, enums) are passed through unchanged.
pub fn check_control_limits(config: &WidgetConfig, value_str: &str) -> Option<Markup> {
    let ctrl = config.metadata.as_ref()?.control.as_ref()?;
    let v: f64 = value_str.trim().parse().ok()?;
    if v < ctrl.limit_low || v > ctrl.limit_high {
        tracing::warn!(
            "[{}] write rejected: {} outside control limits [{}, {}]",
            config.id,
            v,
            ctrl.limit_low,
            ctrl.limit_high
        );
        Some(html! {
            span class="write-err" {
                (v) " outside control range [" (ctrl.limit_low) ", " (ctrl.limit_high) "]"
            }
        })
    } else {
        None
    }
}

/// Thin dispatcher that calls the correct protocol adapter based on `config.protocol`.
/// This will write/put the value on the wire.
///
/// Returns HTML markup indicating:
/// - success ("OK")
/// - External error ("Error: ...")
/// - Internal error ("Internal error")
pub async fn write_channel(
    config: WidgetConfig,
    value_str: String,
    channel_ctx: Arc<ChannelContext>,
) -> Markup {
    if let Some(err) = check_control_limits(&config, &value_str) {
        return err;
    }
    tracing::info!(
        "[{}] write_channel: ch={}, data_type={:?}, value='{}'",
        config.id,
        config.channel_address(),
        config.data_type,
        value_str
    );
    match &config.protocol {
        Some(ProtocolConfig::Local(_)) => {
            match crate::local_channel::local_write(&config, &value_str, &channel_ctx.local_store) {
                Ok(()) => {
                    tracing::info!("[{}] write_channel Local OK", config.id);
                    html! { span class="write-ok" { "OK" } }
                }
                Err(e) => {
                    tracing::error!("[{}] write_channel Local error: {}", config.id, e);
                    html! { span class="write-err" { "Error: " (e) } }
                }
            }
        }
        #[cfg(feature = "epics")]
        Some(ProtocolConfig::EpicsPva(e)) => {
            write_channel_epics(
                &config.id,
                &e.pv_name,
                &config.data_type,
                value_str,
                channel_ctx.epics_ctx.clone(),
            )
            .await
        }
        #[cfg(feature = "modbus")]
        Some(ProtocolConfig::ModbusTcp(m)) => {
            write_channel_modbus(&config.id, m.clone(), value_str, channel_ctx).await
        }
        _ => html! { span class="write-err" { "No protocol configured for this widget" } },
    }
}

#[cfg(feature = "epics")]
async fn write_channel_epics(
    widget_id: &str,
    pv_name: &str,
    data_type: &Option<String>,
    value_str: String,
    write_ctx: Arc<std::sync::Mutex<pvxs_sys::Context>>,
) -> Markup {
    let pv = pv_name.to_string();
    let dt = data_type.clone();
    let result = tokio::task::spawn_blocking(move || -> pvxs_sys::Result<()> {
        let mut ctx = write_ctx.lock().unwrap();
        match dt.as_deref() {
            Some("int32") | Some("int") | Some("integer") | Some("bool") => {
                let v: i32 = value_str.trim().parse().map_err(|_| {
                    pvxs_sys::PvxsError::new(format!("invalid int32: '{}'", value_str.trim()))
                })?;
                ctx.put_int32(&pv, v, 5.0)
            }
            Some("enum") => {
                let v: i16 = value_str.trim().parse().map_err(|_| {
                    pvxs_sys::PvxsError::new(format!("invalid enum index: '{}'", value_str.trim()))
                })?;
                ctx.put_enum(&pv, v, 5.0)
            }
            Some("double") | Some("float") | Some("f64") | Some("f32") => {
                let v: f64 = value_str.trim().parse().map_err(|_| {
                    pvxs_sys::PvxsError::new(format!("invalid float: '{}'", value_str.trim()))
                })?;
                ctx.put_double(&pv, v, 5.0)
            }
            _ => ctx.put_string(&pv, value_str.trim(), 5.0),
        }
    })
    .await;
    match result {
        Ok(Ok(())) => {
            tracing::info!("[{}] write_channel EPICS OK", widget_id);
            html! { span class="write-ok" { "OK" } }
        }
        Ok(Err(e)) => {
            tracing::error!("[{}] write_channel EPICS error: {}", widget_id, e);
            html! { span class="write-err" { "Error: " (e.to_string()) } }
        }
        Err(e) => {
            tracing::error!("[{}] write_channel task panicked: {}", widget_id, e);
            html! { span class="write-err" { "Internal error" } }
        }
    }
}

#[cfg(feature = "modbus")]
async fn write_channel_modbus(
    widget_id: &str,
    m: ModbusTCPConfig,
    value_str: String,
    channel_ctx: Arc<ChannelContext>,
) -> Markup {
    let physical: f64 = match value_str.trim().parse() {
        Ok(v) => v,
        Err(_) => match value_str.trim().to_lowercase().as_str() {
            "true" | "1" | "on" => 1.0,
            "false" | "0" | "off" => 0.0,
            _ => {
                return html! { span class="write-err" { "Invalid value: '" (value_str.trim()) "'" } }
            }
        },
    };
    match crate::modbus_client::modbus_write(&m, physical, &channel_ctx.modbus_pool).await {
        Ok(()) => {
            tracing::info!("[{}] write_channel Modbus OK", widget_id);
            html! { span class="write-ok" { "OK" } }
        }
        Err(e) => {
            tracing::error!("[{}] write_channel Modbus error: {}", widget_id, e);
            html! { span class="write-err" { "Error: " (e) } }
        }
    }
}

/// Map alarm severity to CSS class
pub fn alarm_severity_class(severity: i32) -> &'static str {
    match severity {
        0 => "alarm-none",
        1 => "alarm-minor",
        2 => "alarm-major",
        _ => "alarm-invalid",
    }
}

/// Map alarm status integer to human-readable string (shared across all widgets)
pub fn alarm_status_str(status: i32) -> &'static str {
    match status {
        0 => "No Alarm",
        1 => "Device",
        2 => "Driver",
        3 => "Record",
        4 => "DB",
        5 => "Config",
        6 => "Client",
        _ => "Unknown",
    }
}



/// Build an inline style string from the widget's optional style config (width/height).
/// Always includes `position:relative` so the absolutely-positioned info button
/// anchors to the widget container edge rather than a distant ancestor.
pub fn widget_container_style(config: &crate::config::WidgetConfig) -> Option<String> {
    let mut s = String::from("position:relative;");
    if let Some(style) = &config.style {
        if let Some(w) = &style.width {
            s.push_str(&format!("width:{};", w));
        }
        if let Some(h) = &style.height {
            s.push_str(&format!("height:{};", h));
        }
    }
    Some(s)
}
