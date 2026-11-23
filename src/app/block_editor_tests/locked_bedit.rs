use super::*;

#[test]
fn locked_bedit_no_selection_opens_edit_block_definition_dialog() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let tab = app.active_tab;
    app.tabs[tab].scene.define_block_raw(
        "DOOR",
        Vector3::ZERO,
        vec![EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::UNIT_X,
        ))],
    );
    app.tabs[tab]
        .scene
        .document
        .block_records
        .get_mut("DOOR")
        .unwrap()
        .description = "Wooden door".to_string();
    app.tabs[tab]
        .scene
        .define_block_raw("WINDOW", Vector3::ZERO, vec![]);

    app.tabs[tab].scene.deselect_all();
    let _ = app.dispatch_command("BEDIT");

    assert_eq!(app.active_modal, Some(ModalKind::BlockEditPicker));
    let picker = app.block_edit_picker.as_ref().expect("picker state set");
    assert_eq!(picker.existing_names, vec!["DOOR", "WINDOW"]);
    assert_eq!(picker.name, "DOOR");
    assert_eq!(picker.description, "Wooden door");
    assert!(picker.error.is_none());
}

#[test]
fn locked_bedit_dialog_select_existing_block() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let tab = app.active_tab;
    app.tabs[tab]
        .scene
        .define_block_raw("DOOR", Vector3::ZERO, vec![]);
    app.tabs[tab]
        .scene
        .define_block_raw("WINDOW", Vector3::ZERO, vec![]);

    app.tabs[tab].scene.deselect_all();
    let _ = app.dispatch_command("BEDIT");

    let _ = app.update(Message::BlockEditPickerSelect("WINDOW".to_string()));
    let picker = app.block_edit_picker.as_ref().unwrap();
    assert_eq!(picker.name, "WINDOW");

    let _ = app.update(Message::BlockEditPickerApply);
    assert_eq!(app.active_modal, None);
    assert!(app.tabs[tab].active_block_edit.is_some());
    assert_eq!(
        app.tabs[tab]
            .active_block_edit_session()
            .unwrap()
            .block_name,
        "WINDOW"
    );
}

#[test]
fn locked_bedit_dialog_name_validation() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let tab = app.active_tab;
    app.tabs[tab].scene.deselect_all();
    let _ = app.dispatch_command("BEDIT");

    let _ = app.update(Message::BlockEditPickerName("INVALID/NAME".to_string()));
    assert!(app.block_edit_picker.as_ref().unwrap().error.is_some());

    let _ = app.update(Message::BlockEditPickerApply);
    assert_eq!(app.active_modal, Some(ModalKind::BlockEditPicker));
    assert!(app.tabs[tab].active_block_edit.is_none());

    let _ = app.update(Message::BlockEditPickerName("".to_string()));
    let _ = app.update(Message::BlockEditPickerApply);
    assert_eq!(
        app.block_edit_picker.as_ref().unwrap().error.as_deref(),
        Some("The name must be at least one character long."),
        "an empty name is answered with the reason it was refused"
    );
    assert_eq!(app.active_modal, Some(ModalKind::BlockEditPicker));
    assert!(app.tabs[tab].active_block_edit.is_none());

    let _ = app.update(Message::BlockEditPickerName("USABLE".to_string()));
    assert!(
        app.block_edit_picker.as_ref().unwrap().error.is_none(),
        "typing a name retires the refusal"
    );

    let _ = app.update(Message::CloseModal);
    assert_eq!(app.active_modal, None);
}

#[test]
fn locked_bedit_dialog_create_new_block_closes_without_saving() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let tab = app.active_tab;
    app.tabs[tab].scene.deselect_all();
    let _ = app.dispatch_command("BEDIT");

    let _ = app.update(Message::BlockEditPickerName("BRAND_NEW".to_string()));
    assert!(app.block_edit_picker.as_ref().unwrap().error.is_none());

    let _ = app.update(Message::BlockEditPickerApply);
    assert_eq!(app.active_modal, None);
    assert!(app.tabs[tab].active_block_edit.is_some());
    assert_eq!(
        app.tabs[tab]
            .active_block_edit_session()
            .unwrap()
            .block_name,
        "BRAND_NEW"
    );
    assert!(!app.block_edit_dirty(tab));

    let _ = app.dispatch_command("BCLOSE");
    assert_eq!(app.active_modal, None);
    assert!(app.tabs[tab].active_block_edit.is_none());
    assert!(
        app.tabs[tab]
            .scene
            .document
            .block_records
            .get("BRAND_NEW")
            .is_none(),
        "unpersisted new block closed without saving must not remain in drawing"
    );
}

