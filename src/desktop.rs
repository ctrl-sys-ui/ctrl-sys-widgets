use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, mpsc};

use axum::Router;
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop, EventLoopBuilder, EventLoopProxy},
    window::WindowBuilder,
};
use tokio::task::JoinHandle;
use wry::WebViewBuilder;

use crate::app::AppState;
use crate::axum::http::{
    Request as HttpRequest,
    Response as HttpResponse,
};
use crate::config::AppConfig;
use crate::desktop_transport::DesktopTransport;
use crate::ipc::{IpcCommand, IpcEvent, IpcMessageKind, IpcRequest, IpcResponse};
use crate::ipc_dispatch;

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

enum DesktopUserEvent {
    IpcMessage(String),
    IpcEvent(IpcEvent),
}

struct BackendRequest {
    request: IpcRequest,
    response_tx: mpsc::Sender<IpcResponse>,
}

#[derive(Clone)]
pub struct DesktopWindowSettings {
    pub title: String,
    pub width: f64,
    pub height: f64,
}

impl DesktopWindowSettings {
    pub fn from_app_config(config: &AppConfig) -> Self {
        let window = &config.startup.desktop.window;
        Self {
            title: window
                .title
                .clone()
                .unwrap_or_else(|| config.title.clone()),
            width: window.width.unwrap_or(1400.0),
            height: window.height.unwrap_or(778.0),
        }
    }
}

type BuildAppStateFn = Arc<dyn Fn(AppConfig, Option<String>) -> AppState + Send + Sync>;
type BuildLoopbackRouterFn = Arc<dyn Fn(AppState) -> Router + Send + Sync>;
type IpcProtocolResponseFn = Arc<
    dyn Fn(&AppConfig, &str, HttpRequest<Vec<u8>>) -> HttpResponse<Cow<'static, [u8]>> + Send + Sync,
>;
type SpawnSubscriptionFn = Arc<
    dyn Fn(&AppState, &str, mpsc::Sender<IpcEvent>) -> Result<Vec<JoinHandle<()>>, String>
        + Send
        + Sync,
>;
type StopSubscriptionFn = Arc<dyn Fn(Vec<JoinHandle<()>>) + Send + Sync>;

#[derive(Clone)]
pub struct DesktopRuntimeHooks {
    pub build_app_state: BuildAppStateFn,
    pub build_loopback_router: BuildLoopbackRouterFn,
    pub ipc_protocol_response: IpcProtocolResponseFn,
    pub spawn_screen_subscription: SpawnSubscriptionFn,
    pub stop_screen_subscription: StopSubscriptionFn,
    pub spawn_widget_subscription: SpawnSubscriptionFn,
    pub stop_widget_subscription: StopSubscriptionFn,
    pub initial_path: String,
}

impl DesktopRuntimeHooks {
    pub fn new(
        build_app_state: impl Fn(AppConfig, Option<String>) -> AppState + Send + Sync + 'static,
        build_loopback_router: impl Fn(AppState) -> Router + Send + Sync + 'static,
        ipc_protocol_response: impl Fn(
                &AppConfig,
                &str,
                HttpRequest<Vec<u8>>,
            ) -> HttpResponse<Cow<'static, [u8]>>
            + Send
            + Sync
            + 'static,
        spawn_screen_subscription: impl Fn(
                &AppState,
                &str,
                mpsc::Sender<IpcEvent>,
            ) -> Result<Vec<JoinHandle<()>>, String>
            + Send
            + Sync
            + 'static,
        stop_screen_subscription: impl Fn(Vec<JoinHandle<()>>) + Send + Sync + 'static,
        spawn_widget_subscription: impl Fn(
                &AppState,
                &str,
                mpsc::Sender<IpcEvent>,
            ) -> Result<Vec<JoinHandle<()>>, String>
            + Send
            + Sync
            + 'static,
        stop_widget_subscription: impl Fn(Vec<JoinHandle<()>>) + Send + Sync + 'static,
        initial_path: impl Into<String>,
    ) -> Self {
        Self {
            build_app_state: Arc::new(build_app_state),
            build_loopback_router: Arc::new(build_loopback_router),
            ipc_protocol_response: Arc::new(ipc_protocol_response),
            spawn_screen_subscription: Arc::new(spawn_screen_subscription),
            stop_screen_subscription: Arc::new(stop_screen_subscription),
            spawn_widget_subscription: Arc::new(spawn_widget_subscription),
            stop_widget_subscription: Arc::new(stop_widget_subscription),
            initial_path: initial_path.into(),
        }
    }
}

