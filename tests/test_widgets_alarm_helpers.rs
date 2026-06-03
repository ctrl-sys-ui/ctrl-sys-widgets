mod test_widgets_alarm_helpers {
    use mycela::widgets::{alarm_severity_class, alarm_status_str};

    #[test]
    fn test_alarm_severity_zero_maps_to_alarm_none_class() {
        assert_eq!(alarm_severity_class(0), "alarm-none");
    }

    #[test]
    fn test_alarm_severity_one_maps_to_alarm_minor_class() {
        assert_eq!(alarm_severity_class(1), "alarm-minor");
    }

    #[test]
    fn test_alarm_severity_two_maps_to_alarm_major_class() {
        assert_eq!(alarm_severity_class(2), "alarm-major");
    }

    #[test]
    fn test_alarm_severity_three_and_above_maps_to_alarm_invalid_class() {
        assert_eq!(alarm_severity_class(3), "alarm-invalid");
        assert_eq!(alarm_severity_class(99), "alarm-invalid");
    }

    #[test]
    fn test_known_alarm_status_codes_produce_correct_string_labels() {
        assert_eq!(alarm_status_str(0), "No Alarm");
        assert_eq!(alarm_status_str(1), "Device");
        assert_eq!(alarm_status_str(2), "Driver");
        assert_eq!(alarm_status_str(6), "Client");
    }

    #[test]
    fn test_unknown_alarm_status_code_produces_unknown_label() {
        assert_eq!(alarm_status_str(99), "Unknown");
    }
}
