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
        let codec::objects::ObjectType::DynamicBlock(o) = doc.objects.get_mut(&flip).unwrap()
        else {
            panic!()
        };
        let codec::objects::DynamicBlockData::FlipParameter(p) = &mut o.data else {
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
fn flipped_linear_grip_edits_along_its_reflected_axis_with_history() {
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
        let codec::objects::ObjectType::DynamicBlock(o) =
            doc.objects.get_mut(&flip_action).unwrap()
        else {
            panic!()
        };
        let codec::objects::DynamicBlockData::FlipAction(action) = &mut o.data else {
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
            let codec::objects::ObjectType::DynamicBlock(o) = doc.objects.get_mut(&flip).unwrap()
            else {
                panic!()
            };
            let codec::objects::DynamicBlockData::FlipParameter(p) = &mut o.data else {
                panic!()
            };
            p.parameter.parameter.chain_actions = chained;
            let flip_action =
                actions::create(doc, def, actions::Kind::Flip, flip, &[member], DVec3::ZERO)
                    .unwrap();
            let codec::objects::ObjectType::DynamicBlock(o) =
                doc.objects.get_mut(&flip_action).unwrap()
            else {
                panic!()
            };
            let codec::objects::DynamicBlockData::FlipAction(action) = &mut o.data else {
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
