mod epics_simulator;
mod modbus_simulator;

use mycela::app::{AppState, stop_server, server_status, stop_modbus, modbus_status};
use mycela::config::AppConfig;
use mycela::protocol_control::{self, ProtocolControlError};
use mycela::{modbus_client, server_setup::setup_server_pvs};
use mycela::axum::{routing::{get, post}, extract::State, response::{Html, IntoResponse, Response}, http::StatusCode};
use mycela::maud;
use mycela::tower_http::{services::ServeDir, trace::TraceLayer, cors::CorsLayer};
use mycela::pvxs_sys;
use std::sync::{Arc, Mutex};

// --- Entry point -------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _log_guard = mycela::logging::init_logging(Some(std::path::Path::new("logs")));
    tracing::info!("Starting Mycela Demo Server");

    let config = AppConfig::load(
        concat!(env!("CARGO_MANIFEST_DIR"), "/examples/demo_app.json"),
    ).expect("Failed to load demo_app.json");

    // EPICS / PVXS setup
    let server = pvxs_sys::Server::start_from_env()?;
    for screen in &config.screens {
        setup_server_pvs(&server, &screen.widgets)?;
    }
    for screen in &config.screens {
        epics_simulator::start_demo_simulator(server.handle(), &screen.widgets);
    }
    let epics_ctx = Arc::new(Mutex::new(pvxs_sys::Context::from_env()?));

    // Modbus setup
    let (sim_h, listener_h) = modbus_simulator::start_modbus_simulator(5020);
    tracing::info!("Modbus TCP demo simulator started on port 5020");
    let channel_ctx = mycela::channel::ChannelContext::new(
        epics_ctx, modbus_client::ModbusPool::new(),
    );

    let state = AppState {
        pv_server:   Arc::new(Mutex::new(Some(server))),
        config:      Arc::new(config),
        channel_ctx,
        modbus_task: Arc::new(Mutex::new(Some(vec![sim_h, listener_h]))),
        loopback_token: None,
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
    };

    let app = state.screen_routes()
        .route("/api/server/start",  post(start_server))
        .route("/api/server/stop",   post(stop_server))
        .route("/api/server/status", get(server_status))
        .route("/api/modbus/start",  post(start_modbus))
        .route("/api/modbus/stop",   post(stop_modbus))
        .route("/api/modbus/status", get(modbus_status))
        .nest_service("/static",     ServeDir::new("static"))
        .with_state(state)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    tracing::info!("Server running at http://127.0.0.1:3000");
    axum::serve(listener, app).await?;
    Ok(())
}

// --- Demo API handlers -------------------------------------------------------

async fn start_server(State(state): State<AppState>) -> Response {
    tracing::info!("POST /api/server/start");
    match protocol_control::start_epics_runtime(&state).await {
        Ok(()) => {
            let html = maud::html! {
                div class="success" hx-swap-oob="true" id="server-status" {
                    span { "EPICS Server Running" }
                }
            };
            Html(html.into_string()).into_response()
        }
        Err(ProtocolControlError::AlreadyRunning(_)) => {
            let html = maud::html! { div class="warning" { "EPICS Server is already running" } };
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

async fn start_modbus(State(state): State<AppState>) -> Response {
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

