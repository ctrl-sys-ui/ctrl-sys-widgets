use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use tokio::sync::{mpsc, oneshot};
use tokio_modbus::client::tcp;
use tokio_modbus::prelude::*;

use crate::channel::{ChannelEvent, ChannelValue};
use crate::config::{ModbusTCPConfig, ModbusRegisterType, ProtocolConfig, WidgetConfig};

enum DeviceRequest {
    Read {
        register: u16,
        register_type: ModbusRegisterType,
        word_count: u8,
        respond: oneshot::Sender<Result<Vec<u16>, String>>,
    },
    Write {
        register: u16,
        register_type: ModbusRegisterType,
        values: Vec<u16>,
        respond: oneshot::Sender<Result<(), String>>,
    },
}

/// A cloneable handle to a per-device connection-manager task.
///
/// Multiple widgets sharing the same `host:port:unit_id` key get the same
/// handle, so only one TCP connection is ever opened per device.
pub struct DeviceHandle {
    tx: mpsc::UnboundedSender<DeviceRequest>,
}

impl DeviceHandle {
    pub async fn read(
        &self,
        register: u16,
        register_type: ModbusRegisterType,
        word_count: u8,
    ) -> Result<Vec<u16>, String> {
        let (respond, rx) = oneshot::channel();
        self.tx
            .send(DeviceRequest::Read {
                register,
                register_type,
                word_count,
                respond,
            })
            .map_err(|_| "device task closed".to_string())?;
        rx.await.map_err(|_| "device task dropped respond channel".to_string())?
    }

    /// Returns `true` when the backing device task has exited and this handle
    /// can no longer send requests.  The caller should re-fetch a fresh handle
    /// from the pool (which will spawn a new device task).
    pub fn is_closed(&self) -> bool {
        self.tx.is_closed()
    }

    pub async fn write(
        &self,
        register: u16,
        register_type: ModbusRegisterType,
        values: Vec<u16>,
    ) -> Result<(), String> {
        let (respond, rx) = oneshot::channel();
        self.tx
            .send(DeviceRequest::Write {
                register,
                register_type,
                values,
                respond,
            })
            .map_err(|_| "device task closed".to_string())?;
        rx.await.map_err(|_| "device task dropped respond channel".to_string())?
    }
}

/// Shared Modbus connection pool keyed by `"host:port:unit_id"`.
/// Use `Arc<ModbusPool>` everywhere so all SSE handlers and the write path
/// share the same set of connections.
pub struct ModbusPool {
    devices: DashMap<String, Arc<DeviceHandle>>,
    task_handles: std::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>,
}

impl ModbusPool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            devices: DashMap::new(),
            task_handles: std::sync::Mutex::new(Vec::new()),
        })
    }

    /// Abort all device connection tasks and clear the pool.
    /// Existing `DeviceHandle` senders will receive errors on their next send,
    /// causing `run_modbus_poll` to report `ChannelEvent::Disconnected`.
    pub fn disconnect_all(&self) {
        let mut handles = self.task_handles.lock().unwrap();
        for h in handles.drain(..) {
            h.abort();
        }
        self.devices.clear();
    }

    /// Return the existing handle for this device or create a new connection-manager task.
    pub fn get_or_create(
        &self,
        host: &str,
        port: u16,
        unit_id: u8,
    ) -> Arc<DeviceHandle> {
        let key = format!("{}:{}:{}", host, port, unit_id);
        if let Some(h) = self.devices.get(&key) {
            return h.clone();
        }

        let (tx, rx) = mpsc::unbounded_channel::<DeviceRequest>();
        let handle = Arc::new(DeviceHandle { tx });
        self.devices.insert(key, handle.clone());

        let host = host.to_string();
        let join = tokio::spawn(run_device_task(host, port, unit_id, rx));
        self.task_handles.lock().unwrap().push(join);

        handle
    }
}

