use super::*;

#[test]
fn block_actions_rotate_flip_mouse_grips_authoring_history_and_reopen() {
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let source = app.active_tab;
    let _ = app.run_command_line("BPARAMETER ROTATION 0,0 10 0 5,5");
    let _ = app.run_command_line("BPARAMETER FLIP 0,0 10,0 -2,2");
    let def = app.tabs[source]
        .active_block_edit_session()
        .unwrap()
        .br_handle;
    let rotate =
        actions::parameters(&app.tabs[source].scene.document, def, Kind::Rotate)[0].clone();
    let flip = actions::parameters(&app.tabs[source].scene.document, def, Kind::Flip)[0].clone();
    let flip_at = actions::grip_position(&flip, actions::Value::Flip(false));
    assert!(flip_at.distance(flip.base) < 1e-8);
    let targets = members(&app);
    for (kind, p) in [("Rotate", &rotate), ("Flip", &flip)] {
        let depth = app.tabs[source].history.undo_stack.len();
        let _ = app.dispatch_command(&format!("BACTION {kind}"));
        feed(&mut app, &p.name);
        let _ = app.feed_command(crate::command::StepInput::SelectionComplete(
            targets.clone(),
        ));
        let _ = app.feed_command(crate::command::StepInput::Enter);
        feed(&mut app, "3,4");
        assert!(app.tabs[source].active_cmd.is_none());
        assert_eq!(app.tabs[source].history.undo_stack.len(), depth + 1);
    }
    let baseline = app.tabs[source].scene.document.clone();
    let click = |app: &mut OpenCADStudio, world: glam::DVec3| {
        let scene = &app.tabs[app.active_tab].scene;
        let (w, h) = scene.selection.borrow().view.vp_size;
        let tile = scene.active_model_tile_bounds(w, h);
        let at = scene
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
            at.x + tile.x,
            at.y + tile.y,
        )));
        let _ = app.update(Message::ViewportLeftRelease);
    };
    let check = |app: &OpenCADStudio, insert: Handle, expected: Vector3| {
        let doc = &app.tabs[app.active_tab].scene.document;
        let EntityType::Insert(ins) = doc.get_entity(insert).unwrap() else {
            panic!()
        };
        let h = doc
            .block_records
            .get(&ins.block_name)
            .unwrap()
            .entity_handles[0];
        let EntityType::Line(line) = doc.get_entity(h).unwrap() else {
            panic!()
        };
        assert!(
            line.end.distance(&expected) < 1e-5,
            "{:?} != {expected:?}",
            line.end
        );
    };
    // Exercise the exact viewport grip press/release path in both contexts.
    for test in [true, false] {
        let insert = if test {
            let _ = app.dispatch_command("BTESTBLOCK");
            app.tabs[app.active_tab].block_test.as_ref().unwrap().insert
        } else {
            app.active_tab = source;
            let _ = app.dispatch_command("BSAVE");
            let _ = app.dispatch_command("BEDIT_DISCARD");
            reference
        };
        let i = app.active_tab;
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (1600., 900.);
        app.tabs[i].scene.camera.borrow_mut().fit_to_bounds_f64(
            glam::DVec3::new(-4., -3., 0.),
            glam::DVec3::new(13., 13., 0.),
            1600. / 900.,
        );
        app.tabs[i].scene.selected.insert(insert);
        app.refresh_properties();
        click(&mut app, rotate.end);
        assert!(app.tabs[i].active_cmd.is_some());
        let depth = app.tabs[i].history.undo_stack.len();
        click(&mut app, glam::DVec3::new(0., 10., 0.));
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
        check(&app, insert, Vector3::UNIT_Y);
        app.tabs[i].scene.selected.insert(insert);
        app.refresh_properties();
        let depth = app.tabs[i].history.undo_stack.len();
        let flip_grip = app.tabs[i]
            .selected_grips
            .iter()
            .find(|g| super::super::block_actions::is_test_grip(g.id) && g.world.distance(flip_at) < 1e-5)
            .unwrap();
        assert_eq!(
            flip_grip.shape,
            crate::scene::model::object::GripShape::Arrow
        );
        assert!(flip_grip.dir.unwrap().distance(glam::DVec3::Y) < 1e-5);
        click(&mut app, flip_at);
        assert!(
            app.tabs[i].active_cmd.is_none(),
            "the Flip grip never asks for point input"
        );
        assert!(app.grip_popup.is_none());
        check(&app, insert, -Vector3::UNIT_Y);
        app.tabs[i].scene.selected.insert(insert);
        app.refresh_properties();
        let flip_grip = app.tabs[i]
            .selected_grips
            .iter()
            .find(|g| super::super::block_actions::is_test_grip(g.id) && g.world.distance(flip_at) < 1e-5)
            .unwrap();
        assert!(flip_grip.dir.unwrap().distance(-glam::DVec3::Y) < 1e-5);
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
        app.undo_steps(1);
        check(&app, insert, Vector3::UNIT_Y);
        app.redo_steps(1);
        check(&app, insert, -Vector3::UNIT_Y);
        if test {
            assert_eq!(app.tabs[source].scene.document, baseline);
            let _ = app.dispatch_command("BCLOSETESTBLOCK");
        } else {
            let bytes = document::DwgWriter::write_to_vec(&app.tabs[i].scene.document).unwrap();
            app.tabs[i].scene.document =
                document::DwgReader::from_stream(std::io::Cursor::new(bytes))
                    .read()
                    .unwrap();
            app.tabs[i].scene.selected.insert(insert);
            app.refresh_properties();
            click(&mut app, flip_at);
            check(&app, insert, Vector3::UNIT_Y);
            let EntityType::Line(original) =
                app.tabs[i].scene.document.get_entity(targets[0]).unwrap()
            else {
                panic!()
            };
            assert_eq!(original.end, Vector3::UNIT_X);
        }
    }
}

