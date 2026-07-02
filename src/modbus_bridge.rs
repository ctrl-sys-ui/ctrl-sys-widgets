use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::mpsc as tokio_mpsc;
use tokio::task::JoinHandle;
use tokio_modbus::prelude::ExceptionCode;

use crate::app::AppState;
use crate::config::{
    ModbusBridgeAccessMode, ModbusBridgeRegisterMap, ModbusRegisterType, ProtocolConfig,
    WidgetServerProtocolConfig,
};
use crate::modbus_server::{
    new_register_bank, start_modbus_server, RegisterBank, WriteAccessKind,
};
use crate::protocol_control::ProtocolControlError;

fn write_kind_matches_type(kind: WriteAccessKind, register_type: &ModbusRegisterType) -> bool {
    match register_type {
        ModbusRegisterType::Coil => {
            matches!(kind, WriteAccessKind::SingleCoil | WriteAccessKind::MultiCoil)
        }
        ModbusRegisterType::HoldingRegister => {
            matches!(kind, WriteAccessKind::SingleRegister | WriteAccessKind::MultiRegister)
        }
        // Discrete/Input registers are read-only by Modbus definition.
        ModbusRegisterType::DiscreteInput | ModbusRegisterType::InputRegister => false,
    }
}

pub struct ModbusBridgeContext {
    pub register_bank: RegisterBank,
    pub mappings: DashMap<u16, ModbusBridgeRegisterMap>,
}

struct WriteThroughRequest {
    register: u16,
    mapping: ModbusBridgeRegisterMap,
    value: u16,
}

#[derive(Clone)]
struct LocalProxyWriteTarget {
    widget: crate::config::WidgetConfig,
    mapping: ModbusBridgeRegisterMap,
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
                    tracing::info!(
                        "[bridge] widget {} proxy mapping: exposed register {} -> upstream register {} (type {:?}, word count {})",
                        widget.id,
                        server.proxy_register,
                        modbus.register,
                        modbus.register_type,
                        modbus.word_count.max(1)
                    );
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
                    tracing::info!(
                        "[bridge] widget {} proxy mapping: exposed register {} -> local widget (type {:?}, word count {})",
                        widget.id,
                        server.proxy_register,
                        server_modbus.register_type,
                        server_modbus.word_count.max(1)
                    );
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

fn decode_words_to_widget_value(words: &[u16], register_type: &ModbusRegisterType, word_count: u8) -> f64 {
    let wc = word_count.max(1);
    if wc == 0 || words.is_empty() {
        return 0.0;
    }

    if wc == 1 || matches!(register_type, ModbusRegisterType::Coil | ModbusRegisterType::DiscreteInput) {
        return words[0] as f64;
    }

    if wc == 2 && words.len() >= 2 {
        // For two-word scalar values, decode big-endian IEEE754 f32 words.
        let bits = ((words[0] as u32) << 16) | (words[1] as u32);
        return f32::from_bits(bits) as f64;
    }

    words[0] as f64
}

fn to_local_value_string(widget: &crate::config::WidgetConfig, raw: f64) -> String {
    match widget.data_type.as_deref() {
        Some("bool") => {
            if raw == 0.0 { "0".to_string() } else { "1".to_string() }
        }
        Some("int") | Some("int32") | Some("integer") => (raw as i64).to_string(),
        Some("enum") => (raw as i16).to_string(),
        Some("string") => raw.to_string(),
        _ => raw.to_string(),
    }
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
    let mut local_proxy_targets = HashMap::<u16, LocalProxyWriteTarget>::new();
    for m in &mappings {
        if m.access == ModbusBridgeAccessMode::ReadWrite {
            let width = m.word_count.max(1);
            for offset in 0..width {
                writable.insert(m.exposed_register.saturating_add(offset as u16), m.clone());
            }

            if let Some(widget_id) = m.source_widget_id.as_deref() {
                let maybe_widget = state
                    .config
                    .screens
                    .iter()
                    .flat_map(|screen| crate::widgets::collect_data_widgets(&screen.widgets))
                    .find(|widget| widget.id == widget_id && matches!(widget.protocol, Some(ProtocolConfig::Local(_))));

                if let Some(widget) = maybe_widget {
                    for offset in 0..width {
                        local_proxy_targets.insert(
                            m.exposed_register.saturating_add(offset as u16),
                            LocalProxyWriteTarget {
                                widget: widget.clone(),
                                mapping: m.clone(),
                            },
                        );
                    }
                }
            }
        }
    }

    let (write_tx, mut write_rx) = tokio_mpsc::unbounded_channel::<WriteThroughRequest>();
    let write_ctx = state.channel_ctx.clone();
    let write_upstream = bridge.upstream.clone();
    let local_proxy_targets = Arc::new(local_proxy_targets);
    let local_proxy_targets_for_worker = local_proxy_targets.clone();

    handles.push(tokio::spawn(async move {
        while let Some(req) = write_rx.recv().await {
            if let Some(target) = local_proxy_targets_for_worker.get(&req.register) {
                let base = target.mapping.exposed_register;
                let width = target.mapping.word_count.max(1);

                let mut words = Vec::with_capacity(width as usize);
                for offset in 0..width {
                    let addr = base.saturating_add(offset as u16);
                    if addr == req.register {
                        words.push(req.value);
                    } else {
                        words.push(
                            write_ctx
                                .modbus_bridge
                                .register_bank
                                .get(&addr)
                                .map(|v| *v)
                                .unwrap_or(0),
                        );
                    }
                }

                let raw = decode_words_to_widget_value(
                    &words,
                    &target.mapping.register_type,
                    target.mapping.word_count,
                );
                let value_str = to_local_value_string(&target.widget, raw);

                if let Err(e) = crate::local_channel::local_write(
                    &target.widget,
                    &value_str,
                    &write_ctx.local_store,
                ) {
                    tracing::warn!(
                        "[bridge] failed to push local widget '{}' from proxy write register {} value {}: {}",
                        target.widget.id,
                        req.register,
                        req.value,
                        e
                    );
                }
            }

            let Some(upstream) = write_upstream.clone() else {
                tracing::debug!(
                    "[bridge] write-through request rejected for exposed register {} because no upstream bridge target is configured",
                    req.mapping.exposed_register
                );
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
                    tracing::debug!(
                        "[bridge] write-through forwarded exposed register {} -> upstream {}:{} unit {} register {} value {}",
                        req.mapping.exposed_register,
                        upstream.host,
                        upstream.port,
                        upstream.unit_id,
                        target_register,
                        value
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "[bridge] write-through failed for exposed register {} -> upstream {}:{} unit {} register {} value {}: error - {}",
                        req.mapping.exposed_register,
                        upstream.host,
                        upstream.port,
                        upstream.unit_id,
                        target_register,
                        value,
                        e
                    );
                }
            }
        }
    }));

