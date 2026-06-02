use serde::{Deserialize, Serialize};
use std::fs;
use std::fmt;

/// Custom error type for configuration loading
#[derive(Debug)]
pub enum ConfigError {
    FileError(std::io::Error),
    JsonError { source: serde_json::Error, context: String },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::FileError(e) => write!(f, "Failed to read config file: {}", e),
            ConfigError::JsonError { source, context } => {
                write!(f, "Configuration JSON error: {}\n{}", source, context)
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
    Api { label: String, method: String, path: String },
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
    pub screens: Vec<ScreenConfig>,
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
        let mut seen_screen_ids = std::collections::HashSet::new();
        let mut seen_widget_ids = std::collections::HashSet::new();
        for screen in &config.screens {
            if !seen_screen_ids.insert(screen.id.clone()) {
                let context = format!(
                    "Duplicate screen ID: '{}'\nEach screen must have a unique 'id'.",
                    screen.id
                );
                let err = serde_json::from_str::<()>("\"duplicate_screen_id\"").unwrap_err();
                return Err(ConfigError::JsonError { source: err, context });
            }
            ScreenConfig::validate_widgets(&screen.widgets, &mut seen_widget_ids)?;
        }
        Ok(())
    }
}

/// Screen configuration loaded from JSON
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenConfig {
    pub id: String,
    pub title: String,
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
/// { "type": "epics-pva", "pv_name": "demo:double", ... }
/// { "type": "modbus-tcp", "host": "127.0.0.1", "register": 1000, ... }
/// ```
/// Adding a new protocol = one new enum variant + struct, no changes to WidgetConfig.
/// 
/// This enum is extensible because new protocols will be added over time, 
/// therefore is use will prevent match statments lacking a wildcard arm.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ProtocolConfig {
    #[cfg(feature = "epics")]
    EpicsPva(EpicsPvaConfig),
    #[cfg(feature = "modbus")]
    ModbusTcp(ModbusTCPConfig),
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
    /// Extra PV names for multi-series line charts (max 5 additional, 6 total)
    #[serde(default)]
    pub pv_names: Option<Vec<String>>,
}

#[cfg(feature = "epics")]
impl EpicsPvaConfig {
    /// All PV names for this widget (primary + up to 5 extra series for charts).
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
fn default_modbus_port() -> u16 { 502 }
#[cfg(feature = "modbus")]
fn default_unit_id() -> u8 { 1 }
#[cfg(feature = "modbus")]
fn default_min_poll_interval_ms() -> u64 { 500 }
#[cfg(feature = "modbus")]
fn default_scale() -> f64 { 1.0 }
#[cfg(feature = "modbus")]
fn default_offset() -> f64 { 0.0 }
#[cfg(feature = "modbus")]
fn default_word_count() -> u8 { 1 }

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


/// Individual widget configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WidgetConfig {
    pub id: String,
    #[serde(rename = "type")]
    pub widget_type: WidgetType,
    pub label: String,
    /// Protocol and channel address for this widget.
    /// Required for all data widgets (everything except Group containers).
    #[serde(default)]
    pub protocol: Option<ProtocolConfig>,
    #[serde(default)]
    pub data_type: Option<String>,
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
    /// Value written to the channel when a button is clicked.
    /// Defaults to `1` (ON). Set to `0` for a button that writes OFF/close.
    #[serde(default)]
    pub write_value: Option<u16>,
}

impl WidgetConfig {
    /// Returns a human-readable channel address for logging and the `data-ch` DOM attribute.
    pub fn channel_address(&self) -> String {
        match &self.protocol {
            #[cfg(feature = "epics")]
            Some(ProtocolConfig::EpicsPva(e)) => e.pv_name.clone(),
            #[cfg(feature = "modbus")]
            Some(ProtocolConfig::ModbusTcp(m)) => {
                format!("modbus-tcp://{}:{}/reg{}", m.host, m.port, m.register)
            }
            _ => String::new(),
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
    #[serde(default)]
    pub alarm_serverity: Option<String>,
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
        match s { "MAJOR" => 2, "MINOR" => 1, _ => 0 }
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
    /// Shape defaults to a rectangle; override with `polygon_points` for a bowtie.
    MultiStateLed,
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
                let context = format!(
                    "Widget #{} has duplicate ID: '{}'\n\
                     Each widget must have a unique 'id' field.",
                    idx + 1, widget.id
                );
                let err = serde_json::from_str::<()>("\"duplicate_id\"")
                    .unwrap_err();
                return Err(ConfigError::JsonError { 
                    source: err,
                    context 
                });
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
            context.push_str("   Valid widget types: text_entry, text_update, gauge, led, button, slider, chart, select, toggle_button, group\n");
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
