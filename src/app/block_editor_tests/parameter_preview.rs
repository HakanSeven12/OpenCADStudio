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

fn rotation_arc_samples(preview: &[[f64; 3]]) -> Vec<[f64; 3]> {
    preview
        .iter()
        .copied()
        .filter(|point| point[0].hypot(point[1]) > 1e-6)
        .collect()
}

fn assert_rotation_arc(preview: &[[f64; 3]], start: f64, end: f64) {
    let samples = rotation_arc_samples(preview);
    assert!(!samples.is_empty(), "no rotation preview");
    for point in &samples {
        let radius = point[0].hypot(point[1]);
        assert!(
            radius <= 10.0 + 1e-6,
            "preview point outside the parameter radius: {point:?}"
        );
        let angle = point[1].atan2(point[0]).to_degrees().rem_euclid(360.0);
        assert!(
            (start - 1e-6..=end + 1e-6).contains(&angle),
            "preview point at {angle}° lies outside the {start}–{end}° arc: {point:?}"
        );
    }
    let middle = (start + end).to_radians() * 0.5;
    let target = [10.0 * middle.cos(), 10.0 * middle.sin()];
    assert!(
        samples
            .iter()
            .any(|p| (p[0] - target[0]).hypot(p[1] - target[1]) < 0.5),
        "no arc sample near {target:?}: {samples:?}"
    );
    for (angle, label) in [(start, "base angle"), (end, "default angle")] {
        let target = [
            10.0 * angle.to_radians().cos(),
            10.0 * angle.to_radians().sin(),
        ];
        assert!(
            samples
                .iter()
                .any(|p| (p[0] - target[0]).hypot(p[1] - target[1]) < 1e-6),
            "the {label} radius line is missing: {samples:?}"
        );
    }
}

#[test]
fn rotation_parameter_preview_draws_the_angle_arc_from_the_base_angle() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.dispatch_command("BPARAMETER ROTATION");
    feed(&mut app, "0,0");
    feed(&mut app, "10");
    app.tabs[i].last_cursor_world = DVec3::new(0.0, 10.0, 0.0);
    app.refresh_active_cmd_preview(i);
    assert_rotation_arc(&points(&app.tabs[i].scene.preview_wires), 0.0, 90.0);

    let _ = app.dispatch_command("BPARAMETER ROTATION");
    feed(&mut app, "0,0");
    feed(&mut app, "10");
    feed(&mut app, "B");
    feed(&mut app, "30");
    let cursor = DVec3::new(8.660_254_037_844_386, 5.0, 0.0);
    app.tabs[i].last_cursor_world = cursor;
    app.refresh_active_cmd_preview(i);
    assert_rotation_arc(&points(&app.tabs[i].scene.preview_wires), 30.0, 60.0);
    assert!(app.tabs[i].active_cmd.is_some());
}

fn setup_point_transport(
    testing: bool,
    chained: bool,
) -> (
    OpenCADStudio,
    Handle,
    Handle,
    actions::Parameter,
    actions::Parameter,
    Handle,
) {
    let (mut app, insert, def, root) = setup(0, testing);
    let _ = app.update(Message::CommandEscape);
    let i = app.active_tab;
    let root_parameter = actions::all_parameters(&app.tabs[i].scene.document, def)
        .into_iter()
        .find(|p| p.handle == root)
        .unwrap();
    let root_action = actions::actions(&app.tabs[i].scene.document, def)
        .into_iter()
        .find(|(_, _, _, id)| *id == root_parameter.id)
        .unwrap()
        .0;
    let doc = &mut app.tabs[i].scene.document;
    let codec::objects::ObjectType::DynamicBlock(object) = &doc.objects[&root_action] else {
        panic!()
    };
    let codec::objects::DynamicBlockData::MoveAction(action) = &object.data else {
        panic!()
    };
    let targets = action.action.dependencies.clone();
    let child = point::create(doc, def, Vector3::UNIT_Y * 5., Vector3::UNIT_Y * 7.).unwrap();
    let codec::objects::ObjectType::DynamicBlock(object) = doc.objects.get_mut(&child).unwrap()
    else {
        panic!()
    };
    let codec::objects::DynamicBlockData::PointParameter(parameter) = &mut object.data else {
        panic!()
    };
    parameter.parameter.parameter.chain_actions = chained;
    actions::create_with_options(
        doc,
        def,
        actions::Kind::Move,
        child,
        None,
        &targets,
        DVec3::ZERO,
        None,
        None,
    )
    .unwrap();
    let codec::objects::ObjectType::DynamicBlock(object) =
        doc.objects.get_mut(&root_action).unwrap()
    else {
        panic!()
    };
    let codec::objects::DynamicBlockData::MoveAction(action) = &mut object.data else {
        panic!()
    };
    action.action.dependencies = vec![child];
    let child_parameter = actions::all_parameters(doc, def)
        .into_iter()
        .find(|p| p.handle == child)
        .unwrap();
    if testing {
        let baseline = app.tabs[i].scene.document.clone();
        app.tabs[i].block_test.as_mut().unwrap().baseline = std::sync::Arc::new(baseline);
    }
    app.tabs[i].scene.bump_geometry();
    app.tabs[i].scene.select_entity(insert, true);
    app.tabs[i].selected_handle = Some(insert);
    app.refresh_properties();
    let grip = app.tabs[i]
        .selected_grips
        .iter()
        .find(|g| runtime::decode(g.id).is_some_and(|(id, _)| id == root_parameter.id))
        .unwrap()
        .id;
    assert!(app.start_test_action_grip(insert, grip));
    (
        app,
        insert,
        def,
        root_parameter,
        child_parameter,
        root_action,
    )
}

