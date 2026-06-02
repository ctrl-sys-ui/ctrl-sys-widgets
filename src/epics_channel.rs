use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pvxs_sys::{Context, MonitorEvent, Value};
use tokio::sync::mpsc::UnboundedSender;
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::channel::{ChannelEvent, ChannelValue, PrimaryMeta};
use crate::config::{ProtocolConfig, WidgetConfig};

// ─── Public entry point ───────────────────────────────────────────────────────

/// Create an async stream of `ChannelEvent`s backed by a pvxs-sys EPICS monitor.
///
/// * For regular (single-PV) widgets: spawns one blocking thread that loops on
///   `monitor.pop()` and sends events through a channel.
/// * For multi-series line charts: spawns one blocking thread **per PV**; all
///   threads share a `Mutex<HashMap>` state and re-render on every update.
pub fn epics_stream(
    config: Arc<WidgetConfig>,
    epics_ctx: Arc<Mutex<Context>>,
) -> impl tokio_stream::Stream<Item = ChannelEvent> + Send + 'static {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<ChannelEvent>();

    let is_multi_series = config
        .epics_pva()
        .map(|e| {
            config.chart_type.as_deref().unwrap_or("line") == "line"
                && e.pv_names.as_ref().map_or(false, |v| !v.is_empty())
        })
        .unwrap_or(false);

    if is_multi_series {
        let all_pvs = config
            .epics_pva()
            .map(|e| e.series_pvs())
            .unwrap_or_default();
        tokio::task::spawn_blocking(move || run_multi_monitor(all_pvs, config, epics_ctx, tx));
    } else {
        tokio::task::spawn_blocking(move || run_single_monitor(config, epics_ctx, tx));
    }

    UnboundedReceiverStream::new(rx)
}

// ─── Single-PV monitor ────────────────────────────────────────────────────────

