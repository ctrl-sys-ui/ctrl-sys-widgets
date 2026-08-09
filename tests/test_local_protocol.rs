mod test_local_protocol {
    use std::sync::Arc;
    use std::time::Duration;

    use mycela::channel::{channel_stream, ChannelContext, ChannelEvent};
    use mycela::config::{LocalConfig, ProtocolConfig, WidgetConfig, WidgetType};
    use mycela::widgets;
    use tokio_stream::StreamExt;

    fn local_widget(id: &str, channel: &str) -> WidgetConfig {
        WidgetConfig {
            id: id.to_string(),
            widget_type: WidgetType::TextUpdate,
            label: "Local Widget".to_string(),
            data_type: Some("double".to_string()),
            protocol: Some(ProtocolConfig::Local(LocalConfig {
                channel: channel.to_string(),
                initial_value: Some("1.5".to_string()),
            })),
            ..Default::default()
        }
    }

    fn channel_ctx() -> Arc<ChannelContext> {
        #[cfg(feature = "epics-pvxs")]
        let epics_ctx = Arc::new(std::sync::Mutex::new(
            mycela::pvxs::Context::from_env().expect("pvxs context required"),
        ));

        #[cfg(feature = "modbus")]
        let modbus_pool = mycela::modbus_client::ModbusPool::new();

        #[cfg(all(feature = "epics-pvxs", feature = "modbus"))]
        {
            return ChannelContext::new(epics_ctx, modbus_pool);
        }

        #[cfg(all(feature = "epics-pvxs", not(feature = "modbus")))]
        {
            return ChannelContext::new(epics_ctx);
        }

        #[cfg(all(not(feature = "epics-pvxs"), feature = "modbus"))]
        {
            return ChannelContext::new(modbus_pool);
        }

        #[cfg(all(not(feature = "epics-pvxs"), not(feature = "modbus")))]
        {
            ChannelContext::new()
        }
    }

    async fn next_event(
        stream: &mut std::pin::Pin<Box<dyn tokio_stream::Stream<Item = ChannelEvent> + Send>>,
    ) -> ChannelEvent {
        tokio::time::timeout(Duration::from_secs(2), stream.next())
            .await
            .expect("timed out waiting for local event")
            .expect("local stream ended unexpectedly")
    }

    #[tokio::test]
    async fn local_stream_emits_connected_then_seed_value() {
        let ctx = channel_ctx();
        let widget = local_widget("local-seed", "app:seed");

        let mut stream = Box::pin(channel_stream(Arc::new(widget), ctx));

        match next_event(&mut stream).await {
            ChannelEvent::Connected => {}
            other => panic!("expected Connected, got {:?}", other),
        }

        match next_event(&mut stream).await {
            ChannelEvent::Value(cv) => {
                assert_eq!(cv.value_str, "1.50");
                assert_eq!(cv.raw_value, 1.5);
            }
            other => panic!("expected Value, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn local_write_publishes_to_subscribers() {
        let ctx = channel_ctx();
        let widget = local_widget("local-write", "app:setpoint");

        let mut stream = Box::pin(channel_stream(Arc::new(widget.clone()), ctx.clone()));

        let _ = next_event(&mut stream).await;
        let _ = next_event(&mut stream).await;

        let result = widgets::write_channel(widget, "42".to_string(), ctx).await;
        assert!(result.into_string().contains("write-ok"));

        match next_event(&mut stream).await {
            ChannelEvent::Value(cv) => {
                assert_eq!(cv.value_str, "42.00");
                assert_eq!(cv.raw_value, 42.0);
            }
            other => panic!("expected Value after local write, got {:?}", other),
        }
    }
}
