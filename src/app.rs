//! Application state and standard route handlers shared by all mycela apps.
//!
//! Call [`AppState::screen_routes`] to get a router with all config-driven
//! routes pre-wired, then layer on your own custom routes before finalising
//! with `.with_state(state)`.

use std::sync::{Arc, Mutex};
use axum::{
    Router,
    routing::{get, post},
    extract::{Path, State, Form},
    response::{Html, IntoResponse, Response, sse::{Event, KeepAlive, Sse}},
    http::StatusCode,
};
use crate::{
    channel::ChannelContext,
    config::{AppConfig, WidgetType},
    protocol_control::{self, ProtocolControlError},
    widgets,
};

#[cfg(feature = "epics")]
use crate::server_setup::setup_server_pvs;

#[cfg(feature = "epics")]
pub type EpicsStartHook = Arc<
    dyn Fn(&AppState, &pvxs_sys::Server) -> Result<(), ProtocolControlError> + Send + Sync,
>;

#[cfg(feature = "modbus")]
pub type ModbusStartHook = Arc<
    dyn Fn(&AppState) -> Result<Vec<tokio::task::JoinHandle<()>>, ProtocolControlError> + Send + Sync,
>;

// --- Application state -------------------------------------------------------

/// Shared application state threaded through every axum handler.
///
/// Construct this in `main`, set the fields, then call
/// [`AppState::screen_routes`] to build the config-driven router.
#[derive(Clone)]
pub struct AppState {
    /// Running PVXS server, if the EPICS feature is enabled.
    #[cfg(feature = "epics")]
    pub pv_server:   Arc<Mutex<Option<pvxs_sys::Server>>>,
    /// Loaded application configuration (all screens).
    pub config:      Arc<AppConfig>,
    /// Channel context shared by all widget streams.
    pub channel_ctx: Arc<ChannelContext>,
    /// Handles for any background Modbus simulator/connection tasks.
    pub modbus_task: Arc<Mutex<Option<Vec<tokio::task::JoinHandle<()>>>>>,
    /// Optional callback to attach app-specific EPICS simulator behavior after server start.
    #[cfg(feature = "epics")]
    pub epics_start_hook: Option<EpicsStartHook>,
    /// Optional callback to construct app-specific Modbus tasks when starting Modbus runtime.
    #[cfg(feature = "modbus")]
    pub modbus_start_hook: Option<ModbusStartHook>,
        /// Optional loopback session token for rendering.
        pub loopback_token: Option<String>,
    
}

impl AppState {
    /// Returns `true` when the EPICS PVA server is currently running.
    pub fn is_server_running(&self) -> bool {
        #[cfg(feature = "epics")]
        { return self.pv_server.lock().unwrap().is_some(); }
        #[allow(unreachable_code)]
        false
    }

    /// Returns `true` when at least one Modbus task is still alive.
    pub fn is_modbus_running(&self) -> bool {
        self.modbus_task.lock().unwrap()
            .as_ref()
            .map(|v| v.iter().any(|h| !h.is_finished()))
            .unwrap_or(false)
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
            .route("/",                              get(render_home))
            .route("/screen/{screen_id}",            get(render_screen))
            .route("/stream/screen/{screen_id}",     get(stream_screen_widgets))
            .route("/stream/all",                    get(stream_all_widgets))
            .route("/stream/widget/{widget_id}",     get(stream_widget))
            .route("/api/widget/{widget_id}/set",    post(write_widget))
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
    let widget = state.config.screens.iter()
        .flat_map(|s| widgets::collect_data_widgets(&s.widgets))
        .find(|w| w.id == widget_id);

    match widget {
        None => (
            StatusCode::NOT_FOUND,
            maud::html! {
                span class="write-err" { "Widget '" (widget_id) "' not found" }
            },
        ),
        Some(w) => (
            StatusCode::OK,
            widgets::write_channel(w, value, state.channel_ctx.clone()).await,
        ),
    }
}

// --- Home + screen render ----------------------------------------------------

async fn render_home(State(state): State<AppState>) -> Result<Html<String>, StatusCode> {
    let screen = match &state.config.home_screen {
        Some(id) => state.config.screens.iter().find(|s| &s.id == id),
        None     => state.config.screens.first(),
    }.ok_or(StatusCode::NOT_FOUND)?;
    Ok(Html(
        widgets::render_screen_with_options(
            screen,
            true,
            None,
            state.loopback_token.as_deref(),
        )
        .into_string(),
    ))
}

pub async fn render_screen(
    Path(screen_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Html<String>, StatusCode> {
    tracing::info!("Rendering screen: {}", screen_id);
    let screen = state.config.screens.iter()
        .find(|s| s.id == screen_id)
        .ok_or_else(|| {
            tracing::error!("Screen '{}' not found in AppConfig", screen_id);
            StatusCode::NOT_FOUND
        })?;
    Ok(Html(
        widgets::render_screen_with_options(
            screen,
            true,
            None,
            state.loopback_token.as_deref(),
        )
        .into_string(),
    ))
}

// --- Server control ----------------------------------------------------------

pub async fn stop_server(State(state): State<AppState>) -> Response {
    tracing::info!("POST /api/server/stop");
    stop_server_impl(state).await
}

#[cfg(feature = "epics")]
async fn stop_server_impl(state: AppState) -> Response {
    match protocol_control::stop_epics_server(&state).await {
        Ok(()) => Html(maud::html! {
            div class="warning" hx-swap-oob="true" id="server-status" {
                span { "EPICS Server Stopped" }
            }
        }.into_string()).into_response(),
        Err(ProtocolControlError::NotRunning(_)) => (StatusCode::BAD_REQUEST, Html(
            maud::html! { div class="warning" { "EPICS Server is not running" } }.into_string()
        )).into_response(),
        Err(ProtocolControlError::Operation(e)) => {
            tracing::error!("Failed to stop server: {}", e);
            (StatusCode::BAD_REQUEST, Html(
                maud::html! { div class="error" { "Error: " (e.to_string()) } }.into_string()
            )).into_response()
        }
        Err(ProtocolControlError::Internal(e)) => {
            tracing::error!("Server stop task panicked: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Html(
                maud::html! { div class="error" { "Internal error" } }.into_string()
            )).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to stop server: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Html(
                maud::html! { div class="error" { "Internal error" } }.into_string()
            )).into_response()
        }
    }
}

#[cfg(not(feature = "epics"))]
async fn stop_server_impl(_state: AppState) -> Response {
    StatusCode::NOT_IMPLEMENTED.into_response()
}

pub async fn server_status(State(state): State<AppState>) -> Html<String> {
    let is_running = state.is_server_running();
    Html(maud::html! {
        div id="server-status" class=(if is_running { "success" } else { "warning" }) {
            span { @if is_running { "EPICS Server Running" } @else { "EPICS Server Stopped" } }
        }
    }.into_string())
}

// --- Modbus control ----------------------------------------------------------

pub async fn stop_modbus(State(state): State<AppState>) -> Response {
    tracing::info!("POST /api/modbus/stop");
    match protocol_control::stop_modbus_tasks(&state) {
        Ok(()) => {
            tracing::info!("Modbus TCP stopped");
            Html(maud::html! {
                div id="modbus-status" class="warning" hx-swap-oob="true" {
                    span { "Modbus TCP Stopped" }
                }
            }.into_string()).into_response()
        }
        Err(ProtocolControlError::NotRunning(_)) => (StatusCode::BAD_REQUEST, Html(
            maud::html! { div class="warning" { "Modbus TCP is not running" } }.into_string()
        )).into_response(),
        Err(e) => {
            tracing::error!("Failed to stop Modbus TCP: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Html(
                maud::html! { div class="error" { "Internal error" } }.into_string()
            )).into_response()
        }
    }
}

