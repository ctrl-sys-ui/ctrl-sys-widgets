use crate::app::AppState;
use crate::channel::ChannelEvent;
use crate::config::{ProtocolConfig, WidgetConfig};
use crate::ipc::{IpcCommand, IpcError, IpcErrorCode, IpcMessageKind, IpcRequest, IpcResponse};
use crate::protocol_control::{self, ProtocolControlError};
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use tokio_stream::StreamExt;

#[derive(Deserialize)]
struct WidgetWritePayload {
    widget_id: String,
    value: String,
}

#[derive(Deserialize)]
struct ChannelReadPayload {
    widget_id: String,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[derive(Deserialize)]
struct ChannelWritePayload {
    widget_id: String,
    value: String,
}

fn find_widget_by_id(state: &AppState, widget_id: &str) -> Option<WidgetConfig> {
    state
        .config
        .screens
        .iter()
        .flat_map(|screen| crate::widgets::collect_data_widgets(&screen.widgets))
        .find(|widget| widget.id == widget_id)
}

#[cfg(feature = "epics")]
fn widget_is_epics(widget: &WidgetConfig) -> bool {
    matches!(widget.protocol.as_ref(), Some(ProtocolConfig::EpicsPva(_)))
}

#[cfg(not(feature = "epics"))]
fn widget_is_epics(_widget: &WidgetConfig) -> bool {
    false
}

fn widget_is_modbus(widget: &WidgetConfig) -> bool {
    matches!(widget.protocol.as_ref(), Some(ProtocolConfig::ModbusTcp(_)))
}

async fn read_widget_value(
    state: &AppState,
    widget: WidgetConfig,
    timeout_ms: u64,
) -> Result<serde_json::Value, String> {
    let mut stream =
        crate::channel::channel_stream(Arc::new(widget.clone()), state.channel_ctx.clone());

    let receive = async {
        while let Some(event) = stream.next().await {
            match event {
                ChannelEvent::Connected => continue,
                ChannelEvent::Value(value) => {
                    return Ok(json!({
                        "widget_id": widget.id,
                        "channel": widget.channel_address(),
                        "value": value.value_str,
                        "raw_value": value.raw_value,
                        "units": value.units,
                        "alarm_severity": value.alarm_severity,
                    }));
                }
                ChannelEvent::Disconnected(message) => {
                    return Err(format!("Channel disconnected: {}", message));
                }
                ChannelEvent::Error(message) => {
                    return Err(format!("Channel error: {}", message));
                }
            }
        }
        Err("Channel stream ended before value was received".to_string())
    };

    match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), receive).await {
        Ok(result) => result,
        Err(_) => Err(format!("Timed out waiting for value ({} ms)", timeout_ms)),
    }
}

