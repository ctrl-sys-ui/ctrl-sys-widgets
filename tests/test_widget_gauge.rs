mod test_widget_gauge {
    use mycela::channel::ChannelValue;
    use mycela::config::{WidgetConfig, WidgetType};
    use mycela::widgets::gauge::{render_inner_connected, render_inner_disconnected};
    use mycela::widgets::{render_gauge, MAJOR_ALARM_SVG, MINOR_ALARM_SVG};

    fn gauge_widget() -> WidgetConfig {
        WidgetConfig {
            id: "g".to_string(),
            widget_type: WidgetType::Gauge,
            label: "Gauge".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_disconnected_gauge_renders_offline_icon_and_placeholder() {
        let html = render_inner_disconnected(&gauge_widget()).into_string();
        assert!(html.contains("widget-status-icon"), "got: {html}");
        assert!(html.contains("--"));
    }

    #[test]
    fn test_connected_gauge_with_no_alarm_shows_no_alarm_icons() {
        let cv = ChannelValue {
            value_str: "42.5".to_string(),
            alarm_severity: 0,
            display_low: 0.0,
            display_high: 100.0,
            ..ChannelValue::default()
        };
        let html = render_inner_connected(&gauge_widget(), &cv).into_string();
        assert!(!html.contains(MINOR_ALARM_SVG));
        assert!(!html.contains(MAJOR_ALARM_SVG));
    }

    #[test]
    fn test_connected_gauge_with_minor_alarm_severity_shows_minor_icon() {
        let cv = ChannelValue { alarm_severity: 1, ..ChannelValue::default() };
        let html = render_inner_connected(&gauge_widget(), &cv).into_string();
        assert!(html.contains(MINOR_ALARM_SVG));
    }

    #[test]
    fn test_connected_gauge_with_major_alarm_severity_shows_major_icon() {
        let cv = ChannelValue { alarm_severity: 2, ..ChannelValue::default() };
        let html = render_inner_connected(&gauge_widget(), &cv).into_string();
        assert!(html.contains(MAJOR_ALARM_SVG));
    }

    #[test]
    fn test_gauge_fill_bar_width_reflects_value_as_percentage_of_range() {
        let cv = ChannelValue {
            raw_value: 50.0,
            display_low: 0.0,
            display_high: 100.0,
            ..ChannelValue::default()
        };
        let html = render_inner_connected(&gauge_widget(), &cv).into_string();
        assert!(
            html.contains("width: 50.0%") || html.contains("width:50"),
            "expected 50% fill, got: {html}"
        );
    }

    #[test]
    fn test_gauge_alarm_markers_are_rendered_when_limits_configured() {
        let cv = ChannelValue {
            raw_value: 50.0,
            display_low: 0.0,
            display_high: 100.0,
            low_alarm_limit: 10.0,
            low_warn_limit: 20.0,
            high_warn_limit: 80.0,
            high_alarm_limit: 90.0,
            ..ChannelValue::default()
        };
        let html = render_inner_connected(&gauge_widget(), &cv).into_string();
        assert!(html.contains("gauge-marker--alarm"));
        assert!(html.contains("gauge-marker--warn"));
    }

    #[test]
    fn test_gauge_alarm_markers_are_absent_when_limits_at_default_values() {
        let cv = ChannelValue {
            display_low: 0.0,
            display_high: 100.0,
            ..ChannelValue::default()
        };
        let html = render_inner_connected(&gauge_widget(), &cv).into_string();
        assert!(!html.contains("gauge-marker--alarm"));
        assert!(!html.contains("gauge-marker--warn"));
    }

    #[test]
    fn test_gauge_outer_html_contains_widget_id_data_attribute() {
        let w = gauge_widget();
        let html = render_gauge(&w).into_string();
        assert!(html.contains("data-widget-id=\"g\""));
    }
}
