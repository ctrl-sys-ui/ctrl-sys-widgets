mod test_config_modbus_serde {
    use mycela::config::{ModbusTcpConfig, ModbusRegisterType, ScreenConfig};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn write_temp_config(json: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("mycela-config-{unique}.json"));
        fs::write(&path, json).unwrap();
        path
    }

    fn load_temp_config(json: &str) -> Result<ScreenConfig, mycela::config::ConfigError> {
        let path = write_temp_config(json);
        let result = ScreenConfig::load(path.to_str().unwrap());
        let _ = fs::remove_file(path);
        result
    }

    #[test]
    fn test_modbus_config_poll_interval_ms_alias_is_accepted() {
        let json = r#"{
            "host": "127.0.0.1",
            "register": 100,
            "register_type": "holding_register",
            "poll_interval_ms": 1000
        }"#;
        let m: ModbusTcpConfig = serde_json::from_str(json).unwrap();
        assert_eq!(m.min_poll_interval_ms, 1000);
    }

    #[test]
    fn test_modbus_config_missing_fields_use_correct_defaults() {
        let json = r#"{"host":"127.0.0.1","register":100,"register_type":"coil"}"#;
        let m: ModbusTcpConfig = serde_json::from_str(json).unwrap();
        assert_eq!(m.port, 502);
        assert_eq!(m.unit_id, 1);
        assert_eq!(m.min_poll_interval_ms, 500);
        assert_eq!(m.word_count, 1);
        assert_eq!(m.scale, 1.0);
        assert_eq!(m.offset, 0.0);
    }

    #[test]
    fn test_modbus_register_type_serde_roundtrip_for_all_variants() {
        let cases = [
            (ModbusRegisterType::HoldingRegister, "\"holding_register\""),
            (ModbusRegisterType::InputRegister, "\"input_register\""),
            (ModbusRegisterType::Coil, "\"coil\""),
            (ModbusRegisterType::DiscreteInput, "\"discrete_input\""),
        ];
        for (variant, expected_json) in cases {
            assert_eq!(serde_json::to_string(&variant).unwrap(), expected_json);
            let parsed: ModbusRegisterType = serde_json::from_str(expected_json).unwrap();
            assert_eq!(parsed, variant);
        }
    }

    #[test]
    fn test_screen_config_load_returns_error_for_missing_file() {
        assert!(ScreenConfig::load("/nonexistent/path/config.json").is_err());
    }

    #[test]
    fn test_screen_config_deserializes_correctly_from_valid_json() {
        let json = r#"{
            "id": "screen1",
            "title": "Test Screen",
            "description": "A test screen",
            "widgets": [{
                "id": "w1", "type": "text_update", "label": "Value",
                "protocol": {"type": "epics-pvxs", "pv_name": "test:double"}
            }]
        }"#;
        let sc: ScreenConfig = serde_json::from_str(json).unwrap();
        assert_eq!(sc.id, "screen1");
        assert_eq!(sc.widgets.len(), 1);
        assert_eq!(sc.widgets[0].id, "w1");
    }

    #[test]
    fn test_screen_config_validation_rejects_duplicate_widget_ids() {
        let json = r#"{
            "id": "s", "title": "T", "description": "D",
            "widgets": [
                {"id": "dup", "type": "text_update", "label": "A",
                 "protocol": {"type": "epics-pvxs", "pv_name": "a:pv"}},
                {"id": "dup", "type": "text_update", "label": "B",
                 "protocol": {"type": "epics-pvxs", "pv_name": "b:pv"}}
            ]
        }"#;
        let sc: ScreenConfig = serde_json::from_str(json).unwrap();
        assert!(ScreenConfig::validate_config(&sc).is_err());
    }

    #[test]
    fn test_screen_config_load_rejects_missing_root_fields_from_json_file() {
        let json = r#"{
            "id": "test",
            "widgets": []
        }"#;

        let err = load_temp_config(json).unwrap_err().to_string();
        assert!(err.contains("missing field `title`"), "unexpected error: {err}");
    }

    #[test]
    fn test_screen_config_load_rejects_missing_widget_label_from_json_file() {
        let json = r#"{
            "id": "test",
            "title": "Test Config",
            "description": "Test config with missing label",
            "widgets": [
                {
                    "id": "widget1",
                    "type": "text_update",
                    "data_type": "double",
                    "protocol": { "type": "epics-pvxs", "pv_name": "demo:test:pv" }
                }
            ]
        }"#;

        let err = load_temp_config(json).unwrap_err().to_string();
        assert!(err.contains("missing field `label`"), "unexpected error: {err}");
    }

    #[test]
    fn test_screen_config_load_rejects_invalid_widget_type_from_json_file() {
        let json = r#"{
            "id": "test",
            "title": "Test Config",
            "description": "Test config with invalid widget type",
            "widgets": [
                {
                    "id": "widget1",
                    "type": "invalid_type",
                    "label": "Test Widget",
                    "data_type": "double",
                    "protocol": { "type": "epics-pvxs", "pv_name": "demo:test:pv" }
                }
            ]
        }"#;

        let err = load_temp_config(json).unwrap_err().to_string();
        assert!(err.contains("unknown variant `invalid_type`"), "unexpected error: {err}");
    }

    #[test]
    fn test_screen_config_load_rejects_duplicate_widget_ids_from_json_file() {
        let json = r#"{
            "id": "test",
            "title": "Test Config",
            "description": "Test config with duplicate IDs",
            "widgets": [
                {
                    "id": "widget1",
                    "type": "text_update",
                    "label": "Test Widget 1",
                    "data_type": "double",
                    "protocol": { "type": "epics-pvxs", "pv_name": "demo:test:pv1" }
                },
                {
                    "id": "widget1",
                    "type": "text_entry",
                    "label": "Test Widget 2",
                    "data_type": "double",
                    "protocol": { "type": "epics-pvxs", "pv_name": "demo:test:pv2" }
                }
            ]
        }"#;

        let err = load_temp_config(json).unwrap_err().to_string();
        assert!(err.contains("duplicate ID"), "unexpected error: {err}");
    }
}
