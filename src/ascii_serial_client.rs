use std::sync::Arc;
use std::time::Duration;

use crate::channel::{ChannelEvent, ChannelValue};
use crate::config::{
    AsciiLineEnding, AsciiResponseMode, ProtocolConfig, SerialDataBits, SerialParity,
    SerialStopBits, AsciiSerialConfig, WidgetConfig,
};

fn to_line_ending(ending: AsciiLineEnding) -> ascii_serial::LineEnding {
    match ending {
        AsciiLineEnding::Lf => ascii_serial::LineEnding::Lf,
        AsciiLineEnding::CrLf => ascii_serial::LineEnding::CrLf,
        AsciiLineEnding::Cr => ascii_serial::LineEnding::Cr,
    }
}

fn to_data_bits(bits: SerialDataBits) -> ascii_serial::DataBits {
    match bits {
        SerialDataBits::Five => ascii_serial::DataBits::Five,
        SerialDataBits::Six => ascii_serial::DataBits::Six,
        SerialDataBits::Seven => ascii_serial::DataBits::Seven,
        SerialDataBits::Eight => ascii_serial::DataBits::Eight,
    }
}

fn to_parity(parity: SerialParity) -> ascii_serial::Parity {
    match parity {
        SerialParity::None => ascii_serial::Parity::None,
        SerialParity::Odd => ascii_serial::Parity::Odd,
        SerialParity::Even => ascii_serial::Parity::Even,
    }
}

fn to_stop_bits(bits: SerialStopBits) -> ascii_serial::StopBits {
    match bits {
        SerialStopBits::One => ascii_serial::StopBits::One,
        SerialStopBits::Two => ascii_serial::StopBits::Two,
    }
}

fn parse_numeric_response(s: &str) -> Result<f64, String> {
    for token in s
        .split(|c: char| c.is_whitespace() || c == ',' || c == ';' || c == '=')
        .filter(|t| !t.is_empty())
    {
        if let Ok(v) = token.parse::<f64>() {
            return Ok(v);
        }
    }
    Err(format!("No numeric token found in response '{}'", s))
}

fn parse_bool_response(s: &str) -> Result<f64, String> {
    let norm = s.trim().to_ascii_lowercase();
    match norm.as_str() {
        "1" | "true" | "on" | "open" => Ok(1.0),
        "0" | "false" | "off" | "closed" => Ok(0.0),
        _ => Err(format!("Could not parse boolean response '{}'", s)),
    }
}

