use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;

/// Custom error type for configuration loading
#[derive(Debug)]
pub enum ConfigError {
    FileError(std::io::Error),
    JsonError {
        source: serde_json::Error,
        context: String,
    },
    /// A semantic validation rule failed (e.g. duplicate IDs, out-of-range values).
    /// The message contains a human-readable description of the problem.
    ValidationError(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::FileError(e) => write!(f, "Failed to read config file: {}", e),
            ConfigError::JsonError { source, context } => {
                write!(f, "Configuration JSON error: {}\n{}", source, context)
            }
            ConfigError::ValidationError(msg) => {
                write!(f, "Configuration validation error: {}", msg)
            }
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<std::io::Error> for ConfigError {
    fn from(err: std::io::Error) -> Self {
        ConfigError::FileError(err)
    }
}

/// Navigation / action button attached to a screen header.
///
/// Each action renders as a button or link in the screen's nav bar.
/// JSON uses an internally-tagged enum: `{ "type": "navigate", ... }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ActionConfig {
    /// Button that navigates to another screen in the same tab.
    Navigate { label: String, to: String },
    /// Button that goes back to the home screen.
    Back { label: String },
    /// Button that opens another screen in a new browser tab.
    Popup { label: String, to: String },
    /// Button that opens another screen in a new browser window.
    Window { label: String, to: String },
    /// HTMX button that calls a custom API endpoint.
    Api {
        label: String,
        method: String,
        path: String,
    },
}

/// Application configuration — the top-level `app.json` format.
///
/// Wraps one or more [`ScreenConfig`]s.  Load with [`AppConfig::load`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub title: String,
    /// Screen `id` to render at `/`. Defaults to the first screen.
    #[serde(default)]
    pub home_screen: Option<String>,
    /// Optional startup/runtime settings for desktop launch.
    #[serde(default)]
    pub startup: AppStartupConfig,
    pub screens: Vec<ScreenConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppStartupConfig {
    #[serde(default)]
    pub desktop: DesktopStartupConfig,
    /// When true, widget tooltips show adaptor/proxy addresses instead of PLC/channel addresses.
    #[serde(default)]
    pub tooltip_use_adaptor_address: bool,
    #[cfg(feature = "modbus")]
    #[serde(default)]
    pub modbus_bridge: ModbusBridgeStartupConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesktopStartupConfig {
    /// Desktop transport mode selected from JSON.
    /// Accepted values: `loopback`, `http`, `localhost`, `ipc`, `bridge`.
    #[serde(default)]
    pub transport: Option<String>,
    /// When true, `MYCELA_DESKTOP_TRANSPORT` may override JSON transport.
    #[serde(default = "default_allow_env_transport_override")]
    pub allow_env_transport_override: bool,
    /// Optional desktop window settings.
    #[serde(default)]
    pub window: DesktopWindowConfig,
}

impl Default for DesktopStartupConfig {
    fn default() -> Self {
        Self {
            transport: None,
            allow_env_transport_override: default_allow_env_transport_override(),
            window: DesktopWindowConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesktopWindowConfig {
    /// Optional native window title override.
    #[serde(default)]
    pub title: Option<String>,
    /// Optional native window width in logical pixels.
    #[serde(default)]
    pub width: Option<f64>,
    /// Optional native window height in logical pixels.
    #[serde(default)]
    pub height: Option<f64>,
}

impl Default for DesktopWindowConfig {
    fn default() -> Self {
        Self {
            title: None,
            width: None,
            height: None,
        }
    }
}

#[cfg(feature = "modbus")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModbusBridgeStartupConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_modbus_bridge_listen_addr")]
    pub listen_addr: String,
    #[serde(default = "default_modbus_bridge_listen_port")]
    pub listen_port: u16,
    #[serde(default)]
    pub upstream: Option<ModbusBridgeUpstreamConfig>,
    #[serde(default)]
    pub registers: Vec<ModbusBridgeRegisterMap>,
}

#[cfg(feature = "modbus")]
impl Default for ModbusBridgeStartupConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            listen_addr: default_modbus_bridge_listen_addr(),
            listen_port: default_modbus_bridge_listen_port(),
            upstream: None,
            registers: Vec::new(),
        }
    }
}

#[cfg(feature = "modbus")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModbusBridgeUpstreamConfig {
    pub host: String,
    #[serde(default = "default_modbus_port")]
    pub port: u16,
    #[serde(default = "default_unit_id")]
    pub unit_id: u8,
}

