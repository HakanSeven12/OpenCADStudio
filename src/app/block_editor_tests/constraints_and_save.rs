use super::*;

#[test]
fn block_editor_save_redraws_only_the_saved_blocks_palette_preview() {
    let (mut app, reference) = fixture();
    let i = app.active_tab;
    app.tabs[i].scene.define_block_raw(
        "OTHER",
        Vector3::ZERO,
        vec![EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::UNIT_Y,
        ))],
    );
    app.show_block_palette = true;
    let preview = |app: &OpenCADStudio, name: &str| {
        std::rc::Rc::clone(&app.block_palette.current_entry(name).unwrap().preview)
    };
    begin(&mut app, reference);
    circle(&mut app, 3.);
    // Up to date right before the save, as after any handled message.
    app.refresh_block_palette_if_stale();
    let (edited, other) = (preview(&app, "EDIT_ME"), preview(&app, "OTHER"));
    let _ = app.dispatch_command("BSAVE");
    app.refresh_block_palette_if_stale();
    assert!(!std::rc::Rc::ptr_eq(&preview(&app, "EDIT_ME"), &edited));
    assert!(std::rc::Rc::ptr_eq(&preview(&app, "OTHER"), &other));
}

#[test]
fn block_constraint_display_variable_is_transient_validated_and_resets_on_entry() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let doc = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let dirty = app.tabs[i].dirty;
    assert!(!app.tabs[i].scene.block_constraint_status_mode);
    let _ = app.dispatch_command("BCONSTATUSMODE 1");
    assert!(app.tabs[i].scene.block_constraint_status_mode);
    for input in ["2", "-1", "1.5", "invalid"] {
        let _ = app.dispatch_command(&format!("SETVAR BCONSTATUSMODE {input}"));
        assert!(app.tabs[i].scene.block_constraint_status_mode);
    }
    let _ = app.dispatch_command("SETVAR BCONSTATUSMODE");
    feed(&mut app, "0");
    assert!(!app.tabs[i].scene.block_constraint_status_mode);
    let _ = app.update(Message::ToggleBlockConstraintStatus);
    assert!(app.tabs[i].scene.block_constraint_status_mode);
    assert_eq!(app.tabs[i].scene.document, doc);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    assert_eq!(app.tabs[i].dirty, dirty);
    let _ = app.open_block_test(i);
    assert!(!app.tabs[app.active_tab].scene.block_constraint_status_mode);
    let _ = app.dispatch_command("BCLOSETESTBLOCK");
    assert!(app.tabs[i].scene.block_constraint_status_mode);
    let _ = app.dispatch_command("BCLOSE");
    begin(&mut app, reference);
    assert!(!app.tabs[i].scene.block_constraint_status_mode);
}

#[test]
fn block_constraint_colors_are_staged_validated_and_do_not_write_drawing_history() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let handle = members(&app)[0];
    app.tabs[i]
        .scene
        .parametric_constraint_set_mut(
            crate::scene::parametric_constraints::ParametricScope::Block(definition),
        )
        .add(
            crate::scene::parametric_constraints::ConstraintKind::Fixed,
            vec![crate::scene::parametric_constraints::ParametricRef::whole(
                handle,
            )],
            None,
        );
    let _ = app.dispatch_command("BCONSTATUSMODE 1");
    let report = app.tabs[i].scene.block_constraint_report(definition);
    let _ = app.tabs[i].scene.entity_wires();
    let original = app.block_editor_preferences.clone();
    let doc = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let _ = app.dispatch_command("BESETTINGS");
    let _ = app.update(Message::BlockConstraintColorInput(2, "RGB:10,20,30".into()));
    let _ = app.update(Message::CloseModal);
    assert_eq!(app.block_editor_preferences, original);
    let _ = app.dispatch_command("BESETTINGS");
    let _ = app.update(Message::BlockConstraintColorInput(2, "BYLAYER".into()));
    let _ = app.update(Message::BlockEditorSettingsApply);
    assert_eq!(app.block_editor_preferences, original);
    assert!(app.block_editor_settings.as_ref().unwrap().error.is_some());
    let _ = app.update(Message::BlockConstraintColorInput(2, "RGB:10,20,30".into()));
    let _ = app.update(Message::BlockEditorSettingsApply);
    assert_eq!(
        app.block_editor_preferences.constraint_colors[2],
        "RGB:10,20,30"
    );
    assert!(std::sync::Arc::ptr_eq(
        &report,
        &app.tabs[i].scene.block_constraint_report(definition)
    ));
    assert!(app.tabs[i]
        .scene
        .entity_wires()
        .iter()
        .filter(|wire| wire.name == handle.value().to_string())
        .any(|wire| wire.color == [10. / 255., 20. / 255., 30. / 255., 1.]));
    assert!(app.tabs[i].scene.block_constraint_status_mode);
    assert_eq!(app.tabs[i].scene.document, doc);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    let json = serde_json::to_value(&app.block_editor_preferences).unwrap();
    assert!(json.get("constraint_colors").is_some());
    assert!(json.get("constraint_status_mode").is_none());
}

