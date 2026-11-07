use super::*;
use crate::scene::{
    block_actions as actions, block_distance_parameter as distance,
    block_orientation_parameter as orientation,
};
use glam::DVec3;

#[test]
fn transported_flip_grip_uses_reference_frame_in_drawing_and_test_block() {
    for testing in [false, true] {
        let (mut app, reference) = fixture();
        let source = app.active_tab;
        begin(&mut app, reference);
        let definition = app.tabs[source]
            .active_block_edit_session()
            .unwrap()
            .br_handle;
        let member = members(&app)[0];
        let doc = &mut app.tabs[source].scene.document;
        let parent = distance::create(
            doc,
            definition,
            distance::Kind::Linear,
            Vector3::ZERO,
            Vector3::new(0., 10., 0.),
            Vector3::UNIT_X,
        )
        .unwrap();
        let flip = orientation::create(
            doc,
            definition,
            orientation::Kind::Flip,
            Vector3::new(2., 3., 0.),
            Vector3::new(12., 3., 0.),
            Vector3::new(4., 6., 0.),
        )
        .unwrap();
        let document::objects::ObjectType::DynamicBlock(o) = doc.objects.get_mut(&flip).unwrap()
        else {
            panic!()
        };
        let document::objects::DynamicBlockData::FlipParameter(p) = &mut o.data else {
            panic!()
        };
        p.parameter.parameter.chain_actions = true;
        actions::create(
            doc,
            definition,
            actions::Kind::Move,
            parent,
            &[flip],
            DVec3::ZERO,
        )
        .unwrap();
        actions::create(
            doc,
            definition,
            actions::Kind::Flip,
            flip,
            &[member],
            DVec3::ZERO,
        )
        .unwrap();
        let p = actions::all_parameters(doc, definition)
            .into_iter()
            .find(|p| p.handle == flip)
            .unwrap();
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
        app.tabs[i].scene.selected.insert(insert);
        app.tabs[i].selected_handle = Some(insert);
        app.refresh_properties();
        let grip = |app: &OpenCADStudio| {
            app.tabs[i]
                .selected_grips
                .iter()
                .find(|g| {
                    crate::scene::block_grips::runtime::decode(g.id)
                        .is_some_and(|(node, _)| node == p.id)
                })
                .unwrap()
                .clone()
        };
        let before = grip(&app);
        let doc = &app.tabs[i].scene.document;
        let input = actions::Value::Point(DVec3::new(0., 14., 0.));
        let changed = if testing {
            actions::instance::evaluate_test_value(
                doc,
                &app.tabs[i].block_test.as_ref().unwrap().baseline,
                definition,
                insert,
                parent,
                input,
            )
        } else {
            actions::instance::evaluate_value(doc, insert, parent, input)
        }
        .unwrap()
        .unwrap();
        app.tabs[i].scene.document = changed;
        app.tabs[i].scene.bump_geometry();
        app.refresh_properties();
        let moved = grip(&app);
        let EntityType::Insert(reference) = app.tabs[i].scene.document.get_entity(insert).unwrap()
        else {
            panic!()
        };
        let transform =
            crate::scene::render_graph::insert_transform(&app.tabs[i].scene.document, reference);
        let delta = actions::v(transform.apply_rotation(actions::native(DVec3::Y * 4.)));
        assert!((moved.world - before.world - delta).length() < 1e-8);
        assert!(app.start_test_action_grip(insert, moved.id));
        assert_eq!(
            actions::instance::value(&app.tabs[i].scene.document, insert, &p).unwrap(),
            actions::Value::Flip(true)
        );
        let toggled = grip(&app);
        assert!((toggled.world - moved.world).length() < 1e-8);
        assert!((toggled.dir.unwrap() + moved.dir.unwrap()).length() < 1e-6);
    }
}

