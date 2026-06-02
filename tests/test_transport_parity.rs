// Copyright 2026 Tine Zata
// SPDX-License-Identifier: MPL-2.0

mod test_transport_parity {
    use std::sync::{Arc, Mutex};

    use mycela::app::{modbus_status, server_status, write_widget_markup, AppState};
    use mycela::axum::extract::State;
    use mycela::axum::http::StatusCode;
    use mycela::channel::ChannelContext;
    use mycela::config::{AppConfig, ScreenConfig, WidgetConfig, WidgetType};
    use mycela::ipc::{IpcCommand, IpcMessageKind, IpcRequest};
    use mycela::ipc_dispatch::dispatch_request;

    fn make_app_state_with_widget(widget: WidgetConfig) -> AppState {
        let config = Arc::new(AppConfig {
            title: "transport parity".to_string(),
            home_screen: Some("s1".to_string()),
            screens: vec![ScreenConfig {
                id: "s1".to_string(),
                title: "Screen 1".to_string(),
                description: "test".to_string(),
                actions: None,
                widgets: vec![widget],
            }],
        });

        #[cfg(feature = "epics")]
        let epics_ctx = Arc::new(Mutex::new(
            mycela::pvxs_sys::Context::from_env().expect("pvxs context required"),
        ));

        #[cfg(feature = "modbus")]
        let modbus_pool = mycela::modbus_client::ModbusPool::new();

        #[cfg(all(feature = "epics", feature = "modbus"))]
        let channel_ctx = ChannelContext::new(epics_ctx, modbus_pool);

        #[cfg(all(feature = "epics", not(feature = "modbus")))]
        let channel_ctx = ChannelContext::new(epics_ctx);

        #[cfg(all(not(feature = "epics"), feature = "modbus"))]
        let channel_ctx = ChannelContext::new(modbus_pool);

        #[cfg(all(not(feature = "epics"), not(feature = "modbus")))]
        let channel_ctx = ChannelContext::new();

        AppState {
            #[cfg(feature = "epics")]
            pv_server: Arc::new(Mutex::new(None)),
            config,
            channel_ctx,
            modbus_task: Arc::new(Mutex::new(None)),
            #[cfg(feature = "epics")]
            epics_start_hook: None,
            #[cfg(feature = "modbus")]
            modbus_start_hook: None,
            loopback_token: None,
        }
    }

    fn make_request(cmd: IpcCommand, payload: serde_json::Value) -> IpcRequest {
        IpcRequest {
            v: 1,
            kind: IpcMessageKind::Request,
            id: "req-1".to_string(),
            cmd,
            token: None,
            payload,
            ts: 0,
        }
    }

    #[tokio::test]
    async fn test_widget_write_parity_http_and_ipc() {
        let widget = WidgetConfig {
            id: "w1".to_string(),
            widget_type: WidgetType::TextEntry,
            label: "Widget 1".to_string(),
            ..Default::default()
        };
        let state = make_app_state_with_widget(widget);

        let (status, markup) = write_widget_markup(&state, "w1", "42".to_string()).await;
        assert_eq!(status, StatusCode::OK);
        let http_html = markup.into_string();

        let request = make_request(
            IpcCommand::AppWidgetWrite,
            serde_json::json!({
                "widget_id": "w1",
                "value": "42"
            }),
        );
        let ipc_response = dispatch_request(&state, request, None).await;

        assert!(ipc_response.ok);
        let ipc_html = ipc_response
            .result
            .expect("ipc result present")["html"]
            .as_str()
            .expect("ipc html string")
            .to_string();

        assert_eq!(ipc_html, http_html);
    }

    #[tokio::test]
    async fn test_epics_status_parity_http_and_ipc_when_stopped() {
        let widget = WidgetConfig {
            id: "w1".to_string(),
            widget_type: WidgetType::TextUpdate,
            label: "Widget 1".to_string(),
            ..Default::default()
        };
        let state = make_app_state_with_widget(widget);

        let http = server_status(State(state.clone())).await.0;

        let request = make_request(IpcCommand::EpicsServerStatusGet, serde_json::json!({}));
        let ipc = dispatch_request(&state, request, None).await;

        assert!(http.contains("EPICS Server Stopped"));
        assert!(ipc.ok);
        assert_eq!(ipc.result.expect("ipc result present")["running"], false);
    }

    #[tokio::test]
    async fn test_modbus_status_parity_http_and_ipc_when_stopped() {
        let widget = WidgetConfig {
            id: "w1".to_string(),
            widget_type: WidgetType::TextUpdate,
            label: "Widget 1".to_string(),
            ..Default::default()
        };
        let state = make_app_state_with_widget(widget);

        let http = modbus_status(State(state.clone())).await.0;

        let request = make_request(IpcCommand::ModbusSimStatusGet, serde_json::json!({}));
        let ipc = dispatch_request(&state, request, None).await;

        assert!(http.contains("Modbus TCP Stopped"));
        assert!(ipc.ok);
        assert_eq!(ipc.result.expect("ipc result present")["running"], false);
    }
}
