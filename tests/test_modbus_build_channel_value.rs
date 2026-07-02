#![cfg(feature = "modbus")]

mod test_modbus_build_channel_value {
    use mycela::config::{
        AlarmMetadata, ControlMetadata, DisplayMetadata, ModbusTCPConfig, ModbusRegisterType,
        PvMetadata, WidgetConfig, WidgetType,
    };
    use mycela::modbus_client::build_channel_value;

    fn widget(id: &str, widget_type: WidgetType) -> WidgetConfig {
        WidgetConfig {
            id: id.to_string(),
            widget_type,
            label: format!("{id} label"),
            ..Default::default()
        }
    }

    /// Modbus TCP channel on localhost:502, register 1000, scale/offset as given.
    fn modbus_channel(scale: f64, offset: f64) -> ModbusTCPConfig {
        ModbusTCPConfig {
            host: "127.0.0.1".to_string(),
            port: 502,
            unit_id: 1,
            register: 1000,
            register_type: ModbusRegisterType::HoldingRegister,
            min_poll_interval_ms: 500,
            scale,
            offset,
            word_count: 1,
            bit_index: None,
        }
    }

    /// Alarm band: low alarm < 10 (MAJOR), low warn < 20 (MINOR),
    ///             high warn > 80 (MINOR), high alarm > 90 (MAJOR).
    fn alarm_limits() -> AlarmMetadata {
        AlarmMetadata {
            low_alarm_limit: 10.0,
            low_warning_limit: 20.0,
            high_warning_limit: 80.0,
            high_alarm_limit: 90.0,
            low_alarm_severity: "MAJOR".to_string(),
            low_warning_severity: "MINOR".to_string(),
            high_warning_severity: "MINOR".to_string(),
            high_alarm_severity: "MAJOR".to_string(),
            hysteresis: 1,
        }
    }

    #[test]
    fn test_float_value_is_formatted_according_to_precision() {
        let cv = build_channel_value(3.14159, &modbus_channel(1.0, 0.0), &widget("w", WidgetType::Gauge));
        assert_eq!(cv.value_str, "3.14"); // default precision = 2
    }

    #[test]
    fn test_int32_data_type_formats_value_as_integer_string() {
        let mut w = widget("w", WidgetType::TextUpdate);
        w.data_type = Some("int32".to_string());
        let cv = build_channel_value(42.9, &modbus_channel(1.0, 0.0), &w);
        assert_eq!(cv.value_str, "42");
    }

    #[test]
    fn test_int_data_type_alias_also_formats_as_integer_string() {
        let mut w = widget("w", WidgetType::TextUpdate);
        w.data_type = Some("int".to_string());
        let cv = build_channel_value(7.7, &modbus_channel(1.0, 0.0), &w);
        assert_eq!(cv.value_str, "7");
    }

    #[test]
    fn test_enum_data_type_sets_integer_value_string_for_select_widgets() {
        let mut w = widget("w", WidgetType::Select);
        w.data_type = Some("enum".to_string());
        w.options = Some(vec!["CLOSED".to_string(), "OPENED".to_string()]);
        let cv = build_channel_value(1.0, &modbus_channel(1.0, 0.0), &w);
        assert_eq!(cv.value_str, "1");
    }

    #[test]
    fn test_enum_data_type_populates_enum_index_and_choices() {
        let mut w = widget("w", WidgetType::Select);
        w.data_type = Some("enum".to_string());
        w.options = Some(vec!["Off".to_string(), "Manual".to_string(), "Auto".to_string()]);
        let cv = build_channel_value(2.0, &modbus_channel(1.0, 0.0), &w);
        assert_eq!(cv.enum_index, 2);
        assert_eq!(cv.enum_choices, vec!["Off", "Manual", "Auto"]);
    }

    #[test]
    fn test_display_range_is_derived_from_register_when_no_metadata() {
        // scale=1, offset=0 → display_low = 0*1+0 = 0, display_high = 65535*1+0 = 65535
        let cv = build_channel_value(50.0, &modbus_channel(1.0, 0.0), &widget("w", WidgetType::Gauge));
        assert_eq!(cv.display_low, 0.0);
        assert!(cv.display_high > 0.0);
    }

    #[test]
    fn test_display_metadata_overrides_default_range_precision_and_units() {
        let mut w = widget("w", WidgetType::Gauge);
        w.metadata = Some(PvMetadata {
            display: Some(DisplayMetadata {
                limit_low: 0.0,
                limit_high: 200.0,
                description: String::new(),
                precision: 1,
                units: "bar".to_string(),
            }),
            control: None,
            alarm: None,
        });
        let cv = build_channel_value(100.0, &modbus_channel(1.0, 0.0), &w);
        assert_eq!(cv.units, "bar");
        assert_eq!(cv.precision, 1);
        assert_eq!(cv.display_high, 200.0);
        assert_eq!(cv.value_str, "100.0");
    }

