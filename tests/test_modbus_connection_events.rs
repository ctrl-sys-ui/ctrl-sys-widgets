#![cfg(feature = "modbus")]

mod test_modbus_connection_events {
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio_stream::StreamExt;

    use mycela::channel::ChannelEvent;
    use mycela::config::{ModbusTCPConfig, ModbusRegisterType, ProtocolConfig, WidgetConfig, WidgetType};
    use mycela::modbus_client::{modbus_stream, ModbusPool};

    // ── in-process mock Modbus TCP server ─────────────────────────────────────

    /// Binds to an OS-assigned port and loops forever accepting connections,
    /// serving every FC=0x03 read with a fixed register value of 1234.
    /// Abort the returned `JoinHandle` to stop the server.
    async fn start_mock_server() -> (u16, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = tokio::spawn(async move {
            loop {
                if let Ok((socket, _)) = listener.accept().await {
                    tokio::spawn(serve_modbus(socket, None));
                }
            }
        });
        (port, handle)
    }

    /// Binds to an OS-assigned port, accepts exactly ONE connection, serves at
    /// most `max_requests` reads on it, then drops the socket (closing the
    /// connection) and the listener (so reconnect attempts fail immediately).
    async fn start_one_shot_server(max_requests: usize) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            if let Ok((socket, _)) = listener.accept().await {
                serve_modbus(socket, Some(max_requests)).await;
            }
            // listener dropped here — subsequent connect attempts get "connection refused"
        });
        port
    }

    /// Respond to FC=0x03 (Read Holding Registers) requests with value 1234.
    /// If `max` is `Some(n)`, exits after serving `n` requests (dropping the socket).
    async fn serve_modbus(mut socket: TcpStream, max: Option<usize>) {
        let mut buf = vec![0u8; 256];
        let mut served = 0usize;
        loop {
            if max.is_some_and(|m| served >= m) {
                break;
            }
            let n = match socket.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            if n < 12 {
                continue;
            }
            let txn_id  = u16::from_be_bytes([buf[0], buf[1]]);
            let unit_id = buf[6];
            let fc      = buf[7];
            let count   = u16::from_be_bytes([buf[10], buf[11]]);

            let pdu: Vec<u8> = if fc == 0x03 {
                let bc = (count * 2) as u8;
                let mut p = vec![0x03, bc];
                for _ in 0..count {
                    p.extend_from_slice(&1234u16.to_be_bytes());
                }
                p
            } else {
                vec![fc | 0x80, 0x01] // Modbus exception for any other FC
            };

            let len = (1 + pdu.len()) as u16;
            let mut resp = Vec::new();
            resp.extend_from_slice(&txn_id.to_be_bytes());
            resp.extend_from_slice(&0u16.to_be_bytes()); // protocol id
            resp.extend_from_slice(&len.to_be_bytes());
            resp.push(unit_id);
            resp.extend_from_slice(&pdu);

            if socket.write_all(&resp).await.is_err() {
                break;
            }
            served += 1;
        }
    }

    // ── helpers ───────────────────────────────────────────────────────────────

    fn widget_cfg(port: u16) -> Arc<WidgetConfig> {
        Arc::new(WidgetConfig {
            id: "mb-test".to_string(),
            widget_type: WidgetType::TextUpdate,
            label: "test".to_string(),
            protocol: Some(ProtocolConfig::ModbusTcp(ModbusTCPConfig {
                host: "127.0.0.1".to_string(),
                port,
                unit_id: 1,
                register: 1000,
                register_type: ModbusRegisterType::HoldingRegister,
                min_poll_interval_ms: 50,
                scale: 1.0,
                offset: 0.0,
                word_count: 1,
                bit_index: None,
            })),
            ..Default::default()
        })
    }

    async fn next_event(
        stream: &mut (impl tokio_stream::Stream<Item = ChannelEvent> + Unpin),
    ) -> ChannelEvent {
        tokio::time::timeout(Duration::from_secs(10), stream.next())
            .await
            .expect("timed out waiting for channel event")
            .expect("stream ended unexpectedly")
    }

    // ── pool unit tests ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn pool_get_or_create_deduplicates_same_key() {
        let pool = ModbusPool::new();
        let h1 = pool.get_or_create("127.0.0.1", 9901, 1);
        let h2 = pool.get_or_create("127.0.0.1", 9901, 1);
        assert!(Arc::ptr_eq(&h1, &h2), "same key should return the same Arc");
    }

    #[tokio::test]
    async fn pool_different_keys_return_different_handles() {
        let pool = ModbusPool::new();
        let h1 = pool.get_or_create("127.0.0.1", 9902, 1);
        let h2 = pool.get_or_create("127.0.0.1", 9903, 1);
        assert!(!Arc::ptr_eq(&h1, &h2), "different keys should return distinct Arcs");
    }

    #[tokio::test]
    async fn pool_handle_not_closed_when_newly_created() {
        let pool = ModbusPool::new();
        let h = pool.get_or_create("127.0.0.1", 9904, 1);
        assert!(!h.is_closed(), "freshly spawned device task should be running");
    }

    #[tokio::test]
    async fn pool_handle_is_closed_after_disconnect_all() {
        let pool = ModbusPool::new();
        let h = pool.get_or_create("127.0.0.1", 9905, 1);
        assert!(!h.is_closed());

        pool.disconnect_all();
        // Yield to let the tokio runtime process the abort.
        tokio::task::yield_now().await;

        assert!(h.is_closed(), "handle sender should be closed after disconnect_all");
    }

    #[tokio::test]
    async fn pool_get_or_create_returns_fresh_open_handle_after_disconnect_all() {
        let pool = ModbusPool::new();
        let old = pool.get_or_create("127.0.0.1", 9906, 1);
        pool.disconnect_all();
        tokio::task::yield_now().await;

        let new = pool.get_or_create("127.0.0.1", 9906, 1);

        assert!(old.is_closed(), "old handle should be closed");
        assert!(!new.is_closed(), "new handle should have a live task");
        assert!(!Arc::ptr_eq(&old, &new), "should be different Arc instances");
    }

    // ── stream event tests ────────────────────────────────────────────────────

    #[tokio::test]
    async fn stream_emits_connected_then_value_when_server_is_up() {
        let (port, _server) = start_mock_server().await;
        let pool   = ModbusPool::new();
        let config = widget_cfg(port);
        let mut stream = Box::pin(modbus_stream(config, pool));

        let ev = next_event(&mut stream).await;
        assert!(
            matches!(ev, ChannelEvent::Connected),
            "first event should be Connected, got {:?}",
            ev
        );

        let ev = next_event(&mut stream).await;
        assert!(
            matches!(ev, ChannelEvent::Value(_)),
            "second event should be Value, got {:?}",
            ev
        );
    }

    #[tokio::test]
    async fn stream_emits_disconnected_after_server_closes_connection() {
        // Server serves 2 requests then closes the socket and stops listening.
        let port = start_one_shot_server(2).await;
        let pool = ModbusPool::new();
        let config = widget_cfg(port);
        let mut stream = Box::pin(modbus_stream(config, pool));

        // Wait for the initial Connected + at least one Value.
        assert!(matches!(next_event(&mut stream).await, ChannelEvent::Connected));
        assert!(matches!(next_event(&mut stream).await, ChannelEvent::Value(_)));

        // Drain until Disconnected; skip any extra Value events that arrive before
        // the device task detects the broken connection.
        loop {
            match next_event(&mut stream).await {
                ChannelEvent::Disconnected(_) => break,
                ChannelEvent::Value(_)        => continue,
                other => panic!("unexpected event while waiting for Disconnected: {:?}", other),
            }
        }
    }
}
