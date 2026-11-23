use super::*;

#[test]
fn block_actions_topology_save_updates_values_members_properties_and_history() {
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    let i = app.active_tab;
    let second = app.tabs[i]
        .scene
        .document
        .add_entity(EntityType::Insert(Insert::new(
            "EDIT_ME",
            Vector3::new(40., 0., 0.),
        )))
        .unwrap();
    begin(&mut app, reference);
    circle(&mut app, 3.);
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
    let def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let p = actions::parameters(&app.tabs[i].scene.document, def, Kind::Move)[0].handle;
    let old_members = members(&app);
    let a = actions::create(
        &mut app.tabs[i].scene.document,
        def,
        Kind::Move,
        p,
        &old_members[..1],
        glam::DVec3::ZERO,
    )
    .unwrap();
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");
    for (insert, x) in [(reference, 12.), (second, 54.)] {
        let _ = app.handle_block_action_result(crate::command::CmdResult::EvaluateBlockParameter {
            grip: None,
            insert: Some(insert),
            document: app.tabs[i].id,
            definition: def,
            parameter: p,
            value: actions::Value::Point(glam::DVec3::new(x, 0., 0.)),
        });
    }
    let check = |app: &OpenCADStudio, end: f64, center: f64| {
        let doc = &app.tabs[app.active_tab].scene.document;
        for (insert, delta) in [(reference, 2.), (second, 4.)] {
            let EntityType::Insert(ins) = doc.get_entity(insert).unwrap() else {
                panic!()
            };
            let handles = &doc
                .block_records
                .get(&ins.block_name)
                .unwrap()
                .entity_handles;
            assert_eq!(handles.len(), 2);
            let EntityType::Line(line) = doc.get_entity(handles[0]).unwrap() else {
                panic!()
            };
            assert_eq!(line.start.x, delta);
            assert_eq!(line.end.x, end + delta);
            let EntityType::Circle(circle) = doc.get_entity(handles[1]).unwrap() else {
                panic!()
            };
            assert_eq!(circle.center.x, center);
        }
    };
    check(&app, 1., 3.);
    begin(&mut app, reference);
    app.push_undo_snapshot(i, "Definition edit");
    let EntityType::Line(line) = app.tabs[i]
        .scene
        .document
        .get_entity_mut(old_members[0])
        .unwrap()
    else {
        panic!()
    };
    line.end.x = 2.;
    line.common.color = document::types::Color::Index(3);
    app.tabs[i].scene.erase_entities(&old_members[1..]);
    app.finish_pending_history(i);
    circle(&mut app, 7.);
    let added = members(&app)[1];
    let _ = app.dispatch_command("BSAVE");
    check(&app, 2., 7.);
    let saved = app.tabs[i].scene.document.clone();
    let depth = main_depth(&app);
    let _ = app.dispatch_command("BSAVE");
    assert_eq!(main_depth(&app), depth);
    assert_eq!(
        app.tabs[i].scene.document, saved,
        "saving unchanged authoring does not rebuild again"
    );
    let _ = app.dispatch_command("BCLOSE");
    app.undo_steps(1);
    check(&app, 1., 3.);
    assert!(app.tabs[i].scene.document.get_entity(added).is_none());
    app.redo_steps(1);
    check(&app, 2., 7.);
    for insert in [reference, second] {
        let doc = &app.tabs[i].scene.document;
        let EntityType::Insert(ins) = doc.get_entity(insert).unwrap() else {
            panic!()
        };
        let member = doc
            .block_records
            .get(&ins.block_name)
            .unwrap()
            .entity_handles[0];
        assert_eq!(
            doc.get_entity(member).unwrap().common().color,
            document::types::Color::Index(3)
        );
    }
    let bytes = document::DwgWriter::write_to_vec(&app.tabs[i].scene.document).unwrap();
    app.tabs[i].scene.document = document::DwgReader::from_stream(std::io::Cursor::new(bytes))
        .read()
        .unwrap();
    check(&app, 2., 7.);
    begin(&mut app, reference);
    edit_action_set(&mut app, a, "Add", vec![added]);
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");
    // New geometry can now join an action even on previously evaluated references.
    for (insert, center) in [(reference, 9.), (second, 11.)] {
        let doc = &app.tabs[i].scene.document;
        let EntityType::Insert(ins) = doc.get_entity(insert).unwrap() else {
            panic!()
        };
        let h = doc
            .block_records
            .get(&ins.block_name)
            .unwrap()
            .entity_handles[1];
        let EntityType::Circle(circle) = doc.get_entity(h).unwrap() else {
            panic!()
        };
        assert_eq!(circle.center.x, center);
    }
}

