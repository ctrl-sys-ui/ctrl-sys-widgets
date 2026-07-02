mod test_widget_chart {
    use mycela::channel::ChannelValue;
    use mycela::config::{WidgetConfig, WidgetType};
    use mycela::widgets::chart::{render_inner_disconnected, render_inner_disconnected_with_last};

    fn w() -> WidgetConfig {
        WidgetConfig {
            id: "chart".to_string(),
            widget_type: WidgetType::Chart,
            label: "Chart".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_disconnected_chart_without_last_value_shows_placeholder() {
        let html = render_inner_disconnected(&w()).into_string();
        assert!(html.contains("Waiting for data"), "got: {html}");
    }

    #[test]
    fn test_disconnected_chart_keeps_last_series_svg_when_available() {
        let cv = ChannelValue {
            array_values: vec![1.0, 2.0, 1.5, 2.5],
            ..ChannelValue::default()
        };
        let html = render_inner_disconnected_with_last(&w(), Some(&cv)).into_string();
        assert!(html.contains("<svg"), "got: {html}");
        assert!(!html.contains("Waiting for data"), "got: {html}");
    }
}