#[test]
fn block_actions_drawing_grips_history_cancel_transform_and_reopen() {
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
    let def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let p = actions::parameters(&app.tabs[i].scene.document, def, Kind::Move)[0].clone();
    let targets = members(&app);
    actions::create(
        &mut app.tabs[i].scene.document,
        def,
        Kind::Move,
        p.handle,
        &targets,
        glam::DVec3::ZERO,
    )
    .unwrap();
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BEDIT_DISCARD");
    let sibling = app.tabs[i]
        .scene
        .document
        .add_entity(EntityType::Insert(Insert::new(
            "EDIT_ME",
            Vector3::new(40., 0., 0.),
        )))
        .unwrap();
    let EntityType::Insert(ins) = app.tabs[i]
        .scene
        .document
        .get_entity_mut(reference)
        .unwrap()
    else {
        panic!()
    };
    ins.insert_point = Vector3::new(20., 30., 0.);
    ins.rotation = std::f64::consts::FRAC_PI_2;
    ins.set_x_scale(2.);
    ins.set_y_scale(2.);
    ins.set_z_scale(2.);
    let original = app.tabs[i].scene.document.clone();
    app.tabs[i].scene.selected.insert(reference);
    app.tabs[i].selected_handle = Some(reference);
    app.refresh_properties();
    let grip = app.tabs[i]
        .selected_grips
        .iter()
        .find(|g| super::super::block_actions::is_test_grip(g.id))
        .unwrap()
        .clone();
    assert!(grip.world.distance(glam::DVec3::new(20., 50., 0.)) < 1e-8);
    let depth = app.tabs[i].history.undo_stack.len();
    assert!(app.start_test_action_grip(reference, grip.id));
    let _ = app.update(Message::CommandEscape);
    assert_eq!(app.tabs[i].scene.document, original);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    // Escape also clears the ordinary drawing selection.
    app.tabs[i].scene.selected.insert(reference);
    app.refresh_properties();
    assert!(app.start_test_action_grip(reference, grip.id));
    feed(&mut app, "20,70");
    assert!(app.tabs[i].active_cmd.is_none());
    let changed = app.tabs[i].scene.document.clone();
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    let EntityType::Insert(ins) = changed.get_entity(reference).unwrap() else {
        panic!()
    };
    assert_ne!(ins.block_name, "EDIT_ME");
    let member = changed
        .block_records
        .get(&ins.block_name)
        .unwrap()
        .entity_handles[0];
    let EntityType::Line(line) = changed.get_entity(member).unwrap() else {
        panic!()
    };
    assert!((line.start.x - 10.).abs() < 1e-8);
    assert_eq!(
        changed.get_entity(targets[0]),
        original.get_entity(targets[0])
    );
    assert_eq!(changed.get_entity(sibling), original.get_entity(sibling));
    app.undo_steps(1);
    assert_eq!(
        app.tabs[i].scene.document.get_entity(reference),
        original.get_entity(reference)
    );
    assert!(app.tabs[i].scene.document.get_entity(member).is_none());
    app.redo_steps(1);
    assert_eq!(
        app.tabs[i].scene.document.get_entity(member),
        changed.get_entity(member)
    );
    assert_eq!(
        app.tabs[i].scene.document.get_entity(reference),
        changed.get_entity(reference)
    );
    let bytes = document::DwgWriter::write_to_vec(&app.tabs[i].scene.document).unwrap();
    app.tabs[i].scene.document = document::DwgReader::from_stream(std::io::Cursor::new(bytes))
        .read()
        .unwrap();
    app.refresh_properties();
    let grip = app.tabs[i]
        .selected_grips
        .iter()
        .find(|g| super::super::block_actions::is_test_grip(g.id))
        .unwrap();
    assert!(grip.world.distance(glam::DVec3::new(20., 70., 0.)) < 1e-8);
    let id = grip.id;
    assert!(app.start_test_action_grip(reference, id));
    feed(&mut app, "20,50");
    let EntityType::Line(line) = app.tabs[i].scene.document.get_entity(member).unwrap() else {
        panic!()
    };
    assert!(line.start.x.abs() < 1e-8);
}