#[test]
fn block_actions_topology_save_coalesces_bounded_layer_history_in_both_directions() {
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
    let def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let p = actions::parameters(&app.tabs[i].scene.document, def, Kind::Move)[0].handle;
    let targets = members(&app);
    actions::create(
        &mut app.tabs[i].scene.document,
        def,
        Kind::Move,
        p,
        &targets,
        glam::DVec3::ZERO,
    )
    .unwrap();
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");
    let _ = app.handle_block_action_result(crate::command::CmdResult::EvaluateBlockParameter {
        grip: None,
        insert: Some(reference),
        document: app.tabs[i].id,
        definition: def,
        parameter: p,
        value: actions::Value::Point(glam::DVec3::new(12., 0., 0.)),
    });
    begin(&mut app, reference);
    circle(&mut app, 3.);
    let original = app.tabs[i].scene.document.layers.get("0").unwrap().color;
    let pending = app.begin_layer_undo(i, "Layer color", &["0".into()]);
    app.tabs[i]
        .scene
        .document
        .layers
        .get_mut("0")
        .unwrap()
        .color = document::types::Color::Index(1);
    app.commit_layer_undo(i, pending);
    let _ = app.dispatch_command("BSAVE");
    assert!(
        matches!(app.tabs[i].history.undo_stack.last(),Some(super::super::tab_document::HistorySnapshot::Delta(d)) if matches!(d.structure,Some(super::super::tab_document::StructureSnapshot::Combined(_))))
    );
    app.undo_steps(1);
    assert_eq!(
        app.tabs[i].scene.document.layers.get("0").unwrap().color,
        original
    );
    app.undo_steps(1);
    assert_eq!(members(&app).len(), 1);
    app.redo_steps(2);
    let doc = &app.tabs[i].scene.document;
    assert_eq!(
        doc.layers.get("0").unwrap().color,
        document::types::Color::Index(1)
    );
    let EntityType::Insert(ins) = doc.get_entity(reference).unwrap() else {
        panic!()
    };
    let record = doc.block_records.get(&ins.block_name).unwrap();
    assert_eq!(record.entity_handles.len(), 2);
    assert!(record
        .entity_handles
        .iter()
        .all(|h| doc.get_entity(*h).is_some()));
    assert_eq!(members(&app).len(), 2);
}

#[test]
fn block_actions_topology_local_undo_after_save_resave_has_no_orphan_members() {
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
    let def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let p = actions::parameters(&app.tabs[i].scene.document, def, Kind::Move)[0].handle;
    let targets = members(&app);
    actions::create(
        &mut app.tabs[i].scene.document,
        def,
        Kind::Move,
        p,
        &targets,
        glam::DVec3::ZERO,
    )
    .unwrap();
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");
    let _ = app.handle_block_action_result(crate::command::CmdResult::EvaluateBlockParameter {
        grip: None,
        insert: Some(reference),
        document: app.tabs[i].id,
        definition: def,
        parameter: p,
        value: actions::Value::Point(glam::DVec3::new(12., 0., 0.)),
    });
    begin(&mut app, reference);
    circle(&mut app, 3.);
    let _ = app.dispatch_command("BSAVE");
    app.undo_steps(1);
    circle(&mut app, 7.);
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");
    let doc = &app.tabs[i].scene.document;
    let EntityType::Insert(ins) = doc.get_entity(reference).unwrap() else {
        panic!()
    };
    let record = doc.block_records.get(&ins.block_name).unwrap();
    assert_eq!(record.entity_handles.len(), 2);
    let owned = doc
        .entities()
        .filter(|e| {
            e.common().owner_handle == record.handle
                && !matches!(e, EntityType::Block(_) | EntityType::BlockEnd(_))
        })
        .count();
    assert_eq!(owned, 2);
    let EntityType::Circle(circle) = doc.get_entity(record.entity_handles[1]).unwrap() else {
        panic!()
    };
    assert_eq!(circle.center.x, 7.);
    let bytes = document::DwgWriter::write_to_vec(doc).unwrap();
    let reopened = document::DwgReader::from_stream(std::io::Cursor::new(bytes))
        .read()
        .unwrap();
    let record = reopened.block_records.get(&ins.block_name).unwrap();
    assert_eq!(record.entity_handles.len(), 2);
}