async fn run_device_task(
    host: String,
    port: u16,
    unit_id: u8,
    mut rx: mpsc::UnboundedReceiver<DeviceRequest>,
) {
    let addr: SocketAddr = format!("{}:{}", host, port)
        .parse()
        .unwrap_or_else(|_| "127.0.0.1:502".parse().unwrap());
    let unit = Slave(unit_id);

    loop {
        // Connect (or reconnect after an error)
        let mut ctx = loop {
            match tokio::time::timeout(
                Duration::from_secs(2),
                tcp::connect_slave(addr, unit),
            ).await {
                Ok(Ok(c)) => {
                    tracing::info!(
                        "Modbus connected to {}:{} unit {}",
                        host,
                        port,
                        unit_id
                    );
                    break c;
                }
                Ok(Err(e)) => {
                    tracing::warn!(
                        "Modbus connect failed for {}:{}: {} -- retrying in 2 s",
                        host,
                        port,
                        e
                    );
                    // Drain ALL pending requests during the retry window so every
                    // widget gets an immediate error response, not just the first.
                    let retry_at = tokio::time::Instant::now() + Duration::from_secs(2);
                    loop {
                        tokio::select! {
                            req = rx.recv() => match req {
                                Some(r) => send_error(r, format!("Connection failed: {}", e)),
                                None => return, // pool dropped
                            },
                            _ = tokio::time::sleep_until(retry_at) => break,
                        }
                    }
                }
                Err(_) => {
                    tracing::warn!(
                        "Modbus connect timed out for {}:{} -- retrying in 2 s",
                        host,
                        port,
                    );
                    // Drain ALL pending requests during the retry window.
                    let retry_at = tokio::time::Instant::now() + Duration::from_secs(2);
                    loop {
                        tokio::select! {
                            req = rx.recv() => match req {
                                Some(r) => send_error(r, "Connection timed out".to_string()),
                                None => return, // pool dropped
                            },
                            _ = tokio::time::sleep_until(retry_at) => break,
                        }
                    }
                }
            }
        };

        // Serve requests until the connection breaks
        loop {
            let req = match rx.recv().await {
                Some(r) => r,
                None => return, // pool dropped
            };

            let success = handle_request(&mut ctx, req).await;
            if !success {
                tracing::warn!(
                    "Modbus connection to {}:{} lost, reconnecting...",
                    host,
                    port
                );
                break; // reconnect outer loop
            }
        }
    }
}

/// Execute one device request.  Returns `true` on success, `false` if the
/// connection should be dropped and re-established.
async fn handle_request(ctx: &mut tokio_modbus::client::Context, req: DeviceRequest) -> bool {
    match req {
        DeviceRequest::Read {
            register,
            register_type,
            word_count,
            respond,
        } => {
            let result = tokio::time::timeout(
                Duration::from_secs(1),
                execute_read(ctx, register, &register_type, word_count),
            )
            .await
            .unwrap_or_else(|_| Err("read timed out".to_string()));
            let ok = result.is_ok();
            let _ = respond.send(result);
            ok
        }
        DeviceRequest::Write {
            register,
            register_type,
            values,
            respond,
        } => {
            let result = tokio::time::timeout(
                Duration::from_secs(1),
                execute_write(ctx, register, &register_type, &values),
            )
            .await
            .unwrap_or_else(|_| Err("write timed out".to_string()));
            let ok = result.is_ok();
            let _ = respond.send(result);
            ok
        }
    }
}

async fn execute_read(
    ctx: &mut tokio_modbus::client::Context,
    register: u16,
    register_type: &ModbusRegisterType,
    word_count: u8,
) -> Result<Vec<u16>, String> {
    let count = word_count as u16;
    match register_type {
        ModbusRegisterType::HoldingRegister => ctx
            .read_holding_registers(register, count)
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r.map_err(|e| e.to_string())),
        ModbusRegisterType::InputRegister => ctx
            .read_input_registers(register, count)
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r.map_err(|e| e.to_string())),
        ModbusRegisterType::Coil => ctx
            .read_coils(register, count)
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r.map_err(|e| e.to_string()))
            .map(|bits| {
                bits.into_iter()
                    .map(|b| if b { 1u16 } else { 0u16 })
                    .collect()
            }),
        ModbusRegisterType::DiscreteInput => ctx
            .read_discrete_inputs(register, count)
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r.map_err(|e| e.to_string()))
            .map(|bits| {
                bits.into_iter()
                    .map(|b| if b { 1u16 } else { 0u16 })
                    .collect()
            }),
    }
}

