// Copyright 2026 Tine Zata
// SPDX-License-Identifier: MPL-2.0

mod test_widgets_container_style {
    use mycela::config::{EpicsPvaConfig, ProtocolConfig, WidgetConfig, WidgetStyle, WidgetType};
    use mycela::widgets::{render_gauge, widget_container_style};

    fn make_widget(style: Option<WidgetStyle>) -> WidgetConfig {
        WidgetConfig {
            id: "test1".into(),
            widget_type: WidgetType::Gauge,
            label: "Test".into(),
            protocol: Some(ProtocolConfig::EpicsPva(EpicsPvaConfig {
                pv_name: "demo:pv".into(),
                server: None,
                pv_names: None,
            })),
            style,
            ..Default::default()
        }
    }

    #[test]
    fn test_no_style_config_produces_no_style_attribute_in_html() {
        let w = make_widget(None);
        let css = widget_container_style(&w).expect("container style is always present");
        assert_eq!(css, "position:relative;");
        let html = render_gauge(&w).into_string();
        let outer_tag = &html[..html.find('>').unwrap() + 1];
        assert!(
            outer_tag.contains("style=\"position:relative;\""),
            "expected default relative-position style on outer div, got: {outer_tag}"
        );
    }

    #[test]
    fn test_width_and_height_style_config_produces_inline_style_attribute() {
        let w = make_widget(Some(WidgetStyle {
            width: Some("400px".into()),
            height: Some("200px".into()),
            background: None,
            left: None,
            top: None,
        }));
        let css = widget_container_style(&w).unwrap();
        assert!(css.contains("width:400px;"), "CSS must contain width");
        assert!(css.contains("height:200px;"), "CSS must contain height");

        let html = render_gauge(&w).into_string();
        assert!(
            html.contains("style=\"position:relative;width:400px;height:200px;\""),
            "rendered HTML must include inline style, got: {html}"
        );
    }

    #[test]
    fn test_width_only_style_config_produces_width_without_height() {
        let w = make_widget(Some(WidgetStyle {
            width: Some("50%".into()),
            height: None,
            background: None,
            left: None,
            top: None,
        }));
        let css = widget_container_style(&w).unwrap();
        assert_eq!(css, "position:relative;width:50%;");
        assert!(!css.contains("height"));
    }
}