#[test]
fn block_move_point_chain_preview_events_commit_and_undo_effective_point_caches() {
    for testing in [false, true] {
        for chained in [false, true] {
            let (mut app, insert, _, root_parameter, child_parameter, _) =
                setup_point_transport(testing, chained);
            let i = app.active_tab;
            let before = app.tabs[i].scene.document.clone();
            let depth = app.tabs[i].history.undo_stack.len();
            let EntityType::Insert(reference) = before.get_entity(insert).unwrap() else {
                panic!()
            };
            let transform = crate::scene::render_graph::insert_transform(&before, reference);
            let cursor = actions::v(transform.apply(actions::native(DVec3::new(20., 8., 0.))));
            app.tabs[i].last_cursor_world = cursor;
            app.refresh_active_cmd_preview(i);
            let preview = points(&app.tabs[i].scene.preview_wires);
            assert!(!preview.is_empty());
            assert!(app.tabs[i].scene.document == before);
            assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
            let _ = app.feed_command(crate::command::StepInput::Point(cursor));
            assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
            let current = &app.tabs[i].scene.document;
            let parent = actions::instance::frame(current, insert, &root_parameter).unwrap();
            let child = actions::instance::frame(current, insert, &child_parameter).unwrap();
            assert!(
                child
                    .end
                    .distance(child_parameter.end + parent.end - root_parameter.end)
                    < 1e-8
            );
            let committed = app.tabs[i]
                .scene
                .reference_document_preview(current, insert)
                .unwrap();
            assert_eq!(points(&committed.0), preview);
            app.undo_steps(1);
            assert_eq!(
                app.tabs[i].scene.document.get_entity(insert),
                before.get_entity(insert)
            );
            assert_eq!(app.tabs[i].scene.document.objects, before.objects);
            app.redo_steps(1);
            assert_eq!(
                actions::instance::frame(&app.tabs[i].scene.document, insert, &child_parameter)
                    .unwrap(),
                child
            );
        }
    }
}

#[test]
fn block_move_point_chain_properties_refresh_existing_references_and_preserve_undo() {
    let (mut app, insert, _, root, child, action) = setup_point_transport(false, false);
    let i = app.active_tab;
    let read_line = |doc: &codec::Document| {
        let EntityType::Insert(reference) = doc.get_entity(insert).unwrap() else {
            panic!()
        };
        doc.block_records
            .get(&reference.block_name)
            .unwrap()
            .entity_handles
            .iter()
            .find_map(|h| {
                if let Some(EntityType::Line(line)) = doc.get_entity(*h) {
                    Some(actions::v(line.start))
                } else {
                    None
                }
            })
            .unwrap()
    };
    let original = read_line(&app.tabs[i].scene.document);
    let EntityType::Insert(reference) = app.tabs[i].scene.document.get_entity(insert).unwrap()
    else {
        panic!()
    };
    let transform =
        crate::scene::render_graph::insert_transform(&app.tabs[i].scene.document, reference);
    let cursor = actions::v(transform.apply(actions::native(root.end + DVec3::new(3., 4., 0.))));
    let _ = app.feed_command(crate::command::StepInput::Point(cursor));
    let offset = actions::instance::frame(&app.tabs[i].scene.document, insert, &root)
        .unwrap()
        .end
        - root.end;
    assert!(offset.length() > 1.);
    assert!(read_line(&app.tabs[i].scene.document).distance(original) < 1e-8);
    begin(&mut app, insert);
    let i = app.active_tab;
    app.tabs[i].selected_authoring = Some(child.handle);
    app.refresh_properties();
    assert!(app.tabs[i]
        .properties
        .sections
        .iter()
        .flat_map(|s| &s.props)
        .any(|p| p.field == "authoring_chain_actions"
            && matches!(
                &p.value,
                crate::scene::model::object::PropValue::BoolToggle { value: false, .. }
            )));
    let depth = app.tabs[i].history.undo_stack.len();
    let _ = app.update(Message::PropBoolToggle("authoring_chain_actions"));
    app.finish_pending_history(i);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    assert!(read_line(&app.tabs[i].scene.document).distance(original + offset) < 1e-8);
    app.undo_steps(1);
    assert!(read_line(&app.tabs[i].scene.document).distance(original) < 1e-8);
    app.redo_steps(1);
    assert!(read_line(&app.tabs[i].scene.document).distance(original + offset) < 1e-8);
    app.tabs[i].selected_authoring = Some(action);
    app.refresh_properties();
    let _ = app.update(Message::PropGeomInput {
        field: "authoring_action_multiplier",
        value: "2".into(),
    });
    let _ = app.update(Message::PropGeomCommit("authoring_action_multiplier"));
    app.finish_pending_history(i);
    assert!(read_line(&app.tabs[i].scene.document).distance(original + offset * 2.) < 1e-8);
    app.undo_steps(1);
    assert!(read_line(&app.tabs[i].scene.document).distance(original + offset) < 1e-8);
    app.redo_steps(1);
    assert!(read_line(&app.tabs[i].scene.document).distance(original + offset * 2.) < 1e-8);
}