fn normalized_initial_path(path: &str) -> String {
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{}", path)
    }
}

fn generate_session_token(prefix: &str) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos();
    format!("{}-{}-{}", prefix, std::process::id(), now)
}

fn screen_subscription_response(id: &str, ok: bool, message: Option<&str>) -> IpcResponse {
    IpcResponse {
        v: 1,
        kind: IpcMessageKind::Response,
        id: id.to_string(),
        ok,
        result: Some(serde_json::json!({ "subscribed": ok })),
        error: message.map(|msg| crate::ipc::IpcError {
            code: crate::ipc::IpcErrorCode::PayloadInvalid,
            message: msg.to_string(),
            details: None,
        }),
        ts: now_millis(),
    }
}

fn release_subscription(
    subscriptions: &mut HashMap<String, (usize, Vec<JoinHandle<()>>)>,
    key: &str,
    stop: &StopSubscriptionFn,
) {
    let remove_entry = match subscriptions.get_mut(key) {
        Some((count, _)) if *count > 1 => {
            *count -= 1;
            false
        }
        Some(_) => true,
        None => false,
    };

    if remove_entry {
        if let Some((_count, handles)) = subscriptions.remove(key) {
            (stop)(handles);
        }
    }
}

fn spawn_ipc_backend(
    config: AppConfig,
    session_token: String,
    proxy: EventLoopProxy<DesktopUserEvent>,
    hooks: DesktopRuntimeHooks,
) -> mpsc::Sender<BackendRequest> {
    let (backend_tx, backend_rx) = mpsc::channel::<BackendRequest>();

    std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let state = (hooks.build_app_state)(config, None);
        let (event_tx, event_rx) = mpsc::channel::<IpcEvent>();
        let mut subscription_to_screen = HashMap::<String, String>::new();
        let mut screen_subscriptions = HashMap::<String, (usize, Vec<JoinHandle<()>>)>::new();
        let mut subscription_to_widget = HashMap::<String, String>::new();
        let mut widget_subscriptions = HashMap::<String, (usize, Vec<JoinHandle<()>>)>::new();

        let proxy_clone = proxy.clone();
        std::thread::spawn(move || {
            while let Ok(event) = event_rx.recv() {
                if proxy_clone.send_event(DesktopUserEvent::IpcEvent(event)).is_err() {
                    break;
                }
            }
        });

        while let Ok(backend_request) = backend_rx.recv() {
            let response = if backend_request.request.cmd == IpcCommand::AppScreenSubscribe {
                let screen_id = backend_request
                    .request
                    .payload
                    .get("screen_id")
                    .and_then(|value| value.as_str())
                    .map(str::to_string);
                let subscription_id = backend_request
                    .request
                    .payload
                    .get("subscription_id")
                    .and_then(|value| value.as_str())
                    .map(str::to_string);

                match (screen_id, subscription_id) {
                    (Some(screen_id), Some(subscription_id)) => {
                        if let Some(previous_screen_id) =
                            subscription_to_screen.remove(&subscription_id)
                        {
                            release_subscription(
                                &mut screen_subscriptions,
                                &previous_screen_id,
                                &hooks.stop_screen_subscription,
                            );
                        }

                        if let Some((count, _)) = screen_subscriptions.get_mut(&screen_id) {
                            *count += 1;
                            subscription_to_screen.insert(subscription_id, screen_id);
                            screen_subscription_response(&backend_request.request.id, true, None)
                        } else {
                            match runtime.block_on(async {
                                (hooks.spawn_screen_subscription)(
                                    &state,
                                    &screen_id,
                                    event_tx.clone(),
                                )
                            }) {
                                Ok(handles) => {
                                    screen_subscriptions.insert(screen_id.clone(), (1, handles));
                                    subscription_to_screen.insert(subscription_id, screen_id);
                                    screen_subscription_response(&backend_request.request.id, true, None)
                                }
                                Err(error) => screen_subscription_response(
                                    &backend_request.request.id,
                                    false,
                                    Some(&error),
                                ),
                            }
                        }
                    }
                    _ => screen_subscription_response(
                        &backend_request.request.id,
                        false,
                        Some("Missing screen_id or subscription_id"),
                    ),
                }
            } else if backend_request.request.cmd == IpcCommand::AppScreenUnsubscribe {
                let subscription_id = backend_request
                    .request
                    .payload
                    .get("subscription_id")
                    .and_then(|value| value.as_str())
                    .map(str::to_string);

                match subscription_id {
                    Some(subscription_id) => {
                        if let Some(screen_id) = subscription_to_screen.remove(&subscription_id) {
                            release_subscription(
                                &mut screen_subscriptions,
                                &screen_id,
                                &hooks.stop_screen_subscription,
                            );
                        }
                        screen_subscription_response(&backend_request.request.id, true, None)
                    }
                    None => screen_subscription_response(
                        &backend_request.request.id,
                        false,
                        Some("Missing subscription_id"),
                    ),
                }
            } else if matches!(
                backend_request.request.cmd,
                IpcCommand::EpicsPvSubscribe | IpcCommand::ModbusSubscribe
            ) {
                let widget_id = backend_request
                    .request
                    .payload
                    .get("widget_id")
                    .and_then(|value| value.as_str())
                    .map(str::to_string);
                let subscription_id = backend_request
                    .request
                    .payload
                    .get("subscription_id")
                    .and_then(|value| value.as_str())
                    .map(str::to_string);

                match (widget_id, subscription_id) {
                    (Some(widget_id), Some(subscription_id)) => {
                        if let Some(previous_widget_id) =
                            subscription_to_widget.remove(&subscription_id)
                        {
                            release_subscription(
                                &mut widget_subscriptions,
                                &previous_widget_id,
                                &hooks.stop_widget_subscription,
                            );
                        }

                        if let Some((count, _)) = widget_subscriptions.get_mut(&widget_id) {
                            *count += 1;
                            subscription_to_widget.insert(subscription_id, widget_id);
                            screen_subscription_response(&backend_request.request.id, true, None)
                        } else {
                            match runtime.block_on(async {
                                (hooks.spawn_widget_subscription)(
                                    &state,
                                    &widget_id,
                                    event_tx.clone(),
                                )
                            }) {
                                Ok(handles) => {
                                    widget_subscriptions.insert(widget_id.clone(), (1, handles));
                                    subscription_to_widget.insert(subscription_id, widget_id);
                                    screen_subscription_response(&backend_request.request.id, true, None)
                                }
                                Err(error) => screen_subscription_response(
                                    &backend_request.request.id,
                                    false,
                                    Some(&error),
                                ),
                            }
                        }
                    }
                    _ => screen_subscription_response(
                        &backend_request.request.id,
                        false,
                        Some("Missing widget_id or subscription_id"),
                    ),
                }
            } else if matches!(
                backend_request.request.cmd,
                IpcCommand::EpicsPvUnsubscribe | IpcCommand::ModbusUnsubscribe
            ) {
                let subscription_id = backend_request
                    .request
                    .payload
                    .get("subscription_id")
                    .and_then(|value| value.as_str())
                    .map(str::to_string);

                match subscription_id {
                    Some(subscription_id) => {
                        if let Some(widget_id) = subscription_to_widget.remove(&subscription_id) {
                            release_subscription(
                                &mut widget_subscriptions,
                                &widget_id,
                                &hooks.stop_widget_subscription,
                            );
                        }
                        screen_subscription_response(&backend_request.request.id, true, None)
                    }
                    None => screen_subscription_response(
                        &backend_request.request.id,
                        false,
                        Some("Missing subscription_id"),
                    ),
                }
            } else {
                runtime.block_on(ipc_dispatch::dispatch_request(
                    &state,
                    backend_request.request,
                    Some(&session_token),
                ))
            };

            let _ = backend_request.response_tx.send(response);
        }
    });

    backend_tx
}

