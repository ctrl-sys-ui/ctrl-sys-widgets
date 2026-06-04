mod test_config_widget_config {
    use mycela::config::{
        AppConfig, EpicsPvaConfig, ModbusTCPConfig, ModbusRegisterType,
        ProtocolConfig, WidgetConfig, WidgetType,
    };

    fn find_widget<'a>(widgets: &'a [WidgetConfig], id: &str) -> Option<&'a WidgetConfig> {
        for widget in widgets {
            if widget.id == id {
                return Some(widget);
            }
            if let Some(children) = &widget.children {
                if let Some(found) = find_widget(children, id) {
                    return Some(found);
                }
            }
        }
        None
    }

    fn find_widget_in_screens<'a>(config: &'a AppConfig, id: &str) -> Option<&'a WidgetConfig> {
        config
            .screens
            .iter()
            .find_map(|screen| find_widget(&screen.widgets, id))
    }

    fn widget(id: &str, widget_type: WidgetType) -> WidgetConfig {
        WidgetConfig {
            id: id.to_string(),
            widget_type,
            label: format!("{id} label"),
            ..Default::default()
        }
    }

    #[test]
    fn test_epics_pva_protocol_produces_pv_name_as_channel_address() {
        let mut w = widget("w1", WidgetType::TextUpdate);
        w.protocol = Some(ProtocolConfig::EpicsPva(EpicsPvaConfig {
            pv_name: "test:pv".to_string(),
            server: None,
            pv_names: None,
        }));
        assert_eq!(w.channel_address(), "test:pv");
    }

    #[test]
    fn test_modbus_tcp_protocol_produces_url_as_channel_address() {
        let mut w = widget("w2", WidgetType::Gauge);
        w.protocol = Some(ProtocolConfig::ModbusTcp(ModbusTCPConfig {
            host: "127.0.0.1".to_string(),
            port: 502,
            unit_id: 1,
            register: 1000,
            register_type: ModbusRegisterType::HoldingRegister,
            min_poll_interval_ms: 500,
            scale: 1.0,
            offset: 0.0,
            word_count: 1,
            bit_index: None,
        }));
        assert_eq!(w.channel_address(), "modbus-tcp://127.0.0.1:502/reg1000");
    }

    #[test]
    fn test_no_protocol_configured_returns_empty_channel_address() {
        assert_eq!(widget("w3", WidgetType::TextUpdate).channel_address(), "");
    }

    #[test]
    fn test_epics_pva_accessor_returns_some_and_modbus_returns_none() {
        let mut w = widget("e", WidgetType::TextUpdate);
        w.protocol = Some(ProtocolConfig::EpicsPva(EpicsPvaConfig {
            pv_name: "x:pv".to_string(),
            server: None,
            pv_names: None,
        }));
        assert!(w.epics_pva().is_some());
        assert!(w.modbus_tcp().is_none());
    }

    #[test]
    fn test_modbus_tcp_accessor_returns_some_and_epics_returns_none() {
        let mut w = widget("m", WidgetType::Gauge);
        w.protocol = Some(ProtocolConfig::ModbusTcp(ModbusTCPConfig {
            host: "127.0.0.1".to_string(),
            port: 502,
            unit_id: 1,
            register: 1000,
            register_type: ModbusRegisterType::HoldingRegister,
            min_poll_interval_ms: 500,
            scale: 1.0,
            offset: 0.0,
            word_count: 1,
            bit_index: None,
        }));
        assert!(w.modbus_tcp().is_some());
        assert!(w.epics_pva().is_none());
    }

    #[test]
    fn test_series_pvs_with_only_primary_pv_returns_single_element() {
        let e = EpicsPvaConfig {
            pv_name: "main:pv".to_string(),
            server: None,
            pv_names: None,
        };
        assert_eq!(e.series_pvs(), vec!["main:pv"]);
    }

    #[test]
    fn test_series_pvs_with_additional_pv_names_returns_all_combined() {
        let e = EpicsPvaConfig {
            pv_name: "main:pv".to_string(),
            server: None,
            pv_names: Some(vec!["e1".to_string(), "e2".to_string()]),
        };
        assert_eq!(e.series_pvs(), vec!["main:pv", "e1", "e2"]);
    }

    #[test]
    fn test_series_pvs_capped_at_six_total_including_primary() {
        let e = EpicsPvaConfig {
            pv_name: "main:pv".to_string(),
            server: None,
            pv_names: Some((0..10).map(|i| format!("extra:{i}")).collect()),
        };
        assert_eq!(e.series_pvs().len(), 6);
    }

    #[test]
    fn test_demo_text_update_dark_retains_display_and_alarm_metadata() {
        let config = AppConfig::load("examples/demo_app.json").unwrap();
        let widget = find_widget_in_screens(&config, "text_update_dark").unwrap();

        let metadata = widget.metadata.as_ref().unwrap();
        let display = metadata.display.as_ref().unwrap();
        let alarm = metadata.alarm.as_ref().unwrap();

        assert_eq!(widget.widget_type, WidgetType::TextUpdate);
        assert_eq!(display.units, "mm");
        assert_eq!(display.precision, 3);
        assert_eq!(alarm.low_alarm_limit, 5.0);
        assert_eq!(alarm.high_alarm_limit, 95.0);
    }

    #[test]
    fn test_demo_app_text_update_dark_retains_display_and_alarm_metadata() {
        let config = AppConfig::load("examples/demo_app.json").unwrap();
        let widget = find_widget_in_screens(&config, "text_update_dark").unwrap();

        let metadata = widget.metadata.as_ref().unwrap();
        let display = metadata.display.as_ref().unwrap();
        let alarm = metadata.alarm.as_ref().unwrap();

        assert_eq!(widget.widget_type, WidgetType::TextUpdate);
        assert_eq!(display.units, "mm");
        assert_eq!(display.precision, 3);
        assert_eq!(alarm.low_alarm_limit, 5.0);
        assert_eq!(alarm.high_alarm_limit, 95.0);
    }

    #[test]
    fn test_toggle_button_reset_timeout_deserializes() {
        let parsed: WidgetConfig = serde_json::from_value(serde_json::json!({
            "id": "tb_reset",
            "type": "toggle_button",
            "label": "Toggle",
            "reset_timeout": 1500,
            "reset_default": 0
        }))
        .expect("widget config should deserialize");

        assert_eq!(parsed.widget_type, WidgetType::ToggleButton);
        assert_eq!(parsed.reset_timeout, Some(1500));
        assert_eq!(parsed.reset_default, Some(0));
    }

    #[test]
    fn test_toggle_button_reset_default_omitted_deserializes_to_none() {
        let parsed: WidgetConfig = serde_json::from_value(serde_json::json!({
            "id": "tb_reset_none",
            "type": "toggle_button",
            "label": "Toggle",
            "reset_timeout": 500
        }))
        .expect("widget config should deserialize");

        assert_eq!(parsed.widget_type, WidgetType::ToggleButton);
        assert_eq!(parsed.reset_timeout, Some(500));
        assert_eq!(parsed.reset_default, None);
    }
}