#[test]
fn block_mixed_order_and_point_transport_grip_undo_redo() {
    for scale_first in [false, true] {
        let (mut app, insert) = fixture();
        let i = app.active_tab;
        let other = app.tabs[i]
            .scene
            .document
            .add_entity(EntityType::Insert(Insert::new(
                "EDIT_ME",
                Vector3::UNIT_X * 50.,
            )))
            .unwrap();
        begin(&mut app, insert);
        let def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
        let member = members(&app)[0];
        let doc = &mut app.tabs[i].scene.document;
        let parent = distance::create(
            doc,
            def,
            distance::Kind::Linear,
            Vector3::ZERO,
            Vector3::UNIT_X * 10.,
            Vector3::UNIT_Y,
        )
        .unwrap();
        let child = point::create(doc, def, Vector3::ZERO, Vector3::UNIT_Y).unwrap();
        let second_parent = point::create(doc, def, Vector3::ZERO, Vector3::UNIT_Y * 2.).unwrap();
        for handle in [parent, child] {
            let codec::objects::ObjectType::DynamicBlock(object) =
                doc.objects.get_mut(&handle).unwrap()
            else {
                panic!()
            };
            match &mut object.data {
                codec::objects::DynamicBlockData::LinearParameter(p) => {
                    p.parameter.parameter.chain_actions = true
                }
                codec::objects::DynamicBlockData::PointParameter(p) => {
                    p.parameter.parameter.chain_actions = true
                }
                _ => panic!(),
            }
        }
        let moved = actions::create(
            doc,
            def,
            actions::Kind::Move,
            parent,
            &[member],
            DVec3::ZERO,
        )
        .unwrap();
        let scaled = actions::create(
            doc,
            def,
            actions::Kind::Scale,
            parent,
            &[member],
            DVec3::ZERO,
        )
        .unwrap();
        actions::create(doc, def, actions::Kind::Move, child, &[member], DVec3::ZERO).unwrap();
        let second_move = actions::create(
            doc,
            def,
            actions::Kind::Move,
            second_parent,
            &[member],
            DVec3::ZERO,
        )
        .unwrap();
        let codec::objects::ObjectType::DynamicBlock(object) =
            doc.objects.get_mut(&second_move).unwrap()
        else {
            panic!()
        };
        let codec::objects::DynamicBlockData::MoveAction(action) = &mut object.data else {
            panic!()
        };
        action.action.dependencies = vec![child];
        let codec::objects::ObjectType::DynamicBlock(object) = doc.objects.get_mut(&moved).unwrap()
        else {
            panic!()
        };
        let codec::objects::DynamicBlockData::MoveAction(action) = &mut object.data else {
            panic!()
        };
        action.action.dependencies = vec![member, child];
        let (head, tail) = if scale_first {
            (scaled, moved)
        } else {
            (moved, scaled)
        };
        assert!(actions::chain::set(doc, def, head, Some(tail)).unwrap());
        let params = actions::all_parameters(doc, def);
        let root = params.iter().find(|p| p.handle == parent).unwrap().clone();
        let point = params.iter().find(|p| p.handle == child).unwrap().clone();
        let second_point = params
            .iter()
            .find(|p| p.handle == second_parent)
            .unwrap()
            .clone();
        let _ = app.dispatch_command("BSAVE");
        let _ = app.dispatch_command("BCLOSE");
        app.tabs[i].scene.select_entity(insert, true);
        app.tabs[i].selected_handle = Some(insert);
        app.refresh_properties();
        let grip = app.tabs[i]
            .selected_grips
            .iter()
            .find(|g| runtime::decode(g.id).is_some_and(|(id, _)| id == root.id))
            .unwrap()
            .id;
        assert!(app.start_test_action_grip(insert, grip));
        let before = app.tabs[i].scene.document.clone();
        let depth = app.tabs[i].history.undo_stack.len();
        let line = |doc: &codec::Document, handle: Handle| {
            let EntityType::Insert(reference) = doc.get_entity(handle).unwrap() else {
                panic!()
            };
            doc.block_records
                .get(&reference.block_name)
                .unwrap()
                .entity_handles
                .iter()
                .find_map(|h| {
                    if let Some(EntityType::Line(line)) = doc.get_entity(*h) {
                        Some((actions::v(line.start), actions::v(line.end)))
                    } else {
                        None
                    }
                })
                .unwrap()
        };
        let original = line(&before, other);
        let start = if scale_first { 20. } else { 30. };
        let _ = app.feed_command(crate::command::StepInput::Point(DVec3::new(20., 0., 0.)));
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
        let after = &app.tabs[i].scene.document;
        assert_eq!(
            line(after, insert),
            (DVec3::X * start, DVec3::X * (start + 2.))
        );
        assert_eq!(line(after, other), original);
        let frame = actions::instance::frame(after, insert, &point).unwrap();
        assert_eq!(frame.end, DVec3::X * 10.);
        app.undo_steps(1);
        assert_eq!(
            line(&app.tabs[i].scene.document, insert),
            line(&before, insert)
        );
        assert_eq!(
            actions::instance::frame(&app.tabs[i].scene.document, insert, &point)
                .unwrap()
                .end,
            point.end
        );
        assert_eq!(line(&app.tabs[i].scene.document, other), original);
        app.redo_steps(1);
        assert_eq!(
            line(&app.tabs[i].scene.document, insert),
            (DVec3::X * start, DVec3::X * (start + 2.))
        );
        assert_eq!(
            actions::instance::frame(&app.tabs[i].scene.document, insert, &point).unwrap(),
            frame
        );
        assert_eq!(line(&app.tabs[i].scene.document, other), original);

        // A direct child edit changes its autonomous input while retaining the
        // parent's transported displacement and the named action order.
        app.tabs[i].scene.select_entity(insert, true);
        app.tabs[i].selected_handle = Some(insert);
        app.refresh_properties();
        let child_grip = app.tabs[i]
            .selected_grips
            .iter()
            .find(|g| runtime::decode(g.id).is_some_and(|(id, _)| id == point.id))
            .unwrap()
            .id;
        assert!(app.start_test_action_grip(insert, child_grip));
        let _ = app.feed_command(crate::command::StepInput::Point(DVec3::new(15., 0., 0.)));
        assert_eq!(
            line(&app.tabs[i].scene.document, insert),
            (DVec3::X * (start + 5.), DVec3::X * (start + 7.))
        );
        assert_eq!(
            actions::instance::frame(&app.tabs[i].scene.document, insert, &point)
                .unwrap()
                .end,
            DVec3::X * 15.
        );
        app.undo_steps(1);
        assert_eq!(
            line(&app.tabs[i].scene.document, insert),
            (DVec3::X * start, DVec3::X * (start + 2.))
        );
        assert_eq!(
            actions::instance::frame(&app.tabs[i].scene.document, insert, &point).unwrap(),
            frame
        );
        app.redo_steps(1);
        assert_eq!(
            line(&app.tabs[i].scene.document, insert),
            (DVec3::X * (start + 5.), DVec3::X * (start + 7.))
        );
        assert_eq!(line(&app.tabs[i].scene.document, other), original);

        // Drive a second parent after the direct child edit. Both its delta and
        // the child's autonomous input must survive subsequent parent history.
        app.tabs[i].scene.select_entity(insert, true);
        app.tabs[i].selected_handle = Some(insert);
        app.refresh_properties();
        let second_grip = app.tabs[i]
            .selected_grips
            .iter()
            .find(|g| runtime::decode(g.id).is_some_and(|(id, _)| id == second_point.id))
            .unwrap()
            .id;
        assert!(app.start_test_action_grip(insert, second_grip));
        let _ = app.feed_command(crate::command::StepInput::Point(DVec3::new(3., 0., 0.)));
        assert_eq!(
            line(&app.tabs[i].scene.document, insert),
            (DVec3::X * (start + 8.), DVec3::X * (start + 10.))
        );
        assert_eq!(
            actions::instance::frame(&app.tabs[i].scene.document, insert, &point)
                .unwrap()
                .end,
            DVec3::X * 18.
        );
        app.undo_steps(1);
        assert_eq!(
            line(&app.tabs[i].scene.document, insert),
            (DVec3::X * (start + 5.), DVec3::X * (start + 7.))
        );
        assert_eq!(
            actions::instance::frame(&app.tabs[i].scene.document, insert, &point)
                .unwrap()
                .end,
            DVec3::X * 15.
        );
        app.redo_steps(1);
        assert_eq!(
            actions::instance::frame(&app.tabs[i].scene.document, insert, &point)
                .unwrap()
                .end,
            DVec3::X * 18.
        );
        assert_eq!(line(&app.tabs[i].scene.document, other), original);

        app.tabs[i].scene.select_entity(insert, true);
        app.tabs[i].selected_handle = Some(insert);
        app.refresh_properties();
        let root_grip = app.tabs[i]
            .selected_grips
            .iter()
            .find(|g| runtime::decode(g.id).is_some_and(|(id, _)| id == root.id))
            .unwrap()
            .id;
        assert!(app.start_test_action_grip(insert, root_grip));
        let _ = app.feed_command(crate::command::StepInput::Point(DVec3::new(10., 0., 0.)));
        assert_eq!(
            line(&app.tabs[i].scene.document, insert),
            (DVec3::X * 8., DVec3::X * 9.)
        );
        assert_eq!(
            actions::instance::frame(&app.tabs[i].scene.document, insert, &point)
                .unwrap()
                .end,
            DVec3::X * 8.
        );
        app.undo_steps(1);
        assert_eq!(
            line(&app.tabs[i].scene.document, insert),
            (DVec3::X * (start + 8.), DVec3::X * (start + 10.))
        );
        assert_eq!(
            actions::instance::frame(&app.tabs[i].scene.document, insert, &point)
                .unwrap()
                .end,
            DVec3::X * 18.
        );
        app.redo_steps(1);
        assert_eq!(
            line(&app.tabs[i].scene.document, insert),
            (DVec3::X * 8., DVec3::X * 9.)
        );
        assert_eq!(
            actions::instance::frame(&app.tabs[i].scene.document, insert, &point)
                .unwrap()
                .end,
            DVec3::X * 8.
        );
        assert_eq!(line(&app.tabs[i].scene.document, other), original);
    }
}