#[test]
fn authoring_edit_basepoint_attribute_relocation_undo_and_cancel() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER BASEPOINT 2,3");
    let def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let handle = crate::scene::block_basepoint::parameter(&app.tabs[i].scene.document, def)
        .unwrap()
        .0;
    let mut attribute = document::entities::AttributeEntity::new("TAG".into(), "value".into());
    attribute.insertion_point = Vector3::new(7., 8., 0.);
    let EntityType::Insert(insert) = app.tabs[i]
        .scene
        .document
        .get_entity_mut(reference)
        .unwrap()
    else {
        panic!()
    };
    insert.attributes.push(attribute);
    let before = app.tabs[i].scene.document.clone();
    let result = crate::command::CmdResult::EditBlockAuthoring {
        document: app.tabs[i].id,
        definition: def,
        handle,
        part: crate::scene::block_authoring::edit::Part::Point,
        position: glam::DVec3::new(4., 6., 0.),
    };
    let _ = app.handle_authoring_edit(result);
    assert_ne!(
        app.tabs[i].scene.document.get_entity(reference),
        before.get_entity(reference)
    );
    app.undo_steps(1);
    assert_eq!(app.tabs[i].scene.document, before);
    app.redo_steps(1);
    assert_ne!(
        app.tabs[i].scene.document.get_entity(reference),
        before.get_entity(reference)
    );
    app.tabs[i].selected_authoring = Some(handle);
    app.refresh_properties();
    let id = app.tabs[i].selected_grips[0].id;
    assert!(app.start_authoring_grip(handle, id));
    let after = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let _ = app.update(Message::CommandEscape);
    assert_eq!(app.tabs[i].scene.document, after);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    let _ = app.update(Message::CommandEscape);
    assert_eq!(app.tabs[i].selected_authoring, None);
}

