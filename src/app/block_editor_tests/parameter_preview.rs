use super::*;
use crate::scene::WireModel;
use crate::scene::{
    block_actions as actions, block_distance_parameter as distance, block_grips::runtime,
    block_orientation_parameter as orientation, block_point_parameter as point,
};
use glam::DVec3;

fn points(wires: &[WireModel]) -> Vec<[f64; 3]> {
    let mut points = Vec::new();
    for wire in wires {
        for (i, p) in wire.points.iter().enumerate() {
            let lo = wire.points_low.get(i).copied().unwrap_or([0.; 3]);
            let point = std::array::from_fn(|j| f64::from(p[j]) + f64::from(lo[j]));
            // NaN vertices separate batched wire segments; they are not geometry.
            if point.iter().all(|coordinate| coordinate.is_finite()) {
                points.push(point);
            }
        }
    }
    points.sort_by(|a, b| {
        a[0].total_cmp(&b[0])
            .then(a[1].total_cmp(&b[1]))
            .then(a[2].total_cmp(&b[2]))
    });
    points
}

fn setup(family: usize, testing: bool) -> (OpenCADStudio, Handle, Handle, Handle) {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let source = app.active_tab;
    let def = app.tabs[source]
        .active_block_edit_session()
        .unwrap()
        .br_handle;
    let member = members(&app)[0];
    let doc = &mut app.tabs[source].scene.document;
    let parameter = match family {
        0 => point::create(doc, def, Vector3::ZERO, Vector3::UNIT_Y),
        4 => orientation::create(
            doc,
            def,
            orientation::Kind::Rotation,
            Vector3::ZERO,
            Vector3::new(10., 0., 0.),
            Vector3::new(10., 0., 0.),
        ),
        _ => distance::create(
            doc,
            def,
            match family {
                2 => distance::Kind::Polar,
                6 => distance::Kind::XY,
                _ => distance::Kind::Linear,
            },
            Vector3::ZERO,
            Vector3::new(10., if family == 6 { 10. } else { 0. }, 0.),
            Vector3::UNIT_Y,
        ),
    }
    .unwrap();
    let kind = match family {
        1 => actions::Kind::Stretch,
        2 => actions::Kind::PolarStretch,
        3 => actions::Kind::Scale,
        4 => actions::Kind::Rotate,
        5 => actions::Kind::Array,
        _ => actions::Kind::Move,
    };
    actions::create_with_options(
        doc,
        def,
        kind,
        parameter,
        None,
        &[member],
        DVec3::ZERO,
        actions::needs_frame(kind).then_some([DVec3::new(-2., -2., 0.), DVec3::new(2., 2., 0.)]),
        (kind == actions::Kind::Array).then_some((2., 2.)),
    )
    .unwrap();
    // An unchanged fill must remain visible while the whole reference is hidden.
    let mut hatch = codec::entities::Hatch::new();
    hatch.is_solid = true;
    let mut path = codec::entities::BoundaryPath::new();
    path.add_edge(codec::entities::BoundaryEdge::Polyline(
        codec::entities::PolylineEdge::new(
            [[30., 30.], [32., 30.], [32., 32.], [30., 32.]]
                .into_iter()
                .map(|p| codec::types::Vector2::new(p[0], p[1]))
                .collect(),
            true,
        ),
    ));
    hatch.paths.push(path);
    hatch.common.owner_handle = def;
    doc.add_entity(EntityType::Hatch(Box::new(hatch))).unwrap();
    if testing {
        let _ = app.open_block_test(source);
    } else {
        let _ = app.dispatch_command("BSAVE");
        let _ = app.dispatch_command("BCLOSE");
    }
    let i = app.active_tab;
    let insert = app.tabs[i]
        .block_test
        .as_ref()
        .map_or(reference, |test| test.insert);
    let EntityType::Insert(ins) = app.tabs[i].scene.document.get_entity_mut(insert).unwrap() else {
        panic!()
    };
    ins.insert_point = Vector3::new(100., 200., 30.);
    ins.rotation = 0.3;
    ins.set_x_scale(2.);
    ins.set_y_scale(2.);
    ins.set_z_scale(2.);
    if family == 3 {
        ins.set_x_scale(-2.);
    }
    if family == 4 {
        ins.normal = Vector3::new(
            0.,
            std::f64::consts::FRAC_1_SQRT_2,
            std::f64::consts::FRAC_1_SQRT_2,
        );
    }
    app.tabs[i].scene.bump_geometry();
    app.tabs[i].scene.select_entity(insert, true);
    app.tabs[i].selected_handle = Some(insert);
    app.refresh_properties();
    let node = actions::all_parameters(&app.tabs[i].scene.document, def)
        .into_iter()
        .find(|p| p.handle == parameter)
        .unwrap()
        .id;
    let grip = app.tabs[i]
        .selected_grips
        .iter()
        .find(|g| runtime::decode(g.id).is_some_and(|(id, _)| id == node))
        .unwrap()
        .id;
    assert!(app.start_test_action_grip(insert, grip));
    (app, insert, def, parameter)
}