#[test]
fn flipped_linear_grip_and_properties_follow_the_reflected_axis_with_history() {
    for testing in [false, true] {
        let (mut app, reference) = fixture();
        let source = app.active_tab;
        begin(&mut app, reference);
        let def = app.tabs[source]
            .active_block_edit_session()
            .unwrap()
            .br_handle;
        let member = members(&app)[0];
        let doc = &mut app.tabs[source].scene.document;
        let linear = distance::create(
            doc,
            def,
            distance::Kind::Linear,
            Vector3::new(0., 2., 0.),
            Vector3::new(10., 7., 0.),
            Vector3::new(5., 7., 0.),
        )
        .unwrap();
        let move_action = actions::create(
            doc,
            def,
            actions::Kind::Move,
            linear,
            &[member],
            DVec3::ZERO,
        )
        .unwrap();
        let flip = orientation::create(
            doc,
            def,
            orientation::Kind::Flip,
            Vector3::ZERO,
            Vector3::new(10., 0., 0.),
            Vector3::UNIT_Y,
        )
        .unwrap();
        let flip_action =
            actions::create(doc, def, actions::Kind::Flip, flip, &[member], DVec3::ZERO).unwrap();
        let document::objects::ObjectType::DynamicBlock(o) =
            doc.objects.get_mut(&flip_action).unwrap()
        else {
            panic!()
        };
        let document::objects::DynamicBlockData::FlipAction(action) = &mut o.data else {
            panic!()
        };
        action.action.dependencies.extend([linear, move_action]);
        let parameter = actions::all_parameters(doc, def)
            .into_iter()
            .find(|p| p.handle == linear)
            .unwrap();
        let flip_parameter = actions::all_parameters(doc, def)
            .into_iter()
            .find(|p| p.handle == flip)
            .unwrap();
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
        let select = |app: &mut OpenCADStudio| {
            app.tabs[i].scene.selected.insert(insert);
            app.tabs[i].selected_handle = Some(insert);
            app.refresh_properties();
        };
        let grip = |app: &OpenCADStudio, node| {
            app.tabs[i]
                .selected_grips
                .iter()
                .find(|g| {
                    crate::scene::block_grips::runtime::decode(g.id)
                        .is_some_and(|(id, _)| id == node)
                })
                .unwrap()
                .clone()
        };
        select(&mut app);
        let flip_grip = grip(&app, flip_parameter.id);
        assert!(app.start_test_action_grip(insert, flip_grip.id));
        assert_eq!(
            actions::instance::value(&app.tabs[i].scene.document, insert, &flip_parameter).unwrap(),
            actions::Value::Flip(true)
        );
        select(&mut app);
        let linear_grip = grip(&app, parameter.id);
        let before =
            actions::instance::frame(&app.tabs[i].scene.document, insert, &parameter).unwrap();
        assert_eq!(before.base, DVec3::new(0., -2., 0.));
        assert_eq!(before.end, DVec3::new(10., -7., 0.));
        let axis = (before.end - before.base).normalize();
        let direction = linear_grip.dir.unwrap().normalize();
        assert!(direction.dot(axis).abs() > 0.999);
        let EntityType::Insert(reference) = app.tabs[i].scene.document.get_entity(insert).unwrap()
        else {
            panic!()
        };
        let transform =
            crate::scene::render_graph::insert_transform(&app.tabs[i].scene.document, reference);
        let world_delta = actions::v(transform.apply_rotation(actions::native(axis * 5.)));
        assert!(app.start_test_action_grip(insert, linear_grip.id));
        let _ = app.feed_command(crate::command::StepInput::Point(
            linear_grip.world + world_delta,
        ));
        assert!(
            app.command_line.last_error.is_none(),
            "{:?}",
            app.command_line.last_error
        );
        let after =
            actions::instance::frame(&app.tabs[i].scene.document, insert, &parameter).unwrap();
        assert!((after.end - before.end - axis * 5.).length() < 1e-7);
        app.undo_steps(1);
        assert_eq!(
            actions::instance::frame(&app.tabs[i].scene.document, insert, &parameter).unwrap(),
            before
        );
        app.redo_steps(1);
        assert_eq!(
            actions::instance::frame(&app.tabs[i].scene.document, insert, &parameter).unwrap(),
            after
        );

        use crate::scene::model::object::{BlockParameterComponent, PropValue};
        select(&mut app);
        let key = app.tabs[i]
            .properties
            .sections
            .iter()
            .flat_map(|section| &section.props)
            .find_map(|property| match property.value {
                PropValue::BlockNumber { key, .. }
                    if key.parameter == linear
                        && key.component == BlockParameterComponent::Distance =>
                {
                    Some(key)
                }
                _ => None,
            })
            .expect("Linear distance in reference Properties");
        let set_distance = |app: &mut OpenCADStudio| {
            let _ = app.update(Message::PropBlockInput {
                key,
                value: "20".into(),
            });
            let _ = app.update(Message::PropBlockCommit(key));
        };
        let depth = app.tabs[i].history.undo_stack.len();
        set_distance(&mut app);
        assert!(
            app.command_line.last_error.is_none(),
            "{:?}",
            app.command_line.last_error
        );
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
        let check = |app: &OpenCADStudio, distance: f64| {
            let doc = &app.tabs[i].scene.document;
            let frame = actions::instance::frame(doc, insert, &parameter).unwrap();
            assert_eq!(frame.base, before.base);
            assert!(frame.end.distance(before.base + axis * distance) < 1e-7);
            let EntityType::Insert(reference) = doc.get_entity(insert).unwrap() else {
                panic!()
            };
            let line = doc
                .entities_in_block(&reference.block_name)
                .find_map(|entity| match entity {
                    EntityType::Line(line) => Some(line),
                    _ => None,
                })
                .unwrap();
            let expected = axis * (distance - before.end.distance(before.base));
            assert!(actions::v(line.start).distance(expected) < 1e-7);
        };
        check(&app, 20.);
        set_distance(&mut app);
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
        app.undo_steps(1);
        check(&app, after.end.distance(after.base));
        app.redo_steps(1);
        check(&app, 20.);
    }
}

