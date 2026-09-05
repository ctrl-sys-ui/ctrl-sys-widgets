use std::sync::Arc;
use tokio::sync::watch;
use dashmap::DashMap;
use crate::channel::value::ChannelValue;

/// Holds all protocol-level handles needed to create channel streams.
/// Passed through `AppState` and into every SSE handler.
/// Add new protocol handles here when new protocols are introduced.
#[non_exhaustive]
pub struct ChannelContext {
    pub local_store: Arc<crate::local_channel::LocalStore>,
    #[cfg(feature = "epics-pvxs")]
    pub epics_ctx: Arc<std::sync::Mutex<pvxs::Context>>,
    #[cfg(feature = "modbus")]
    pub modbus_pool: Arc<crate::modbus_client::ModbusPool>,
    #[cfg(feature = "modbus-server")]
    pub modbus_bridge: Arc<crate::modbus_bridge::ModbusBridgeContext>,
    /// Persistent ASCII-TCP connections, shared so widgets on one endpoint never overlap.
    #[cfg(feature = "ascii-tcp")]
    pub ascii_tcp_pool: Arc<ascii_tcp::ConnectionPool>,
    /// Per-widget enabled/disabled state bus. `true` = enabled (default for unmanaged widgets).
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
            Some(tx) => {
                tx.send_replace(enabled);
            }
            None => {
                let (tx, _rx) = watch::channel(enabled);
                self.widget_enabled.insert(widget_id.to_string(), tx);
            }
        }
    }

    /// Subscribe to the enabled state of a widget. Returns `true` (enabled) by default
    /// when no explicit state has been set yet, so unmanaged widgets are active once connected.
    pub fn subscribe_widget_enabled(&self, widget_id: &str) -> watch::Receiver<bool> {
        self.widget_enabled
            .entry(widget_id.to_string())
            .or_insert_with(|| watch::channel(true).0)
            .subscribe()
    }

    /// Publish the latest channel value for a widget onto the value bus.
    /// Widget monitors call this so app logic can react to value changes.
    pub fn publish_widget_value(&self, widget_id: &str, cv: ChannelValue) {
        match self.widget_value_bus.get(widget_id) {
            Some(tx) => {
                tx.send_replace(cv);
            }
            None => {
                let (tx, _rx) = watch::channel(cv);
                self.widget_value_bus.insert(widget_id.to_string(), tx);
            }
        }
    }

    /// Subscribe to the latest channel value stream for a widget, will trigger updates even if the value hasn't changed.
    ///
    /// Useful for app-level logic that reacts to live data (e.g. enable/disable controls based on sensor readings).
    ///
    /// Note: uses a send_replace watch receiver, so every successful poll can trigger a change/update to the subscriber,
    /// even if the value hasn't changed. This is useful for widgets that want to know when a poll has completed,
    /// even if the value hasn't changed.
    pub fn subscribe_widget_value_updates(&self, widget_id: &str) -> watch::Receiver<ChannelValue> {
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

    /// Subscribe to the latest known connection state for a widget, will trigger updates even if the state hasn't changed.
    ///
    /// Note: uses a send_replace watch receiver, so every successful poll can trigger a change/update to the subscriber,
    /// even if the connection state hasn't changed. This is useful for widgets that want to know
    /// when a poll has completed, even if the connection state hasn't changed.
    ///
    /// Defaults to `true` when no monitor has published state yet to preserve
    /// existing startup behavior for unmanaged widgets.
    ///
    /// TODO: considering changing default to `false` once all widgets are managed by a monitor.
    pub fn subscribe_widget_connection_updates(&self, widget_id: &str) -> watch::Receiver<bool> {
        self.widget_connected
            .entry(widget_id.to_string())
            .or_insert_with(|| watch::channel(true).0)
            .subscribe()
    }

    #[cfg(all(feature = "epics-pvxs", feature = "modbus"))]
    pub fn new(
        epics_ctx: Arc<std::sync::Mutex<pvxs::Context>>,
        modbus_pool: Arc<crate::modbus_client::ModbusPool>,
    ) -> Arc<Self> {
        Arc::new(Self {
            local_store: crate::local_channel::LocalStore::new(),
            epics_ctx,
            modbus_pool,
            #[cfg(feature = "modbus-server")]
            modbus_bridge: crate::modbus_bridge::ModbusBridgeContext::new(),
            #[cfg(feature = "ascii-tcp")]
            ascii_tcp_pool: Arc::new(ascii_tcp::ConnectionPool::new()),
            widget_enabled: DashMap::new(),
            widget_value_bus: DashMap::new(),
            widget_connected: DashMap::new(),
        })
    }

    #[cfg(all(feature = "epics-pvxs", not(feature = "modbus")))]
    pub fn new(epics_ctx: Arc<std::sync::Mutex<pvxs::Context>>) -> Arc<Self> {
        Arc::new(Self {
            local_store: crate::local_channel::LocalStore::new(),
            epics_ctx,
            #[cfg(feature = "ascii-tcp")]
            ascii_tcp_pool: Arc::new(ascii_tcp::ConnectionPool::new()),
            widget_enabled: DashMap::new(),
            widget_value_bus: DashMap::new(),
            widget_connected: DashMap::new(),
        })
    }

    #[cfg(all(not(feature = "epics-pvxs"), feature = "modbus"))]
    pub fn new(modbus_pool: Arc<crate::modbus_client::ModbusPool>) -> Arc<Self> {
        Arc::new(Self {
            local_store: crate::local_channel::LocalStore::new(),
            modbus_pool,
            #[cfg(feature = "modbus-server")]
            modbus_bridge: crate::modbus_bridge::ModbusBridgeContext::new(),
            #[cfg(feature = "ascii-tcp")]
            ascii_tcp_pool: Arc::new(ascii_tcp::ConnectionPool::new()),
            widget_enabled: DashMap::new(),
            widget_value_bus: DashMap::new(),
            widget_connected: DashMap::new(),
        })
    }

    #[cfg(all(not(feature = "epics-pvxs"), not(feature = "modbus")))]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            local_store: crate::local_channel::LocalStore::new(),
            #[cfg(feature = "ascii-tcp")]
            ascii_tcp_pool: Arc::new(ascii_tcp::ConnectionPool::new()),
            widget_enabled: DashMap::new(),
            widget_value_bus: DashMap::new(),
            widget_connected: DashMap::new(),
        })
    }
}
