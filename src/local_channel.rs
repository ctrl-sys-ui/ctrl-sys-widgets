use crate::channel::{ChannelEvent, ChannelValue, PrimaryMeta};
use crate::config::{ProtocolConfig, WidgetConfig, WidgetType};
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::watch;

/// In-memory bus for `local` protocol channels.
///
/// Each local channel keeps its latest `ChannelValue` in a watch channel so any
/// number of SSE/IPC subscribers can receive updates without network I/O.
#[derive(Default)]
pub struct LocalStore {
    channels: DashMap<String, watch::Sender<ChannelValue>>,
}

impl LocalStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            channels: DashMap::new(),
        })
    }

    pub fn subscribe(&self, channel: &str, initial: ChannelValue) -> watch::Receiver<ChannelValue> {
        let sender = self.get_or_create_sender(channel, initial);
        sender.subscribe()
    }

    pub fn publish(&self, channel: &str, value: ChannelValue) {
        let sender = self.get_or_create_sender(channel, value.clone());
        sender.send_replace(value);
    }

    fn get_or_create_sender(&self, channel: &str, initial: ChannelValue) -> watch::Sender<ChannelValue> {
        if let Some(existing) = self.channels.get(channel) {
            return existing.clone();
        }

        let (sender, _rx) = watch::channel(initial);
        let key = channel.to_string();
        self.channels
            .entry(key)
            .or_insert_with(|| sender.clone())
            .clone()
    }
}

pub fn local_stream(
    config: Arc<WidgetConfig>,
    store: Arc<LocalStore>,
) -> impl tokio_stream::Stream<Item = ChannelEvent> + Send + 'static {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<ChannelEvent>();

    let local = match config.protocol.as_ref() {
        Some(ProtocolConfig::Local(l)) => l.clone(),
        _ => {
            let _ = tx.send(ChannelEvent::Error("local_stream: not a local widget".into()));
            return tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
        }
    };

    tokio::spawn(async move {
        let seed = local
            .initial_value
            .clone()
            .unwrap_or_else(|| default_seed_value(&config));
        let initial = channel_value_from_local_str(&seed, &config);
        let mut watch_rx = store.subscribe(&local.channel, initial);

        if tx.send(ChannelEvent::Connected).is_err() {
            return;
        }

        let first = watch_rx.borrow().clone();
        let mut last_value = first.value_str.clone();
        if tx.send(ChannelEvent::Value(first)).is_err() {
            return;
        }

        loop {
            if watch_rx.changed().await.is_err() {
                break;
            }
            let cv = watch_rx.borrow().clone();
            if cv.value_str == last_value {
                continue;
            }
            last_value = cv.value_str.clone();
            if tx.send(ChannelEvent::Value(cv)).is_err() {
                break;
            }
        }
    });

    tokio_stream::wrappers::UnboundedReceiverStream::new(rx)
}

pub fn local_write(
    config: &WidgetConfig,
    value_str: &str,
    store: &LocalStore,
) -> Result<(), String> {
    let local = match config.protocol.as_ref() {
        Some(ProtocolConfig::Local(l)) => l,
        _ => return Err("Widget is not configured for local protocol".to_string()),
    };

    let cv = channel_value_from_local_str(value_str, config);
    store.publish(&local.channel, cv);
    Ok(())
}

fn default_seed_value(config: &WidgetConfig) -> String {
    match config.data_type.as_deref() {
        Some("bool") | Some("int") | Some("int32") | Some("integer") | Some("enum") => {
            "0".to_string()
        }
        Some("string") => String::new(),
        _ => "0".to_string(),
    }
}

fn parse_bool_like(v: &str) -> Option<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "on" => Some(true),
        "0" | "false" | "off" => Some(false),
        _ => None,
    }
}

fn channel_value_from_local_str(value_str: &str, config: &WidgetConfig) -> ChannelValue {
    let meta_display = config.metadata.as_ref().and_then(|md| md.display.as_ref());
    let meta_control = config.metadata.as_ref().and_then(|md| md.control.as_ref());
    let meta_alarm = config.metadata.as_ref().and_then(|md| md.alarm.as_ref());

    let precision = meta_display.map(|d| d.precision).unwrap_or(2);
    let units = meta_display.map(|d| d.units.clone()).unwrap_or_default();
    let description = meta_display
        .map(|d| d.description.clone())
        .unwrap_or_default();

    let trimmed = value_str.trim();
    let (raw_value, normalized_value, enum_index) = match config.data_type.as_deref() {
        Some("bool") => {
            let b = parse_bool_like(trimmed).unwrap_or(false);
            (if b { 1.0 } else { 0.0 }, if b { "1" } else { "0" }.to_string(), 0)
        }
        Some("int") | Some("int32") | Some("integer") => {
            let v = trimmed.parse::<i64>().unwrap_or(0);
            (v as f64, v.to_string(), 0)
        }
        Some("enum") => {
            let idx = trimmed.parse::<i16>().unwrap_or(0);
            (idx as f64, idx.to_string(), idx)
        }
        Some("string") => {
            let raw = trimmed.parse::<f64>().unwrap_or(0.0);
            (raw, trimmed.to_string(), 0)
        }
        _ => {
            let raw = trimmed.parse::<f64>().unwrap_or(0.0);
            (raw, format!("{:.prec$}", raw, prec = precision as usize), 0)
        }
    };

    let display_low = meta_display.map(|d| d.limit_low).unwrap_or(0.0);
    let display_high = meta_display.map(|d| d.limit_high).unwrap_or(100.0);
    let control_low = meta_control.map(|c| c.limit_low).unwrap_or(display_low);
    let control_high = meta_control.map(|c| c.limit_high).unwrap_or(display_high);

    let alarm_severity = meta_alarm.map(|a| a.compute_severity(raw_value)).unwrap_or(0);

    let array_values = if matches!(config.widget_type, WidgetType::Chart)
        && matches!(config.data_type.as_deref(), Some("double_array"))
    {
        value_str
            .split(',')
            .filter_map(|x| x.trim().parse::<f64>().ok())
            .collect()
    } else {
        Vec::new()
    };

    ChannelValue {
        raw_value,
        value_str: normalized_value,
        array_values,
        precision,
        display_low,
        display_high,
        control_low,
        control_high,
        low_alarm_limit: meta_alarm.map(|a| a.low_alarm_limit).unwrap_or(0.0),
        low_warn_limit: meta_alarm.map(|a| a.low_warning_limit).unwrap_or(0.0),
        high_warn_limit: meta_alarm.map(|a| a.high_warning_limit).unwrap_or(display_high),
        high_alarm_limit: meta_alarm.map(|a| a.high_alarm_limit).unwrap_or(display_high),
        alarm_severity,
        enum_index,
        enum_choices: config.options.clone().unwrap_or_default(),
        primary_meta: PrimaryMeta {
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