#[cfg(feature = "modbus")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModbusBridgeRegisterMap {
    pub exposed_register: u16,
    pub register_type: ModbusRegisterType,
    #[serde(default = "default_word_count")]
    pub word_count: u8,
    #[serde(default)]
    pub source_widget_id: Option<String>,
    #[serde(default)]
    pub source_upstream_register: Option<u16>,
    #[serde(default)]
    pub source_upstream_register_type: Option<ModbusRegisterType>,
    #[serde(default)]
    pub target_upstream_register: Option<u16>,
    #[serde(default)]
    pub access: ModbusBridgeAccessMode,
}

#[cfg(feature = "modbus")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WidgetServerConfig {
    pub proxy_register: u16,
    #[serde(default)]
    pub access: ModbusBridgeAccessMode,
    #[serde(default)]
    pub target_upstream_register: Option<u16>,
    /// Optional bridge-side protocol declaration.
    ///
    /// Required when widget protocol is not `modbus-tcp` (for example `local`) so
    /// bridge register packing can be defined explicitly.
    #[serde(default)]
    pub protocol: Option<WidgetServerProtocolConfig>,
}

#[cfg(feature = "modbus")]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum WidgetServerProtocolConfig {
    ModbusTcp(WidgetServerModbusTcpConfig),
    #[cfg(feature = "epics")]
    EpicsPva(WidgetServerEpicsPvaConfig),
}

#[cfg(feature = "modbus")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WidgetServerModbusTcpConfig {
    pub register_type: ModbusRegisterType,
    #[serde(default = "default_word_count")]
    pub word_count: u8,
}

#[cfg(all(feature = "modbus", feature = "epics"))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WidgetServerEpicsPvaConfig {
    pub pv_name: String,
}

#[cfg(feature = "modbus")]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ModbusBridgeAccessMode {
    #[default]
    ReadOnly,
    ReadWrite,
}

#[cfg(feature = "modbus")]
fn default_modbus_bridge_listen_addr() -> String {
    "0.0.0.0".to_string()
}

#[cfg(feature = "modbus")]
fn default_modbus_bridge_listen_port() -> u16 {
    1502
}

fn default_allow_env_transport_override() -> bool {
    true
}

impl AppConfig {
    /// Load application configuration from a JSON file.
    pub fn load(path: &str) -> Result<Self, ConfigError> {
        let content = fs::read_to_string(path)?;
        match serde_json::from_str::<AppConfig>(&content) {
            Ok(config) => {
                Self::validate_app_config(&config)?;
                Ok(config)
            }
            Err(e) => {
                let context = ScreenConfig::build_error_context(&e, &content, path);
                Err(ConfigError::JsonError { source: e, context })
            }
        }
    }

    fn validate_app_config(config: &AppConfig) -> Result<(), ConfigError> {
        if let Some(transport) = config.startup.desktop.transport.as_deref() {
            let normalized = transport.trim().to_ascii_lowercase();
            let is_valid = matches!(
                normalized.as_str(),
                "loopback" | "http" | "localhost" | "ipc" | "bridge"
            );
            if !is_valid {
                return Err(ConfigError::ValidationError(format!(
                    "Invalid startup.desktop.transport value: '{}'\n\
                     Expected one of: loopback, http, localhost, ipc, bridge.",
                    transport
                )));
            }
        }

        let window = &config.startup.desktop.window;
        if let Some(width) = window.width {
            if !width.is_finite() || width <= 0.0 {
                return Err(ConfigError::ValidationError(format!(
                    "Invalid startup.desktop.window.width value: {}\n\
                     Width must be a positive finite number.",
                    width
                )));
            }
        }

        if let Some(height) = window.height {
            if !height.is_finite() || height <= 0.0 {
                return Err(ConfigError::ValidationError(format!(
                    "Invalid startup.desktop.window.height value: {}\n\
                     Height must be a positive finite number.",
                    height
                )));
            }
        }

        let mut seen_screen_ids = std::collections::HashSet::new();
        let mut seen_widget_ids = std::collections::HashSet::new();
        for screen in &config.screens {
            if !seen_screen_ids.insert(screen.id.clone()) {
                return Err(ConfigError::ValidationError(format!(
                    "Duplicate screen ID: '{}'\nEach screen must have a unique 'id'.",
                    screen.id
                )));
            }
            ScreenConfig::validate_widgets(&screen.widgets, &mut seen_widget_ids)?;
        }

        #[cfg(feature = "modbus")]
        Self::validate_modbus_bridge_config(config)?;

        Ok(())
    }

