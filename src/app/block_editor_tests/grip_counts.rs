use super::*;

#[test]
fn block_grip_count_command_and_properties_are_atomic_and_undoable() {
    use crate::scene::block_grips::config;
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER XY 0,0 10,10 5,5");
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let parameter =
        crate::scene::block_actions::all_parameters(&app.tabs[i].scene.document, definition)[0]
            .handle;
    app.tabs[i].selected_authoring = Some(parameter);
    app.refresh_properties();
    assert!(app.tabs[i]
        .properties
        .sections
        .iter()
        .flat_map(|s| &s.props)
        .any(|p| p.field == "authoring_grip_count"));
    let initial = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let _ = app.run_command_line("BGRIPSET 4");
    assert_eq!(
        config::count(&app.tabs[i].scene.document, definition, parameter),
        Some(4)
    );
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    let changed = app.tabs[i].scene.document.clone();
    let _ = app.update(Message::PropGeomInput {
        field: "authoring_grip_count",
        value: "3".into(),
    });
    let _ = app.update(Message::PropGeomCommit("authoring_grip_count"));
    assert_eq!(app.tabs[i].scene.document, changed);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    let _ = app.update(Message::PropGeomInput {
        field: "authoring_grip_count",
        value: "0".into(),
    });
    let _ = app.update(Message::PropGeomCommit("authoring_grip_count"));
    assert_eq!(
        config::count(&app.tabs[i].scene.document, definition, parameter),
        Some(0)
    );
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 2);
    app.undo_steps(2);
    assert_eq!(app.tabs[i].scene.document.objects, initial.objects);
    app.redo_steps(2);
    assert_eq!(
        config::count(&app.tabs[i].scene.document, definition, parameter),
        Some(0)
    );
    let _ = app.dispatch_command("BGRIPSET");
    let before = app.tabs[i].scene.document.clone();
    let _ = app.update(Message::CommandEscape);
    assert_eq!(app.tabs[i].scene.document, before);
    let _ = app.run_command_line("BGRIPSET R");
    assert_eq!(app.tabs[i].scene.document, before);
    let _ = app.dispatch_command("BEDIT_DISCARD");
    let before = app.tabs[i].scene.document.clone();
    let _ = app.dispatch_command("BGRIPSET");
    assert!(app.tabs[i].active_cmd.is_none());
    assert_eq!(app.tabs[i].scene.document, before);
}

#[test]
fn block_grip_count_command_rejects_stale_native_snapshot() {
    use crate::command::CadCommand;
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let parameter =
        crate::scene::block_actions::all_parameters(&app.tabs[i].scene.document, definition)[0]
            .handle;
    let mut command = crate::modules::block_editor::grip_set::GripSetCommand::new(
        app.tabs[i].id,
        definition,
        &app.tabs[i].scene.document,
        Some(parameter),
    )
    .unwrap();
    let result = command.on_text_input("2").unwrap();
    crate::scene::block_grips::config::set(
        &mut app.tabs[i].scene.document,
        definition,
        parameter,
        Some(0),
    )
    .unwrap();
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let _ = app.set_block_parameter_grips(result);
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
}

#[test]
fn block_grip_count_hides_and_restores_native_visibility_grip_after_save() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER VISIBILITY 3,4");
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let parameter =
        crate::scene::block_visibility::parameter(&app.tabs[i].scene.document, definition)
            .unwrap()
            .handle;
    for desired in [0, 1] {
        app.tabs[i].selected_authoring = Some(parameter);
        let _ = app.run_command_line(&format!("BGRIPSET {desired}"));
        assert_eq!(
            crate::scene::block_grips::config::count(
                &app.tabs[i].scene.document,
                definition,
                parameter
            ),
            Some(desired)
        );
        let _ = app.dispatch_command("BSAVE");
        let _ = app.dispatch_command("BCLOSE");
        assert!(app.tabs[i].active_block_edit_session().is_none());
        app.tabs[i].selected_handle = Some(reference);
        app.tabs[i].selected_grips.clear();
        app.tabs[i].selected_grip_handles.clear();
        app.refresh_visibility_grip([0.; 3]);
        assert_eq!(app.tabs[i].visibility_grip.is_some(), desired > 0);
        if desired == 0 {
            begin(&mut app, reference);
        }
    }
}