#[test]
fn authoring_edit_mouse_selection_endpoint_label_marker_history_and_reopen() {
    use crate::scene::block_actions::{self as actions, Kind};
    use crate::scene::block_authoring::edit::{self, Part};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let _ = app.dispatch_command("BACTIONBARMODE 0");
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LINEAR 2,2 6,2 4,3");
    let def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let p = actions::parameters(&app.tabs[i].scene.document, def, Kind::Move)[0].clone();
    let targets = members(&app);
    let a = actions::create(
        &mut app.tabs[i].scene.document,
        def,
        Kind::Move,
        p.handle,
        &targets,
        glam::DVec3::new(5., 5., 0.),
    )
    .unwrap();
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (1600., 900.);
    app.tabs[i].scene.camera.borrow_mut().fit_to_bounds_f64(
        glam::DVec3::new(-1., -1., 0.),
        glam::DVec3::new(10., 7., 0.),
        1600. / 900.,
    );
    let click = |app: &mut OpenCADStudio, point: Vector3| {
        let scene = &app.tabs[app.active_tab].scene;
        let (w, h) = scene.selection.borrow().view.vp_size;
        let b = scene.active_model_tile_bounds(w, h);
        let s = scene
            .camera
            .borrow()
            .project(
                glam::DVec3::new(point.x, point.y, point.z),
                iced::Rectangle {
                    x: 0.,
                    y: 0.,
                    width: b.width,
                    height: b.height,
                },
            )
            .unwrap();
        let _ = app.update(Message::ViewportLeftPressAt(iced::Point::new(
            s.x + b.x,
            s.y + b.y,
        )));
        let _ = app.update(Message::ViewportLeftRelease);
    };
    click(&mut app, Vector3::new(4., 3., 0.));
    assert_eq!(app.tabs[i].selected_authoring, Some(p.handle));
    assert_eq!(app.tabs[i].selected_grips.len(), 3);
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    click(&mut app, Vector3::new(6., 2., 0.));
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().map(|c| c.name()),
        Some("BEDITGRIP")
    );
    feed(&mut app, "8,2");
    assert!(app.tabs[i].active_cmd.is_none());
    assert_eq!(
        actions::parameters(&app.tabs[i].scene.document, def, Kind::Move)[0].end,
        glam::DVec3::new(8., 2., 0.)
    );
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    app.undo_steps(1);
    assert_eq!(app.tabs[i].scene.document, before);
    app.redo_steps(1);
    let label = edit::items(&app.tabs[i].scene.document, def)
        .into_iter()
        .find(|i| i.handle == p.handle)
        .unwrap()
        .grips
        .into_iter()
        .find(|(p, _)| *p == Part::Label(0))
        .unwrap()
        .1;
    click(&mut app, label);
    feed(&mut app, "5,4");
    let graphic = edit::items(&app.tabs[i].scene.document, def)
        .into_iter()
        .find(|i| i.handle == p.handle)
        .unwrap();
    assert_eq!(graphic.graphic.labels[0].0, Vector3::new(5., 4., 0.));
    let _ = app.update(Message::CommandEscape);
    click(&mut app, Vector3::new(5., 5., 0.));
    assert_eq!(app.tabs[i].selected_authoring, Some(a));
    click(&mut app, Vector3::new(5., 5., 0.));
    feed(&mut app, "7,5");
    let action = actions::actions(&app.tabs[i].scene.document, def);
    assert_eq!(action[0].1.display_location, Vector3::new(7., 5., 0.));
    assert_eq!(action[0].1.dependencies, targets);
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BEDIT_DISCARD");
    assert!(app.tabs[i]
        .selected_grips
        .iter()
        .all(|g| !super::super::block_authoring_edit::is_editor_grip(g.id)));
    // Persisted native coordinates, rather than session-only display positions.
    use std::io::Cursor;
    let bytes = document::DwgWriter::write_to_vec(&app.tabs[i].scene.document).unwrap();
    let loaded = document::DwgReader::from_stream(Cursor::new(bytes))
        .read()
        .unwrap();
    assert_eq!(
        actions::parameters(&loaded, def, Kind::Move)[0].end,
        glam::DVec3::new(8., 2., 0.)
    );
    assert_eq!(
        actions::actions(&loaded, def)[0].1.display_location,
        Vector3::new(7., 5., 0.)
    );
    app.tabs[i].scene.document = loaded;
    begin(&mut app, reference);
    click(&mut app, Vector3::new(8., 2., 0.));
    assert_eq!(app.tabs[i].selected_authoring, Some(p.handle));
}

