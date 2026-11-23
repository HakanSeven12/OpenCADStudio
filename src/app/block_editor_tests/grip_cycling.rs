use super::*;

#[test]
fn block_grip_cycling_dialog_is_staged_undoable_and_rejects_stale_grips() {
    let (mut app, reference) = fixture();
    let i = app.active_tab;
    let _ = app.dispatch_command("LINE");
    let _ = app.dispatch_command("BCYCLEORDER");
    assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(), "LINE");
    begin(&mut app, reference);
    let _ = app.dispatch_command("BCYCLEORDER");
    assert!(app.block_cycle_order.as_ref().unwrap().rows.is_empty());
    assert_eq!(app.block_cycle_order.as_ref().unwrap().selected, None);
    let _ = app.update(Message::CloseModal);
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2 1");
    let _ = app.run_command_line("BPARAMETER POINT 3,4 6,6");
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let baseline = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let _ = app.dispatch_command("BCYCLEORDER");
    assert_eq!(app.active_modal, Some(ModalKind::BlockCycleOrder));
    assert_eq!(app.block_cycle_order.as_ref().unwrap().rows.len(), 2);
    let _ = app.update(Message::BlockCycleSelect(0));
    let selected_handle = app.block_cycle_order.as_ref().unwrap().rows[0].handle;
    assert_eq!(
        app.block_cycle_order.as_ref().unwrap().rows[0].parameter_type,
        "Linear"
    );
    let _ = app.update(Message::BlockCycleEnable(0, true));
    let _ = app.update(Message::BlockCycleEnable(1, true));
    let _ = app.update(Message::BlockCycleMove(0, false));
    let state = app.block_cycle_order.as_ref().unwrap();
    assert_eq!(state.selected, Some(0));
    assert_ne!(state.rows[state.selected.unwrap()].handle, selected_handle);
    let _ = app.update(Message::BlockCycleSelect(1));
    let _ = app.update(Message::BlockCycleMove(1, true));
    let state = app.block_cycle_order.as_ref().unwrap();
    assert_eq!(state.selected, Some(0));
    assert_eq!(state.rows[0].handle, selected_handle);
    let _ = app.update(Message::CloseModal);
    assert_eq!(app.tabs[i].scene.document, baseline);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    assert!(app.block_cycle_order.is_none());
    let _ = app.dispatch_command("BCYCLEORDER");
    let handles = app
        .block_cycle_order
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .map(|row| row.handle)
        .collect::<Vec<_>>();
    let _ = app.update(Message::BlockCycleEnable(0, true));
    let _ = app.update(Message::BlockCycleEnable(1, true));
    let _ = app.update(Message::BlockCycleMove(0, false));
    let _ = app.update(Message::BlockCycleApply);
    assert!(app.active_modal.is_none());
    let grips = crate::scene::block_grips::grips(&app.tabs[i].scene.document, definition).unwrap();
    assert_eq!(
        grips.iter().map(|grip| grip.handle).collect::<Vec<_>>(),
        vec![handles[1], handles[0]]
    );
    assert!(grips.iter().all(|grip| grip.enabled));
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    app.undo_steps(1);
    assert_eq!(
        crate::scene::block_grips::insertion_points(&app.tabs[i].scene.document, definition),
        crate::scene::block_grips::insertion_points(&baseline, definition)
    );
    app.redo_steps(1);
    let points =
        crate::scene::block_grips::insertion_points(&app.tabs[i].scene.document, definition);
    assert_eq!(points.len(), 2);
    let _ = app.dispatch_command("BCYCLEORDER");
    let _ = app.update(Message::BlockCycleApply);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    let _ = app.dispatch_command("BCYCLEORDER");
    let Some(document::objects::ObjectType::DynamicBlock(object)) =
        app.tabs[i].scene.document.objects.get_mut(&handles[0])
    else {
        panic!()
    };
    let document::objects::DynamicBlockData::LinearGrip(grip) = &mut object.data else {
        panic!()
    };
    grip.grip.location.x += 1.0;
    let externally_changed = app.tabs[i].scene.document.clone();
    let _ = app.update(Message::BlockCycleApply);
    assert_eq!(app.tabs[i].scene.document, externally_changed);
    assert!(app
        .block_cycle_order
        .as_ref()
        .unwrap()
        .error
        .as_ref()
        .unwrap()
        .contains("changed"));
    let _ = app.update(Message::CloseModal);
}

#[test]
fn block_grip_cycling_ctrl_edges_use_saved_order_during_classic_insertion() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2 1");
    let _ = app.run_command_line("BPARAMETER POINT 3,4 6,6");
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let grips = crate::scene::block_grips::grips(&app.tabs[i].scene.document, definition).unwrap();
    let rows = grips
        .iter()
        .rev()
        .map(|g| (g.handle, true))
        .collect::<Vec<_>>();
    crate::scene::block_grips::set_cycle_order(&mut app.tabs[i].scene.document, definition, &rows)
        .unwrap();
    let points =
        crate::scene::block_grips::insertion_points(&app.tabs[i].scene.document, definition);
    app.tabs[i].dirty = true;
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");
    let _ = app.dispatch_command("-INSERT");
    feed(&mut app, "EDIT_ME");
    let _ = app.update(Message::SetModifiers {
        shift: false,
        ctrl: true,
        alt: false,
    });
    let prompt = app.tabs[i].active_cmd.as_ref().unwrap().prompt();
    assert!(prompt.contains("(2/3)"), "{prompt}");
    let _ = app.update(Message::SetModifiers {
        shift: false,
        ctrl: true,
        alt: false,
    });
    assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().prompt(), prompt);
    let _ = app.update(Message::SetModifiers {
        shift: false,
        ctrl: false,
        alt: false,
    });
    assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().prompt(), prompt);
    let _ = app.update(Message::SetModifiers {
        shift: false,
        ctrl: true,
        alt: false,
    });
    assert!(app.tabs[i]
        .active_cmd
        .as_ref()
        .unwrap()
        .prompt()
        .contains("(3/3)"));
    feed(&mut app, "100,200");
    for _ in 0..3 {
        let _ = app.feed_command(crate::command::StepInput::Enter);
    }
    let inserted = app.tabs[i]
        .scene
        .document
        .entities()
        .find_map(|entity| match entity {
            EntityType::Insert(insert)
                if insert.common.handle != reference && insert.block_name == "EDIT_ME" =>
            {
                Some(insert)
            }
            _ => None,
        })
        .unwrap();
    let landed =
        crate::scene::render_graph::insert_transform(&app.tabs[i].scene.document, inserted)
            .apply(points[1]);
    assert!(landed.distance(&Vector3::new(100.0, 200.0, 0.0)) < 1e-8);
    let handle = inserted.common.handle;
    app.undo_steps(1);
    assert!(app.tabs[i].scene.document.get_entity(handle).is_none());
}