#[test]
fn locked_bedit_dialog_create_new_block_discard_dirty_leaves_no_block() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let tab = app.active_tab;
    app.tabs[tab].scene.deselect_all();
    let _ = app.dispatch_command("BEDIT");

    let _ = app.update(Message::BlockEditPickerName("BRAND_NEW2".to_string()));
    let _ = app.update(Message::BlockEditPickerApply);
    assert!(app.tabs[tab].active_block_edit.is_some());

    circle(&mut app, 1.0);
    assert!(app.block_edit_dirty(tab));

    let _ = app.dispatch_command("BCLOSE");
    assert_eq!(app.active_modal, Some(ModalKind::BlockEditChanges));

    let _ = app.update(Message::BlockEditChangesDiscard);
    assert_eq!(app.active_modal, None);
    assert!(app.tabs[tab].active_block_edit.is_none());
    assert!(
        app.tabs[tab]
            .scene
            .document
            .block_records
            .get("BRAND_NEW2")
            .is_none(),
        "discarded new block must not remain in drawing"
    );
}

#[test]
fn locked_bedit_dialog_create_new_block_save_persists_and_undoes() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let tab = app.active_tab;
    app.tabs[tab].scene.deselect_all();
    let _ = app.dispatch_command("BEDIT");

    let _ = app.update(Message::BlockEditPickerName("SAVED_BLOCK".to_string()));
    let _ = app.update(Message::BlockEditPickerApply);
    assert!(app.tabs[tab].active_block_edit.is_some());

    circle(&mut app, 1.0);
    let _ = app.dispatch_command("BSAVE");
    assert!(app.tabs[tab]
        .scene
        .document
        .block_records
        .get("SAVED_BLOCK")
        .is_some());

    let _ = app.dispatch_command("BCLOSE");
    assert!(app.tabs[tab].active_block_edit.is_none());
    assert!(app.tabs[tab]
        .scene
        .document
        .block_records
        .get("SAVED_BLOCK")
        .is_some());

    app.undo_steps(1);
    assert!(
        app.tabs[tab]
            .scene
            .document
            .block_records
            .get("SAVED_BLOCK")
            .is_none(),
        "undo must revert block creation"
    );

    app.redo_steps(1);
    assert!(
        app.tabs[tab]
            .scene
            .document
            .block_records
            .get("SAVED_BLOCK")
            .is_some(),
        "redo must restore block creation"
    );
}

#[test]
fn locked_bedit_with_preselected_insert_opens_directly() {
    let (mut app, reference) = fixture();
    let tab = app.active_tab;
    app.tabs[tab].scene.deselect_all();
    app.tabs[tab].scene.select_entity(reference, true);

    let _ = app.dispatch_command("BEDIT");
    assert_eq!(app.active_modal, None);
    assert!(app.tabs[tab].active_block_edit.is_some());
    assert_eq!(
        app.tabs[tab]
            .active_block_edit_session()
            .unwrap()
            .block_name,
        "EDIT_ME"
    );
}

#[test]
fn locked_bedit_clean_close_preserves_block_epoch_and_render_caches() {
    let (mut app, reference) = fixture();
    let tab = app.active_tab;
    let epoch_before = app.tabs[tab].scene.block_epoch;
    begin(&mut app, reference);
    let _ = app.dispatch_command("BCLOSE");
    assert_eq!(app.tabs[tab].scene.block_epoch, epoch_before);
}

#[test]
fn locked_bedit_discard_preserves_block_epoch_and_render_caches() {
    let (mut app, reference) = fixture();
    let tab = app.active_tab;
    let epoch_before = app.tabs[tab].scene.block_epoch;
    begin(&mut app, reference);
    circle(&mut app, 1.0);
    let _ = app.dispatch_command("BCLOSE");
    let _ = app.update(Message::BlockEditChangesDiscard);
    assert_eq!(app.tabs[tab].scene.block_epoch, epoch_before);
}