#[test]
fn moving_flip_axis_updates_linked_grips_with_undo_and_redo() {
    for testing in [false, true] {
        for chained in [false, true] {
            let (mut app, reference) = fixture();
            let source = app.active_tab;
            begin(&mut app, reference);
            let def = app.tabs[source]
                .active_block_edit_session()
                .unwrap()
                .br_handle;
            let member = members(&app)[0];
            let doc = &mut app.tabs[source].scene.document;
            let child = distance::create(
                doc,
                def,
                distance::Kind::Linear,
                Vector3::new(0., 2., 0.),
                Vector3::new(10., 2., 0.),
                Vector3::new(5., 4., 0.),
            )
            .unwrap();
            let linked =
                actions::create(doc, def, actions::Kind::Move, child, &[member], DVec3::ZERO)
                    .unwrap();
            let flip = orientation::create(
                doc,
                def,
                orientation::Kind::Flip,
                Vector3::ZERO,
                Vector3::new(10., 0., 0.),
                Vector3::UNIT_Y,
            )
            .unwrap();
            let document::objects::ObjectType::DynamicBlock(o) = doc.objects.get_mut(&flip).unwrap()
            else {
                panic!()
            };
            let document::objects::DynamicBlockData::FlipParameter(p) = &mut o.data else {
                panic!()
            };
            p.parameter.parameter.chain_actions = chained;
            let flip_action =
                actions::create(doc, def, actions::Kind::Flip, flip, &[member], DVec3::ZERO)
                    .unwrap();
            let document::objects::ObjectType::DynamicBlock(o) =
                doc.objects.get_mut(&flip_action).unwrap()
            else {
                panic!()
            };
            let document::objects::DynamicBlockData::FlipAction(action) = &mut o.data else {
                panic!()
            };
            action.action.dependencies.extend([child, linked]);
            let parent = distance::create(
                doc,
                def,
                distance::Kind::Linear,
                Vector3::new(0., 10., 0.),
                Vector3::new(0., 20., 0.),
                Vector3::UNIT_X,
            )
            .unwrap();
            actions::create(doc, def, actions::Kind::Move, parent, &[flip], DVec3::ZERO).unwrap();
            let params = actions::all_parameters(doc, def);
            let child_p = params.iter().find(|p| p.handle == child).unwrap().clone();
            let flip_p = params.iter().find(|p| p.handle == flip).unwrap().clone();
            let parent_p = params.iter().find(|p| p.handle == parent).unwrap().clone();
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
            let doc = &app.tabs[i].scene.document;
            let input = actions::Value::Point(DVec3::new(0., 24., 0.));
            let changed = if testing {
                actions::instance::evaluate_test_value(
                    doc,
                    &app.tabs[i].block_test.as_ref().unwrap().baseline,
                    def,
                    insert,
                    parent,
                    input,
                )
            } else {
                actions::instance::evaluate_value(doc, insert, parent, input)
            }
            .unwrap()
            .unwrap();
            app.tabs[i].scene.document = changed;
            app.tabs[i].scene.bump_geometry();
            let select = |app: &mut OpenCADStudio| {
                app.tabs[i].scene.selected.insert(insert);
                app.tabs[i].selected_handle = Some(insert);
                app.refresh_properties();
            };
            let grip = |app: &OpenCADStudio, node| {
                app.tabs[i]
                    .selected_grips
                    .iter()
                    .find(|g| {
                        crate::scene::block_grips::runtime::decode(g.id)
                            .is_some_and(|(id, _)| id == node)
                    })
                    .unwrap()
                    .clone()
            };
            select(&mut app);
            let flip_grip = grip(&app, flip_p.id);
            assert!(app.start_test_action_grip(insert, flip_grip.id));
            assert_eq!(
                actions::instance::value(&app.tabs[i].scene.document, insert, &flip_p).unwrap(),
                actions::Value::Flip(true)
            );
            select(&mut app);
            let parent_grip = grip(&app, parent_p.id);
            let before =
                actions::instance::frame(&app.tabs[i].scene.document, insert, &child_p).unwrap();
            assert_eq!(before.base.y, if chained { 6. } else { -2. });
            let EntityType::Insert(reference) =
                app.tabs[i].scene.document.get_entity(insert).unwrap()
            else {
                panic!()
            };
            let transform = crate::scene::render_graph::insert_transform(
                &app.tabs[i].scene.document,
                reference,
            );
            let delta = actions::v(transform.apply_rotation(actions::native(DVec3::Y * 2.)));
            assert!(app.start_test_action_grip(insert, parent_grip.id));
            let _ = app.feed_command(crate::command::StepInput::Point(parent_grip.world + delta));
            assert!(
                app.command_line.last_error.is_none(),
                "{:?}",
                app.command_line.last_error
            );
            let after =
                actions::instance::frame(&app.tabs[i].scene.document, insert, &child_p).unwrap();
            assert_eq!(after.base.y, if chained { 10. } else { -2. });
            app.undo_steps(1);
            assert_eq!(
                actions::instance::frame(&app.tabs[i].scene.document, insert, &child_p).unwrap(),
                before
            );
            app.redo_steps(1);
            assert_eq!(
                actions::instance::frame(&app.tabs[i].scene.document, insert, &child_p).unwrap(),
                after
            );
        }
    }
}

