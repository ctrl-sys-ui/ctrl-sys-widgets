mod test_widget_text_readout {
    use mycela::channel::ChannelValue;
    use mycela::config::{WidgetConfig, WidgetType};
    use mycela::widgets::text_update::{render_inner_connected, render_inner_disconnected};

    fn w() -> WidgetConfig {
        WidgetConfig {
            id: "tu".to_string(),
            widget_type: WidgetType::TextUpdate,
            label: "Text Update".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_disconnected_text_update_shows_alarm_disconnected_class_and_placeholder() {
        let html = render_inner_disconnected(&w(), "reason", None).into_string();
        assert!(html.contains("alarm-disconnected"), "alarm-disconnected must be in HTML, got: {html}");
        assert!(html.contains("--"));
    }

    #[test]
    fn test_disconnected_text_update_keeps_last_known_value_and_units() {
        let cv = ChannelValue {
            value_str: "99.9".to_string(),
            units: "degC".to_string(),
            ..ChannelValue::default()
        };
        let html = render_inner_disconnected(&w(), "reason", Some(&cv)).into_string();
        assert!(html.contains("alarm-disconnected"), "alarm-disconnected must be in HTML, got: {html}");
        assert!(html.contains("99.9"), "last value should be preserved, got: {html}");
        assert!(html.contains("degC"), "last units should be preserved, got: {html}");
    }

    #[test]
    fn test_connected_text_update_with_no_alarm_uses_alarm_none_class_and_displays_value() {
        let cv = ChannelValue {
            value_str: "42.0".to_string(),
            alarm_severity: 0,
            ..ChannelValue::default()
        };
        let html = render_inner_connected(&w(), &cv).into_string();
        assert!(html.contains("alarm-none"), "alarm-none must be in HTML, got: {html}");
        assert!(html.contains("42.0"));
    }

    #[test]
    fn test_connected_text_update_with_minor_alarm_uses_alarm_minor_class() {
        let cv = ChannelValue {
            alarm_severity: 1,
            value_str: "5.0".to_string(),
            ..ChannelValue::default()
        };
        let html = render_inner_connected(&w(), &cv).into_string();
        assert!(html.contains("alarm-minor"), "alarm-minor must be in HTML, got: {html}");
    }

    #[test]
    fn test_connected_text_update_with_major_alarm_uses_alarm_major_class() {
        let cv = ChannelValue { alarm_severity: 2, ..ChannelValue::default() };
        let html = render_inner_connected(&w(), &cv).into_string();
        assert!(html.contains("alarm-major"), "alarm-major must be in HTML, got: {html}");
    }

    #[test]
    fn test_connected_text_update_renders_value_string_and_units() {
        let cv = ChannelValue {
            value_str: "99.9".to_string(),
            units: "degC".to_string(),
            ..ChannelValue::default()
        };
        let html = render_inner_connected(&w(), &cv).into_string();
        assert!(html.contains("99.9"));
        assert!(html.contains("degC"));
    }

    #[test]
    fn test_alarm_only_change_produces_different_html() {
        let cv_no_alarm = ChannelValue { value_str: "42.0".to_string(), alarm_severity: 0, ..ChannelValue::default() };
        let cv_alarm    = ChannelValue { value_str: "42.0".to_string(), alarm_severity: 1, ..ChannelValue::default() };
        let html_none  = render_inner_connected(&w(), &cv_no_alarm).into_string();
        let html_alarm = render_inner_connected(&w(), &cv_alarm).into_string();
        assert_ne!(html_none, html_alarm, "alarm change with same value must produce different HTML");
        assert!(html_alarm.contains("alarm-minor"));
    }
}