#[test]
fn block_action_set_edits_point_targets_and_refreshes_reference_history() {
    let (mut app, insert, def, root, mut child, action) = setup_point_transport(false, true);
    let i = app.active_tab;
    let read_line = |doc: &codec::Document| {
        let EntityType::Insert(reference) = doc.get_entity(insert).unwrap() else {
            panic!()
        };
        doc.block_records
            .get(&reference.block_name)
            .unwrap()
            .entity_handles
            .iter()
            .find_map(|h| {
                if let Some(EntityType::Line(line)) = doc.get_entity(*h) {
                    Some(actions::v(line.start))
                } else {
                    None
                }
            })
            .unwrap()
    };
    let original = read_line(&app.tabs[i].scene.document);
    let EntityType::Insert(reference) = app.tabs[i].scene.document.get_entity(insert).unwrap()
    else {
        panic!()
    };
    let transform =
        crate::scene::render_graph::insert_transform(&app.tabs[i].scene.document, reference);
    let cursor = actions::v(transform.apply(actions::native(root.end + DVec3::new(3., 4., 0.))));
    let _ = app.feed_command(crate::command::StepInput::Point(cursor));
    let offset = actions::instance::frame(&app.tabs[i].scene.document, insert, &root)
        .unwrap()
        .end
        - root.end;
    assert!(read_line(&app.tabs[i].scene.document).distance(original + offset) < 1e-8);
    begin(&mut app, insert);
    child.name = "  Door \"A\"  ".to_owned();
    let codec::objects::ObjectType::DynamicBlock(object) = app.tabs[i]
        .scene
        .document
        .objects
        .get_mut(&child.handle)
        .unwrap()
    else {
        panic!()
    };
    let codec::objects::DynamicBlockData::PointParameter(parameter) = &mut object.data else {
        panic!()
    };
    parameter.position_name = child.name.clone();
    let member = members(&app)[0];
    edit_action_set(&mut app, action, "Add", vec![member]);
    assert!(read_line(&app.tabs[i].scene.document).distance(original + offset * 2.) < 1e-8);
    let linked = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let select_point = |app: &mut OpenCADStudio, mode: &str, entities: Vec<Handle>| {
        app.tabs[i].selected_authoring = Some(action);
        let _ = app.dispatch_command("BACTIONSET");
        feed(app, mode);
        feed(app, "Parameter missing");
        assert!(app.tabs[i].active_cmd.is_some());
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some(crate::tr!("block-action-targets", "unique-point").as_str())
        );
        feed(
            app,
            &format!("Parameter {}", serde_json::to_string(&child.name).unwrap()),
        );
        let _ = app.feed_command(crate::command::StepInput::SelectionComplete(entities));
        let _ = app.feed_command(crate::command::StepInput::Enter);
        assert!(app.tabs[i].active_cmd.is_none());
    };
    select_point(&mut app, "Remove", vec![]);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    assert!(read_line(&app.tabs[i].scene.document).distance(original + offset) < 1e-8);
    assert_eq!(
        actions::instance::frame(&app.tabs[i].scene.document, insert, &child)
            .unwrap()
            .end,
        child.end
    );
    app.undo_steps(1);
    assert!(read_line(&app.tabs[i].scene.document).distance(original + offset * 2.) < 1e-8);
    assert_eq!(
        app.tabs[i].scene.document.objects[&action],
        linked.objects[&action]
    );
    app.redo_steps(1);
    assert!(read_line(&app.tabs[i].scene.document).distance(original + offset) < 1e-8);
    select_point(&mut app, "Add", vec![]);
    assert!(read_line(&app.tabs[i].scene.document).distance(original + offset * 2.) < 1e-8);
    assert!(
        actions::instance::frame(&app.tabs[i].scene.document, insert, &child)
            .unwrap()
            .end
            .distance(child.end + offset)
            < 1e-8
    );
    select_point(&mut app, "New", vec![]);
    assert!(read_line(&app.tabs[i].scene.document).distance(original + offset) < 1e-8);
    app.undo_steps(1);
    assert!(read_line(&app.tabs[i].scene.document).distance(original + offset * 2.) < 1e-8);
    let unchanged_depth = app.tabs[i].history.undo_stack.len();
    select_point(&mut app, "New", vec![member]);
    assert_eq!(app.tabs[i].history.undo_stack.len(), unchanged_depth);
    assert!(read_line(&app.tabs[i].scene.document).distance(original + offset * 2.) < 1e-8);
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");
    for saved in [
        codec::DwgReader::from_stream(std::io::Cursor::new(
            codec::DwgWriter::write_to_vec(&app.tabs[i].scene.document).unwrap(),
        ))
        .read()
        .unwrap(),
        codec::DxfReader::from_reader(std::io::Cursor::new(
            codec::DxfWriter::new(&app.tabs[i].scene.document)
                .write_to_vec()
                .unwrap(),
        ))
        .unwrap()
        .read()
        .unwrap(),
    ] {
        assert!(read_line(&saved).distance(original + offset * 2.) < 1e-8);
        assert!(
            actions::instance::frame(&saved, insert, &child)
                .unwrap()
                .end
                .distance(child.end + offset)
                < 1e-8
        );
    }
    assert_eq!(
        app.tabs[i]
            .scene
            .document
            .block_records
            .get("EDIT_ME")
            .unwrap()
            .handle,
        def
    );
}