#[test]
fn block_actions_topology_erase_last_target_empty_definition_discard_and_save() {
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
    let def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let p = actions::parameters(&app.tabs[i].scene.document, def, Kind::Move)[0].handle;
    let targets = members(&app);
    let a = actions::create(
        &mut app.tabs[i].scene.document,
        def,
        Kind::Move,
        p,
        &targets,
        glam::DVec3::ZERO,
    )
    .unwrap();
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");
    let _ = app.handle_block_action_result(crate::command::CmdResult::EvaluateBlockParameter {
        grip: None,
        insert: Some(reference),
        document: app.tabs[i].id,
        definition: def,
        parameter: p,
        value: actions::Value::Point(glam::DVec3::new(12., 0., 0.)),
    });
    let original = app.tabs[i].scene.document.clone();
    begin(&mut app, reference);
    circle(&mut app, 5.);
    let _ = app.dispatch_command("BEDIT_DISCARD");
    assert_eq!(app.tabs[i].scene.document, original);
    begin(&mut app, reference);
    app.push_undo_snapshot(i, "Erase targets");
    app.tabs[i].scene.erase_entities(&targets);
    app.finish_pending_history(i);
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");
    assert!(actions::actions(&app.tabs[i].scene.document, def)[0]
        .1
        .dependencies
        .is_empty());
    assert!(app.tabs[i].scene.document.objects.contains_key(&a));
    let EntityType::Insert(ins) = app.tabs[i].scene.document.get_entity(reference).unwrap() else {
        panic!()
    };
    assert!(app.tabs[i]
        .scene
        .document
        .block_records
        .get(&ins.block_name)
        .unwrap()
        .entity_handles
        .is_empty());
    app.undo_steps(1);
    assert_eq!(
        app.tabs[i].scene.document.get_entity(reference),
        original.get_entity(reference)
    );
    assert!(app.tabs[i].scene.document.get_entity(targets[0]).is_some());
    app.redo_steps(1);
    assert!(app.tabs[i].scene.document.get_entity(targets[0]).is_none());
}

#[test]
fn block_actions_set_edit_refresh_local_history_discard_save_and_drawing_history() {
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    circle(&mut app, 3.);
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
    let def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let p = actions::parameters(&app.tabs[i].scene.document, def, Kind::Move)[0].clone();
    let targets = members(&app);
    let action = actions::create(
        &mut app.tabs[i].scene.document,
        def,
        Kind::Move,
        p.handle,
        &targets[..1],
        glam::DVec3::new(3., 4., 0.),
    )
    .unwrap();
    edit_action_set(&mut app, action, "Add", targets[1..].to_vec());
    assert_eq!(
        actions::actions(&app.tabs[i].scene.document, def)[0]
            .1
            .dependencies,
        targets
    );
    app.undo_steps(1);
    assert_eq!(
        actions::actions(&app.tabs[i].scene.document, def)[0]
            .1
            .dependencies,
        targets[..1]
    );
    app.redo_steps(1);
    edit_action_set(&mut app, action, "Remove", targets[..1].to_vec());
    assert_eq!(
        actions::actions(&app.tabs[i].scene.document, def)[0]
            .1
            .dependencies,
        targets[1..]
    );
    edit_action_set(&mut app, action, "New", targets[..1].to_vec());
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");
    app.tabs[i].scene.selected.insert(reference);
    app.refresh_properties();
    let grip = app.tabs[i]
        .selected_grips
        .iter()
        .find(|g| super::super::block_actions::is_test_grip(g.id))
        .unwrap()
        .id;
    assert!(app.start_test_action_grip(reference, grip));
    feed(&mut app, "12,0");
    let check = |app: &OpenCADStudio, start: f64, center: f64| {
        let doc = &app.tabs[app.active_tab].scene.document;
        let EntityType::Insert(ins) = doc.get_entity(reference).unwrap() else {
            panic!()
        };
        let members = &doc
            .block_records
            .get(&ins.block_name)
            .unwrap()
            .entity_handles;
        let EntityType::Line(line) = doc.get_entity(members[0]).unwrap() else {
            panic!()
        };
        let EntityType::Circle(circle) = doc.get_entity(members[1]).unwrap() else {
            panic!()
        };
        assert_eq!(line.start.x, start);
        assert_eq!(circle.center.x, center);
    };
    check(&app, 2., 3.);
    begin(&mut app, reference);
    let depth = app.tabs[i].history.undo_stack.len();
    edit_action_set(&mut app, action, "New", targets[1..].to_vec());
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    check(&app, 0., 5.);
    app.undo_steps(1);
    check(&app, 2., 3.);
    app.redo_steps(1);
    check(&app, 0., 5.);
    let _ = app.dispatch_command("BEDIT_DISCARD");
    check(&app, 2., 3.);
    begin(&mut app, reference);
    edit_action_set(&mut app, action, "New", targets[1..].to_vec());
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");
    check(&app, 0., 5.);
    app.undo_steps(1);
    check(&app, 2., 3.);
    assert_eq!(
        actions::actions(&app.tabs[i].scene.document, def)[0]
            .1
            .dependencies,
        targets[..1]
    );
    app.redo_steps(1);
    check(&app, 0., 5.);
    let bytes = document::DwgWriter::write_to_vec(&app.tabs[i].scene.document).unwrap();
    app.tabs[i].scene.document = document::DwgReader::from_stream(std::io::Cursor::new(bytes))
        .read()
        .unwrap();
    check(&app, 0., 5.);
    assert_eq!(
        actions::actions(&app.tabs[i].scene.document, def)[0]
            .1
            .dependencies,
        targets[1..]
    );
}

