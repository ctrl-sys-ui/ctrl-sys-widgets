use std::sync::Arc;
use dashmap::DashMap;
use tokio::sync::watch;

// ─── Unified value type ───────────────────────────────────────────────────────

/// A normalised snapshot of a channel value, protocol-independent.
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
            alarm_status: 3, // INVALID
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

// ─── Channel events ───────────────────────────────────────────────────────────

/// Events emitted by a channel stream, independent of the underlying protocol.
#[derive(Debug)]
pub enum ChannelEvent {
    /// The channel has successfully connected to its data source.
    Connected,
    /// The channel has disconnected (e.g. device offline, remote server not found).
    Disconnected(String),
    /// A new value has been received from the data source.
    Value(ChannelValue),
    /// An error occurred (connection failure, protocol error, etc.).
    Error(String),
}

// ─── Channel context ──────────────────────────────────────────────────────────

/// Holds all protocol-level handles needed to create channel streams.
/// Passed through `AppState` and into every SSE handler.
/// Add new protocol handles here when new protocols are introduced.
pub struct ChannelContext {
    pub local_store: Arc<crate::local_channel::LocalStore>,
    #[cfg(feature = "epics")]
    pub epics_ctx: Arc<std::sync::Mutex<pvxs_sys::Context>>,
    #[cfg(feature = "modbus")]
    pub modbus_pool: Arc<crate::modbus_client::ModbusPool>,
    #[cfg(feature = "modbus-server")]
    pub modbus_bridge: Arc<crate::modbus_bridge::ModbusBridgeContext>,
    /// Per-widget enabled/disabled state bus. `false` = enabled (default).
    pub widget_enabled: DashMap<String, watch::Sender<bool>>,
    /// Per-widget latest-value bus, for app-logic subscriptions.
    pub widget_value_bus: DashMap<String, watch::Sender<ChannelValue>>,
    /// Per-widget connection state bus. `false` = connected (default when unknown).
    pub widget_connected: DashMap<String, watch::Sender<bool>>,
}

impl ChannelContext {
    /// Enable or disable a widget by ID.
    ///
    /// The change propagates to any running widget monitor via a watch channel,
    /// triggering an immediate re-render without waiting for the next data poll.
    pub fn set_widget_enabled(&self, widget_id: &str, enabled: bool) {
        match self.widget_enabled.get(widget_id) {
            Some(tx) => { tx.send_replace(enabled); }
            None => {
                let (tx, _rx) = watch::channel(enabled);
                self.widget_enabled.insert(widget_id.to_string(), tx);
            }
        }
    }

    /// Subscribe to the enabled state of a widget. Returns `false` by default
    /// if no explicit state has been set yet.
    pub fn subscribe_widget_enabled(&self, widget_id: &str) -> watch::Receiver<bool> {
        self.widget_enabled
            .entry(widget_id.to_string())
            .or_insert_with(|| watch::channel(false).0)
            .subscribe()
    }

    /// Publish the latest channel value for a widget onto the value bus.
    /// Widget monitors call this so app logic can react to value changes.
    pub fn publish_widget_value(&self, widget_id: &str, cv: ChannelValue) {
        match self.widget_value_bus.get(widget_id) {
            Some(tx) => { tx.send_replace(cv); }
            None => {
                let (tx, _rx) = watch::channel(cv);
                self.widget_value_bus.insert(widget_id.to_string(), tx);
            }
        }
    }

    /// Subscribe to the latest channel value stream for a widget.
    /// Useful for app-level logic that reacts to live data (e.g. enable/disable
    /// controls based on sensor readings).
    pub fn subscribe_widget_value(&self, widget_id: &str) -> watch::Receiver<ChannelValue> {
        self.widget_value_bus
            .entry(widget_id.to_string())
            .or_insert_with(|| watch::channel(ChannelValue::default()).0)
            .subscribe()
    }

    /// Publish the current connection state for a widget.
    pub fn set_widget_connected(&self, widget_id: &str, connected: bool) {
        match self.widget_connected.get(widget_id) {
            Some(tx) => {
                tx.send_replace(connected);
            }
            None => {
                let (tx, _rx) = watch::channel(connected);
                self.widget_connected.insert(widget_id.to_string(), tx);
            }
        }
    }

