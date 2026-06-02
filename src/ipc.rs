use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IpcMessageKind {
    Request,
    Response,
    Event,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IpcCommand {
    #[serde(rename = "app.screen.subscribe", alias = "app_screen_subscribe")]
    AppScreenSubscribe,
    #[serde(rename = "app.screen.unsubscribe", alias = "app_screen_unsubscribe")]
    AppScreenUnsubscribe,
    #[serde(rename = "app.widget.write", alias = "app_widget_write")]
    AppWidgetWrite,
    #[serde(rename = "epics.server.start", alias = "epics_server_start")]
    EpicsServerStart,
    #[serde(rename = "epics.server.stop", alias = "epics_server_stop")]
    EpicsServerStop,
    #[serde(rename = "epics.server.status.get", alias = "epics_server_status_get")]
    EpicsServerStatusGet,
    #[serde(rename = "epics.pv.read", alias = "epics_pv_read")]
    EpicsPvRead,
    #[serde(rename = "epics.pv.write", alias = "epics_pv_write")]
    EpicsPvWrite,
    #[serde(rename = "epics.pv.subscribe", alias = "epics_pv_subscribe")]
    EpicsPvSubscribe,
    #[serde(rename = "epics.pv.unsubscribe", alias = "epics_pv_unsubscribe")]
    EpicsPvUnsubscribe,
    #[serde(rename = "modbus.sim.start", alias = "modbus_sim_start")]
    ModbusSimStart,
    #[serde(rename = "modbus.sim.stop", alias = "modbus_sim_stop")]
    ModbusSimStop,
    #[serde(rename = "modbus.sim.status.get", alias = "modbus_sim_status_get")]
    ModbusSimStatusGet,
    #[serde(rename = "modbus.read", alias = "modbus_read")]
    ModbusRead,
    #[serde(rename = "modbus.write", alias = "modbus_write")]
    ModbusWrite,
    #[serde(rename = "modbus.subscribe", alias = "modbus_subscribe")]
    ModbusSubscribe,
    #[serde(rename = "modbus.unsubscribe", alias = "modbus_unsubscribe")]
    ModbusUnsubscribe,
    #[serde(rename = "app.ping", alias = "app_ping")]
    AppPing,
    #[serde(rename = "app.version.get", alias = "app_version_get")]
    AppVersionGet,
}

impl IpcCommand {
    pub fn is_mutating(&self) -> bool {
        matches!(
            self,
            Self::AppWidgetWrite
                | Self::EpicsServerStart
                | Self::EpicsServerStop
                | Self::EpicsPvWrite
                | Self::ModbusSimStart
                | Self::ModbusSimStop
                | Self::ModbusWrite
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcRequest {
    pub v: u16,
    pub kind: IpcMessageKind,
    pub id: String,
    pub cmd: IpcCommand,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub payload: Value,
    pub ts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcError {
    pub code: IpcErrorCode,
    pub message: String,
    #[serde(default)]
    pub details: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IpcErrorCode {
    AuthInvalidToken,
    AuthExpired,
    CmdUnknown,
    PayloadInvalid,
    StateConflict,
    InternalError,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcResponse {
    pub v: u16,
    pub kind: IpcMessageKind,
    pub id: String,
    pub ok: bool,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<IpcError>,
    pub ts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcEvent {
    pub v: u16,
    pub kind: IpcMessageKind,
    pub event: String,
    pub data: Value,
    pub ts: i64,
}
