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
fn block_test_initial_extents_include_dynamic_action_grips() {
    use crate::scene::{block_actions, block_distance_parameter as distance};

    let (mut app, source, _) = fixture();
    let definition = app.tabs[source]
        .active_block_edit_session()
        .unwrap()
        .br_handle;
    let line = app.tabs[source]
        .scene
        .document
        .block_records
        .get("SAMPLE")
        .unwrap()
        .entity_handles[0];
    let document = &mut app.tabs[source].scene.document;
    let parameter_handle = distance::create(
        document,
        definition,
        distance::Kind::Linear,
        Vector3::new(10., 20., 0.),
        Vector3::new(110., 20., 0.),
        Vector3::new(10., 22., 0.),
    )
    .unwrap();
    block_actions::create(
        document,
        definition,
        block_actions::Kind::Move,
        parameter_handle,
        &[line],
        glam::DVec3::new(10., 20., 0.),
    )
    .unwrap();

    let _ = app.open_block_test(source);
    let test = app.active_tab;
    let session = app.tabs[test].block_test.as_ref().unwrap();
    let EntityType::Insert(insert) = app.tabs[test]
        .scene
        .document
        .get_entity(session.insert)
        .unwrap()
    else {
        panic!("Test Block reference missing")
    };
    let transform =
        crate::scene::render_graph::insert_transform(&app.tabs[test].scene.document, insert);
    let p = block_actions::all_parameters(&app.tabs[test].scene.document, definition)
        .into_iter()
        .find(|parameter| parameter.handle == parameter_handle)
        .expect("authored parameter missing");
    let value = block_actions::value(&app.tabs[test].scene.document, &p).unwrap();
    let grip = block_actions::v(transform.apply(block_actions::native(
        block_actions::grip_position(&p, value),
    )));
    let bounds = iced::Rectangle::with_size(iced::Size::new(1600., 900.));
    let projected = app.tabs[test]
        .scene
        .camera
        .borrow()
        .project(grip, bounds)
        .expect("dynamic grip should project");
    assert!(
        (0.0..=bounds.width).contains(&projected.x) && (0.0..=bounds.height).contains(&projected.y),
        "Test Block should initially frame the action grip: {projected:?}"
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

/// A door, a window frame and a cat in `SAMPLE`, edited in a block test, with
/// a `WindowHidden` state controlling only the frame. Returns the test tab,
/// its model-space reference and the hidden state's index.
fn visibility_reference() -> (OpenCADStudio, usize, Handle, usize) {
    let (mut app, source, _) = fixture();
    let doc = &mut app.tabs[source].scene.document;
    let owner = doc.block_records.get("SAMPLE").unwrap().handle;
    let mut frame = Line::from_points(
        Vector3::new(10.0, 21.0, 0.0),
        Vector3::new(15.0, 21.0, 0.0),
    );
    frame.common.owner_handle = owner;
    let frame = doc.add_entity(EntityType::Line(frame)).unwrap();
    let mut cat = Line::from_points(
        Vector3::new(10.0, 22.0, 0.0),
        Vector3::new(15.0, 22.0, 0.0),
    );
    cat.common.owner_handle = owner;
    doc.add_entity(EntityType::Line(cat)).unwrap();
    let parameter = crate::scene::block_visibility::create(doc, owner, Vector3::ZERO).unwrap();
    let hidden =
        crate::scene::block_visibility::add_state(doc, parameter, "WindowHidden", 0).unwrap();
    crate::scene::block_visibility::set_visible(doc, parameter, hidden, &[frame], false);

    let _ = app.open_block_test(source);
    let test = app.active_tab;
    let model = app.tabs[test].scene.document.header.model_space_block_handle;
    let reference = app.tabs[test]
        .scene
        .document
        .entities()
        .find(|e| e.common().owner_handle == model && matches!(e, EntityType::Insert(_)))
        .unwrap()
        .common()
        .handle;
    (app, test, reference, hidden)
}

/// A state hides only the objects its parameter controls. A reference to the
/// definition keeps drawing the members the state never mentions.
#[test]
fn block_test_visibility_state_hides_only_the_controlled_member() {
    let (mut app, test, reference, hidden) = visibility_reference();
    app.apply_visibility_state(reference, hidden);
    app.finish_pending_history(test);

    let doc = &app.tabs[test].scene.document;
    let EntityType::Insert(insert) = doc.get_entity(reference).unwrap() else {
        unreachable!()
    };
    let mut visible: Vec<f64> = doc
        .entities_in_block(&insert.block_name)
        .filter(|entity| !entity.common().invisible)
        .filter_map(|entity| match entity {
            EntityType::Line(line) => Some(line.start.y),
            _ => None,
        })
        .collect();
    visible.sort_by(f64::total_cmp);
    assert_eq!(
        visible,
        vec![20.0, 22.0],
        "the door and the cat stay visible; only the window frame is hidden"
    );

    let Some(crate::scene::model::object::Property {
        value:
            crate::scene::model::object::PropValue::ChoiceWithTooltip {
                selected, options, ..
            },
        ..
    }) = OpenCADStudio::visibility_property(doc, reference)
    else {
        panic!("a visibility row for the reference")
    };
    assert_eq!(options, ["VisibilityState0", "WindowHidden"]);
    assert_eq!(selected, "WindowHidden");

    app.tabs[test].scene.select_entity(reference, true);
    app.tabs[test].selected_handle = Some(reference);
    app.refresh_visibility_grip([0.0, 0.0, 0.0]);
    assert_eq!(
        app.tabs[test]
            .visibility_grip
            .as_ref()
            .and_then(|grip| grip.current),
        Some(hidden),
        "the popup ticks the state the reference shows"
    );
}

/// The palette names the state once, on the editable `Visibility1` row; it
/// neither lists every state nor repeats the parameter name as a value.
#[test]
fn block_visibility_state_is_shown_once_as_the_editable_row() {
    let (mut app, test, reference, hidden) = visibility_reference();
    app.apply_visibility_state(reference, hidden);
    app.finish_pending_history(test);
    app.tabs[test].scene.select_entity(reference, true);
    app.tabs[test].selected_handle = Some(reference);
    app.refresh_properties();

    let rows: Vec<&crate::scene::model::object::Property> =
        app.tabs[test].properties.sections.iter().flat_map(|s| &s.props).collect();
    let visibility: Vec<&&crate::scene::model::object::Property> = rows
        .iter()
        .filter(|row| row.field == "dyn_visibility")
        .collect();
    assert_eq!(visibility.len(), 1, "one row carries the state");
    assert!(
        matches!(&visibility[0].value, crate::scene::model::object::PropValue::ChoiceWithTooltip { selected, .. } if selected == "WindowHidden"),
        "the row shows the current state: {:?}",
        visibility[0].value
    );
    assert!(app.tabs[test].properties.choice_combos.contains_key("dyn_visibility"));
    for label in ["Visibility", "Visibility states"] {
        assert!(
            !rows.iter().any(|row| row.label == label),
            "the palette repeats the state as '{label}'"
        );
    }
}

#[test]
fn block_test_lookup_choice_evaluates_temporary_reference_only() {
    use crate::scene::model::object::PropValue;
    use crate::scene::{block_actions, block_distance_parameter as distance, block_lookup};
    use codec::objects::BlockLookupAction;

    let (mut app, source, _) = fixture();
    let definition = app.tabs[source]
        .active_block_edit_session()
        .unwrap()
        .br_handle;
    let line = app.tabs[source]
        .scene
        .document
        .block_records
        .get("SAMPLE")
        .unwrap()
        .entity_handles[0];
    let doc = &mut app.tabs[source].scene.document;
    let parameter = distance::create(
        doc,
        definition,
        distance::Kind::Linear,
        Vector3::new(10.0, 20.0, 0.0),
        Vector3::new(12.0, 20.0, 0.0),
        Vector3::new(10.0, 21.0, 0.0),
    )
    .unwrap();
    block_actions::create(
        doc,
        definition,
        block_actions::Kind::Move,
        parameter,
        &[line],
        glam::DVec3::new(10.0, 20.0, 0.0),
    )
    .unwrap();
    let lookup_parameter =
        block_lookup::create(doc, definition, Vector3::new(11.0, 21.0, 0.0)).unwrap();
    let mut table = block_lookup::empty_table(
        block_lookup::parameters(doc, definition)
            .into_iter()
            .find(|(handle, _)| *handle == lookup_parameter)
            .unwrap()
            .1,
    );
    table.columns.insert(
        0,
        block_lookup::properties(doc, definition)[0].column.clone(),
    );
    table.column_count = 2;
    table.row_count = 2;
    table.expressions = vec!["2".into(), "Small".into(), "4".into(), "Large".into()];
    block_lookup::apply_table(doc, definition, lookup_parameter, table).unwrap();

    let source_before = app.tabs[source].scene.document.clone();
    let _ = app.open_block_test(source);
    let test = app.active_tab;
    let insert = app.tabs[test].block_test.as_ref().unwrap().insert;
    app.tabs[test].scene.selected.insert(insert);
    app.tabs[test].selected_handle = Some(insert);
    app.refresh_properties();
    let lookup_property = app.tabs[test]
        .properties
        .sections
        .iter()
        .flat_map(|section| &section.props)
        .find(|property| property.field == "dyn_lookup")
        .expect("Test Block should expose the Lookup table choice");
    let PropValue::BlockChoice { key, selected, options, .. } = &lookup_property.value else {
        panic!("Lookup table property should be a choice")
    };
    assert_eq!(*selected, 0);
    assert_eq!(options, &["Small", "Large"]);

    let key = *key;
    let _ = app.update(super::Message::PropBlockChoice { key, index: 1 });
    let EntityType::Insert(evaluated) = app.tabs[test].scene.document.get_entity(insert).unwrap()
    else {
        panic!("expected the temporary Test Block reference")
    };
    assert_ne!(evaluated.block_name, "SAMPLE");
    let member = app.tabs[test]
        .scene
        .document
        .block_records
        .get(&evaluated.block_name)
        .unwrap()
        .entity_handles[0];
    let EntityType::Line(line) = app.tabs[test].scene.document.get_entity(member).unwrap() else {
        panic!("expected the materialized line")
    };
    assert_eq!(line.start, Vector3::new(12.0, 20.0, 0.0));
    assert_eq!(line.end, Vector3::new(17.0, 20.0, 0.0));
    assert_eq!(app.tabs[source].scene.document, source_before);
    assert!(matches!(
        block_lookup::table(&app.tabs[test].scene.document, definition, lookup_parameter).unwrap(),
        Some((_, BlockLookupAction { row_count: 2, .. }))
    ));
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
