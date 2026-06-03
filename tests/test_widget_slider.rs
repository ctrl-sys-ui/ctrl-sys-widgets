mod test_widget_slider {
    use mycela::channel::ChannelValue;
    use mycela::config::{WidgetConfig, WidgetType};
    use mycela::widgets::slider::{render_inner_connected, render_inner_disconnected};
    use mycela::widgets::{MAJOR_ALARM_SVG, MINOR_ALARM_SVG};

    fn w() -> WidgetConfig {
        WidgetConfig {
            id: "sl".to_string(),
            widget_type: WidgetType::Slider,
            label: "Slider".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_disconnected_slider_shows_offline_icon_and_disabled_input() {
        let html = render_inner_disconnected(&w()).into_string();
        assert!(html.contains("widget-status-icon"), "{html}");
        assert!(html.contains("disabled"));
    }

    #[test]
    fn test_connected_slider_with_no_alarm_shows_no_status_icon() {
        let cv = ChannelValue { value_str: "50.0".to_string(), ..ChannelValue::default() };
        let html = render_inner_connected(&w(), &cv).into_string();
        assert!(!html.contains("widget-status-icon"), "got: {html}");
    }

    #[test]
    fn test_connected_slider_with_minor_alarm_shows_minor_alarm_icon() {
        let cv = ChannelValue { alarm_severity: 1, ..ChannelValue::default() };
        let html = render_inner_connected(&w(), &cv).into_string();
        assert!(html.contains(MINOR_ALARM_SVG));
    }

    #[test]
    fn test_connected_slider_with_major_alarm_shows_major_alarm_icon() {
        let cv = ChannelValue { alarm_severity: 2, ..ChannelValue::default() };
        let html = render_inner_connected(&w(), &cv).into_string();
        assert!(html.contains(MAJOR_ALARM_SVG));
    }

    #[test]
    fn test_connected_slider_renders_control_range_min_and_max_in_html() {
        let cv = ChannelValue {
            control_low: 10.0,
            control_high: 90.0,
            raw_value: 50.0,
            value_str: "50.0".to_string(),
            ..ChannelValue::default()
        };
        let html = render_inner_connected(&w(), &cv).into_string();
        assert!(
            html.contains("10") && html.contains("90"),
            "expected range in output, got: {html}"
        );
    }
}
