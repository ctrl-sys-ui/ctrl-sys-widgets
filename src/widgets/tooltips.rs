use maud::html;


/// Build a tooltip string from a `ChannelValue` — shared by all widgets.
pub(super) fn build_tooltip(config: &crate::config::WidgetConfig, cv: &crate::channel::ChannelValue) -> String {
    use crate::config::ProtocolConfig;
    let mut t = String::new();

    let protocol_label = match &config.protocol {
        Some(ProtocolConfig::Local(_)) => "Local",
        #[cfg(feature = "epics")]
        Some(ProtocolConfig::EpicsPva(_)) => "EPICS PVA",
        #[cfg(feature = "modbus")]
        Some(ProtocolConfig::ModbusTcp(_)) => "Modbus TCP",
        _ => "None",
    };
    t.push_str(&format!("ID: {}\n", config.id));
    t.push_str(&format!("Protocol: {}\n", protocol_label));
    t.push_str(&format!("Channel: {}\n", config.channel_address()));

    if !cv.primary_meta.description.is_empty() {
        t.push_str(&cv.primary_meta.description);
        t.push('\n');
    }
    if !cv.units.is_empty() {
        t.push_str(&format!("Units: {}\n", cv.units));
    }
    t.push_str(&format!("Precision: {}\n", cv.precision));

    if cv.control_low != std::f64::MIN || cv.control_high != std::f64::MAX {
        t.push_str(&format!("Control Low: {}\n", cv.control_low));
        t.push_str(&format!("Control High: {}\n", cv.control_high));
    }

    if cv.display_low != std::f64::MIN || cv.display_high != std::f64::MAX {
        t.push_str(&format!("Display Low: {}\n", cv.display_low));
        t.push_str(&format!("Display High: {}\n", cv.display_high));
    }
    if cv.control_low != cv.display_low || cv.control_high != cv.display_high {
        t.push_str(&format!("Control Low: {}\n", cv.control_low));
        t.push_str(&format!("Control High: {}\n", cv.control_high));
    }
    if cv.low_alarm_limit != std::f64::MIN || cv.high_alarm_limit != std::f64::MAX {
        t.push_str(&format!("Low Alarm Limit: {}\n", cv.low_alarm_limit));
        t.push_str(&format!("Low Warning Limit: {}\n", cv.low_warn_limit));
        t.push_str(&format!("High Warning Limit: {}\n", cv.high_warn_limit));
        t.push_str(&format!("High Alarm Limit: {}\n", cv.high_alarm_limit));
    }
    let sev_str = match cv.alarm_severity {
        0 => "No Alarm",
        1 => "Minor",
        2 => "Major",
        _ => "Invalid",
    };
    t.push_str(&format!("Alarm Severity: {}\n", sev_str));
    t.push_str(&format!(
        "Alarm Status: {}\n",
        crate::widgets::alarm_status_str(cv.alarm_status)
    ));

    t.trim_end().to_string()
}

fn tooltip_for_boolean_channel(config: &crate::config::WidgetConfig, cv: &crate::channel::ChannelValue) -> String {
    let sev_str = match cv.alarm_severity {
        0 => "No Alarm",
        1 => "Minor",
        2 => "Major",
        _ => "Invalid",
    };
    format!(
        "ID: {}\nProtocol: {}\nAlarm Severity: {}\nAlarm Status: {}",
        config.id,
        config.channel_address(),
        sev_str,
        crate::widgets::alarm_status_str(cv.alarm_status),
    )
}

/// Simplified tooltip for binary indicators (LED, MultiStateLed).
/// Shows only the fields relevant to a bool channel.
pub(super) fn build_led_tooltip(config: &crate::config::WidgetConfig, cv: &crate::channel::ChannelValue) -> String {
    tooltip_for_boolean_channel(config, cv)
}

/// Build a tooltip for button widgets. 
/// Shows only the fields relevant to a button channel.
pub(super) fn build_button_tooltip(config: &crate::config::WidgetConfig, cv: &crate::channel::ChannelValue) -> String {
    tooltip_for_boolean_channel(config, cv)
}

/// Build enum tootip for widgets like select
/// Shows only the fields relevant to a select channel.
pub (super) fn build_enum_tooltip(config: &crate::config::WidgetConfig, cv: &crate::channel::ChannelValue) -> String {
    let mut t = tooltip_for_boolean_channel(config, cv);
    if !cv.enum_choices.is_empty() {
        t.push_str("\nEnum Choices:\n");
        for (i, choice) in cv.enum_choices.iter().enumerate() {
            t.push_str(&format!("  {}: {}\n", i, choice));
        }
    }
    t.trim_end().to_string()
}

/// Build a minimal tooltip for a disconnected widget — shows ID and channel address
/// so the info button appears and is useful even before a connection is established.
pub(super) fn build_disconnected_tooltip(config: &crate::config::WidgetConfig) -> String {
    let ch = config.channel_address();
    if ch.is_empty() {
        format!("ID: {}\nStatus: Disconnected", config.id)
    } else {
        format!("ID: {}\nChannel: {}\nStatus: Disconnected", config.id, ch)
    }
}

/// Render an info button — two icon variants let CSS pick the right one per theme.
/// The button is absolutely positioned in the top-left corner of the nearest
/// `position:relative` ancestor (i.e. the widget container), so it never
/// participates in the widget-inner flex layout.
pub(super) fn render_tooltip_info_btn(tooltip: &str) -> maud::Markup {
    html! {
        button class="widget-info-btn"
               data-tooltip=(tooltip)
               type="button"
               style="position:absolute;top:2px;left:2px;z-index:10;" {
            img class="info-icon info-icon--dark"  src=(super::INFO_SVG_DARK)  alt="info";
            img class="info-icon info-icon--light" src=(super::INFO_SVG_LIGHT) alt="info";
        }
    }
}
