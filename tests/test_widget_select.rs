mod test_widget_select {
    use mycela::channel::ChannelValue;
    use mycela::config::{WidgetConfig, WidgetType};
    use mycela::widgets::select::{render_inner_connected, render_inner_disconnected};

    fn w() -> WidgetConfig {
        WidgetConfig {
            id: "sel".to_string(),
            widget_type: WidgetType::Select,
            label: "Select".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_disconnected_select_shows_alarm_disconnected_class() {
        let html = render_inner_disconnected(&w()).into_string();
        assert!(html.contains("alarm-disconnected"));
    }

    #[test]
    fn test_connected_select_with_no_alarm_uses_alarm_none_class() {
        let cv = ChannelValue {
            enum_index: 0,
            enum_choices: vec![],
            ..ChannelValue::default()
        };
        let cv = ChannelValue {
            alarm_severity: 0,
            ..cv
        };
        let html = render_inner_connected(&w(), &cv, true).into_string();
        assert!(html.contains("widget-select alarm-none"), "got: {html}");
    }

    #[test]
    fn test_connected_select_with_minor_alarm_uses_alarm_minor_class() {
        let cv = ChannelValue {
            alarm_severity: 1,
            ..ChannelValue::default()
        };
        let html = render_inner_connected(&w(), &cv, true).into_string();
        assert!(html.contains("widget-select alarm-minor"));
    }

    #[test]
    fn test_connected_select_with_major_alarm_uses_alarm_major_class() {
        let cv = ChannelValue {
            alarm_severity: 2,
            ..ChannelValue::default()
        };
        let html = render_inner_connected(&w(), &cv, true).into_string();
        assert!(html.contains("widget-select alarm-major"));
    }

    #[test]
    fn test_enum_choices_in_channel_value_are_rendered_as_select_options() {
        let cv = ChannelValue {
            enum_index: 1,
            enum_choices: vec!["Auto".to_string(), "Manual".to_string(), "Off".to_string()],
            ..ChannelValue::default()
        };
        let html = render_inner_connected(&w(), &cv, true).into_string();
        assert!(html.contains("Auto"));
        assert!(html.contains("Manual"));
        assert!(html.contains("Off"));
    }

    #[test]
    fn test_current_enum_index_option_is_marked_as_selected() {
        let cv = ChannelValue {
            enum_index: 2,
            enum_choices: vec!["A".to_string(), "B".to_string(), "C".to_string()],
            ..ChannelValue::default()
        };
        let html = render_inner_connected(&w(), &cv, true).into_string();
        assert!(html.contains("selected"));
    }

    #[test]
    fn test_connected_select_includes_status_placeholder_for_write_feedback() {
        let cv = ChannelValue {
            enum_index: 0,
            enum_choices: vec!["Off".to_string(), "On".to_string()],
            ..ChannelValue::default()
        };
        let html = render_inner_connected(&w(), &cv, true).into_string();
        assert!(html.contains("class=\"status\""), "got: {html}");
    }
}
