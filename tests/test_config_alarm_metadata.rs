mod test_config_alarm_metadata {
    use mycela::config::AlarmMetadata;

    /// Alarm band used by all severity tests.
    ///   low alarm  < 10  → MAJOR (2)
    ///   low warn   < 20  → MINOR (1)
    ///   normal  20..=80  → none  (0)
    ///   high warn  > 80  → MINOR (1)
    ///   high alarm > 90  → MAJOR (2)
    fn alarm_limits() -> AlarmMetadata {
        AlarmMetadata {
            low_alarm_limit: 10.0,
            low_warning_limit: 20.0,
            high_warning_limit: 80.0,
            high_alarm_limit: 90.0,
            low_alarm_severity: "MAJOR".to_string(),
            low_warning_severity: "MINOR".to_string(),
            high_warning_severity: "MINOR".to_string(),
            high_alarm_severity: "MAJOR".to_string(),
            hysteresis: 1,
        }
    }

    #[test]
    fn test_value_below_low_alarm_limit_produces_major_severity() {
        assert_eq!(alarm_limits().compute_severity(5.0), 2);
    }

    #[test]
    fn test_value_above_high_alarm_limit_produces_major_severity() {
        assert_eq!(alarm_limits().compute_severity(95.0), 2);
    }

    #[test]
    fn test_value_between_low_alarm_and_low_warning_produces_minor_severity() {
        // 15.0 is between low_alarm(10) and low_warning(20) → MINOR
        assert_eq!(alarm_limits().compute_severity(15.0), 1);
    }

    #[test]
    fn test_value_above_high_warning_limit_produces_minor_severity() {
        assert_eq!(alarm_limits().compute_severity(85.0), 1);
    }

    #[test]
    fn test_value_in_normal_range_produces_no_alarm_severity() {
        assert_eq!(alarm_limits().compute_severity(50.0), 0);
    }

    #[test]
    fn test_value_at_low_alarm_limit_boundary_produces_minor_not_major() {
        // value == low_alarm_limit(10.0) does NOT trigger the alarm clause (uses <),
        // but IS < low_warning_limit(20.0), so it returns MINOR (1).
        assert_eq!(alarm_limits().compute_severity(10.0), 1);
    }

    #[test]
    fn test_value_at_high_alarm_limit_boundary_produces_minor_not_major() {
        // value == high_alarm_limit(90.0) does NOT trigger the alarm clause (uses >),
        // but IS > high_warning_limit(80.0), so it returns MINOR (1).
        assert_eq!(alarm_limits().compute_severity(90.0), 1);
    }

    #[test]
    fn test_unrecognised_severity_string_in_metadata_produces_no_alarm() {
        let m = AlarmMetadata {
            low_alarm_limit: 10.0,
            low_warning_limit: 20.0,
            high_warning_limit: 80.0,
            high_alarm_limit: 90.0,
            low_alarm_severity: "BADVAL".to_string(),
            low_warning_severity: "".to_string(),
            high_warning_severity: "".to_string(),
            high_alarm_severity: "BADVAL".to_string(),
            hysteresis: 0,
        };
        assert_eq!(m.compute_severity(5.0), 0);
        assert_eq!(m.compute_severity(95.0), 0);
    }
}
