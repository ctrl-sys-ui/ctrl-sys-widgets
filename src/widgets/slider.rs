use maud::{html, Markup};
use std::sync::Arc;
use futures::StreamExt;
use crate::channel::{ChannelContext, ChannelEvent, ChannelValue};
use crate::config::WidgetConfig;

pub struct Slider {
    config: WidgetConfig,
}

impl Slider {
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
            yield Ok(axum::response::sse::Event::default().data(
                render_inner_disconnected(&config).into_string()
            ));
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
        let mut enabled_rx = ctx.subscribe_widget_enabled(&config.id);
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
        let mut last_value: Option<ChannelValue> = None;
        let mut last_html = String::new();

        while let Some(event) = stream.next().await {
            let html = match event {
                ChannelEvent::Value(cv) => {
                    ctx_clone.set_widget_connected(&widget_id, true);
                    let enabled = *enabled_rx.borrow();
                    last_value = Some(cv.clone());
                    render_inner_connected(&config, &cv, enabled).into_string()
                }
                ChannelEvent::Disconnected(_) | ChannelEvent::Error(_) => {
                    ctx_clone.set_widget_connected(&widget_id, false);
                    render_inner_disconnected_with_last(&config, last_value.as_ref()).into_string()
                }
                ChannelEvent::Connected => {
                    ctx_clone.set_widget_connected(&widget_id, true);
                    continue;
                }
            };
            if html != last_html {
                last_html = html.clone();
                if tx.send(html).is_err() {
                    ctx_clone.set_widget_connected(&widget_id, false);
                    break;
                }
            }

            while enabled_rx.has_changed().unwrap_or(false) {
                if enabled_rx.changed().await.is_err() {
                    break;
                }
                let html = if ctx_clone.is_widget_connected(&widget_id) {
                    match &last_value {
                        Some(cv) => {
                            render_inner_connected(&config, cv, *enabled_rx.borrow()).into_string()
                        }
                        None => render_inner_disconnected(&config).into_string(),
                    }
                } else {
                    render_inner_disconnected_with_last(&config, last_value.as_ref()).into_string()
                };
                if html != last_html {
                    last_html = html.clone();
                    if tx.send(html).is_err() {
                        ctx_clone.set_widget_connected(&widget_id, false);
                        break;
                    }
                }
            }
        }
    }
}

pub fn render_inner_connected(config: &WidgetConfig, cv: &ChannelValue, enabled: bool) -> Markup {
    let alarm_class = super::alarm_severity_class(cv.alarm_severity);
    let icon: Option<&str> = match cv.alarm_severity {
        1 => Some(super::MINOR_ALARM_SVG),
        2 => Some(super::MAJOR_ALARM_SVG),
        3 => Some(super::INVALID_SVG),
        _ => None,
    };
    let display_value = cv.value_str.clone();
    let min = cv.control_low;
    let max = if (cv.control_high - cv.control_low).abs() < f64::EPSILON {
        cv.control_low + 100.0
    } else {
        cv.control_high
    };
    let precision_step = 10f64.powi(-(cv.precision as i32).max(0));
    let step = config
        .metadata
        .as_ref()
        .and_then(|m| m.control.as_ref())
        .map(|c| c.min_step)
        .filter(|&s| s > 0.0)
        .unwrap_or(precision_step);
    render_slider_html(
        config,
        cv.raw_value,
        &display_value,
        &cv.units,
        min,
        max,
        step,
        alarm_class,
        icon,
        !enabled,
        &super::tooltips::build_tooltip(config, cv),
    )
}

pub fn render_inner_disconnected(config: &WidgetConfig) -> Markup {
    render_inner_disconnected_with_last(config, None)
}