    #[test]
    fn test_control_metadata_sets_independent_control_range() {
        let mut w = widget("w", WidgetType::Slider);
        w.metadata = Some(PvMetadata {
            display: Some(DisplayMetadata {
                limit_low: 0.0,
                limit_high: 100.0,
                description: String::new(),
                precision: 2,
                units: String::new(),
            }),
            control: Some(ControlMetadata { limit_low: 10.0, limit_high: 90.0, min_step: 1.0 }),
            alarm: None,
        });
        let cv = build_channel_value(50.0, &modbus_channel(1.0, 0.0), &w);
        assert_eq!(cv.control_low, 10.0);
        assert_eq!(cv.control_high, 90.0);
    }

    #[test]
    fn test_control_range_falls_back_to_display_range_when_absent() {
        let mut w = widget("w", WidgetType::Slider);
        w.metadata = Some(PvMetadata {
            display: Some(DisplayMetadata {
                limit_low: 5.0,
                limit_high: 95.0,
                description: String::new(),
                precision: 2,
                units: String::new(),
            }),
            control: None,
            alarm: None,
        });
        let cv = build_channel_value(50.0, &modbus_channel(1.0, 0.0), &w);
        assert_eq!(cv.control_low, cv.display_low);
        assert_eq!(cv.control_high, cv.display_high);
    }

    #[test]
    fn test_no_alarm_metadata_produces_zero_alarm_severity() {
        let cv = build_channel_value(50.0, &modbus_channel(1.0, 0.0), &widget("w", WidgetType::Gauge));
        assert_eq!(cv.alarm_severity, 0);
    }

    #[test]
    fn test_alarm_severity_is_zero_for_value_in_normal_range() {
        let mut w = widget("w", WidgetType::Gauge);
        w.metadata = Some(PvMetadata { display: None, control: None, alarm: Some(alarm_limits()) });
        let cv = build_channel_value(50.0, &modbus_channel(1.0, 0.0), &w);
        assert_eq!(cv.alarm_severity, 0);
    }

    #[test]
    fn test_alarm_severity_is_minor_for_value_above_high_warning() {
        let mut w = widget("w", WidgetType::Gauge);
        w.metadata = Some(PvMetadata { display: None, control: None, alarm: Some(alarm_limits()) });
        let cv = build_channel_value(85.0, &modbus_channel(1.0, 0.0), &w);
        assert_eq!(cv.alarm_severity, 1);
    }

    #[test]
    fn test_alarm_severity_is_major_for_value_above_high_alarm() {
        let mut w = widget("w", WidgetType::Gauge);
        w.metadata = Some(PvMetadata { display: None, control: None, alarm: Some(alarm_limits()) });
        let cv = build_channel_value(95.0, &modbus_channel(1.0, 0.0), &w);
        assert_eq!(cv.alarm_severity, 2);
    }

    #[test]
    fn test_alarm_severity_is_minor_for_value_below_low_warning() {
        let mut w = widget("w", WidgetType::Gauge);
        w.metadata = Some(PvMetadata { display: None, control: None, alarm: Some(alarm_limits()) });
        let cv = build_channel_value(15.0, &modbus_channel(1.0, 0.0), &w);
        assert_eq!(cv.alarm_severity, 1);
    }

    #[test]
    fn test_alarm_severity_is_major_for_value_below_low_alarm() {
        let mut w = widget("w", WidgetType::Gauge);
        w.metadata = Some(PvMetadata { display: None, control: None, alarm: Some(alarm_limits()) });
        let cv = build_channel_value(5.0, &modbus_channel(1.0, 0.0), &w);
        assert_eq!(cv.alarm_severity, 2);
    }

    #[test]
    fn test_alarm_limit_fields_are_copied_into_channel_value() {
        let mut w = widget("w", WidgetType::Gauge);
        w.metadata = Some(PvMetadata { display: None, control: None, alarm: Some(alarm_limits()) });
        let cv = build_channel_value(50.0, &modbus_channel(1.0, 0.0), &w);
        assert_eq!(cv.low_alarm_limit, 10.0);
        assert_eq!(cv.low_warn_limit, 20.0);
        assert_eq!(cv.high_warn_limit, 80.0);
        assert_eq!(cv.high_alarm_limit, 90.0);
    }

    #[test]
    fn test_raw_value_field_matches_the_physical_input() {
        let cv = build_channel_value(37.5, &modbus_channel(1.0, 0.0), &widget("w", WidgetType::Gauge));
        assert_eq!(cv.raw_value, 37.5);
    }
}

