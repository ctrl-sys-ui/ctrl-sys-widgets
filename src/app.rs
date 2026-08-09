//! Application state and standard route handlers shared by all mycela apps.
//!
//! Call [`AppState::screen_routes`] to get a router with all config-driven
//! routes pre-wired, then layer on your own custom routes before finalising
//! with `.with_state(state)`.

use crate::{
    channel::ChannelContext,
    config::{AppConfig, WidgetType},
    protocol_control::{self, ProtocolControlError},
    widgets,
};
use axum::{
    extract::{Form, Path, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        Html, IntoResponse, Response,
    },
    routing::{get, post},
    Router,
};
#[cfg(any(feature = "epics-pvxs", feature = "ascii-tcp"))]
use std::sync::Mutex;
use std::sync::Arc;

#[cfg(feature = "epics-pvxs")]
use crate::server_setup::setup_server_pvs;

#[cfg(feature = "epics-pvxs")]
pub type EpicsStartHook =
    Arc<dyn Fn(&AppState, &pvxs::Server) -> Result<(), ProtocolControlError> + Send + Sync>;

#[cfg(feature = "modbus")]
pub type ModbusStartHook = Arc<
    dyn Fn(&AppState) -> Result<Vec<tokio::task::JoinHandle<()>>, ProtocolControlError>
        + Send
        + Sync,
>;

#[cfg(feature = "ascii-tcp")]
pub type AsciiTcpStartHook = Arc<
    dyn Fn(&AppState) -> Result<tokio::task::JoinHandle<()>, ProtocolControlError> + Send + Sync,
>;

// --- Application state -------------------------------------------------------

/// Shared application state threaded through every axum handler.
///
/// Construct this in `main`, set the fields, then call
/// [`AppState::screen_routes`] to build the config-driven router.
#[derive(Clone)]
pub struct AppState {
    /// Loaded application configuration (all screens).
    pub config: Arc<AppConfig>,
    /// Channel context shared by all widget streams.
    pub channel_ctx: Arc<ChannelContext>,
    /// Optional loopback session token for rendering.
    pub loopback_token: Option<String>,
    /// Running PVXS server, if the EPICS feature is enabled.
    #[cfg(feature = "epics-pvxs")]
    pub pv_server: Arc<Mutex<Option<pvxs::Server>>>,
    /// Optional callback to attach app-specific EPICS simulator behavior after server start.
    #[cfg(feature = "epics-pvxs")]
    pub epics_start_hook: Option<EpicsStartHook>,
    #[cfg(feature = "modbus")]
    /// Handles for any background Modbus simulator/connection tasks.
    pub modbus_task: Arc<Mutex<Option<Vec<tokio::task::JoinHandle<()>>>>>,
    /// Optional callback to construct app-specific Modbus tasks when starting Modbus runtime.
    #[cfg(feature = "modbus")]
    pub modbus_start_hook: Option<ModbusStartHook>,
    /// Handle for the background ASCII TCP server task.
    #[cfg(feature = "ascii-tcp")]
    pub ascii_tcp_task: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    /// Optional callback to construct the app-specific ASCII TCP server task.
    #[cfg(feature = "ascii-tcp")]
    pub ascii_tcp_start_hook: Option<AsciiTcpStartHook>,
}

impl AppState {
    /// Returns `true` when the PVXS server is currently running.
    pub fn is_server_running(&self) -> bool {
        #[cfg(feature = "epics-pvxs")]
        {
            return self.pv_server.lock().unwrap().is_some();
        }
        #[allow(unreachable_code)]
        false
    }

    /// Enable or disable a widget by ID.
    ///
    /// The change propagates automatically to any running widget monitor —
    /// the button re-renders as disabled/enabled without any extra wiring.
    ///
    /// ```rust
    /// # use mycela::app::AppState;
    /// # fn update_interlock(state: &AppState) {
    /// state.set_widget_enabled("cmd_fire", false); // locked
    /// state.set_widget_enabled("cmd_fire", true);  // ready
    /// # }
    /// ```
    pub fn set_widget_enabled(&self, widget_id: &str, enabled: bool) {
        self.channel_ctx.set_widget_enabled(widget_id, enabled);
    }

    /// Returns `true` when at least one Modbus task is still alive.
    pub fn is_modbus_running(&self) -> bool {
        #[cfg(feature = "modbus")]
        {
            return self
                .modbus_task
                .lock()
                .unwrap()
                .as_ref()
                .map(|v| v.iter().any(|h| !h.is_finished()))
                .unwrap_or(false);
        }
        #[allow(unreachable_code)]
        false
    }