fn run_loopback_desktop(
    config: AppConfig,
    window: DesktopWindowSettings,
    hooks: DesktopRuntimeHooks,
) {
    let loopback_token = generate_session_token("http");
    let (port_tx, port_rx) = std::sync::mpsc::channel::<u16>();

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");

        rt.block_on(async move {
            let state = (hooks.build_app_state)(config, Some(loopback_token));
            let app = (hooks.build_loopback_router)(state);

            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("failed to bind Axum listener");
            let port = listener.local_addr().expect("local addr").port();
            port_tx.send(port).expect("main thread already exited?");

            axum::serve(listener, app).await.expect("Axum server error");
        });
    });

    let port = port_rx.recv().expect("server thread failed to start");
    let entry_path = normalized_initial_path(&hooks.initial_path);
    let url = format!("http://127.0.0.1:{}{}", port, entry_path);

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title(window.title)
        .with_inner_size(LogicalSize::new(window.width, window.height))
        .build(&event_loop)
        .expect("failed to create window");

    let webview = WebViewBuilder::new()
        .with_url(&url)
        .build(&window)
        .expect("failed to create webview");

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        let _ = &webview;

        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            *control_flow = ControlFlow::Exit;
        }
    });
}

fn run_ipc_desktop(config: AppConfig, window: DesktopWindowSettings, hooks: DesktopRuntimeHooks) {
    let session_token = generate_session_token("ipc");
    let event_loop = EventLoopBuilder::<DesktopUserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let backend_tx = spawn_ipc_backend(config.clone(), session_token.clone(), proxy.clone(), hooks.clone());

    let window = WindowBuilder::new()
        .with_title(window.title)
        .with_inner_size(LogicalSize::new(window.width, window.height))
        .build(&event_loop)
        .expect("failed to create window");

    let protocol_config = config.clone();
    let protocol_token = session_token.clone();
    let protocol_response = hooks.ipc_protocol_response.clone();
    let proxy_for_ipc = proxy.clone();
    let webview = WebViewBuilder::new()
        .with_custom_protocol("mycela".into(), move |_webview_id, request| {
            (protocol_response)(&protocol_config, &protocol_token, request)
        })
        .with_url(&format!("mycela://app{}", normalized_initial_path(&hooks.initial_path)))
        .with_ipc_handler(move |request: wry::http::Request<String>| {
            let _ = proxy_for_ipc.send_event(DesktopUserEvent::IpcMessage(request.body().clone()));
        })
        .build(&window)
        .expect("failed to create webview");

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                *control_flow = ControlFlow::Exit;
            }
            Event::UserEvent(DesktopUserEvent::IpcMessage(payload)) => {
                let ipc_text = payload.trim().trim_matches('"').to_ascii_lowercase();
                if ipc_text == "quit" || ipc_text == "close" {
                    *control_flow = ControlFlow::Exit;
                    return;
                }

                let request = match serde_json::from_str::<IpcRequest>(&payload) {
                    Ok(request) => request,
                    Err(error) => {
                        tracing::error!("Failed to parse IPC request: {}", error);
                        return;
                    }
                };

                let (response_tx, response_rx) = mpsc::channel();
                if backend_tx.send(BackendRequest { request, response_tx }).is_err() {
                    tracing::error!("Failed to send IPC request to backend");
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
                let script = format!("window.__MYCELA_IPC_DELIVER({});", response_json);
                if let Err(error) = webview.evaluate_script(&script) {
                    tracing::error!("Failed to deliver IPC response to webview: {}", error);
                }
            }
            Event::UserEvent(DesktopUserEvent::IpcEvent(ipc_event)) => {
                let event_json = match serde_json::to_string(&ipc_event) {
                    Ok(json) => json,
                    Err(error) => {
                        tracing::error!("Failed to serialize IPC event: {}", error);
                        return;
                    }
                };
                let script = format!("window.__MYCELA_IPC_EVENT_DELIVER({});", event_json);
                if let Err(error) = webview.evaluate_script(&script) {
                    tracing::error!("Failed to deliver IPC event to webview: {}", error);
                }
            }
            _ => {}
        }
    });
}

pub fn run_desktop(config: AppConfig, hooks: DesktopRuntimeHooks) {
    let transport = DesktopTransport::from_app_config(&config);
    tracing::info!("Selected desktop transport: {}", transport.as_str());

    let window_settings = DesktopWindowSettings::from_app_config(&config);
    match transport {
        DesktopTransport::Loopback => run_loopback_desktop(config, window_settings, hooks),
        DesktopTransport::Ipc => run_ipc_desktop(config, window_settings, hooks),
    }
}