#[test]
fn block_parameter_geometry_preview_matches_committed_actions_in_drawing_and_test_block() {
    for testing in [false, true] {
        for family in 0..7 {
            let (mut app, insert, _, _) = setup(family, testing);
            let i = app.active_tab;
            let before = app.tabs[i].scene.document.clone();
            let baseline = app.tabs[i].block_test.as_ref().map(|t| t.baseline.clone());
            let depth = app.tabs[i].history.undo_stack.len();
            let dirty = app.tabs[i].dirty;
            let EntityType::Insert(reference) = before.get_entity(insert).unwrap() else {
                panic!()
            };
            let transform = crate::scene::render_graph::insert_transform(&before, reference);
            let mut final_points = Vec::new();
            let mut final_cursor = DVec3::ZERO;
            for local in [DVec3::new(5., 4., 0.), DVec3::new(20., 8., 0.)] {
                let cursor = actions::v(transform.apply(actions::native(local)));
                app.tabs[i].last_cursor_world = cursor;
                app.refresh_active_cmd_preview(i);
                let preview = &app.tabs[i].scene.preview_wires;
                assert!(!preview.is_empty(), "family={family} test={testing}");
                assert!(!app.tabs[i].scene.preview_hatches.is_empty());
                assert!(app.tabs[i]
                    .scene
                    .visible_entities_for_edit(|entity| entity.common().handle == insert)
                    .is_empty());
                assert!(app.tabs[i].scene.document == before);
                assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
                assert_eq!(app.tabs[i].dirty, dirty);
                let current = points(preview);
                if !final_points.is_empty() {
                    assert_ne!(current, final_points);
                }
                final_points = current;
                final_cursor = cursor;
            }
            let _ = app.feed_command(crate::command::StepInput::Point(final_cursor));
            assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
            assert!(!app.tabs[i]
                .scene
                .visible_entities_for_edit(|entity| entity.common().handle == insert)
                .is_empty());
            assert!(app.tabs[i].scene.preview_wires.is_empty());
            assert!(app.tabs[i].scene.preview_hatches.is_empty());
            let committed = app.tabs[i]
                .scene
                .reference_document_preview(&app.tabs[i].scene.document, insert)
                .unwrap();
            assert_eq!(
                points(&committed.0),
                final_points,
                "family={family} test={testing}"
            );
            if let Some(baseline) = baseline {
                assert!(*app.tabs[i].block_test.as_ref().unwrap().baseline == *baseline);
            }
            app.undo_steps(1);
            assert_eq!(
                app.tabs[i].scene.document.get_entity(insert),
                before.get_entity(insert)
            );
            assert!(app.tabs[i].scene.document.objects == before.objects);
            app.redo_steps(1);
            assert!(!app.tabs[i]
                .scene
                .visible_entities_for_edit(|entity| entity.common().handle == insert)
                .is_empty());
        }
    }
}

#[test]
fn block_parameter_geometry_preview_cancel_and_invalid_input_restore_resident_reference() {
    for testing in [false, true] {
        let (mut app, insert, def, parameter) = setup(0, testing);
        let i = app.active_tab;
        let before = app.tabs[i].scene.document.clone();
        let depth = app.tabs[i].history.undo_stack.len();
        app.tabs[i].last_cursor_world = DVec3::new(120., 220., 30.);
        app.refresh_active_cmd_preview(i);
        assert!(app.tabs[i]
            .scene
            .visible_entities_for_edit(|entity| entity.common().handle == insert)
            .is_empty());
        app.tabs[i].last_cursor_world = DVec3::NAN;
        app.refresh_active_cmd_preview(i);
        assert!(app.tabs[i].scene.preview_wires.is_empty());
        assert!(app.tabs[i].scene.preview_hatches.is_empty());
        assert!(!app.tabs[i]
            .scene
            .visible_entities_for_edit(|entity| entity.common().handle == insert)
            .is_empty());
        app.tabs[i].last_cursor_world = DVec3::new(120., 220., 30.);
        app.refresh_active_cmd_preview(i);
        assert!(app.tabs[i]
            .scene
            .visible_entities_for_edit(|entity| entity.common().handle == insert)
            .is_empty());
        let _ = app.update(Message::CommandEscape);
        assert!(!app.tabs[i]
            .scene
            .visible_entities_for_edit(|entity| entity.common().handle == insert)
            .is_empty());
        assert!(app.tabs[i].scene.preview_wires.is_empty());
        assert!(app.tabs[i].scene.preview_hatches.is_empty());
        assert!(app.tabs[i].scene.document == before);
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
        assert!(actions::has_action(
            &app.tabs[i].scene.document,
            def,
            parameter
        ));
    }
}