#[test]
fn blank_pick_clears_block_authoring_selection() {
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 4,0 2,1");
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let parameter =
        actions::parameters(&app.tabs[i].scene.document, definition, Kind::Move)[0].handle;
    app.tabs[i].selected_authoring = Some(parameter);
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800., 600.);

    assert!(!app.pick_authoring_object(
        iced::Point::new(-1000., -1000.),
        iced::Rectangle {
            x: 0.,
            y: 0.,
            width: 800.,
            height: 600.
        },
    ));
    assert_eq!(app.tabs[i].selected_authoring, None);
}

#[test]
fn block_parameter_leader_and_crossing_box_select_then_delete() {
    use crate::scene::block_actions::{self as actions, Kind};
    use crate::scene::block_authoring::edit;
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 6,0 3,2");
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let handle = actions::parameters(&app.tabs[i].scene.document, definition, Kind::Move)[0].handle;
    let item = edit::items(&app.tabs[i].scene.document, definition)
        .into_iter()
        .find(|item| item.handle == handle)
        .unwrap();
    let [a, b] = item.graphic.lines[0];
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800., 600.);
    app.tabs[i].scene.camera.borrow_mut().fit_to_bounds_f64(
        glam::DVec3::new(-1., -2., 0.),
        glam::DVec3::new(7., 4., 0.),
        800. / 600.,
    );
    let bounds = iced::Rectangle {
        x: 0.,
        y: 0.,
        width: 800.,
        height: 600.,
    };
    let project = |point: Vector3| {
        let p = app.tabs[i]
            .scene
            .camera
            .borrow()
            .project(glam::DVec3::new(point.x, point.y, point.z), bounds)
            .unwrap();
        iced::Point::new(p.x, p.y)
    };
    let (pa, pb) = (project(a), project(b));
    let midpoint = iced::Point::new((pa.x + pb.x) * 0.5, (pa.y + pb.y) * 0.5);
    assert!(app.pick_authoring_object(midpoint, bounds));
    assert_eq!(app.tabs[i].selected_authoring, Some(handle));

    app.tabs[i].selected_authoring = None;
    let box_bounds = iced::Rectangle {
        x: midpoint.x - 1.,
        y: midpoint.y - 1.,
        width: 2.,
        height: 2.,
    };
    assert!(app.pick_authoring_box(
        iced::Point::new(box_bounds.x, box_bounds.y),
        iced::Point::new(
            box_bounds.x + box_bounds.width,
            box_bounds.y + box_bounds.height
        ),
        true,
        false,
    ));
    assert_eq!(app.tabs[i].selected_authoring, Some(handle));

    let _ = app.update(Message::DeleteSelected);
    assert!(edit::items(&app.tabs[i].scene.document, definition)
        .iter()
        .all(|item| item.handle != handle));
}

#[test]
fn erase_removes_selected_action_and_parameter_takes_its_actions() {
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 6,0 3,2");
    let def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let p = actions::parameters(&app.tabs[i].scene.document, def, Kind::Move)[0].handle;
    let targets = members(&app);
    let mut action = || {
        let doc = &mut app.tabs[i].scene.document;
        actions::create(doc, def, Kind::Move, p, &targets, glam::DVec3::ZERO).unwrap()
    };
    let (first, second) = (action(), action());
    let live = |app: &OpenCADStudio| {
        let doc = &app.tabs[app.active_tab].scene.document;
        actions::actions(doc, def)
            .iter()
            .map(|a| a.0)
            .collect::<Vec<_>>()
    };

    app.tabs[i].selected_authoring = Some(first);
    let _ = app.dispatch_command("ERASE");
    assert_eq!(live(&app), vec![second]);

    app.tabs[i].selected_authoring = Some(p);
    let _ = app.update(Message::DeleteSelected);
    assert!(live(&app).is_empty());
    assert!(app.tabs[i].scene.document.objects.get(&second).is_none());
}

