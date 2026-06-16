use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use tokio::sync::mpsc as tokio_mpsc;
use tokio::task::JoinHandle;
use tokio_modbus::prelude::ExceptionCode;

use crate::app::AppState;
use crate::config::{
    ModbusBridgeAccessMode, ModbusBridgeRegisterMap, ModbusRegisterType, ProtocolConfig,
    WidgetServerProtocolConfig,
};
use crate::modbus_server::{new_register_bank, start_modbus_server, RegisterBank};
use crate::protocol_control::ProtocolControlError;

pub struct ModbusBridgeContext {
    pub register_bank: RegisterBank,
    pub mappings: DashMap<u16, ModbusBridgeRegisterMap>,
}

struct WriteThroughRequest {
    mapping: ModbusBridgeRegisterMap,
    value: u16,
    respond: std::sync::mpsc::Sender<Result<(), ExceptionCode>>,
}

impl ModbusBridgeContext {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            register_bank: new_register_bank(),
            mappings: DashMap::new(),
        })
    }

    pub fn configure(&self, mappings: &[ModbusBridgeRegisterMap]) {
        self.register_bank.clear();
        self.mappings.clear();

        for mapping in mappings {
            self.mappings
                .insert(mapping.exposed_register, mapping.clone());
        }
    }
}

fn collect_widget_proxy_mappings(state: &AppState) -> Vec<ModbusBridgeRegisterMap> {
    let mut mappings = Vec::new();
    for screen in &state.config.screens {
        for widget in crate::widgets::collect_data_widgets(&screen.widgets) {
            let Some(server) = widget.server.clone() else {
                continue;
            };
            match widget.protocol.clone() {
                Some(ProtocolConfig::ModbusTcp(modbus)) => {
                    mappings.push(ModbusBridgeRegisterMap {
                        exposed_register: server.proxy_register,
                        register_type: modbus.register_type.clone(),
                        word_count: modbus.word_count.max(1),
                        source_widget_id: Some(widget.id.clone()),
                        source_upstream_register: Some(modbus.register),
                        source_upstream_register_type: Some(modbus.register_type.clone()),
                        target_upstream_register: server
                            .target_upstream_register
                            .or(Some(modbus.register)),
                        access: server.access,
                    });
                }
                Some(ProtocolConfig::Local(_)) => {
                    let Some(WidgetServerProtocolConfig::ModbusTcp(server_modbus)) =
                        server.protocol.clone()
                    else {
                        continue;
                    };

                    mappings.push(ModbusBridgeRegisterMap {
                        exposed_register: server.proxy_register,
                        register_type: server_modbus.register_type.clone(),
                        word_count: server_modbus.word_count.max(1),
                        source_widget_id: Some(widget.id.clone()),
                        source_upstream_register: None,
                        source_upstream_register_type: None,
                        target_upstream_register: server.target_upstream_register,
                        access: server.access,
                    });
                }
                _ => {}
            }
        }
    }
    mappings
}

fn encode_widget_value_to_words(raw: f64, register_type: &ModbusRegisterType, word_count: u8) -> Vec<u16> {
    let wc = word_count.max(1);
    if !raw.is_finite() {
        return vec![0; wc as usize];
    }

    if wc == 1 || matches!(register_type, ModbusRegisterType::Coil | ModbusRegisterType::DiscreteInput) {
        return vec![raw.round().clamp(0.0, u16::MAX as f64) as u16];
    }

    // For two-word scalar values, expose big-endian IEEE754 f32 words.
    if wc == 2 {
        let bits = (raw as f32).to_bits();
        return vec![(bits >> 16) as u16, (bits & 0xFFFF) as u16];
    }

    vec![raw.round().clamp(0.0, u16::MAX as f64) as u16]
}