    /// Returns the latest known connection state for a widget.
    ///
    /// Defaults to `true` when no monitor has published state yet to avoid
    /// rejecting writes in startup/test paths where monitors are not running.
    pub fn is_widget_connected(&self, widget_id: &str) -> bool {
        self.widget_connected
            .get(widget_id)
            .map(|tx| *tx.borrow())
            .unwrap_or(true)
    }

    #[cfg(all(feature = "epics", feature = "modbus"))]
    pub fn new(
        epics_ctx: Arc<std::sync::Mutex<pvxs_sys::Context>>,
        modbus_pool: Arc<crate::modbus_client::ModbusPool>,
    ) -> Arc<Self> {
        Arc::new(Self {
            local_store: crate::local_channel::LocalStore::new(),
            epics_ctx,
            modbus_pool,
            #[cfg(feature = "modbus-server")]
            modbus_bridge: crate::modbus_bridge::ModbusBridgeContext::new(),
            widget_enabled: DashMap::new(),
            widget_value_bus: DashMap::new(),
            widget_connected: DashMap::new(),
        })
    }

    #[cfg(all(feature = "epics", not(feature = "modbus")))]
    pub fn new(epics_ctx: Arc<std::sync::Mutex<pvxs_sys::Context>>) -> Arc<Self> {
        Arc::new(Self {
            local_store: crate::local_channel::LocalStore::new(),
            epics_ctx,
            widget_enabled: DashMap::new(),
            widget_value_bus: DashMap::new(),
            widget_connected: DashMap::new(),
        })
    }

    #[cfg(all(not(feature = "epics"), feature = "modbus"))]
    pub fn new(modbus_pool: Arc<crate::modbus_client::ModbusPool>) -> Arc<Self> {
        Arc::new(Self {
            local_store: crate::local_channel::LocalStore::new(),
            modbus_pool,
            #[cfg(feature = "modbus-server")]
            modbus_bridge: crate::modbus_bridge::ModbusBridgeContext::new(),
            widget_enabled: DashMap::new(),
            widget_value_bus: DashMap::new(),
            widget_connected: DashMap::new(),
        })
    }

    #[cfg(all(not(feature = "epics"), not(feature = "modbus")))]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            local_store: crate::local_channel::LocalStore::new(),
            widget_enabled: DashMap::new(),
            widget_value_bus: DashMap::new(),
            widget_connected: DashMap::new(),
        })
    }
}

// ─── Routing ─────────────────────────────────────────────────────────────────

/// Create a live stream of `ChannelEvent`s for the given widget config.
///
/// Routes to the correct protocol backend based on `config.protocol`.
/// Returns a boxed `Stream` so callers need not know which backend is active.
pub fn channel_stream(
    config: Arc<crate::config::WidgetConfig>,
    ctx: Arc<ChannelContext>,
) -> futures::stream::BoxStream<'static, ChannelEvent> {
    use crate::config::ProtocolConfig;

    if let Some(ProtocolConfig::Local(_)) = config.protocol.as_ref() {
        return Box::pin(crate::local_channel::local_stream(
            config,
            ctx.local_store.clone(),
        ));
    }
    
    #[cfg(feature = "epics")]
    if matches!(
        config.protocol.as_ref(),
        Some(ProtocolConfig::EpicsPva(_)) | None
    ) {
        return Box::pin(crate::epics_channel::epics_stream(
            config,
            ctx.epics_ctx.clone(),
        ));
    }

    #[cfg(feature = "modbus")]
    if let Some(ProtocolConfig::ModbusTcp(_)) = config.protocol.as_ref() {
        return Box::pin(crate::modbus_client::modbus_stream(
            config,
            ctx.modbus_pool.clone(),
        ));
    }

    // No protocol configured or no matching feature enabled.
    let _ = ctx;
    Box::pin(futures::stream::empty::<ChannelEvent>())
}