#[test]
fn authoring_parameter_grip_drag_shows_preview_before_commit() {
    use crate::scene::block_actions::{self as actions, Kind};
    use crate::scene::block_authoring::edit::{self, Part};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 6,0 3,2");
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let parameter =
        actions::parameters(&app.tabs[i].scene.document, definition, Kind::Move)[0].handle;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800., 600.);
    app.tabs[i].scene.camera.borrow_mut().fit_to_bounds_f64(
        glam::DVec3::new(-1., -2., 0.),
        glam::DVec3::new(7., 4., 0.),
        800. / 600.,
    );
    let item = edit::items(&app.tabs[i].scene.document, definition)
        .into_iter()
        .find(|item| item.handle == parameter)
        .unwrap();
    let grip_index = item
        .grips
        .iter()
        .position(|(part, _)| *part == Part::End)
        .unwrap();
    app.tabs[i].selected_authoring = Some(parameter);
    app.refresh_properties();
    let grip_id = app.tabs[i].selected_grips[grip_index].id;
    assert!(app.start_authoring_grip(parameter, grip_id));

    let bounds = iced::Rectangle {
        x: 0.,
        y: 0.,
        width: 800.,
        height: 600.,
    };
    let cursor = app.tabs[i]
        .scene
        .camera
        .borrow()
        .project(glam::DVec3::new(7., 2., 0.), bounds)
        .unwrap();
    let _ = app.update(Message::ViewportMove(iced::Point::new(cursor.x, cursor.y)));

    assert!(!app.tabs[i].scene.preview_wires.is_empty());
    assert_eq!(
        actions::parameters(&app.tabs[i].scene.document, definition, Kind::Move)[0].end,
        glam::DVec3::new(6., 0., 0.)
    );
    assert!(app.tabs[i].active_cmd.is_some());
}

#[test]
fn block_editor_renames_orientation_basepoint_and_visibility_parameters() {
    use crate::scene::block_authoring::edit;
    use crate::scene::block_orientation_parameter::{self as orientation, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let rename = |app: &mut OpenCADStudio, handle: Handle, name: &str| {
        app.tabs[i].selected_authoring = Some(handle);
        let _ = app.update(Message::PropGeomInput {
            field: "authoring_name",
            value: name.to_string(),
        });
        let _ = app.update(Message::PropGeomCommit("authoring_name"));
        assert_eq!(
            edit::items(&app.tabs[i].scene.document, definition)
                .into_iter()
                .find(|item| item.handle == handle)
                .unwrap()
                .name,
            name,
        );
    };

    let alignment = orientation::create(
        &mut app.tabs[i].scene.document,
        definition,
        Kind::Alignment,
        Vector3::ZERO,
        Vector3::new(4., 0., 0.),
        Vector3::new(3., 1., 0.),
    )
    .unwrap();
    rename(&mut app, alignment, "Align renamed");

    let flip = orientation::create(
        &mut app.tabs[i].scene.document,
        definition,
        Kind::Flip,
        Vector3::ZERO,
        Vector3::new(4., 0., 0.),
        Vector3::new(3., 1., 0.),
    )
    .unwrap();
    rename(&mut app, flip, "Mirror state");

    let basepoint = crate::scene::block_basepoint::create(
        &mut app.tabs[i].scene.document,
        definition,
        Vector3::new(1., 2., 0.),
    )
    .unwrap();
    rename(&mut app, basepoint, "Origin marker");

    let visibility = crate::scene::block_visibility::create(
        &mut app.tabs[i].scene.document,
        definition,
        Vector3::new(2., 3., 0.),
    )
    .unwrap();
    rename(&mut app, visibility, "Display state");
    assert_eq!(
        crate::scene::block_visibility::parameter(&app.tabs[i].scene.document, definition)
            .unwrap()
            .name,
        "Display state",
    );
}

#[test]
fn double_clicking_block_parameter_label_starts_rename() {
    use crate::scene::block_actions::{self as actions, Kind};
    use crate::scene::block_authoring::edit;
    for kind in ["LINEAR", "XY", "POLAR"] {
        let (mut app, reference) = fixture();
        begin(&mut app, reference);
        let i = app.active_tab;
        let _ = app.run_command_line(&format!("BPARAMETER {kind} 0,0 6,2 3,2"));
        let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
        let parameter =
            actions::parameters(&app.tabs[i].scene.document, definition, Kind::Move)[0].handle;
        let item = edit::items(&app.tabs[i].scene.document, definition)
            .into_iter()
            .find(|item| item.handle == parameter)
            .unwrap();
        let label = item.graphic.labels[0].0;
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800., 600.);
        app.tabs[i].scene.camera.borrow_mut().fit_to_bounds_f64(
            glam::DVec3::new(-1., -2., 0.),
            glam::DVec3::new(7., 4., 0.),
            800. / 600.,
        );
        let tile = app.tabs[i].scene.active_model_tile_bounds(800., 600.);
        let bounds = iced::Rectangle {
            x: 0.,
            y: 0.,
            width: tile.width,
            height: tile.height,
        };
        let screen = app.tabs[i]
            .scene
            .camera
            .borrow()
            .project(glam::DVec3::new(label.x, label.y, label.z), bounds)
            .unwrap();
        let cursor_local = iced::Point::new(screen.x + 10.0, screen.y - 10.0);
        let cursor = iced::Point::new(tile.x + cursor_local.x, tile.y + cursor_local.y);
        assert_eq!(
            app.authoring_label_at(cursor_local, bounds),
            Some((parameter, item.graphic.labels[0].1.clone()))
        );
        assert!(app.authoring_name_is_editable(parameter));
        app.last_vp_click_time =
            Some(std::time::Instant::now() - std::time::Duration::from_millis(100));
        app.last_vp_click_pos = Some(cursor_local);
        let _ = app.update(Message::ViewportMove(cursor));
        let _ = app.update(Message::ViewportLeftPressAt(cursor));

        assert_eq!(app.tabs[i].selected_authoring, Some(parameter));
        let expected_field = if kind == "POLAR" {
            "authoring_secondary_name"
        } else {
            "authoring_name"
        };
        assert_eq!(
            app.tabs[i]
                .properties
                .edit_buf
                .get(&crate::ui::properties::FieldKey::Geom(expected_field))
                .map(String::as_str),
            Some(item.graphic.labels[0].1.as_str()),
        );
    }
}

