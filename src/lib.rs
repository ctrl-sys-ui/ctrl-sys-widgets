// Library exports for mycela
pub mod app;
pub mod channel;
pub mod config;
#[cfg(feature = "desktop")]
pub mod desktop_transport;
#[cfg(feature = "desktop")]
pub mod desktop;
pub mod ipc;
pub mod ipc_dispatch;
pub mod logging;
// Declare all protocol backends here so `channel_stream` can route to them.
// All protocols other than "local" should be behind feature flags so they can be optional dependencies.
pub mod local_channel;
#[cfg(feature = "epics-pvxs")]
pub mod epics_channel;
#[cfg(feature = "modbus")]
pub mod modbus_client;
#[cfg(feature = "ascii-tcp")]
pub mod ascii_tcp_client;
#[cfg(feature = "ascii-tcp")]
pub mod ascii_tcp_server;
#[cfg(feature = "ascii-serial")]
pub mod ascii_serial_client;
#[cfg(feature = "modbus-server")]
pub mod modbus_server;
#[cfg(feature = "modbus-server")]
pub mod modbus_bridge;
pub mod protocol_control;
#[cfg(feature = "epics-pvxs")]
pub mod server_setup;
pub mod widgets;

// Re-export framework crates so downstream apps only need `mycela` as a
// dependency and can import everything via `mycela::<crate>::...`.
pub use axum;
pub use maud;
pub use tower_http;
pub use tokio_stream;
pub use async_stream;
#[cfg(feature = "epics-pvxs")]
pub use pvxs;
#[cfg(feature = "desktop")]
pub use winit;
#[cfg(feature = "desktop")]
pub use wry;
#[cfg(feature = "desktop")]
pub use tao;