#[test]
fn block_action_set_rejects_ambiguous_points_and_transport_cycles_atomically() {
    let (mut app, insert, def, root, child, action) = setup_point_transport(false, true);
    let i = app.active_tab;
    let _ = app.update(Message::CommandEscape);
    begin(&mut app, insert);
    let _ = app.dispatch_command("BACTIONBARMODE 0");
    let duplicate = point::create(
        &mut app.tabs[i].scene.document,
        def,
        Vector3::UNIT_Y * 9.,
        Vector3::UNIT_Y * 10.,
    )
    .unwrap();
    let codec::objects::ObjectType::DynamicBlock(object) = app.tabs[i]
        .scene
        .document
        .objects
        .get_mut(&duplicate)
        .unwrap()
    else {
        panic!()
    };
    let codec::objects::DynamicBlockData::PointParameter(parameter) = &mut object.data else {
        panic!()
    };
    parameter.position_name = child.name.clone();
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    app.tabs[i].selected_authoring = Some(action);
    let _ = app.dispatch_command("BACTIONSET");
    assert!(
        app.tabs[i].active_cmd.is_some(),
        "cannot start: {:?}",
        app.command_line.last_error
    );
    feed(&mut app, "Add");
    assert!(
        app.tabs[i].active_cmd.is_some(),
        "cannot select mode: {:?}",
        app.command_line.last_error
    );
    feed(&mut app, &format!("Parameter {}", child.name));
    assert!(app.tabs[i].active_cmd.is_some());
    assert_eq!(
        app.command_line.last_error.as_deref(),
        Some(crate::tr!("block-action-targets", "unique-point").as_str())
    );
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    let _ = app.update(Message::CommandEscape);
    let child_action = actions::actions(&app.tabs[i].scene.document, def)
        .into_iter()
        .find(|(_, _, kind, id)| *kind == actions::Kind::Move && *id == child.id)
        .unwrap()
        .0;
    app.tabs[i].selected_authoring = Some(child_action);
    let _ = app.dispatch_command("BACTIONSET");
    assert!(
        app.tabs[i].active_cmd.is_some(),
        "cannot start: {:?}",
        app.command_line.last_error
    );
    feed(&mut app, "Add");
    assert!(
        app.tabs[i].active_cmd.is_some(),
        "cannot select mode: {:?}",
        app.command_line.last_error
    );
    feed(&mut app, &format!("Parameter {}", root.name));
    let _ = app.feed_command(crate::command::StepInput::SelectionComplete(vec![]));
    let _ = app.feed_command(crate::command::StepInput::Enter);
    assert!(app.tabs[i].active_cmd.is_some());
    assert_eq!(
        app.command_line.last_error.as_deref(),
        Some(crate::t!("The block action chain contains a cycle.").as_ref())
    );
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
}

#[test]
fn block_action_set_rejects_stale_point_name_choices() {
    let (mut app, insert, _, _, child, action) = setup_point_transport(false, true);
    let i = app.active_tab;
    let _ = app.update(Message::CommandEscape);
    begin(&mut app, insert);
    let _ = app.dispatch_command("BACTIONBARMODE 0");
    app.tabs[i].selected_authoring = Some(action);
    let _ = app.dispatch_command("BACTIONSET");
    feed(&mut app, "Remove");
    feed(&mut app, &format!("Parameter {}", child.name));
    let codec::objects::ObjectType::DynamicBlock(object) = app.tabs[i]
        .scene
        .document
        .objects
        .get_mut(&child.handle)
        .unwrap()
    else {
        panic!()
    };
    let codec::objects::DynamicBlockData::PointParameter(parameter) = &mut object.data else {
        panic!()
    };
    parameter.position_name = "Changed while selecting".to_owned();
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let _ = app.feed_command(crate::command::StepInput::SelectionComplete(vec![]));
    let _ = app.feed_command(crate::command::StepInput::Enter);
    assert_eq!(
        app.command_line.last_error.as_deref(),
        Some(
            crate::t!("The action or its Block Editor session changed; restart BACTIONSET.")
                .as_ref()
        )
    );
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
}