    #[cfg(feature = "modbus")]
    fn validate_modbus_bridge_config(config: &AppConfig) -> Result<(), ConfigError> {
        let bridge = &config.startup.modbus_bridge;
        if !bridge.enabled {
            return Ok(());
        }

        if bridge.listen_addr.trim().is_empty() {
            return Err(ConfigError::ValidationError(
                "Invalid startup.modbus_bridge.listen_addr: value cannot be empty".to_string(),
            ));
        }

        let mut seen = std::collections::HashSet::new();
        let mut mapping_count = 0usize;
        for m in &bridge.registers {
            mapping_count += 1;
            if !seen.insert(m.exposed_register) {
                return Err(ConfigError::ValidationError(format!(
                    "Duplicate startup.modbus_bridge mapping for register {}",
                    m.exposed_register
                )));
            }

            if m.source_widget_id.is_none() && m.source_upstream_register.is_none() {
                return Err(ConfigError::ValidationError(format!(
                    "Invalid startup.modbus_bridge mapping for register {}: one of source_widget_id or source_upstream_register is required",
                    m.exposed_register
                )));
            }

            if m.word_count == 0 {
                return Err(ConfigError::ValidationError(format!(
                    "Invalid startup.modbus_bridge mapping for register {}: word_count must be >= 1",
                    m.exposed_register
                )));
            }

            if m.access == ModbusBridgeAccessMode::ReadWrite
                && m.target_upstream_register.is_none()
            {
                return Err(ConfigError::ValidationError(format!(
                    "Invalid startup.modbus_bridge mapping for register {}: target_upstream_register is required for read_write access",
                    m.exposed_register
                )));
            }

            if m.access == ModbusBridgeAccessMode::ReadWrite && bridge.upstream.is_none() {
                return Err(ConfigError::ValidationError(format!(
                    "Invalid startup.modbus_bridge mapping for register {}: upstream config is required for read_write access",
                    m.exposed_register
                )));
            }
        }

        for screen in &config.screens {
            Self::validate_widget_proxy_mappings(
                &screen.widgets,
                bridge,
                &mut seen,
                &mut mapping_count,
            )?;
        }

        if mapping_count == 0 {
            return Err(ConfigError::ValidationError(
                "Invalid startup.modbus_bridge: at least one mapping is required when bridge is enabled (either startup.modbus_bridge.registers or widget server.proxy_register)".to_string(),
            ));
        }

        Ok(())
    }

    #[cfg(feature = "modbus")]
    fn validate_widget_proxy_mappings(
        widgets: &[WidgetConfig],
        bridge: &ModbusBridgeStartupConfig,
        seen: &mut std::collections::HashSet<u16>,
        mapping_count: &mut usize,
    ) -> Result<(), ConfigError> {
        for widget in widgets {
            if let Some(server) = &widget.server {
                *mapping_count += 1;
                if !seen.insert(server.proxy_register) {
                    return Err(ConfigError::ValidationError(format!(
                        "Duplicate modbus bridge mapping for register {}",
                        server.proxy_register
                    )));
                }

                if widget.modbus_tcp().is_none() {
                    let is_local = matches!(widget.protocol, Some(ProtocolConfig::Local(_)));
                    let has_server_protocol = server.protocol.is_some();
                    if !is_local || !has_server_protocol {
                        return Err(ConfigError::ValidationError(format!(
                            "Widget '{}' has server.proxy_register but no bridge protocol format. For local widgets, set server.protocol={{\"type\":\"modbus-tcp\",...}} or server.protocol={{\"type\":\"epics-pva\",...}}",
                            widget.id
                        )));
                    }
                }

                if server.access == ModbusBridgeAccessMode::ReadWrite && bridge.upstream.is_none()
                {
                    return Err(ConfigError::ValidationError(format!(
                        "Widget '{}' has read_write server.proxy_register but startup.modbus_bridge.upstream is missing",
                        widget.id
                    )));
                }
            }

            if let Some(children) = &widget.children {
                Self::validate_widget_proxy_mappings(children, bridge, seen, mapping_count)?;
            }
        }
        Ok(())
    }
}

/// Screen configuration loaded from JSON
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenConfig {
    pub id: String,
    pub title: String,
    /// Human-readable description of the screen.
    /// This field may be omitted in JSON; it defaults to an empty string.
    #[serde(default)]
    pub description: String,
    /// Navigation / action buttons shown in the screen header.
    #[serde(default)]
    pub actions: Option<Vec<ActionConfig>>,
    pub widgets: Vec<WidgetConfig>,
}

