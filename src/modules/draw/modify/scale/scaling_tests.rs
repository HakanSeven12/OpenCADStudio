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
        let CmdResult::TransformSelected(_, EntityTransform::Scale { center, factor }) = result
        else {
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

/// A closed rectangle `w` x `h` with its lower-left corner at `origin`.
fn rectangle(origin: DVec3, w: f64, h: f64) -> WireModel {
    WireModel::solid(
        "rect".into(),
        vec![
            [0.0; 3],
            [w as f32, 0.0, 0.0],
            [w as f32, h as f32, 0.0],
            [0.0, h as f32, 0.0],
            [0.0; 3],
        ],
        WireModel::CYAN,
        false,
    )
    .mapped(|p| p + origin)
}

/// The factor a click `offset` away from the base gives, with the preview and
/// the live value checked against it.
fn picked_factor(source: WireModel, base: DVec3, offset: DVec3) -> f64 {
    let source_width = preview_width(&source);
    let mut command = ScaleCommand::new(vec![], vec![source]);
    command.on_point(base);
    let cursor = base + offset;
    let live = command.dyn_live_value(cursor).expect("live factor");
    let preview = command.on_preview_wires(cursor);
    let CmdResult::TransformSelected(_, EntityTransform::Scale { center, factor }) =
        command.on_point(cursor)
    else {
        panic!("scale transform");
    };
    assert_eq!(center, base);
    assert!(
        (live - factor).abs() < 1e-12,
        "live {live} != factor {factor}"
    );
    let width = source_width * factor;
    assert!(
        (preview_width(&preview[0]) - width).abs() < 1e-6,
        "preview not {width} wide"
    );
    factor
}

fn preview_width(wire: &WireModel) -> f64 {
    wire.point_world(1, 100.0).x - wire.point_world(0, 100.0).x
}

#[test]
fn a_picked_scale_factor_is_measured_against_half_the_selection_size() {
    let base = DVec3::new(600_000.0, 400_000.0, 0.0);
    // 8 x 5: half the larger side is 4, so a point 4 away keeps the size
    // and a point 2 away halves it.
    let wide = || rectangle(base, 8.0, 5.0);
    assert!((picked_factor(wide(), base, DVec3::new(4.0, 0.0, 0.0)) - 1.0).abs() < 1e-9);
    assert!((picked_factor(wide(), base, DVec3::new(0.0, 2.0, 0.0)) - 0.5).abs() < 1e-9);
    // 2 x 9: the larger side is the height, so 4.5 away keeps the size.
    let tall = rectangle(base, 2.0, 9.0);
    assert!((picked_factor(tall, base, DVec3::new(0.0, 4.5, 0.0)) - 1.0).abs() < 1e-9);
}

#[test]
fn the_dragged_preview_has_the_size_a_click_would_give() {
    let base = DVec3::new(1000.0, 2000.0, 0.0);
    let mut command = ScaleCommand::new(vec![], vec![rectangle(base, 8000.0, 5000.0)]);
    command.on_point(base);
    // A cursor 4000 units away is half the 8000 wide object: no change.
    let preview = command.on_preview_wires(base + DVec3::new(0.0, 4000.0, 0.0));
    assert!((preview_width(&preview[0]) - 8000.0).abs() < 1e-3);
}

#[test]
fn a_selection_without_extent_keeps_the_plain_distance() {
    let base = DVec3::new(5.0, 5.0, 0.0);
    let dot = WireModel::solid("dot".into(), vec![[5.0, 5.0, 0.0]], WireModel::CYAN, false);
    let mut command = ScaleCommand::new(vec![], vec![dot]);
    command.on_point(base);
    let CmdResult::TransformSelected(_, EntityTransform::Scale { factor, .. }) =
        command.on_point(base + DVec3::new(3.0, 0.0, 0.0))
    else {
        panic!("scale transform");
    };
    assert!((factor - 3.0).abs() < 1e-12);
}