fn build_serial_channel_value(
    physical: f64,
    raw_response: &str,
    config: &WidgetConfig,
) -> ChannelValue {
    let meta_display = config.metadata.as_ref().and_then(|md| md.display.as_ref());
    let meta_control = config.metadata.as_ref().and_then(|md| md.control.as_ref());
    let meta_alarm = config.metadata.as_ref().and_then(|md| md.alarm.as_ref());

    let precision = meta_display.map(|d| d.precision).unwrap_or(2);
    let units = meta_display.map(|d| d.units.clone()).unwrap_or_default();
    let description = meta_display
        .map(|d| d.description.clone())
        .unwrap_or_default();

    let value_str = match config.data_type.as_deref() {
        Some("bool") | Some("int32") | Some("int") | Some("enum") => {
            (physical as i64).to_string()
        }
        Some("string") => raw_response.to_string(),
        _ => format!("{:.prec$}", physical, prec = precision as usize),
    };

    let display_low = meta_display.map(|d| d.limit_low).unwrap_or(std::f64::MIN);
    let display_high = meta_display.map(|d| d.limit_high).unwrap_or(std::f64::MAX);
    let control_low = meta_control.map(|c| c.limit_low).unwrap_or(display_low);
    let control_high = meta_control.map(|c| c.limit_high).unwrap_or(display_high);
    let alarm_severity = meta_alarm
        .map(|a| a.compute_severity(physical))
        .unwrap_or(0);

    ChannelValue {
        raw_value: physical,
        value_str,
        precision,
        display_low,
        display_high,
        control_low,
        control_high,
        low_alarm_limit: meta_alarm.map(|a| a.low_alarm_limit).unwrap_or(std::f64::MIN),
        low_warn_limit: meta_alarm
            .map(|a| a.low_warning_limit)
            .unwrap_or(std::f64::MIN),
        high_warn_limit: meta_alarm
            .map(|a| a.high_warning_limit)
            .unwrap_or(std::f64::MAX),
        high_alarm_limit: meta_alarm.map(|a| a.high_alarm_limit).unwrap_or(std::f64::MAX),
        alarm_severity,
        enum_index: if matches!(config.data_type.as_deref(), Some("enum")) {
            physical.round() as i16
        } else {
            0
        },
        enum_choices: config.options.clone().unwrap_or_default(),
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

fn parse_response(cfg: &AsciiSerialConfig, response: &str) -> Result<f64, String> {
    match cfg.response_mode {
        AsciiResponseMode::Number => parse_numeric_response(response),
        AsciiResponseMode::Bool => parse_bool_response(response),
        AsciiResponseMode::Text => Ok(0.0),
    }
}

pub fn ascii_serial_stream(
    config: Arc<WidgetConfig>,
) -> impl tokio_stream::Stream<Item = ChannelEvent> + Send + 'static {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<ChannelEvent>();

    let s = match config.protocol.as_ref() {
        Some(ProtocolConfig::AsciiSerial(s)) => s.clone(),
        _ => {
            let _ = tx.send(ChannelEvent::Error(
                "ascii_serial_stream: not a ascii-serial widget".into(),
            ));
            return tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
        }
    };

    tokio::spawn(run_ascii_serial_poll(s, config, tx));

    tokio_stream::wrappers::UnboundedReceiverStream::new(rx)
}

async fn run_ascii_serial_poll(
    s: AsciiSerialConfig,
    config: Arc<WidgetConfig>,
    tx: tokio::sync::mpsc::UnboundedSender<ChannelEvent>,
) {
    let mut interval = tokio::time::interval(Duration::from_millis(s.min_poll_interval_ms.max(50)));
    let mut was_connected = false;
    let mut last_value_str: Option<String> = None;

    loop {
        interval.tick().await;

        let request_cfg = ascii_serial::AsciiSerialConfig {
            port_path: s.port_path.clone(),
            baud_rate: s.baud_rate,
            data_bits: to_data_bits(s.data_bits),
            parity: to_parity(s.parity),
            stop_bits: to_stop_bits(s.stop_bits),
            open_timeout: Duration::from_secs(2),
            io_timeout: Duration::from_secs(2),
            line_ending: to_line_ending(s.line_ending),
        };

        match ascii_serial::exchange_line(&request_cfg, &s.read_command).await {
            Ok(response) => {
                if !was_connected {
                    was_connected = true;
                    let _ = tx.send(ChannelEvent::Connected);
                }

                let physical = match s.response_mode {
                    AsciiResponseMode::Text => 0.0,
                    _ => match parse_response(&s, &response) {
                        Ok(raw) => raw * s.scale + s.offset,
                        Err(e) => {
                            if tx.send(ChannelEvent::Error(e)).is_err() {
                                break;
                            }
                            continue;
                        }
                    },
                };

                let cv = if matches!(s.response_mode, AsciiResponseMode::Text) {
                    let mut cv = build_serial_channel_value(0.0, &response, &config);
                    cv.value_str = response;
                    cv
                } else {
                    build_serial_channel_value(physical, &response, &config)
                };

                if last_value_str.as_deref() != Some(&cv.value_str) {
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
                    if tx.send(ChannelEvent::Disconnected(e.to_string())).is_err() {
                        break;
                    }
                }
            }
        }
    }
}

pub async fn ascii_serial_write(s: &AsciiSerialConfig, value_str: &str) -> Result<(), String> {
    let outbound_value = if matches!(s.response_mode, AsciiResponseMode::Number) {
        let physical: f64 = value_str
            .trim()
            .parse()
            .map_err(|_| format!("Invalid numeric value '{}'", value_str.trim()))?;
        let raw = (physical - s.offset) / s.scale;
        raw.to_string()
    } else {
        value_str.trim().to_string()
    };

    let command = match &s.write_command {
        Some(tpl) => tpl.replace("{value}", &outbound_value),
        None => outbound_value,
    };

    let request_cfg = ascii_serial::AsciiSerialConfig {
        port_path: s.port_path.clone(),
        baud_rate: s.baud_rate,
        data_bits: to_data_bits(s.data_bits),
        parity: to_parity(s.parity),
        stop_bits: to_stop_bits(s.stop_bits),
        open_timeout: Duration::from_secs(2),
        io_timeout: Duration::from_secs(2),
        line_ending: to_line_ending(s.line_ending),
    };

    ascii_serial::exchange_line(&request_cfg, &command)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}
