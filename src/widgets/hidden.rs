use crate::channel::{ChannelContext, ChannelEvent};
use crate::config::WidgetConfig;
use futures::StreamExt;
use maud::{html, Markup};
use std::sync::Arc;

pub struct Hidden {
    config: WidgetConfig,
}

impl Hidden {
    pub fn new(config: WidgetConfig) -> Self {
        Self { config }
    }

    pub fn into_sse_stream(
        self,
        ctx: Arc<ChannelContext>,
    ) -> impl tokio_stream::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>
           + Send
           + 'static {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let config = Arc::new(self.config);

        tokio::spawn(Self::run_monitor_async(config.clone(), ctx, tx));

        async_stream::stream! {
            let mut rx = rx;
            while let Some(html) = rx.recv().await {
                yield Ok(axum::response::sse::Event::default().data(html));
            }
        }
    }

    pub(crate) async fn run_monitor_async(
        config: Arc<WidgetConfig>,
        ctx: Arc<ChannelContext>,
        tx: tokio::sync::mpsc::UnboundedSender<String>,
    ) {
        let ctx_clone = ctx.clone();
        let widget_id = config.id.clone();
        let ctx_publish = ctx_clone.clone();
        let widget_id_publish = widget_id.clone();
        let mut stream = crate::channel::channel_stream(config.clone(), ctx)
            .inspect(move |e| {
                if let ChannelEvent::Value(cv) = e {
                    ctx_publish.publish_widget_value(&widget_id_publish, cv.clone());
                }
            });

        while let Some(event) = stream.next().await {
            match event {
                ChannelEvent::Connected => continue,
                ChannelEvent::Value(_) | ChannelEvent::Disconnected(_) | ChannelEvent::Error(_) => {
                    if tx.send(render_hidden(&config).into_string()).is_err() {
                        ctx_clone.set_widget_connected(&widget_id, false);
                        break;
                    }
                }
            }
        }
    }
}

pub fn render_hidden(_widget: &WidgetConfig) -> Markup {
    // Intentionally render no DOM. Hidden widgets still run monitors and publish values.
    html! {}
}