#[test]
fn block_move_creation_accepts_named_point_targets_and_undoes() {
    let (mut app, insert, def, root, child, _) = setup_point_transport(false, true);
    let _ = app.update(Message::CommandEscape);
    let i = app.active_tab;
    let offset = DVec3::new(3., 4., 0.);
    app.tabs[i].scene.document = actions::instance::evaluate_value(
        &app.tabs[i].scene.document,
        insert,
        root.handle,
        actions::Value::Point(root.end + offset),
    )
    .unwrap()
    .unwrap();
    begin(&mut app, insert);
    let before = app.tabs[i].scene.document.clone();
    let count = actions::actions(&before, def).len();
    let depth = app.tabs[i].history.undo_stack.len();
    let _ = app.dispatch_command("BACTION MOVE");
    feed(&mut app, &root.name);
    feed(&mut app, "Parameter missing");
    assert_eq!(
        app.command_line.last_error.as_deref(),
        Some(crate::tr!("block-action-targets", "unique-point").as_str())
    );
    feed(
        &mut app,
        &format!("Parameter {}", serde_json::to_string(&child.name).unwrap()),
    );
    let _ = app.feed_command(crate::command::StepInput::SelectionComplete(vec![]));
    let _ = app.feed_command(crate::command::StepInput::Enter);
    let _ = app.feed_command(crate::command::StepInput::Point(DVec3::new(2., 3., 0.)));
    assert!(app.tabs[i].active_cmd.is_none());
    let created = actions::actions(&app.tabs[i].scene.document, def);
    assert_eq!(created.len(), count + 1);
    assert!(created
        .iter()
        .any(|(_, action, _, _)| action.dependencies == vec![child.handle]));
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    assert!(
        actions::instance::frame(&app.tabs[i].scene.document, insert, &child)
            .unwrap()
            .end
            .distance(child.end + offset * 2.)
            < 1e-8
    );
    app.undo_steps(1);
    assert_eq!(
        actions::actions(&app.tabs[i].scene.document, def).len(),
        count
    );
    assert!(
        actions::instance::frame(&app.tabs[i].scene.document, insert, &child)
            .unwrap()
            .end
            .distance(child.end + offset)
            < 1e-8
    );
    app.redo_steps(1);
    assert_eq!(
        actions::actions(&app.tabs[i].scene.document, def).len(),
        count + 1
    );
    assert!(
        actions::instance::frame(&app.tabs[i].scene.document, insert, &child)
            .unwrap()
            .end
            .distance(child.end + offset * 2.)
            < 1e-8
    );
}

#[test]
fn block_move_creation_rejects_stale_point_choices() {
    let (mut app, insert, _, root, child, _) = setup_point_transport(false, true);
    let _ = app.update(Message::CommandEscape);
    begin(&mut app, insert);
    let i = app.active_tab;
    let _ = app.dispatch_command("BACTION MOVE");
    feed(&mut app, &root.name);
    feed(
        &mut app,
        &format!("Parameter {}", serde_json::to_string(&child.name).unwrap()),
    );
    let _ = app.feed_command(crate::command::StepInput::Enter);
    let codec::objects::ObjectType::DynamicBlock(object) = app.tabs[i]
        .scene
        .document
        .objects
        .get_mut(&child.handle)
        .unwrap()
    else {
        panic!()
    };
    let codec::objects::DynamicBlockData::PointParameter(point) = &mut object.data else {
        panic!()
    };
    point.position_name = "Changed".to_owned();
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let _ = app.feed_command(crate::command::StepInput::Point(DVec3::new(2., 3., 0.)));
    assert_eq!(
        app.command_line.last_error.as_deref(),
        Some(crate::tr!("block-action-targets", "create-stale").as_str())
    );
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
}

#[test]
fn block_move_point_targets_can_be_picked_in_the_viewport() {
    let (mut app, insert, def, root, child, existing) = setup_point_transport(false, true);
    let _ = app.update(Message::CommandEscape);
    begin(&mut app, insert);
    let i = app.active_tab;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (1600., 900.);
    app.tabs[i].scene.camera.borrow_mut().fit_to_bounds_f64(
        DVec3::new(-2., -2., 0.),
        DVec3::new(5., 10., 0.),
        1600. / 900.,
    );
    let behind = crate::command::PointPickContext {
        view: glam::camera::rh::proj::directx::perspective(
            45_f32.to_radians(),
            1600. / 900.,
            0.1,
            100.,
        ),
        eye: DVec3::new(0., 0., -10.),
        bounds: iced::Rectangle {
            x: 0.,
            y: 0.,
            width: 1600.,
            height: 900.,
        },
        aperture_px: 10.,
    };
    assert!(crate::modules::block_editor::action_set::pick_point_marker(
        &app.tabs[i].scene,
        def,
        |h| h == child.handle,
        iced::Point::new(800., 450.),
        behind,
    )
    .is_none());
    let click = |app: &mut OpenCADStudio, world: DVec3, shift: bool| {
        let scene = &app.tabs[app.active_tab].scene;
        let (vw, vh) = scene.selection.borrow().view.vp_size;
        let tile = scene.active_model_tile_bounds(vw, vh);
        let screen = scene
            .camera
            .borrow()
            .project(
                world,
                iced::Rectangle {
                    x: 0.,
                    y: 0.,
                    width: tile.width,
                    height: tile.height,
                },
            )
            .unwrap();
        app.shift_down = shift;
        let _ = app.update(Message::ViewportLeftPressAt(iced::Point::new(
            screen.x + tile.x,
            screen.y + tile.y,
        )));
        let _ = app.update(Message::ViewportLeftRelease);
        app.shift_down = false;
    };
    let _ = app.dispatch_command("BACTION MOVE");
    feed(&mut app, &root.name);
    click(&mut app, child.end, false);
    assert!(app.tabs[i]
        .active_cmd
        .as_ref()
        .unwrap()
        .selection_marker_selected(child.handle));
    assert!(!app.tabs[i]
        .active_cmd
        .as_ref()
        .unwrap()
        .selection_marker_selected(root.handle));
    click(&mut app, child.end, true);
    assert!(!app.tabs[i]
        .active_cmd
        .as_ref()
        .unwrap()
        .selection_marker_selected(child.handle));
    let _ = app.feed_command(crate::command::StepInput::Enter);
    assert!(app.tabs[i]
        .active_cmd
        .as_ref()
        .unwrap()
        .is_selection_gathering());
    click(&mut app, child.end, false);
    let _ = app.feed_command(crate::command::StepInput::Enter);
    let _ = app.feed_command(crate::command::StepInput::Point(DVec3::new(3., 3., 0.)));
    assert!(app.tabs[i].active_cmd.is_none());
    assert!(actions::actions(&app.tabs[i].scene.document, def)
        .iter()
        .any(|(handle, action, _, _)| *handle != existing
            && action.dependencies == vec![child.handle]));

    let geometry = members(&app)[0];
    edit_action_set(&mut app, existing, "Add", vec![geometry]);
    app.tabs[i].selected_authoring = Some(existing);
    let _ = app.dispatch_command("BACTIONSET");
    feed(&mut app, "Remove");
    click(&mut app, child.end, false);
    assert!(app.tabs[i]
        .active_cmd
        .as_ref()
        .unwrap()
        .selection_marker_selected(child.handle));
    let _ = app.feed_command(crate::command::StepInput::SelectionComplete(vec![]));
    let _ = app.feed_command(crate::command::StepInput::Enter);
    assert!(app.tabs[i].active_cmd.is_none());
    let action = actions::actions(&app.tabs[i].scene.document, def)
        .into_iter()
        .find(|(handle, _, _, _)| *handle == existing)
        .unwrap();
    assert!(action.1.dependencies.contains(&geometry));
    assert!(!action.1.dependencies.contains(&child.handle));
}

