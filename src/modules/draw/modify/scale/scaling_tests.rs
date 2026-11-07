use super::*;

#[test]
fn table_scaling_interactive_preview_matches_commit_at_distant_bases() {
    for reference in [None, Some(2.0)] {
        let base = DVec3::new(600_000.123, 400_000.456, 0.0);
        let source = WireModel::solid(
            "source".into(),
            vec![[0.0; 3], [2.0, 0.0, 0.0]],
            WireModel::CYAN,
            false,
        )
        .mapped(|p| p + base);
        let mut command = ScaleCommand::new(vec![], vec![source]);
        command.on_point(base);
        if let Some(length) = reference {
            command.on_text_input("R");
            command.on_text_input(&length.to_string());
        }
        let cursor = base + DVec3::new(0.25, 0.0, 0.0);
        let preview = command.on_preview_wires(cursor);
        let result = command.on_point(cursor);
        let CmdResult::TransformSelected(_, EntityTransform::Scale { center, factor }) = result else {
            panic!("scale transform");
        };
        assert_eq!(center, base);
        assert_eq!(factor, 0.25 / reference.unwrap_or(1.0));
        for index in 0..2 {
            let expected = base + DVec3::X * (index as f64 * 2.0 * factor);
            let actual = preview[0].point_world(index, 100.0);
            assert!(
                actual.distance(expected) < 1e-7,
                "preview {actual} != {expected}"
            );
        }
        let band = preview.last().unwrap();
        assert!(band.point_world(0, 100.0).distance(base) < 1e-7);
        assert!(band.point_world(1, 100.0).distance(cursor) < 1e-7);
    }
}
