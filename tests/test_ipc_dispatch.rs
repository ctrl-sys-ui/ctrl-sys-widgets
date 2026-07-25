mod test_ipc_dispatch {
    use std::sync::{Arc, Mutex};

    use mycela::app::AppState;
    use mycela::channel::ChannelContext;
    use mycela::config::AppConfig;
    use mycela::ipc::{IpcCommand, IpcErrorCode, IpcMessageKind, IpcRequest};
    use mycela::ipc_dispatch::dispatch_request;

    fn make_app_state() -> AppState {
        let config = Arc::new(AppConfig {
            title: "test".to_string(),
            home_screen: None,
            startup: Default::default(),
            screens: Vec::new(),
        });

        #[cfg(feature = "epics-pvxs")]
        let epics_ctx = Arc::new(Mutex::new(
            mycela::pvxs_sys::Context::from_env().expect("pvxs context required"),
        ));

        #[cfg(feature = "modbus")]
        let modbus_pool = mycela::modbus_client::ModbusPool::new();

        #[cfg(all(feature = "epics-pvxs", feature = "modbus"))]
        let channel_ctx = ChannelContext::new(epics_ctx, modbus_pool);

        #[cfg(all(feature = "epics-pvxs", not(feature = "modbus")))]
        let channel_ctx = ChannelContext::new(epics_ctx);

        #[cfg(all(not(feature = "epics-pvxs"), feature = "modbus"))]
        let channel_ctx = ChannelContext::new(modbus_pool);

        #[cfg(all(not(feature = "epics-pvxs"), not(feature = "modbus")))]
        let channel_ctx = ChannelContext::new();

        AppState {
            #[cfg(feature = "epics-pvxs")]
            pv_server: Arc::new(Mutex::new(None)),
            config,
            channel_ctx,
            modbus_task: Arc::new(Mutex::new(None)),
            #[cfg(feature = "epics-pvxs")]
            epics_start_hook: None,
            #[cfg(feature = "modbus")]
            modbus_start_hook: None,
            loopback_token: None,
        }
    }

    fn make_request(cmd: IpcCommand) -> IpcRequest {
        IpcRequest {
            v: 1,
            kind: IpcMessageKind::Request,
            id: "req-1".to_string(),
            cmd,
            token: None,
            payload: serde_json::json!({}),
            ts: 0,
        }
    }

    #[tokio::test]
    async fn test_rejects_non_request_message_kind() {
        let state = make_app_state();
        let mut request = make_request(IpcCommand::AppPing);
        request.kind = IpcMessageKind::Event;

        let response = dispatch_request(&state, request, None).await;

        assert!(!response.ok);
        assert_eq!(
            response.error.expect("error present").code,
            IpcErrorCode::PayloadInvalid
        );
    }

    #[tokio::test]
    async fn test_rejects_mutating_command_with_invalid_token() {
        let state = make_app_state();
        let mut request = make_request(IpcCommand::AppWidgetWrite);
        request.payload = serde_json::json!({ "widget_id": "x", "value": "1" });

        let response = dispatch_request(&state, request, Some("expected-token")).await;

        assert!(!response.ok);
        assert_eq!(
            response.error.expect("error present").code,
            IpcErrorCode::AuthInvalidToken
        );
    }

    #[tokio::test]
    async fn test_ping_returns_ok_response() {
        let state = make_app_state();
        let request = make_request(IpcCommand::AppPing);

        let response = dispatch_request(&state, request, None).await;

        assert!(response.ok);
        assert_eq!(response.kind, IpcMessageKind::Response);
        assert_eq!(response.result.expect("result present")["pong"], true);
    }

    #[tokio::test]
    async fn test_protocol_subscribe_commands_are_orchestrated_outside_dispatcher() {
        let state = make_app_state();
        let request = make_request(IpcCommand::EpicsPvSubscribe);

        let response = dispatch_request(&state, request, None).await;

        assert!(!response.ok);
        assert_eq!(
            response.error.expect("error present").code,
            IpcErrorCode::CmdUnknown
        );
    }

    #[tokio::test]
    async fn test_version_get_returns_name_and_version() {
        let state = make_app_state();
        let request = make_request(IpcCommand::AppVersionGet);

        let response = dispatch_request(&state, request, None).await;

        assert!(response.ok);
        let result = response.result.expect("result present");
        assert_eq!(result["name"], "mycela");
        assert!(!result["version"].as_str().expect("version is string").is_empty());
    }

    #[tokio::test]
    async fn test_mutating_command_accepted_with_correct_token() {
        let state = make_app_state();
        let mut request = make_request(IpcCommand::AppWidgetWrite);
        request.token = Some("correct".to_string());
        request.payload = serde_json::json!({ "widget_id": "does_not_exist", "value": "1" });

        // Token matches — auth passes, fails on missing widget (PayloadInvalid), not AuthInvalidToken.
        let response = dispatch_request(&state, request, Some("correct")).await;
        assert!(!response.ok);
        assert_eq!(
            response.error.expect("error present").code,
            IpcErrorCode::PayloadInvalid
        );
    }

    #[tokio::test]
    async fn test_modbus_subscribe_returns_cmd_unknown() {
        let state = make_app_state();
        let request = make_request(IpcCommand::ModbusSubscribe);

        let response = dispatch_request(&state, request, None).await;

        assert!(!response.ok);
        assert_eq!(
            response.error.expect("error present").code,
            IpcErrorCode::CmdUnknown
        );
    }

    // ── Feature: modbus ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_modbus_sim_status_returns_not_running_when_idle() {
        let state = make_app_state();
        let request = make_request(IpcCommand::ModbusSimStatusGet);

        let response = dispatch_request(&state, request, None).await;

        assert!(response.ok);
        assert_eq!(response.result.expect("result present")["running"], false);
    }

    #[tokio::test]
    async fn test_modbus_sim_stop_returns_state_conflict_when_not_running() {
        let state = make_app_state();
        let mut request = make_request(IpcCommand::ModbusSimStop);
        request.token = Some("tok".to_string());

        let response = dispatch_request(&state, request, Some("tok")).await;

        assert!(!response.ok);
        assert_eq!(
            response.error.expect("error present").code,
            IpcErrorCode::StateConflict
        );
    }

    #[cfg(feature = "modbus")]
    #[tokio::test]
    async fn test_modbus_sim_start_returns_internal_error_without_start_hook() {
        // With no modbus_start_hook set, start_modbus_runtime returns an error.
        let state = make_app_state();
        let mut request = make_request(IpcCommand::ModbusSimStart);
        request.token = Some("tok".to_string());

        let response = dispatch_request(&state, request, Some("tok")).await;

        assert!(!response.ok);
        let code = response.error.expect("error present").code;
        assert!(
            code == IpcErrorCode::StateConflict || code == IpcErrorCode::InternalError,
            "expected StateConflict or InternalError, got {:?}", code
        );
    }

    #[cfg(not(feature = "modbus"))]
    #[tokio::test]
    async fn test_modbus_sim_start_returns_internal_error_when_feature_disabled() {
        let state = make_app_state();
        let mut request = make_request(IpcCommand::ModbusSimStart);
        request.token = Some("tok".to_string());

        let response = dispatch_request(&state, request, Some("tok")).await;

        assert!(!response.ok);
        assert_eq!(
            response.error.expect("error present").code,
            IpcErrorCode::InternalError
        );
    }

    #[tokio::test]
    async fn test_modbus_read_returns_payload_invalid_for_missing_widget() {
        let state = make_app_state();
        let mut request = make_request(IpcCommand::ModbusRead);
        request.payload = serde_json::json!({ "widget_id": "no_such_widget" });

        let response = dispatch_request(&state, request, None).await;

        assert!(!response.ok);
        assert_eq!(
            response.error.expect("error present").code,
            IpcErrorCode::PayloadInvalid
        );
    }

    #[tokio::test]
    async fn test_modbus_write_returns_payload_invalid_for_missing_widget() {
        let state = make_app_state();
        let mut request = make_request(IpcCommand::ModbusWrite);
        request.token = Some("tok".to_string());
        request.payload = serde_json::json!({ "widget_id": "no_such_widget", "value": "42" });

        let response = dispatch_request(&state, request, Some("tok")).await;

        assert!(!response.ok);
        assert_eq!(
            response.error.expect("error present").code,
            IpcErrorCode::PayloadInvalid
        );
    }

    // ── Feature: epics ────────────────────────────────────────────────────────

    #[cfg(not(feature = "epics-pvxs"))]
    #[tokio::test]
    async fn test_epics_server_start_returns_cmd_unknown_when_feature_disabled() {
        let state = make_app_state();
        let mut request = make_request(IpcCommand::EpicsServerStart);
        request.token = Some("tok".to_string());

        let response = dispatch_request(&state, request, Some("tok")).await;

        assert!(!response.ok);
        assert_eq!(
            response.error.expect("error present").code,
            IpcErrorCode::CmdUnknown
        );
    }

    #[cfg(not(feature = "epics-pvxs"))]
    #[tokio::test]
    async fn test_epics_server_stop_returns_cmd_unknown_when_feature_disabled() {
        let state = make_app_state();
        let mut request = make_request(IpcCommand::EpicsServerStop);
        request.token = Some("tok".to_string());

        let response = dispatch_request(&state, request, Some("tok")).await;

        assert!(!response.ok);
        assert_eq!(
            response.error.expect("error present").code,
            IpcErrorCode::CmdUnknown
        );
    }

    #[cfg(not(feature = "epics-pvxs"))]
    #[tokio::test]
    async fn test_epics_server_status_returns_not_running_when_feature_disabled() {
        let state = make_app_state();
        let request = make_request(IpcCommand::EpicsServerStatusGet);

        let response = dispatch_request(&state, request, None).await;

        assert!(response.ok);
        assert_eq!(response.result.expect("result present")["running"], false);
    }

    #[cfg(not(feature = "epics-pvxs"))]
    #[tokio::test]
    async fn test_epics_pv_read_returns_payload_invalid_for_missing_widget_without_epics() {
        let state = make_app_state();
        let mut request = make_request(IpcCommand::EpicsPvRead);
        request.payload = serde_json::json!({ "widget_id": "no_such_pv" });

        let response = dispatch_request(&state, request, None).await;

        assert!(!response.ok);
        assert_eq!(
            response.error.expect("error present").code,
            IpcErrorCode::PayloadInvalid
        );
    }

    #[cfg(feature = "epics-pvxs")]
    #[tokio::test]
    async fn test_epics_server_status_returns_not_running_when_no_server_started() {
        let state = make_app_state();
        let request = make_request(IpcCommand::EpicsServerStatusGet);

        let response = dispatch_request(&state, request, None).await;

        assert!(response.ok);
        assert_eq!(response.result.expect("result present")["running"], false);
    }

    #[cfg(feature = "epics-pvxs")]
    #[tokio::test]
    async fn test_epics_server_stop_returns_state_conflict_when_not_running() {
        let state = make_app_state();
        let mut request = make_request(IpcCommand::EpicsServerStop);
        request.token = Some("tok".to_string());

        let response = dispatch_request(&state, request, Some("tok")).await;

        assert!(!response.ok);
        assert_eq!(
            response.error.expect("error present").code,
            IpcErrorCode::StateConflict
        );
    }
}
