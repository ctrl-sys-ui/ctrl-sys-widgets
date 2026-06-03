#[path = "../demo_server/epics_simulator.rs"]
mod epics_simulator;
#[path = "../demo_server/modbus_simulator.rs"]
mod modbus_simulator;
mod assets;

use mycela::app::{
    modbus_status, server_status, stop_modbus, stop_server, AppState,
};
use mycela::axum::{
    extract::{Path, State},
    http::{header, Request as HttpRequest, Response as HttpResponse, StatusCode},
    response::{Html, IntoResponse, Response as AxumResponse},
    routing::{get, post},
    Router,
};
use mycela::channel::ChannelContext;
use mycela::config::{AppConfig, ScreenConfig, WidgetConfig};
use mycela::desktop::{run_desktop, DesktopRuntimeHooks};
use mycela::ipc::{IpcEvent, IpcMessageKind};
use mycela::protocol_control::{self, ProtocolControlError};
use mycela::pvxs_sys;
use mycela::server_setup::setup_server_pvs;
use mycela::{modbus_client, widgets};
use std::borrow::Cow;
use std::sync::{Arc, Mutex, mpsc};

async fn static_file_handler(Path(path): Path<String>) -> impl IntoResponse {
    match assets::get_asset(&path) {
        Some((bytes, content_type)) => {
            ([(header::CONTENT_TYPE, content_type)], bytes).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn start_server(State(state): State<AppState>) -> AxumResponse {
    tracing::info!("POST /api/server/start");
    match protocol_control::start_epics_runtime(&state).await {
        Ok(()) => {
            let html = mycela::maud::html! {
                div class="success" hx-swap-oob="true" id="server-status" {
                    span { "Server Running" }
                }
            };
            Html(html.into_string()).into_response()
        }
        Err(ProtocolControlError::AlreadyRunning(_)) => {
            let html = mycela::maud::html! { div class="warning" { "Server is already running" } };
            (StatusCode::BAD_REQUEST, Html(html.into_string())).into_response()
        }
        Err(ProtocolControlError::Operation(error)) => {
            tracing::error!("Failed to start server: {}", error);
            let html = mycela::maud::html! { div class="error" { "Error: " (error.to_string()) } };
            (StatusCode::BAD_REQUEST, Html(html.into_string())).into_response()
        }
        Err(ProtocolControlError::Internal(error)) => {
            tracing::error!("Server start task panicked: {}", error);
            let html = mycela::maud::html! { div class="error" { "Internal error" } };
            (StatusCode::INTERNAL_SERVER_ERROR, Html(html.into_string())).into_response()
        }
        Err(error) => {
            tracing::error!("Failed to start server: {}", error);
            let html = mycela::maud::html! { div class="error" { "Internal error" } };
            (StatusCode::INTERNAL_SERVER_ERROR, Html(html.into_string())).into_response()
        }
    }
}

async fn start_modbus(State(state): State<AppState>) -> AxumResponse {
    tracing::info!("POST /api/modbus/start");
    match protocol_control::start_modbus_runtime(&state) {
        Ok(()) => {
            tracing::info!("Modbus TCP demo simulator restarted on port 5020");
            let html = mycela::maud::html! {
                div id="modbus-status" class="success" hx-swap-oob="true" {
                    span { "Modbus TCP Running" }
                }
            };
            Html(html.into_string()).into_response()
        }
        Err(ProtocolControlError::AlreadyRunning(_)) => {
            let html = mycela::maud::html! { div class="warning" { "Modbus TCP simulator is already running" } };
            (StatusCode::BAD_REQUEST, Html(html.into_string())).into_response()
        }
        Err(error) => {
            tracing::error!("Failed to start Modbus simulator: {}", error);
            let html = mycela::maud::html! { div class="error" { "Internal error" } };
            (StatusCode::INTERNAL_SERVER_ERROR, Html(html.into_string())).into_response()
        }
    }
}

fn find_home_screen(config: &AppConfig) -> Option<&ScreenConfig> {
    match &config.home_screen {
        Some(id) => config.screens.iter().find(|screen| &screen.id == id),
        None => config.screens.first(),
    }
}

fn render_screen_html(
    screen: &ScreenConfig,
    enable_streaming: bool,
    ipc_token: Option<&str>,
    loopback_token: Option<&str>,
) -> String {
    widgets::render_screen_with_options(screen, enable_streaming, ipc_token, loopback_token)
        .into_string()
}

fn ipc_html_response(html: String) -> HttpResponse<Cow<'static, [u8]>> {
    HttpResponse::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(Cow::Owned(html.into_bytes()))
        .expect("failed to build HTML response")
}

fn ipc_text_response(status: StatusCode, body: &str) -> HttpResponse<Cow<'static, [u8]>> {
    HttpResponse::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Cow::Owned(body.as_bytes().to_vec()))
        .expect("failed to build text response")
}