#[test]
fn block_constraint_status_properties_are_scope_local_read_only_and_refresh_after_changes() {
    use crate::scene::parametric_constraints::{ConstraintKind, ParametricRef, ParametricScope};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let handle = members(&app)[0];
    app.tabs[i].scene.select_entity(handle, true);
    let value = |app: &OpenCADStudio| {
        app.tabs[i]
            .properties
            .sections
            .iter()
            .flat_map(|s| &s.props)
            .find_map(|p| {
                if p.field == "block_constraint_status" {
                    match &p.value {
                        crate::scene::model::object::PropValue::ReadOnly(value) => {
                            Some(value.clone())
                        }
                        _ => panic!("status must be read-only"),
                    }
                } else {
                    None
                }
            })
            .unwrap()
    };
    app.refresh_properties();
    assert_eq!(value(&app), "Unconstrained");
    app.tabs[i]
        .scene
        .parametric_constraint_set_mut(ParametricScope::Block(definition))
        .add(
            ConstraintKind::Fixed,
            vec![ParametricRef::whole(handle)],
            None,
        );
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    app.refresh_properties();
    assert_eq!(value(&app), "Fully constrained");
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    for set in &mut app.tabs[i].scene.parametric_constraints {
        for constraint in &mut set.constraints {
            constraint.enabled = false;
        }
    }
    app.refresh_properties();
    assert_eq!(value(&app), "Unconstrained");
    let _ = app.dispatch_command("BEDIT_DISCARD");
    app.tabs[i].scene.select_entity(reference, true);
    app.refresh_properties();
    assert!(!app.tabs[i]
        .properties
        .sections
        .iter()
        .flat_map(|s| &s.props)
        .any(|p| p.field == "block_constraint_status"));
}