#[test]
fn linear_parameter_value_set_is_editable_and_undoable_in_properties() {
    use crate::scene::block_actions::{self as actions, Kind};
    use document::objects::{DynamicBlockData, ObjectType};

    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 6,0 3,2");
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let parameter =
        actions::parameters(&app.tabs[i].scene.document, definition, Kind::Move)[0].handle;
    app.tabs[i].selected_authoring = Some(parameter);
    app.refresh_properties();

    let fields = app.tabs[i]
        .properties
        .sections
        .iter()
        .flat_map(|section| section.props.iter())
        .map(|property| property.field)
        .collect::<Vec<_>>();
    assert!(fields.contains(&"authoring_chain_actions"));
    assert!(fields.contains(&"authoring_set_linear_distance_minimum"));
    assert!(fields.contains(&"authoring_set_linear_distance_maximum"));
    assert!(fields.contains(&"authoring_set_linear_distance_increment"));
    assert!(fields.contains(&"authoring_set_linear_distance_values"));
    let undo_depth = app.tabs[i].history.undo_stack.len();

    for (field, value) in [
        ("authoring_set_linear_distance_maximum", "20"),
        ("authoring_set_linear_distance_minimum", "2"),
        ("authoring_set_linear_distance_increment", "0.5"),
        ("authoring_set_linear_distance_values", "2, 4, 8"),
    ] {
        let _ = app.update(Message::PropGeomInput {
            field,
            value: value.into(),
        });
        let _ = app.update(Message::PropGeomCommit(field));
    }
    let ObjectType::DynamicBlock(object) = &app.tabs[i].scene.document.objects[&parameter] else {
        panic!("expected a dynamic parameter")
    };
    let DynamicBlockData::LinearParameter(parameter_data) = &object.data else {
        panic!("expected a Linear parameter")
    };
    assert_eq!(parameter_data.value_set.minimum, 2.0);
    assert_eq!(parameter_data.value_set.maximum, 20.0);
    assert_eq!(parameter_data.value_set.increment, 0.5);
    assert_eq!(parameter_data.value_set.values, vec![2.0, 4.0, 8.0]);
    assert_eq!(parameter_data.value_set.flags, 11);
    assert_eq!(app.tabs[i].history.undo_stack.len(), undo_depth + 4);
}