fn ipc_protocol_response(
    config: &AppConfig,
    session_token: &str,
    request: HttpRequest<Vec<u8>>,
) -> HttpResponse<Cow<'static, [u8]>> {
    let path = request.uri().path();

    if let Some(asset_path) = path.strip_prefix("/static/") {
        return match assets::get_asset(asset_path) {
            Some((bytes, content_type)) => HttpResponse::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, content_type)
                .body(Cow::Borrowed(bytes))
                .expect("failed to build asset response"),
            None => ipc_text_response(StatusCode::NOT_FOUND, "asset not found"),
        };
    }

    match path {
        "/" | "" | "/index.html" => match find_home_screen(config) {
            Some(screen) => {
                ipc_html_response(render_screen_html(screen, false, Some(session_token), None))
            }
            None => ipc_text_response(StatusCode::NOT_FOUND, "home screen not found"),
        },
        _ if path.starts_with("/screen/") => {
            let screen_id = &path["/screen/".len()..];
            match config.screens.iter().find(|screen| screen.id == screen_id) {
                Some(screen) => {
                    ipc_html_response(render_screen_html(screen, false, Some(session_token), None))
                }
                None => ipc_text_response(StatusCode::NOT_FOUND, "screen not found"),
            }
        }
        _ => ipc_text_response(StatusCode::NOT_FOUND, "not found"),
    }
}

fn build_app_state(config: AppConfig, loopback_token: Option<String>) -> AppState {
    let all_widgets: Vec<_> = config
        .screens
        .iter()
        .flat_map(|screen| widgets::collect_data_widgets(&screen.widgets))
        .collect();

    let pv_server = {
        let has_server_pvs = all_widgets
            .iter()
            .any(|widget| widget.epics_pva().and_then(|epics| epics.server.as_ref()).is_some());

        if !has_server_pvs {
            tracing::info!("No server PVs configured, running in client-only mode");
            Arc::new(Mutex::new(None))
        } else {
            let server = pvxs_sys::Server::start_from_env().expect("PVXS server start");
            for screen in &config.screens {
                setup_server_pvs(&server, &screen.widgets).expect("PVXS setup");
            }
            tracing::info!("PVXS server started");
            for screen in &config.screens {
                epics_simulator::start_demo_simulator(server.handle(), &screen.widgets);
            }
            Arc::new(Mutex::new(Some(server)))
        }
    };

    let epics_ctx = Arc::new(Mutex::new(
        pvxs_sys::Context::from_env().expect("PVXS context"),
    ));
    let (sim_h, listener_h) = modbus_simulator::start_modbus_simulator(5020);
    tracing::info!("Modbus TCP simulator started on port 5020");
    let modbus_pool = modbus_client::ModbusPool::new();
    let channel_ctx = ChannelContext::new(epics_ctx, modbus_pool);

    AppState {
        pv_server,
        config: Arc::new(config),
        channel_ctx,
        modbus_task: Arc::new(Mutex::new(Some(vec![sim_h, listener_h]))),
        epics_start_hook: Some(Arc::new(|state, server| {
            for screen in &state.config.screens {
                epics_simulator::start_demo_simulator(server.handle(), &screen.widgets);
            }
            Ok(())
        })),
        modbus_start_hook: Some(Arc::new(|_state| {
            let (sim_h, listener_h) = modbus_simulator::start_modbus_simulator(5020);
            Ok(vec![sim_h, listener_h])
        })),
        loopback_token,
    }
}