    let server = start_modbus_server(
        addr,
        state.channel_ctx.modbus_bridge.register_bank.clone(),
        move |register, value, access_kind| {
            match writable.get(&register) {
                Some(m) => {
                    if !write_kind_matches_type(access_kind, &m.register_type) {
                        tracing::warn!(
                            "[bridge] incompatible write rejected for exposed register {} type {:?} using {:?} value {}",
                            register,
                            m.register_type,
                            access_kind,
                            value
                        );
                        return Err(ExceptionCode::IllegalFunction);
                    }

                    let mut normalized = value;
                    if m.register_type == ModbusRegisterType::Coil {
                        normalized = if value == 0 { 0 } else { 1 };
                    }

                    tracing::debug!(
                        "[bridge] write-through request for exposed register {} value {}",
                        register,
                        normalized
                    );

                    if write_tx
                        .send(WriteThroughRequest {
                            register,
                            mapping: m.clone(),
                            value: normalized,
                        })
                        .is_err()
                    {
                        tracing::warn!(
                            "[bridge] write-forward worker unavailable for exposed register {} value {}; accepting local write only",
                            register,
                            normalized
                        );
                    }

                    // Accept immediately so local register-bank state is committed by the server,
                    // even if upstream forwarding is slow or disconnected.
                    Ok(())
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
        // Incoming events from widgets or upstream registers are used to update the exposed register in the bridge's register bank.
        // Then expose these events upstream to the bridge's Modbus server.
        if let Some(widget_id) = mapping.source_widget_id.clone() {
            let ctx = state.channel_ctx.clone();
            let exposed = mapping.exposed_register;
            // Subscribe to widget value changes and update the exposed register in the bridge's register bank.
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
        }
        // Incoming event from upstream registers are used to update the exposed register in the bridge's register bank.
        // Then expose these events downstream to widgets or other clients connected to the bridge's Modbus server.
        else if let Some(source_register) = mapping.source_upstream_register {
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
                // pstream polling loop now uses exponential back-off — on each failed read the 
                // retry interval doubles (250ms → 500ms → 1s → 2s → 5s max), and resets to 250ms on the next successful read. 
                let mut poll_interval_ms = 250u64;
                loop {
                    let device =
                        ctx.modbus_pool
                            .get_or_create(&upstream.host, upstream.port, upstream.unit_id);
                    let read = device
                        .read(source_register, register_type.clone(), mapping.word_count.max(1))
                        .await;
                    match read {
                        Ok(words) => {
                            poll_interval_ms = 250;
                            for (offset, word) in words.iter().copied().enumerate() {
                                let addr = exposed.saturating_add(offset as u16);
                                ctx.modbus_bridge.register_bank.insert(addr, word);
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                "[bridge] upstream poll failed for {}:{} unit {} register {} (retrying in {}ms): {}",
                                upstream.host,
                                upstream.port,
                                upstream.unit_id,
                                source_register,
                                poll_interval_ms,
                                e
                            );
                            poll_interval_ms = (poll_interval_ms * 2).min(5000);
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(poll_interval_ms)).await;
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
