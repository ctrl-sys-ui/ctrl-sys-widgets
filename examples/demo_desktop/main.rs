#[path = "../demo_server/epics_simulator.rs"]
mod epics_simulator;
#[path = "../demo_server/modbus_simulator.rs"]
mod modbus_simulator;
mod assets;

use mycela::app::{
    AppState,
    stop_server, server_status, stop_modbus, modbus_status,
};
use mycela::axum::{
    body::Body,
    routing::{get, post},
    extract::{Path, State},
    response::{Html, IntoResponse, Response as AxumResponse},
    middleware,
    http::{Method, Request as HttpRequest, Response as HttpResponse, StatusCode, header},
};
use mycela::tower_http::{trace::TraceLayer, cors::CorsLayer};
use mycela::maud::{self};
use mycela::winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    window::{Window, WindowId},
};
use mycela::wry::{WebViewBuilder, NewWindowFeatures, NewWindowResponse};
use mycela::pvxs_sys;
use mycela::channel::ChannelContext;
use mycela::config::AppConfig;
use mycela::desktop_transport::{DESKTOP_TRANSPORT_ENV, DesktopTransport};
use mycela::ipc::{IpcCommand, IpcEvent, IpcMessageKind, IpcRequest, IpcResponse};
use mycela::ipc_dispatch;
use mycela::protocol_control::{self, ProtocolControlError};
use mycela::server_setup::setup_server_pvs;
use mycela::{modbus_client, widgets};
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, mpsc};

// --- Static file handler (embedded assets) -----------------------------------

