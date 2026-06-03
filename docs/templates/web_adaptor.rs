use mycela::app::AppState;
use mycela::axum::routing::get;
use mycela::config::AppConfig;
use mycela::desktop::DesktopWindowSettings;

// Web adaptor starter template.
//
// Use this when you want the browser / loopback version of a mycela app.
// The app crate keeps the screen-specific renderers and static assets; this
// module only wires up the HTTP routes and startup configuration.

mod assets;

const APP_SCREEN_ID: &str = "replace_with_your_screen_id";
const APP_ENTRY_PATH: &str = "/replace_with_your_entry_path";
const APP_SCREEN_PATH: &str = "/screen/replace_with_your_screen_id";
const APP_BACKGROUND_ASSET_PATH: &str = "/static/replace_with_your_background.svg";

pub fn build_app_state(config: AppConfig, loopback_token: Option<String>) -> AppState {
    mycela::app::AppState {
        config: std::sync::Arc::new(config),
        channel_ctx: mycela::channel::ChannelContext::new(mycela::modbus_client::ModbusPool::new()),
        modbus_task: std::sync::Arc::new(std::sync::Mutex::new(None)),
        modbus_start_hook: None,
        loopback_token,
    }
}

pub fn build_routes(state: AppState) -> mycela::axum::Router<AppState> {
    state
        .screen_routes()
        .route(APP_ENTRY_PATH, get(render_app_screen))
        .route("/static/htmx.min.js", get(assets::serve_htmx))
        .route("/static/style.css", get(assets::serve_css))
        .route("/static/tooltip.js", get(assets::serve_tooltip))
        .route("/static/desktop_transport.js", get(assets::serve_desktop_transport))
        .route(APP_BACKGROUND_ASSET_PATH, get(assets::serve_background_svg))
        .with_state(state)
}

pub fn default_window_settings(config: &AppConfig) -> DesktopWindowSettings {
    DesktopWindowSettings::from_app_config(config)
}

async fn render_app_screen() {
    // TODO: provide the app's screen handler or rename/remove this route.
}