pub fn start_bridge_runtime(state: &AppState) -> Result<Vec<JoinHandle<()>>, ProtocolControlError> {
    let bridge = state.config.startup.modbus_bridge.clone();
    if !bridge.enabled {
        return Err(ProtocolControlError::Operation(
            "Modbus bridge is disabled in startup.modbus_bridge.enabled".to_string(),
        ));
    }

    let mut mappings = bridge.registers.clone();
    mappings.extend(collect_widget_proxy_mappings(state));
    state.channel_ctx.modbus_bridge.configure(&mappings);

    let listen = format!("{}:{}", bridge.listen_addr, bridge.listen_port);
    let addr: SocketAddr = listen
        .parse()
        .map_err(|e| ProtocolControlError::Operation(format!("Invalid modbus bridge listen endpoint '{}': {}", listen, e)))?;

    let mut handles = Vec::new();

    let mut writable = HashMap::<u16, ModbusBridgeRegisterMap>::new();
    for m in &mappings {
        if m.access == ModbusBridgeAccessMode::ReadWrite {
            writable.insert(m.exposed_register, m.clone());
        }
    }

    let (write_tx, mut write_rx) = tokio_mpsc::unbounded_channel::<WriteThroughRequest>();
    let write_ctx = state.channel_ctx.clone();
    let write_upstream = bridge.upstream.clone();
    handles.push(tokio::spawn(async move {
        while let Some(req) = write_rx.recv().await {
            let Some(upstream) = write_upstream.clone() else {
                tracing::warn!(
                    "[bridge] write-through request rejected for exposed register {} because no upstream bridge target is configured",
                    req.mapping.exposed_register
                );
                let _ = req.respond.send(Err(ExceptionCode::IllegalFunction));
                continue;
            };

            let target_register = req
                .mapping
                .target_upstream_register
                .or(req.mapping.source_upstream_register)
                .unwrap_or(req.mapping.exposed_register);

            let mut value = req.value;
            if req.mapping.register_type == ModbusRegisterType::Coil {
                value = if value == 0 { 0 } else { 1 };
            }

            let handle = write_ctx
                .modbus_pool
                .get_or_create(&upstream.host, upstream.port, upstream.unit_id);

            let result = handle
                .write(
                    target_register,
                    req.mapping.register_type.clone(),
                    vec![value],
                )
                .await;
            match result {
                Ok(()) => {
                    tracing::info!(
                        "[bridge] write-through forwarded exposed register {} -> upstream {}:{} unit {} register {} value {}",
                        req.mapping.exposed_register,
                        upstream.host,
                        upstream.port,
                        upstream.unit_id,
                        target_register,
                        value
                    );
                    let _ = req.respond.send(Ok(()));
                }
                Err(e) => {
                    tracing::warn!(
                        "[bridge] write-through failed for exposed register {} -> upstream {}:{} unit {} register {}: {}",
                        req.mapping.exposed_register,
                        upstream.host,
                        upstream.port,
                        upstream.unit_id,
                        target_register,
                        e
                    );
                    let _ = req.respond.send(Err(ExceptionCode::IllegalFunction));
                }
            }
        }
    }));

    let server = start_modbus_server(
        addr,
        state.channel_ctx.modbus_bridge.register_bank.clone(),
        move |register, value| {
            match writable.get(&register) {
                Some(m) => {
                    let (resp_tx, resp_rx) = std::sync::mpsc::channel::<Result<(), ExceptionCode>>();
                    if write_tx
                        .send(WriteThroughRequest {
                            mapping: m.clone(),
                            value,
                            respond: resp_tx,
                        })
                        .is_err()
                    {
                        return Err(ExceptionCode::IllegalFunction);
                    }

                    match resp_rx.recv_timeout(Duration::from_millis(1500)) {
                        Ok(result) => result,
                        Err(_) => {
                            tracing::warn!(
                                "[bridge] write-through timed out for exposed register {} value {}",
                                register,
                                value
                            );
                            Err(ExceptionCode::IllegalFunction)
                        }
                    }
                }
                None => {
                    tracing::warn!(
                        "[bridge] rejected external write to read-only or unmapped exposed register {} value {}",
                        register,
                        value
                    );
                    Err(ExceptionCode::IllegalFunction)
                }
            }
        },
    );
    handles.push(server);

    for mapping in mappings {
        if let Some(widget_id) = mapping.source_widget_id.clone() {
            let ctx = state.channel_ctx.clone();
            let exposed = mapping.exposed_register;
            handles.push(tokio::spawn(async move {
                let mut rx = ctx.subscribe_widget_value(&widget_id);
                loop {
                    if rx.changed().await.is_err() {
                        break;
                    }
                    let raw = rx.borrow().raw_value;
                    let encoded =
                        encode_widget_value_to_words(raw, &mapping.register_type, mapping.word_count);
                    for (offset, word) in encoded.into_iter().enumerate() {
                        let addr = exposed.saturating_add(offset as u16);
                        ctx.modbus_bridge.register_bank.insert(addr, word);
                    }
                }
            }));
        } else if let Some(source_register) = mapping.source_upstream_register {
            let Some(upstream) = bridge.upstream.clone() else {
                continue;
            };
            let register_type = mapping
                .source_upstream_register_type
                .clone()
                .unwrap_or(ModbusRegisterType::HoldingRegister);
            let ctx = state.channel_ctx.clone();
            let exposed = mapping.exposed_register;
            handles.push(tokio::spawn(async move {
                loop {
                    let device =
                        ctx.modbus_pool
                            .get_or_create(&upstream.host, upstream.port, upstream.unit_id);
                    let read = device
                        .read(source_register, register_type.clone(), mapping.word_count.max(1))
                        .await;
                    match read {
                        Ok(words) => {
                            for (offset, word) in words.iter().copied().enumerate() {
                                let addr = exposed.saturating_add(offset as u16);
                                ctx.modbus_bridge.register_bank.insert(addr, word);
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                "[bridge] upstream poll failed for {}:{} unit {} register {}: {}",
                                upstream.host,
                                upstream.port,
                                upstream.unit_id,
                                source_register,
                                e
                            );
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                }
            }));
        }
    }

    tracing::info!(
        "[bridge] runtime started on {}:{} with {} mapping entries",
        bridge.listen_addr,
        bridge.listen_port,
        state.channel_ctx.modbus_bridge.mappings.len()
    );

    Ok(handles)
}