#[test]
fn block_parameter_geometry_preview_drops_stale_locked_and_disassociated_targets() {
    for testing in [false, true] {
        for invalid in 0..4 {
            let (mut app, insert, def, _) = setup(0, testing);
            let i = app.active_tab;
            let cursor = DVec3::new(120., 220., 30.);
            app.tabs[i].last_cursor_world = cursor;
            app.refresh_active_cmd_preview(i);
            assert!(!app.tabs[i].scene.preview_wires.is_empty());
            match invalid {
                0 => app.tabs[i].id += 1,
                1 => app.tabs[i].selected_handle = None,
                2 => {
                    app.tabs[i]
                        .scene
                        .document
                        .layers
                        .get_mut("0")
                        .unwrap()
                        .flags
                        .locked = true
                }
                _ => {
                    let action = actions::actions(&app.tabs[i].scene.document, def)[0].0;
                    let codec::objects::ObjectType::DynamicBlock(object) =
                        app.tabs[i].scene.document.objects.get_mut(&action).unwrap()
                    else {
                        panic!()
                    };
                    let codec::objects::DynamicBlockData::MoveAction(action) = &mut object.data
                    else {
                        panic!()
                    };
                    action.connections[1].code = -1;
                    if app.tabs[i].block_test.is_some() {
                        let baseline = app.tabs[i].scene.document.clone();
                        app.tabs[i].block_test.as_mut().unwrap().baseline =
                            std::sync::Arc::new(baseline);
                    }
                }
            }
            let before = app.tabs[i].scene.document.clone();
            let depth = app.tabs[i].history.undo_stack.len();
            app.refresh_active_cmd_preview(i);
            assert!(app.tabs[i].scene.preview_wires.is_empty());
            assert!(app.tabs[i].scene.preview_hatches.is_empty());
            assert!(!app.tabs[i]
                .scene
                .visible_entities_for_edit(|entity| entity.common().handle == insert)
                .is_empty());
            assert!(app.tabs[i].scene.document == before);
            assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
        }
    }
}
#[test]
fn block_array_over_budget_preview_restores_source_and_preserves_history() {
    for testing in [false, true] {
        let (mut app, insert, _, _) = setup(5, testing);
        let i = app.active_tab;
        let before = app.tabs[i].scene.document.clone();
        let depth = app.tabs[i].history.undo_stack.len();
        let dirty = app.tabs[i].dirty;
        let EntityType::Insert(reference) = before.get_entity(insert).unwrap() else {
            panic!()
        };
        let transform = crate::scene::render_graph::insert_transform(&before, reference);
        let cursor =
            |distance: f64| actions::v(transform.apply(actions::native(DVec3::X * distance)));
        app.tabs[i].last_cursor_world = cursor(20.);
        app.refresh_active_cmd_preview(i);
        assert!(!app.tabs[i].scene.preview_wires.is_empty());
        app.tabs[i].last_cursor_world = cursor(300_000.);
        app.refresh_active_cmd_preview(i);
        assert!(app.tabs[i].scene.preview_wires.is_empty());
        assert!(app.tabs[i].scene.preview_hatches.is_empty());
        assert!(!app.tabs[i]
            .scene
            .visible_entities_for_edit(|entity| entity.common().handle == insert)
            .is_empty());
        assert!(app.tabs[i].scene.document == before);
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
        assert_eq!(app.tabs[i].dirty, dirty);
        let _ = app.feed_command(crate::command::StepInput::Point(cursor(300_000.)));
        assert!(app.tabs[i].scene.document == before);
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
        assert_eq!(app.tabs[i].dirty, dirty);
        // The live command remains usable after the rejected point.
        app.tabs[i].last_cursor_world = cursor(30.);
        app.refresh_active_cmd_preview(i);
        assert!(!app.tabs[i].scene.preview_wires.is_empty());
        let _ = app.feed_command(crate::command::StepInput::Point(cursor(30.)));
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
        app.undo_steps(1);
        assert_eq!(
            app.tabs[i].scene.document.get_entity(insert),
            before.get_entity(insert)
        );
        assert_eq!(app.tabs[i].scene.document.objects, before.objects);
    }
}