    /// Returns `true` when the ASCII TCP server task is still alive.
    pub fn is_ascii_tcp_running(&self) -> bool {
        #[cfg(feature = "ascii-tcp")]
        {
            return self
                .ascii_tcp_task
                .lock()
                .unwrap()
                .as_ref()
                .map(|handle| !handle.is_finished())
                .unwrap_or(false);
        }
        #[allow(unreachable_code)]
        false
    }

    /// Build a [`Router`] containing all routes that the config-driven page and
    /// widget system needs.
    ///
    /// Routes included (all derived from `AppConfig`):
    /// - `GET /`                            → home screen
    /// - `GET /screen/{screen_id}`          → render any named screen
    /// - `GET /stream/screen/{screen_id}`   → multiplexed SSE for a screen
    /// - `GET /stream/all`                  → SSE for every widget across all screens
    /// - `GET /stream/widget/{widget_id}`   → SSE for a single widget
    /// - `POST /api/widget/{widget_id}/set` → write a widget value
    ///
    /// Append your own custom routes (simulators, status APIs, static files)
    /// before calling `.with_state(state)`.
    pub fn screen_routes(&self) -> Router<AppState> {
        Router::new()
            .route("/", get(render_home))
            .route("/screen/{screen_id}", get(render_screen))
            .route("/stream/screen/{screen_id}", get(stream_screen_widgets))
            .route("/stream/all", get(stream_all_widgets))
            .route("/stream/widget/{widget_id}", get(stream_widget))
            .route("/api/widget/{widget_id}/set", post(write_widget))
    }

    /// Force a value of a widget by ID.
    ///
    /// The change propagates overriding the UI. 
    /// This can be used to programmatically set a widget value from app logic.
    /// 
    /// Sometimes can be useful to reset a previously set value like a cancelling a button press or resetting a toggle button.
    /// 
    /// This can only be used for widgets that have a `write` protocol configured, otherwise it will have no effect.
    /// ```rust
    /// # use mycela::app::AppState;
    /// # fn reset_command(state: &AppState) {
    /// state.force_widget_value("cmd_arm", 0.0); // force the "cmd_arm" button to its default state
    /// # }
    /// ```
    pub fn force_widget_value(&self, widget_id: &str, value: f64) {
        // Look up the widget config. Use collect_data_widgets so Group children are included.
        let widget = self
            .config
            .screens
            .iter()
            .flat_map(|s| widgets::collect_data_widgets(&s.widgets))
            .find(|w| w.id == widget_id);

        match widget {
            None => {
                tracing::warn!("Widget '{}' not found in config, cannot force value", widget_id);
            }
            Some(w) => match w.widget_type {
                WidgetType::Button | WidgetType::ToggleButton | WidgetType::Slider | WidgetType::Select => {
                    let value_str = match w.widget_type {
                        WidgetType::Button | WidgetType::ToggleButton | WidgetType::Select => {
                            format!("{}", value.trunc() as i64)
                        }
                        _ => format!("{}", value),
                    };
                    let ctx = self.channel_ctx.clone();
                    tracing::info!("Widget '{}' force-writing value {}", w.id, value);
                    tokio::spawn(async move {
                        widgets::write_channel(w, value_str, ctx).await;
                    });
                }
                _ => {
                    tracing::warn!("Widget '{}' is not a writable type, cannot force value", widget_id);
                }
            },
        }
    }
}

// --- SSE type alias ----------------------------------------------------------

pub type SseStream = std::pin::Pin<
    Box<dyn tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>> + Send>,
>;

// --- Widget write ------------------------------------------------------------

pub async fn write_widget(
    Path(widget_id): Path<String>,
    State(state): State<AppState>,
    Form(form): Form<widgets::WriteForm>,
) -> Response {
    let (status, markup) = write_widget_markup(&state, &widget_id, form.value).await;
    (status, Html(markup.into_string())).into_response()
}

