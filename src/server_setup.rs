use crate::config::{ServerConfig, WidgetConfig};
use crate::widgets::collect_data_widgets;
use std::collections::HashSet;

pub fn setup_server_pvs(
    server: &pvxs_sys::Server,
    widgets: &[WidgetConfig],
) -> pvxs_sys::Result<()> {
    let data_widgets = collect_data_widgets(widgets);
    let mut created: HashSet<String> = HashSet::new();

    for widget in &data_widgets {
        let epics = match widget.epics_pva() {
            Some(e) => e,
            None => continue,
        };
        let Some(server_config) = &epics.server else {
            continue;
        };
        if created.insert(epics.pv_name.clone()) {
            create_widget_pv(server, widget, &epics.pv_name, server_config)?;
            tracing::info!("Added PV: {}", epics.pv_name);
        }

        // For multi-series line charts, also create PVs for each extra entry in pv_names.
        if widget.chart_type.as_deref().unwrap_or("line") == "line" {
            if let Some(extra_pvs) = &epics.pv_names {
                let max_points = widget.max_points.unwrap_or(100);
                for extra_name in extra_pvs.iter().take(5) {
                    if created.insert(extra_name.clone()) {
                        let meta = build_pv_metadata(server_config);
                        tracing::info!(
                            "Creating DOUBLE ARRAY PV (extra series): {} ({} points)",
                            extra_name,
                            max_points
                        );
                        server.create_pv_double_array(extra_name, vec![0.0; max_points], meta)?;
                        tracing::info!("Added extra series PV: {}", extra_name);
                    }
                }
            }
        }
    }
    Ok(())
}

fn create_widget_pv(
    server: &pvxs_sys::Server,
    widget: &WidgetConfig,
    pv_name: &str,
    server_config: &ServerConfig,
) -> pvxs_sys::Result<()> {
    match widget.data_type.as_deref() {
        Some("enum") => {
            tracing::info!("Creating ENUM PV: {}", pv_name);
            let choices: Vec<&str> = widget
                .options
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .map(|s| s.as_str())
                .collect();
            let metadata = build_enum_metadata(server_config);
            server.create_pv_enum(pv_name, choices, 0, metadata)?;
        }
        _ => {
            let metadata = build_pv_metadata(server_config);
            match widget.data_type.as_deref() {
                Some("double") | Some("float") => {
                    tracing::info!("Creating DOUBLE PV: {}", pv_name);
                    server.create_pv_double(pv_name, 1.0, metadata)?;
                }
                Some("double_array") => {
                    let max_points = widget.max_points.unwrap_or(100);
                    tracing::info!(
                        "Creating DOUBLE ARRAY PV: {} ({} points)",
                        pv_name,
                        max_points
                    );
                    server.create_pv_double_array(pv_name, vec![0.0; max_points], metadata)?;
                }
                Some("int32") | Some("int") | Some("integer") | Some("bool") => {
                    tracing::info!("Creating INT32 PV: {}", pv_name);
                    server.create_pv_int32(pv_name, 0, metadata)?
                }
                Some("string") | None => {
                    tracing::info!("Creating STRING PV: {}", pv_name);
                    server.create_pv_string(pv_name, "", metadata)?;
                }
                Some(other) => {
                    tracing::warn!(
                        "Unknown data_type '{}' for {}, defaulting to STRING",
                        other,
                        pv_name
                    );
                    server.create_pv_string(pv_name, "", metadata)?;
                }
            }
        }
    }
    Ok(())
}

fn build_enum_metadata(server_config: &ServerConfig) -> pvxs_sys::NTEnumMetadataBuilder {
    let severity = server_config
        .alarm_severity
        .as_ref()
        .map(|s| parse_alarm_severity(s))
        .unwrap_or(pvxs_sys::AlarmSeverity::NoAlarm);
    let status = server_config
        .alarm_status
        .as_ref()
        .map(|s| parse_alarm_status(s))
        .unwrap_or(pvxs_sys::AlarmStatus::NoAlarm);

    pvxs_sys::NTEnumMetadataBuilder::new().alarm(
        severity as i32,
        status as i32,
        server_config.alarm_message.as_deref().unwrap_or(""),
    )
}