fn run_single_monitor(config: Arc<WidgetConfig>, epics_ctx: Arc<Mutex<Context>>, tx: UnboundedSender<ChannelEvent>) {
    let pv_name = match config.protocol.as_ref() {
        Some(ProtocolConfig::EpicsPva(e)) => e.pv_name.clone(),
        _ => {
            let _ = tx.send(ChannelEvent::Error("epics_stream: not an epics-pva widget".into()));
            return;
        }
    };

    tracing::info!("EPICS monitor starting for: {}", pv_name);

    let mut monitor = {
        let mut ctx = epics_ctx.lock().unwrap();
        match ctx
            .monitor_builder(&pv_name)
            .and_then(|b| b.connect_exception(true).disconnect_exception(true).exec())
        {
            Ok(m) => m,
            Err(e) => {
                let _ = tx.send(ChannelEvent::Error(e.to_string()));
                return;
            }
        }
    };

    if let Err(e) = monitor.start() {
        let _ = tx.send(ChannelEvent::Error(e.to_string()));
        return;
    }

    loop {
        match monitor.pop() {
            Ok(Some(raw)) => {
                let cv = channel_value_from_epics(&raw, &config);
                if tx.send(ChannelEvent::Value(cv)).is_err() {
                    break;
                }
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(MonitorEvent::Connected(msg)) => {
                tracing::info!("{}: connected — {}", pv_name, msg);
                let _ = tx.send(ChannelEvent::Connected);
            }
            Err(MonitorEvent::Disconnected(msg)) => {
                tracing::warn!("{}: disconnected — {}", pv_name, msg);
                if tx.send(ChannelEvent::Disconnected(msg)).is_err() {
                    break;
                }
            }
            Err(MonitorEvent::Finished(msg)) => {
                tracing::info!("{}: finished — {}", pv_name, msg);
                break;
            }
            Err(MonitorEvent::RemoteError(msg) | MonitorEvent::ClientError(msg)) => {
                tracing::error!("{}: error — {}", pv_name, msg);
                if tx.send(ChannelEvent::Error(msg)).is_err() {
                    break;
                }
            }
        }
    }

    tracing::info!("EPICS monitor stopped for: {}", pv_name);
}

// ─── Multi-series chart monitor ───────────────────────────────────────────────

fn run_multi_monitor(
    all_pvs: Vec<String>,
    _config: Arc<WidgetConfig>,
    epics_ctx: Arc<Mutex<Context>>,
    tx: UnboundedSender<ChannelEvent>,
) {
    type State = Arc<Mutex<(HashMap<String, Vec<f64>>, PrimaryMeta)>>;
    let state: State = Arc::new(Mutex::new((HashMap::new(), PrimaryMeta::default())));

    let handles: Vec<_> = all_pvs
        .iter()
        .cloned()
        .enumerate()
        .map(|(idx, pv_name)| {
            let all_pvs = all_pvs.clone();
            let state = state.clone();
            let tx = tx.clone();
            let is_primary = idx == 0;
            let epics_ctx = epics_ctx.clone();

            std::thread::spawn(move || {
                let mut monitor = {
                    let mut ctx = epics_ctx.lock().unwrap();
                    match ctx
                        .monitor_builder(&pv_name)
                        .and_then(|b| b.connect_exception(true).disconnect_exception(true).exec())
                    {
                        Ok(m) => m,
                        Err(e) => {
                            tracing::error!("Multi-monitor: monitor failed for {}: {}", pv_name, e);
                            return;
                        }
                    }
                };

                if let Err(e) = monitor.start() {
                    tracing::error!("Multi-monitor: start failed for {}: {}", pv_name, e);
                    return;
                }

                loop {
                    match monitor.pop() {
                        Ok(Some(raw)) => {
                            if let Ok(arr) = raw.get_field_double_array("value") {
                                let mut guard = state.lock().unwrap();
                                guard.0.insert(pv_name.clone(), arr);
                                if is_primary {
                                    guard.1 = PrimaryMeta {
                                        alarm_severity: raw
                                            .get_field_int32("alarm.severity")
                                            .unwrap_or(0),
                                        description: raw
                                            .get_field_string("display.description")
                                            .unwrap_or_default(),
                                        units: raw
                                            .get_field_string("display.units")
                                            .unwrap_or_default(),
                                        limit_lo: raw
                                            .get_field_double("display.limitLow")
                                            .unwrap_or(0.0),
                                        limit_hi: raw
                                            .get_field_double("display.limitHigh")
                                            .unwrap_or(0.0),
                                    };
                                }
                                let mut cv = ChannelValue::default();
                                cv.named_series = guard.0.clone();
                                cv.primary_meta = guard.1.clone();
                                cv.alarm_severity = guard.1.alarm_severity;
                                drop(guard);
                                // Attach the ordered PV list via a side-channel in the
                                // named_series map ordering — the chart renderer uses
                                // `all_pvs` order when iterating series.
                                // Keep this join to document that series order follows
                                // the original `all_pvs` list (not hash map key order).
                                let _ = all_pvs.join(","); // unused, order via `all_pvs` arg
                                if tx.send(ChannelEvent::Value(cv)).is_err() {
                                    break;
                                }
                            }
                        }
                        Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                        Err(MonitorEvent::Connected(msg)) => {
                            tracing::info!("Multi-monitor {}: connected — {}", pv_name, msg);
                        }
                        Err(MonitorEvent::Disconnected(msg)) => {
                            tracing::warn!("Multi-monitor {}: disconnected — {}", pv_name, msg);
                            if tx.send(ChannelEvent::Disconnected(msg)).is_err() {
                                break;
                            }
                        }
                        Err(MonitorEvent::Finished(_)) => break,
                        Err(MonitorEvent::RemoteError(msg) | MonitorEvent::ClientError(msg)) => {
                            tracing::error!("Multi-monitor {}: error — {}", pv_name, msg);
                            if tx.send(ChannelEvent::Error(msg)).is_err() {
                                break;
                            }
                        }
                    }
                }
                tracing::info!("Multi-monitor stopped for: {}", pv_name);
            })
        })
        .collect();

    for h in handles {
        let _ = h.join();
    }
}

// ─── Value mapping ────────────────────────────────────────────────────────────

/// Convert a `pvxs_sys::Value` into a protocol-neutral `ChannelValue`.
pub fn channel_value_from_epics(raw: &Value, config: &WidgetConfig) -> ChannelValue {
    // Widget-level metadata as fallback when EPICS has not yet delivered server metadata.
    let meta_display = config.metadata.as_ref().and_then(|m| m.display.as_ref());
    let meta_control = config.metadata.as_ref().and_then(|m| m.control.as_ref());
    let meta_alarm   = config.metadata.as_ref().and_then(|m| m.alarm.as_ref());

    let pv_alarm_severity = raw.get_field_int32("alarm.severity").unwrap_or(0);
    let alarm_status       = raw.get_field_int32("alarm.status").unwrap_or(0);
    let units = raw.get_field_string("display.units").ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| meta_display.map(|d| d.units.clone()).unwrap_or_default());
    let precision = raw.get_field_int32("display.precision")
        .unwrap_or_else(|_| meta_display.map(|d| d.precision).unwrap_or(2));
    let display_low  = raw.get_field_double("display.limitLow")
        .unwrap_or_else(|_| meta_display.map(|d| d.limit_low).unwrap_or(0.0));
    let display_high = raw.get_field_double("display.limitHigh")
        .unwrap_or_else(|_| meta_display.map(|d| d.limit_high).unwrap_or(100.0));
    let control_low  = raw.get_field_double("control.limitLow")
        .unwrap_or_else(|_| meta_control.map(|c| c.limit_low).unwrap_or(0.0));
    let control_high = raw.get_field_double("control.limitHigh")
        .unwrap_or_else(|_| meta_control.map(|c| c.limit_high).unwrap_or(100.0));
    let low_alarm_limit = raw.get_field_double("valueAlarm.lowAlarmLimit")
        .unwrap_or_else(|_| meta_alarm.map(|a| a.low_alarm_limit).unwrap_or(0.0));
    let low_warn_limit = raw.get_field_double("valueAlarm.lowWarningLimit")
        .unwrap_or_else(|_| meta_alarm.map(|a| a.low_warning_limit).unwrap_or(0.0));
    let high_warn_limit = raw.get_field_double("valueAlarm.highWarningLimit")
        .unwrap_or_else(|_| meta_alarm.map(|a| a.high_warning_limit).unwrap_or(100.0));
    let high_alarm_limit = raw.get_field_double("valueAlarm.highAlarmLimit")
        .unwrap_or_else(|_| meta_alarm.map(|a| a.high_alarm_limit).unwrap_or(100.0));

    let array_values = raw.get_field_double_array("value").unwrap_or_default();
    let enum_index = raw.get_field_enum("value.index").unwrap_or(0);
    let enum_choices = raw
        .get_field_string_array("value.choices")
        .unwrap_or_default();
    let raw_value = raw.get_field_double("value").unwrap_or(0.0);

    let value_str = if !array_values.is_empty() {
        String::new()
    } else {
        match config.data_type.as_deref() {
            Some("string") => raw.get_field_string("value").unwrap_or_default(),
            Some("int32") | Some("int") | Some("integer") | Some("bool") => {
                raw.get_field_int32("value")
                    .ok()
                    .map(|v| v.to_string())
                    .unwrap_or_default()
            }
            _ => format!("{:.prec$}", raw_value, prec = precision as usize),
        }
    };

    let description = raw
        .get_field_string("display.description")
        .unwrap_or_default();
    let description = if description.is_empty() {
        meta_display.map(|d| d.description.clone()).unwrap_or_default()
    } else {
        description
    };

    // Use server alarm severity when the server has alarm monitoring configured
    // (i.e. valueAlarm fields are present in the PV update).
    // If the server published no alarm limit fields at all, fall back to locally-
    // computed severity from config metadata — that covers EPICS PVs that only
    // stream a raw value without any IOC-side alarm configuration.
    let server_has_alarm = raw.get_field_double("valueAlarm.lowAlarmLimit").is_ok()
        || raw.get_field_double("valueAlarm.highAlarmLimit").is_ok();
    let alarm_severity = if server_has_alarm {
        pv_alarm_severity
    } else {
        meta_alarm.map(|a| a.compute_severity(raw_value)).unwrap_or(0)
    };

    ChannelValue {
        raw_value,
        value_str,
        array_values,
        named_series: HashMap::new(),
        alarm_severity,
        alarm_status,
        units: units.clone(),
        display_low,
        display_high,
        control_low,
        control_high,
        precision,
        low_alarm_limit,
        low_warn_limit,
        high_warn_limit,
        high_alarm_limit,
        enum_index,
        enum_choices,
        primary_meta: PrimaryMeta {
            alarm_severity,
            description,
            units,
            limit_lo: display_low,
            limit_hi: display_high,
        },
    }
}