pub async fn write_widget_markup(
    state: &AppState,
    widget_id: &str,
    value: String,
) -> (StatusCode, maud::Markup) {
    let widget = state
        .config
        .screens
        .iter()
        .flat_map(|s| widgets::collect_data_widgets(&s.widgets))
        .find(|w| w.id == widget_id);

    match widget {
        None => (
            StatusCode::NOT_FOUND,
            maud::html! {
                span class="write-err" { "Widget '" (widget_id) "' not found" }
            },
        ),
        Some(w) => {
            let enabled = *state.channel_ctx.subscribe_widget_enabled(widget_id).borrow();
            let requested_reset_write = matches!(
                w.widget_type,
                WidgetType::ToggleButton
            ) && value
                .trim()
                .parse::<f64>()
                .ok()
                .and_then(|requested| w.reset_default.map(|reset_default| requested == reset_default || requested == 0.0))
                .unwrap_or(false);

            if !enabled && !requested_reset_write {
                tracing::warn!(
                    "write rejected (disabled): widget_id='{}' value='{}' widget_type='{:?}'",
                    widget_id,
                    value,
                    w.widget_type
                );
                return (
                    StatusCode::FORBIDDEN,
                    maud::html! {
                        span class="write-err" { "Widget is disabled" }
                    },
                );
            }

            let is_writable_widget = matches!(
                w.widget_type,
                WidgetType::Button
                    | WidgetType::ToggleButton
                    | WidgetType::Slider
                    | WidgetType::Select
                    | WidgetType::TextEntry
            );
            let is_local_protocol = matches!(
                w.protocol.as_ref(),
                Some(crate::config::ProtocolConfig::Local(_))
            );
            let is_connected = state.channel_ctx.is_widget_connected(widget_id);
            if is_writable_widget
                && !is_local_protocol
                && !is_connected
                && !requested_reset_write
            {
                tracing::warn!(
                    "write rejected (disconnected): widget_id='{}' value='{}' widget_type='{:?}'",
                    widget_id,
                    value,
                    w.widget_type
                );
                return (
                    StatusCode::FORBIDDEN,
                    maud::html! {
                        span class="write-err" { "Widget is disconnected" }
                    },
                );
            }

            let status = StatusCode::OK;
            // Write the value to the channel and get the updated widget HTML to return in the response.
            let markup =
                widgets::write_channel(w.clone(), value.clone(), state.channel_ctx.clone()).await;
            (status, markup)
        }
    }
}

// --- Home + screen render ----------------------------------------------------

async fn render_home(State(state): State<AppState>) -> Result<Html<String>, StatusCode> {
    let screen = match &state.config.home_screen {
        Some(id) => state.config.screens.iter().find(|s| &s.id == id),
        None => state.config.screens.first(),
    }
    .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Html(
        widgets::render_screen_with_options(screen, true, None, state.loopback_token.as_deref())
            .into_string(),
    ))
}

