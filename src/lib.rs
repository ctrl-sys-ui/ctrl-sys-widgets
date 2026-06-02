// Library exports for mycela
pub mod app;
pub mod channel;
pub mod config;
#[cfg(feature = "desktop")]
pub mod desktop_transport;
pub mod ipc;
pub mod ipc_dispatch;
pub mod logging;
#[cfg(feature = "epics")]
pub mod epics_channel;
#[cfg(feature = "modbus")]
pub mod modbus_client;
#[cfg(feature = "modbus-server")]
pub mod modbus_server;
pub mod protocol_control;
#[cfg(feature = "epics")]
pub mod server_setup;
pub mod widgets;

// Re-export framework crates so downstream apps only need `mycela` as a
// dependency and can import everything via `mycela::<crate>::...`.
pub use axum;
pub use maud;
pub use tower_http;
pub use tokio_stream;
pub use async_stream;
#[cfg(feature = "epics")]
pub use pvxs_sys;
#[cfg(feature = "desktop")]
pub use winit;
#[cfg(feature = "desktop")]
pub use wry;