#[test]
fn block_move_point_targets_support_window_and_lasso_selection() {
    for (method, pick_add) in (0..3).flat_map(|method| [true, false].map(|add| (method, add))) {
        let lasso = method == 1;
        let (mut app, insert, def, root, child, existing) = setup_point_transport(false, true);
        let _ = app.update(Message::CommandEscape);
        let i = app.active_tab;
        let other = point::create(
            &mut app.tabs[i].scene.document,
            def,
            Vector3::new(3., 6., 0.),
            Vector3::new(3., 8., 0.),
        )
        .unwrap();
        let other_name = actions::all_parameters(&app.tabs[i].scene.document, def)
            .into_iter()
            .find(|p| p.handle == other)
            .unwrap()
            .name;
        begin(&mut app, insert);
        app.pick_add = pick_add;
        app.pick_drag_rect = method == 2;
        let i = app.active_tab;
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (1600., 900.);
        app.tabs[i].scene.camera.borrow_mut().fit_to_bounds_f64(
            DVec3::new(-2., -2., 0.),
            DVec3::new(5., 10., 0.),
            1600. / 900.,
        );
        let screen = |app: &OpenCADStudio, world: DVec3| {
            let scene = &app.tabs[app.active_tab].scene;
            let (vw, vh) = scene.selection.borrow().view.vp_size;
            let tile = scene.active_model_tile_bounds(vw, vh);
            let point = scene
                .camera
                .borrow()
                .project(
                    world,
                    iced::Rectangle {
                        x: 0.,
                        y: 0.,
                        width: tile.width,
                        height: tile.height,
                    },
                )
                .unwrap();
            iced::Point::new(point.x + tile.x, point.y + tile.y)
        };
        let bounds = iced::Rectangle {
            x: 0.,
            y: 0.,
            width: 1600.,
            height: 900.,
        };
        let camera = app.tabs[i].scene.camera.borrow();
        let context = crate::command::PointPickContext {
            view: camera.view_proj_rte(bounds),
            eye: camera.eye(),
            bounds,
            aperture_px: 10.,
        };
        let mid = screen(&app, DVec3::new(0., 6., 0.));
        let crossing = [
            iced::Point::new(mid.x - 4., mid.y - 4.),
            iced::Point::new(mid.x + 4., mid.y - 4.),
            iced::Point::new(mid.x + 4., mid.y + 4.),
            iced::Point::new(mid.x - 4., mid.y + 4.),
        ];
        let pick = |crossing_mode| {
            crate::modules::block_editor::action_set::point_markers_in_region(
                &app.tabs[i].scene,
                def,
                |h| h == child.handle,
                &crossing,
                context,
                crossing_mode,
            )
        };
        assert_eq!(pick(true).len(), 1);
        assert!(pick(false).is_empty());
        drop(camera);
        let before_count = actions::actions(&app.tabs[i].scene.document, def).len();
        let corners = [
            screen(&app, DVec3::new(-1., 4., 0.)),
            screen(&app, DVec3::new(1., 8., 0.)),
        ];
        let lo = iced::Point::new(
            corners[0].x.min(corners[1].x),
            corners[0].y.min(corners[1].y),
        );
        let hi = iced::Point::new(
            corners[0].x.max(corners[1].x),
            corners[0].y.max(corners[1].y),
        );
        let select = |app: &mut OpenCADStudio, shift: bool| {
            app.shift_down = shift;
            if lasso {
                let mut selection = app.tabs[i].scene.selection.borrow_mut();
                selection.input.left_down = true;
                selection.input.left_dragging = true;
                selection.input.last_move_pos = Some(lo);
                selection.gesture.poly_active = true;
                selection.gesture.poly_crossing = false;
                selection.gesture.poly_points = vec![
                    lo,
                    iced::Point::new(hi.x, lo.y),
                    hi,
                    iced::Point::new(lo.x, hi.y),
                ];
                drop(selection);
                let _ = app.update(Message::ViewportLeftRelease);
            } else if method == 2 {
                let _ = app.update(Message::ViewportLeftPressAt(lo));
                let _ = app.update(Message::ViewportMove(hi));
                assert!(app.tabs[i].scene.selection.borrow().input.left_dragging);
                let _ = app.update(Message::ViewportLeftRelease);
            } else {
                for p in [lo, hi] {
                    let _ = app.update(Message::ViewportLeftPressAt(p));
                    let _ = app.update(Message::ViewportLeftRelease);
                }
            }
            app.shift_down = false;
        };
        let _ = app.dispatch_command("BACTION MOVE");
        feed(&mut app, &root.name);
        if !pick_add {
            feed(
                &mut app,
                &format!("Parameter {}", serde_json::to_string(&other_name).unwrap()),
            );
            let geometry = members(&app)[0];
            app.tabs[i].scene.select_entities(&[geometry]);
            let _ = app.feed_command(crate::command::StepInput::SelectionComplete(vec![geometry]));
        }
        select(&mut app, false);
        let cmd = app.tabs[i].active_cmd.as_ref().unwrap();
        assert!(
            cmd.selection_marker_selected(child.handle),
            "method={method}, add={pick_add}"
        );
        assert!(
            !cmd.selection_marker_selected(other),
            "replaced Point method={method}"
        );
        assert!(
            app.tabs[i].scene.selected_entities().is_empty(),
            "replaced geometry method={method}"
        );
        select(&mut app, true);
        assert!(!app.tabs[i]
            .active_cmd
            .as_ref()
            .unwrap()
            .selection_marker_selected(child.handle));
        let _ = app.feed_command(crate::command::StepInput::Enter);
        assert!(
            app.tabs[i]
                .active_cmd
                .as_ref()
                .unwrap()
                .is_selection_gathering(),
            "removed region for lasso={lasso}"
        );
        select(&mut app, false);
        let _ = app.feed_command(crate::command::StepInput::Enter);
        let _ = app.feed_command(crate::command::StepInput::Point(DVec3::new(3., 3., 0.)));
        assert!(app.tabs[i].active_cmd.is_none(), "lasso={lasso}");
        assert_eq!(
            actions::actions(&app.tabs[i].scene.document, def).len(),
            before_count + 1
        );
        assert!(actions::actions(&app.tabs[i].scene.document, def)
            .iter()
            .any(|(handle, action, _, _)| *handle != existing
                && action.dependencies == vec![child.handle]));
        let geometry = members(&app)[0];
        edit_action_set(&mut app, existing, "Add", vec![geometry]);
        app.tabs[i].selected_authoring = Some(existing);
        let _ = app.dispatch_command("BACTIONSET");
        feed(&mut app, "Remove");
        select(&mut app, false);
        let _ = app.feed_command(crate::command::StepInput::Enter);
        let action = actions::actions(&app.tabs[i].scene.document, def)
            .into_iter()
            .find(|(handle, _, _, _)| *handle == existing)
            .unwrap();
        assert!(action.1.dependencies.contains(&geometry));
        assert!(!action.1.dependencies.contains(&child.handle));
    }
}