// ─── Protocol configuration ───────────────────────────────────────────────────

/// Protocol-specific channel configuration.
///
/// Uses serde's internally-tagged enum so JSON looks like:
/// ```json
/// { "type": "local", "channel": "app:my:value", ... }
/// { "type": "epics-pva", "pv_name": "demo:double", ... }
/// { "type": "modbus-tcp", "host": "127.0.0.1", "register": 1000, ... }
/// ```
/// Adding a new protocol = one new enum variant + struct, no changes to WidgetConfig.
///
/// This enum is extensible because new protocols will be added over time,
/// therefore it will prevent match statments lacking a wildcard arm.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ProtocolConfig {
    Local(LocalConfig),
    #[cfg(feature = "epics")]
    EpicsPva(EpicsPvaConfig),
    #[cfg(feature = "modbus")]
    ModbusTcp(ModbusTCPConfig),
}

/// In-process local channel configuration.
///
/// Local channels never send data on the network. Values are shared only inside
/// the running mycela process and still flow through the normal SSE/IPC paths.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalConfig {
    /// Logical local channel name (e.g. "app:temperature:setpoint").
    pub channel: String,
    /// Optional initial value used when the channel is first created.
    #[serde(default)]
    pub initial_value: Option<String>,
}

/// EPICS Process Variable Access channel configuration.
#[cfg(feature = "epics")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpicsPvaConfig {
    /// EPICS PV name (e.g. "demo:double")
    pub pv_name: String,
    /// Optional embedded PVXS server PV definition (creates the PV on start-up)
    #[serde(default)]
    pub server: Option<ServerConfig>,
    /// Extra PV names for multi-series line charts.
    /// Maximum 5 additional PVs are accepted (6 total including the primary `pv_name`);
    /// any further entries are silently ignored.
    #[serde(default)]
    pub pv_names: Option<Vec<String>>,
}

#[cfg(feature = "epics")]
impl EpicsPvaConfig {
    /// All PV names for this widget — primary first, then up to 5 extra series.
    /// The 6-series cap matches the server-side limit enforced in `setup_server_pvs`.
    pub fn series_pvs(&self) -> Vec<String> {
        let mut pvs = vec![self.pv_name.clone()];
        if let Some(extras) = &self.pv_names {
            pvs.extend(extras.iter().take(5).cloned());
        }
        pvs
    }
}

/// Modbus TCP channel configuration.
#[cfg(feature = "modbus")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModbusTCPConfig {
    /// Modbus server hostname or IP address
    pub host: String,
    /// TCP port (default: 502)
    #[serde(default = "default_modbus_port")]
    pub port: u16,
    /// Modbus unit ID (default: 1)
    #[serde(default = "default_unit_id", alias = "slave_id")]
    pub unit_id: u8,
    /// Starting register address
    pub register: u16,
    /// Register type
    pub register_type: ModbusRegisterType,
    /// Minimum poll interval in milliseconds (default: 500); actual rate may be lower under load
    #[serde(default = "default_min_poll_interval_ms", alias = "poll_interval_ms")]
    pub min_poll_interval_ms: u64,
    /// Scale factor applied to the raw register value: physical = raw * scale + offset
    #[serde(default = "default_scale")]
    pub scale: f64,
    /// Offset applied after scaling: physical = raw * scale + offset
    #[serde(default = "default_offset")]
    pub offset: f64,
    /// Number of 16-bit registers to read (1 = u16, 2 = f32/u32 big-endian)
    #[serde(default = "default_word_count")]
    pub word_count: u8,
    /// Optional bit index (0..15) to extract from the first 16-bit word.
    /// Useful when status flags are packed into a 3x/4x register.
    #[serde(default)]
    pub bit_index: Option<u8>,
}

#[cfg(feature = "modbus")]
fn default_modbus_port() -> u16 {
    502
}
#[cfg(feature = "modbus")]
fn default_unit_id() -> u8 {
    1
}
#[cfg(feature = "modbus")]
fn default_min_poll_interval_ms() -> u64 {
    500
}
#[cfg(feature = "modbus")]
fn default_scale() -> f64 {
    1.0
}
#[cfg(feature = "modbus")]
fn default_offset() -> f64 {
    0.0
}
#[cfg(feature = "modbus")]
fn default_word_count() -> u8 {
    1
}

