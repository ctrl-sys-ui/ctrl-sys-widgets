use std::borrow::Cow;
use std::sync::{Arc, Mutex, mpsc};

use axum::routing::get;
use mycela::app::AppState;
use mycela::axum::http::{
    Request as HttpRequest,
    Response as HttpResponse,
    StatusCode,
    header,
};
use mycela::config::AppConfig;
use mycela::desktop::DesktopRuntimeHooks;
use mycela::ipc::{IpcEvent, IpcMessageKind};
use mycela::modbus_client::ModbusPool;
use mycela::widgets;

// Desktop adaptor starter template.
//
// Copy this into your app crate and replace the TODO sections with your app-specific
// screen, asset, and subscription wiring.

mod assets;

const APP_SCREEN_ID: &str = "replace_with_your_screen_id";
const APP_ENTRY_PATH: &str = "/replace_with_your_entry_path";
const APP_SCREEN_PATH: &str = "/screen/replace_with_your_screen_id";
const APP_BACKGROUND_ASSET_PATH: &str = "/static/replace_with_your_background.svg";

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

pub fn build_hooks() -> DesktopRuntimeHooks {
    DesktopRuntimeHooks::new(
        build_app_state,
        |state| {
            state
                .screen_routes()
                .route(APP_ENTRY_PATH, get(render_app_screen))
                .route("/static/htmx.min.js", get(assets::serve_htmx))
                .route("/static/style.css", get(assets::serve_css))
                .route("/static/tooltip.js", get(assets::serve_tooltip))
                .route("/static/client_transport.js", get(assets::serve_client_transport))
                .route(APP_BACKGROUND_ASSET_PATH, get(assets::serve_background_svg))
                .with_state(state)
        },
        ipc_protocol_response,
        spawn_screen_subscription,
        stop_screen_subscription,
        spawn_widget_subscription,
        stop_widget_subscription,
        "/pid",
    )
}

fn build_app_state(config: AppConfig, loopback_token: Option<String>) -> AppState {
    let pool = ModbusPool::new();
    let channel_ctx = mycela::channel::ChannelContext::new(pool);

    AppState {
        config: Arc::new(config),
        channel_ctx,
        modbus_task: Arc::new(Mutex::new(None)),
        modbus_start_hook: None,
        loopback_token,
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

    if path == "/" || path.is_empty() || path == "/index.html" || path == APP_ENTRY_PATH || path == APP_SCREEN_PATH {
        ipc_html_response(render_app_screen_html(config, Some(session_token)))
    } else {
        ipc_text_response(StatusCode::NOT_FOUND, "not found")
    }
}

async fn render_app_screen() {
    // TODO: provide the app's screen handler or remove this route if you only
    // need the IPC custom protocol entry point.
}

fn render_app_screen_html(config: &AppConfig, ipc_token: Option<&str>) -> String {
    // TODO: replace with your app-specific HTML renderer.
    // This should usually call into your app crate's screen renderer.
    let _ = config;
    let _ = ipc_token;
    String::new()
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