#[test]
fn block_actions_set_frame_cancel_stale_and_context_guards() {
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    let _ = app.dispatch_command("LINE");
    let _ = app.dispatch_command("BACTIONSET");
    assert_eq!(
        app.tabs[app.active_tab].active_cmd.as_ref().unwrap().name(),
        "LINE"
    );
    let _ = app.update(Message::CommandEscape);
    begin(&mut app, reference);
    let _ = app.dispatch_command("BACTIONBARMODE 0");
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
    let def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let p = actions::parameters(&app.tabs[i].scene.document, def, Kind::Stretch)[0].handle;
    let targets = members(&app);
    let action = actions::create_with_frame(
        &mut app.tabs[i].scene.document,
        def,
        Kind::Stretch,
        p,
        &targets,
        glam::DVec3::new(3., 4., 0.),
        Some([glam::DVec3::new(0.5, -1., 0.), glam::DVec3::new(2., 1., 0.)]),
    )
    .unwrap();
    let original = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let _ = app.dispatch_command("BACTIONSET");
    let name = actions::actions(&original, def)[0].1.element.name.clone();
    feed(&mut app, &name);
    feed(&mut app, "Frame");
    feed(&mut app, "-1,-1");
    let _ = app.update(Message::CommandEscape);
    assert_eq!(app.tabs[i].scene.document, original);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    app.tabs[i].selected_authoring = Some(action);
    let _ = app.dispatch_command("BACTIONSET");
    feed(&mut app, "Frame");
    feed(&mut app, "-1,-1");
    feed(&mut app, "-1,1");
    assert_eq!(app.tabs[i].scene.document, original);
    feed(&mut app, "0.5,1");
    assert!(app.tabs[i].active_cmd.is_none());
    assert_eq!(
        actions::frame_outline(&app.tabs[i].scene.document, action)[0],
        Vector3::new(-1., -1., 0.)
    );
    app.undo_steps(1);
    assert_eq!(
        app.tabs[i].scene.document.objects[&action],
        original.objects[&action]
    );
    let expected = Box::new(original.objects[&action].clone());
    let _ = app.handle_block_action_result(crate::command::CmdResult::EditBlockActionSet {
        document: app.tabs[i].id + 1,
        definition: def,
        action,
        expected: expected.clone(),
        point_choices: None,
        targets: targets.clone(),
        frame: None,
        rotate_only: None,
    });
    assert_eq!(app.tabs[i].scene.document, original);
    crate::scene::block_authoring::edit::move_part(
        &mut app.tabs[i].scene.document,
        def,
        action,
        crate::scene::block_authoring::edit::Part::Point,
        Vector3::new(8., 8., 0.),
    )
    .unwrap();
    let stale = app.tabs[i].scene.document.clone();
    let _ = app.handle_block_action_result(crate::command::CmdResult::EditBlockActionSet {
        document: app.tabs[i].id,
        definition: def,
        action,
        expected,
        point_choices: None,
        targets,
        frame: None,
        rotate_only: None,
    });
    assert_eq!(app.tabs[i].scene.document, stale);
}