/// Modbus register / coil type.
#[cfg(feature = "modbus")]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ModbusRegisterType {
    HoldingRegister,
    InputRegister,
    Coil,
    DiscreteInput,
}

/// Individual widget configuration.
///
/// Every widget is described by a sinle `WidgetConfig` struct.
///
/// Required fields: `id`, `widget_type`, `label`, `protocol` and `data_type`.
///
/// **Key insights**:
/// - `WidgetConfig` can be cloned freely — it is a plain data struct with no live connections, so it is cheap to pass around.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WidgetConfig {
    /// Unique widget ID (used for DOM attributes and as a key in the widget registry).
    pub id: String,
    /// Enum for widget all types. This may be extended in the future.
    #[serde(rename = "type")]
    pub widget_type: WidgetType,
    /// Label string for the widget.
    pub label: String,
    /// Protocol and channel address for this widget.
    /// Required for all data widgets, ignored for containers, e.g. Group.
    #[serde(default)]
    pub protocol: Option<ProtocolConfig>,
    /// Primitive data type for this widget's channel (e.g. "string", "double", "boolean", "enum").
    /// Required for all data widgets; ignored for containers, e.g. Group.
    #[serde(default)]
    pub data_type: Option<String>,
    /// Optional Modbus bridge server proxy settings.
    ///
    /// When set for a widget using `protocol.type = "modbus-tcp"`, this widget
    /// is exposed by the embedded Modbus server on `proxy_register`.
    /// The bridge automatically inherits register semantics from the widget
    /// protocol (register type and poll behavior).
    #[cfg(feature = "modbus")]
    #[serde(default)]
    pub server: Option<WidgetServerConfig>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub style: Option<WidgetStyle>,
    /// Enum choice labels for Select widgets backed by enum PVs
    #[serde(default)]
    pub options: Option<Vec<String>>,
    /// Gauge orientation: "horizontal" (default) or "vertical"
    #[serde(default)]
    pub orientation: Option<String>,
    /// Heading level for Group containers: 1 (H1), 2 (H2), 3 (H3). Default: 1
    #[serde(default)]
    pub level: Option<u8>,
    /// Child widgets for Group containers
    #[serde(default)]
    pub children: Option<Vec<WidgetConfig>>,
    /// Maximum data points for Chart widgets (default: 100)
    #[serde(default)]
    pub max_points: Option<usize>,
    /// Chart type: "line" (default), "histogram", "scatter", "scatter_histogram"
    #[serde(default)]
    pub chart_type: Option<String>,
    /// X-axis label
    #[serde(default)]
    pub axis_label_x: Option<String>,
    /// Y-axis label
    #[serde(default)]
    pub axis_label_y: Option<String>,
    /// Explicit size for Group containers (sets min-width / min-height via inline CSS)
    #[serde(default)]
    pub size: Option<WidgetSize>,
    /// Widget-level default metadata (display limits, units, precision, alarm bands).
    /// Used as fallback when the protocol backend has not yet delivered its own metadata
    /// (e.g. EPICS PVA before the first monitor update) and as the primary metadata
    /// source for protocols that carry no metadata themselves (e.g. Modbus TCP).
    #[serde(default)]
    pub metadata: Option<PvMetadata>,
    /// SVG polygon `points` attribute for the `ValveState` widget.
    /// Defaults to a rectangle; set to a bowtie shape in application config.
    /// Example bowtie (30 × 18): `"0,0 0,18 15,9 30,18 30,0 15,9"`
    #[serde(default)]
    pub polygon_points: Option<String>,
    /// Invert the open/closed interpretation of a `ValveState` register.
    /// Set `true` when the register is a "closed" flag (1 = closed, 0 = open).
    #[serde(default)]
    pub invert: Option<bool>,
    /// Position of the label and status text relative to the polygon SVG.
    /// Accepted values: `"top"`, `"bottom"`, `"left"` (default), `"right"`.
    #[serde(default)]
    pub label_position: Option<String>,
    /// Colour theme for buttons.
    /// Accepted values: `"green"`, `"red"`, `"blue"` (default).
    #[serde(default)]
    pub color: Option<String>,
    /// Value written to the channel when a widget is actioned. 
    /// For example, when a button is clicked. 
    /// Defaults to `1` (ON). Set to `0` for a button that writes OFF/close.
    #[serde(default)]
    pub write_value: Option<f64>,
    /// Optional delayed auto-reset for `toggle_button` writes, in milliseconds.
    ///
    /// When set to a non-zero value, a toggle write is followed by an automatic
    /// write of `reset_default` after the delay.
    #[serde(default)]
    pub reset_timeout: Option<u64>,
    /// Default integer value written by delayed toggle auto-reset.
    ///
    /// Used by `toggle_button` when `reset_timeout` is set to a non-zero value.
    /// If omitted, the reset target defaults to `0`.
    #[serde(default)]
    pub reset_default: Option<f64>,
}