async fn static_file_handler(Path(path): Path<String>) -> impl IntoResponse {
    match assets::get_asset(&path) {
        Some((bytes, content_type)) => {
            ([(header::CONTENT_TYPE, content_type)], bytes).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

// --- API handlers ------------------------------------------------------------

async fn start_server(State(state): State<AppState>) -> AxumResponse {
    tracing::info!("POST /api/server/start");
    match protocol_control::start_epics_runtime(&state).await {
        Ok(()) => {
            let html = maud::html! {
                div class="success" hx-swap-oob="true" id="server-status" {
                    span { "Server Running" }
                }
            };
            Html(html.into_string()).into_response()
        }
        Err(ProtocolControlError::AlreadyRunning(_)) => {
            let html = maud::html! { div class="warning" { "Server is already running" } };
            (StatusCode::BAD_REQUEST, Html(html.into_string())).into_response()
        }
        Err(ProtocolControlError::Operation(e)) => {
            tracing::error!("Failed to start server: {}", e);
            let html = maud::html! { div class="error" { "Error: " (e.to_string()) } };
            (StatusCode::BAD_REQUEST, Html(html.into_string())).into_response()
        }
        Err(ProtocolControlError::Internal(e)) => {
            tracing::error!("Server start task panicked: {}", e);
            let html = maud::html! { div class="error" { "Internal error" } };
            (StatusCode::INTERNAL_SERVER_ERROR, Html(html.into_string())).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to start server: {}", e);
            let html = maud::html! { div class="error" { "Internal error" } };
            (StatusCode::INTERNAL_SERVER_ERROR, Html(html.into_string())).into_response()
        }
    }
}

async fn start_modbus(State(state): State<AppState>) -> AxumResponse {
    tracing::info!("POST /api/modbus/start");
    match protocol_control::start_modbus_runtime(&state) {
        Ok(()) => {
            tracing::info!("Modbus TCP demo simulator restarted on port 5020");
            let html = maud::html! {
                div id="modbus-status" class="success" hx-swap-oob="true" {
                    span { "Modbus TCP Running" }
                }
            };
            Html(html.into_string()).into_response()
        }
        Err(ProtocolControlError::AlreadyRunning(_)) => {
            let html = maud::html! { div class="warning" { "Modbus TCP simulator is already running" } };
            (StatusCode::BAD_REQUEST, Html(html.into_string())).into_response()
        }
        Err(e) => {
            tracing::error!("Failed to start Modbus simulator: {}", e);
            let html = maud::html! { div class="error" { "Internal error" } };
            (StatusCode::INTERNAL_SERVER_ERROR, Html(html.into_string())).into_response()
        }
    }
}

// --- Desktop window ----------------------------------------------------------

enum DesktopUserEvent {
    OpenWindow(String),
    IpcMessage(String),
    IpcEvent(IpcEvent),
}

struct BackendRequest {
    request: IpcRequest,
    response_tx: mpsc::Sender<IpcResponse>,
}

struct DesktopApp {
    transport: DesktopTransport,
    base_url: Option<String>,
    windows:  Vec<(Window, wry::WebView)>,
    proxy:    EventLoopProxy<DesktopUserEvent>,
    backend_tx: Option<mpsc::Sender<BackendRequest>>,
    ipc_config: Option<Arc<AppConfig>>,
    ipc_session_token: Option<String>,
}

fn is_allowed_loopback_window_target(target_url: &str, base_url: &str) -> bool {
    let origin_prefix = base_url.trim_end_matches('/');
    target_url.starts_with(origin_prefix)
}

impl DesktopApp {
    fn create_window(&mut self, event_loop: &ActiveEventLoop, url: Option<&str>) {
        let window = event_loop
            .create_window(Window::default_attributes().with_title("Mycela"))
            .expect("failed to create window");
        let proxy = self.proxy.clone();
        let webview = match self.transport {
            DesktopTransport::Loopback => {
                let allow_base_url = self
                    .base_url
                    .clone()
                    .expect("loopback desktop requires base URL");
                WebViewBuilder::new()
                    .with_url(url.expect("loopback window requires URL"))
                    .with_new_window_req_handler(move |u, _features: NewWindowFeatures| {
                        if is_allowed_loopback_window_target(&u, &allow_base_url) {
                            let _ = proxy.send_event(DesktopUserEvent::OpenWindow(u));
                        } else {
                            tracing::warn!(
                                "Blocked loopback-mode window.open request to {} (allowed base: {})",
                                u,
                                allow_base_url
                            );
                        }
                        NewWindowResponse::Deny
                    })
                    .build(&window)
                    .expect("failed to create webview")
            }
            DesktopTransport::Ipc => {
                let ipc_url = url.expect("ipc window requires URL");
                let config = self
                    .ipc_config
                    .clone()
                    .expect("ipc desktop requires UI config");
                let session_token = self
                    .ipc_session_token
                    .clone()
                    .expect("ipc desktop requires session token");
                let new_window_proxy = self.proxy.clone();

                WebViewBuilder::new()
                    .with_custom_protocol("mycela".into(), move |_webview_id, request| {
                        ipc_protocol_response(config.as_ref(), &session_token, request)
                    })
                    .with_url(ipc_url)
                    .with_ipc_handler(move |request| {
                        let _ = proxy.send_event(DesktopUserEvent::IpcMessage(request.body().clone()));
                    })
                    .with_new_window_req_handler(move |u, _features: NewWindowFeatures| {
                        if u.starts_with("mycela://") {
                            let _ = new_window_proxy.send_event(DesktopUserEvent::OpenWindow(u));
                        } else {
                            tracing::warn!("Blocked IPC-mode window.open request to {}", u);
                        }
                        NewWindowResponse::Deny
                    })
                    .build(&window)
                    .expect("failed to create webview")
            }
        };
        self.windows.push((window, webview));
    }

    fn handle_ipc_message(&mut self, payload: String) {
        tracing::debug!("IPC <- UI raw: {}", payload);

        let request = match serde_json::from_str::<IpcRequest>(&payload) {
            Ok(request) => request,
            Err(error) => {
                tracing::error!("Failed to parse IPC request: {}", error);
                return;
            }
        };

            tracing::debug!(
            "IPC <- UI request: id='{}' cmd='{:?}' kind='{:?}'",
            request.id,
            request.cmd,
            request.kind
        );

        let Some(backend_tx) = &self.backend_tx else {
            tracing::warn!("Dropped IPC request because no backend is configured");
            return;
        };

        let (response_tx, response_rx) = mpsc::channel();
        if let Err(error) = backend_tx.send(BackendRequest { request, response_tx }) {
            tracing::error!("Failed to send IPC request to backend: {}", error);
            return;
        }

        let response = match response_rx.recv() {
            Ok(response) => response,
            Err(error) => {
                tracing::error!("Failed to receive IPC response from backend: {}", error);
                return;
            }
        };

        let response_json = match serde_json::to_string(&response) {
            Ok(json) => json,
            Err(error) => {
                tracing::error!("Failed to serialize IPC response: {}", error);
                return;
            }
        };
        tracing::debug!("IPC -> UI response: {}", response_json);
        let script = format!("window.__MYCELA_IPC_DELIVER({});", response_json);

        for (_, webview) in &self.windows {
            if let Err(error) = webview.evaluate_script(&script) {
                tracing::error!("Failed to deliver IPC response to webview: {}", error);
            }
        }
    }

    fn handle_ipc_event(&mut self, event: IpcEvent) {
        let event_json = match serde_json::to_string(&event) {
            Ok(json) => json,
            Err(error) => {
                tracing::error!("Failed to serialize IPC event: {}", error);
                return;
            }
        };
            tracing::debug!("IPC -> UI event: {}", event_json);
        let script = format!("window.__MYCELA_IPC_EVENT_DELIVER({});", event_json);

        for (_, webview) in &self.windows {
            if let Err(error) = webview.evaluate_script(&script) {
                tracing::error!("Failed to deliver IPC event to webview: {}", error);
            }
        }
    }
}

impl ApplicationHandler<DesktopUserEvent> for DesktopApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if !self.windows.is_empty() {
            return;
        }
        let url = self.base_url.clone();
        self.create_window(event_loop, url.as_deref());
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: DesktopUserEvent) {
        match event {
            DesktopUserEvent::OpenWindow(url) => {
                if self.transport == DesktopTransport::Loopback {
                    if let Some(base_url) = self.base_url.as_deref() {
                        if is_allowed_loopback_window_target(&url, base_url) {
                            self.create_window(event_loop, Some(&url));
                        } else {
                            tracing::warn!(
                                "Ignored loopback-mode new window request outside base origin: {}",
                                url
                            );
                        }
                    } else {
                        tracing::warn!(
                            "Ignored loopback-mode new window request without base URL configured: {}",
                            url
                        );
                    }
                } else if url.starts_with("mycela://") {
                    self.create_window(event_loop, Some(&url));
                } else {
                    tracing::warn!("Ignored new window request in IPC mode: {}", url);
                }
            }
            DesktopUserEvent::IpcMessage(payload) => self.handle_ipc_message(payload),
            DesktopUserEvent::IpcEvent(event) => self.handle_ipc_event(event),
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        id: WindowId,
        event: WindowEvent,
    ) {
        if matches!(event, WindowEvent::CloseRequested) {
            self.windows.retain(|(w, _)| w.id() != id);
            if self.windows.is_empty() {
                event_loop.exit();
            }
        }
    }
}

fn find_home_screen(config: &AppConfig) -> Option<&mycela::config::ScreenConfig> {
    match &config.home_screen {
        Some(id) => config.screens.iter().find(|screen| &screen.id == id),
        None => config.screens.first(),
    }
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
            Some(screen) => ipc_html_response(
                widgets::render_screen_with_options(screen, false, Some(session_token), None)
                    .into_string(),
            ),
            None => ipc_text_response(StatusCode::NOT_FOUND, "home screen not found"),
        },
        _ if path.starts_with("/screen/") => {
            let screen_id = &path["/screen/".len()..];
            match config.screens.iter().find(|screen| screen.id == screen_id) {
                Some(screen) => ipc_html_response(
                    widgets::render_screen_with_options(screen, false, Some(session_token), None)
                        .into_string(),
                ),
                None => ipc_text_response(StatusCode::NOT_FOUND, "screen not found"),
            }
        }
        _ => ipc_text_response(StatusCode::NOT_FOUND, "not found"),
    }
}