fn click_flip(app: &mut OpenCADStudio, grip: &crate::scene::model::object::GripDef) {
    let scene = &app.tabs[app.active_tab].scene;
    let size = iced::Size::new(1600., 900.);
    let radius = DVec3::new(10., 10., 0.);
    scene.selection.borrow_mut().view.vp_size = (size.width, size.height);
    let mut camera = scene.camera.borrow_mut();
    camera.fit_to_bounds_f64(
        grip.world - radius,
        grip.world + radius,
        size.width / size.height,
    );
    let at = camera
        .project(grip.world, iced::Rectangle::with_size(size))
        .unwrap();
    drop(camera);
    let _ = app.update(Message::ViewportLeftPressAt(iced::Point::new(at.x, at.y)));
    let _ = app.update(Message::ViewportLeftRelease);
    assert!(app.tabs[app.active_tab].active_cmd.is_none());
    assert!(app.grip_popup.is_none());
    assert!(app.visibility_popup.is_none());
    assert!(app.table_popup.is_none());
}

fn coupled_flip_document() -> (document::Document, Handle, Handle) {
    let (mut app, insert) = fixture();
    let i = app.active_tab;
    begin(&mut app, insert);
    let def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    circle(&mut app, 8.);
    let targets = members(&app);
    let doc = &mut app.tabs[i].scene.document;
    let linear = distance::create(
        doc,
        def,
        distance::Kind::Linear,
        Vector3::ZERO,
        Vector3::new(10., 0., 0.),
        Vector3::new(5., -4., 0.),
    )
    .unwrap();
    crate::scene::block_grips::config::set(doc, def, linear, Some(1)).unwrap();
    let flip_v = orientation::create(
        doc,
        def,
        orientation::Kind::Flip,
        Vector3::new(5., 0., 0.),
        Vector3::new(5., 10., 0.),
        Vector3::new(5., 12., 0.),
    )
    .unwrap();
    let flip_h = orientation::create(
        doc,
        def,
        orientation::Kind::Flip,
        Vector3::new(0., 2., 0.),
        Vector3::new(10., 2., 0.),
        Vector3::new(12., 2., 0.),
    )
    .unwrap();
    actions::create_with_options(
        doc,
        def,
        actions::Kind::Stretch,
        linear,
        Some(actions::XyKeyPoint::End),
        &targets,
        DVec3::ZERO,
        Some([DVec3::new(0.5, -5., 0.), DVec3::new(12., 5., 0.)]),
        None,
    )
    .unwrap();
    let mut flip_targets = targets;
    flip_targets.push(linear);
    actions::create(doc, def, actions::Kind::Flip, flip_v, &flip_targets, DVec3::ZERO).unwrap();
    actions::create(doc, def, actions::Kind::Flip, flip_h, &flip_targets, DVec3::ZERO).unwrap();
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");
    (app.tabs[i].scene.document.clone(), insert, def)
}