impl WidgetConfig {
    /// Returns a human-readable channel address for logging and the `data-ch` DOM attribute.
    pub fn channel_address(&self) -> String {
        match &self.protocol {
            Some(ProtocolConfig::Local(l)) => format!("local://{}", l.channel),
            #[cfg(feature = "epics")]
            Some(ProtocolConfig::EpicsPva(e)) => e.pv_name.clone(),
            #[cfg(feature = "modbus")]
            Some(ProtocolConfig::ModbusTcp(m)) => {
                format!("modbus-tcp://{}:{}/reg{}", m.host, m.port, m.register)
            }
            _ => String::new(),
        }
    }

    /// Returns the `LocalConfig` if this widget uses the `local` protocol.
    pub fn local(&self) -> Option<&LocalConfig> {
        match &self.protocol {
            Some(ProtocolConfig::Local(l)) => Some(l),
            _ => None,
        }
    }

    /// Returns the `EpicsPvaConfig` if this widget uses the `epics-pva` protocol.
    #[cfg(feature = "epics")]
    pub fn epics_pva(&self) -> Option<&EpicsPvaConfig> {
        match &self.protocol {
            Some(ProtocolConfig::EpicsPva(e)) => Some(e),
            _ => None,
        }
    }

    /// Returns the `ModbusTCPConfig` if this widget uses the `modbus-tcp` protocol.
    #[cfg(feature = "modbus")]
    pub fn modbus_tcp(&self) -> Option<&ModbusTCPConfig> {
        match &self.protocol {
            Some(ProtocolConfig::ModbusTcp(m)) => Some(m),
            _ => None,
        }
    }
}

/// Server configuration for providing an EPICS PV (lives inside `EpicsPvaConfig.server`).
#[cfg(feature = "epics")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Alarm severity for this PV's initial state. Accepted values: `NONE`, `MINOR`, `MAJOR`, `INVALID`.
    #[serde(default)]
    pub alarm_severity: Option<String>,
    #[serde(default)]
    pub alarm_status: Option<String>,
    #[serde(default)]
    pub alarm_message: Option<String>,
    #[serde(default)]
    pub metadata: Option<PvMetadata>,
}

/// PV metadata configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PvMetadata {
    #[serde(default)]
    pub display: Option<DisplayMetadata>,
    #[serde(default)]
    pub control: Option<ControlMetadata>,
    #[serde(default)]
    pub alarm: Option<AlarmMetadata>,
}

/// Display metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayMetadata {
    pub limit_low: f64,
    pub limit_high: f64,
    pub description: String,
    pub precision: i32,
    pub units: String,
}

/// Control metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlMetadata {
    pub limit_low: f64,
    pub limit_high: f64,
    #[serde(default)]
    pub min_step: f64,
}

/// Alarm metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlarmMetadata {
    pub low_alarm_limit: f64,
    pub low_warning_limit: f64,
    pub high_alarm_limit: f64,
    pub high_warning_limit: f64,
    pub low_alarm_severity: String,
    pub low_warning_severity: String,
    pub high_warning_severity: String,
    pub high_alarm_severity: String,
    pub hysteresis: i32,
}

impl AlarmMetadata {
    fn severity_int(s: &str) -> i32 {
        match s {
            "MAJOR" => 2,
            "MINOR" => 1,
            _ => 0,
        }
    }

    /// Compute alarm severity (0=none, 1=MINOR, 2=MAJOR) for a given scalar value.
    pub fn compute_severity(&self, value: f64) -> i32 {
        if value < self.low_alarm_limit {
            Self::severity_int(&self.low_alarm_severity)
        } else if value > self.high_alarm_limit {
            Self::severity_int(&self.high_alarm_severity)
        } else if value < self.low_warning_limit {
            Self::severity_int(&self.low_warning_severity)
        } else if value > self.high_warning_limit {
            Self::severity_int(&self.high_warning_severity)
        } else {
            0
        }
    }
}

