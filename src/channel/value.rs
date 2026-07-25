/// A normalised snapshot of a channel value, protocol-independent.
///
/// Contains fields designed to be protocol independent.
///
/// Fields that are not meaningful for a given protocol default to safe values
/// (zero / empty string / 0–100 display range) so widget render functions can
/// use them unconditionally.
#[derive(Debug, Clone)]
pub struct ChannelValue {
    /// Scalar numeric value (applies to all non-array types)
    pub raw_value: f64,
    /// Pre-formatted display string (honours precision, handles integers / strings)
    pub value_str: String,
    /// Sample array for single-series chart widgets
    pub array_values: Vec<f64>,
    /// Sample arrays for multi-series line charts
    pub named_series: Vec<(String, Vec<f64>)>,
    /// Alarm severity  (0 = NO_ALARM, 1 = MINOR, 2 = MAJOR, 3 = INVALID)
    pub alarm_severity: i32,
    /// Alarm status code (protocol-specific; 0 = no alarm)
    pub alarm_status: i32,
    /// Engineering units string (e.g. "mm", "°C")
    pub units: String,
    /// Display range low limit
    pub display_low: f64,
    /// Display range high limit
    pub display_high: f64,
    /// Controllable range low limit
    pub control_low: f64,
    /// Controllable range high limit
    pub control_high: f64,
    /// Number of decimal places for display
    pub precision: i32,
    /// Alarm/warning band limits
    pub low_alarm_limit: f64,
    pub low_warn_limit: f64,
    pub high_warn_limit: f64,
    pub high_alarm_limit: f64,
    /// Current enum index (Select / ToggleButton widgets)
    pub enum_index: i16,
    /// Enum choice strings (Select widget)
    pub enum_choices: Vec<String>,
    /// Extra metadata used by multi-series Chart rendering
    pub primary_meta: PrimaryMeta,
}

impl Default for ChannelValue {
    fn default() -> Self {
        Self {
            raw_value: 0.0,
            value_str: String::new(),
            array_values: Vec::new(),
            named_series: Vec::new(),
            alarm_severity: 3, // INVALID
            alarm_status: 3,   // INVALID
            units: String::new(),
            display_low: std::f64::MIN,
            display_high: std::f64::MAX,
            control_low: std::f64::MIN,
            control_high: std::f64::MAX,
            precision: -1,
            low_alarm_limit: std::f64::MIN,
            low_warn_limit: std::f64::MIN,
            high_warn_limit: std::f64::MAX,
            high_alarm_limit: std::f64::MAX,
            enum_index: -1,
            enum_choices: Vec::new(),
            primary_meta: PrimaryMeta::default(),
        }
    }
}

/// Lightweight metadata snapshot used by multi-series chart rendering.
#[derive(Debug, Clone, Default)]
pub struct PrimaryMeta {
    pub alarm_severity: i32,
    pub description: String,
    pub units: String,
    pub limit_lo: f64,
    pub limit_hi: f64,
}
