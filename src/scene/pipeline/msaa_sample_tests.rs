
    use super::msaa_samples_from;

    /// `OCS_MSAA` takes 1 or 4; anything else, or nothing, keeps the 4 the
    /// pipelines always used.
    #[test]
    fn ocs_msaa_takes_one_or_four_and_keeps_four_otherwise() {
        assert_eq!(msaa_samples_from(Some("1")), 1);
        assert_eq!(msaa_samples_from(Some(" 1 ")), 1);
        assert_eq!(msaa_samples_from(Some("4")), 4);
        for other in ["2", "8", "0", "", "x", "-1"] {
            assert_eq!(msaa_samples_from(Some(other)), 4, "OCS_MSAA={other:?}");
        }
        assert_eq!(msaa_samples_from(None), 4);
    }