#[test]
fn block_move_marker_click_respects_pickadd_replacement() {
    let (mut app, insert, def, root, child, _) = setup_point_transport(false, true);
    let _ = app.update(Message::CommandEscape);
    let i = app.active_tab;
    let other = point::create(
        &mut app.tabs[i].scene.document,
        def,
        Vector3::new(3., 6., 0.),
        Vector3::new(3., 8., 0.),
    )
    .unwrap();
    begin(&mut app, insert);
    app.pick_add = false;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (1600., 900.);
    app.tabs[i].scene.camera.borrow_mut().fit_to_bounds_f64(
        DVec3::new(-2., -2., 0.),
        DVec3::new(6., 10., 0.),
        1600. / 900.,
    );
    let click = |app: &mut OpenCADStudio, world: DVec3| {
        let scene = &app.tabs[app.active_tab].scene;
        let (vw, vh) = scene.selection.borrow().view.vp_size;
        let tile = scene.active_model_tile_bounds(vw, vh);
        let screen = scene
            .camera
            .borrow()
            .project(
                world,
                iced::Rectangle {
                    x: 0.,
                    y: 0.,
                    width: tile.width,
                    height: tile.height,
                },
            )
            .unwrap();
        let _ = app.update(Message::ViewportLeftPressAt(iced::Point::new(
            screen.x + tile.x,
            screen.y + tile.y,
        )));
        let _ = app.update(Message::ViewportLeftRelease);
    };
    let _ = app.dispatch_command("BACTION MOVE");
    feed(&mut app, &root.name);
    let geometry = members(&app)[0];
    app.tabs[i].scene.select_entities(&[geometry]);
    let _ = app.feed_command(crate::command::StepInput::SelectionComplete(vec![geometry]));
    click(&mut app, child.end);
    assert!(app.tabs[i].scene.selected_entities().is_empty());
    assert!(app.tabs[i]
        .active_cmd
        .as_ref()
        .unwrap()
        .selection_marker_selected(child.handle));
    click(&mut app, DVec3::new(3., 6., 0.));
    let command = app.tabs[i].active_cmd.as_ref().unwrap();
    assert!(!command.selection_marker_selected(child.handle));
    assert!(command.selection_marker_selected(other));
    click(&mut app, DVec3::new(0.5, 0., 0.));
    assert!(!app.tabs[i]
        .active_cmd
        .as_ref()
        .unwrap()
        .selection_marker_selected(other));
    assert!(app.tabs[i].scene.selected.contains(&geometry));
    click(&mut app, DVec3::new(3., 6., 0.));
    assert!(app.tabs[i].scene.selected_entities().is_empty());
    let _ = app.feed_command(crate::command::StepInput::Enter);
    let _ = app.feed_command(crate::command::StepInput::Point(DVec3::new(3., 3., 0.)));
    assert!(app.tabs[i].active_cmd.is_none());
    assert!(actions::actions(&app.tabs[i].scene.document, def)
        .iter()
        .any(|(_, action, _, _)| action.dependencies == vec![other]));
}
