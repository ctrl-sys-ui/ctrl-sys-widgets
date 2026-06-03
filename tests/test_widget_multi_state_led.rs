mod test_widget_multi_state_led {
    use mycela::channel::ChannelValue;
    use mycela::config::{WidgetConfig, WidgetType};
    use mycela::widgets::multi_state_led::{render_inner_connected, render_inner_pending};
    use mycela::widgets::{MAJOR_ALARM_SVG, MINOR_ALARM_SVG, OFFLINE_SVG};

    fn w() -> WidgetConfig {
        WidgetConfig {
            id: "msl".to_string(),
            widget_type: WidgetType::MultiStateLed,
            label: "Valve".to_string(),
            ..Default::default()
        }
    }

    fn w_inverted() -> WidgetConfig {
        WidgetConfig { invert: Some(true), ..w() }
    }

    fn w_bowtie() -> WidgetConfig {
        WidgetConfig {
            polygon_points: Some("0,0 0,18 15,9 30,18 30,0 15,9".to_string()),
            ..w()
        }
    }

    // ── pending / disconnected state ─────────────────────────────────────────

    #[test]
    fn test_pending_state_renders_vs_pending_class() {
        let html = render_inner_pending(&w()).into_string();
        assert!(html.contains("vs-pending"), "got: {html}");
    }

    #[test]
    fn test_pending_state_renders_double_dash_label() {
        let html = render_inner_pending(&w()).into_string();
        assert!(html.contains("--"), "got: {html}");
    }

    #[test]
    fn test_pending_state_renders_widget_label() {
        let html = render_inner_pending(&w()).into_string();
        assert!(html.contains("Valve"), "got: {html}");
    }

    // ── open state (raw_value = 1, invert = false) ───────────────────────────

    #[test]
    fn test_value_1_normal_renders_vs_open_class() {
        let cv = ChannelValue { raw_value: 1.0, ..ChannelValue::default() };
        let html = render_inner_connected(&w(), &cv).into_string();
        assert!(html.contains("vs-open"), "got: {html}");
    }

    #[test]
    fn test_value_1_normal_renders_open_label() {
        let cv = ChannelValue { raw_value: 1.0, ..ChannelValue::default() };
        let html = render_inner_connected(&w(), &cv).into_string();
        assert!(html.contains("OPEN"), "got: {html}");
    }

    // ── closed state (raw_value = 0, invert = false) ─────────────────────────

    #[test]
    fn test_value_0_normal_renders_vs_closed_class() {
        let cv = ChannelValue { raw_value: 0.0, ..ChannelValue::default() };
        let html = render_inner_connected(&w(), &cv).into_string();
        assert!(html.contains("vs-closed"), "got: {html}");
    }

    #[test]
    fn test_value_0_normal_renders_closed_label() {
        let cv = ChannelValue { raw_value: 0.0, ..ChannelValue::default() };
        let html = render_inner_connected(&w(), &cv).into_string();
        assert!(html.contains("CLOSED"), "got: {html}");
    }

    // ── inverted: open state (raw_value = 0, invert = true) ──────────────────

    #[test]
    fn test_value_0_inverted_renders_vs_open_class() {
        let cv = ChannelValue { raw_value: 0.0, ..ChannelValue::default() };
        let html = render_inner_connected(&w_inverted(), &cv).into_string();
        assert!(html.contains("vs-open"), "got: {html}");
    }

    #[test]
    fn test_value_0_inverted_renders_open_label() {
        let cv = ChannelValue { raw_value: 0.0, ..ChannelValue::default() };
        let html = render_inner_connected(&w_inverted(), &cv).into_string();
        assert!(html.contains("OPEN"), "got: {html}");
    }

    // ── inverted: closed state (raw_value = 1, invert = true) ────────────────

    #[test]
    fn test_value_1_inverted_renders_vs_closed_class() {
        let cv = ChannelValue { raw_value: 1.0, ..ChannelValue::default() };
        let html = render_inner_connected(&w_inverted(), &cv).into_string();
        assert!(html.contains("vs-closed"), "got: {html}");
    }

    #[test]
    fn test_value_1_inverted_renders_closed_label() {
        let cv = ChannelValue { raw_value: 1.0, ..ChannelValue::default() };
        let html = render_inner_connected(&w_inverted(), &cv).into_string();
        assert!(html.contains("CLOSED"), "got: {html}");
    }

    // ── unknown value (raw_value = 2) renders pending ─────────────────────────

    #[test]
    fn test_value_2_renders_vs_pending_class() {
        let cv = ChannelValue { raw_value: 2.0, ..ChannelValue::default() };
        let html = render_inner_connected(&w(), &cv).into_string();
        assert!(html.contains("vs-pending"), "got: {html}");
    }

    #[test]
    fn test_value_2_renders_double_dash_label() {
        let cv = ChannelValue { raw_value: 2.0, ..ChannelValue::default() };
        let html = render_inner_connected(&w(), &cv).into_string();
        assert!(html.contains("--"), "got: {html}");
    }

    // ── default polygon points ────────────────────────────────────────────────

    #[test]
    fn test_default_polygon_points_are_rectangle() {
        let cv = ChannelValue { raw_value: 1.0, ..ChannelValue::default() };
        let html = render_inner_connected(&w(), &cv).into_string();
        assert!(html.contains("0,0 30,0 30,18 0,18"), "got: {html}");
    }

    // ── custom bowtie polygon points ──────────────────────────────────────────

    #[test]
    fn test_custom_polygon_points_are_rendered() {
        let cv = ChannelValue { raw_value: 1.0, ..ChannelValue::default() };
        let html = render_inner_connected(&w_bowtie(), &cv).into_string();
        assert!(
            html.contains("0,0 0,18 15,9 30,18 30,0 15,9"),
            "got: {html}"
        );
    }

    #[test]
    fn test_default_polygon_points_not_used_when_custom_points_set() {
        let cv = ChannelValue { raw_value: 1.0, ..ChannelValue::default() };
        let html = render_inner_connected(&w_bowtie(), &cv).into_string();
        assert!(
            !html.contains("0,0 30,0 30,18 0,18"),
            "default rectangle should not appear when custom points set, got: {html}"
        );
    }

    // ── widget label is always rendered ──────────────────────────────────────

    #[test]
    fn test_widget_label_rendered_in_connected_state() {
        let cv = ChannelValue { raw_value: 1.0, ..ChannelValue::default() };
        let html = render_inner_connected(&w(), &cv).into_string();
        assert!(html.contains("Valve"), "got: {html}");
    }

    // ── SVG structure ─────────────────────────────────────────────────────────

    #[test]
    fn test_svg_polygon_element_is_present() {
        let cv = ChannelValue { raw_value: 0.0, ..ChannelValue::default() };
        let html = render_inner_connected(&w(), &cv).into_string();
        assert!(html.contains("<polygon"), "got: {html}");
    }

    #[test]
    fn test_vs_widget_class_is_present() {
        let html = render_inner_pending(&w()).into_string();
        assert!(html.contains("vs-widget"), "got: {html}");
    }

    // ── alarm and offline icons ───────────────────────────────────────────────

    #[test]
    fn test_pending_state_renders_offline_icon() {
        let html = render_inner_pending(&w()).into_string();
        assert!(html.contains(OFFLINE_SVG), "got: {html}");
    }

    #[test]
    fn test_minor_alarm_renders_minor_alarm_icon() {
        let cv = ChannelValue { raw_value: 1.0, alarm_severity: 1, ..ChannelValue::default() };
        let html = render_inner_connected(&w(), &cv).into_string();
        assert!(html.contains(MINOR_ALARM_SVG), "got: {html}");
    }

    #[test]
    fn test_major_alarm_renders_major_alarm_icon() {
        let cv = ChannelValue { raw_value: 1.0, alarm_severity: 2, ..ChannelValue::default() };
        let html = render_inner_connected(&w(), &cv).into_string();
        assert!(html.contains(MAJOR_ALARM_SVG), "got: {html}");
    }

    #[test]
    fn test_no_alarm_does_not_render_alarm_icon() {
        let cv = ChannelValue { raw_value: 1.0, alarm_severity: 0, ..ChannelValue::default() };
        let html = render_inner_connected(&w(), &cv).into_string();
        assert!(!html.contains(MINOR_ALARM_SVG), "unexpected minor alarm icon, got: {html}");
        assert!(!html.contains(MAJOR_ALARM_SVG), "unexpected major alarm icon, got: {html}");
    }
}
