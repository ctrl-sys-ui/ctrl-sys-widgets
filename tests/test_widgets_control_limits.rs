mod test_widgets_control_limits {
    use mycela::config::{ControlMetadata, PvMetadata, WidgetConfig, WidgetType};
    use mycela::widgets::check_control_limits;

    fn widget_with_limits(low: f64, high: f64) -> WidgetConfig {
        WidgetConfig {
            id: "w".to_string(),
            widget_type: WidgetType::TextEntry,
            label: "w label".to_string(),
            metadata: Some(PvMetadata {
                display: None,
                control: Some(ControlMetadata {
                    limit_low: low,
                    limit_high: high,
                    min_step: 0.0,
                }),
                alarm: None,
            }),
            ..Default::default()
        }
    }

    fn widget_no_protocol() -> WidgetConfig {
        WidgetConfig {
            id: "w".to_string(),
            widget_type: WidgetType::TextEntry,
            label: "w label".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_value_above_high_control_limit_is_rejected_with_error_markup() {
        let html = check_control_limits(&widget_with_limits(0.0, 100.0), "1300.0")
            .expect("expected Some(err)")
            .into_string();
        assert!(html.contains("write-err"), "got: {html}");
        assert!(html.contains("1300"), "should include the value, got: {html}");
    }

    #[test]
    fn test_value_below_low_control_limit_is_rejected_with_error_markup() {
        let html = check_control_limits(&widget_with_limits(0.0, 100.0), "-5.0")
            .expect("expected Some(err)")
            .into_string();
        assert!(html.contains("write-err"), "got: {html}");
    }

    #[test]
    fn test_value_within_control_limits_returns_none_allowing_write() {
        assert!(check_control_limits(&widget_with_limits(0.0, 100.0), "50.0").is_none());
    }

    #[test]
    fn test_value_at_exact_low_control_limit_is_accepted() {
        assert!(check_control_limits(&widget_with_limits(0.0, 100.0), "0.0").is_none());
    }

    #[test]
    fn test_value_at_exact_high_control_limit_is_accepted() {
        assert!(check_control_limits(&widget_with_limits(0.0, 100.0), "100.0").is_none());
    }

    #[test]
    fn test_widget_without_metadata_skips_control_limit_check() {
        let w = widget_no_protocol();
        assert!(check_control_limits(&w, "999999.0").is_none());
    }

    #[test]
    fn test_non_numeric_value_string_bypasses_control_limit_check() {
        let html_opt = check_control_limits(&widget_with_limits(0.0, 1.0), "true");
        assert!(html_opt.is_none(), "non-numeric should bypass limit check");
    }
}