#[test]
fn coupled_flip_viewport_click_all_masks_and_orders_recover_refresh_and_reset() {
    let (doc, insert, def) = coupled_flip_document();
    let flips: Vec<_> = actions::all_parameters(&doc, def)
        .into_iter()
        .filter(|p| p.flip && actions::has_action(&doc, def, p.handle))
        .collect();
    assert_eq!(flips.len(), 2);
    for mask in 1..4 {
        for reverse in [false, true] {
            if (mask == 1 || mask == 2) && reverse {
                continue;
            }
            let mut app = OpenCADStudio::new_for_test();
            let i = app.active_tab;
            app.tabs[i].scene.document = doc.clone();
            let select = |app: &mut OpenCADStudio| {
                app.tabs[i].scene.selected.insert(insert);
                app.tabs[i].selected_handle = Some(insert);
                app.refresh_properties();
            };
            let mut order = flips.clone();
            if reverse {
                order.reverse();
            }
            for p in &order {
                let position = flips.iter().position(|f| f.handle == p.handle).unwrap();
                if mask & (1 << position) == 0 {
                    continue;
                }
                select(&mut app);
                let grip = app.tabs[i]
                    .selected_grips
                    .iter()
                    .find(|g| {
                        crate::scene::block_grips::runtime::decode(g.id)
                            .is_some_and(|(id, _)| id == p.id)
                    })
                    .expect("Flip grip must be displayed")
                    .clone();
                assert_eq!(grip.shape, crate::scene::model::object::GripShape::Arrow);
                click_flip(&mut app, &grip);
                assert!(
                    app.command_line.last_error.is_none(),
                    "Flip {}: {:?}",
                    p.id,
                    app.command_line.last_error
                );
                assert_eq!(
                    actions::instance::value(&app.tabs[i].scene.document, insert, p).unwrap(),
                    actions::Value::Flip(true)
                );
                let current = &app.tabs[i].scene.document;
                let EntityType::Insert(reference) = current.get_entity(insert).unwrap() else {
                    panic!()
                };
                let record = current.block_records.get(&reference.block_name).unwrap();
                let Some(EntityType::Block(marker)) =
                    current.get_entity(record.block_entity_handle)
                else {
                    panic!("The evaluated block must retain its BLOCK marker");
                };
                assert_eq!(marker.base_point, record.base_point);
                select(&mut app);
                let reversed = app.tabs[i]
                    .selected_grips
                    .iter()
                    .find(|marker| marker.id == grip.id)
                    .expect("Flip arrow must remain visible after toggling");
                assert!((reversed.dir.unwrap() + grip.dir.unwrap()).length() < 1e-8);
            }
            actions::instance::clear_canonical_inputs(&mut app.tabs[i].scene.document, insert);
            select(&mut app);
            let params = actions::all_parameters(&app.tabs[i].scene.document, def);
            let (grip, p) = app.tabs[i]
                .selected_grips
                .iter()
                .find_map(|g| {
                    let (id, _) = crate::scene::block_grips::runtime::decode(g.id)?;
                    let p = params.iter().find(|p| p.id == id && p.linear)?;
                    Some((g.clone(), p.clone()))
                })
                .expect("Linear grip must be displayed");
            let frame = actions::instance::frame(&app.tabs[i].scene.document, insert, &p).unwrap();
            let EntityType::Insert(reference) =
                app.tabs[i].scene.document.get_entity(insert).unwrap()
            else {
                panic!()
            };
            let transform = crate::scene::render_graph::insert_transform(
                &app.tabs[i].scene.document,
                reference,
            );
            let delta = actions::v(
                transform
                    .apply_rotation(actions::native((frame.end - frame.base).normalize() * 3.)),
            );
            assert!(app.start_test_action_grip(insert, grip.id));
            let _ = app.feed_command(crate::command::StepInput::Point(grip.world + delta));
            assert!(
                app.command_line.last_error.is_none(),
                "Linear {}: {:?}",
                p.id,
                app.command_line.last_error
            );
            let changed =
                actions::instance::frame(&app.tabs[i].scene.document, insert, &p).unwrap();
            assert!(changed.end.distance(frame.end) > 1e-6);
            assert!(
                (changed.end.distance(changed.base) - frame.end.distance(frame.base) - 3.).abs()
                    < 1e-6
            );
            let current = &app.tabs[i].scene.document;
            let refreshed = actions::instance::refresh(current, def).unwrap();
            for parameter in &params {
                assert_eq!(
                    actions::instance::frame(&refreshed, insert, parameter).unwrap(),
                    actions::instance::frame(current, insert, parameter).unwrap(),
                );
                assert_eq!(
                    actions::instance::value(&refreshed, insert, parameter).unwrap(),
                    actions::instance::value(current, insert, parameter).unwrap(),
                );
            }
            app.undo_steps(1);
            assert_eq!(
                actions::instance::frame(&app.tabs[i].scene.document, insert, &p).unwrap(),
                frame
            );
            app.redo_steps(1);
            assert_eq!(
                actions::instance::frame(&app.tabs[i].scene.document, insert, &p).unwrap(),
                changed
            );
            for extension in ["dwg", "dxf"] {
                let current = &app.tabs[i].scene.document;
                let bytes = crate::io::save_to_bytes(current, extension, current.version).unwrap();
                let saved = crate::io::load_bytes(&format!("coupled-reference.{extension}"), bytes)
                    .unwrap();
                assert_eq!(
                    actions::instance::frame(&saved, insert, &p).unwrap(),
                    changed
                );
                let mut reopened = OpenCADStudio::new_for_test();
                let j = reopened.active_tab;
                reopened.tabs[j].scene.document = saved.clone();
                for flip in order.iter().rev() {
                    let position = flips.iter().position(|f| f.handle == flip.handle).unwrap();
                    if mask & (1 << position) == 0 {
                        continue;
                    }
                    reopened.tabs[j].scene.selected.insert(insert);
                    reopened.tabs[j].selected_handle = Some(insert);
                    reopened.refresh_properties();
                    let grip = reopened.tabs[j]
                        .selected_grips
                        .iter()
                        .find(|g| {
                            crate::scene::block_grips::runtime::decode(g.id)
                                .is_some_and(|(id, _)| id == flip.id)
                        })
                        .unwrap()
                        .clone();
                    click_flip(&mut reopened, &grip);
                    assert!(
                        reopened.command_line.last_error.is_none(),
                        "reopened Flip {}: {:?}",
                        flip.id,
                        reopened.command_line.last_error
                    );
                    assert_eq!(
                        actions::instance::value(&reopened.tabs[j].scene.document, insert, flip)
                            .unwrap(),
                        actions::Value::Flip(false)
                    );
                }
                let unflipped =
                    actions::instance::frame(&reopened.tabs[j].scene.document, insert, &p).unwrap();
                assert!(
                    (unflipped.end.distance(unflipped.base) - changed.end.distance(changed.base))
                        .abs()
                        < 1e-6
                );
                let reset = actions::instance::reset(&saved, insert).unwrap().unwrap();
                for original in &params {
                    assert_eq!(
                        actions::instance::frame(&reset, insert, original).unwrap(),
                        actions::instance::ParameterFrame {
                            base: original.base,
                            end: original.end
                        }
                    );
                }
            }
        }
    }
}

