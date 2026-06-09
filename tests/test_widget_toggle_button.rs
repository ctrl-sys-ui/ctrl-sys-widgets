mod test_widget_toggle_button {
    use mycela::channel::ChannelValue;
    use mycela::config::{WidgetConfig, WidgetType};
    use mycela::widgets::toggle_button::{
        render_inner_connected, render_inner_disconnected,
    };

    fn w() -> WidgetConfig {
        WidgetConfig {
            id: "tb".to_string(),
            widget_type: WidgetType::ToggleButton,
            label: "Toggle".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_disconnected_toggle_button_renders_with_disabled_attribute() {
        let html = render_inner_disconnected(&w()).into_string();
        assert!(html.contains("disabled"));
    }

    #[test]
    fn test_toggle_button_with_nonzero_value_renders_on_state() {
        let cv = ChannelValue { raw_value: 1.0, ..ChannelValue::default() };
        let html = render_inner_connected(&w(), &cv).into_string();
        assert!(html.contains("widget-toggle-btn--on"));
        assert!(html.contains("ON"));
    }

    #[test]
    fn test_toggle_button_with_zero_value_renders_off_state() {
        let cv = ChannelValue { raw_value: 0.0, ..ChannelValue::default() };
        let html = render_inner_connected(&w(), &cv).into_string();
        assert!(html.contains("widget-toggle-btn--off"));
        assert!(html.contains("OFF"));
    }

    #[test]
    fn test_toggle_button_htmx_post_value_is_inverse_of_current_state() {
        // When ON (1.0), clicking should send 0
        let cv_on = ChannelValue { raw_value: 1.0, ..ChannelValue::default() };
        let html_on = render_inner_connected(&w(), &cv_on).into_string();
        assert!(
            html_on.contains("&quot;0&quot;") || html_on.contains(r#""0""#),
            "expected next_val=0 in: {html_on}"
        );

        // When OFF (0.0), clicking should send 1
        let cv_off = ChannelValue { raw_value: 0.0, ..ChannelValue::default() };
        let html_off = render_inner_connected(&w(), &cv_off).into_string();
        assert!(
            html_off.contains("&quot;1&quot;") || html_off.contains(r#""1""#),
            "expected next_val=1 in: {html_off}"
        );
    }

    #[test]
    fn test_toggle_button_countdown_renders_for_on_state_with_reset_timeout() {
        let cfg = WidgetConfig {
            reset_timeout: Some(5000),
            ..w()
        };
        let cv = ChannelValue { raw_value: 1.0, ..ChannelValue::default() };
        let html = render_inner_connected(&cfg, &cv).into_string();
        assert!(html.contains("widget-toggle-btn-countdown"));
    }

    #[test]
    fn test_toggle_button_countdown_not_rendered_for_off_state() {
        let cfg = WidgetConfig {
            reset_timeout: Some(5000),
            ..w()
        };
        let cv = ChannelValue { raw_value: 0.0, ..ChannelValue::default() };
        let html = render_inner_connected(&cfg, &cv).into_string();
        assert!(!html.contains("widget-toggle-btn-countdown"));
    }
}
