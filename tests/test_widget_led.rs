mod test_widget_led {
    use mycela::channel::ChannelValue;
    use mycela::config::{WidgetConfig, WidgetType};
    use mycela::widgets::led::{
        render_inner_connected,
        render_inner_disconnected,
        render_inner_disconnected_with_last,
    };
    use mycela::widgets::{MAJOR_ALARM_SVG, MINOR_ALARM_SVG, OFFLINE_SVG};

    fn w() -> WidgetConfig {
        WidgetConfig {
            id: "led".to_string(),
            widget_type: WidgetType::Led,
            label: "LED".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_disconnected_led_shows_offline_status_icon() {
        let html = render_inner_disconnected(&w()).into_string();
        assert!(html.contains(OFFLINE_SVG));
    }

    #[test]
    fn test_disconnected_led_keeps_last_on_state() {
        let cv = ChannelValue { raw_value: 1.0, ..ChannelValue::default() };
        let html = render_inner_disconnected_with_last(&w(), Some(&cv)).into_string();
        assert!(html.contains("led-on"), "got: {html}");
        assert!(html.contains("ON"), "got: {html}");
    }

    #[test]
    fn test_led_with_nonzero_raw_value_renders_on_state() {
        let cv = ChannelValue { raw_value: 1.0, ..ChannelValue::default() };
        let html = render_inner_connected(&w(), &cv).into_string();
        assert!(html.contains("led-on"));
        assert!(html.contains("ON"));
    }

    #[test]
    fn test_led_with_zero_raw_value_renders_off_state() {
        let cv = ChannelValue { raw_value: 0.0, ..ChannelValue::default() };
        let html = render_inner_connected(&w(), &cv).into_string();
        assert!(html.contains("led-off"));
        assert!(html.contains("OFF"));
    }

    #[test]
    fn test_led_with_minor_alarm_severity_shows_minor_alarm_icon() {
        let cv = ChannelValue { alarm_severity: 1, raw_value: 1.0, ..ChannelValue::default() };
        let html = render_inner_connected(&w(), &cv).into_string();
        assert!(html.contains(MINOR_ALARM_SVG));
    }

    #[test]
    fn test_led_with_major_alarm_severity_shows_major_alarm_icon() {
        let cv = ChannelValue { alarm_severity: 2, ..ChannelValue::default() };
        let html = render_inner_connected(&w(), &cv).into_string();
        assert!(html.contains(MAJOR_ALARM_SVG));
    }
}