/// Widget type enumeration
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum WidgetType {
    TextEntry,
    #[default]
    TextUpdate,
    Gauge,
    Led,
    Button,
    ToggleButton,
    Slider,
    Chart,
    Select,
    Group,
    /// Multi-state polygon LED: open (green) / closed (red) / pending (grey).
    /// Shape defaults to a rectangle; override with `polygon_points`.
    MultiStateLed,
    /// Hidden widget type used for hold values internally. Not rendered in the UI.
    Hidden,
}

/// Explicit container size for Group widgets (applied as inline min-width/min-height)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WidgetSize {
    #[serde(default)]
    pub width: Option<String>,
    #[serde(default)]
    pub height: Option<String>,
}

/// Optional widget styling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WidgetStyle {
    #[serde(default)]
    pub width: Option<String>,
    #[serde(default)]
    pub height: Option<String>,
    #[serde(default)]
    pub background: Option<String>,
    #[serde(default)]
    pub left: Option<String>,
    #[serde(default)]
    pub top: Option<String>,
}

impl ScreenConfig {
    /// Load screen configuration from JSON file
    pub fn load(path: &str) -> Result<Self, ConfigError> {
        let content = fs::read_to_string(path)?;

        match serde_json::from_str::<ScreenConfig>(&content) {
            Ok(config) => {
                // Validate the config has required fields populated correctly
                Self::validate_config(&config)?;
                Ok(config)
            }
            Err(e) => {
                let context = Self::build_error_context(&e, &content, path);
                Err(ConfigError::JsonError { source: e, context })
            }
        }
    }

    /// Validate that the configuration has all required data
    pub fn validate_config(config: &ScreenConfig) -> Result<(), ConfigError> {
        let mut seen_ids = std::collections::HashSet::new();
        Self::validate_widgets(&config.widgets, &mut seen_ids)
    }

    /// Recursively validate widgets (including children of groups)
    fn validate_widgets(
        widgets: &[WidgetConfig],
        seen_ids: &mut std::collections::HashSet<String>,
    ) -> Result<(), ConfigError> {
        for (idx, widget) in widgets.iter().enumerate() {
            if !seen_ids.insert(widget.id.clone()) {
                return Err(ConfigError::ValidationError(format!(
                    "Widget #{} has duplicate ID: '{}'\n\
                     Each widget must have a unique 'id' field.",
                    idx + 1,
                    widget.id
                )));
            }
            if let Some(children) = &widget.children {
                Self::validate_widgets(children, seen_ids)?;
            }
        }
        Ok(())
    }

    /// Build a helpful error context message
    fn build_error_context(error: &serde_json::Error, content: &str, path: &str) -> String {
        let mut context = format!("File: {}\n", path);

        // Try to determine what's wrong and where
        let line = error.line();
        if line > 0 {
            context.push_str(&format!("Line: {}, Column: {}\n\n", line, error.column()));

            // Show the problematic line and surrounding context
            let lines: Vec<&str> = content.lines().collect();
            let start = line.saturating_sub(3);
            let end = (line + 2).min(lines.len());

            context.push_str("Context:\n");
            for (i, line_content) in lines[start..end].iter().enumerate() {
                let line_num = start + i + 1;
                if line_num == line {
                    context.push_str(&format!("  {}: {}\n", line_num, line_content));
                } else {
                    context.push_str(&format!("    {}: {}\n", line_num, line_content));
                }
            }
            context.push_str("\n");
        }

        // Add helpful hints based on error message
        let error_msg = error.to_string();
        context.push_str("Error: ");
        context.push_str(&error_msg);
        context.push_str("\n\n");

        if error_msg.contains("missing field") {
            if let Some(field_name) = Self::extract_field_name(&error_msg) {
                context.push_str("💡 Hint: ");
                context.push_str(&Self::get_field_hint(&field_name));
                context.push_str("\n");
            }
        } else if error_msg.contains("unknown variant") || error_msg.contains("unknown field") {
            context.push_str("💡 Hint: Check for typos in field names or enum values.\n");
            context.push_str("   Valid widget types: text_entry, text_update, gauge, led, button, slider, chart, select, toggle_button, group, multi_state_led\n");
        } else if error_msg.contains("invalid type") {
            context.push_str("💡 Hint: Check that the field has the correct data type (string, number, boolean, etc.)\n");
        }

        context
    }