/// The flag that says a parameter's actions run one after the other is the only
/// record of the order the operator chose, so the panel has to answer to it both
/// ways: the row reads the bit off the record, clicking it writes the record, and
/// undoing puts the bit back without disturbing anything else the parameter holds.
#[test]
fn the_chain_actions_row_toggles_the_parameter_and_undoes() {
    use crate::scene::block_actions::{self as actions, Kind};
    use crate::scene::model::object::PropValue;
    use document::objects::{DynamicBlockData, ObjectType};

    fn linear(
        app: &OpenCADStudio,
        tab: usize,
        parameter: Handle,
    ) -> &document::objects::BlockLinearParameter {
        let ObjectType::DynamicBlock(object) = &app.tabs[tab].scene.document.objects[&parameter]
        else {
            panic!("expected a dynamic parameter")
        };
        let DynamicBlockData::LinearParameter(data) = &object.data else {
            panic!("expected a Linear parameter")
        };
        data
    }
    fn chained(app: &OpenCADStudio, tab: usize, parameter: Handle) -> bool {
        linear(app, tab, parameter)
            .parameter
            .parameter
            .chain_actions
    }
    // The distance limits are the part of the record the toggle must not disturb.
    fn limits(app: &OpenCADStudio, tab: usize, parameter: Handle) -> (f64, f64) {
        let set = &linear(app, tab, parameter).value_set;
        (set.minimum, set.maximum)
    }
    let row = |app: &mut OpenCADStudio| {
        app.refresh_properties();
        app.tabs[app.active_tab]
            .properties
            .sections
            .iter()
            .flat_map(|section| section.props.iter())
            .find(|property| property.field == "authoring_chain_actions")
            .cloned()
            .expect("the parameter has a Chain actions row")
    };

    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 6,0 3,2");
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let parameter =
        actions::parameters(&app.tabs[i].scene.document, definition, Kind::Move)[0].handle;
    app.tabs[i].selected_authoring = Some(parameter);
    let _ = app.update(Message::PropGeomInput {
        field: "authoring_set_linear_distance_maximum",
        value: "20".into(),
    });
    let _ = app.update(Message::PropGeomCommit(
        "authoring_set_linear_distance_maximum",
    ));
    let limits_before = limits(&app, i, parameter);
    let graph_before = actions::actions(&app.tabs[i].scene.document, definition).len();
    let depth = app.tabs[i].history.undo_stack.len();

    assert_eq!(
        chained(&app, i, parameter),
        false,
        "a fresh parameter is not chained"
    );
    let PropValue::BoolToggle { value, .. } = row(&mut app).value else {
        panic!("the Chain actions row is not a toggle")
    };
    assert_eq!(value, false, "the row reads the bit off the record");

    let _ = app.update(Message::PropBoolToggle("authoring_chain_actions"));
    assert_eq!(
        chained(&app, i, parameter),
        true,
        "clicking the row writes the chaining bit into the record"
    );
    let PropValue::BoolToggle { value, .. } = row(&mut app).value else {
        panic!("the Chain actions row is not a toggle")
    };
    assert_eq!(value, true, "the row reads back what was written");
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    assert_eq!(
        limits(&app, i, parameter),
        limits_before,
        "only the bit moves"
    );

    let _ = app.update(Message::Undo);
    assert_eq!(
        chained(&app, i, parameter),
        false,
        "undoing the toggle leaves the parameter unchained"
    );
    assert_eq!(
        limits(&app, i, parameter),
        limits_before,
        "undo moves the bit alone"
    );
    assert_eq!(
        actions::actions(&app.tabs[i].scene.document, definition).len(),
        graph_before,
        "the evaluation graph is unchanged by a chaining toggle"
    );
}
