mod test_widget_text_entry {
    use mycela::channel::ChannelValue;
    use mycela::config::{WidgetConfig, WidgetType};
    use mycela::widgets::text_entry::{render_inner_connected, render_inner_disconnected};

    fn w() -> WidgetConfig {
        WidgetConfig {
            id: "te".to_string(),
            widget_type: WidgetType::TextEntry,
            label: "Text Entry".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_disconnected_text_entry_shows_alarm_disconnected_class_and_placeholder() {
        let html = render_inner_disconnected(&w(), "reason").into_string();
        assert!(html.contains("alarm-disconnected"));
        assert!(html.contains("--"));
    }

    #[test]
    fn test_connected_text_entry_with_no_alarm_uses_alarm_none_class() {
        let cv = ChannelValue { value_str: "50.0".to_string(), ..ChannelValue::default() };
        let html = render_inner_connected(&w(), &cv).into_string();
        assert!(html.contains("alarm-none"), "got: {html}");
    }

    #[test]
    fn test_connected_text_entry_with_minor_alarm_uses_alarm_minor_class() {
        let cv = ChannelValue { alarm_severity: 1, ..ChannelValue::default() };
        let html = render_inner_connected(&w(), &cv).into_string();
        assert!(html.contains("alarm-minor"));
    }

    #[test]
    fn test_connected_text_entry_with_major_alarm_uses_alarm_major_class() {
        let cv = ChannelValue { alarm_severity: 2, ..ChannelValue::default() };
        let html = render_inner_connected(&w(), &cv).into_string();
        assert!(html.contains("alarm-major"));
    }

    #[test]
    fn test_channel_value_string_appears_in_text_entry_input_field() {
        let cv = ChannelValue { value_str: "12.34".to_string(), ..ChannelValue::default() };
        let html = render_inner_connected(&w(), &cv).into_string();
        assert!(html.contains("12.34"));
    }

    #[test]
    fn test_string_data_type_renders_as_text_type_input() {
        let mut widget = w();
        widget.data_type = Some("string".to_string());
        let cv = ChannelValue { value_str: "hello".to_string(), ..ChannelValue::default() };
        let html = render_inner_connected(&widget, &cv).into_string();
        assert!(
            html.contains("type=\"text\"") || html.contains("type='text'")
        );
    }
}
