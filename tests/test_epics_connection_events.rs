#![cfg(feature = "epics-pvxs")]

mod test_epics_connection_events {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tokio_stream::StreamExt;

    use mycela::channel::ChannelEvent;
    use mycela::config::{EpicsPvxsConfig, ProtocolConfig, WidgetConfig, WidgetType};
    use mycela::epics_channel::epics_stream;

    fn epics_widget(pv_name: &str) -> Arc<WidgetConfig> {
        Arc::new(WidgetConfig {
            id: "epics-test".to_string(),
            widget_type: WidgetType::TextUpdate,
            label: "EPICS test".to_string(),
            protocol: Some(ProtocolConfig::EpicsPvxs(EpicsPvxsConfig {
                pv_name: pv_name.to_string(),
                server: None,
                pv_names: None,
            })),
            ..Default::default()
        })
    }

    /// Widget with no protocol — routes to `epics_stream` via `channel_stream`
    /// but has no PV name, so `run_single_monitor` emits `ChannelEvent::Error`.
    fn no_protocol_widget() -> Arc<WidgetConfig> {
        Arc::new(WidgetConfig {
            id: "no-proto".to_string(),
            widget_type: WidgetType::TextUpdate,
            label: "no protocol".to_string(),
            ..Default::default()
        })
    }

    async fn next_event(
        stream: &mut (impl tokio_stream::Stream<Item = ChannelEvent> + Unpin),
    ) -> ChannelEvent {
        tokio::time::timeout(Duration::from_secs(10), stream.next())
            .await
            .expect("timed out waiting for EPICS channel event")
            .expect("stream ended unexpectedly")
    }

    async fn stop_server_with_timeout(server: pvxs::Server, timeout: Duration) {
        let (result_tx, result_rx) = tokio::sync::oneshot::channel();

        // A timed-out spawn_blocking task keeps running and can hold the Tokio
        // blocking pool open. A detached OS thread lets the test fail promptly.
        std::thread::spawn(move || {
            let _ = result_tx.send(server.stop_drop());
        });

        match tokio::time::timeout(timeout, result_rx).await {
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(error))) => panic!("PVXS server stop failed: {error}"),
            Ok(Err(_)) => panic!("PVXS server stop thread exited without a result"),
            Err(_) => panic!("timed out stopping PVXS server after {timeout:?}"),
        }
    }

    // ── error-path test (no network required) ─────────────────────────────────

    /// When a widget has no `EpicsPva` protocol config, `run_single_monitor`
    /// emits an `Error` event immediately and exits — no IOC connection needed.
    #[tokio::test]
    async fn epics_stream_emits_error_for_widget_without_epics_protocol() {
        let ctx = Arc::new(Mutex::new(pvxs::Context::from_env().unwrap()));
        let config = no_protocol_widget();
        let mut stream = Box::pin(epics_stream(config, ctx));

        let ev = next_event(&mut stream).await;
        assert!(
            matches!(ev, ChannelEvent::Error(_)),
            "expected Error for widget without an EpicsPva protocol, got {:?}",
            ev
        );
    }

    // ── integration tests (require in-process PVXS server) ───────────────────
    //
    // These tests start a local PVXS server, create a test PV, and verify that
    // the monitor stream fires the expected Connected / Disconnected events.
    //
    // They are marked `#[ignore]` because the PVXS connection-check timeout can
    // make the Disconnected test take up to ~30 s depending on EPICS env config.
    // Run them explicitly with:
    //
    //   cargo test --features epics -- --ignored

    /// Verify that `Connected` (and then `Value`) events arrive when the monitored
    /// PV is served by a local PVXS server started in the same process.
    #[tokio::test]
    #[ignore = "starts a local PVXS server; may be slow — run with --ignored"]
    async fn epics_stream_emits_connected_when_pv_is_served_locally() {
        let server = tokio::task::spawn_blocking(|| pvxs::Server::start_from_env())
            .await
            .unwrap()
            .expect("could not start local PVXS server");

        server
            .create_pv_double(
                "test:mycela:connected",
                42.0,
                pvxs::NTScalarMetadataBuilder::new(),
            )
            .expect("could not create test PV");

        // Allow the server to fully advertise the PV before the client connects.
        tokio::time::sleep(Duration::from_millis(250)).await;

        let ctx = Arc::new(Mutex::new(pvxs::Context::from_env().unwrap()));
        let config = epics_widget("test:mycela:connected");
        let mut stream = Box::pin(epics_stream(config, ctx));

        // The monitor fires `Connected` before the first value snapshot.
        let ev = next_event(&mut stream).await;
        assert!(
            matches!(ev, ChannelEvent::Connected | ChannelEvent::Value(_)),
            "expected Connected or Value from local PVXS server, got {:?}",
            ev
        );
    }

    /// Verify that `Disconnected` is emitted after the PVXS server that serves
    /// the monitored PV is stopped.
    #[tokio::test]
    #[ignore = "starts a local PVXS server and waits for PVA disconnect detection; may be slow — run with --ignored"]
    async fn epics_stream_emits_disconnected_after_server_stops() {
        let server = tokio::task::spawn_blocking(|| pvxs::Server::start_from_env())
            .await
            .unwrap()
            .expect("could not start local PVXS server");

        server
            .create_pv_double(
                "test:mycela:disconnect",
                1.0,
                pvxs::NTScalarMetadataBuilder::new(),
            )
            .expect("could not create test PV");

        tokio::time::sleep(Duration::from_millis(250)).await;

        let ctx = Arc::new(Mutex::new(pvxs::Context::from_env().unwrap()));
        let config = epics_widget("test:mycela:disconnect");
        let mut stream = Box::pin(epics_stream(config, ctx));

        // Wait for Connected (or Value — whichever arrives first).
        loop {
            match next_event(&mut stream).await {
                ChannelEvent::Connected | ChannelEvent::Value(_) => break,
                ChannelEvent::Error(e) => panic!("unexpected Error before Connected: {e}"),
                ChannelEvent::Disconnected(_) => panic!("unexpected Disconnected before Connected"),
            }
        }

        // Stop the server — the PVA client should detect the broken connection.
        stop_server_with_timeout(server, Duration::from_secs(5)).await;

        // Use one deadline for the whole phase. A timeout around each event can
        // run forever if the stream keeps producing Value events.
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let ev = stream
                    .next()
                    .await
                    .expect("stream ended before Disconnected");

                match ev {
                    ChannelEvent::Disconnected(_) => break,
                    ChannelEvent::Value(_) | ChannelEvent::Connected => continue,
                    ChannelEvent::Error(e) => panic!("unexpected Error: {e}"),
                }
            }
        })
        .await
        .expect("timed out waiting for Disconnected after server stop");
    }
}
