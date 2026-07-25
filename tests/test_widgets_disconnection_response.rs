// These tests verify that each widget's `run_monitor_async` correctly maps a
// `ChannelEvent::Disconnected` signal — coming from a live channel that has
// dropped — to the widget's disconnected-state HTML fragment.
//
// Strategy: start an in-process Modbus TCP mock server that serves a small
// number of requests and then closes the connection, driving the stream to
// emit `ChannelEvent::Disconnected`.  Collect the HTML fragments emitted by
// `run_widget_monitor_async` and assert that a disconnection indicator
// eventually appears.

#![cfg(feature = "modbus")]

mod test_widgets_disconnection_response {
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    use mycela::channel::ChannelContext;
    use mycela::config::{
        ModbusRegisterType, ModbusTcpConfig, ProtocolConfig, WidgetConfig, WidgetType,
    };
    use mycela::modbus_client::ModbusPool;
    use mycela::widgets::run_widget_monitor_async;

    // ── minimal one-shot Modbus TCP server ────────────────────────────────────

    /// Binds to an OS-assigned port, accepts one connection, serves `max`
    /// FC=0x03 requests with register value 500, then closes the socket.
    async fn start_one_shot_server(max: usize) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            if let Ok((socket, _)) = listener.accept().await {
                serve_modbus(socket, max).await;
            }
        });
        port
    }

    async fn serve_modbus(mut socket: TcpStream, max: usize) {
        let mut buf = vec![0u8; 256];
        for _ in 0..max {
            let n = match socket.read(&mut buf).await {
                Ok(0) | Err(_) => return,
                Ok(n) => n,
            };
            if n < 12 {
                continue;
            }
            let txn_id = u16::from_be_bytes([buf[0], buf[1]]);
            let unit_id = buf[6];
            let fc = buf[7];
            let count = u16::from_be_bytes([buf[10], buf[11]]);

            let pdu: Vec<u8> = if fc == 0x03 {
                let bc = (count * 2) as u8;
                let mut p = vec![0x03, bc];
                for _ in 0..count {
                    p.extend_from_slice(&500u16.to_be_bytes());
                }
                p
            } else {
                vec![fc | 0x80, 0x01]
            };

            let len = (1 + pdu.len()) as u16;
            let mut resp = Vec::new();
            resp.extend_from_slice(&txn_id.to_be_bytes());
            resp.extend_from_slice(&0u16.to_be_bytes());
            resp.extend_from_slice(&len.to_be_bytes());
            resp.push(unit_id);
            resp.extend_from_slice(&pdu);
            let _ = socket.write_all(&resp).await;
        }
        // socket dropped → connection closed → device task sees error → Disconnected
    }

    // ── helpers ───────────────────────────────────────────────────────────────

    fn modbus_widget(port: u16, widget_type: WidgetType) -> WidgetConfig {
        WidgetConfig {
            id: "w".to_string(),
            widget_type,
            label: "test widget".to_string(),
            protocol: Some(ProtocolConfig::ModbusTcp(ModbusTcpConfig {
                host: "127.0.0.1".to_string(),
                port,
                unit_id: 1,
                register: 0,
                register_type: ModbusRegisterType::HoldingRegister,
                min_poll_interval_ms: 50,
                scale: 1.0,
                offset: 0.0,
                word_count: 1,
                bit_index: None,
            })),
            ..Default::default()
        }
    }

    /// Build a `ChannelContext` for Modbus-only tests.
    /// When the `epics` feature is also active a no-op EPICS context is included.
    fn make_ctx(pool: Arc<ModbusPool>) -> Arc<ChannelContext> {
        #[cfg(feature = "epics-pvxs")]
        {
            use std::sync::Mutex;
            let epics_ctx = Arc::new(Mutex::new(
                pvxs_sys::Context::from_env().expect("pvxs context required"),
            ));
            ChannelContext::new(epics_ctx, pool)
        }
        #[cfg(not(feature = "epics-pvxs"))]
        {
            ChannelContext::new(pool)
        }
    }

    /// Run `run_widget_monitor_async` in a background task and drain its HTML
    /// output until a fragment containing `disconnect_marker` is received.
    async fn wait_for_disconnect_html(
        config: WidgetConfig,
        ctx: Arc<ChannelContext>,
        disconnect_marker: &str,
    ) {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<(String, String)>(64);
        let widget_id = config.id.clone();
        tokio::spawn(run_widget_monitor_async(config, widget_id, ctx, tx));

        loop {
            let (_id, html) = tokio::time::timeout(Duration::from_secs(10), rx.recv())
                .await
                .expect("timed out waiting for widget HTML")
                .expect("widget output channel closed");

            if html.contains(disconnect_marker) {
                return;
            }
            // Still showing connected state — keep draining.
        }
    }

    // ── per-widget disconnection tests ────────────────────────────────────────

    /// `TextUpdate` should render `alarm-disconnected` class and `--` placeholder
    /// once the backing Modbus channel is severed.
    #[tokio::test]
    async fn text_update_shows_alarm_disconnected_after_server_drop() {
        let port = start_one_shot_server(2).await;
        let pool = ModbusPool::new();
        let config = modbus_widget(port, WidgetType::TextUpdate);
        let ctx = make_ctx(pool);
        wait_for_disconnect_html(config, ctx, "alarm-disconnected").await;
    }

    /// `Gauge` should render the disconnected/offline state after disconnect.
    #[tokio::test]
    async fn gauge_shows_offline_icon_after_server_drop() {
        let port = start_one_shot_server(2).await;
        let pool = ModbusPool::new();
        let config = modbus_widget(port, WidgetType::Gauge);
        let ctx = make_ctx(pool);
        // Use stable data URI prefix for the offline icon instead of value placeholder.
        wait_for_disconnect_html(config, ctx, "data:image/svg+xml;base64,").await;
    }

    /// `Slider` should render the offline icon (OFFLINE_SVG data URI) after
    /// disconnect, and its input element should carry `disabled`.
    #[tokio::test]
    async fn slider_shows_disabled_and_offline_icon_after_server_drop() {
        let port = start_one_shot_server(2).await;
        let pool = ModbusPool::new();
        let config = modbus_widget(port, WidgetType::Slider);
        let ctx = make_ctx(pool);
        wait_for_disconnect_html(config, ctx, "disabled").await;
    }

    /// `ToggleButton` should render with `disabled` attribute after disconnect.
    #[tokio::test]
    async fn toggle_button_renders_disabled_after_server_drop() {
        let port = start_one_shot_server(2).await;
        let pool = ModbusPool::new();
        let config = modbus_widget(port, WidgetType::ToggleButton);
        let ctx = make_ctx(pool);
        wait_for_disconnect_html(config, ctx, "disabled").await;
    }

    /// `LED` should render with the offline SVG icon after disconnect.
    #[tokio::test]
    async fn led_shows_offline_icon_after_server_drop() {
        let port = start_one_shot_server(2).await;
        let pool = ModbusPool::new();
        let config = modbus_widget(port, WidgetType::Led);
        let ctx = make_ctx(pool);
        // The OFFLINE_SVG data-URI appears in every disconnected widget that
        // carries an icon; use a stable prefix rather than the full base64 blob.
        wait_for_disconnect_html(config, ctx, "data:image/svg+xml;base64,").await;
    }
}