pub async fn render_screen(
    Path(screen_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Html<String>, StatusCode> {
    tracing::info!("Rendering screen: {}", screen_id);
    let screen = state
        .config
        .screens
        .iter()
        .find(|s| s.id == screen_id)
        .ok_or_else(|| {
            tracing::error!("Screen '{}' not found in AppConfig", screen_id);
            StatusCode::NOT_FOUND
        })?;
    Ok(Html(
        widgets::render_screen_with_options(screen, true, None, state.loopback_token.as_deref())
            .into_string(),
    ))
}

// --- Server control ----------------------------------------------------------

pub async fn stop_server(State(state): State<AppState>) -> Response {
    tracing::info!("POST /api/server/stop");
    stop_server_impl(state).await
}

#[cfg(feature = "epics-pvxs")]
async fn stop_server_impl(state: AppState) -> Response {
    match protocol_control::stop_epics_server(&state).await {
        Ok(()) => Html(
            maud::html! {
                div class="warning" hx-swap-oob="true" id="server-status" {
                    span { "EPICS Server Stopped" }
                }
            }
            .into_string(),
        )
        .into_response(),
        Err(ProtocolControlError::NotRunning(_)) => (
            StatusCode::BAD_REQUEST,
            Html(
                maud::html! { div class="warning" { "EPICS Server is not running" } }.into_string(),
            ),
        )
            .into_response(),
        Err(ProtocolControlError::Operation(e)) => {
            tracing::error!("Failed to stop server: {}", e);
            (
                StatusCode::BAD_REQUEST,
                Html(maud::html! { div class="error" { "Error: " (e.to_string()) } }.into_string()),
            )
                .into_response()
        }
        Err(ProtocolControlError::Internal(e)) => {
            tracing::error!("Server stop task panicked: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html(maud::html! { div class="error" { "Internal error" } }.into_string()),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("Failed to stop server: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html(maud::html! { div class="error" { "Internal error" } }.into_string()),
            )
                .into_response()
        }
    }
}
#[cfg(not(feature = "epics-pvxs"))]
async fn stop_server_impl(_state: AppState) -> Response {
    StatusCode::NOT_IMPLEMENTED.into_response()
}

pub async fn server_status(State(state): State<AppState>) -> Html<String> {
    let is_running = state.is_server_running();
    Html(
        maud::html! {
            div id="server-status" class=(if is_running { "success" } else { "warning" }) {
                span { @if is_running { "EPICS Server Running" } @else { "EPICS Server Stopped" } }
            }
        }
        .into_string(),
    )
}

// --- Modbus control ----------------------------------------------------------

pub async fn stop_modbus(State(state): State<AppState>) -> Response {
    tracing::info!("POST /api/modbus/stop");
    match protocol_control::stop_modbus_tasks(&state) {
        Ok(()) => {
            tracing::info!("Modbus TCP stopped");
            Html(
                maud::html! {
                    div id="modbus-status" class="warning" hx-swap-oob="true" {
                        span { "Modbus TCP Stopped" }
                    }
                }
                .into_string(),
            )
            .into_response()
        }
        Err(ProtocolControlError::NotRunning(_)) => (
            StatusCode::BAD_REQUEST,
            Html(maud::html! { div class="warning" { "Modbus TCP is not running" } }.into_string()),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Failed to stop Modbus TCP: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html(maud::html! { div class="error" { "Internal error" } }.into_string()),
            )
                .into_response()
        }
    }
}

pub async fn modbus_status(State(state): State<AppState>) -> Html<String> {
    let is_running = state.is_modbus_running();
    Html(
        maud::html! {
            div id="modbus-status" class=(if is_running { "success" } else { "warning" }) {
                span { @if is_running { "Modbus TCP Running" } @else { "Modbus TCP Stopped" } }
            }
        }
        .into_string(),
    )
}

// --- ASCII TCP control -------------------------------------------------------

pub async fn start_ascii_tcp(State(state): State<AppState>) -> Response {
    tracing::info!("POST /api/ascii-tcp/start");
    match protocol_control::start_ascii_tcp_runtime(&state) {
        Ok(()) => Html(
            maud::html! {
                div id="ascii-tcp-status" class="success" hx-swap-oob="true" {
                    span { "ASCII TCP Running" }
                }
            }
            .into_string(),
        )
        .into_response(),
        Err(ProtocolControlError::AlreadyRunning(_)) => (
            StatusCode::BAD_REQUEST,
            Html(maud::html! { div class="warning" { "ASCII TCP server is already running" } }.into_string()),
        )
            .into_response(),
        Err(error) => {
            tracing::error!("Failed to start ASCII TCP server: {}", error);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html(maud::html! { div class="error" { "Error: " (error.to_string()) } }.into_string()),
            )
                .into_response()
        }
    }
}

pub async fn stop_ascii_tcp(State(state): State<AppState>) -> Response {
    tracing::info!("POST /api/ascii-tcp/stop");
    match protocol_control::stop_ascii_tcp_runtime(&state) {
        Ok(()) => Html(
            maud::html! {
                div id="ascii-tcp-status" class="warning" hx-swap-oob="true" {
                    span { "ASCII TCP Stopped" }
                }
            }
            .into_string(),
        )
        .into_response(),
        Err(ProtocolControlError::NotRunning(_)) => (
            StatusCode::BAD_REQUEST,
            Html(maud::html! { div class="warning" { "ASCII TCP server is not running" } }.into_string()),
        )
            .into_response(),
        Err(error) => {
            tracing::error!("Failed to stop ASCII TCP server: {}", error);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html(maud::html! { div class="error" { "Error: " (error.to_string()) } }.into_string()),
            )
                .into_response()
        }
    }
}

pub async fn ascii_tcp_status(State(state): State<AppState>) -> Html<String> {
    let is_running = state.is_ascii_tcp_running();
    Html(
        maud::html! {
            div id="ascii-tcp-status" class=(if is_running { "success" } else { "warning" }) {
                span { @if is_running { "ASCII TCP Running" } @else { "ASCII TCP Stopped" } }
            }
        }
        .into_string(),
    )
}

// --- SSE streams -------------------------------------------------------------