async fn execute_write(
    ctx: &mut tokio_modbus::client::Context,
    register: u16,
    register_type: &ModbusRegisterType,
    values: &[u16],
) -> Result<(), String> {
    match register_type {
        ModbusRegisterType::HoldingRegister => {
            if values.len() == 1 {
                ctx.write_single_register(register, values[0])
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|r| r.map_err(|e| e.to_string()))
            } else {
                ctx.write_multiple_registers(register, values)
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|r| r.map_err(|e| e.to_string()))
            }
        }
        ModbusRegisterType::Coil => ctx
            .write_single_coil(register, values.first().copied().unwrap_or(0) != 0)
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r.map_err(|e| e.to_string())),
        ModbusRegisterType::InputRegister | ModbusRegisterType::DiscreteInput => {
            Err("Cannot write to read-only register types (input_register / discrete_input)".into())
        }
    }
}

fn send_error(req: DeviceRequest, msg: String) {
    match req {
        DeviceRequest::Read { respond, .. } => {
            let _ = respond.send(Err(msg));
        }
        DeviceRequest::Write { respond, .. } => {
            let _ = respond.send(Err(msg));
        }
    }
}

/// Create an async stream of `ChannelEvent`s by polling a Modbus register
/// at the interval specified in `config.protocol.modbus`.
pub fn modbus_stream(
    config: Arc<WidgetConfig>,
    pool: Arc<ModbusPool>,
) -> impl tokio_stream::Stream<Item = ChannelEvent> + Send + 'static {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<ChannelEvent>();

    let m = match config.protocol.as_ref() {
        Some(ProtocolConfig::ModbusTcp(m)) => m.clone(),
        _ => {
            let _ = tx.send(ChannelEvent::Error(
                "modbus_stream: not a modbus widget".into(),
            ));
            return tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
        }
    };

    tokio::spawn(run_modbus_poll(m, config, pool, tx));

    tokio_stream::wrappers::UnboundedReceiverStream::new(rx)
}

async fn run_modbus_poll(
    m: ModbusTCPConfig,
    config: Arc<WidgetConfig>,
    pool: Arc<ModbusPool>,
    tx: tokio::sync::mpsc::UnboundedSender<ChannelEvent>,
) {
    tracing::info!(
        "Modbus monitor starting: widget_id='{}' label='{}' host='{}' port={} unit_id={} register={} register_type='{:?}' word_count={} poll_ms={}",
        config.id,
        config.label,
        m.host,
        m.port,
        m.unit_id,
        m.register,
        m.register_type,
        m.word_count,
        m.min_poll_interval_ms.max(50)
    );

    let mut handle = pool.get_or_create(&m.host, m.port, m.unit_id);
    let mut interval =
        tokio::time::interval(Duration::from_millis(m.min_poll_interval_ms.max(50)));
    let mut was_connected = false;
    let mut last_value_str: Option<String> = None;

    loop {
        interval.tick().await;

        // If the device task was killed (e.g. after a Stop/Start cycle) signal
        // disconnection to the widget first, then re-acquire a fresh handle so
        // the pool can spawn a new connection task and reconnect automatically.
        if handle.is_closed() {
            if was_connected {
                was_connected = false;
                last_value_str = None;
                if tx.send(ChannelEvent::Disconnected("connection closed".to_string())).is_err() {
                    break;
                }
            }
            handle = pool.get_or_create(&m.host, m.port, m.unit_id);
        }

        match handle
            .read(m.register, m.register_type.clone(), m.word_count)
            .await
        {
            Ok(words) => {
                if !was_connected {
                    was_connected = true;
                    let _ = tx.send(ChannelEvent::Connected);
                }
                let raw = if let Some(bit) = m.bit_index {
                    let bit = bit.min(15);
                    let word0 = words.first().copied().unwrap_or(0);
                    ((word0 >> bit) & 1) as f64
                } else {
                    decode_words(&words, m.word_count)
                };
                let physical = raw * m.scale + m.offset;
                let cv = build_channel_value(physical, &m, &config);

                // Only push an update when the value actually changed -- this
                // matches EPICS monitor semantics and prevents the SSE stream
                // from overwriting an in-progress text-entry on every tick.
                if last_value_str.as_deref() != Some(&cv.value_str) {
                    let bit_info = m
                        .bit_index
                        .map(|b| format!(", bit_index={}", b))
                        .unwrap_or_default();
                    tracing::info!(
                        "[{}] read_channel: ch=modbus-tcp://{}:{}/reg{}, register_type={:?}, word_count={}{} words={:?}, value='{}'",
                        config.id,
                        m.host,
                        m.port,
                        m.register,
                        m.register_type,
                        m.word_count,
                        bit_info,
                        words,
                        cv.value_str
                    );
                    last_value_str = Some(cv.value_str.clone());
                    if tx.send(ChannelEvent::Value(cv)).is_err() {
                        break;
                    }
                }
            }
            Err(e) => {
                if was_connected {
                    was_connected = false;
                    last_value_str = None;
                    tracing::warn!(
                        "Modbus poll error for {}:{}/reg{}: {}",
                        m.host,
                        m.port,
                        m.register,
                        e
                    );
                    if tx.send(ChannelEvent::Disconnected(e.clone())).is_err() {
                        break;
                    }
                }
            }
        }
    }
}