fn build_app_state(config: AppConfig, loopback_token: Option<String>) -> AppState {
        let all_widgets: Vec<_> = config.screens.iter()
                .flat_map(|s| widgets::collect_data_widgets(&s.widgets))
                .collect();
        let pv_server = {
                let has_server_pvs = all_widgets.iter()
                        .any(|w| w.epics_pva().and_then(|e| e.server.as_ref()).is_some());
                if !has_server_pvs {
                        tracing::info!("No server PVs configured, running in client-only mode");
                        Arc::new(Mutex::new(None))
                } else {
                        let server = pvxs_sys::Server::start_from_env()
                                .expect("PVXS server start");
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
            loopback_token,
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
        }
}

fn screen_subscription_response(id: &str, ok: bool, message: Option<&str>) -> IpcResponse {
    IpcResponse {
        v: 1,
        kind: IpcMessageKind::Response,
        id: id.to_string(),
        ok,
        result: Some(serde_json::json!({ "subscribed": ok })),
        error: message.map(|msg| mycela::ipc::IpcError {
            code: mycela::ipc::IpcErrorCode::PayloadInvalid,
            message: msg.to_string(),
            details: None,
        }),
        ts: chrono::Utc::now().timestamp_millis(),
    }
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
    tracing::debug!(
        "Preparing screen subscription '{}' with {} widget monitors",
        screen_id,
        data_widgets.len()
    );
    let mut handles = Vec::with_capacity(data_widgets.len());
    for widget_config in data_widgets {
        tracing::debug!(
            "Screen '{}' spawning widget monitor: widget='{}' channel='{}' type='{:?}'",
            screen_id,
            widget_config.id,
            widget_config.channel_address(),
            widget_config.widget_type
        );
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
                            ts: chrono::Utc::now().timestamp_millis(),
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

fn find_data_widget_by_id(state: &AppState, widget_id: &str) -> Option<mycela::config::WidgetConfig> {
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

    tracing::debug!(
        "Spawning protocol subscription monitor: widget='{}' channel='{}' type='{:?}'",
        widget_config.id,
        widget_config.channel_address(),
        widget_config.widget_type
    );

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
                        ts: chrono::Utc::now().timestamp_millis(),
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

fn release_widget_subscription(
    widget_subscriptions: &mut HashMap<String, (usize, Vec<tokio::task::JoinHandle<()>>)>,
    widget_id: &str,
) {
    let remove_entry = match widget_subscriptions.get_mut(widget_id) {
        Some((count, _)) if *count > 1 => {
            *count -= 1;
            false
        }
        Some(_) => true,
        None => false,
    };

    if remove_entry {
        if let Some((_count, handles)) = widget_subscriptions.remove(widget_id) {
            tracing::debug!(
                "Stopping shared protocol monitor for widget '{}' ({} task(s))",
                widget_id,
                handles.len()
            );
            stop_widget_subscription(handles);
        }
    }
}

fn release_screen_subscription(
    screen_subscriptions: &mut HashMap<String, (usize, Vec<tokio::task::JoinHandle<()>>)>,
    screen_id: &str,
) {
    let remove_entry = match screen_subscriptions.get_mut(screen_id) {
        Some((count, _)) if *count > 1 => {
            *count -= 1;
            false
        }
        Some(_) => true,
        None => false,
    };

    if remove_entry {
        if let Some((_count, handles)) = screen_subscriptions.remove(screen_id) {
            tracing::debug!(
                "Stopping shared monitor set for screen '{}' ({} widget tasks)",
                screen_id,
                handles.len()
            );
            stop_screen_subscription(handles);
        }
    }
}

fn spawn_ipc_backend(
    config: AppConfig,
    session_token: String,
    proxy: EventLoopProxy<DesktopUserEvent>,
) -> mpsc::Sender<BackendRequest> {
        let (backend_tx, backend_rx) = mpsc::channel::<BackendRequest>();

        std::thread::spawn(move || {
                let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
                let state = runtime.block_on(async move { build_app_state(config, None) });
                let (event_tx, event_rx) = mpsc::channel::<IpcEvent>();
                let mut subscription_to_screen = HashMap::<String, String>::new();
                let mut screen_subscriptions = HashMap::<String, (usize, Vec<tokio::task::JoinHandle<()>>)>::new();
                let mut subscription_to_widget = HashMap::<String, String>::new();
                let mut widget_subscriptions = HashMap::<String, (usize, Vec<tokio::task::JoinHandle<()>>)>::new();
                let proxy_clone = proxy.clone();

                std::thread::spawn(move || {
                    while let Ok(event) = event_rx.recv() {
                        if let Err(error) = proxy_clone.send_event(DesktopUserEvent::IpcEvent(event)) {
                            tracing::error!("Failed to forward IPC event to UI thread: {}", error);
                            break;
                        }
                    }
                });

                while let Ok(backend_request) = backend_rx.recv() {
                        let response = if backend_request.request.cmd == IpcCommand::AppScreenSubscribe {
                                let screen_id = backend_request.request.payload.get("screen_id")
                                    .and_then(|value| value.as_str())
                                    .map(str::to_string);
                                let subscription_id = backend_request.request.payload.get("subscription_id")
                                    .and_then(|value| value.as_str())
                                    .map(str::to_string);

                                match (screen_id, subscription_id) {
                                    (Some(screen_id), Some(subscription_id)) => {
                                        if let Some(previous_screen_id) = subscription_to_screen.remove(&subscription_id) {
                                            tracing::debug!(
                                                "Replacing existing IPC subscription '{}' for screen '{}'",
                                                subscription_id,
                                                previous_screen_id
                                            );
                                            release_screen_subscription(&mut screen_subscriptions, &previous_screen_id);
                                        }

                                        if let Some((count, _)) = screen_subscriptions.get_mut(&screen_id) {
                                            *count += 1;
                                            tracing::debug!(
                                                "Reusing shared monitor set for screen '{}' (ref_count={}) via subscription '{}'",
                                                screen_id,
                                                *count,
                                                subscription_id
                                            );
                                            subscription_to_screen.insert(subscription_id, screen_id);
                                            screen_subscription_response(&backend_request.request.id, true, None)
                                        } else {
                                            match runtime.block_on(async {
                                                spawn_screen_subscription(&state, &screen_id, event_tx.clone())
                                            }) {
                                                Ok(handles) => {
                                                    let task_count = handles.len();
                                                    screen_subscriptions.insert(screen_id.clone(), (1, handles));
                                                    subscription_to_screen.insert(subscription_id.clone(), screen_id.clone());
                                                    tracing::debug!(
                                                        "Started shared monitor set for screen '{}' via subscription '{}' ({} widget tasks)",
                                                        screen_id,
                                                        subscription_id,
                                                        task_count
                                                    );
                                                    screen_subscription_response(&backend_request.request.id, true, None)
                                                }
                                                Err(error) => screen_subscription_response(&backend_request.request.id, false, Some(&error)),
                                            }
                                        }
                                    }
                                    _ => screen_subscription_response(&backend_request.request.id, false, Some("Missing screen_id or subscription_id")),
                                }
                        } else if backend_request.request.cmd == IpcCommand::AppScreenUnsubscribe {
                                let subscription_id = backend_request.request.payload.get("subscription_id")
                                    .and_then(|value| value.as_str())
                                    .map(str::to_string);

                                match subscription_id {
                                    Some(subscription_id) => {
                                        if let Some(screen_id) = subscription_to_screen.remove(&subscription_id) {
                                                tracing::debug!(
                                                "Unsubscribed IPC subscription '{}' for screen '{}'",
                                                subscription_id,
                                                screen_id
                                                );
                                            release_screen_subscription(&mut screen_subscriptions, &screen_id);
                                        } else {
                                            tracing::debug!(
                                                "Unsubscribe requested for unknown IPC subscription '{}'",
                                                subscription_id
                                            );
                                        }
                                        screen_subscription_response(&backend_request.request.id, true, None)
                                    }
                                    None => screen_subscription_response(&backend_request.request.id, false, Some("Missing subscription_id")),
                                }
                        } else if matches!(
                            backend_request.request.cmd,
                            IpcCommand::EpicsPvSubscribe | IpcCommand::ModbusSubscribe
                        ) {
                                let widget_id = backend_request.request.payload.get("widget_id")
                                    .and_then(|value| value.as_str())
                                    .map(str::to_string);
                                let subscription_id = backend_request.request.payload.get("subscription_id")
                                    .and_then(|value| value.as_str())
                                    .map(str::to_string);

                                match (widget_id, subscription_id) {
                                    (Some(widget_id), Some(subscription_id)) => {
                                        if let Some(previous_widget_id) = subscription_to_widget.remove(&subscription_id) {
                                            tracing::debug!(
                                                "Replacing existing protocol subscription '{}' for widget '{}'",
                                                subscription_id,
                                                previous_widget_id
                                            );
                                            release_widget_subscription(&mut widget_subscriptions, &previous_widget_id);
                                        }

                                        if let Some((count, _)) = widget_subscriptions.get_mut(&widget_id) {
                                            *count += 1;
                                            tracing::debug!(
                                                "Reusing shared protocol monitor for widget '{}' (ref_count={}) via subscription '{}'",
                                                widget_id,
                                                *count,
                                                subscription_id
                                            );
                                            subscription_to_widget.insert(subscription_id, widget_id);
                                            screen_subscription_response(&backend_request.request.id, true, None)
                                        } else {
                                            match runtime.block_on(async {
                                                spawn_widget_subscription(&state, &widget_id, event_tx.clone())
                                            }) {
                                                Ok(handles) => {
                                                    let task_count = handles.len();
                                                    widget_subscriptions.insert(widget_id.clone(), (1, handles));
                                                    subscription_to_widget.insert(subscription_id.clone(), widget_id.clone());
                                                    tracing::debug!(
                                                        "Started shared protocol monitor for widget '{}' via subscription '{}' ({} task(s))",
                                                        widget_id,
                                                        subscription_id,
                                                        task_count
                                                    );
                                                    screen_subscription_response(&backend_request.request.id, true, None)
                                                }
                                                Err(error) => screen_subscription_response(&backend_request.request.id, false, Some(&error)),
                                            }
                                        }
                                    }
                                    _ => screen_subscription_response(&backend_request.request.id, false, Some("Missing widget_id or subscription_id")),
                                }
                        } else if matches!(
                            backend_request.request.cmd,
                            IpcCommand::EpicsPvUnsubscribe | IpcCommand::ModbusUnsubscribe
                        ) {
                                let subscription_id = backend_request.request.payload.get("subscription_id")
                                    .and_then(|value| value.as_str())
                                    .map(str::to_string);

                                match subscription_id {
                                    Some(subscription_id) => {
                                        if let Some(widget_id) = subscription_to_widget.remove(&subscription_id) {
                                            tracing::debug!(
                                                "Unsubscribed protocol subscription '{}' for widget '{}'",
                                                subscription_id,
                                                widget_id
                                            );
                                            release_widget_subscription(&mut widget_subscriptions, &widget_id);
                                        } else {
                                            tracing::debug!(
                                                "Unsubscribe requested for unknown protocol subscription '{}'",
                                                subscription_id
                                            );
                                        }
                                        screen_subscription_response(&backend_request.request.id, true, None)
                                    }
                                    None => screen_subscription_response(&backend_request.request.id, false, Some("Missing subscription_id")),
                                }
                        } else {
                            runtime.block_on(ipc_dispatch::dispatch_request(
                                    &state,
                                    backend_request.request,
                            Some(&session_token),
                            ))
                        };
                        if let Err(error) = backend_request.response_tx.send(response) {
                                tracing::error!("Failed to send IPC response to UI thread: {}", error);
                        }
                }

                for (subscription_id, screen_id) in subscription_to_screen.drain() {
                    tracing::debug!(
                        "Cleaning up IPC subscription '{}' for screen '{}' on backend shutdown",
                        subscription_id,
                        screen_id
                    );
                }

                for (subscription_id, widget_id) in subscription_to_widget.drain() {
                    tracing::debug!(
                        "Cleaning up protocol subscription '{}' for widget '{}' on backend shutdown",
                        subscription_id,
                        widget_id
                    );
                }

                let remaining_screen_subscriptions: Vec<_> = screen_subscriptions.drain().collect();
                let remaining_widget_subscriptions: Vec<_> = widget_subscriptions.drain().collect();

                for (screen_id, (_count, handles)) in remaining_screen_subscriptions {
                    tracing::debug!(
                        "Cleaning up shared monitor set for screen '{}' on backend shutdown ({} widget tasks)",
                        screen_id,
                        handles.len()
                    );
                    stop_screen_subscription(handles);
                }

                for (widget_id, (_count, handles)) in remaining_widget_subscriptions {
                    tracing::debug!(
                        "Cleaning up shared protocol monitor for widget '{}' on backend shutdown ({} task(s))",
                        widget_id,
                        handles.len()
                    );
                    stop_widget_subscription(handles);
                }
        });

        backend_tx
}

fn generate_ipc_session_token() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos();
    format!("ipc-{}-{}", std::process::id(), now)
}

fn generate_loopback_session_token() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos();
    format!("http-{}-{}", std::process::id(), now)
}

async fn enforce_loopback_mutation_token(
    State(expected_token): State<String>,
    req: HttpRequest<Body>,
    next: middleware::Next,
) -> AxumResponse {
    let is_mutating_api = req.method() == Method::POST
        && req.uri().path().starts_with("/api/");

    if is_mutating_api {
        let provided_token = req
            .headers()
            .get("x-mycela-session-token")
            .and_then(|value| value.to_str().ok());

        if provided_token != Some(expected_token.as_str()) {
            tracing::warn!(
                "Rejected mutating loopback request without valid session token: {} {}",
                req.method(),
                req.uri().path()
            );
            return StatusCode::UNAUTHORIZED.into_response();
        }
    }

    next.run(req).await
}

// --- Entry point -------------------------------------------------------------

fn main() {
    let _log_guard = mycela::logging::init_logging(Some(std::path::Path::new("logs")));
    tracing::info!("Starting Mycela Desktop");

    let config: AppConfig = serde_json::from_str(
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/examples/demo_app.json")),
    )
    .expect("embedded demo_app.json is invalid");

    if let Ok(raw_transport) = std::env::var(DESKTOP_TRANSPORT_ENV) {
        if DesktopTransport::parse(&raw_transport).is_none() {
            tracing::warn!(
                "Unknown {} value '{}'; falling back to loopback",
                DESKTOP_TRANSPORT_ENV,
                raw_transport
            );
        }
    }

    let transport = DesktopTransport::from_env();
    tracing::info!(
        "Selected desktop transport: {} (set with {})",
        transport.as_str(),
        DESKTOP_TRANSPORT_ENV
    );

    match transport {
        DesktopTransport::Loopback => run_loopback_desktop(config),
        DesktopTransport::Ipc => run_ipc_desktop(config),
    }
}

fn run_ipc_desktop(config: AppConfig) {
    let session_token = generate_ipc_session_token();
    let event_loop = EventLoop::<DesktopUserEvent>::with_user_event().build().unwrap();
    let proxy = event_loop.create_proxy();
    let backend_tx = spawn_ipc_backend(config.clone(), session_token.clone(), proxy.clone());
    let mut app = DesktopApp {
        transport: DesktopTransport::Ipc,
        base_url: Some("mycela://app/".to_string()),
        windows: Vec::new(),
        proxy,
        backend_tx: Some(backend_tx),
        ipc_config: Some(Arc::new(config)),
        ipc_session_token: Some(session_token),
    };
    event_loop.run_app(&mut app).unwrap();
}

fn run_loopback_desktop(config: AppConfig) {
    let loopback_token = generate_loopback_session_token();

    // Channel: background server thread sends the bound port to the main thread.
    let (port_tx, port_rx) = std::sync::mpsc::channel::<u16>();
    let loopback_token_for_server = loopback_token.clone();

    std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        runtime.block_on(async move {
            let state = build_app_state(config, Some(loopback_token_for_server.clone()));

            // Bind to an OS-assigned port so nothing is hardcoded.
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("TCP bind");
            let port = listener.local_addr().unwrap().port();
            port_tx.send(port).unwrap();
            tracing::info!("Axum server bound on port {}", port);

            let allowed_origin = format!("http://127.0.0.1:{}", port)
                .parse::<header::HeaderValue>()
                .expect("valid loopback origin header");

            let cors = CorsLayer::new()
                .allow_origin(allowed_origin)
                .allow_methods([Method::GET, Method::POST])
                .allow_headers([
                    header::CONTENT_TYPE,
                    header::HeaderName::from_static("x-mycela-session-token"),
                ]);

            let expected_token = loopback_token_for_server.clone();

            let app = state.screen_routes()
                .route("/api/server/start",              post(start_server))
                .route("/api/server/stop",               post(stop_server))
                .route("/api/server/status",             get(server_status))
                .route("/api/modbus/start",              post(start_modbus))
                .route("/api/modbus/stop",               post(stop_modbus))
                .route("/api/modbus/status",             get(modbus_status))
                .route("/static/{*path}",                get(static_file_handler))
                .with_state(state)
                .layer(TraceLayer::new_for_http())
                .layer(middleware::from_fn_with_state(
                    expected_token,
                    enforce_loopback_mutation_token,
                ))
                .layer(cors);

            axum::serve(listener, app).await.expect("axum serve");
        });
    });

    // Block briefly until the server is ready and has sent its port.
    let port = port_rx.recv().expect("server thread exited before sending port");
    let url  = format!("http://127.0.0.1:{}/", port);
    tracing::info!("Desktop window opening {}", url);

    // winit owns the main thread for the duration of the app.
    let event_loop = EventLoop::<DesktopUserEvent>::with_user_event().build().unwrap();
    let proxy = event_loop.create_proxy();
    let mut app = DesktopApp {
        transport: DesktopTransport::Loopback,
        base_url: Some(url),
        windows: Vec::new(),
        proxy,
        backend_tx: None,
        ipc_config: None,
        ipc_session_token: None,
    };
    event_loop.run_app(&mut app).unwrap();
}
