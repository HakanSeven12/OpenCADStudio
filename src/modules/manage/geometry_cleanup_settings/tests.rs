use super::*;

#[test]
fn a_full_file_gives_its_length_tolerances_and_keeps_its_angle_rows() {
    let settings = CleanupSettings::parse_gcs(SAMPLE_GCS).unwrap();
    for kind in [
        EndErrorKind::Gap,
        EndErrorKind::Overshoot,
        EndErrorKind::Undershoot,
    ] {
        assert_eq!(settings.tolerance(kind), Some(0.4));
    }
    assert_eq!(settings.tolerances.len(), 7);
    assert_eq!(settings.tolerances[3].name, "absolute-angle");
    assert_eq!(settings.tolerances[3].sub_name, "0");
}

#[test]
fn a_file_written_and_read_again_is_the_same() {
    let settings = CleanupSettings::parse_gcs(SAMPLE_GCS).unwrap();
    assert_eq!(
        CleanupSettings::parse_gcs(&settings.to_gcs()).unwrap(),
        settings
    );
    assert!(settings.to_gcs().contains("\"subName\""));
}

#[test]
fn a_type_left_out_of_the_file_is_not_scanned() {
    let text = r#"{"tolerances":[{"name":"gap","subName":"","tolerance":0.4}]}"#;
    let settings = CleanupSettings::parse_gcs(text).unwrap();
    assert_eq!(
        settings.tolerances(),
        Tolerances {
            gap: Some(0.4),
            overshoot: None,
            undershoot: None
        }
    );
}

#[test]
fn a_file_that_cannot_be_used_is_an_error() {
    for text in [
        "not json",
        r#"{"other":[]}"#,
        r#"{"tolerances":[{"name":"gap","subName":"","tolerance":-1.0}]}"#,
        r#"{"tolerances":[{"name":"gap","subName":"","tolerance":"x"}]}"#,
    ] {
        assert!(CleanupSettings::parse_gcs(text).is_err(), "{text}");
    }
}

#[test]
fn a_tolerance_below_the_minimum_becomes_the_minimum() {
    let text = r#"{"tolerances":[{"name":"gap","subName":"","tolerance":0.00001}]}"#;
    assert_eq!(
        CleanupSettings::parse_gcs(text)
            .unwrap()
            .tolerance(EndErrorKind::Gap),
        Some(MIN_TOLERANCE)
    );
    let mut settings = CleanupSettings::proposed(1.0);
    settings.set(EndErrorKind::Overshoot, Some(0.0));
    assert_eq!(
        settings.tolerance(EndErrorKind::Overshoot),
        Some(MIN_TOLERANCE)
    );
    settings.set(EndErrorKind::Gap, Some(0.1234567));
    assert_eq!(settings.tolerance(EndErrorKind::Gap), Some(0.123457));
}

#[test]
fn a_type_added_back_goes_before_the_angle_rows() {
    let mut settings = CleanupSettings::parse_gcs(SAMPLE_GCS).unwrap();
    settings.set(EndErrorKind::Overshoot, None);
    assert_eq!(settings.tolerances.len(), 6);
    settings.set(EndErrorKind::Overshoot, Some(0.3));
    let names: Vec<&str> = settings
        .tolerances
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    assert_eq!(
        &names[..4],
        ["gap", "overshoot", "undershoot", "absolute-angle"]
    );
}

#[test]
fn a_tolerance_too_large_to_keep_is_an_error() {
    let text = r#"{"tolerances":[{"name":"gap","subName":"","tolerance":1e303}]}"#;
    assert!(CleanupSettings::parse_gcs(text).is_err());
    assert_eq!(clean_tolerance(1e303), None);
    assert_eq!(clean_tolerance(0.25), Some(0.25));
}

#[test]
fn a_tolerance_of_zero_in_a_file_becomes_the_minimum() {
    let text = r#"{"tolerances":[{"name":"gap","subName":"","tolerance":0.0}]}"#;
    assert_eq!(
        CleanupSettings::parse_gcs(text)
            .unwrap()
            .tolerance(EndErrorKind::Gap),
        Some(MIN_TOLERANCE)
    );
}
