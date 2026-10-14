use super::{Message, ModalKind, OpenCADStudio};
use codec::entities::{Insert, Line};
use codec::types::Vector3;
use codec::{EntityType, Handle};

fn fixture() -> (OpenCADStudio, usize, Handle) {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.tabs[i].scene.define_block_raw(
        "SAMPLE",
        Vector3::new(10.0, 20.0, 0.0),
        vec![EntityType::Line(Line::from_points(
            Vector3::new(10.0, 20.0, 0.0),
            Vector3::new(15.0, 20.0, 0.0),
        ))],
    );
    app.tabs[i]
        .scene
        .document
        .block_records
        .get_mut("SAMPLE")
        .unwrap()
        .base_point = Vector3::new(10.0, 20.0, 0.0);
    let reference = app.tabs[i]
        .scene
        .document
        .add_entity(EntityType::Insert(Insert::new(
            "SAMPLE",
            Vector3::new(100.0, 200.0, 0.0),
        )))
        .unwrap();
    let _ = app.dispatch_command(&format!("BEDIT_BEGIN:{}", reference.value()));
    (app, i, reference)
}

#[test]
fn block_test_uses_unsaved_definition_and_preserves_editor_history_and_view() {
    let (mut app, source, _) = fixture();
    assert_eq!(
        app.automation_op(r#"{"op":"run","cmd":"CIRCLE 12,20 2"}"#)["ok"],
        true
    );
    app.finish_pending_history(source);
    let document = app.tabs[source].scene.document.clone();
    let depth = app.tabs[source].history.undo_stack.len();
    let camera = app.tabs[source].scene.camera.borrow().clone();
    let source_id = app.tabs[source].id;
    assert_eq!(
        app.automation_op(r#"{"op":"run","cmd":"BTESTBLOCK"}"#)["ok"],
        true
    );
    let test = app.active_tab;
    assert_ne!(app.tabs[test].id, source_id);
    assert!(app.tabs[test].active_block_edit.is_none());
    assert_eq!(
        app.tabs[test].scene.document.block_records.get("SAMPLE"),
        document.block_records.get("SAMPLE")
    );
    let model = app.tabs[test]
        .scene
        .document
        .header
        .model_space_block_handle;
    let top: Vec<_> = app.tabs[test]
        .scene
        .document
        .entities()
        .filter(|e| {
            e.common().owner_handle == model
                && !matches!(e, EntityType::Block(_) | EntityType::BlockEnd(_))
        })
        .collect();
    assert_eq!(top.len(), 1);
    let EntityType::Insert(reference) = top[0] else {
        panic!("missing test reference")
    };
    assert_eq!(reference.insert_point, Vector3::ZERO);
    assert_eq!(reference.block_name, "SAMPLE");
    assert!(app.tabs[test].history.undo_stack.is_empty());
    assert_eq!(
        app.automation_op(r#"{"op":"run","cmd":"CIRCLE 1000,1000 8"}"#)["ok"],
        true
    );
    assert_eq!(
        app.automation_op(r#"{"op":"run","cmd":"BCLOSETESTBLOCK"}"#)["ok"],
        true
    );
    assert_eq!(app.active_tab, source);
    assert_eq!(app.tabs[source].scene.document, document);
    assert_eq!(app.tabs[source].history.undo_stack.len(), depth);
    assert_eq!(app.tabs[source].scene.camera.borrow().target, camera.target);
    assert!(app.active_modal.is_none());
    app.undo_steps(1);
    assert_eq!(
        app.tabs[source]
            .scene
            .document
            .entities_in_block("SAMPLE")
            .count(),
        1
    );
}

#[test]
fn block_test_keytips_follow_context_and_offer_close() {
    let (mut app, source, _) = fixture();
    let tab_tip = |app: &OpenCADStudio, target: &str| {
        app.ribbon.keytips_json(false)["tips"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tip| tip["target"] == target)
    };
    app.ribbon.show_keytips(false);
    assert!(!tab_tip(&app, "block_test"));
    let _ = app.ribbon.keytip_type(Some("H"));
    assert!(tab_tip(&app, "LINE"));
    assert!(!tab_tip(&app, "BCLOSETESTBLOCK"));
    let _ = app.ribbon.hide_keytips();
    let _ = app.open_block_test(source);
    app.ribbon.show_keytips(false);
    assert_eq!(app.ribbon.keytips_json(false)["tab"], "draw");
    assert!(!tab_tip(&app, "block_test"));
    assert!(!tab_tip(&app, "block_editor"));
    let _ = app.ribbon.keytip_type(Some("H"));
    assert!(tab_tip(&app, "LINE"));
    assert!(tab_tip(&app, "Close"));
    assert!(tab_tip(&app, "BCLOSETESTBLOCK"));
    let _ = app.ribbon.keytip_type(Some("C"));
    let crate::ui::ribbon::KeyTipStep::Run(close) = app.ribbon.keytip_type(Some("T")) else {
        panic!("close key tip did not dispatch a ribbon action");
    };
    assert!(matches!(
        &close,
        Message::RibbonToolClick { event: crate::modules::ModuleEvent::Command(command), .. }
            if command == "BCLOSETESTBLOCK"
    ));
    let _ = app.ribbon.hide_keytips();
    let _ = app.update(close);
    app.ribbon.show_keytips(false);
    assert!(!tab_tip(&app, "block_test"));
    assert!(tab_tip(&app, "block_editor"));
    assert_eq!(app.ribbon.keytips_json(false)["tab"], "block_editor");
    let _ = app.ribbon.keytip_type(Some("H"));
    assert!(!tab_tip(&app, "Close"));
    assert!(!tab_tip(&app, "BCLOSETESTBLOCK"));
    let _ = app.ribbon.hide_keytips();
}

#[test]
fn block_test_close_x_discards_dirty_edits_and_repeated_test_refreshes() {
    let (mut app, source, _) = fixture();
    let _ = app.open_block_test(source);
    let first = app.tabs[app.active_tab].id;
    let _ = app.open_block_test(app.active_tab);
    assert!(!app.tabs.iter().any(|tab| tab.id == first));
    assert_eq!(
        app.tabs
            .iter()
            .filter(|tab| tab.block_test.is_some())
            .count(),
        1
    );
    let i = app.active_tab;
    app.tabs[i].dirty = true;
    let _ = app.on_tab_close(i);
    assert_eq!(app.active_tab, source);
    assert!(app.pending_close.is_none());
    assert!(app.active_modal.is_none());
}

#[test]
fn block_test_refuses_persistence_and_replacing_the_scratch_drawing() {
    let (mut app, source, _) = fixture();
    let _ = app.open_block_test(source);
    let test = app.active_tab;
    for command in [
        "SAVE",
        "QSAVE",
        "SAVEAS",
        "SAVEALL",
        "BSAVE",
        "BSAVEAS:OTHER",
        "BEDIT_OPEN:SAMPLE",
        "REFEDIT",
    ] {
        let request = serde_json::json!({"op":"run", "cmd":command});
        assert_eq!(
            app.automation_op(&request.to_string())["ok"],
            false,
            "{command}"
        );
    }
    assert_eq!(
        app.automation_op(r#"{"op":"save","path":"blocked-test.dwg"}"#)["ok"],
        false
    );
    assert_eq!(app.automation_op(r#"{"op":"new"}"#)["ok"], false);
    let _ = app.update(Message::SaveFile);
    let _ = app.update(Message::SaveAs);
    assert!(app.active_modal.is_none());
    assert_eq!(app.active_tab, test);
    assert!(app.tabs[test].current_path.is_none());
    assert!(app.active_save_jobs.is_empty());
    app.tabs[test].dirty = true;
    let _ = app.update(Message::AutoSave);
    assert!(app.active_save_jobs.is_empty());
}

#[test]
fn block_test_parent_close_cancel_keeps_test_and_discard_removes_it() {
    let (mut app, source, _) = fixture();
    app.automation_op(r#"{"op":"run","cmd":"CIRCLE 12,20 2"}"#);
    let _ = app.open_block_test(source);
    let test_id = app.tabs[app.active_tab].id;
    let _ = app.on_tab_close(source);
    assert_eq!(app.active_modal, Some(ModalKind::BlockEditChanges));
    let _ = app.update(Message::BlockEditChangesCancel);
    assert!(app.tabs.iter().any(|tab| tab.id == test_id));
    let _ = app.on_tab_close(source);
    let _ = app.update(Message::BlockEditChangesDiscard);
    assert!(!app.tabs.iter().any(|tab| tab.id == test_id));
    assert!(!app.tabs.iter().any(|tab| tab.block_test.is_some()));
}

#[test]
fn block_test_no_editor_is_refused() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let count = app.tabs.len();
    let _ = app.open_block_test(app.active_tab);
    assert_eq!(app.tabs.len(), count);
    assert!(app.command_line.last_error.is_some());
}

#[test]
fn block_test_reordering_before_its_source_keeps_close_targets_stable() {
    let (mut app, source, _) = fixture();
    let source_id = app.tabs[source].id;
    let _ = app.open_block_test(source);
    let test = app.active_tab;
    app.tabs.swap(source, test);
    app.active_tab = test;
    let _ = app.dispatch_command("BEDIT_DISCARD");
    assert!(!app.tabs.iter().any(|tab| tab.block_test.is_some()));
    assert_eq!(app.tabs[app.active_tab].id, source_id);
    assert!(app.tabs[app.active_tab].active_block_edit.is_none());
    assert_eq!(app.tabs.len(), 1);
}

#[test]
fn block_test_unsaved_visibility_states_work_without_changing_the_editor() {
    let (mut app, source, _) = fixture();
    let doc = &mut app.tabs[source].scene.document;
    let owner = doc.block_records.get("SAMPLE").unwrap().handle;
    let first = doc.block_records.get("SAMPLE").unwrap().entity_handles[0];
    let mut line = Line::from_points(Vector3::new(10.0, 23.0, 0.0), Vector3::new(15.0, 23.0, 0.0));
    line.common.owner_handle = owner;
    let second = doc.add_entity(EntityType::Line(line)).unwrap();
    let parameter = crate::scene::block_visibility::create(doc, owner, Vector3::ZERO).unwrap();
    crate::scene::block_visibility::add_state(doc, parameter, "Alternative", 0).unwrap();
    crate::scene::block_visibility::set_visible(doc, parameter, 0, &[second], false);
    crate::scene::block_visibility::set_visible(doc, parameter, 1, &[first], false);
    let original = app.tabs[source].scene.document.clone();
    let _ = app.open_block_test(source);
    let test = app.active_tab;
    let doc = &app.tabs[test].scene.document;
    let model = doc.header.model_space_block_handle;
    let reference = doc
        .entities()
        .find(|e| e.common().owner_handle == model && matches!(e, EntityType::Insert(_)))
        .unwrap()
        .common()
        .handle;
    assert!(doc.dynamic_visibility_for_insert(reference).is_some());
    app.apply_visibility_state(reference, 1);
    app.finish_pending_history(test);
    let doc = &app.tabs[test].scene.document;
    let EntityType::Insert(insert) = doc.get_entity(reference).unwrap() else {
        unreachable!()
    };
    assert_ne!(insert.block_name, "SAMPLE");
    let visible: Vec<_> = doc
        .entities_in_block(&insert.block_name)
        .filter(|e| !e.common().invisible)
        .collect();
    assert_eq!(visible.len(), 1);
    let EntityType::Line(line) = visible[0] else {
        panic!("expected the alternative line")
    };
    assert_eq!(line.start.y, 23.0);
    assert_eq!(app.tabs[source].scene.document, original);
    app.visibility_popup = Some(super::visibility::VisibilityPopup {
        insert_handle: reference,
        anchor: iced::Point::ORIGIN,
        items: vec!["Alternative".into()],
        current: Some(1),
    });
    let _ = app.close_block_test(test);
    assert!(app.visibility_popup.is_none());
    assert_eq!(app.tabs[source].scene.document, original);
}

#[test]
fn block_test_attributes_use_defaults_and_the_stored_base_point() {
    use codec::entities::AttributeDefinition;
    let (mut app, source, _) = fixture();
    let doc = &mut app.tabs[source].scene.document;
    let owner = doc.block_records.get("SAMPLE").unwrap().handle;
    let mut attribute = AttributeDefinition::new("TAG".into(), "Value".into(), "default".into());
    attribute.insertion_point = Vector3::new(12.0, 23.0, 0.0);
    attribute.common.owner_handle = owner;
    let definition = doc
        .add_entity(EntityType::AttributeDefinition(attribute))
        .unwrap();
    let _ = app.open_block_test(source);
    let doc = &app.tabs[app.active_tab].scene.document;
    let model = doc.header.model_space_block_handle;
    let insert = doc
        .entities()
        .find_map(|entity| match entity {
            EntityType::Insert(insert) if insert.common.owner_handle == model => Some(insert),
            _ => None,
        })
        .unwrap();
    assert_eq!(insert.attributes.len(), 1);
    let attribute = &insert.attributes[0];
    assert_eq!(attribute.value, "default");
    assert_eq!(attribute.insertion_point, Vector3::new(2.0, 3.0, 0.0));
    assert_eq!(attribute.attdef_handle, definition);
    assert!(!attribute.common.handle.is_null());
    assert_eq!(attribute.common.owner_handle, insert.common.handle);
}

#[test]
fn block_test_close_others_discards_the_test_without_prompting() {
    let (mut app, source, _) = fixture();
    let _ = app.open_block_test(source);
    app.tabs[app.active_tab].dirty = true;
    let test = app.tabs[app.active_tab].id;
    let _ = app.update(Message::DocTabCloseOthers(source));
    assert!(!app.tabs.iter().any(|tab| tab.id == test));
    assert!(app.active_modal.is_none());
    assert!(app.tabs[source].active_block_edit.is_some());
}