pub async fn modbus_status(State(state): State<AppState>) -> Html<String> {
    let is_running = state.is_modbus_running();
    Html(maud::html! {
        div id="modbus-status" class=(if is_running { "success" } else { "warning" }) {
            span { @if is_running { "Modbus TCP Running" } @else { "Modbus TCP Stopped" } }
        }
    }.into_string())
}

// --- SSE streams -------------------------------------------------------------

pub async fn stream_widget(
    Path(widget_id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    tracing::info!("SSE stream requested for widget: {}", widget_id);
    let data_widgets: Vec<_> = state.config.screens.iter()
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
        WidgetType::TextEntry    => Box::pin(widgets::text_entry::TextEntry::new(config).into_sse_stream(ctx)),
        WidgetType::TextUpdate   => Box::pin(widgets::text_update::TextUpdate::new(config).into_sse_stream(ctx)),
        WidgetType::Gauge        => Box::pin(widgets::gauge::Gauge::new(config).into_sse_stream(ctx)),
        WidgetType::Led          => Box::pin(widgets::led::Led::new(config).into_sse_stream(ctx)),
        WidgetType::Slider       => Box::pin(widgets::slider::Slider::new(config).into_sse_stream(ctx)),
        WidgetType::Button       => Box::pin(widgets::button::Button::new(config).into_sse_stream(ctx)),
        WidgetType::ToggleButton => Box::pin(widgets::toggle_button::ToggleButton::new(config).into_sse_stream(ctx)),
        WidgetType::Chart        => Box::pin(widgets::chart::Chart::new(config).into_sse_stream(ctx)),
        WidgetType::Select       => Box::pin(widgets::select::Select::new(config).into_sse_stream(ctx)),
        WidgetType::MultiStateLed => Box::pin(widgets::multi_state_led::MultiStateLed::new(config).into_sse_stream(ctx)),
        WidgetType::Group        => {
            let stream: SseStream = Box::pin(async_stream::stream! {
                yield Ok(Event::default().data("<!-- group widget has no stream -->"));
            });
            return Sse::new(stream).keep_alive(KeepAlive::default());
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}

pub async fn stream_all_widgets(State(state): State<AppState>) -> impl IntoResponse {
    tracing::info!("Multiplexed SSE stream requested for all widgets");
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(String, String)>();
    let data_widgets: Vec<_> = state.config.screens.iter()
        .flat_map(|s| widgets::collect_data_widgets(&s.widgets))
        .collect();
    for config in data_widgets {
        let tx        = tx.clone();
        let widget_id = config.id.clone();
        let ctx       = state.channel_ctx.clone();
        tokio::spawn(widgets::run_widget_monitor_async(config, widget_id, ctx, tx));
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

    #[cfg(feature = "epics")]
    if let Some(server) = state.pv_server.lock().unwrap().as_ref() {
        if let Err(e) = setup_server_pvs(server, &screen.widgets) {
            tracing::warn!("Failed to setup server PVs for screen {}: {}", screen_id, e);
        }
    }

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(String, String)>();
    let data_widgets = widgets::collect_data_widgets(&screen.widgets);
    for widget_config in data_widgets {
        let tx        = tx.clone();
        let widget_id = widget_config.id.clone();
        let ctx       = state.channel_ctx.clone();
        tokio::spawn(widgets::run_widget_monitor_async(widget_config, widget_id, ctx, tx));
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