/// Decode one or two u16 register words into an f64.
/// * `word_count == 1` -> treat as unsigned 16-bit integer.
/// * `word_count == 2` -> treat as IEEE 754 single-precision float (big-endian
///   word order: high word first).
fn decode_words(words: &[u16], word_count: u8) -> f64 {
    match (word_count, words) {
        (2, [hi, lo, ..]) => {
            let bits = ((*hi as u32) << 16) | (*lo as u32);
            f32::from_bits(bits) as f64
        }
        (_, [w, ..]) => *w as f64,
        _ => 0.0,
    }
}

pub fn build_channel_value(physical: f64, m: &ModbusTCPConfig, config: &WidgetConfig) -> ChannelValue {
    let meta_display = config.metadata.as_ref().and_then(|md| md.display.as_ref());
    let meta_control = config.metadata.as_ref().and_then(|md| md.control.as_ref());
    let meta_alarm   = config.metadata.as_ref().and_then(|md| md.alarm.as_ref());

    let precision    = meta_display.map(|d| d.precision).unwrap_or(2);
    let units       = meta_display.map(|d| d.units.clone()).unwrap_or_default();
    let description = meta_display.map(|d| d.description.clone()).unwrap_or_default();

    let value_str = match config.data_type.as_deref() {
        Some("bool") | Some("int32") | Some("int") => (physical as i64).to_string(),
        _ => format!("{:.prec$}", physical, prec = precision as usize),
    };

    // Display / control range: prefer config metadata, then derive from register range
    let raw_range_high = 65535.0 * m.scale + m.offset;
    let display_low  = meta_display.map(|d| d.limit_low) .unwrap_or(m.offset);
    let display_high = meta_display.map(|d| d.limit_high).unwrap_or(raw_range_high);
    let control_low  = meta_control.map(|c| c.limit_low) .unwrap_or(display_low);
    let control_high = meta_control.map(|c| c.limit_high).unwrap_or(display_high);

    let alarm_severity = meta_alarm.map(|a| a.compute_severity(physical)).unwrap_or(0);

    ChannelValue {
        raw_value: physical,
        value_str,
        precision,
        display_low,
        display_high,
        control_low,
        control_high,
        low_alarm_limit:  meta_alarm.map(|a| a.low_alarm_limit)    .unwrap_or(0.0),
        low_warn_limit:   meta_alarm.map(|a| a.low_warning_limit)  .unwrap_or(0.0),
        high_warn_limit:  meta_alarm.map(|a| a.high_warning_limit) .unwrap_or(display_high),
        high_alarm_limit: meta_alarm.map(|a| a.high_alarm_limit)   .unwrap_or(display_high),
        alarm_severity,
        primary_meta: crate::channel::PrimaryMeta {
            alarm_severity,
            description,
            units: units.clone(),
            limit_lo: display_low,
            limit_hi: display_high,
        },
        units,
        ..ChannelValue::default()
    }
}

/// Write a physical value back to a Modbus register, reversing the scale/offset.
/// Write a physical value back to a Modbus register, reversing the scale/offset.
pub async fn modbus_write(
    m: &ModbusTCPConfig,
    physical_value: f64,
    pool: &ModbusPool,
) -> Result<(), String> {
    let handle = pool.get_or_create(&m.host, m.port, m.unit_id);
    let raw = (physical_value - m.offset) / m.scale;

    let words: Vec<u16> = if m.word_count == 2 {
        let bits = (raw as f32).to_bits();
        vec![(bits >> 16) as u16, (bits & 0xFFFF) as u16]
    } else {
        // Round before casting -- prevents floating-point edge cases where
        // e.g. (99.1 / 0.1) evaluates to 990.9999... and floors to 990.
        vec![raw.round().clamp(0.0, 65535.0) as u16]
    };

    handle.write(m.register, m.register_type.clone(), words).await
}


