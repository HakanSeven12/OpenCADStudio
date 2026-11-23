use super::*;

#[test]
fn block_editor_display_changes_keep_rotated_labels_and_large_grips_pickable() {
    use crate::ui::overlay::authoring::{label_angle, label_corners};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 6,6 1,4");
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let item = crate::scene::block_authoring::edit::items(&app.tabs[i].scene.document, definition)
        .into_iter()
        .find(|item| item.name.starts_with("Distance"))
        .unwrap();
    let bounds = iced::Rectangle {
        x: 0.0,
        y: 0.0,
        width: 800.0,
        height: 600.0,
    };
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
    app.tabs[i].scene.camera.borrow_mut().fit_to_bounds_f64(
        glam::DVec3::new(-2.0, -2.0, 0.0),
        glam::DVec3::new(10.0, 10.0, 0.0),
        800.0 / 600.0,
    );
    let _ = app.dispatch_command("BPARAMETERSIZE 48");
    let _ = app.dispatch_command("BPTEXTHORIZONTAL 0");
    let camera = app.tabs[i].scene.camera.borrow();
    let project = |v: Vector3| {
        let p = camera
            .project(glam::DVec3::new(v.x, v.y, v.z), bounds)
            .unwrap();
        iced::Point::new(p.x, p.y)
    };
    let dimensions: Vec<_> = item
        .graphic
        .dimensions
        .iter()
        .map(|[a, b]| [project(*a), project(*b)])
        .collect();
    let (anchor, label) = &item.graphic.labels[0];
    let corners = label_corners(
        project(*anchor),
        label,
        label_angle(&dimensions, 0, false),
        48,
        "Simplex.shx",
    );
    let cursor = iced::Point::new(
        (corners[0].x + corners[2].x) / 2.0,
        (corners[0].y + corners[2].y) / 2.0,
    );
    assert_eq!(
        app.authoring_label_at(cursor, bounds),
        Some((item.handle, label.clone()))
    );
    drop(camera);
    assert!(app.pick_authoring_object(cursor, bounds));
    let _ = app.dispatch_command("BGRIPOBJSIZE 80");
    app.refresh_authoring_grips([0.0; 3]);
    let g = &app.tabs[i].selected_grips[0];
    let p = app.tabs[i]
        .scene
        .camera
        .borrow()
        .project(g.world, bounds)
        .unwrap();
    let cursor = iced::Point::new(p.x + 35.0, p.y + 35.0);
    assert!(app.authoring_grip_hit(cursor, bounds).is_some());
    let _ = app.dispatch_command("BGRIPOBJSIZE 8");
    assert!(app.authoring_grip_hit(cursor, bounds).is_none());
}

#[test]
fn block_action_association_repairs_multiple_orphans_with_local_undo_and_stale_guards() {
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    let i = app.active_tab;
    let _ = app.dispatch_command("LINE");
    let _ = app.dispatch_command("BASSOCIATE");
    assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(), "LINE");
    begin(&mut app, reference);
    let _ = app.dispatch_command("BACTIONBARMODE 0");
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let original = actions::all_parameters(&app.tabs[i].scene.document, definition)[0].clone();
    let _ = app.run_command_line("BPARAMETER XY 0,0 10,10 5,5");
    let replacement = actions::all_parameters(&app.tabs[i].scene.document, definition)
        .into_iter()
        .find(|p| p.xy)
        .unwrap();
    let target = members(&app)[0];
    let mut handles = vec![];
    for x in [3.0, 5.0] {
        let action = actions::create(
            &mut app.tabs[i].scene.document,
            definition,
            Kind::Move,
            original.handle,
            &[target],
            glam::DVec3::new(x, 3.0, 0.0),
        )
        .unwrap();
        let Some(document::objects::ObjectType::DynamicBlock(object)) =
            app.tabs[i].scene.document.objects.get_mut(&action)
        else {
            panic!()
        };
        let document::objects::DynamicBlockData::MoveAction(native) = &mut object.data else {
            panic!()
        };
        native
            .connections
            .iter_mut()
            .for_each(|connection| connection.code = -1);
        handles.push(action);
    }
    let baseline = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    app.tabs[i].selected_authoring = Some(handles[0]);
    let _ = app.dispatch_command("BASSOCIATE");
    let _ = app.feed_command(crate::command::StepInput::Enter);
    assert_eq!(app.tabs[i].scene.document, baseline);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    let _ = app.dispatch_command("BASSOCIATE");
    feed(&mut app, &replacement.name);
    feed(&mut app, "X");
    assert!(app.tabs[i].active_cmd.is_none());
    assert_eq!(
        actions::association::orphaned(&app.tabs[i].scene.document, definition),
        vec![handles[1]]
    );
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    app.tabs[i].selected_authoring = Some(handles[1]);
    let _ = app.dispatch_command("BASSOCIATE");
    feed(&mut app, &original.name);
    let _ = app.feed_command(crate::command::StepInput::Enter);
    assert!(app.tabs[i].active_cmd.is_none());
    assert!(actions::association::orphaned(&app.tabs[i].scene.document, definition).is_empty());
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 2);
    app.undo_steps(1);
    assert_eq!(
        actions::association::orphaned(&app.tabs[i].scene.document, definition),
        vec![handles[1]]
    );
    let expected = app.tabs[i].scene.document.objects[&handles[1]].clone();
    let Some(document::objects::ObjectType::DynamicBlock(object)) =
        app.tabs[i].scene.document.objects.get_mut(&handles[1])
    else {
        panic!()
    };
    let document::objects::DynamicBlockData::MoveAction(native) = &mut object.data else {
        panic!()
    };
    native.action.display_location.x += 1.0;
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let _ = app.handle_block_action_result(crate::command::CmdResult::AssociateBlockAction {
        document: app.tabs[i].id,
        definition,
        action: handles[1],
        expected: Box::new(expected),
        parameter: original.handle,
        key_point: Some(actions::XyKeyPoint::End),
    });
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    assert!(app
        .command_line
        .last_error
        .as_ref()
        .unwrap()
        .contains("changed"));
}
