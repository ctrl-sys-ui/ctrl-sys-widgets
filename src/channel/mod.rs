pub mod context;
pub mod value;

pub use self::context::ChannelContext;
pub use self::value::{ChannelValue, PrimaryMeta};

use std::sync::Arc;

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

    #[cfg(feature = "epics-pvxs")]
    if matches!(
        config.protocol.as_ref(),
        Some(ProtocolConfig::EpicsPvxs(_)) | None
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

    #[cfg(feature = "ascii-tcp")]
    if let Some(ProtocolConfig::AsciiTcp(_)) = config.protocol.as_ref() {
        return Box::pin(crate::ascii_tcp_client::stream(config));
    }

    #[cfg(feature = "ascii-serial")]
    if let Some(ProtocolConfig::AsciiSerial(_)) = config.protocol.as_ref() {
        return Box::pin(crate::ascii_serial_client::ascii_serial_stream(config));
    }

    // No protocol configured or no matching feature enabled.
    let _ = ctx;
    Box::pin(futures::stream::empty::<ChannelEvent>())
}
