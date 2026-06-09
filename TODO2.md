# Rust-Only Learning Guide: Toggle Button Countdown

Goal: render a live countdown at the button bottom-right corner for ToggleButton
widgets with non-zero reset_timeout, without adding any JavaScript.

This version is fully server-driven: Rust computes each countdown tick and pushes
updated HTML through SSE.

## 1. How the feature is split across layers

1. Config layer: WidgetConfig carries reset_timeout/reset_default.
2. Write path: app.rs schedules delayed reset write in background.
3. Stream path: toggle_button.rs emits HTML snapshots over SSE.
4. View layer: style.css positions the countdown text in button corner.

Important architecture point:
The delayed reset write and the visual countdown are independent.
- app.rs decides when to write reset_default.
- toggle_button.rs decides how to render countdown ticks every second.

## 2. Config and reset behavior

In src/config.rs, reset_timeout is Option<u64> (milliseconds).
In src/app.rs, maybe_schedule_toggle_reset:
- exits unless widget_type == ToggleButton
- exits unless reset_timeout > 0
- waits timeout_ms
- writes reset_default (or 0)

This function should not try to push UI ticks directly, because it has no widget
SSE tx sender in scope. UI ticking belongs inside the widget monitor stream.

## 3. Where the countdown should be rendered

File: src/widgets/toggle_button.rs

render_toggle_html now accepts countdown_secs: Option<u64>.
If countdown_secs is Some(n), it renders:

```rust
span class="widget-toggle-btn-countdown" { (format!("{}s", n)) }
```

If countdown_secs is None, no countdown node is rendered.

This keeps rendering declarative: markup is always a pure function of state.

## 4. Rust-only ticking model in run_monitor_async

The monitor keeps two extra pieces of state:
- countdown_end: Option<Instant>
- last_value: Option<ChannelValue>

Event handling:
1. ChannelEvent::Value(cv)
- If ON and reset_timeout > 0: set countdown_end = now + timeout.
- If OFF: clear countdown_end.
- Render immediately with current countdown value.

2. Tick event (every 1 second while countdown_end exists)
- Recompute remaining seconds from countdown_end - now.
- Re-render same ON value with updated countdown.

3. Disconnected/Error
- Clear countdown and render disconnected variant.

Concurrency pattern:
Use tokio::select! to wait on either:
- next channel event
- 1-second timer event

This is the key to Rust-only countdown updates.

## 5. Why this works without JavaScript

The browser still uses htmx SSE swap, but receives fully rendered HTML frames
from Rust every second during countdown.

So the visible sequence is:
- ON: 5s
- ON: 4s
- ON: 3s
- ...
- reset write fires in app.rs
- channel goes OFF
- OFF render arrives (countdown element disappears)

No client timers are required.

## 6. CSS requirements

File: static/style.css

Ensure:
1. .widget-toggle-btn has position: relative
2. .widget-toggle-btn-countdown is absolute in bottom-right
3. pointer-events: none so it never blocks button clicks

## 7. Tests to keep

File: tests/test_widget_toggle_button.rs

Keep existing behavior tests and add:
1. Countdown renders for ON + reset_timeout > 0.
2. Countdown does not render for OFF.

Unit tests validate markup rules. Integration timing behavior is naturally
covered during manual run with demo config.

## 8. End-to-end mental model

1. Click toggle ON.
2. app.rs schedules delayed reset write.
3. toggle_button monitor starts countdown_end and emits updated HTML every second.
4. At timeout, app.rs writes reset_default.
5. New channel value OFF arrives.
6. SSE pushes OFF HTML, countdown disappears.

This gives you deterministic, server-authoritative UI updates and keeps all
countdown logic in Rust.