#[test]
fn block_grip_locations_edit_undo_and_drag_in_transformed_references_and_test_block() {
    use crate::scene::{block_actions as actions, block_grips::config};
    use glam::DVec3;
    for testing in [false, true] {
        let (mut app, reference) = fixture();
        begin(&mut app, reference);
        let source = app.active_tab;
        let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2 1");
        let definition = app.tabs[source]
            .active_block_edit_session()
            .unwrap()
            .br_handle;
        let parameter =
            actions::all_parameters(&app.tabs[source].scene.document, definition)[0].handle;
        let targets = members(&app);
        actions::create(
            &mut app.tabs[source].scene.document,
            definition,
            actions::Kind::Move,
            parameter,
            &targets,
            DVec3::ZERO,
        )
        .unwrap();
        app.tabs[source].selected_authoring = Some(parameter);
        app.refresh_properties();
        let parameter_before = app.tabs[source].scene.document.objects[&parameter].clone();
        let depth = app.tabs[source].history.undo_stack.len();
        for (field, value) in [
            ("authoring_display_grip_end_x", "20+5"),
            ("authoring_display_grip_end_y", "7"),
        ] {
            let _ = app.update(Message::PropGeomInput {
                field,
                value: value.into(),
            });
            let _ = app.update(Message::PropGeomCommit(field));
        }
        assert_eq!(
            app.tabs[source].scene.document.objects[&parameter],
            parameter_before
        );
        assert_eq!(
            config::locations(&app.tabs[source].scene.document, definition, parameter).unwrap()[0]
                .point,
            Vector3::new(25., 7., 0.)
        );
        assert_eq!(app.tabs[source].history.undo_stack.len(), depth + 2);
        let before = app.tabs[source].scene.document.clone();
        let _ = app.update(Message::PropGeomInput {
            field: "authoring_display_grip_end_y",
            value: "NaN".into(),
        });
        let _ = app.update(Message::PropGeomCommit("authoring_display_grip_end_y"));
        assert_eq!(app.tabs[source].scene.document, before);
        app.undo_steps(2);
        assert_eq!(
            config::locations(&app.tabs[source].scene.document, definition, parameter).unwrap()[0]
                .point,
            Vector3::new(10., 0., 0.)
        );
        app.redo_steps(2);
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
            .map(|test| test.insert)
            .unwrap_or(reference);
        let EntityType::Insert(native) = app.tabs[i].scene.document.get_entity_mut(insert).unwrap()
        else {
            panic!()
        };
        native.insert_point = Vector3::new(100., 200., 0.);
        native.rotation = std::f64::consts::FRAC_PI_2;
        native.set_x_scale(2.);
        native.set_y_scale(2.);
        native.set_z_scale(2.);
        app.tabs[i].scene.select_entity(insert, true);
        app.tabs[i].selected_handle = Some(insert);
        app.tabs[i].selected_grips.clear();
        app.tabs[i].selected_grip_handles.clear();
        app.refresh_test_action_grips([0.; 3]);
        let id = crate::scene::block_grips::runtime::id(
            actions::all_parameters(&app.tabs[i].scene.document, definition)[0].id,
            1,
        )
        .unwrap();
        let displayed = app.tabs[i]
            .selected_grips
            .iter()
            .find(|grip| grip.id == id)
            .unwrap()
            .world;
        assert!(displayed.distance(DVec3::new(86., 250., 0.)) < 1e-8);
        let grip = config::locations(&app.tabs[i].scene.document, definition, parameter).unwrap()
            [0]
        .handle;
        let grip_before = app.tabs[i].scene.document.objects[&grip].clone();
        assert!(app.start_test_action_grip(insert, id));
        let before = app.tabs[i].scene.document.clone();
        let _ = app.update(Message::CommandEscape);
        assert_eq!(app.tabs[i].scene.document, before);
        app.tabs[i].scene.select_entity(insert, true);
        app.tabs[i].selected_handle = Some(insert);
        assert!(app.start_test_action_grip(insert, id));
        let result = app.tabs[i]
            .active_cmd
            .as_mut()
            .unwrap()
            .on_point(DVec3::new(86., 260., 0.));
        let _ = app.handle_block_action_result(result);
        let p = actions::all_parameters(&app.tabs[i].scene.document, definition).remove(0);
        let value = if testing {
            actions::value(&app.tabs[i].scene.document, &p)
        } else {
            actions::instance::value(&app.tabs[i].scene.document, insert, &p)
        }
        .unwrap();
        assert!(
            matches!(value, actions::Value::Point(point) if point.distance(DVec3::new(15.,0.,0.)) < 1e-8)
        );
        assert_eq!(app.tabs[i].scene.document.objects[&grip], grip_before);
        let EntityType::Insert(native) = app.tabs[i].scene.document.get_entity(insert).unwrap()
        else {
            panic!()
        };
        let line = app.tabs[i]
            .scene
            .document
            .block_records
            .get(&native.block_name)
            .unwrap()
            .entity_handles
            .iter()
            .find_map(
                |handle| match app.tabs[i].scene.document.get_entity(*handle) {
                    Some(EntityType::Line(line)) => Some(line),
                    _ => None,
                },
            )
            .unwrap();
        assert!(line.start.distance(&Vector3::new(5., 0., 0.)) < 1e-8);
    }
}