    /// Extract field name from serde error message
    fn extract_field_name(error_msg: &str) -> Option<String> {
        // Pattern: "missing field `fieldname`"
        if let Some(start) = error_msg.find("missing field `") {
            let start = start + 15; // length of "missing field `"
            if let Some(end) = error_msg[start..].find('`') {
                return Some(error_msg[start..start + end].to_string());
            }
        }
        None
    }

    /// Get helpful hint for a missing field
    fn get_field_hint(field_name: &str) -> String {
        match field_name {
            "id" => "Each widget must have a unique 'id' field (string).".to_string(),
            "type" => "Each widget must have a 'type' field. Valid types: text_entry, text_update, gauge, led, button, slider, chart, select, toggle_button, group".to_string(),
            "label" => "Each widget must have a 'label' field for display (string).".to_string(),
            "title" => "The config root must have a 'title' field (string).".to_string(),
            "description" => "The config root must have a 'description' field (string).".to_string(),
            "widgets" => "The config root must have a 'widgets' array containing widget configurations.".to_string(),
            "pv_name" => "Inside an 'epics-pva' protocol block, 'pv_name' must be set to the EPICS PV name.".to_string(),
            "host" => "Inside a 'modbus' protocol block, 'host' must be set to the device IP/hostname.".to_string(),
            "register" => "Inside a 'modbus' protocol block, 'register' must be the register address (u16).".to_string(),
            "register_type" => "Inside a 'modbus' protocol block, 'register_type' must be one of: holding_register, input_register, coil, discrete_input.".to_string(),
            _ => format!("The field '{}' is required but missing.", field_name),
        }
    }

    /// Save screen configuration to JSON file
    pub fn save(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::AppConfig;

    fn minimal_screen_json() -> &'static str {
        r#"
        {
          "id": "screen_one",
          "title": "Screen One",
          "description": "desc",
          "widgets": []
        }
        "#
    }

    #[test]
    fn startup_defaults_when_missing() {
        let json = format!(
            r#"{{
              "title": "Test",
              "screens": [{}]
            }}"#,
            minimal_screen_json()
        );

        let config: AppConfig = serde_json::from_str(&json).expect("config should parse");
        assert!(config.startup.desktop.transport.is_none());
        assert!(config.startup.desktop.allow_env_transport_override);
        assert!(config.startup.desktop.window.title.is_none());
        assert!(config.startup.desktop.window.width.is_none());
        assert!(config.startup.desktop.window.height.is_none());
    }

    #[test]
    fn startup_valid_transport_values_pass_validation() {
        for value in ["loopback", "http", "localhost", "ipc", "bridge"] {
            let json = format!(
                r#"{{
                  "title": "Test",
                  "startup": {{ "desktop": {{ "transport": "{}" }} }},
                  "screens": [{}]
                }}"#,
                value,
                minimal_screen_json()
            );

            let config: AppConfig = serde_json::from_str(&json).expect("config should parse");
            AppConfig::validate_app_config(&config).expect("transport should validate");
        }
    }

    #[test]
    fn startup_invalid_transport_fails_validation() {
        let json = format!(
            r#"{{
              "title": "Test",
              "startup": {{ "desktop": {{ "transport": "serial" }} }},
              "screens": [{}]
            }}"#,
            minimal_screen_json()
        );

        let config: AppConfig = serde_json::from_str(&json).expect("config should parse");
        let error = AppConfig::validate_app_config(&config).expect_err("validation should fail");
        let text = format!("{}", error);
        assert!(text.contains("startup.desktop.transport"));
    }

    #[test]
    fn startup_invalid_window_dimensions_fail_validation() {
        let width_json = format!(
            r#"{{
              "title": "Test",
              "startup": {{ "desktop": {{ "window": {{ "width": 0.0 }} }} }},
              "screens": [{}]
            }}"#,
            minimal_screen_json()
        );
        let width_config: AppConfig =
            serde_json::from_str(&width_json).expect("config should parse");
        let width_error = AppConfig::validate_app_config(&width_config)
            .expect_err("width validation should fail");
        assert!(format!("{}", width_error).contains("startup.desktop.window.width"));

        let height_json = format!(
            r#"{{
              "title": "Test",
              "startup": {{ "desktop": {{ "window": {{ "height": -1.0 }} }} }},
              "screens": [{}]
            }}"#,
            minimal_screen_json()
        );
        let height_config: AppConfig =
            serde_json::from_str(&height_json).expect("config should parse");
        let height_error = AppConfig::validate_app_config(&height_config)
            .expect_err("height validation should fail");
        assert!(format!("{}", height_error).contains("startup.desktop.window.height"));
    }
}
