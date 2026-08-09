use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use tokio::task::JoinHandle;

use mycela::config::AsciiLineEnding;

struct SimulatorState {
    setpoint_bits: AtomicU64,
    enabled: AtomicBool,
}

pub fn start_ascii_tcp_simulator(port: u16) -> JoinHandle<()> {
    let state = Arc::new(SimulatorState {
        setpoint_bits: AtomicU64::new(50.0_f64.to_bits()),
        enabled: AtomicBool::new(false),
    });

    mycela::ascii_tcp_server::start_server_with_line_ending(
        ([0, 0, 0, 0], port).into(),
        AsciiLineEnding::CrLf,
        move |request, _peer| {
            let state = Arc::clone(&state);
            async move { handle_request(&state, request.trim()) }
        },
    )
}

fn handle_request(state: &SimulatorState, request: &str) -> Result<String, String> {
    let normalized = request.to_ascii_uppercase();

    match normalized.as_str() {
        "READ TEMPERATURE" => {
            let elapsed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64();
            Ok(format!("{:.2}", 25.0 + 7.0 * (elapsed / 5.0).sin()))
        }
        "READ SETPOINT" => Ok(format!(
            "{:.1}",
            f64::from_bits(state.setpoint_bits.load(Ordering::Relaxed))
        )),
        "READ ENABLED" => Ok(if state.enabled.load(Ordering::Relaxed) {
            "ON".to_string()
        } else {
            "OFF".to_string()
        }),
        "READ MESSAGE" => Ok("ASCII TCP server online".to_string()),
        _ if normalized.starts_with("SET SETPOINT ") => {
            let value = request["SET SETPOINT ".len()..]
                .trim()
                .parse::<f64>()
                .map_err(|_| format!("Invalid setpoint in '{request}'"))?;
            state
                .setpoint_bits
                .store(value.to_bits(), Ordering::Relaxed);
            Ok("OK".to_string())
        }
        _ if normalized.starts_with("SET ENABLED ") => {
            let value = request["SET ENABLED ".len()..].trim().to_ascii_lowercase();
            let enabled = match value.as_str() {
                "1" | "true" | "on" => true,
                "0" | "false" | "off" => false,
                _ => return Err(format!("Invalid enabled value '{value}'")),
            };
            state.enabled.store(enabled, Ordering::Relaxed);
            Ok("OK".to_string())
        }
        _ => Err(format!("Unknown ASCII TCP command '{request}'")),
    }
}
