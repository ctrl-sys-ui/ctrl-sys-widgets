// Copyright 2026 Tine Zata
// SPDX-License-Identifier: MPL-2.0

mod test_ipc_contract {
    use mycela::ipc::{IpcCommand, IpcMessageKind, IpcRequest};

    #[test]
    fn test_command_parses_namespaced_and_legacy_aliases() {
        let namespaced: IpcCommand = serde_json::from_str("\"epics.server.start\"")
            .expect("namespaced command should parse");
        let legacy: IpcCommand = serde_json::from_str("\"epics_server_start\"")
            .expect("legacy command alias should parse");

        assert_eq!(namespaced, IpcCommand::EpicsServerStart);
        assert_eq!(legacy, IpcCommand::EpicsServerStart);

        let modbus_namespaced: IpcCommand = serde_json::from_str("\"modbus.subscribe\"")
            .expect("namespaced modbus subscribe should parse");
        let modbus_legacy: IpcCommand = serde_json::from_str("\"modbus_subscribe\"")
            .expect("legacy modbus subscribe alias should parse");

        assert_eq!(modbus_namespaced, IpcCommand::ModbusSubscribe);
        assert_eq!(modbus_legacy, IpcCommand::ModbusSubscribe);
    }

    #[test]
    fn test_command_serializes_to_namespaced_form() {
        let json = serde_json::to_string(&IpcCommand::AppVersionGet)
            .expect("command should serialize");
        assert_eq!(json, "\"app.version.get\"");
    }

    #[test]
    fn test_mutating_command_detection_is_correct() {
        assert!(IpcCommand::EpicsPvWrite.is_mutating());
        assert!(IpcCommand::ModbusWrite.is_mutating());
        assert!(!IpcCommand::EpicsPvRead.is_mutating());
        assert!(!IpcCommand::ModbusRead.is_mutating());
        assert!(!IpcCommand::EpicsPvSubscribe.is_mutating());
    }

    #[test]
    fn test_request_roundtrip_preserves_contract_shape() {
        let request = IpcRequest {
            v: 1,
            kind: IpcMessageKind::Request,
            id: "req-123".to_string(),
            cmd: IpcCommand::ModbusRead,
            token: None,
            payload: serde_json::json!({ "widget_id": "pump_pressure" }),
            ts: 123456,
        };

        let encoded = serde_json::to_string(&request).expect("request should serialize");
        let decoded: IpcRequest =
            serde_json::from_str(&encoded).expect("request should deserialize");

        assert_eq!(decoded.v, 1);
        assert_eq!(decoded.kind, IpcMessageKind::Request);
        assert_eq!(decoded.id, "req-123");
        assert_eq!(decoded.cmd, IpcCommand::ModbusRead);
        assert_eq!(decoded.payload["widget_id"], "pump_pressure");
    }
}