pub async fn stream_widget(
    Path(widget_id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    tracing::info!("SSE stream requested for widget: {}", widget_id);
    let data_widgets: Vec<_> = state
        .config
        .screens
        .iter()
        .flat_map(|s| widgets::collect_data_widgets(&s.widgets))
        .collect();
    let Some(config) = data_widgets.into_iter().find(|w| w.id == widget_id) else {
        tracing::error!("Widget '{}' not found", widget_id);
        let stream: SseStream = Box::pin(async_stream::stream! {
            yield Ok(Event::default().data("<!-- widget not found -->"));
        });
        return Sse::new(stream).keep_alive(KeepAlive::default());
    };

    let ctx = state.channel_ctx.clone();
    let stream: SseStream = match config.widget_type {
        WidgetType::TextEntry => {
            Box::pin(widgets::text_entry::TextEntry::new(config).into_sse_stream(ctx))
        }
        WidgetType::TextUpdate => {
            Box::pin(widgets::text_update::TextUpdate::new(config).into_sse_stream(ctx))
        }
        WidgetType::Gauge => Box::pin(widgets::gauge::Gauge::new(config).into_sse_stream(ctx)),
        WidgetType::Led => Box::pin(widgets::led::Led::new(config).into_sse_stream(ctx)),
        WidgetType::Slider => Box::pin(widgets::slider::Slider::new(config).into_sse_stream(ctx)),
        WidgetType::Button => Box::pin(widgets::button::Button::new(config).into_sse_stream(ctx)),
        WidgetType::ToggleButton => Box::pin(widgets::toggle_button::ToggleButton::new(config).into_sse_stream(ctx)),
        WidgetType::Chart => Box::pin(widgets::chart::Chart::new(config).into_sse_stream(ctx)),
        WidgetType::Select => Box::pin(widgets::select::Select::new(config).into_sse_stream(ctx)),
        WidgetType::MultiStateLed => Box::pin(widgets::multi_state_led::MultiStateLed::new(config).into_sse_stream(ctx)),
        WidgetType::Group => {
            let stream: SseStream = Box::pin(async_stream::stream! {
                yield Ok(Event::default().data("<!-- group widget has no stream -->"));
            });
            return Sse::new(stream).keep_alive(KeepAlive::default());
        }
        WidgetType::Hidden => Box::pin(widgets::hidden::Hidden::new(config).into_sse_stream(ctx)),
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}

pub async fn stream_all_widgets(State(state): State<AppState>) -> impl IntoResponse {
    tracing::info!("Multiplexed SSE stream requested for all widgets");
    let (tx, mut rx) = tokio::sync::mpsc::channel::<(String, String)>(64);
    let data_widgets: Vec<_> = state
        .config
        .screens
        .iter()
        .flat_map(|s| widgets::collect_data_widgets(&s.widgets))
        .collect();
    for config in data_widgets {
        let tx = tx.clone();
        let widget_id = config.id.clone();
        let ctx = state.channel_ctx.clone();
        tokio::spawn(widgets::run_widget_monitor_async(
            config, widget_id, ctx, tx,
        ));
    }
    drop(tx);

    let stream: SseStream = Box::pin(async_stream::stream! {
        struct SseDropGuard;
        impl Drop for SseDropGuard {
            fn drop(&mut self) {
                tracing::warn!("SSE stream DROPPED — browser disconnected or connection lost");
            }
        }
        let _guard = SseDropGuard;
        while let Some((widget_id, html)) = rx.recv().await {
            yield Ok(Event::default().event(widget_id).data(html));
        }
        tracing::info!("SSE stream ended normally (all senders dropped)");
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

pub async fn stream_screen_widgets(
    Path(screen_id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    tracing::info!("SSE stream requested for screen: {}", screen_id);

    let Some(screen) = state.config.screens.iter().find(|s| s.id == screen_id) else {
        tracing::error!("Screen '{}' not found for SSE stream", screen_id);
        let stream: SseStream = Box::pin(async_stream::stream! {
            yield Ok(Event::default().data("<!-- screen not found -->"));
        });
        return Sse::new(stream).keep_alive(KeepAlive::default());
    };

    #[cfg(feature = "epics-pvxs")]
    if let Some(server) = state.pv_server.lock().unwrap().as_ref() {
        if let Err(e) = setup_server_pvs(server, &screen.widgets) {
            tracing::warn!("Failed to setup server PVs for screen {}: {}", screen_id, e);
        }
    }

    let (tx, mut rx) = tokio::sync::mpsc::channel::<(String, String)>(64);
    let data_widgets = widgets::collect_data_widgets(&screen.widgets);
    for widget_config in data_widgets {
        let tx = tx.clone();
        let widget_id = widget_config.id.clone();
        let ctx = state.channel_ctx.clone();
        tokio::spawn(widgets::run_widget_monitor_async(
            widget_config,
            widget_id,
            ctx,
            tx,
        ));
    }
    drop(tx);

    let stream: SseStream = Box::pin(async_stream::stream! {
        struct SseDropGuard(String);
        impl Drop for SseDropGuard {
            fn drop(&mut self) {
                tracing::warn!("Screen '{}' SSE stream DROPPED", self.0);
            }
        }
        let _guard = SseDropGuard(screen_id);
        while let Some((widget_id, html)) = rx.recv().await {
            yield Ok(Event::default().event(widget_id).data(html));
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}
