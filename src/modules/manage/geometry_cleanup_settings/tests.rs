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

#[test]
fn the_proposed_settings_have_the_default_angle_rows() {
    assert_eq!(
        CleanupSettings::proposed(0.4),
        CleanupSettings::parse_gcs(SAMPLE_GCS).unwrap()
    );
}

#[test]
fn the_angle_rows_give_the_standard_angles_of_each_type() {
    let tolerances = CleanupSettings::parse_gcs(SAMPLE_GCS)
        .unwrap()
        .angle_tolerances();
    let standards = |angles: [f64; 2]| {
        angles
            .map(|angle| AngleTolerance {
                angle,
                tolerance: 5.0,
            })
            .to_vec()
    };
    assert_eq!(tolerances.absolute, standards([0.0, 90.0]));
    assert_eq!(tolerances.relative, standards([0.0, 90.0]));
}

#[test]
fn an_angle_row_that_is_not_a_number_or_a_second_time_is_kept_but_not_scanned() {
    let text = r#"{"tolerances":[
        {"name":"absolute-angle","subName":"right","tolerance":2.0},
        {"name":"absolute-angle","subName":"-45","tolerance":2.0},
        {"name":"absolute-angle","subName":"315","tolerance":3.0},
        {"name":"relative-angle","subName":"","tolerance":2.0},
        {"name":"relative-angle","subName":"60","tolerance":0.00001}
    ]}"#;
    let settings = CleanupSettings::parse_gcs(text).unwrap();
    assert_eq!(settings.tolerances.len(), 5);
    let tolerances = settings.angle_tolerances();
    assert_eq!(
        tolerances.absolute,
        [AngleTolerance {
            angle: 135.0,
            tolerance: 2.0
        }]
    );
    assert_eq!(
        tolerances.relative,
        [AngleTolerance {
            angle: 60.0,
            tolerance: MIN_TOLERANCE
        }]
    );
    assert!(CleanupSettings::parse_gcs(&settings.to_gcs())
        .unwrap()
        .to_gcs()
        .contains("\"right\""));
}

#[test]
fn a_byte_order_mark_before_the_file_is_skipped() {
    let with_mark = format!("\u{feff}{SAMPLE_GCS}");
    assert_eq!(
        CleanupSettings::parse_gcs(&with_mark).unwrap(),
        CleanupSettings::parse_gcs(SAMPLE_GCS).unwrap()
    );
}

/// A file with a field of its own and a field in a row that OCS does not
/// know.
const GCS_WITH_UNKNOWN_FIELDS: &str = r#"{
  "version": 2,
  "tolerances": [
    { "name": "gap", "subName": "", "tolerance": 0.4, "enabled": true }
  ]
}"#;

#[test]
fn fields_that_ocs_does_not_know_are_written_back() {
    let settings = CleanupSettings::parse_gcs(GCS_WITH_UNKNOWN_FIELDS).unwrap();
    let written: serde_json::Value = serde_json::from_str(&settings.to_gcs()).unwrap();
    assert_eq!(written["version"], 2);
    assert_eq!(written["tolerances"][0]["enabled"], true);
    assert_eq!(written["tolerances"][0]["tolerance"], 0.4);
}

#[test]
fn a_changed_tolerance_keeps_the_unknown_fields_of_its_row() {
    let mut settings = CleanupSettings::parse_gcs(GCS_WITH_UNKNOWN_FIELDS).unwrap();
    settings.set(EndErrorKind::Gap, Some(0.7));
    let row = &settings.tolerances[0];
    assert_eq!(row.tolerance, 0.7);
    assert_eq!(row.extra["enabled"], true);
}

#[test]
fn a_negative_tolerance_names_its_type_and_value() {
    let error = CleanupSettings::parse_gcs(
        r#"{ "tolerances": [ { "name": "overshoot", "subName": "", "tolerance": -1.5 } ] }"#,
    )
    .unwrap_err();
    assert!(error.contains("overshoot"), "{error}");
    assert!(error.contains("-1.5"), "{error}");
    assert!(error.contains("0 or more"), "{error}");
}