#[test]
fn block_construction_command_history_display_cancel_and_stale_session() {
    use crate::modules::block_editor::construction::Operation;
    use crate::scene::block_construction as construction;
    let (mut app, reference) = fixture();
    let i = app.active_tab;
    let _ = app.dispatch_command("LINE");
    let _ = app.dispatch_command("BCONSTRUCTION CONVERT");
    assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(), "LINE");
    let _ = app.update(Message::CommandEscape);
    begin(&mut app, reference);
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let handles = members(&app);
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let _ = app.dispatch_command("BCONSTRUCTION");
    let _ = app.feed_command(crate::command::StepInput::SelectionComplete(
        handles.clone(),
    ));
    let _ = app.update(Message::CommandEscape);
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    app.tabs[i].scene.select_entity(handles[0], true);
    app.refresh_properties();
    let _ = app.dispatch_command("BCONSTRUCTION CONVERT");
    assert!(app.tabs[i].active_cmd.is_none());
    assert!(construction::is_construction(
        app.tabs[i]
            .scene
            .document
            .get_entity(handles[0])
            .unwrap()
            .common()
    ));
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    let assert_history_label = |app: &OpenCADStudio, redo: bool| {
        let history = &app.tabs[app.active_tab].history;
        let stack = if redo {
            &history.redo_stack
        } else {
            &history.undo_stack
        };
        let snapshot = stack.last().unwrap();
        assert_eq!(snapshot.label(), "Edit construction geometry");
        assert!(matches!(snapshot,
            super::super::tab_document::HistorySnapshot::Delta(delta)
                if matches!(delta.label, super::super::tab_document::HistoryLabel::Message(_))));
        let expected = crate::t!("Edit construction geometry").into_owned();
        assert_eq!(
            super::super::history::history_dropdown_labels(stack).first(),
            Some(&expected)
        );
        let crate::ui::popup::context_menu::MenuContext::Idle {
            undo_label,
            redo_label,
            ..
        } = app.context_menu_context()
        else {
            panic!("construction command must leave an idle context menu");
        };
        assert_eq!(if redo { redo_label } else { undo_label }, Some(expected));
    };
    assert_history_label(&app, false);
    let changed = app.tabs[i].scene.document.clone();
    for command in ["BCONSTRUCTION HIDE", "BCONSTRUCTION SHOW ALL"] {
        let _ = app.dispatch_command(command);
        assert_eq!(app.tabs[i].scene.document, changed);
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    }
    app.tabs[i].scene.select_entity(handles[0], true);
    app.refresh_properties();
    for field in ["layer", "color", "linetype"] {
        assert!(app.tabs[i]
            .properties
            .sections
            .iter()
            .flat_map(|s| &s.props)
            .filter(|p| p.field == field)
            .all(|p| matches!(p.value, crate::scene::model::object::PropValue::ReadOnly(_))));
    }
    let _ = app.dispatch_command("BCONSTRUCTION CONVERT");
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    let _ = app.set_block_construction(crate::command::CmdResult::SetBlockConstruction {
        document: app.tabs[i].id + 1,
        definition,
        handles: handles.clone(),
        operation: Operation::Revert,
    });
    assert_eq!(app.tabs[i].scene.document, changed);
    let _ = app.dispatch_command("BCONSTRUCTION REVERT");
    assert!(!construction::is_construction(
        app.tabs[i]
            .scene
            .document
            .get_entity(handles[0])
            .unwrap()
            .common()
    ));
    app.undo_steps(1);
    assert_history_label(&app, true);
    assert!(construction::is_construction(
        app.tabs[i]
            .scene
            .document
            .get_entity(handles[0])
            .unwrap()
            .common()
    ));
    app.undo_steps(1);
    assert!(!construction::is_construction(
        app.tabs[i]
            .scene
            .document
            .get_entity(handles[0])
            .unwrap()
            .common()
    ));
    app.redo_steps(2);
    assert_history_label(&app, false);
    assert!(!construction::is_construction(
        app.tabs[i]
            .scene
            .document
            .get_entity(handles[0])
            .unwrap()
            .common()
    ));
    let _ = app.dispatch_command("BEDIT_DISCARD");
    let closed = app.tabs[i].scene.document.clone();
    let _ = app.set_block_construction(crate::command::CmdResult::SetBlockConstruction {
        document: app.tabs[i].id,
        definition,
        handles,
        operation: Operation::Convert,
    });
    assert_eq!(app.tabs[i].scene.document, closed);
}

#[test]
fn block_construction_save_discard_test_block_and_reopen_preserve_membership() {
    use crate::scene::block_construction as construction;
    for save in [false, true] {
        let (mut app, reference) = fixture();
        begin(&mut app, reference);
        let i = app.active_tab;
        let handle = members(&app)[0];
        app.tabs[i].scene.select_entity(handle, true);
        let _ = app.dispatch_command("BCONSTRUCTION CONVERT");
        let source = app.tabs[i].scene.document.clone();
        let _ = app.open_block_test(i);
        let test = app.active_tab;
        assert!(app.tabs[test].block_test.is_some());
        assert!(app.tabs[test].scene.block_edit_block.is_none());
        assert!(app.tabs[test].scene.display_entity_invisible(
            app.tabs[test]
                .scene
                .document
                .get_entity(handle)
                .unwrap()
                .common()
        ));
        assert_eq!(app.tabs[i].scene.document, source);
        let _ = app.dispatch_command("BCLOSETESTBLOCK");
        assert_eq!(app.active_tab, i);
        if save {
            let _ = app.dispatch_command("BSAVE");
            let _ = app.dispatch_command("BCLOSE");
        } else {
            let _ = app.dispatch_command("BEDIT_DISCARD");
        }
        assert_eq!(
            construction::is_construction(
                app.tabs[i]
                    .scene
                    .document
                    .get_entity(handle)
                    .unwrap()
                    .common()
            ),
            save
        );
        if save {
            for extension in ["dwg", "dxf"] {
                let doc = &app.tabs[i].scene.document;
                let data = crate::io::save_to_bytes(doc, extension, doc.version).unwrap();
                let reopened =
                    crate::io::load_bytes(&format!("construction-editor.{extension}"), data)
                        .unwrap();
                assert!(construction::is_construction(
                    reopened.get_entity(handle).unwrap().common()
                ));
                assert!(reopened.get_entity(handle).unwrap().common().invisible);
            }
        }
    }
}