pub async fn dispatch_request(
    state: &AppState,
    request: IpcRequest,
    expected_token: Option<&str>,
) -> IpcResponse {
    if request.kind != IpcMessageKind::Request {
        return error_response(
            &request.id,
            IpcErrorCode::PayloadInvalid,
            "IPC message kind must be request",
        );
    }

    if request.cmd.is_mutating() {
        match (expected_token, request.token.as_deref()) {
            (Some(expected), Some(token)) if expected == token => {}
            (Some(_), _) => {
                return error_response(
                    &request.id,
                    IpcErrorCode::AuthInvalidToken,
                    "Token missing or invalid",
                );
            }
            (None, _) => {}
        }
    }

    match request.cmd {
        IpcCommand::AppWidgetWrite => {
            let payload = match serde_json::from_value::<WidgetWritePayload>(request.payload) {
                Ok(payload) => payload,
                Err(error) => {
                    return error_response(
                        &request.id,
                        IpcErrorCode::PayloadInvalid,
                        &format!("Invalid widget write payload: {}", error),
                    );
                }
            };

            let (status, markup) =
                crate::app::write_widget_markup(state, &payload.widget_id, payload.value).await;

            if status == StatusCode::OK {
                ok_response(
                    &request.id,
                    json!({
                        "widget_id": payload.widget_id,
                        "html": markup.into_string(),
                    }),
                )
            } else {
                IpcResponse {
                    v: 1,
                    kind: IpcMessageKind::Response,
                    id: request.id,
                    ok: false,
                    result: Some(json!({
                        "widget_id": payload.widget_id,
                        "html": markup.into_string(),
                    })),
                    error: Some(IpcError {
                        code: IpcErrorCode::PayloadInvalid,
                        message: "Widget write failed".to_string(),
                        details: None,
                    }),
                    ts: chrono::Utc::now().timestamp_millis(),
                }
            }
        }
        IpcCommand::EpicsServerStart => {
            #[cfg(feature = "epics")]
            {
                match protocol_control::start_epics_runtime(state).await {
                    Ok(()) => ok_response(&request.id, json!({ "running": true })),
                    Err(error) => protocol_error_response(&request.id, error),
                }
            }
            #[cfg(not(feature = "epics"))]
            {
                error_response(
                    &request.id,
                    IpcErrorCode::CmdUnknown,
                    "EPICS feature is not enabled",
                )
            }
        }
        IpcCommand::EpicsServerStop => {
            #[cfg(feature = "epics")]
            {
                match protocol_control::stop_epics_server(state).await {
                    Ok(()) => ok_response(&request.id, json!({ "running": false })),
                    Err(error) => protocol_error_response(&request.id, error),
                }
            }
            #[cfg(not(feature = "epics"))]
            {
                error_response(
                    &request.id,
                    IpcErrorCode::CmdUnknown,
                    "EPICS feature is not enabled",
                )
            }
        }
        IpcCommand::EpicsServerStatusGet => {
            #[cfg(feature = "epics")]
            {
                ok_response(&request.id, json!({ "running": state.is_server_running() }))
            }
            #[cfg(not(feature = "epics"))]
            {
                ok_response(&request.id, json!({ "running": false }))
            }
        }
        IpcCommand::EpicsPvRead => {
            let payload = match serde_json::from_value::<ChannelReadPayload>(request.payload) {
                Ok(payload) => payload,
                Err(error) => {
                    return error_response(
                        &request.id,
                        IpcErrorCode::PayloadInvalid,
                        &format!("Invalid EPICS read payload: {}", error),
                    );
                }
            };

            let Some(widget) = find_widget_by_id(state, &payload.widget_id) else {
                return error_response(
                    &request.id,
                    IpcErrorCode::PayloadInvalid,
                    &format!("Widget '{}' not found", payload.widget_id),
                );
            };

            if !widget_is_epics(&widget) {
                return error_response(
                    &request.id,
                    IpcErrorCode::PayloadInvalid,
                    &format!("Widget '{}' is not configured for EPICS", payload.widget_id),
                );
            }

            match read_widget_value(state, widget, payload.timeout_ms.unwrap_or(1500)).await {
                Ok(result) => ok_response(&request.id, result),
                Err(message) => error_response(&request.id, IpcErrorCode::Timeout, &message),
            }
        }
        IpcCommand::EpicsPvWrite => {
            let payload = match serde_json::from_value::<ChannelWritePayload>(request.payload) {
                Ok(payload) => payload,
                Err(error) => {
                    return error_response(
                        &request.id,
                        IpcErrorCode::PayloadInvalid,
                        &format!("Invalid EPICS write payload: {}", error),
                    );
                }
            };

            let Some(widget) = find_widget_by_id(state, &payload.widget_id) else {
                return error_response(
                    &request.id,
                    IpcErrorCode::PayloadInvalid,
                    &format!("Widget '{}' not found", payload.widget_id),
                );
            };

            if !widget_is_epics(&widget) {
                return error_response(
                    &request.id,
                    IpcErrorCode::PayloadInvalid,
                    &format!("Widget '{}' is not configured for EPICS", payload.widget_id),
                );
            }

            let (status, markup) =
                crate::app::write_widget_markup(state, &payload.widget_id, payload.value).await;

            if status == StatusCode::OK {
                ok_response(
                    &request.id,
                    json!({
                        "widget_id": payload.widget_id,
                        "html": markup.into_string(),
                    }),
                )
            } else {
                error_response(
                    &request.id,
                    IpcErrorCode::PayloadInvalid,
                    "EPICS write failed",
                )
            }
        }
        IpcCommand::EpicsPvSubscribe | IpcCommand::EpicsPvUnsubscribe => error_response(
            &request.id,
            IpcErrorCode::CmdUnknown,
            "Command is handled by desktop backend subscription orchestration",
        ),
        IpcCommand::ModbusSimStart => match protocol_control::start_modbus_runtime(state) {
            Ok(()) => ok_response(&request.id, json!({ "running": true })),
            Err(error) => protocol_error_response(&request.id, error),
        },
        IpcCommand::ModbusSimStop => match protocol_control::stop_modbus_tasks(state) {
            Ok(()) => ok_response(&request.id, json!({ "running": false })),
            Err(error) => protocol_error_response(&request.id, error),
        },
        IpcCommand::ModbusSimStatusGet => {
            ok_response(&request.id, json!({ "running": state.is_modbus_running() }))
        }
        IpcCommand::ModbusRead => {
            let payload = match serde_json::from_value::<ChannelReadPayload>(request.payload) {
                Ok(payload) => payload,
                Err(error) => {
                    return error_response(
                        &request.id,
                        IpcErrorCode::PayloadInvalid,
                        &format!("Invalid Modbus read payload: {}", error),
                    );
                }
            };

            let Some(widget) = find_widget_by_id(state, &payload.widget_id) else {
                return error_response(
                    &request.id,
                    IpcErrorCode::PayloadInvalid,
                    &format!("Widget '{}' not found", payload.widget_id),
                );
            };

            if !widget_is_modbus(&widget) {
                return error_response(
                    &request.id,
                    IpcErrorCode::PayloadInvalid,
                    &format!(
                        "Widget '{}' is not configured for Modbus",
                        payload.widget_id
                    ),
                );
            }

            match read_widget_value(state, widget, payload.timeout_ms.unwrap_or(1500)).await {
                Ok(result) => ok_response(&request.id, result),
                Err(message) => error_response(&request.id, IpcErrorCode::Timeout, &message),
            }
        }
        IpcCommand::ModbusWrite => {
            let payload = match serde_json::from_value::<ChannelWritePayload>(request.payload) {
                Ok(payload) => payload,
                Err(error) => {
                    return error_response(
                        &request.id,
                        IpcErrorCode::PayloadInvalid,
                        &format!("Invalid Modbus write payload: {}", error),
                    );
                }
            };

            let Some(widget) = find_widget_by_id(state, &payload.widget_id) else {
                return error_response(
                    &request.id,
                    IpcErrorCode::PayloadInvalid,
                    &format!("Widget '{}' not found", payload.widget_id),
                );
            };

            if !widget_is_modbus(&widget) {
                return error_response(
                    &request.id,
                    IpcErrorCode::PayloadInvalid,
                    &format!(
                        "Widget '{}' is not configured for Modbus",
                        payload.widget_id
                    ),
                );
            }

            let (status, markup) =
                crate::app::write_widget_markup(state, &payload.widget_id, payload.value).await;

            if status == StatusCode::OK {
                ok_response(
                    &request.id,
                    json!({
                        "widget_id": payload.widget_id,
                        "html": markup.into_string(),
                    }),
                )
            } else {
                error_response(
                    &request.id,
                    IpcErrorCode::PayloadInvalid,
                    "Modbus write failed",
                )
            }
        }
        IpcCommand::ModbusSubscribe | IpcCommand::ModbusUnsubscribe => error_response(
            &request.id,
            IpcErrorCode::CmdUnknown,
            "Command is handled by desktop backend subscription orchestration",
        ),
        IpcCommand::AppPing => ok_response(&request.id, json!({ "pong": true })),
        IpcCommand::AppVersionGet => ok_response(
            &request.id,
            json!({ "name": env!("CARGO_PKG_NAME"), "version": env!("CARGO_PKG_VERSION") }),
        ),
        // AppScreenSubscribe, AppScreenUnsubscribe, EpicsPvSubscribe, EpicsPvUnsubscribe,
        // ModbusSubscribe, and ModbusUnsubscribe are all orchestrated at the desktop-backend
        // level (spawn_ipc_backend) and must never reach this dispatcher.  The wildcard arm
        // acts as a safety net for those commands and any future commands not yet wired up.
        _ => error_response(
            &request.id,
            IpcErrorCode::CmdUnknown,
            "Command not implemented in dispatcher yet",
        ),
    }
}

fn ok_response(id: &str, result: serde_json::Value) -> IpcResponse {
    IpcResponse {
        v: 1,
        kind: IpcMessageKind::Response,
        id: id.to_string(),
        ok: true,
        result: Some(result),
        error: None,
        ts: chrono::Utc::now().timestamp_millis(),
    }
}

fn error_response(id: &str, code: IpcErrorCode, message: &str) -> IpcResponse {
    IpcResponse {
        v: 1,
        kind: IpcMessageKind::Response,
        id: id.to_string(),
        ok: false,
        result: None,
        error: Some(IpcError {
            code,
            message: message.to_string(),
            details: None,
        }),
        ts: chrono::Utc::now().timestamp_millis(),
    }
}

fn protocol_error_response(id: &str, error: ProtocolControlError) -> IpcResponse {
    match error {
        ProtocolControlError::AlreadyRunning(message)
        | ProtocolControlError::NotRunning(message) => {
            error_response(id, IpcErrorCode::StateConflict, message)
        }
        ProtocolControlError::Operation(message) => {
            error_response(id, IpcErrorCode::InternalError, &message)
        }
        ProtocolControlError::Internal(message) => {
            error_response(id, IpcErrorCode::InternalError, &message)
        }
    }
}