fn render_inner_disconnected_with_last(config: &WidgetConfig, last: Option<&ChannelValue>) -> Markup {
    if let Some(cv) = last {
        let min = cv.control_low;
        let max = if (cv.control_high - cv.control_low).abs() < f64::EPSILON {
            cv.control_low + 100.0
        } else {
            cv.control_high
        };
        let precision_step = 10f64.powi(-(cv.precision as i32).max(0));
        let step = config
            .metadata
            .as_ref()
            .and_then(|m| m.control.as_ref())
            .map(|c| c.min_step)
            .filter(|&s| s > 0.0)
            .unwrap_or(precision_step);

        return render_slider_html(
            config,
            cv.raw_value,
            &cv.value_str,
            &cv.units,
            min,
            max,
            step,
            "alarm-disconnected",
            Some(super::OFFLINE_SVG),
            true,
            "",
        );
    }

    render_slider_html(
        config,
        0.0,
        "--",
        "",
        0.0,
        100.0,
        0.1,
        "alarm-disconnected",
        Some(super::OFFLINE_SVG),
        true,
        "",
    )
}

fn render_slider_html(
    config: &WidgetConfig,
    current_value: f64,
    display_value: &str,
    units: &str,
    min: f64,
    max: f64,
    step: f64,
    alarm_class: &str,
    icon: Option<&str>,
    disabled: bool,
    tooltip: &str,
) -> Markup {
    // Slider drag, update number input (display only, no submit)
    let slider_oninput = "this.nextElementSibling.value=this.value";
    // Enter on slider, update number input value then fire keyup so HTMX picks it up
    let slider_onkeyup = "if(event.key==='Enter'){let ni=this.nextElementSibling;ni.value=this.value;ni.dispatchEvent(new KeyboardEvent('keyup',{key:'Enter',bubbles:true}));}";
    // Typing in number input, update slider position
    let numentry_oninput = "let r=this.previousElementSibling;let v=parseFloat(this.value);if(!isNaN(v)&&Number(v)>=Number(r.min)&&Number(v)<=Number(r.max))r.value=v";
    // When focus leaves the container, reset both controls to the last confirmed value
    let container_focusout = "setTimeout((c=>()=>{if(!c.contains(document.activeElement)){let ni=c.querySelector('.slider-text-entry');let v=ni.dataset.confirmed;ni.value=v;c.querySelector('input[type=range]').value=v;}})(this),0)";
    // After a successful post, store the new confirmed value and sync the slider
    let after_request = "this.dataset.confirmed=this.value;this.previousElementSibling.value=this.value";
    // Before posting, reject non-finite values and reset to confirmed
    let before_request = "if(isNaN(parseFloat(this.value))||!isFinite(this.value)){this.value=this.dataset.confirmed;this.previousElementSibling.value=this.dataset.confirmed;event.preventDefault();return false;}";

    html! {
        div class="widget-inner" data-widget-enabled=(if disabled { "false" } else { "true" }) {
            div class="slider-container" onfocusout=(container_focusout) {
                input type="range"
                    class="widget-slider"
                    name="value"
                    min=(format!("{}", min))
                    max=(format!("{}", max))
                    step=(format!("{}", step))
                    value=(format!("{}", current_value))
                    disabled[disabled]
                    oninput=(slider_oninput)
                    onkeyup=(slider_onkeyup);
                input type="number"
                    class=(format!("slider-text-entry {}", alarm_class))
                    name="value"
                    min=(format!("{}", min))
                    max=(format!("{}", max))
                    step=(format!("{}", step))
                    value=(display_value)
                    data-confirmed=(display_value)
                    disabled[disabled]
                    hx-post={"/api/widget/" (config.id) "/set"}
                    hx-trigger="keyup[key=='Enter']"
                    hx-target="next .status"
                    hx-swap="innerHTML"
                    hx-on--before-request=(before_request)
                    hx-on--after-request=(after_request)
                    oninput=(numentry_oninput);
                @if !units.is_empty() {
                    span class="slider-units" { (units) }
                }
            }
            span class="status" {}
            label class="widget-label" {
                (config.label)
                @if let Some(src) = icon {
                    img class="widget-status-icon" src=(src) alt="status";
                }
                @if !tooltip.is_empty() {
                    (super::tooltips::render_tooltip_info_btn(tooltip))
                }
            }
        }
    }
}

pub fn render_slider(widget: &WidgetConfig) -> Markup {
    html! {
        div style=[super::widget_container_style(widget)]
            data-widget-id=(widget.id)
            data-ch=(widget.channel_address())
            data-widget-enabled="false"
            hx-sse=(format!("swap:{}", widget.id)) {
            (render_inner_disconnected(widget))
        }
    }
}