fn build_routes(state: AppState) -> Router<AppState> {
    state
        .screen_routes()
        .route("/api/server/start", post(start_server))
        .route("/api/server/stop", post(stop_server))
        .route("/api/server/status", get(server_status))
        .route("/api/modbus/start", post(start_modbus))
        .route("/api/modbus/stop", post(stop_modbus))
        .route("/api/modbus/status", get(modbus_status))
        .route("/static/{*path}", get(static_file_handler))
        .with_state(state)
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn spawn_screen_subscription(
    state: &AppState,
    screen_id: &str,
    event_proxy: mpsc::Sender<IpcEvent>,
) -> Result<Vec<tokio::task::JoinHandle<()>>, String> {
    let Some(screen) = state.config.screens.iter().find(|screen| screen.id == screen_id) else {
        return Err(format!("Screen '{}' not found", screen_id));
    };

    let data_widgets = widgets::collect_data_widgets(&screen.widgets);
    let mut handles = Vec::with_capacity(data_widgets.len());
    for widget_config in data_widgets {
        let widget_id = widget_config.id.clone();
        let ctx = state.channel_ctx.clone();
        let event_proxy = event_proxy.clone();
        handles.push(tokio::spawn(async move {
            let (html_tx, mut html_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
            let monitor = widgets::run_widget_monitor_html_async(widget_config, ctx, html_tx);
            tokio::pin!(monitor);

            loop {
                tokio::select! {
                    _ = &mut monitor => break,
                    maybe_html = html_rx.recv() => {
                        let Some(html) = maybe_html else {
                            break;
                        };

                        let event = IpcEvent {
                            v: 1,
                            kind: IpcMessageKind::Event,
                            event: "widget_html".to_string(),
                            data: serde_json::json!({
                                "widget_id": widget_id,
                                "html": html,
                            }),
                            ts: now_millis(),
                        };

                        if event_proxy.send(event).is_err() {
                            break;
                        }
                    }
                }
            }
        }));
    }

    Ok(handles)
}

fn stop_screen_subscription(handles: Vec<tokio::task::JoinHandle<()>>) {
    for handle in handles {
        handle.abort();
    }
}

fn find_data_widget_by_id(state: &AppState, widget_id: &str) -> Option<WidgetConfig> {
    state
        .config
        .screens
        .iter()
        .flat_map(|screen| widgets::collect_data_widgets(&screen.widgets))
        .find(|widget| widget.id == widget_id)
}

fn spawn_widget_subscription(
    state: &AppState,
    widget_id: &str,
    event_proxy: mpsc::Sender<IpcEvent>,
) -> Result<Vec<tokio::task::JoinHandle<()>>, String> {
    let Some(widget_config) = find_data_widget_by_id(state, widget_id) else {
        return Err(format!("Widget '{}' not found", widget_id));
    };

    let widget_id_owned = widget_config.id.clone();
    let ctx = state.channel_ctx.clone();
    let handle = tokio::spawn(async move {
        let (html_tx, mut html_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let monitor = widgets::run_widget_monitor_html_async(widget_config, ctx, html_tx);
        tokio::pin!(monitor);

        loop {
            tokio::select! {
                _ = &mut monitor => break,
                maybe_html = html_rx.recv() => {
                    let Some(html) = maybe_html else {
                        break;
                    };

                    let event = IpcEvent {
                        v: 1,
                        kind: IpcMessageKind::Event,
                        event: "widget_html".to_string(),
                        data: serde_json::json!({
                            "widget_id": widget_id_owned,
                            "html": html,
                        }),
                        ts: now_millis(),
                    };

                    if event_proxy.send(event).is_err() {
                        break;
                    }
                }
            }
        }
    });

    Ok(vec![handle])
}

fn stop_widget_subscription(handles: Vec<tokio::task::JoinHandle<()>>) {
    for handle in handles {
        handle.abort();
    }
}

fn build_hooks() -> DesktopRuntimeHooks {
    DesktopRuntimeHooks::new(
        build_app_state,
        build_routes,
        ipc_protocol_response,
        spawn_screen_subscription,
        stop_screen_subscription,
        spawn_widget_subscription,
        stop_widget_subscription,
        "/",
    )
}

fn main() {
    let _log_guard = mycela::logging::init_logging(Some(std::path::Path::new("logs")));
    tracing::info!("Starting Mycela Desktop");

    let config: AppConfig = serde_json::from_str(
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/examples/demo_app.json")),
    )
    .expect("embedded demo_app.json is invalid");

    run_desktop(config, build_hooks());
}
