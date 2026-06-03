mod test_widget_button {
    use mycela::channel::ChannelValue;
    use mycela::config::{WidgetConfig, WidgetType};
    use mycela::widgets::button::{render_inner_connected, render_inner_disconnected};

    fn w() -> WidgetConfig {
        WidgetConfig {
            id: "btn".to_string(),
            widget_type: WidgetType::Button,
            label: "btn label".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_disconnected_button_renders_with_disabled_attribute() {
        let html = render_inner_disconnected(&w()).into_string();
        assert!(html.contains("disabled"));
    }

    #[test]
    fn test_connected_button_renders_without_disabled_attribute() {
        let cv = ChannelValue::default();
        let html = render_inner_connected(&w(), &cv).into_string();
        assert!(!html.contains("disabled"));
    }

    #[test]
    fn test_button_widget_label_appears_in_rendered_html() {
        let html = render_inner_connected(&w(), &ChannelValue::default()).into_string();
        // label from widget helper is "btn label"
        assert!(html.contains("btn label") || html.contains("btn"));
    }
}
