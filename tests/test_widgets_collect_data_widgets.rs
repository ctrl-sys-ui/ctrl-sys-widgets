mod test_widgets_collect_data_widgets {
    use mycela::config::{WidgetConfig, WidgetType};
    use mycela::widgets::collect_data_widgets;

    fn simple_widget(id: &str, wtype: WidgetType) -> WidgetConfig {
        WidgetConfig {
            id: id.to_string(),
            widget_type: wtype,
            label: id.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_flat_widget_list_is_returned_unchanged_by_collect() {
        let ws = vec![
            simple_widget("w1", WidgetType::TextUpdate),
            simple_widget("w2", WidgetType::Gauge),
        ];
        let result = collect_data_widgets(&ws);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, "w1");
        assert_eq!(result[1].id, "w2");
    }

    #[test]
    fn test_group_widget_is_replaced_by_its_children_in_collect() {
        let mut grp = simple_widget("grp", WidgetType::Group);
        grp.children = Some(vec![
            simple_widget("c1", WidgetType::Led),
            simple_widget("c2", WidgetType::Slider),
        ]);
        let ws = vec![simple_widget("top", WidgetType::TextUpdate), grp];
        let result = collect_data_widgets(&ws);
        assert_eq!(result.len(), 3);
        assert!(result.iter().all(|w| w.widget_type != WidgetType::Group));
        assert!(result.iter().any(|w| w.id == "top"));
        assert!(result.iter().any(|w| w.id == "c1"));
        assert!(result.iter().any(|w| w.id == "c2"));
    }

    #[test]
    fn test_nested_group_hierarchy_is_fully_flattened_by_collect() {
        let mut inner = simple_widget("inner", WidgetType::Group);
        inner.children = Some(vec![simple_widget("deep", WidgetType::Gauge)]);
        let mut outer = simple_widget("outer", WidgetType::Group);
        outer.children = Some(vec![inner]);
        let result = collect_data_widgets(&[outer]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "deep");
    }

    #[test]
    fn test_empty_widget_list_returns_empty_result_from_collect() {
        assert!(collect_data_widgets(&[]).is_empty());
    }

    #[test]
    fn test_group_with_no_children_contributes_nothing_to_collect() {
        let grp = simple_widget("empty_grp", WidgetType::Group);
        let result = collect_data_widgets(&[grp]);
        assert!(result.is_empty());
    }
}