fn build_pv_metadata(server_config: &ServerConfig) -> pvxs_sys::NTScalarMetadataBuilder {
    let severity = server_config
        .alarm_severity
        .as_ref()
        .map(|s| parse_alarm_severity(s))
        .unwrap_or(pvxs_sys::AlarmSeverity::NoAlarm);
    let status = server_config
        .alarm_status
        .as_ref()
        .map(|s| parse_alarm_status(s))
        .unwrap_or(pvxs_sys::AlarmStatus::NoAlarm);

    let mut builder = pvxs_sys::NTScalarMetadataBuilder::new().alarm(
        severity,
        status,
        server_config.alarm_message.as_deref().unwrap_or(""),
    );

    if let Some(metadata) = &server_config.metadata {
        if let Some(display) = &metadata.display {
            builder = builder.display(pvxs_sys::DisplayMetadata {
                // Round before truncating: preserves 0.5 → 1 rather than 0.
                // A future pvxs-sys update may expose f64 limits directly.
                limit_low: display.limit_low.round() as i64,
                limit_high: display.limit_high.round() as i64,
                description: display.description.clone(),
                units: display.units.clone(),
                precision: display.precision,
            });
        }
        if let Some(control) = &metadata.control {
            builder = builder.control(pvxs_sys::ControlMetadata {
                limit_low: control.limit_low,
                limit_high: control.limit_high,
                min_step: control.min_step,
            });
        }
        if let Some(alarm) = &metadata.alarm {
            builder = builder.alarm_metadata(pvxs_sys::AlarmMetadata {
                active: true,
                low_alarm_limit: alarm.low_alarm_limit,
                low_warning_limit: alarm.low_warning_limit,
                high_warning_limit: alarm.high_warning_limit,
                high_alarm_limit: alarm.high_alarm_limit,
                low_alarm_severity: parse_alarm_severity(&alarm.low_alarm_severity),
                low_warning_severity: parse_alarm_severity(&alarm.low_warning_severity),
                high_warning_severity: parse_alarm_severity(&alarm.high_warning_severity),
                high_alarm_severity: parse_alarm_severity(&alarm.high_alarm_severity),
                // Clamp before cast: i32 outside 0..=255 would wrap silently.
                hysteresis: alarm.hysteresis.clamp(0, 255) as u8,
            });
        }
    }
    builder
}

fn parse_alarm_severity(severity: &str) -> pvxs_sys::AlarmSeverity {
    match severity.to_uppercase().as_str() {
        "NONE" => pvxs_sys::AlarmSeverity::NoAlarm,
        "MINOR" => pvxs_sys::AlarmSeverity::Minor,
        "MAJOR" => pvxs_sys::AlarmSeverity::Major,
        "INVALID" => pvxs_sys::AlarmSeverity::Invalid,
        _ => {
            tracing::warn!("Unknown alarm severity '{}', using NoAlarm", severity);
            pvxs_sys::AlarmSeverity::NoAlarm
        }
    }
}

fn parse_alarm_status(status: &str) -> pvxs_sys::AlarmStatus {
    match status.to_uppercase().as_str() {
        "NOALARM" | "NO_ALARM" | "NONE" => pvxs_sys::AlarmStatus::NoAlarm,
        "DEVICE" => pvxs_sys::AlarmStatus::DeviceStatus,
        "DRIVER" => pvxs_sys::AlarmStatus::DriverStatus,
        "RECORD" => pvxs_sys::AlarmStatus::RecordStatus,
        "DB" => pvxs_sys::AlarmStatus::DbStatus,
        "CONFIG" => pvxs_sys::AlarmStatus::ConfigStatus,
        "CLIENT" => pvxs_sys::AlarmStatus::ClientStatus,
        _ => {
            tracing::warn!("Unknown alarm status '{}', using DeviceStatus", status);
            pvxs_sys::AlarmStatus::DeviceStatus
        }
    }
}