#[test]
fn block_actions_stretch_frame_selection_history_test_and_saved_grip() {
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
    let p = actions::parameters(&app.tabs[i].scene.document, def, Kind::Stretch)[0].clone();
    let before = app.tabs[i].scene.document.clone();
    // A half-authored frame and Escape create neither objects nor history.
    let depth = app.tabs[i].history.undo_stack.len();
    let _ = app.dispatch_command("BACTION Stretch");
    feed(&mut app, &p.name);
    feed(&mut app, "0.5,-1");
    let _ = app.update(Message::CommandEscape);
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    let _ = app.dispatch_command("BACTION Stretch");
    feed(&mut app, &p.name);
    assert!(!app.tabs[i]
        .active_cmd
        .as_ref()
        .unwrap()
        .is_selection_gathering());
    feed(&mut app, "0.5,-1");
    feed(&mut app, "0.5,1"); // zero-width error retains first corner
    assert!(!app.tabs[i]
        .active_cmd
        .as_ref()
        .unwrap()
        .is_selection_gathering());
    feed(&mut app, "2,1");
    assert!(app.tabs[i]
        .active_cmd
        .as_ref()
        .unwrap()
        .is_selection_gathering());
    let targets = members(&app);
    let _ = app.feed_command(crate::command::StepInput::SelectionComplete(
        targets.clone(),
    ));
    let _ = app.feed_command(crate::command::StepInput::Enter);
    feed(&mut app, "3,4");
    assert!(app.tabs[i].active_cmd.is_none());
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    let action = actions::actions(&app.tabs[i].scene.document, def)[0].0;
    assert_eq!(
        actions::frame_outline(&app.tabs[i].scene.document, action).len(),
        4
    );
    app.undo_steps(1);
    assert!(actions::actions(&app.tabs[i].scene.document, def).is_empty());
    app.redo_steps(1);
    let editor = app.tabs[i].scene.document.clone();
    let _ = app.dispatch_command("BTESTBLOCK");
    let ti = app.active_tab;
    let test_insert = app.tabs[ti].block_test.as_ref().unwrap().insert;
    app.tabs[ti].scene.selected.insert(test_insert);
    app.refresh_properties();
    let grip = app.tabs[ti]
        .selected_grips
        .iter()
        .find(|g| super::super::block_actions::is_test_grip(g.id))
        .unwrap()
        .id;
    assert!(app.start_test_action_grip(test_insert, grip));
    feed(&mut app, "12,0");
    let EntityType::Line(line) = app.tabs[ti].scene.document.get_entity(targets[0]).unwrap() else {
        panic!()
    };
    assert_eq!(line.start, Vector3::ZERO);
    assert_eq!(line.end, Vector3::new(3., 0., 0.));
    let _ = app.dispatch_command("BCLOSETESTBLOCK");
    assert_eq!(app.tabs[i].scene.document, editor);
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");
    app.tabs[i].scene.selected.insert(reference);
    app.refresh_properties();
    let grip = app.tabs[i]
        .selected_grips
        .iter()
        .find(|g| super::super::block_actions::is_test_grip(g.id))
        .unwrap()
        .id;
    let depth = app.tabs[i].history.undo_stack.len();
    assert!(app.start_test_action_grip(reference, grip));
    feed(&mut app, "15,0");
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    let changed = app.tabs[i].scene.document.clone();
    let EntityType::Insert(insert) = changed.get_entity(reference).unwrap() else {
        panic!()
    };
    let member = changed
        .block_records
        .get(&insert.block_name)
        .unwrap()
        .entity_handles[0];
    let EntityType::Line(line) = changed.get_entity(member).unwrap() else {
        panic!()
    };
    assert_eq!(line.start, Vector3::ZERO);
    assert_eq!(line.end, Vector3::new(6., 0., 0.));
    assert_eq!(
        changed.get_entity(targets[0]),
        editor.get_entity(targets[0])
    );
    app.undo_steps(1);
    assert_eq!(
        app.tabs[i].scene.document.get_entity(reference),
        editor.get_entity(reference)
    );
    app.redo_steps(1);
    assert_eq!(
        app.tabs[i].scene.document.get_entity(member),
        changed.get_entity(member)
    );
    let bytes = document::DwgWriter::write_to_vec(&changed).unwrap();
    app.tabs[i].scene.document = document::DwgReader::from_stream(std::io::Cursor::new(bytes))
        .read()
        .unwrap();
    begin(&mut app, reference);
    assert_eq!(
        app.tabs[i].active_block_edit_session().unwrap().br_handle,
        def
    );
    assert_eq!(
        actions::frame_outline(&app.tabs[i].scene.document, action).len(),
        4
    );
}
