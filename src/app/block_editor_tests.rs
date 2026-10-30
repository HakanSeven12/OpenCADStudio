use super::{Message, ModalKind, OpenCADStudio};
use codec::entities::{Insert, Line};
use codec::types::Vector3;
use codec::{EntityType, Handle};

mod retained_save_as;
mod table_properties;
mod lookup_properties;
mod visibility_properties;
mod grip_descriptions;
mod flip_state_labels;
mod properties_internal_rows;

fn fixture() -> (OpenCADStudio, Handle) {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.tabs[i].scene.define_block_raw(
        "EDIT_ME",
        Vector3::ZERO,
        vec![EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::UNIT_X,
        ))],
    );
    let reference = app.tabs[i]
        .scene
        .document
        .add_entity(EntityType::Insert(Insert::new("EDIT_ME", Vector3::ZERO)))
        .unwrap();
    app.tabs[i].dirty = false;
    (app, reference)
}

fn begin(app: &mut OpenCADStudio, reference: Handle) {
    let _ = app.dispatch_command(&format!("BEDIT_BEGIN:{}", reference.value()));
    assert!(app.tabs[app.active_tab].active_block_edit.is_some());
}

fn circle(app: &mut OpenCADStudio, x: f64) {
    let result = app.automation_op(&format!(r#"{{"op":"run","cmd":"CIRCLE {x},0 2"}}"#));
    assert_eq!(result["ok"], true, "{result}");
    app.finish_pending_history(app.active_tab);
}

fn members(app: &OpenCADStudio) -> Vec<Handle> {
    app.tabs[app.active_tab]
        .scene
        .document
        .block_records
        .get("EDIT_ME")
        .unwrap()
        .entity_handles
        .clone()
}

fn main_depth(app: &OpenCADStudio) -> usize {
    let tab = &app.tabs[app.active_tab];
    tab.block_edit_history
        .as_ref()
        .map(|h| h.main.undo_stack.len())
        .unwrap_or(tab.history.undo_stack.len())
}

fn feed(app: &mut OpenCADStudio, text: &str) {
    let _ = app.update(Message::CommandInput(text.to_string()));
    let _ = app.update(Message::CommandSubmit);
}

fn edit_action_set(app: &mut OpenCADStudio, action: Handle, mode: &str, targets: Vec<Handle>) {
    let _ = app.dispatch_command("BACTIONBARMODE 0");
    app.tabs[app.active_tab].selected_authoring = Some(action);
    let _ = app.dispatch_command("BACTIONSET");
    feed(app, mode);
    let _ = app.feed_command(crate::command::StepInput::SelectionComplete(targets));
    let _ = app.feed_command(crate::command::StepInput::Enter);
    assert!(app.tabs[app.active_tab].active_cmd.is_none());
}

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
    app.tabs[i].scene.parametric_constraint_set_mut(crate::scene::parametric_constraints::ParametricScope::Block(definition))
        .add(crate::scene::parametric_constraints::ConstraintKind::Fixed,
            vec![crate::scene::parametric_constraints::ParametricRef::whole(handle)], None);
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
    assert_eq!(app.block_editor_preferences.constraint_colors[2], "RGB:10,20,30");
    assert!(std::sync::Arc::ptr_eq(&report, &app.tabs[i].scene.block_constraint_report(definition)));
    assert!(app.tabs[i].scene.entity_wires().iter().filter(|wire| wire.name == handle.value().to_string())
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
    let value = |app: &OpenCADStudio| app.tabs[i].properties.sections.iter().flat_map(|s| &s.props)
        .find_map(|p| if p.field == "block_constraint_status" {
            match &p.value { crate::scene::model::object::PropValue::ReadOnly(value) => Some(value.clone()), _ => panic!("status must be read-only") }
        } else { None }).unwrap();
    app.refresh_properties();
    assert_eq!(value(&app), "Unconstrained");
    app.tabs[i].scene.parametric_constraint_set_mut(ParametricScope::Block(definition))
        .add(ConstraintKind::Fixed, vec![ParametricRef::whole(handle)], None);
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    app.refresh_properties();
    assert_eq!(value(&app), "Fully constrained");
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    for set in &mut app.tabs[i].scene.parametric_constraints {
        for constraint in &mut set.constraints { constraint.enabled = false; }
    }
    app.refresh_properties();
    assert_eq!(value(&app), "Unconstrained");
    let _ = app.dispatch_command("BEDIT_DISCARD");
    app.tabs[i].scene.select_entity(reference, true);
    app.refresh_properties();
    assert!(!app.tabs[i].properties.sections.iter().flat_map(|s| &s.props).any(|p| p.field == "block_constraint_status"));
}

#[test]
fn block_construction_command_history_display_cancel_and_stale_session() {
    use crate::scene::block_construction as construction;
    use crate::modules::block_editor::construction::Operation;
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
    let _ = app.feed_command(crate::command::StepInput::SelectionComplete(handles.clone()));
    let _ = app.update(Message::CommandEscape);
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    app.tabs[i].scene.select_entity(handles[0], true);
    app.refresh_properties();
    let _ = app.dispatch_command("BCONSTRUCTION CONVERT");
    assert!(app.tabs[i].active_cmd.is_none());
    assert!(construction::is_construction(app.tabs[i].scene.document.get_entity(handles[0]).unwrap().common()));
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    let changed = app.tabs[i].scene.document.clone();
    for command in ["BCONSTRUCTION HIDE", "BCONSTRUCTION SHOW ALL"] {
        let _ = app.dispatch_command(command);
        assert_eq!(app.tabs[i].scene.document, changed);
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    }
    app.tabs[i].scene.select_entity(handles[0], true);
    app.refresh_properties();
    for field in ["layer", "color", "linetype"] {
        assert!(app.tabs[i].properties.sections.iter().flat_map(|s| &s.props)
            .filter(|p| p.field == field).all(|p| matches!(p.value, crate::scene::model::object::PropValue::ReadOnly(_))));
    }
    let _ = app.dispatch_command("BCONSTRUCTION CONVERT");
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    let _ = app.set_block_construction(crate::command::CmdResult::SetBlockConstruction {
        document: app.tabs[i].id + 1, definition, handles: handles.clone(), operation: Operation::Revert });
    assert_eq!(app.tabs[i].scene.document, changed);
    let _ = app.dispatch_command("BCONSTRUCTION REVERT");
    assert!(!construction::is_construction(app.tabs[i].scene.document.get_entity(handles[0]).unwrap().common()));
    app.undo_steps(1);
    assert!(construction::is_construction(app.tabs[i].scene.document.get_entity(handles[0]).unwrap().common()));
    app.undo_steps(1);
    assert!(!construction::is_construction(app.tabs[i].scene.document.get_entity(handles[0]).unwrap().common()));
    app.redo_steps(2);
    assert!(!construction::is_construction(app.tabs[i].scene.document.get_entity(handles[0]).unwrap().common()));
    let _ = app.dispatch_command("BEDIT_DISCARD");
    let closed = app.tabs[i].scene.document.clone();
    let _ = app.set_block_construction(crate::command::CmdResult::SetBlockConstruction {
        document: app.tabs[i].id, definition, handles, operation: Operation::Convert });
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
        assert!(app.tabs[test].scene.display_entity_invisible(app.tabs[test].scene.document.get_entity(handle).unwrap().common()));
        assert_eq!(app.tabs[i].scene.document, source);
        let _ = app.dispatch_command("BCLOSETESTBLOCK");
        assert_eq!(app.active_tab, i);
        if save { let _ = app.dispatch_command("BSAVE"); let _ = app.dispatch_command("BCLOSE"); }
        else { let _ = app.dispatch_command("BEDIT_DISCARD"); }
        assert_eq!(construction::is_construction(app.tabs[i].scene.document.get_entity(handle).unwrap().common()), save);
        if save {
            for extension in ["dwg", "dxf"] {
                let doc = &app.tabs[i].scene.document;
                let data = crate::io::save_to_bytes(doc, extension, doc.version).unwrap();
                let reopened = crate::io::load_bytes(&format!("construction-editor.{extension}"), data).unwrap();
                assert!(construction::is_construction(reopened.get_entity(handle).unwrap().common()));
                assert!(reopened.get_entity(handle).unwrap().common().invisible);
            }
        }
    }
}

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
        let _ = app.handle_block_action_result(crate::command::CmdResult::EvaluateBlockParameter { grip: None,
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
    line.common.color = codec::types::Color::Index(3);
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
            codec::types::Color::Index(3)
        );
    }
    let bytes = codec::DwgWriter::write_to_vec(&app.tabs[i].scene.document).unwrap();
    app.tabs[i].scene.document = codec::DwgReader::from_stream(std::io::Cursor::new(bytes))
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
    let _ = app.handle_block_action_result(crate::command::CmdResult::EvaluateBlockParameter { grip: None,
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
        .color = codec::types::Color::Index(1);
    app.commit_layer_undo(i, pending);
    let _ = app.dispatch_command("BSAVE");
    assert!(
        matches!(app.tabs[i].history.undo_stack.last(),Some(super::document::HistorySnapshot::Delta(d)) if matches!(d.structure,Some(super::document::StructureSnapshot::Combined(_))))
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
        codec::types::Color::Index(1)
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
    let _ = app.handle_block_action_result(crate::command::CmdResult::EvaluateBlockParameter { grip: None,
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
    let bytes = codec::DwgWriter::write_to_vec(doc).unwrap();
    let reopened = codec::DwgReader::from_stream(std::io::Cursor::new(bytes))
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
    let _ = app.handle_block_action_result(crate::command::CmdResult::EvaluateBlockParameter { grip: None,
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
        .find(|g| super::block_actions::is_test_grip(g.id))
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
    let bytes = codec::DwgWriter::write_to_vec(&app.tabs[i].scene.document).unwrap();
    app.tabs[i].scene.document = codec::DwgReader::from_stream(std::io::Cursor::new(bytes))
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
        .find(|g| super::block_actions::is_test_grip(g.id))
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
        .find(|g| super::block_actions::is_test_grip(g.id))
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
    let bytes = codec::DwgWriter::write_to_vec(&changed).unwrap();
    app.tabs[i].scene.document = codec::DwgReader::from_stream(std::io::Cursor::new(bytes))
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

#[test]
fn block_parameters_rotation_only_save_close_reopen_then_flip() {
    use crate::scene::block_actions::{self as actions, Kind};
    for save_command in ["BEDIT_SAVE", "BSAVE"] {
        let (mut app, reference) = fixture();
        begin(&mut app, reference);
        let i = app.active_tab;
        let def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
        let _ = app.run_command_line("BPARAMETER ROTATION 0,0 10 0");
        assert_eq!(
            actions::parameters(&app.tabs[i].scene.document, def, Kind::Rotate).len(),
            1
        );
        let _ = app.dispatch_command(save_command);
        if save_command == "BSAVE" {
            let _ = app.dispatch_command("BCLOSE");
        }
        assert!(app.tabs[i].active_block_edit.is_none());
        begin(&mut app, reference);
        assert_eq!(
            actions::parameters(&app.tabs[i].scene.document, def, Kind::Rotate).len(),
            1,
            "{save_command}"
        );
        let _ = app.run_command_line("BPARAMETER FLIP 0,0 10,0 -2,2");
        assert_eq!(
            actions::parameters(&app.tabs[i].scene.document, def, Kind::Flip).len(),
            1
        );
        let _ = app.dispatch_command("BEDIT_SAVE");
        begin(&mut app, reference);
        assert_eq!(
            actions::parameters(&app.tabs[i].scene.document, def, Kind::Rotate).len(),
            1
        );
        assert_eq!(
            actions::parameters(&app.tabs[i].scene.document, def, Kind::Flip).len(),
            1
        );
    }
}

#[test]
fn block_actions_save_close_and_reedit_evaluated_reference() {
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
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
    let _ = app.dispatch_command("BCLOSE");
    assert!(app.tabs[i].active_block_edit.is_none());
    assert_eq!(
        actions::parameters(&app.tabs[i].scene.document, def, Kind::Move).len(),
        1
    );
    assert_eq!(actions::actions(&app.tabs[i].scene.document, def).len(), 1);
    app.tabs[i].scene.selected.insert(reference);
    app.refresh_properties();
    let grip = app.tabs[i]
        .selected_grips
        .iter()
        .find(|g| super::block_actions::is_test_grip(g.id))
        .unwrap()
        .id;
    assert!(app.start_test_action_grip(reference, grip));
    feed(&mut app, "12,0");
    let EntityType::Insert(insert) = app.tabs[i].scene.document.get_entity(reference).unwrap()
    else {
        panic!()
    };
    assert_ne!(insert.block_name, "EDIT_ME");
    // Native save/reload must preserve the link from the evaluated variant to
    // the definition that owns the parameters and actions.
    let bytes = codec::DwgWriter::write_to_vec(&app.tabs[i].scene.document).unwrap();
    app.tabs[i].scene.document = codec::DwgReader::from_stream(std::io::Cursor::new(bytes))
        .read()
        .unwrap();
    app.tabs[i].scene.selected.insert(reference);
    let _ = app.dispatch_command("BEDIT");
    assert_eq!(
        app.tabs[i].active_block_edit_session().unwrap().br_handle,
        def
    );
    assert_eq!(
        actions::parameters(&app.tabs[i].scene.document, def, Kind::Move).len(),
        1
    );
    assert_eq!(actions::actions(&app.tabs[i].scene.document, def).len(), 1);
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");
    app.tabs[i].scene.selected.insert(reference);
    app.refresh_properties();
    assert!(app.tabs[i]
        .selected_grips
        .iter()
        .any(|g| super::block_actions::is_test_grip(g.id)));
}

#[test]
fn block_actions_rotate_flip_mouse_grips_authoring_history_and_reopen() {
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let source = app.active_tab;
    let _ = app.run_command_line("BPARAMETER ROTATION 0,0 10 0");
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
            .find(|g| {
                super::block_actions::is_test_grip(g.id) && g.world.distance(flip_at) < 1e-5
            })
            .unwrap();
        assert_eq!(
            flip_grip.shape,
            crate::scene::model::object::GripShape::Triangle
        );
        assert!(flip_grip.dir.unwrap().distance(glam::DVec3::Y) < 1e-5);
        click(&mut app, flip_at);
        assert!(
            app.tabs[i].active_cmd.is_none(),
            "the flip chevron never asks for point input"
        );
        if test {
            check(&app, insert, -Vector3::UNIT_Y);
        } else {
            let flipped = 1 - app.flip_popup.as_ref().expect("flip state list").current;
            let _ = app.update(Message::FlipPick(flipped));
            check(&app, insert, -Vector3::UNIT_Y);
        }
        app.tabs[i].scene.selected.insert(insert);
        app.refresh_properties();
        let flip_grip = app.tabs[i]
            .selected_grips
            .iter()
            .find(|g| {
                super::block_actions::is_test_grip(g.id) && g.world.distance(flip_at) < 1e-5
            })
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
            let bytes = codec::DwgWriter::write_to_vec(&app.tabs[i].scene.document).unwrap();
            app.tabs[i].scene.document = codec::DwgReader::from_stream(std::io::Cursor::new(bytes))
                .read()
                .unwrap();
            app.tabs[i].scene.selected.insert(insert);
            app.refresh_properties();
            click(&mut app, flip_at);
            let flipped = 1 - app.flip_popup.as_ref().expect("flip state list").current;
            let _ = app.update(Message::FlipPick(flipped));
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
        .find(|g| super::block_actions::is_test_grip(g.id))
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
    let bytes = codec::DwgWriter::write_to_vec(&app.tabs[i].scene.document).unwrap();
    app.tabs[i].scene.document = codec::DwgReader::from_stream(std::io::Cursor::new(bytes))
        .read()
        .unwrap();
    app.refresh_properties();
    let grip = app.tabs[i]
        .selected_grips
        .iter()
        .find(|g| super::block_actions::is_test_grip(g.id))
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
    let mut attribute = codec::entities::AttributeEntity::new("TAG".into(), "value".into());
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
    use crate::scene::block_authoring::edit::{self, Part};
    use crate::scene::block_actions::{self as actions, Kind};
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
        .all(|g| !super::block_authoring_edit::is_editor_grip(g.id)));
    // Persisted native coordinates, rather than session-only display positions.
    use std::io::Cursor;
    let bytes = codec::DwgWriter::write_to_vec(&app.tabs[i].scene.document).unwrap();
    let loaded = codec::DwgReader::from_stream(Cursor::new(bytes))
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
    let parameter = actions::parameters(&app.tabs[i].scene.document, definition, Kind::Move)[0]
        .handle;
    app.tabs[i].selected_authoring = Some(parameter);
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800., 600.);

    assert!(!app.pick_authoring_object(
        iced::Point::new(-1000., -1000.),
        iced::Rectangle { x: 0., y: 0., width: 800., height: 600. },
    ));
    assert_eq!(app.tabs[i].selected_authoring, None);
}

#[test]
fn block_parameter_leader_and_crossing_box_select_then_delete() {
    use crate::scene::block_authoring::edit;
    use crate::scene::block_actions::{self as actions, Kind};
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
    let bounds = iced::Rectangle { x: 0., y: 0., width: 800., height: 600. };
    let project = |point: Vector3| {
        let p = app.tabs[i].scene.camera.borrow().project(
            glam::DVec3::new(point.x, point.y, point.z), bounds,
        ).unwrap();
        iced::Point::new(p.x, p.y)
    };
    let (pa, pb) = (project(a), project(b));
    let midpoint = iced::Point::new((pa.x + pb.x) * 0.5, (pa.y + pb.y) * 0.5);
    assert!(app.pick_authoring_object(midpoint, bounds));
    assert_eq!(app.tabs[i].selected_authoring, Some(handle));

    app.tabs[i].selected_authoring = None;
    let box_bounds = iced::Rectangle {
        x: midpoint.x - 1., y: midpoint.y - 1., width: 2., height: 2.,
    };
    assert!(app.pick_authoring_box(
        iced::Point::new(box_bounds.x, box_bounds.y),
        iced::Point::new(box_bounds.x + box_bounds.width, box_bounds.y + box_bounds.height),
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
        actions::actions(doc, def).iter().map(|a| a.0).collect::<Vec<_>>()
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
    use crate::scene::block_authoring::edit::{self, Part};
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 6,0 3,2");
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let parameter = actions::parameters(&app.tabs[i].scene.document, definition, Kind::Move)[0].handle;
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
    let grip_index = item.grips
        .iter()
        .position(|(part, _)| *part == Part::End)
        .unwrap();
    app.tabs[i].selected_authoring = Some(parameter);
    app.refresh_properties();
    let grip_id = app.tabs[i].selected_grips[grip_index].id;
    assert!(app.start_authoring_grip(parameter, grip_id));

    let bounds = iced::Rectangle { x: 0., y: 0., width: 800., height: 600. };
    let cursor = app.tabs[i].scene.camera.borrow().project(
        glam::DVec3::new(7., 2., 0.),
        bounds,
    ).unwrap();
    let _ = app.update(Message::ViewportMove(iced::Point::new(cursor.x, cursor.y)));

    assert!(!app.tabs[i].scene.preview_wires.is_empty());
    assert_eq!(actions::parameters(&app.tabs[i].scene.document, definition, Kind::Move)[0].end,
        glam::DVec3::new(6., 0., 0.));
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
    use crate::scene::block_authoring::edit;
    use crate::scene::block_actions::{self as actions, Kind};
    for kind in ["LINEAR", "XY", "POLAR"] {
        let (mut app, reference) = fixture();
        begin(&mut app, reference);
        let i = app.active_tab;
        let _ = app.run_command_line(&format!("BPARAMETER {kind} 0,0 6,2 3,2"));
        let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
        let parameter = actions::parameters(&app.tabs[i].scene.document, definition, Kind::Move)[0].handle;
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
        let screen = app.tabs[i].scene.camera.borrow().project(
            glam::DVec3::new(label.x, label.y, label.z),
            bounds,
        )
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
        assert_eq!(
            app.tabs[i]
                .properties
                .edit_buf
                .get(&crate::ui::properties::FieldKey::Geom("authoring_name"))
                .map(String::as_str),
            Some(crate::scene::block_authoring::metadata::read(&app.tabs[i].scene.document, definition, parameter).unwrap().text[0].value),
        );
    }
}

#[test]
fn linear_parameter_value_set_is_editable_and_undoable_in_properties() {
    use crate::scene::block_actions::{self as actions, Kind};
    use codec::objects::{DynamicBlockData, ObjectType};

    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 6,0 3,2");
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let parameter = actions::parameters(&app.tabs[i].scene.document, definition, Kind::Move)[0].handle;
    app.tabs[i].selected_authoring = Some(parameter);
    app.refresh_properties();

    let fields = app.tabs[i]
        .properties
        .sections
        .iter()
        .flat_map(|section| section.props.iter())
        .map(|property| property.field)
        .collect::<Vec<_>>();
    assert!(fields.contains(&"authoring_chain_actions_state"));
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
        let _ = app.update(Message::PropGeomInput { field, value: value.into() });
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

#[test]
fn block_actions_mouse_selection_waits_for_enter_for_move_and_scale() {
    use crate::scene::block_actions::{self as actions, Kind};
    for (kind, command) in [(Kind::Move, "BACTION MOVE"), (Kind::Scale, "BACTION SCALE")] {
        let (mut app, reference) = fixture();
        begin(&mut app, reference);
        let source = app.active_tab;
        let _ = app.run_command_line("CIRCLE 0.5,0.5 0.2");
        let _ = app.run_command_line("BPARAMETER LINEAR 0,0 2,0 1,-0.2");
        let definition = app.tabs[source]
            .active_block_edit_session()
            .unwrap()
            .br_handle;
        app.tabs[source].scene.selection.borrow_mut().view.vp_size = (1600., 900.);
        app.tabs[source]
            .scene
            .camera
            .borrow_mut()
            .fit_to_bounds_f64(
                glam::DVec3::new(-0.5, -0.5, 0.),
                glam::DVec3::new(2.5, 1.5, 0.),
                1600. / 900.,
            );
        let _ = app.dispatch_command(command);
        let click = |app: &mut OpenCADStudio, point: [f64; 3], modifiers: &str| {
            let scene = &app.tabs[app.active_tab].scene;
            let (vw, vh) = scene.selection.borrow().view.vp_size;
            let tile = scene.active_model_tile_bounds(vw, vh);
            let screen = scene
                .camera
                .borrow()
                .project(
                    glam::DVec3::from_array(point),
                    iced::Rectangle {
                        x: 0.,
                        y: 0.,
                        width: tile.width,
                        height: tile.height,
                    },
                )
                .unwrap();
            let cursor = iced::Point::new(screen.x + tile.x, screen.y + tile.y);
            let previous = (app.ctrl_down, app.shift_down);
            app.ctrl_down = modifiers.contains("ctrl");
            app.shift_down = modifiers.contains("shift");
            let _ = app.update(Message::ViewportLeftPressAt(cursor));
            let _ = app.update(Message::ViewportLeftRelease);
            (app.ctrl_down, app.shift_down) = previous;
        };
        click(&mut app, [2., 0., 0.], "");
        assert!(app.tabs[source]
            .active_cmd
            .as_ref()
            .unwrap()
            .is_selection_gathering());
        feed(&mut app, ""); // Empty selection does not cancel or advance.
        assert!(app.tabs[source]
            .active_cmd
            .as_ref()
            .unwrap()
            .is_selection_gathering());
        for (point, modifiers, count) in [
            ([0.5, 0., 0.], "", 1),
            ([0.5, 0.7, 0.], "", 2),
            ([0.5, 0., 0.], "shift", 1),
            ([0.5, 0., 0.], "", 2),
        ] {
            click(&mut app, point, modifiers);
            assert!(
                app.tabs[source]
                    .active_cmd
                    .as_ref()
                    .unwrap()
                    .is_selection_gathering(),
                "every mouse pick/removal stays in object selection"
            );
            assert_eq!(app.tabs[source].scene.selected.len(), count);
            assert!(actions::actions(&app.tabs[source].scene.document, definition).is_empty());
        }
        feed(&mut app, "");
        assert!(!app.tabs[source]
            .active_cmd
            .as_ref()
            .unwrap()
            .is_selection_gathering());
        click(&mut app, [1.5, 1., 0.], "");
        assert!(app.tabs[source].active_cmd.is_none());
        let authored = actions::actions(&app.tabs[source].scene.document, definition);
        assert_eq!(authored.len(), 1);
        assert_eq!(authored[0].2, kind);
        assert_eq!(authored[0].1.dependencies.len(), 2);
        let original = app.tabs[source].scene.document.clone();
        let _ = app.dispatch_command("BTESTBLOCK");
        let i = app.active_tab;
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (1600., 900.);
        app.tabs[i].scene.camera.borrow_mut().fit_to_bounds_f64(
            glam::DVec3::new(-0.5, -0.5, 0.),
            glam::DVec3::new(4.5, 1.5, 0.),
            1600. / 900.,
        );
        click(&mut app, [0.5, 0., 0.], "");
        click(&mut app, [2., 0., 0.], "");
        assert_eq!(
            app.tabs[i].active_cmd.as_ref().map(|c| c.name()),
            Some("BTESTBLOCK")
        );
        click(&mut app, [4., 0., 0.], "");
        assert!(app.tabs[i].active_cmd.is_none());
        let circle = app.tabs[i]
            .scene
            .document
            .entities_in_block("EDIT_ME")
            .find_map(|e| {
                if let EntityType::Circle(c) = e {
                    Some(c)
                } else {
                    None
                }
            })
            .unwrap();
        let (center, radius) = if kind == Kind::Move {
            (Vector3::new(2.5, 0.5, 0.), 0.2)
        } else {
            (Vector3::new(1., 1., 0.), 0.4)
        };
        // Mouse projection uses f32 screen coordinates; allow sub-pixel roundoff.
        assert!(
            (circle.center - center).length() < 1e-5,
            "{kind:?}: actual {:?}, expected {center:?}",
            circle.center
        );
        assert!((circle.radius - radius).abs() < 1e-5);
        assert_eq!(app.tabs[source].scene.document, original);
    }
}

#[test]
fn block_actions_command_selection_cancel_undo_and_test_block_isolation() {
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let source = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
    let definition = app.tabs[source]
        .active_block_edit_session()
        .unwrap()
        .br_handle;
    let p =
        actions::parameters(&app.tabs[source].scene.document, definition, Kind::Move)[0].clone();
    let targets = members(&app);
    let before = app.tabs[source].scene.document.clone();
    let depth = app.tabs[source].history.undo_stack.len();
    let _ = app.dispatch_command("BACTION MOVE");
    feed(&mut app, &p.name);
    assert!(app.tabs[source]
        .active_cmd
        .as_ref()
        .unwrap()
        .is_selection_gathering());
    let _ = app.feed_command(crate::command::StepInput::SelectionComplete(
        targets.clone(),
    ));
    let _ = app.update(Message::CommandEscape);
    assert_eq!(app.tabs[source].scene.document, before);
    assert_eq!(app.tabs[source].history.undo_stack.len(), depth);
    for kind in ["Move", "Scale"] {
        let _ = app.dispatch_command(&format!("BACTION {kind}"));
        feed(&mut app, &p.name);
        let _ = app.feed_command(crate::command::StepInput::SelectionComplete(
            targets.clone(),
        ));
        assert!(app.tabs[source]
            .active_cmd
            .as_ref()
            .unwrap()
            .is_selection_gathering());
        let _ = app.feed_command(crate::command::StepInput::Enter);
        feed(&mut app, "3,4");
        assert!(app.tabs[source].active_cmd.is_none());
    }
    assert_eq!(
        actions::actions(&app.tabs[source].scene.document, definition).len(),
        2
    );
    assert_eq!(app.tabs[source].history.undo_stack.len(), depth + 2);
    app.undo_steps(1);
    assert_eq!(
        actions::actions(&app.tabs[source].scene.document, definition).len(),
        1
    );
    app.redo_steps(1);
    assert_eq!(
        actions::actions(&app.tabs[source].scene.document, definition).len(),
        2
    );
    let original = app.tabs[source].scene.document.clone();
    let _ = app.dispatch_command("BTESTBLOCK");
    let test = app.active_tab;
    let insert = app.tabs[test].block_test.as_ref().unwrap().insert;
    app.tabs[test].selected_handle = Some(insert);
    app.refresh_test_action_grips([0.; 3]);
    let grip = app.tabs[test].selected_grips.last().unwrap().id;
    assert!(app.start_test_action_grip(insert, grip));
    feed(&mut app, "20,5");
    let EntityType::Line(line) = app.tabs[test]
        .scene
        .document
        .get_entity(targets[0])
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(line.start, Vector3::new(20., 0., 0.));
    assert_eq!(line.end, Vector3::new(22., 0., 0.));
    assert_eq!(app.tabs[source].scene.document, original);
    app.undo_steps(1);
    let EntityType::Line(line) = app.tabs[test]
        .scene
        .document
        .get_entity(targets[0])
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(line.start, Vector3::ZERO);
    app.redo_steps(1);
    let EntityType::Line(line) = app.tabs[test]
        .scene
        .document
        .get_entity(targets[0])
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(line.start, Vector3::new(20., 0., 0.));
    app.tabs[test]
        .scene
        .document
        .add_entity(EntityType::Insert(Insert::new(
            "EDIT_ME",
            Vector3::new(50., 0., 0.),
        )))
        .unwrap();
    let shared_before = app.tabs[test].scene.document.clone();
    let id = app.tabs[test].id;
    let _ = app.handle_block_action_result(crate::command::CmdResult::EvaluateBlockParameter { grip: None,
        insert: None,
        document: id,
        definition,
        parameter: p.handle,
        value: crate::scene::block_actions::Value::Point(glam::DVec3::new(30., 0., 0.)),
    });
    assert_eq!(
        app.tabs[test].scene.document, shared_before,
        "shared references are refused rather than silently modified together"
    );
    let _ = app.dispatch_command("BCLOSETESTBLOCK");
    assert_eq!(app.active_tab, source);
    assert_eq!(app.tabs[source].scene.document, original);
}

#[test]
fn stretch_pair_sets_guide_both_frames_with_undo_and_cancel() {
    use crate::command::StepInput;
    use crate::scene::block_actions::{self as actions, Kind};
    for label in [
        "Linear Stretch Pair",
        "Polar Stretch Pair",
        "Linear Move Pair",
    ] {
        let kind = if label.contains("Stretch") {
            Kind::Stretch
        } else {
            Kind::Move
        };
        let (mut app, reference) = fixture();
        begin(&mut app, reference);
        let i = app.active_tab;
        let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
        let targets = members(&app);
        let _ = app.dispatch_command(&format!("BPARAMETERSET {label}"));
        for point in [
            glam::DVec3::ZERO,
            glam::DVec3::new(10., 0., 0.),
            glam::DVec3::new(5., 2., 0.),
        ] {
            let _ = app.feed_command(StepInput::Point(point));
        }
        for (side, expected) in [
            (0, ["BaseXDelta", "BaseYDelta"]),
            (1, ["EndXDelta", "EndYDelta"]),
        ] {
            if kind == Kind::Stretch {
                assert!(app.tabs[i]
                    .active_cmd
                    .as_ref()
                    .unwrap()
                    .prompt()
                    .contains(if side == 0 { "base" } else { "endpoint" }));
                for point in [
                    glam::DVec3::new(side as f64 - 0.25, -1., 0.),
                    glam::DVec3::new(side as f64 + 0.25, 1., 0.),
                ] {
                    let _ = app.feed_command(StepInput::Point(point));
                }
            }
            assert!(app.tabs[i]
                .active_cmd
                .as_ref()
                .unwrap()
                .is_selection_gathering());
            let _ = app.feed_command(StepInput::SelectionComplete(targets.clone()));
            let _ = app.feed_command(StepInput::Enter);
            let _ = app.feed_command(StepInput::Point(glam::DVec3::new(3., 4., 0.)));
            let authored = actions::actions(&app.tabs[i].scene.document, definition);
            assert_eq!(authored.len(), side + 1);
            assert_eq!(authored[side].2, kind);
            let codec::objects::ObjectType::DynamicBlock(object) =
                &app.tabs[i].scene.document.objects[&authored[side].0]
            else {
                panic!()
            };
            let connections = match &object.data {
                codec::objects::DynamicBlockData::StretchAction(a) => &a.connections,
                codec::objects::DynamicBlockData::MoveAction(a) => &a.connections,
                _ => panic!(),
            };
            assert_eq!(
                [connections[0].name.as_str(), connections[1].name.as_str()],
                expected
            );
        }
        assert!(app.tabs[i].pending_action_pair.is_none());
        assert!(app.tabs[i].active_cmd.is_none());
        app.undo_steps(1);
        assert_eq!(
            actions::actions(&app.tabs[i].scene.document, definition).len(),
            1
        );
        app.redo_steps(1);
        assert_eq!(
            actions::actions(&app.tabs[i].scene.document, definition).len(),
            2
        );

        let _ = app.dispatch_command(&format!("BPARAMETERSET {label}"));
        for point in [
            glam::DVec3::ZERO,
            glam::DVec3::new(10., 0., 0.),
            glam::DVec3::new(5., 2., 0.),
        ] {
            let _ = app.feed_command(StepInput::Point(point));
        }
        let before = app.tabs[i].scene.document.clone();
        let _ = app.update(Message::CommandEscape);
        assert!(app.tabs[i].pending_action_pair.is_none());
        assert_eq!(app.tabs[i].scene.document, before);

        let _ = app.dispatch_command(&format!("BPARAMETERSET {label}"));
        for point in [
            glam::DVec3::ZERO,
            glam::DVec3::new(10., 0., 0.),
            glam::DVec3::new(5., 2., 0.),
        ] {
            let _ = app.feed_command(StepInput::Point(point));
        }
        assert!(app.tabs[i].pending_action_pair.is_some());
        let _ = app.dispatch_command("LINE");
        assert!(app.tabs[i].pending_action_pair.is_none());
        assert!(app.tabs[i].pending_parameter_set_follow_up.is_none());
        let _ = app.update(Message::CommandEscape);
    }
}

#[test]
fn xy_box_set_guides_four_separate_action_selections() {
    use crate::scene::block_actions::{self as actions, Kind};
    use crate::scene::block_distance_parameter as distance;
    for kind in [Kind::Move, Kind::Stretch] {
        let (mut app, reference) = fixture();
        begin(&mut app, reference);
        let i = app.active_tab;
        let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
        let parameter = distance::create(
            &mut app.tabs[i].scene.document,
            definition,
            distance::Kind::XY,
            Vector3::ZERO,
            Vector3::new(10., 10., 0.),
            Vector3::new(5., 5., 0.),
        )
        .unwrap();
        app.tabs[i].pending_parameter_set_follow_up = Some(if kind == Kind::Move {
            "XY_MOVE_BOX_SET"
        } else {
            "XY_STRETCH_BOX_SET"
        });
        let _ = app.finish_parameter_set_placement(i, parameter);
        let targets = members(&app);
        for (key_point, expected_names) in [
            ("base", ["BaseXDelta", "BaseYDelta"]),
            ("X-axis", ["EndXDelta", "BaseYDelta"]),
            ("Y-axis", ["BaseXDelta", "EndYDelta"]),
            ("end", ["EndXDelta", "EndYDelta"]),
        ] {
            if kind == Kind::Stretch {
                assert!(app.tabs[i]
                    .active_cmd
                    .as_ref()
                    .unwrap()
                    .prompt()
                    .contains(key_point));
                for point in [
                    glam::DVec3::new(-0.25, -1., 0.),
                    glam::DVec3::new(0.25, 1., 0.),
                ] {
                    let _ = app.feed_command(crate::command::StepInput::Point(point));
                }
            }
            assert!(app.tabs[i]
                .active_cmd
                .as_ref()
                .unwrap()
                .prompt()
                .contains(key_point));
            let _ = app.feed_command(crate::command::StepInput::SelectionComplete(
                targets.clone(),
            ));
            let _ = app.feed_command(crate::command::StepInput::Enter);
            let _ = app.feed_command(crate::command::StepInput::Point(glam::DVec3::ZERO));
            let authored = actions::actions(&app.tabs[i].scene.document, definition);
            let action = authored.last().unwrap();
            let codec::objects::ObjectType::DynamicBlock(object) =
                &app.tabs[i].scene.document.objects[&action.0]
            else {
                panic!()
            };
            let connections = match &object.data {
                codec::objects::DynamicBlockData::MoveAction(a) => &a.connections,
                codec::objects::DynamicBlockData::StretchAction(a) => &a.connections,
                _ => panic!(),
            };
            assert_eq!(
                [connections[0].name.as_str(), connections[1].name.as_str(),],
                expected_names
            );
        }
        assert_eq!(
            actions::actions(&app.tabs[i].scene.document, definition).len(),
            4
        );
        assert!(app.tabs[i].active_cmd.is_none());
        assert_eq!(
            actions::parameters(&app.tabs[i].scene.document, definition, Kind::Move).len(),
            1
        );
        assert!(app.tabs[i].pending_xy_box_set.is_none());
    }
}

#[test]
fn block_reset_selection_cancel_and_undo_preserve_reference_isolation() {
    use crate::command::StepInput;
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
    let parameter =
        actions::parameters(&app.tabs[i].scene.document, definition, Kind::Move)[0].handle;
    let targets = members(&app);
    actions::create(
        &mut app.tabs[i].scene.document,
        definition,
        Kind::Move,
        parameter,
        &targets,
        glam::DVec3::ZERO,
    )
    .unwrap();
    assert!(app.save_block_edit(i));
    app.close_block_edit(i, false);
    app.tabs[i].scene.document = actions::instance::evaluate(
        &app.tabs[i].scene.document,
        reference,
        parameter,
        glam::DVec3::new(20., 0., 0.),
    )
    .unwrap()
    .unwrap();
    app.tabs[i].scene.deselect_all();
    let changed = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let _ = app.dispatch_command("RESETBLOCK");
    let _ = app.feed_command(StepInput::SelectionComplete(vec![reference]));
    let _ = app.update(Message::CommandEscape);
    assert_eq!(app.tabs[i].scene.document, changed);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    let _ = app.dispatch_command("RESETBLOCK");
    let _ = app.feed_command(StepInput::SelectionComplete(vec![reference]));
    let _ = app.feed_command(StepInput::Enter);
    let reset = app.tabs[i].scene.document.clone();
    assert_ne!(reset, changed);
    let p = actions::parameters(&reset, definition, Kind::Move)[0].clone();
    assert_eq!(
        actions::instance::current(&reset, reference, &p).unwrap(),
        p.end
    );
    assert_eq!(reset.get_entity(targets[0]), changed.get_entity(targets[0]));
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    app.undo_steps(1);
    assert_eq!(app.tabs[i].scene.document, changed);
    app.redo_steps(1);
    assert_eq!(app.tabs[i].scene.document, reset);
    app.tabs[i].scene.select_entity(reference, true);
    let _ = app.dispatch_command("RESETBLOCK");
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
}

#[test]
fn block_reset_test_window_restores_its_baseline_and_keeps_the_editor_unchanged() {
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let source = app.active_tab;
    let definition = app.tabs[source]
        .active_block_edit_session()
        .unwrap()
        .br_handle;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
    let parameter =
        actions::parameters(&app.tabs[source].scene.document, definition, Kind::Move)[0].handle;
    let targets = members(&app);
    actions::create(
        &mut app.tabs[source].scene.document,
        definition,
        Kind::Move,
        parameter,
        &targets,
        glam::DVec3::ZERO,
    )
    .unwrap();
    let editor = app.tabs[source].scene.document.clone();
    let _ = app.dispatch_command("BTESTBLOCK");
    let i = app.active_tab;
    let insert = app.tabs[i].block_test.as_ref().unwrap().insert;
    let _ = app.handle_block_action_result(crate::command::CmdResult::EvaluateBlockParameter { grip: None,
        insert: None,
        document: app.tabs[i].id,
        definition,
        parameter,
        value: actions::Value::Point(glam::DVec3::new(20., 0., 0.)),
    });
    let changed = app.tabs[i].scene.document.clone();
    assert_ne!(
        changed.get_entity(targets[0]),
        editor.get_entity(targets[0])
    );
    app.tabs[i].scene.select_entity(insert, true);
    let _ = app.dispatch_command("RESETBLOCK");
    assert_eq!(
        app.tabs[i].scene.document.get_entity(targets[0]),
        editor.get_entity(targets[0])
    );
    assert_eq!(app.tabs[source].scene.document, editor);
    let reset = app.tabs[i].scene.document.clone();
    app.undo_steps(1);
    assert_eq!(app.tabs[i].scene.document, changed);
    app.redo_steps(1);
    assert_eq!(app.tabs[i].scene.document, reset);
}

#[test]
fn block_reset_and_table_context_guards_preserve_running_commands() {
    let (mut app, reference) = fixture();
    let i = app.active_tab;
    let _ = app.dispatch_command("LINE");
    let _ = app.dispatch_command("BTABLE");
    assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(), "LINE");
    let _ = app.update(Message::CommandEscape);
    begin(&mut app, reference);
    let _ = app.dispatch_command("LINE");
    let _ = app.dispatch_command("RESETBLOCK");
    assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(), "LINE");
    let _ = app.update(Message::CommandEscape);
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
    let _ = app.dispatch_command("BTABLE");
    assert_eq!(app.active_modal, Some(ModalKind::BlockPropertiesTable));
    let _ = app.update(Message::CloseModal);
    let _ = app.update(Message::NamedParametersOpen);
    assert_eq!(app.active_modal, Some(ModalKind::NamedParameters));
    let _ = app.dispatch_command("PARAMETERSCLOSE");
    assert_eq!(app.active_modal, None);
}

#[test]
fn block_actions_rejected_outside_editor_preserve_running_command_and_stale_results_are_inert() {
    use crate::scene::block_actions::Kind;
    let (mut app, reference) = fixture();
    let i = app.active_tab;
    let _ = app.dispatch_command("LINE");
    let before = app.tabs[i].scene.document.clone();
    let _ = app.dispatch_command("BACTION MOVE");
    assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(), "LINE");
    assert_eq!(app.tabs[i].scene.document, before);
    let _ = app.update(Message::CommandEscape);
    begin(&mut app, reference);
    let _ = app.run_command_line("BPARAMETER POINT 2,3 4,5");
    let def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let p = crate::scene::block_actions::parameters(&app.tabs[i].scene.document, def, Kind::Move)
        [0]
    .handle;
    let before = app.tabs[i].scene.document.clone();
    let _ = app.handle_block_action_result(crate::command::CmdResult::PlaceBlockAction {
        document: app.tabs[i].id + 1,
        definition: def,
        kind: Kind::Move,
        parameter: p,
        xy_key_point: None,
        targets: members(&app),
        location: glam::DVec3::ZERO,
        frame: None,
        array_offsets: None,
        displacement_overrides: None,
        rotate_only: vec![],
    });
    assert_eq!(app.tabs[i].scene.document, before);
    let _ = app.dispatch_command("LINE");
    let _ = app.dispatch_command("BACTION SCALE");
    assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(), "LINE");
}

#[test]
fn block_actions_test_reference_transform_visibility_and_noop_history() {
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let source = app.active_tab;
    let _ = app.run_command_line("BPARAMETER POINT 2,3 4,5");
    let _ = app.run_command_line("BPARAMETER VISIBILITY 8,8");
    let _ = app.run_command_line("BPARAMETER BASEPOINT 1,2");
    let definition = app.tabs[source]
        .active_block_edit_session()
        .unwrap()
        .br_handle;
    let p =
        actions::parameters(&app.tabs[source].scene.document, definition, Kind::Move)[0].clone();
    let targets = members(&app);
    let _ = app.dispatch_command("BACTION MOVE");
    feed(&mut app, &p.name);
    let _ = app.feed_command(crate::command::StepInput::SelectionComplete(targets));
    let _ = app.feed_command(crate::command::StepInput::Enter);
    feed(&mut app, "4,6");
    let _ = app.dispatch_command("BTESTBLOCK");
    let i = app.active_tab;
    let insert = app.tabs[i].block_test.as_ref().unwrap().insert;
    let depth = app.tabs[i].history.undo_stack.len();
    let id = app.tabs[i].id;
    let EntityType::Insert(ins) = app.tabs[i].scene.document.get_entity(insert).unwrap() else {
        panic!()
    };
    let current_world = actions::v(
        crate::scene::render_graph::insert_transform(&app.tabs[i].scene.document, ins)
            .apply(actions::native(p.end)),
    );
    let _ = app.handle_block_action_result(crate::command::CmdResult::EvaluateBlockParameter { grip: None,
        insert: None,
        document: id,
        definition,
        parameter: p.handle,
        value: crate::scene::block_actions::Value::Point(current_world),
    });
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    // Existing visibility UI creates a per-reference anonymous definition.
    app.apply_visibility_state(insert, 0);
    app.finish_pending_history(i);
    let EntityType::Insert(ins) = app.tabs[i].scene.document.get_entity(insert).unwrap() else {
        panic!()
    };
    let after_world = actions::v(
        crate::scene::render_graph::insert_transform(&app.tabs[i].scene.document, ins)
            .apply(actions::native(p.end)),
    );
    assert_eq!(
        after_world, current_world,
        "visibility representation retains the native Basepoint anchor"
    );
    let EntityType::Insert(ins) = app.tabs[i].scene.document.get_entity_mut(insert).unwrap() else {
        panic!()
    };
    ins.insert_point = Vector3::new(100., 200., 0.);
    ins.rotation = std::f64::consts::FRAC_PI_2;
    ins.set_x_scale(2.);
    ins.set_y_scale(3.);
    let EntityType::Insert(ins) = app.tabs[i].scene.document.get_entity(insert).unwrap() else {
        panic!()
    };
    let name = ins.block_name.clone();
    assert_ne!(name, "EDIT_ME");
    let world = actions::v(
        crate::scene::render_graph::insert_transform(&app.tabs[i].scene.document, ins)
            .apply(Vector3::new(12., 13., 0.)),
    );
    let before = app.tabs[i].scene.document.clone();
    let _ = app.handle_block_action_result(crate::command::CmdResult::EvaluateBlockParameter { grip: None,
        insert: None,
        document: id,
        definition,
        parameter: p.handle,
        value: crate::scene::block_actions::Value::Point(world),
    });
    let anon = app.tabs[i]
        .scene
        .document
        .entities_in_block(&name)
        .find_map(|e| {
            if let EntityType::Line(l) = e {
                Some(l)
            } else {
                None
            }
        })
        .unwrap();
    assert!((anon.start.x - 10.).abs() < 1e-9 && (anon.start.y - 10.).abs() < 1e-9);
    app.undo_steps(1);
    assert_eq!(app.tabs[i].scene.document, before);
}

fn authored_points(app: &OpenCADStudio) -> Vec<codec::objects::BlockPointParameter> {
    let tab = &app.tabs[app.active_tab];
    let definition = tab
        .scene
        .document
        .block_records
        .get("EDIT_ME")
        .unwrap()
        .handle;
    crate::scene::block_point_parameter::parameters(&tab.scene.document, definition)
        .into_iter()
        .map(|(_, parameter)| parameter.clone())
        .collect()
}

fn distance_parameter_count(app: &OpenCADStudio) -> usize {
    let tab = &app.tabs[app.active_tab];
    let definition = tab
        .scene
        .document
        .block_records
        .get("EDIT_ME")
        .unwrap()
        .handle;
    crate::scene::block_distance_parameter::parameters(&tab.scene.document, definition).len()
}

fn orientation_parameter_count(app: &OpenCADStudio) -> usize {
    let tab = &app.tabs[app.active_tab];
    let definition = tab
        .scene
        .document
        .block_records
        .get("EDIT_ME")
        .unwrap()
        .handle;
    crate::scene::block_orientation_parameter::parameters(&tab.scene.document, definition).len()
}

#[test]
fn lookup_basepoint_placement_history_test_block_and_saved_parent_undo() {
    use crate::scene::{block_basepoint, block_lookup, render_graph};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let members_before = members(&app);
    let _ = app.run_command_line("BPARAMETER BASEPOINT 3,4");
    let (handle, p) = block_basepoint::parameter(&app.tabs[i].scene.document, definition).unwrap();
    assert_eq!(p.point, Vector3::new(3.0, 4.0, 0.0));
    let depth = app.tabs[i].history.undo_stack.len();
    assert_eq!(depth, 1);
    let _ = app.run_command_line("BPARAMETER BASEPOINT 3,4");
    assert_eq!(
        app.tabs[i].history.undo_stack.len(),
        depth,
        "same anchor is a no-op"
    );
    let _ = app.run_command_line("BPARAMETER BASEPOINT 5,6");
    assert_eq!(
        block_basepoint::parameter(&app.tabs[i].scene.document, definition)
            .unwrap()
            .0,
        handle
    );
    app.undo_steps(1);
    assert_eq!(
        block_basepoint::parameter(&app.tabs[i].scene.document, definition)
            .unwrap()
            .1
            .point,
        Vector3::new(3.0, 4.0, 0.0)
    );
    app.redo_steps(1);
    let _ = app.dispatch_command("BPARAMETER");
    feed(&mut app, "K");
    feed(&mut app, "2,2");
    assert_eq!(
        block_lookup::parameters(&app.tabs[i].scene.document, definition).len(),
        1
    );
    assert_eq!(
        members(&app),
        members_before,
        "authoring objects are not drawing entities"
    );
    let before_test = app.tabs[i].scene.document.clone();
    let _ = app.dispatch_command("BTESTBLOCK");
    let doc = &app.tabs[app.active_tab].scene.document;
    let insert = doc
        .entities()
        .find_map(|e| match e {
            EntityType::Insert(i) => Some(i),
            _ => None,
        })
        .unwrap();
    assert!(
        (render_graph::insert_transform(doc, insert).apply(Vector3::new(5.0, 6.0, 0.0))
            - insert.insert_point)
            .length()
            < 1e-9
    );
    let _ = app.dispatch_command("BCLOSETESTBLOCK");
    assert_eq!(app.tabs[i].scene.document, before_test);
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BEDIT_DISCARD");
    assert!(block_basepoint::parameter(&app.tabs[i].scene.document, definition).is_some());
    app.undo_steps(1);
    assert!(block_basepoint::parameter(&app.tabs[i].scene.document, definition).is_none());
    app.redo_steps(1);
    assert!(block_basepoint::parameter(&app.tabs[i].scene.document, definition).is_some());
}

fn lookup_editor_fixture() -> (OpenCADStudio, Handle, Handle) {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let _ = app.dispatch_command("BACTIONBARMODE 0");
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 2,0 0,1");
    let _ = app.run_command_line("BPARAMETER LOOKUP 1,1");
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let parameter =
        crate::scene::block_lookup::parameters(&app.tabs[i].scene.document, definition)[0].0;
    (app, definition, parameter)
}
fn edit_lookup(app: &mut OpenCADStudio) {
    use crate::ui::window::block_lookup::Action as A;
    let _ = app.update(Message::BlockLookup(A::Property(0)));
    let _ = app.update(Message::BlockLookup(A::AddProperty));
    let _ = app.update(Message::BlockLookup(A::AddRow));
    let _ = app.update(Message::BlockLookup(A::Cell(0, 1, "Small".into())));
}
#[test]
fn lookup_editor_cancel_apply_one_undo_and_stale_guard() {
    use crate::ui::window::block_lookup::Action as A;
    use crate::scene::block_lookup;
    let (mut app, definition, parameter) = lookup_editor_fixture();
    let i = app.active_tab;
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    for cancel in [Message::CloseModal, Message::CommandEscape] {
        let _ = app.dispatch_command("BLOOKUPTABLE");
        edit_lookup(&mut app);
        assert_eq!(app.tabs[i].scene.document, before, "draft is isolated");
        let _ = app.update(cancel);
        assert!(app.block_lookup.is_none());
        assert_eq!(app.tabs[i].scene.document, before);
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    }
    let _ = app.dispatch_command("BLOOKUPTABLE");
    let _ = app.update(Message::BlockLookup(A::Apply));
    assert_eq!(
        app.tabs[i].history.undo_stack.len(),
        depth,
        "unchanged OK is a no-op"
    );
    let _ = app.dispatch_command("BLOOKUPTABLE");
    edit_lookup(&mut app);
    let _ = app.update(Message::BlockLookup(A::Audit));
    assert!(app.block_lookup.as_ref().unwrap().error.is_none());
    let _ = app.update(Message::BlockLookup(A::Apply));
    assert!(app.block_lookup.is_none());
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    assert_eq!(
        block_lookup::table(&app.tabs[i].scene.document, definition, parameter)
            .unwrap()
            .unwrap()
            .1
            .expressions,
        vec!["2", "Small"]
    );
    let saved = app.tabs[i].scene.document.clone();
    app.undo_steps(1);
    assert_eq!(app.tabs[i].scene.document, before);
    app.redo_steps(1);
    assert_eq!(app.tabs[i].scene.document, saved);
    let _ = app.dispatch_command("BLOOKUPTABLE Lookup1");
    let _ = app.update(Message::BlockLookup(A::Cell(0, 1, "Changed".into())));
    app.tabs[i].scene.bump_geometry();
    let _ = app.update(Message::BlockLookup(A::Apply));
    assert!(app
        .block_lookup
        .as_ref()
        .unwrap()
        .error
        .as_ref()
        .unwrap()
        .contains("changed"));
    assert_eq!(app.tabs[i].scene.document, saved);
    let _ = app.update(Message::CloseModal);
}

#[test]
fn lookup_editor_multiple_tables_apply_atomically_and_parameter_cancel_is_clean() {
    use crate::ui::window::block_lookup::Action as A;
    let (mut app, definition, _) = lookup_editor_fixture();
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LOOKUP 3,3");
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    for kind in ["BASEPOINT", "LOOKUP"] {
        let _ = app.dispatch_command(&format!("BPARAMETER {kind}"));
        let _ = app.update(Message::CommandEscape);
        assert_eq!(app.tabs[i].scene.document, before);
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    }
    let _ = app.dispatch_command("BLOOKUPTABLE");
    edit_lookup(&mut app);
    let _ = app.update(Message::BlockLookup(A::Select(1)));
    edit_lookup(&mut app);
    let _ = app.update(Message::BlockLookup(A::Cell(0, 0, "NaN".into())));
    let _ = app.update(Message::BlockLookup(A::Apply));
    assert!(app.block_lookup.as_ref().unwrap().error.is_some());
    assert_eq!(app.tabs[i].scene.document, before);
    let _ = app.update(Message::BlockLookup(A::Cell(0, 0, "5".into())));
    let _ = app.update(Message::BlockLookup(A::Apply));
    assert!(app.block_lookup.is_none());
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    for (h, _) in crate::scene::block_lookup::parameters(&app.tabs[i].scene.document, definition) {
        assert!(
            crate::scene::block_lookup::table(&app.tabs[i].scene.document, definition, h)
                .unwrap()
                .is_some()
        );
    }
    app.undo_steps(1);
    assert_eq!(app.tabs[i].scene.document, before);
    let _ = app.dispatch_command("BEDIT_DISCARD");
    let _ = app.dispatch_command("LINE");
    for cmd in ["BPARAMETER BASEPOINT", "BPARAMETER LOOKUP", "BLOOKUPTABLE"] {
        let _ = app.dispatch_command(cmd);
        assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(), "LINE");
    }
}

#[test]
fn orientation_parameter_batch_numeric_rotation_local_history_save_and_parent_undo() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    for (index, (kind, inputs)) in [
        ("ROTATION", vec!["0,0", "2", "90"]),
        ("ALIGNMENT", vec!["0,0", "0,2"]),
        ("FLIP", vec!["0,0", "0,2", "3,3"]),
    ]
    .into_iter()
    .enumerate()
    {
        let before = app.tabs[i].scene.document.clone();
        let _ = app.dispatch_command(&format!("BPARAMETER {kind}"));
        for text in &inputs[..inputs.len() - 1] {
            feed(&mut app, text);
            assert_eq!(app.tabs[i].scene.document, before, "incomplete placement");
        }
        feed(&mut app, inputs.last().unwrap());
        assert_eq!(orientation_parameter_count(&app), index + 1, "{kind}");
        assert_eq!(app.tabs[i].history.undo_stack.len(), index + 1);
        assert!(app.tabs[i].scene.preview_wires.is_empty());
        app.undo_steps(1);
        assert_eq!(orientation_parameter_count(&app), index);
        app.redo_steps(1);
        assert_eq!(orientation_parameter_count(&app), index + 1);
    }
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let p = &crate::scene::block_orientation_parameter::parameters(
        &app.tabs[i].scene.document,
        definition,
    )[0];
    assert!((p.native.definition_end_point - Vector3::new(0.0, 2.0, 0.0)).length() < 1e-10);
    assert_eq!(p.reference, Vector3::new(2.0, 0.0, 0.0));
    let editor = app.tabs[i].scene.document.clone();
    let _ = app.dispatch_command("BTESTBLOCK");
    assert!(app.tabs[app.active_tab]
        .active_block_edit_session()
        .is_none());
    assert_eq!(
        app.tabs[app.active_tab]
            .scene
            .document
            .entities_in_block("EDIT_ME")
            .count(),
        1
    );
    let _ = app.dispatch_command("BCLOSETESTBLOCK");
    assert_eq!(app.tabs[i].scene.document, editor);
    let _ = app.run_command_line("UCS ORIGIN 1,2,0");
    for p in crate::scene::block_orientation_parameter::parameters(
        &app.tabs[i].scene.document,
        definition,
    ) {
        assert_eq!(
            p.native.definition_base_point,
            Vector3::new(-1.0, -2.0, 0.0)
        );
    }
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BEDIT_DISCARD");
    assert_eq!(orientation_parameter_count(&app), 3);
    app.undo_steps(1);
    assert_eq!(orientation_parameter_count(&app), 0);
    app.redo_steps(1);
    assert_eq!(orientation_parameter_count(&app), 3);
}

#[test]
fn orientation_parameter_chooser_cancels_invalid_picks_and_rotation_default_angle() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let before = app.tabs[i].scene.document.clone();
    for (kind, maximum) in [("R", 2), ("A", 1), ("F", 2)] {
        for picks in 0..=maximum {
            let _ = app.dispatch_command("BPARAMETER");
            feed(&mut app, kind);
            if picks > 0 {
                feed(&mut app, "0,0");
                feed(&mut app, "0,0");
                assert_eq!(app.tabs[i].scene.document, before);
            }
            if picks > 1 {
                feed(&mut app, "0,2");
            }
            let _ = app.update(Message::CommandEscape);
            assert_eq!(app.tabs[i].scene.document, before);
            assert!(app.tabs[i].history.undo_stack.is_empty());
        }
    }
    let _ = app.dispatch_command("BPARAMETER ROTATION");
    feed(&mut app, "0,0");
    feed(&mut app, "-2");
    assert_eq!(app.tabs[i].scene.document, before);
    feed(&mut app, "2");
    feed(&mut app, "");
    assert_eq!(orientation_parameter_count(&app), 1);
    let _ = app.dispatch_command("BEDIT_DISCARD");
    assert_eq!(orientation_parameter_count(&app), 0);
    let _ = app.dispatch_command("LINE");
    for kind in ["ROTATION", "ALIGNMENT", "FLIP"] {
        let _ = app.dispatch_command(&format!("BPARAMETER {kind}"));
        assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(), "LINE");
    }
}

#[test]
fn orientation_parameter_stale_context_and_invalid_result_do_not_start_history() {
    use crate::scene::block_orientation_parameter::Kind;
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let id = app.tabs[i].id;
    let before = app.tabs[i].scene.document.clone();
    for (document_id, end) in [
        (id + 1, glam::DVec3::Y),
        (id, glam::DVec3::ZERO),
        (id, glam::DVec3::splat(f64::NAN)),
    ] {
        let _ = app.handle_place_block_orientation_parameter(
            crate::command::CmdResult::PlaceBlockOrientationParameter {
                document_id,
                definition,
                kind: Kind::Flip,
                perpendicular: false,
                base: glam::DVec3::ZERO,
                end,
                reference: glam::DVec3::X,
            },
        );
        assert_eq!(app.tabs[i].scene.document, before);
        assert!(app.tabs[i].history.undo_stack.is_empty());
    }
}

#[test]
fn distance_parameter_batch_three_picks_local_undo_redo_and_saved_parent_undo() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    for (index, kind) in ["LINEAR", "POLAR", "XY"].iter().enumerate() {
        let before = app.tabs[i].scene.document.clone();
        let _ = app.dispatch_command(&format!("BPARAMETER {kind}"));
        feed(&mut app, "2,3");
        feed(&mut app, "6,7");
        assert_eq!(
            app.tabs[i].scene.document, before,
            "base and endpoint are preview only"
        );
        feed(&mut app, "1,8");
        assert_eq!(distance_parameter_count(&app), index + 1);
        assert_eq!(app.tabs[i].history.undo_stack.len(), index + 1);
        assert!(app.tabs[i].scene.preview_wires.is_empty());
        app.undo_steps(1);
        assert_eq!(distance_parameter_count(&app), index);
        app.redo_steps(1);
        assert_eq!(distance_parameter_count(&app), index + 1);
    }
    let _ = app.run_command_line("BPARAMETER POINT 0,0 1,1");
    let _ = app.run_command_line("BPARAMETER VISIBILITY 0,0");
    let editor = app.tabs[i].scene.document.clone();
    let _ = app.dispatch_command("BTESTBLOCK");
    assert!(app.tabs[app.active_tab]
        .active_block_edit_session()
        .is_none());
    assert_eq!(
        app.tabs[app.active_tab]
            .scene
            .document
            .entities_in_block("EDIT_ME")
            .count(),
        1,
        "parameters and their editor graphics add no drawing geometry"
    );
    assert_eq!(distance_parameter_count(&app), 3);
    let _ = app.dispatch_command("BCLOSETESTBLOCK");
    assert_eq!(app.active_tab, i);
    assert_eq!(app.tabs[i].scene.document, editor);
    let _ = app.run_command_line("UCS ORIGIN 1,2,0");
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    for p in
        crate::scene::block_distance_parameter::parameters(&app.tabs[i].scene.document, definition)
    {
        assert_eq!(p.native.definition_base_point, Vector3::new(1.0, 1.0, 0.0));
        assert_eq!(p.native.definition_end_point, Vector3::new(5.0, 5.0, 0.0));
    }
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BEDIT_DISCARD");
    assert_eq!(distance_parameter_count(&app), 3);
    app.undo_steps(1);
    assert_eq!(distance_parameter_count(&app), 0);
    app.redo_steps(1);
    assert_eq!(distance_parameter_count(&app), 3);
}

#[test]
fn distance_parameter_cancel_chooser_invalid_endpoint_and_discard_are_safe() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let before = app.tabs[i].scene.document.clone();
    for kind in ["Linear", "Polar", "XY"] {
        for picks in 0..3 {
            let _ = app.dispatch_command("BPARAMETER");
            feed(&mut app, kind);
            if picks > 0 {
                feed(&mut app, "2,3");
            }
            if picks > 1 {
                feed(&mut app, "6,7");
            }
            let _ = app.update(Message::CommandEscape);
            assert_eq!(app.tabs[i].scene.document, before);
            assert!(app.tabs[i].history.undo_stack.is_empty());
        }
        let _ = app.dispatch_command(&format!("BPARAMETER {kind}"));
        feed(&mut app, "2,3");
        feed(&mut app, "2,3");
        assert_eq!(app.tabs[i].scene.document, before);
        assert!(
            app.tabs[i].active_cmd.is_some(),
            "invalid endpoint can be retried"
        );
        feed(&mut app, "6,7");
        feed(&mut app, "1,8");
        assert_eq!(distance_parameter_count(&app), 1);
        app.undo_steps(1);
    }
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 1,1 2,2");
    let _ = app.dispatch_command("BEDIT_DISCARD");
    assert_eq!(distance_parameter_count(&app), 0);
    let _ = app.dispatch_command("LINE");
    for kind in ["LINEAR", "POLAR", "XY"] {
        let _ = app.dispatch_command(&format!("BPARAMETER {kind}"));
        assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(), "LINE");
    }
}

#[test]
fn distance_parameter_stale_context_and_invalid_result_do_not_start_history() {
    use crate::scene::block_distance_parameter::Kind;
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let id = app.tabs[i].id;
    let before = app.tabs[i].scene.document.clone();
    for (document_id, end) in [
        (id + 1, glam::DVec3::ONE),
        (id, glam::DVec3::ZERO),
        (id, glam::DVec3::splat(f64::NAN)),
    ] {
        let _ = app.handle_place_block_distance_parameter(
            crate::command::CmdResult::PlaceBlockDistanceParameter {
                document_id,
                definition,
                kind: Kind::Linear,
                base: glam::DVec3::ZERO,
                end,
                label: glam::DVec3::ZERO,
            },
        );
        assert_eq!(app.tabs[i].scene.document, before);
        assert!(app.tabs[i].history.undo_stack.is_empty());
    }
}

#[test]
fn point_parameter_placement_two_picks_one_undo_and_repeated_names() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let before = app.tabs[i].scene.document.clone();
    let _ = app.dispatch_command("BPARAMETER POINT");
    feed(&mut app, "3,4,0");
    assert_eq!(
        app.tabs[i].scene.document, before,
        "first pick is preview only"
    );
    feed(&mut app, "6,7,0");
    assert_eq!(authored_points(&app)[0].position_name, "Position1");
    assert_eq!(
        authored_points(&app)[0].definition_label_point,
        Vector3::new(6.0, 7.0, 0.0)
    );
    assert_eq!(app.tabs[i].history.undo_stack.len(), 1);
    assert!(app.block_edit_dirty(i));
    app.undo_steps(1);
    assert!(authored_points(&app).is_empty());
    app.redo_steps(1);
    assert_eq!(authored_points(&app).len(), 1);
    let _ = app.run_command_line("BPARAMETER POINT 8,9 10,11");
    assert_eq!(authored_points(&app)[1].position_name, "Position2");
    assert_eq!(app.tabs[i].history.undo_stack.len(), 2);
}

#[test]
fn point_parameter_placement_cancel_at_both_prompts_is_inert() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let before = app.tabs[i].scene.document.clone();
    for picked in [false, true] {
        let _ = app.dispatch_command("BPARAMETER POINT");
        if picked {
            feed(&mut app, "3,4");
        }
        let _ = app.update(Message::CommandEscape);
        assert_eq!(app.tabs[i].scene.document, before);
        assert!(app.tabs[i].history.undo_stack.is_empty());
        assert!(!app.block_edit_dirty(i));
    }
    let _ = app.dispatch_command("BPARAMETER POINT");
    feed(&mut app, "3,4");
    feed(&mut app, "");
    assert_eq!(app.tabs[i].scene.document, before);
    assert!(app.tabs[i].history.undo_stack.is_empty());
}

#[test]
fn point_parameter_placement_chooser_works_with_existing_visibility_and_reframes() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER VISIBILITY 0,0");
    let _ = app.dispatch_command("BPARAMETER");
    feed(&mut app, "Point");
    feed(&mut app, "3,4,5");
    feed(&mut app, "6,7,5");
    assert_eq!(authored_points(&app).len(), 1);
    assert!(app.block_visibility_context().is_some());
    let _ = app.run_command_line("UCS ORIGIN 1,2,3");
    assert_eq!(
        authored_points(&app)[0].parameter.definition_point,
        Vector3::new(2.0, 2.0, 2.0)
    );
    assert_eq!(
        authored_points(&app)[0].definition_label_point,
        Vector3::new(5.0, 5.0, 2.0)
    );
    app.undo_steps(1);
    assert_eq!(
        authored_points(&app)[0].parameter.definition_point,
        Vector3::new(3.0, 4.0, 5.0)
    );
    let _ = app.dispatch_command("BSAVE");
    assert!(!app.block_edit_dirty(i));
    let _ = app.dispatch_command("BEDIT_DISCARD");
    assert_eq!(authored_points(&app).len(), 1);
    app.undo_steps(1);
    assert!(
        authored_points(&app).is_empty(),
        "saved block undo restores the parent drawing"
    );
}

#[test]
fn point_parameter_placement_discard_and_stale_context_leave_no_objects() {
    let (mut app, reference) = fixture();
    let i = app.active_tab;
    let before = app.tabs[i].scene.document.clone();
    begin(&mut app, reference);
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let start = app.tabs[i].scene.document.clone();
    let id = app.tabs[i].id;
    for (document, location) in [
        (id + 1, glam::DVec3::ZERO),
        (id, glam::DVec3::splat(f64::NAN)),
    ] {
        let _ = app.handle_place_block_point_parameter(
            crate::command::CmdResult::PlaceBlockPointParameter {
                document_id: document,
                definition,
                location,
                label: glam::DVec3::ZERO,
            },
        );
        assert_eq!(app.tabs[i].scene.document, start);
    }
    let _ = app.run_command_line("BPARAMETER POINT 3,4 6,7");
    let _ = app.dispatch_command("BEDIT_DISCARD");
    let mut after = app.tabs[i].scene.document.clone();
    after.vports = before.vports.clone();
    assert_eq!(after, before);
    let closed = app.tabs[i].scene.document.clone();
    let _ = app.dispatch_command("LINE");
    let _ = app.dispatch_command("BPARAMETER POINT");
    assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(), "LINE");
    assert_eq!(app.tabs[i].scene.document, closed);
}

fn visibility_editor() -> (OpenCADStudio, crate::ui::window::block_visibility::Target) {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let _ = app.run_command_line("BPARAMETER VISIBILITY 0,0");
    let target = app
        .block_visibility_context()
        .expect("placed parameter")
        .target;
    (app, target)
}

fn visibility_action(app: &mut OpenCADStudio, action: crate::ui::window::block_visibility::Action) {
    let _ = app.update(Message::BlockVisibility(action));
}

#[test]
fn visibility_ribbon_manager_cancel_validation_and_single_undo() {
    use crate::ui::window::block_visibility::Action as A;
    let (mut app, target) = visibility_editor();
    let i = app.active_tab;
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    visibility_action(&mut app, A::Open(target));
    visibility_action(&mut app, A::Name("Alternate".into()));
    visibility_action(&mut app, A::New);
    assert_eq!(app.tabs[i].scene.document, before, "dialog is staged");
    let _ = app.update(Message::CommandEscape);
    assert!(app.block_visibility.is_none());
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    assert_eq!(app.tabs[i].scene.document, before);
    visibility_action(&mut app, A::Open(target));
    visibility_action(&mut app, A::Name("visibilitystate0".into()));
    visibility_action(&mut app, A::New);
    assert!(app.block_visibility.as_ref().unwrap().error.is_some());
    visibility_action(&mut app, A::Delete);
    assert_eq!(
        app.block_visibility.as_ref().unwrap().edited.states.len(),
        1
    );
    visibility_action(&mut app, A::Name("Alternate".into()));
    visibility_action(&mut app, A::New);
    visibility_action(&mut app, A::SetCurrent);
    visibility_action(&mut app, A::Name("Alternate renamed".into()));
    visibility_action(&mut app, A::Rename);
    visibility_action(&mut app, A::Move(true));
    visibility_action(&mut app, A::Apply);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    assert_eq!(
        app.block_visibility_context().unwrap().current.as_deref(),
        Some("Alternate renamed")
    );
    app.undo_steps(1);
    assert_eq!(
        app.block_visibility_context().unwrap().names,
        ["VisibilityState0"]
    );
    app.redo_steps(1);
    assert_eq!(
        app.block_visibility_context().unwrap().names,
        ["Alternate renamed", "VisibilityState0"]
    );
    assert_eq!(
        app.block_visibility_context().unwrap().current.as_deref(),
        Some("Alternate renamed")
    );
}

#[test]
fn visibility_ribbon_new_modes_state_selection_and_native_roundtrip() {
    use crate::ui::window::block_visibility::{Action as A, NewVisibility as V};
    let (mut app, target) = visibility_editor();
    let i = app.active_tab;
    let member = members(&app)[0];
    visibility_action(&mut app, A::Open(target));
    for (name, mode) in [("Hidden", V::Hide), ("Shown", V::Show), ("Copy", V::Keep)] {
        visibility_action(&mut app, A::Name(name.into()));
        visibility_action(&mut app, A::NewVisibility(mode));
        visibility_action(&mut app, A::New);
    }
    visibility_action(&mut app, A::Apply);
    let parameter =
        crate::scene::block_visibility::parameter(&app.tabs[i].scene.document, target.definition)
            .unwrap();
    assert!(parameter.states[0].visible_blocks.contains(&member));
    assert!(parameter.states[1].visible_blocks.is_empty());
    assert!(parameter.states[2].visible_blocks.contains(&member));
    assert!(parameter.states[3].visible_blocks.contains(&member));
    visibility_action(&mut app, A::Current(target, "Hidden".into()));
    assert!(
        app.tabs[i]
            .scene
            .document
            .get_entity(member)
            .unwrap()
            .common()
            .invisible
    );
    visibility_action(&mut app, A::Current(target, "Shown".into()));
    visibility_action(&mut app, A::Current(target, "Copy".into()));
    app.undo_steps(1);
    assert_eq!(
        app.block_visibility_context().unwrap().current.as_deref(),
        Some("Shown"),
        "identical states retain their identity in history"
    );
    app.redo_steps(1);
    assert_eq!(
        app.block_visibility_context().unwrap().current.as_deref(),
        Some("Copy")
    );
    app.save_block_edit(i);
    for ext in ["dwg", "dxf"] {
        let document = &app.tabs[i].scene.document;
        let bytes = crate::io::save_to_bytes(document, ext, document.version).unwrap();
        let loaded = crate::io::load_bytes(&format!("visibility.{ext}"), bytes).unwrap();
        let parameter =
            crate::scene::block_visibility::parameter(&loaded, target.definition).unwrap();
        assert_eq!(
            parameter
                .states
                .iter()
                .map(|state| state.name.as_str())
                .collect::<Vec<_>>(),
            ["VisibilityState0", "Hidden", "Shown", "Copy"]
        );
        assert!(parameter.states[1].visible_blocks.is_empty());
        assert!(parameter.states[2].visible_blocks.contains(&member));
    }
}

#[test]
fn visibility_ribbon_mode_and_make_visible_cancel_do_not_change_saved_flags() {
    use crate::ui::window::block_visibility::Action as A;
    let (mut app, target) = visibility_editor();
    let i = app.active_tab;
    let member = members(&app)[0];
    app.tabs[i].scene.select_entity(member, true);
    let _ = app.run_command_line("BVHIDE");
    app.finish_pending_history(i);
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let dirty = app.tabs[i].dirty;
    let wires = |app: &OpenCADStudio| {
        app.tabs[i]
            .scene
            .entity_wires()
            .iter()
            .filter(|wire| crate::scene::Scene::handle_from_wire_name(&wire.name) == Some(member))
            .cloned()
            .collect::<Vec<_>>()
    };
    assert!(wires(&app).is_empty());
    visibility_action(&mut app, A::Mode(target));
    assert!(
        !wires(&app).is_empty(),
        "hidden member is rendered for picking"
    );
    assert_eq!(app.tabs[i].scene.document, before);
    visibility_action(&mut app, A::Mode(target));
    assert!(wires(&app).is_empty());
    let _ = app.run_command_line("BVSHOW");
    app.sync_block_visibility_display();
    assert!(!wires(&app).is_empty());
    let _ = app.update(Message::CommandEscape);
    assert!(wires(&app).is_empty());
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    assert_eq!(app.tabs[i].dirty, dirty);
    let _ = app.run_command_line("BVMODE 1");
    assert!(app.block_visibility_mode);
    let _ = app.run_command_line("SETVAR BVMODE 2");
    assert!(app.block_visibility_mode);
    let _ = app.run_command_line("BVMODE 0");
    assert!(!app.block_visibility_mode);
    assert!(
        app.pending_setvar.is_none(),
        "inline variable must close its probe prompt"
    );
    let _ = app.run_command_line("BVSHOW");
    app.tabs[i].scene.select_entity(member, true);
    let result = app.tabs[i]
        .active_cmd
        .as_mut()
        .unwrap()
        .on_selection_complete(vec![member]);
    let _ = app.apply_cmd_result(result);
    feed(&mut app, "");
    assert!(
        !app.tabs[i]
            .scene
            .document
            .get_entity(member)
            .unwrap()
            .common()
            .invisible
    );
    app.undo_steps(1);
    assert!(
        app.tabs[i]
            .scene
            .document
            .get_entity(member)
            .unwrap()
            .common()
            .invisible,
        "undo uses the hidden flags from before picking"
    );
}

#[test]
fn visibility_ribbon_manager_rejects_stale_target_and_stale_document() {
    use crate::ui::window::block_visibility::Action as A;
    let (mut app, target) = visibility_editor();
    visibility_action(&mut app, A::Open(target));
    visibility_action(&mut app, A::Name("Alternate".into()));
    visibility_action(&mut app, A::New);
    app.tabs[app.active_tab].scene.bump_geometry();
    visibility_action(&mut app, A::Apply);
    assert!(app.block_visibility.as_ref().unwrap().error.is_some());
    assert_eq!(
        app.block_visibility_context().unwrap().names,
        ["VisibilityState0"]
    );
    let _ = app.update(Message::CloseModal);
    let mut stale = target;
    stale.document += 1;
    visibility_action(&mut app, A::Open(stale));
    assert!(app.block_visibility.is_none());
    assert!(app.active_modal.is_none());
    let _ = app.run_command_line("BVSTATE DIALOG");
    assert_eq!(app.active_modal, Some(ModalKind::BlockVisibility));
}

#[test]
fn local_undo_preserves_imported_native_networks_when_constraints_are_unchanged() {
    use codec::objects::{
        AssocNetwork, AssociativeData, AssociativeObject, Dictionary, ObjectType,
    };
    let (mut app, reference) = fixture();
    let i = app.active_tab;
    let doc = &mut app.tabs[i].scene.document;
    let owner = doc.block_records.get("EDIT_ME").unwrap().handle;
    let dictionary_handle = doc.allocate_handle();
    let network_handle = doc.allocate_handle();
    let mut dictionary = Dictionary::new();
    dictionary.handle = dictionary_handle;
    dictionary.owner = owner;
    dictionary.hard_owner = true;
    dictionary.add_entry("ACAD_ASSOCNETWORK", network_handle);
    doc.objects
        .insert(dictionary_handle, ObjectType::Dictionary(dictionary));
    let network = AssociativeObject {
        handle: network_handle,
        owner: dictionary_handle,
        reactors: vec![dictionary_handle],
        dxf_name: "ASSOCNETWORK".into(),
        cpp_class_name: "AcDbAssocNetwork".into(),
        data: AssociativeData::Network(AssocNetwork::default()),
        ..Default::default()
    };
    doc.objects
        .insert(network_handle, ObjectType::Associative(network));
    begin(&mut app, reference);
    let original_objects = app.tabs[i].scene.document.objects.clone();
    circle(&mut app, 1.0);
    assert_eq!(app.tabs[i].scene.document.objects, original_objects);
    app.undo_steps(1);
    assert_eq!(members(&app).len(), 1);
    assert_eq!(app.tabs[i].scene.document.objects, original_objects);
    app.redo_steps(1);
    assert_eq!(members(&app).len(), 2);
    assert_eq!(app.tabs[i].scene.document.objects, original_objects);
    assert!(app.save_block_edit_as(i, "COPY"));
    for (handle, object) in original_objects {
        let mut actual = app.tabs[i].scene.document.objects[&handle].clone();
        if handle == app.tabs[i].scene.document.header.named_objects_dict_handle {
            // Copying the first scope installs the drawing's root network.
            let ObjectType::Dictionary(dictionary) = &mut actual else { panic!() };
            dictionary.entries.retain(|(key, _)| key != "ACAD_ASSOCNETWORK");
        }
        assert_eq!(actual, object);
    }
    assert_eq!(members(&app).len(), 1);
    assert_eq!(app.tabs[i].scene.document.block_records.get("COPY").unwrap().entity_handles.len(), 2);
}

#[test]
fn save_as_forks_unsaved_edits_and_undoes_as_one_drawing_step() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    circle(&mut app, 1.0);
    app.save_block_edit(app.active_tab);
    let saved = members(&app);
    circle(&mut app, 2.0);
    let edited = members(&app).len();
    assert!(app.save_block_edit_as(app.active_tab, "  COPY  "));
    let i = app.active_tab;
    assert_eq!(members(&app), saved);
    assert_eq!(
        app.tabs[i].active_block_edit_session().unwrap().block_name,
        "COPY"
    );
    assert_eq!(
        app.tabs[i]
            .scene
            .document
            .block_records
            .get("COPY")
            .unwrap()
            .entity_handles
            .len(),
        edited
    );
    assert_eq!(main_depth(&app), 2);
    assert!(app.tabs[i].history.undo_stack.is_empty());
    assert!(!app.block_edit_dirty(i));
    app.undo_steps(1);
    assert!(app.tabs[i]
        .scene
        .document
        .block_records
        .get("COPY")
        .is_some());
    circle(&mut app, 3.0);
    app.close_block_edit(i, true);
    assert_eq!(
        app.tabs[i]
            .scene
            .document
            .block_records
            .get("COPY")
            .unwrap()
            .entity_handles
            .len(),
        edited
    );
    app.undo_steps(1);
    assert!(app.tabs[i]
        .scene
        .document
        .block_records
        .get("COPY")
        .is_none());
    assert_eq!(members(&app), saved);
    app.redo_steps(1);
    assert_eq!(
        app.tabs[i]
            .scene
            .document
            .block_records
            .get("COPY")
            .unwrap()
            .entity_handles
            .len(),
        edited
    );
}

#[test]
fn save_as_name_refusals_do_not_change_the_session() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    circle(&mut app, 1.0);
    let i = app.active_tab;
    let document = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    for name in ["", "bad/name", "edit_me", "EDIT_ME"] {
        assert!(!app.save_block_edit_as(i, name));
        assert_eq!(app.tabs[i].scene.document, document);
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
        assert_eq!(main_depth(&app), 0);
        assert_eq!(
            app.tabs[i].active_block_edit_session().unwrap().block_name,
            "EDIT_ME"
        );
    }
    assert_eq!(
        app.command_line.last_error.as_deref(),
        Some("block \"EDIT_ME\" already exists")
    );
}

#[test]
fn save_as_dialog_cancel_and_enter_follow_the_current_session() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    circle(&mut app, 1.0);
    let i = app.active_tab;
    let document = app.tabs[i].scene.document.clone();
    let _ = app.update(Message::DropdownSelectItem {
        dropdown_id: "BLOCK_SAVE",
        cmd: "BSAVEAS",
    });
    assert_eq!(app.active_modal, Some(ModalKind::BlockSaveAs));
    let _ = app.update(Message::BlockSaveAsName("edit_me".into()));
    assert!(app.block_save_as.as_ref().unwrap().error.is_some());
    let _ = app.update(Message::CommandEscape);
    assert!(app.block_save_as.is_none());
    assert_eq!(app.tabs[i].scene.document, document);
    let _ = app.dispatch_command("BSAVEAS");
    let _ = app.update(Message::BlockSaveAsName("COPY".into()));
    assert!(app.block_save_as.as_ref().unwrap().error.is_none());
    let _ = app.update(Message::CommandFinalize);
    assert!(app.active_modal.is_none());
    assert_eq!(
        app.tabs[i].active_block_edit_session().unwrap().block_name,
        "COPY"
    );
}

#[test]
fn save_as_preserves_base_point_description_units_and_nested_attributes() {
    use codec::entities::AttributeEntity;
    let (mut app, reference) = fixture();
    let i = app.active_tab;
    app.tabs[i]
        .scene
        .define_block_raw("CHILD", Vector3::ZERO, vec![]);
    let owner = app.tabs[i]
        .scene
        .document
        .block_records
        .get("EDIT_ME")
        .unwrap()
        .handle;
    let mut nested = Insert::new("CHILD", Vector3::new(4.0, 5.0, 0.0));
    let mut attribute = AttributeEntity::new("TAG".into(), "value".into());
    attribute.common.handle = app.tabs[i].scene.document.allocate_handle();
    nested.attributes.push(attribute);
    nested.common.owner_handle = owner;
    nested.seqend_handle = Some(app.tabs[i].scene.document.allocate_handle());
    let nested = app.tabs[i]
        .scene
        .document
        .add_entity(EntityType::Insert(nested))
        .unwrap();
    let base = Vector3::new(12.0, 34.0, 0.0);
    let record = app.tabs[i]
        .scene
        .document
        .block_records
        .get_mut("EDIT_ME")
        .unwrap();
    record.base_point = base;
    record.description = "A reusable definition".into();
    record.units = 4;
    let source_marker = record.block_entity_handle;
    if let Some(EntityType::Block(marker)) =
        app.tabs[i].scene.document.get_entity_mut(source_marker)
    {
        marker.base_point = base;
    }
    let source_insert = app.tabs[i]
        .scene
        .document
        .get_entity(nested)
        .unwrap()
        .clone();
    begin(&mut app, reference);
    assert!(app.save_block_edit_as(i, "COPY"));
    let doc = &app.tabs[i].scene.document;
    let record = doc.block_records.get("COPY").unwrap();
    assert_eq!(record.base_point, base);
    assert_eq!(record.units, 4);
    assert_eq!(record.description, "A reusable definition");
    assert_eq!(doc.get_entity(nested).unwrap(), &source_insert);
    let copied = doc
        .entities_in_block("COPY")
        .find_map(|e| match e {
            EntityType::Insert(insert) => Some(insert),
            _ => None,
        })
        .unwrap();
    let EntityType::Insert(original) = &source_insert else {
        unreachable!()
    };
    assert_eq!(copied.block_name, "CHILD");
    assert_eq!(copied.insert_point, original.insert_point);
    assert_ne!(
        copied.attributes[0].common.handle,
        original.attributes[0].common.handle
    );
    assert_eq!(
        copied.attributes[0].common.owner_handle,
        copied.common.handle
    );
    assert_ne!(copied.seqend_handle, original.seqend_handle);
    assert_eq!(copied.attributes[0].value, original.attributes[0].value);
}

#[test]
fn save_as_clones_extension_dictionaries_and_draw_order_independently() {
    use codec::objects::{
        Dictionary, ObjectType, SortEntitiesTable, XRecord, XRecordEntry, XRecordValue,
    };
    let (mut app, reference) = fixture();
    let i = app.active_tab;
    let owner = app.tabs[i]
        .scene
        .document
        .block_records
        .get("EDIT_ME")
        .unwrap()
        .handle;
    let first = members(&app)[0];
    let mut second = Line::from_points(Vector3::ZERO, Vector3::UNIT_Y);
    second.common.owner_handle = owner;
    let second = app.tabs[i]
        .scene
        .document
        .add_entity(EntityType::Line(second))
        .unwrap();
    let root = app.tabs[i]
        .scene
        .document
        .ensure_extension_dictionary(owner);
    let mut record = XRecord::new();
    record.handle = app.tabs[i].scene.document.allocate_handle();
    record.owner = root;
    record
        .entries
        .push(XRecordEntry::new(340, XRecordValue::Handle(first)));
    let leaf = record.handle;
    app.tabs[i]
        .scene
        .document
        .objects
        .insert(leaf, ObjectType::XRecord(record));
    let mut order = SortEntitiesTable::for_block(owner);
    order.handle = app.tabs[i].scene.document.allocate_handle();
    order.owner_handle = root;
    order.add_entry(first, Handle::new(0x200));
    order.send_to_back(second);
    let order_handle = order.handle;
    app.tabs[i]
        .scene
        .document
        .objects
        .insert(order_handle, ObjectType::SortEntitiesTable(order));
    let Some(ObjectType::Dictionary(dictionary)) =
        app.tabs[i].scene.document.objects.get_mut(&root)
    else {
        unreachable!()
    };
    dictionary.add_entry("DATA", leaf);
    dictionary.add_entry("ACAD_SORTENTS", order_handle);
    let original: Dictionary = dictionary.clone();
    begin(&mut app, reference);
    assert!(app.save_block_edit_as(i, "COPY"));
    let doc = &app.tabs[i].scene.document;
    let owner = doc.block_records.get("COPY").unwrap().handle;
    let copy_members = doc
        .block_records
        .get("COPY")
        .unwrap()
        .entity_handles
        .clone();
    let copied_root = doc.extension_dictionary_handle(owner).unwrap();
    assert_ne!(root, copied_root);
    assert_eq!(
        doc.objects.get(&root),
        Some(&ObjectType::Dictionary(original))
    );
    let Some(ObjectType::Dictionary(dictionary)) = doc.objects.get(&copied_root) else {
        unreachable!()
    };
    let Some(ObjectType::XRecord(record)) = doc.objects.get(&dictionary.get("DATA").unwrap())
    else {
        unreachable!()
    };
    assert_eq!(record.owner, copied_root);
    assert_eq!(
        record.entries[0].value,
        XRecordValue::Handle(copy_members[0])
    );
    let Some(ObjectType::SortEntitiesTable(order)) =
        doc.objects.get(&dictionary.get("ACAD_SORTENTS").unwrap())
    else {
        unreachable!()
    };
    assert_eq!(order.block_owner_handle, owner);
    assert!(
        order.effective_sort_handle(copy_members[1]) < order.effective_sort_handle(copy_members[0])
    );
    app.close_block_edit(i, true);
    app.undo_steps(1);
    assert!(!app.tabs[i]
        .scene
        .document
        .objects
        .contains_key(&copied_root));
    app.redo_steps(1);
    assert!(app.tabs[i]
        .scene
        .document
        .objects
        .contains_key(&copied_root));
}

#[test]
fn save_as_preserves_block_parameters_and_remaps_constraint_geometry() {
    use crate::scene::parametric_constraints::{ConstraintKind, ParametricRef, ParametricScope};
    let (mut app, reference) = fixture();
    let i = app.active_tab;
    let owner = app.tabs[i]
        .scene
        .document
        .block_records
        .get("EDIT_ME")
        .unwrap()
        .handle;
    let member = members(&app)[0];
    let set = app.tabs[i]
        .scene
        .parametric_constraint_set_mut(ParametricScope::Block(owner));
    set.local_parameters.set("width", "7").unwrap();
    set.add(
        ConstraintKind::Horizontal,
        vec![ParametricRef::whole(member)],
        None,
    );
    begin(&mut app, reference);
    app.tabs[i]
        .scene
        .parametric_constraint_set_mut(ParametricScope::Block(owner))
        .local_parameters
        .set("width", "9")
        .unwrap();
    assert!(app.save_block_edit_as(i, "COPY"));
    let new_owner = app.tabs[i]
        .scene
        .document
        .block_records
        .get("COPY")
        .unwrap()
        .handle;
    let copied = app.tabs[i]
        .scene
        .parametric_constraint_set(ParametricScope::Block(new_owner))
        .unwrap();
    assert_eq!(copied.local_parameters.resolve("width").unwrap(), 9.0);
    assert_ne!(copied.constraints[0].refs[0].entity, member);
    assert_eq!(
        app.tabs[i]
            .scene
            .parametric_constraint_set(ParametricScope::Block(owner))
            .unwrap()
            .local_parameters
            .resolve("width")
            .unwrap(),
        7.0
    );
    app.close_block_edit(i, true);
    app.undo_steps(1);
    assert!(app.tabs[i]
        .scene
        .parametric_constraint_set(ParametricScope::Block(new_owner))
        .is_none());
    app.redo_steps(1);
    assert_eq!(
        app.tabs[i]
            .scene
            .parametric_constraint_set(ParametricScope::Block(new_owner))
            .unwrap()
            .local_parameters
            .resolve("width")
            .unwrap(),
        9.0
    );
}

#[test]
fn save_as_copies_native_authoring_graphs_and_keeps_reference_values_independent() {
    use crate::scene::{
        block_actions as actions, block_lookup as lookup, block_properties_table as tables,
    };
    use codec::objects::{DynamicBlockData, ObjectType};
    use std::io::Cursor;
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let original = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let targets = members(&app);
    for command in [
        "BPARAMETER LINEAR 0,0 10,0 5,2",
        "BPARAMETER POLAR 0,0 10,0 5,2",
        "BPARAMETER ROTATION 0,0 10,0 10,0",
        "BPARAMETER FLIP 0,0 10,0 -2,2",
    ] {
        let _ = app.run_command_line(command);
    }
    let doc = &mut app.tabs[i].scene.document;
    let lookup_parameter = lookup::create(doc, original, Vector3::new(4., 4., 0.)).unwrap();
    let parameter_data = lookup::parameters(doc, original)
        .into_iter()
        .find(|(h, _)| *h == lookup_parameter)
        .unwrap()
        .1;
    let mut table = lookup::empty_table(&parameter_data);
    table
        .columns
        .insert(0, lookup::properties(doc, original)[0].column.clone());
    table.column_count = 2;
    table.row_count = 1;
    table.expressions = vec!["10".into(), "Standard".into()];
    lookup::apply_table(doc, original, lookup_parameter, table).unwrap();
    let columns = tables::columns(doc, original)
        .into_iter()
        .filter(|c| c.connection_name == "UpdatedDistance")
        .take(1)
        .collect::<Vec<_>>();
    tables::apply(doc, original, &columns, &[vec![10.], vec![15.]], false).unwrap();
    let visibility =
        crate::scene::block_visibility::create(doc, original, Vector3::new(3., 3., 0.)).unwrap();
    for kind in [
        actions::Kind::Move,
        actions::Kind::Scale,
        actions::Kind::Stretch,
        actions::Kind::PolarStretch,
        actions::Kind::Rotate,
        actions::Kind::Flip,
        actions::Kind::Array,
    ] {
        let parameter = actions::parameters(doc, original, kind)[0].handle;
        actions::create_with_options(
            doc,
            original,
            kind,
            parameter,
            None,
            &targets,
            glam::DVec3::ZERO,
            actions::needs_frame(kind).then_some([
                glam::DVec3::new(0.5, -1., 0.),
                glam::DVec3::new(1.5, 1., 0.),
            ]),
            (kind == actions::Kind::Array).then_some((2., 2.)),
        )
        .unwrap();
    }
    let authored = app.tabs[i].scene.document.clone();
    assert!(
        app.save_block_edit_as(i, "COPY"),
        "{:?}",
        app.command_line.last_error
    );
    let copied = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let doc = &app.tabs[i].scene.document;
    assert!(
        actions::all_parameters(doc, original).is_empty(),
        "unsaved source authoring remains at its checkpoint"
    );
    assert_eq!(actions::actions(doc, copied).len(), 7);
    assert_eq!(lookup::parameters(doc, copied).len(), 1);
    assert!(tables::table(doc, copied).unwrap().is_some());
    let copied_visibility = crate::scene::block_visibility::parameter(doc, copied).unwrap();
    assert_ne!(copied_visibility.handle, visibility);
    assert!(copied_visibility
        .all_blocks
        .iter()
        .all(|h| !targets.contains(h)));
    let graph = tables::graph_handle(doc, copied).unwrap();
    let original_graph = tables::graph_handle(&authored, original).unwrap();
    assert_ne!(graph, original_graph);
    let original_guid = authored
        .object_xdata(original)
        .unwrap()
        .get_record("AcDbDynamicBlockGUID")
        .unwrap();
    let copied_guid = doc
        .object_xdata(copied)
        .unwrap()
        .get_record("AcDbDynamicBlockGUID")
        .unwrap();
    assert_ne!(original_guid, copied_guid);
    for (handle, action, _, _) in actions::actions(doc, copied) {
        assert!(action.dependencies.iter().all(|h| !targets.contains(h)));
        let ObjectType::DynamicBlock(object) = &doc.objects[&handle] else {
            panic!()
        };
        assert_eq!(object.owner, graph);
    }
    for saved in [
        doc.clone(),
        codec::DwgReader::from_stream(Cursor::new(codec::DwgWriter::write_to_vec(doc).unwrap()))
            .read()
            .unwrap(),
        codec::DxfReader::from_reader(Cursor::new(
            codec::DxfWriter::new(doc).write_to_vec().unwrap(),
        ))
        .unwrap()
        .read()
        .unwrap(),
    ] {
        assert_eq!(actions::actions(&saved, copied).len(), 7);
        assert!(tables::table(&saved, copied).unwrap().is_some());
        assert!(crate::scene::block_visibility::parameter(&saved, copied).is_some());
        let ObjectType::DynamicBlock(object) = &saved.objects[&graph] else {
            panic!()
        };
        let DynamicBlockData::EvaluationGraph(graph) = &object.data else {
            panic!()
        };
        assert!(graph
            .nodes
            .iter()
            .all(|node| saved.objects.contains_key(&node.expression)));
    }
    app.close_block_edit(i, false);
    let copy_reference = app.tabs[i]
        .scene
        .document
        .add_entity(EntityType::Insert(Insert::new(
            "COPY",
            Vector3::new(20., 0., 0.),
        )))
        .unwrap();
    let original_reference = app.tabs[i]
        .scene
        .document
        .get_entity(reference)
        .unwrap()
        .clone();
    let parameter =
        actions::parameters(&app.tabs[i].scene.document, copied, actions::Kind::Rotate)[0].handle;
    let changed = actions::instance::evaluate(
        &app.tabs[i].scene.document,
        copy_reference,
        parameter,
        glam::DVec3::new(0., 10., 0.),
    )
    .unwrap()
    .unwrap();
    assert_eq!(changed.get_entity(reference), Some(&original_reference));
    assert_eq!(
        changed.get_entity(targets[0]),
        app.tabs[i].scene.document.get_entity(targets[0])
    );
}

#[test]
fn save_as_existing_dynamic_definition_retains_both_graphs_and_undoes_atomically() {
    use crate::scene::{block_actions as actions, block_distance_parameter as distance};
    let (mut app, reference) = fixture();
    let i = app.active_tab;
    let original = app.tabs[i]
        .scene
        .document
        .block_records
        .get("EDIT_ME")
        .unwrap()
        .handle;
    let targets = members(&app);
    let doc = &mut app.tabs[i].scene.document;
    let parameter = distance::create(
        doc,
        original,
        distance::Kind::Linear,
        Vector3::ZERO,
        Vector3::new(10., 0., 0.),
        Vector3::new(5., 2., 0.),
    )
    .unwrap();
    actions::create(
        doc,
        original,
        actions::Kind::Move,
        parameter,
        &targets,
        glam::DVec3::ZERO,
    )
    .unwrap();
    let before = app.tabs[i].scene.document.clone();
    begin(&mut app, reference);
    assert!(app.save_block_edit_as(i, "COPY"));
    let copied = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    app.close_block_edit(i, false);
    assert_eq!(
        actions::actions(&app.tabs[i].scene.document, original),
        actions::actions(&before, original)
    );
    app.undo_steps(1);
    assert!(app.tabs[i]
        .scene
        .document
        .block_records
        .get("COPY")
        .is_none());
    assert_eq!(
        actions::actions(&app.tabs[i].scene.document, original),
        actions::actions(&before, original)
    );
    app.redo_steps(1);
    assert_eq!(
        actions::actions(&app.tabs[i].scene.document, copied).len(),
        1
    );
    let copy_reference = app.tabs[i]
        .scene
        .document
        .add_entity(EntityType::Insert(Insert::new("COPY", Vector3::ZERO)))
        .unwrap();
    let copy_parameter =
        actions::parameters(&app.tabs[i].scene.document, copied, actions::Kind::Move)[0].handle;
    assert_ne!(copy_parameter, parameter);
    let changed = actions::instance::evaluate(
        &app.tabs[i].scene.document,
        copy_reference,
        copy_parameter,
        glam::DVec3::new(15., 0., 0.),
    )
    .unwrap()
    .unwrap();
    let original_parameter =
        actions::parameters(&changed, original, actions::Kind::Move)[0].clone();
    assert_eq!(
        actions::instance::current(&changed, reference, &original_parameter).unwrap(),
        glam::DVec3::new(10., 0., 0.)
    );
    assert_eq!(
        changed.get_entity(targets[0]),
        before.get_entity(targets[0])
    );
}

#[test]
fn save_as_refuses_unsupported_owned_graphs_without_losing_edits() {
    use codec::objects::{DynamicBlockObject, ObjectType};
    let (mut app, reference) = fixture();
    let i = app.active_tab;
    let mut object = DynamicBlockObject::new("BLOCKLINEARPARAMETER", "LinearParameter");
    object.handle = app.tabs[i].scene.document.allocate_handle();
    object.owner = app.tabs[i]
        .scene
        .document
        .block_records
        .get("EDIT_ME")
        .unwrap()
        .handle;
    app.tabs[i]
        .scene
        .document
        .objects
        .insert(object.handle, ObjectType::DynamicBlock(object));
    begin(&mut app, reference);
    circle(&mut app, 1.0);
    let document = app.tabs[i].scene.document.clone();
    assert!(!app.save_block_edit_as(i, "COPY"));
    assert_eq!(app.tabs[i].scene.document, document);
    assert_eq!(members(&app).len(), 2);
    assert_eq!(main_depth(&app), 0);
    assert!(app
        .command_line
        .last_error
        .as_ref()
        .unwrap()
        .contains("native object relationships"));
}

#[test]
fn save_as_retains_new_shared_layers_and_nested_definitions() {
    use codec::entities::{AttributeDefinition, AttributeEntity, MText};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.dispatch_command("LAYER NEW COPY_LAYER");
    let _ = app.dispatch_command("CLAYER COPY_LAYER");
    let mut definition = AttributeDefinition::new("TAG".into(), "Prompt".into(), "first".into());
    definition.is_multiline = true;
    definition.mtext_flag = codec::entities::attribute_definition::MTextFlag::MultiLine;
    let mut text = MText::new();
    text.value = "first\\Psecond".into();
    definition.embedded_mtext = Some(Box::new(text));
    app.tabs[i].scene.define_block_raw(
        "NEW_CHILD",
        Vector3::ZERO,
        vec![EntityType::AttributeDefinition(definition)],
    );
    let source_definition = app.tabs[i]
        .scene
        .document
        .block_records
        .get("NEW_CHILD")
        .unwrap()
        .entity_handles[0];
    let mut child = Insert::new("NEW_CHILD", Vector3::ZERO);
    let mut attribute = AttributeEntity::new("TAG".into(), "first".into());
    attribute.attdef_handle = source_definition;
    child.attributes.push(attribute);
    child.common.layer = "COPY_LAYER".into();
    child.common.owner_handle = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    app.tabs[i]
        .scene
        .document
        .add_entity(EntityType::Insert(child))
        .unwrap();
    assert!(app.save_block_edit_as(i, "COPY"));
    assert_eq!(app.tabs[i].active_layer, "COPY_LAYER");
    assert!(app.tabs[i]
        .scene
        .document
        .layers
        .get("COPY_LAYER")
        .is_some());
    assert!(app.tabs[i]
        .scene
        .document
        .block_records
        .get("NEW_CHILD")
        .is_some());
    assert_eq!(members(&app).len(), 1);
    let doc = &app.tabs[i].scene.document;
    let copied_definition = doc.block_records.get("NEW_CHILD").unwrap().entity_handles[0];
    assert_ne!(copied_definition, source_definition);
    let EntityType::AttributeDefinition(definition) = doc.get_entity(copied_definition).unwrap()
    else {
        unreachable!()
    };
    let text = definition.embedded_mtext.as_ref().unwrap();
    assert_eq!(text.value, "first\\Psecond");
    assert!(!text.common.handle.is_null());
    let copied_attribute = doc
        .entities_in_block("COPY")
        .find_map(|entity| match entity {
            EntityType::Insert(insert) => Some(&insert.attributes[0]),
            _ => None,
        })
        .unwrap();
    assert_eq!(copied_attribute.attdef_handle, copied_definition);
    assert!(app.tabs[i].scene.document.entities_in_block("COPY").any(|entity| matches!(entity, EntityType::Insert(insert) if insert.block_name=="NEW_CHILD" && insert.common.layer=="COPY_LAYER")));
}

#[test]
fn save_as_can_create_an_empty_definition() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let original = members(&app);
    app.push_undo_snapshot(i, "Erase");
    app.tabs[i].scene.erase_entities(&original);
    app.finish_pending_history(i);
    assert!(app.save_block_edit_as(i, "EMPTY"));
    assert_eq!(members(&app), original);
    assert!(app.tabs[i]
        .scene
        .document
        .block_records
        .get("EMPTY")
        .unwrap()
        .entity_handles
        .is_empty());
    assert!(!app.block_edit_dirty(i));
}

#[test]
fn session_undo_preserves_membership_and_cannot_reach_drawing_history() {
    let (mut app, reference) = fixture();
    circle(&mut app, 50.0);
    let depth = main_depth(&app);
    begin(&mut app, reference);
    assert_eq!(main_depth(&app), depth);
    circle(&mut app, 1.0);
    assert_eq!(members(&app).len(), 2);
    app.undo_steps(1);
    assert_eq!(members(&app).len(), 1);
    assert!(!app.block_edit_dirty(app.active_tab));
    app.undo_steps(100);
    assert_eq!(main_depth(&app), depth);
    assert!(app.tabs[app.active_tab].active_block_edit.is_some());
    app.redo_steps(1);
    assert_eq!(members(&app).len(), 2);
    assert!(members(&app).iter().all(|h| app.tabs[app.active_tab]
        .scene
        .document
        .get_entity(*h)
        .is_some()));
}

#[test]
fn repeated_saves_stay_open_and_commit_one_drawing_step_each() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    circle(&mut app, 1.0);
    circle(&mut app, 2.0);
    let _ = app.dispatch_command("BSAVE");
    assert!(app.tabs[app.active_tab].active_block_edit.is_some());
    assert_eq!(main_depth(&app), 1);
    assert!(!app.block_edit_dirty(app.active_tab));
    let _ = app.dispatch_command("BSAVE");
    assert_eq!(main_depth(&app), 1, "a clean save is not a new edit");
    circle(&mut app, 3.0);
    let _ = app.dispatch_command("BSAVE");
    assert_eq!(main_depth(&app), 2);
    circle(&mut app, 4.0);
    let _ = app.dispatch_command("BEDIT_DISCARD");
    assert_eq!(members(&app).len(), 4, "discard restores the last save");
    assert_eq!(main_depth(&app), 2);
    assert_eq!(
        app.tabs[app.active_tab]
            .history
            .undo_stack
            .last()
            .unwrap()
            .label(),
        "Edit block EDIT_ME"
    );
    app.undo_steps(1);
    assert_eq!(members(&app).len(), 3);
    app.undo_steps(1);
    assert_eq!(members(&app).len(), 1);
    app.redo_steps(2);
    assert_eq!(members(&app).len(), 4);
}

#[test]
fn discard_preserves_drawing_redo_and_never_leaks_local_edits() {
    let (mut app, reference) = fixture();
    circle(&mut app, 50.0);
    app.undo_steps(1);
    let main_redo = app.tabs[app.active_tab].history.redo_stack.len();
    begin(&mut app, reference);
    circle(&mut app, 1.0);
    let _ = app.dispatch_command("BEDIT_DISCARD");
    assert_eq!(members(&app).len(), 1);
    assert_eq!(app.tabs[app.active_tab].history.redo_stack.len(), main_redo);
    app.redo_steps(1);
    assert_eq!(members(&app).len(), 1);
}

#[test]
fn dirty_close_cancel_discard_and_save_follow_the_checkpoint() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    circle(&mut app, 1.0);
    let _ = app.dispatch_command("BCLOSE");
    assert_eq!(app.active_modal, Some(ModalKind::BlockEditChanges));
    let _ = app.update(Message::BlockEditChangesCancel);
    assert!(app.tabs[app.active_tab].active_block_edit.is_some());
    assert_eq!(members(&app).len(), 2);
    let _ = app.dispatch_command("BCLOSE");
    let _ = app.update(Message::BlockEditChangesDiscard);
    assert!(app.tabs[app.active_tab].active_block_edit.is_none());
    assert_eq!(members(&app).len(), 1);
    assert_eq!(main_depth(&app), 0);
    begin(&mut app, reference);
    circle(&mut app, 2.0);
    let _ = app.dispatch_command("BCLOSE");
    let _ = app.update(Message::BlockEditChangesSave);
    assert!(app.tabs[app.active_tab].active_block_edit.is_none());
    assert_eq!(members(&app).len(), 2);
    assert_eq!(main_depth(&app), 1);
}

#[test]
fn clean_close_does_not_prompt_or_dirty_the_drawing() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    assert!(!app.tabs[app.active_tab].dirty);
    let _ = app.dispatch_command("BCLOSE");
    assert!(app.tabs[app.active_tab].active_block_edit.is_none());
    assert!(app.active_modal.is_none());
    assert!(!app.tabs[app.active_tab].dirty);
    assert_eq!(main_depth(&app), 0);
}

#[test]
fn dirty_switch_can_cancel_and_never_nests_sessions() {
    let (mut app, reference) = fixture();
    let i = app.active_tab;
    app.tabs[i]
        .scene
        .define_block_raw("OTHER", Vector3::ZERO, vec![]);
    let other = app.tabs[i]
        .scene
        .document
        .add_entity(EntityType::Insert(Insert::new("OTHER", Vector3::ZERO)))
        .unwrap();
    begin(&mut app, reference);
    circle(&mut app, 1.0);
    let _ = app.dispatch_command(&format!("BEDIT_BEGIN:{}", other.value()));
    assert_eq!(app.active_modal, Some(ModalKind::BlockEditChanges));
    let _ = app.update(Message::BlockEditChangesCancel);
    assert_eq!(
        app.tabs[i].active_block_edit_session().unwrap().block_name,
        "EDIT_ME"
    );
    let _ = app.dispatch_command(&format!("BEDIT_BEGIN:{}", other.value()));
    let _ = app.update(Message::BlockEditChangesSave);
    assert_eq!(app.tabs[i].block_edits.len(), 1);
    assert_eq!(
        app.tabs[i].active_block_edit_session().unwrap().block_name,
        "OTHER"
    );
    assert_eq!(main_depth(&app), 1);
}

#[test]
fn tab_close_prompts_for_block_edits_before_drawing_changes() {
    let (mut app, reference) = fixture();
    let id = app.tabs[app.active_tab].id;
    begin(&mut app, reference);
    circle(&mut app, 1.0);
    let _ = app.update(Message::TabClose(id));
    assert_eq!(app.active_modal, Some(ModalKind::BlockEditChanges));
    let _ = app.update(Message::BlockEditChangesCancel);
    assert!(app.tabs.iter().any(|t| t.id == id));
    let _ = app.update(Message::TabClose(id));
    let _ = app.update(Message::BlockEditChangesDiscard);
    assert!(!app.tabs.iter().any(|t| t.id == id));
}

#[test]
fn save_refuses_a_cycle_introduced_outside_the_insert_command() {
    let (mut app, reference) = fixture();
    let i = app.active_tab;
    app.tabs[i].scene.define_block_raw(
        "OTHER",
        Vector3::ZERO,
        vec![EntityType::Insert(Insert::new("EDIT_ME", Vector3::ZERO))],
    );
    begin(&mut app, reference);
    app.tabs[i]
        .scene
        .add_entity(EntityType::Insert(Insert::new("OTHER", Vector3::ZERO)));
    assert!(!app.save_block_edit(i));
    assert_eq!(main_depth(&app), 0);
    assert!(app.tabs[i].active_block_edit.is_some());
}

#[test]
fn drawing_history_retains_the_paper_layout_after_save_and_undo() {
    let (mut app, reference) = fixture();
    let i = app.active_tab;
    app.tabs[i].scene.add_layout("Sheet").unwrap();
    app.tabs[i].scene.set_current_layout("Sheet".into());
    begin(&mut app, reference);
    circle(&mut app, 1.0);
    let _ = app.dispatch_command("BEDIT_SAVE");
    assert_eq!(app.tabs[i].scene.current_layout, "Sheet");
    assert!(!app.tabs[i].scene.document.header.show_model_space);
    app.undo_steps(1);
    assert_eq!(app.tabs[i].scene.current_layout, "Sheet");
    assert!(!app.tabs[i].scene.document.header.show_model_space);
    assert_eq!(
        crate::io::saved_active_layout(&app.tabs[i].scene.document).as_deref(),
        Some("Sheet")
    );
    assert_eq!(members(&app).len(), 1);
    app.redo_steps(1);
    assert!(!app.tabs[i].scene.document.header.show_model_space);
    assert_eq!(members(&app).len(), 2);
}

#[test]
fn local_parameters_are_saved_and_discarded_with_the_definition() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let scope = app.tabs[i].current_parametric_scope();
    app.tabs[i]
        .scene
        .parametric_constraint_set_mut(scope)
        .local_parameters
        .set("Width", "7")
        .unwrap();
    assert!(app.save_block_edit(i));
    app.tabs[i]
        .scene
        .parametric_constraint_set_mut(scope)
        .local_parameters
        .set("Width", "9")
        .unwrap();
    let _ = app.dispatch_command("BEDIT_DISCARD");
    assert_eq!(
        app.tabs[i]
            .scene
            .parametric_constraint_set(scope)
            .unwrap()
            .local_parameters
            .resolve("Width")
            .unwrap(),
        7.0
    );
    app.undo_steps(1);
    assert!(!app.tabs[i]
        .scene
        .parametric_constraint_set(scope)
        .unwrap()
        .local_parameters
        .contains("Width"));
    app.redo_steps(1);
    assert_eq!(
        app.tabs[i]
            .scene
            .parametric_constraint_set(scope)
            .unwrap()
            .local_parameters
            .resolve("Width")
            .unwrap(),
        7.0
    );
}

#[test]
fn quit_prompts_for_block_edits_before_unsaved_drawing_changes() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    circle(&mut app, 1.0);
    let _ = app.request_quit_with_block_edits();
    assert_eq!(app.active_modal, Some(ModalKind::BlockEditChanges));
    let _ = app.update(Message::BlockEditChangesCancel);
    assert!(app.pending_close.is_none());
    assert!(app.tabs[app.active_tab].active_block_edit.is_some());
    let _ = app.request_quit_with_block_edits();
    let _ = app.update(Message::BlockEditChangesSave);
    assert!(app.tabs[app.active_tab].active_block_edit.is_none());
    assert_eq!(app.active_modal, Some(ModalKind::Unsaved));
    assert!(matches!(app.pending_close, Some(super::PendingClose::Quit)));
}

#[test]
fn document_replacement_is_refused_until_the_editor_is_closed() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    assert_eq!(app.automation_op(r#"{"op":"new"}"#)["ok"], false);
    assert!(app.tabs[app.active_tab].active_block_edit.is_some());
}

#[test]
fn erasing_the_last_member_saves_an_empty_definition_and_undo_restores_it() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let line = members(&app)[0];
    app.push_undo_snapshot(i, "Erase block member");
    app.tabs[i].scene.erase_entities(&[line]);
    app.finish_pending_history(i);
    assert!(members(&app).is_empty());
    app.undo_steps(1);
    assert_eq!(members(&app), vec![line]);
    app.redo_steps(1);
    assert!(members(&app).is_empty());
    let _ = app.dispatch_command("BEDIT_SAVE");
    assert!(members(&app).is_empty());
    app.undo_steps(1);
    assert_eq!(members(&app), vec![line]);
}

#[test]
fn close_all_resumes_after_discarding_a_dirty_block() {
    let (mut app, reference) = fixture();
    let first = app.tabs[app.active_tab].id;
    app.tabs.push(super::document::DocumentTab::new_drawing(2));
    let second = app.tabs.last().unwrap().id;
    begin(&mut app, reference);
    circle(&mut app, 1.0);
    let _ = app.update(Message::DocTabCloseAll);
    assert_eq!(app.active_modal, Some(ModalKind::BlockEditChanges));
    let _ = app.update(Message::BlockEditChangesDiscard);
    assert!(!app
        .tabs
        .iter()
        .any(|tab| tab.id == first || tab.id == second));
    assert!(app.pending_tab_closes.is_empty());
}

#[test]
fn explode_inside_session_edits_the_definition_and_marks_dirty() {
    let (mut app, reference) = fixture();
    let i = app.active_tab;
    app.tabs[i].scene.define_block_raw(
        "CHILD",
        Vector3::ZERO,
        vec![EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::new(3.0, 0.0, 0.0),
        ))],
    );
    begin(&mut app, reference);
    let _ = app.dispatch_command("-INSERT");
    feed(&mut app, "CHILD");
    feed(&mut app, "0,5");
    feed(&mut app, "1");
    feed(&mut app, "1");
    feed(&mut app, "0");
    app.finish_pending_history(i);
    let before = members(&app);
    assert_eq!(before.len(), 2, "the nested INSERT joins the definition");
    let insert_handle = *before.last().unwrap();
    assert!(
        matches!(
            app.tabs[i].scene.document.get_entity(insert_handle),
            Some(EntityType::Insert(_))
        ),
        "expected the member just added to be the INSERT"
    );
    let depth = main_depth(&app);
    app.tabs[i].scene.select_entity(insert_handle, true);
    let _ = app.dispatch_command("EXPLODE");
    assert!(
        app.block_edit_dirty(i),
        "the explode marks the session dirty"
    );
    assert_eq!(main_depth(&app), depth, "the main stack is untouched");
    let _ = app.dispatch_command("BSAVE");
    let saved = members(&app);
    assert!(
        saved
            .iter()
            .all(|h| app.tabs[i].scene.document.get_entity(*h).is_some()),
        "the saved definition carries no dangling member handle: {saved:?}"
    );
    assert_eq!(saved.len(), 2, "the INSERT is replaced by its piece");
    assert!(
        !saved.contains(&insert_handle),
        "the exploded INSERT leaves the saved definition"
    );
    assert!(
        saved.iter().any(|h| {
            matches!(
                app.tabs[i].scene.document.get_entity(*h),
                Some(EntityType::Line(_))
            )
        }),
        "the child line lands as a member of the edited block"
    );
    let _ = app.dispatch_command("BEDIT_DISCARD");
}

#[test]
fn xclip_inside_session_stays_in_the_definition() {
    let (mut app, reference) = fixture();
    let i = app.active_tab;
    app.tabs[i].scene.define_block_raw(
        "CHILD",
        Vector3::ZERO,
        vec![EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::new(3.0, 0.0, 0.0),
        ))],
    );
    begin(&mut app, reference);
    let _ = app.dispatch_command("-INSERT");
    feed(&mut app, "CHILD");
    feed(&mut app, "0,5");
    feed(&mut app, "1");
    feed(&mut app, "1");
    feed(&mut app, "0");
    app.finish_pending_history(i);
    let before = members(&app);
    let insert_handle = *before.last().unwrap();
    let depth = main_depth(&app);
    app.tabs[i].scene.select_entity(insert_handle, true);
    let _ = app.dispatch_command("XCLIP");
    feed(&mut app, "N");
    feed(&mut app, "R");
    feed(&mut app, "0,0");
    feed(&mut app, "10,10");
    app.finish_pending_history(i);
    assert!(
        crate::scene::pick::xclip::filter_handle(&app.tabs[i].scene.document, insert_handle)
            .is_some(),
        "the clip filter attaches to the in-block reference"
    );
    assert!(
        members(&app).contains(&insert_handle),
        "the clipped reference stays a member of the definition"
    );
    assert!(app.block_edit_dirty(i), "the clip marks the session dirty");
    assert_eq!(main_depth(&app), depth, "the main stack is untouched");
    let _ = app.dispatch_command("BEDIT_DISCARD");
}

#[test]
fn attdef_inside_session_lands_in_the_definition_and_undo_stays_local() {
    let (mut app, reference) = fixture();
    let i = app.active_tab;
    let depth = main_depth(&app);
    begin(&mut app, reference);
    assert_eq!(members(&app).len(), 1);
    let _ = app.dispatch_command("-ATTDEF");
    feed(&mut app, ""); // modes: done
    feed(&mut app, "TAG1");
    feed(&mut app, "prompt text");
    feed(&mut app, ""); // default value
    feed(&mut app, ""); // single line
    feed(&mut app, "0,5");
    feed(&mut app, "2");
    feed(&mut app, "0");
    app.finish_pending_history(i);
    let after = members(&app);
    assert_eq!(after.len(), 2, "the ATTDEF joins the definition");
    assert!(
        matches!(
            app.tabs[i]
                .scene
                .document
                .get_entity(*after.last().unwrap()),
            Some(EntityType::AttributeDefinition(_))
        ),
        "the new member is the ATTDEF"
    );
    assert!(
        app.block_edit_dirty(i),
        "the ATTDEF marks the session dirty"
    );
    assert_eq!(main_depth(&app), depth, "the main stack is untouched");
    app.undo_steps(1);
    assert_eq!(members(&app).len(), 1, "session undo removes the ATTDEF");
    assert_eq!(main_depth(&app), depth, "the main stack is still untouched");
    let _ = app.dispatch_command("BEDIT_DISCARD");
}

#[test]
fn save_after_session_undo_does_not_write_a_dangling_member() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    circle(&mut app, 1.0);
    assert_eq!(members(&app).len(), 2);
    app.undo_steps(1);
    assert_eq!(members(&app).len(), 1);
    let depth = main_depth(&app);
    let _ = app.dispatch_command("BSAVE");
    assert!(app.tabs[app.active_tab].active_block_edit.is_some());
    let saved = members(&app);
    assert!(
        saved.iter().all(|h| app.tabs[app.active_tab]
            .scene
            .document
            .get_entity(*h)
            .is_some()),
        "save after a session undo must not write a member handle whose entity is gone: {saved:?}"
    );
    assert_eq!(
        main_depth(&app),
        depth,
        "a clean save commits no new drawing step"
    );
    let _ = app.dispatch_command("BEDIT_DISCARD");
    assert_eq!(members(&app).len(), 1);
}

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
    assert_eq!(app.active_modal, Some(ModalKind::BlockEditPicker));
    assert!(app.tabs[tab].active_block_edit.is_none());

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

fn placed_visibility(app: &OpenCADStudio) -> &codec::objects::BlockVisibilityParameter {
    let tab = &app.tabs[app.active_tab];
    let definition = tab
        .scene
        .document
        .block_records
        .get("EDIT_ME")
        .unwrap()
        .handle;
    crate::scene::block_visibility::parameter(&tab.scene.document, definition)
        .expect("visibility parameter")
}

#[test]
fn visibility_parameter_placement_can_cancel_without_document_or_history_changes() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let before = app.tabs[i].scene.document.clone();
    let history = app.tabs[i].history.undo_stack.len();
    let _ = app.dispatch_command("BPARAMETER");
    feed(&mut app, "Visibility");
    assert_eq!(
        app.tabs[i].active_cmd.as_ref().unwrap().name(),
        "BPARAMETER"
    );
    assert_eq!(app.tabs[i].scene.document, before);
    let _ = app.apply_cmd_result(crate::command::CmdResult::Cancel);
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), history);
    assert!(!app.block_edit_dirty(i));
}

#[test]
fn visibility_parameter_placement_has_local_undo_redo_save_and_parent_undo() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let before = app.tabs[i].scene.document.clone();
    let _ = app.dispatch_command("BPARAMETER VISIBILITY");
    feed(&mut app, "3,4,0");
    assert!(app.tabs[i].active_cmd.is_none());
    assert_eq!(
        placed_visibility(&app).def_point,
        Vector3::new(3.0, 4.0, 0.0)
    );
    assert_eq!(placed_visibility(&app).states[0].name, "VisibilityState0");
    assert_eq!(
        members(&app),
        before.block_records.get("EDIT_ME").unwrap().entity_handles
    );
    assert_eq!(app.tabs[i].history.undo_stack.len(), 1);
    assert!(app.block_edit_dirty(i));
    app.undo_steps(1);
    assert_eq!(app.tabs[i].scene.document.objects, before.objects);
    assert!(!app.block_edit_dirty(i));
    app.redo_steps(1);
    assert_eq!(
        placed_visibility(&app).def_point,
        Vector3::new(3.0, 4.0, 0.0)
    );
    let _ = app.dispatch_command("BSAVE");
    assert!(!app.block_edit_dirty(i));
    let _ = app.dispatch_command("BEDIT_DISCARD");
    assert_eq!(
        placed_visibility(&app).def_point,
        Vector3::new(3.0, 4.0, 0.0)
    );
    app.undo_steps(1);
    let definition = app.tabs[i]
        .scene
        .document
        .block_records
        .get("EDIT_ME")
        .unwrap()
        .handle;
    assert!(
        crate::scene::block_visibility::parameter(&app.tabs[i].scene.document, definition)
            .is_none()
    );
}

#[test]
fn visibility_parameter_placement_discard_removes_the_unsaved_graph() {
    let (mut app, reference) = fixture();
    let before = app.tabs[app.active_tab].scene.document.clone();
    begin(&mut app, reference);
    let _ = app.dispatch_command("BPARAMETER VISIBILITY");
    feed(&mut app, "7,8,0");
    let _ = app.dispatch_command("BEDIT_DISCARD");
    // Closing the editor synchronizes the restored camera to the active VPORT.
    let mut after = app.tabs[app.active_tab].scene.document.clone();
    after.vports = before.vports.clone();
    assert_eq!(after, before);
}

#[test]
fn visibility_parameter_placement_rejects_duplicates_stale_context_and_nonfinite_locations() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let document_id = app.tabs[i].id;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let before = app.tabs[i].scene.document.clone();
    for (document_id, location) in [
        (document_id + 1, glam::DVec3::ZERO),
        (document_id, glam::DVec3::splat(f64::NAN)),
    ] {
        let _ = app.handle_place_block_visibility_parameter(document_id, definition, location);
        assert_eq!(app.tabs[i].scene.document, before);
        assert!(app.tabs[i].history.undo_stack.is_empty());
    }
    let _ = app.dispatch_command("BPARAMETER VISIBILITY");
    feed(&mut app, "3,4,0");
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let _ = app.dispatch_command("LINE");
    let _ = app.dispatch_command("BPARAMETER VISIBILITY");
    assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(), "LINE");
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    let _ = app.dispatch_command("BTESTBLOCK");
    let test = app.active_tab;
    let before = app.tabs[test].scene.document.clone();
    let _ = app.dispatch_command("LINE");
    let _ = app.dispatch_command("BPARAMETER VISIBILITY");
    assert_eq!(app.tabs[test].active_cmd.as_ref().unwrap().name(), "LINE");
    let _ = app.handle_place_block_visibility_parameter(document_id, definition, glam::DVec3::ZERO);
    assert_eq!(app.tabs[test].scene.document, before);
}

#[test]
fn visibility_parameter_placement_preserves_an_opaque_authoring_graph() {
    use codec::objects::{DynamicBlockObject, DynamicBlockData, ObjectType};
    let (mut app, reference) = fixture();
    let i = app.active_tab;
    let doc = &mut app.tabs[i].scene.document;
    let definition = doc.block_records.get("EDIT_ME").unwrap().handle;
    let dictionary = doc.ensure_extension_dictionary(definition);
    let graph_handle = doc.allocate_handle();
    let mut graph = DynamicBlockObject::new("ACAD_EVALUATION_GRAPH", "AcDbEvalGraph");
    graph.handle = graph_handle;
    graph.owner = dictionary;
    graph.data = DynamicBlockData::Unknown;
    doc.objects
        .insert(graph_handle, ObjectType::DynamicBlock(graph));
    if let Some(ObjectType::Dictionary(dictionary)) = doc.objects.get_mut(&dictionary) {
        dictionary.add_entry("ACAD_ENHANCEDBLOCK", graph_handle);
    }
    begin(&mut app, reference);
    let before = app.tabs[i].scene.document.clone();
    for command in [
        "BPARAMETER VISIBILITY",
        "BVSTATE",
        "_BVSTATE\u{1f}NEW\u{1f}Other",
    ] {
        let _ = app.dispatch_command(command);
        assert_eq!(app.tabs[i].scene.document, before);
        assert!(app.tabs[i].history.undo_stack.is_empty());
    }
    assert!(crate::scene::block_visibility::create(
        &mut app.tabs[i].scene.document,
        definition,
        Vector3::ZERO
    )
    .is_none());
    assert_eq!(app.tabs[i].scene.document, before);
}

#[test]
fn visibility_parameter_placement_roundtrips_its_location_and_reference_grip_in_dwg_and_dxf() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let _ = app.dispatch_command("BPARAMETER VISIBILITY");
    feed(&mut app, "3,4,0");
    assert_eq!(
        app.automation_op(r#"{"op":"run","cmd":"UCS ORIGIN 1,2,0"}"#)["ok"],
        true
    );
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BEDIT_DISCARD");
    let doc = &app.tabs[app.active_tab].scene.document;
    for extension in ["dwg", "dxf"] {
        let bytes = crate::io::save_to_bytes(doc, extension, doc.version).unwrap();
        let read = crate::io::load_bytes(&format!("visibility.{extension}"), bytes).unwrap();
        let (definition, parameter) = read
            .dynamic_visibility_for_insert(reference)
            .expect(extension);
        assert_eq!(
            read.block_records.get("EDIT_ME").unwrap().handle,
            definition
        );
        assert_eq!(parameter.def_point, Vector3::new(2.0, 2.0, 0.0));
        assert_eq!(parameter.states[0].name, "VisibilityState0");
        let grips = read
            .objects
            .values()
            .filter_map(|object| match object {
                codec::objects::ObjectType::DynamicBlock(object)
                    if object.owner == parameter.owner =>
                {
                    match &object.data {
                        codec::objects::DynamicBlockData::VisibilityGrip(grip) => {
                            Some(grip.location)
                        }
                        _ => None,
                    }
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(grips, [Vector3::new(2.0, 2.0, 0.0)], "{extension}");
    }
}

#[test]
fn visibility_parameter_placement_follows_the_editor_coordinate_frame() {
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    assert_eq!(
        app.automation_op(r#"{"op":"run","cmd":"UCS ORIGIN 100,200,300"}"#)["ok"],
        true
    );
    assert_eq!(
        app.automation_op(r#"{"op":"run","cmd":"UCS Z 90"}"#)["ok"],
        true
    );
    // BEDIT bakes UCS changes into the definition and returns to local WCS.
    assert!(app.tabs[app.active_tab].active_ucs.is_none());
    let _ = app.dispatch_command("BPARAMETER VISIBILITY");
    feed(&mut app, "3,4,0");
    assert_eq!(
        placed_visibility(&app).def_point,
        Vector3::new(3.0, 4.0, 0.0)
    );
    assert_eq!(
        app.automation_op(r#"{"op":"run","cmd":"UCS ORIGIN 1,2,0"}"#)["ok"],
        true
    );
    assert_eq!(
        placed_visibility(&app).def_point,
        Vector3::new(2.0, 2.0, 0.0)
    );
}

#[test]
fn visibility_parameter_placement_reframes_an_empty_definition_and_undoes_it() {
    let (mut app, reference) = fixture();
    let i = app.active_tab;
    let handles = app.tabs[i]
        .scene
        .document
        .block_records
        .get("EDIT_ME")
        .unwrap()
        .entity_handles
        .clone();
    for handle in handles {
        app.tabs[i].scene.document.remove_entity(handle);
    }
    begin(&mut app, reference);
    let _ = app.dispatch_command("BPARAMETER VISIBILITY");
    feed(&mut app, "3,4,0");
    let before = app.tabs[i].scene.document.objects.clone();
    assert_eq!(
        app.automation_op(r#"{"op":"run","cmd":"UCS ORIGIN 1,2,0"}"#)["ok"],
        true
    );
    assert_eq!(
        placed_visibility(&app).def_point,
        Vector3::new(2.0, 2.0, 0.0)
    );
    app.undo_steps(1);
    assert_eq!(
        placed_visibility(&app).def_point,
        Vector3::new(3.0, 4.0, 0.0)
    );
    assert_eq!(app.tabs[i].scene.document.objects, before);
    app.redo_steps(1);
    assert_eq!(
        placed_visibility(&app).def_point,
        Vector3::new(2.0, 2.0, 0.0)
    );
}

#[test]
fn block_editor_display_changes_keep_rotated_labels_and_large_grips_pickable() {
    use crate::ui::overlay::authoring::{label_angle, label_corners};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 6,6 1,4");
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let item = crate::scene::block_authoring::edit::items(&app.tabs[i].scene.document, definition)
        .into_iter().find(|item| item.name.starts_with("Distance")).unwrap();
    let bounds = iced::Rectangle { x:0.0, y:0.0, width:800.0, height:600.0 };
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0,600.0);
    app.tabs[i].scene.camera.borrow_mut().fit_to_bounds_f64(
        glam::DVec3::new(-2.0,-2.0,0.0), glam::DVec3::new(10.0,10.0,0.0), 800.0/600.0);
    let _ = app.dispatch_command("BPARAMETERSIZE 48");
    let _ = app.dispatch_command("BPTEXTHORIZONTAL 0");
    let camera = app.tabs[i].scene.camera.borrow();
    let project = |v: Vector3| {
        let p = camera.project(glam::DVec3::new(v.x,v.y,v.z), bounds).unwrap();
        iced::Point::new(p.x,p.y)
    };
    let dimensions: Vec<_> = item.graphic.dimensions.iter().map(|[a,b]| [project(*a),project(*b)]).collect();
    let (anchor,label) = &item.graphic.labels[0];
    let corners = label_corners(project(*anchor), label, label_angle(&dimensions,0,false),48,"Simplex.shx");
    let cursor = iced::Point::new((corners[0].x+corners[2].x)/2.0,(corners[0].y+corners[2].y)/2.0);
    assert_eq!(app.authoring_label_at(cursor,bounds),Some((item.handle,label.clone())));
    drop(camera);
    assert!(app.pick_authoring_object(cursor,bounds));
    let _ = app.dispatch_command("BGRIPOBJSIZE 80");
    app.refresh_authoring_grips([0.0;3]);
    let g = &app.tabs[i].selected_grips[0];
    let p = app.tabs[i].scene.camera.borrow().project(g.world,bounds).unwrap();
    let cursor = iced::Point::new(p.x+35.0,p.y+35.0);
    assert!(app.authoring_grip_hit(cursor,bounds).is_some());
    let _ = app.dispatch_command("BGRIPOBJSIZE 8");
    assert!(app.authoring_grip_hit(cursor,bounds).is_none());
}

#[test]
fn block_action_association_repairs_multiple_orphans_with_local_undo_and_stale_guards() {
    use crate::scene::block_actions::{self as actions,Kind};
    let (mut app,reference)=fixture();
    let i=app.active_tab;
    let _=app.dispatch_command("LINE");
    let _=app.dispatch_command("BASSOCIATE");
    assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(),"LINE");
    begin(&mut app,reference);
    let _ = app.dispatch_command("BACTIONBARMODE 0");
    let _=app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
    let definition=app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let original=actions::all_parameters(&app.tabs[i].scene.document,definition)[0].clone();
    let _=app.run_command_line("BPARAMETER XY 0,0 10,10 5,5");
    let replacement=actions::all_parameters(&app.tabs[i].scene.document,definition).into_iter().find(|p| p.xy).unwrap();
    let target=members(&app)[0];
    let mut handles=vec![];
    for x in [3.0,5.0] {
        let action=actions::create(&mut app.tabs[i].scene.document,definition,Kind::Move,original.handle,&[target],glam::DVec3::new(x,3.0,0.0)).unwrap();
        let Some(codec::objects::ObjectType::DynamicBlock(object))=app.tabs[i].scene.document.objects.get_mut(&action) else { panic!() };
        let codec::objects::DynamicBlockData::MoveAction(native)=&mut object.data else { panic!() };
        native.connections.iter_mut().for_each(|connection| connection.code=-1);
        handles.push(action);
    }
    let baseline=app.tabs[i].scene.document.clone();
    let depth=app.tabs[i].history.undo_stack.len();
    app.tabs[i].selected_authoring=Some(handles[0]);
    let _=app.dispatch_command("BASSOCIATE");
    let _=app.feed_command(crate::command::StepInput::Enter);
    assert_eq!(app.tabs[i].scene.document,baseline);
    assert_eq!(app.tabs[i].history.undo_stack.len(),depth);
    let _=app.dispatch_command("BASSOCIATE");
    feed(&mut app,&replacement.name);
    feed(&mut app,"X");
    assert!(app.tabs[i].active_cmd.is_none());
    assert_eq!(actions::association::orphaned(&app.tabs[i].scene.document,definition),vec![handles[1]]);
    assert_eq!(app.tabs[i].history.undo_stack.len(),depth+1);
    app.tabs[i].selected_authoring=Some(handles[1]);
    let _=app.dispatch_command("BASSOCIATE");
    feed(&mut app,&original.name);
    let _=app.feed_command(crate::command::StepInput::Enter);
    assert!(app.tabs[i].active_cmd.is_none());
    assert!(actions::association::orphaned(&app.tabs[i].scene.document,definition).is_empty());
    assert_eq!(app.tabs[i].history.undo_stack.len(),depth+2);
    app.undo_steps(1);
    assert_eq!(actions::association::orphaned(&app.tabs[i].scene.document,definition),vec![handles[1]]);
    let expected=app.tabs[i].scene.document.objects[&handles[1]].clone();
    let Some(codec::objects::ObjectType::DynamicBlock(object))=app.tabs[i].scene.document.objects.get_mut(&handles[1]) else { panic!() };
    let codec::objects::DynamicBlockData::MoveAction(native)=&mut object.data else { panic!() };
    native.action.display_location.x+=1.0;
    let before=app.tabs[i].scene.document.clone();
    let depth=app.tabs[i].history.undo_stack.len();
    let _=app.handle_block_action_result(crate::command::CmdResult::AssociateBlockAction {
        document:app.tabs[i].id,definition,action:handles[1],expected:Box::new(expected),parameter:original.handle,key_point:Some(actions::XyKeyPoint::End),
    });
    assert_eq!(app.tabs[i].scene.document,before);
    assert_eq!(app.tabs[i].history.undo_stack.len(),depth);
    assert!(app.command_line.last_error.as_ref().unwrap().contains("changed"));
}

#[test]
fn block_grip_cycling_dialog_is_staged_undoable_and_rejects_stale_grips() {
    let (mut app,reference)=fixture();
    let i=app.active_tab;
    let _=app.dispatch_command("LINE");
    let _=app.dispatch_command("BCYCLEORDER");
    assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(),"LINE");
    begin(&mut app,reference);
    let _=app.dispatch_command("BCYCLEORDER");
    assert!(app.block_cycle_order.as_ref().unwrap().rows.is_empty());
    assert_eq!(app.block_cycle_order.as_ref().unwrap().selected,None);
    let _=app.update(Message::CloseModal);
    let _=app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
    let _=app.run_command_line("BPARAMETER POINT 3,4 6,6");
    let definition=app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let baseline=app.tabs[i].scene.document.clone();
    let depth=app.tabs[i].history.undo_stack.len();
    let _=app.dispatch_command("BCYCLEORDER");
    assert_eq!(app.active_modal,Some(ModalKind::BlockCycleOrder));
    assert_eq!(app.block_cycle_order.as_ref().unwrap().rows.len(),2);
    let _=app.update(Message::BlockCycleSelect(0));
    let selected_handle=app.block_cycle_order.as_ref().unwrap().rows[0].handle;
    assert_eq!(app.block_cycle_order.as_ref().unwrap().rows[0].parameter_type,"Linear");
    let _=app.update(Message::BlockCycleEnable(0,true));
    let _=app.update(Message::BlockCycleEnable(1,true));
    let _=app.update(Message::BlockCycleMove(0,false));
    let state=app.block_cycle_order.as_ref().unwrap();
    assert_eq!(state.selected,Some(0));
    assert_ne!(state.rows[state.selected.unwrap()].handle,selected_handle);
    let _=app.update(Message::BlockCycleSelect(1));
    let _=app.update(Message::BlockCycleMove(1,true));
    let state=app.block_cycle_order.as_ref().unwrap();
    assert_eq!(state.selected,Some(0));
    assert_eq!(state.rows[0].handle,selected_handle);
    let _=app.update(Message::CloseModal);
    assert_eq!(app.tabs[i].scene.document,baseline);
    assert_eq!(app.tabs[i].history.undo_stack.len(),depth);
    assert!(app.block_cycle_order.is_none());
    let _=app.dispatch_command("BCYCLEORDER");
    let handles=app.block_cycle_order.as_ref().unwrap().rows.iter().map(|row| row.handle).collect::<Vec<_>>();
    let _=app.update(Message::BlockCycleEnable(0,true));
    let _=app.update(Message::BlockCycleEnable(1,true));
    let _=app.update(Message::BlockCycleMove(0,false));
    let _=app.update(Message::BlockCycleApply);
    assert!(app.active_modal.is_none());
    let grips=crate::scene::block_grips::grips(&app.tabs[i].scene.document,definition).unwrap();
    assert_eq!(grips.iter().map(|grip| grip.handle).collect::<Vec<_>>(),vec![handles[1],handles[0]]);
    assert!(grips.iter().all(|grip| grip.enabled));
    assert_eq!(app.tabs[i].history.undo_stack.len(),depth+1);
    app.undo_steps(1);
    assert_eq!(crate::scene::block_grips::insertion_points(&app.tabs[i].scene.document,definition),
        crate::scene::block_grips::insertion_points(&baseline,definition));
    app.redo_steps(1);
    let points=crate::scene::block_grips::insertion_points(&app.tabs[i].scene.document,definition);
    assert_eq!(points.len(),2);
    let _=app.dispatch_command("BCYCLEORDER");
    let _=app.update(Message::BlockCycleApply);
    assert_eq!(app.tabs[i].history.undo_stack.len(),depth+1);
    let _=app.dispatch_command("BCYCLEORDER");
    let Some(codec::objects::ObjectType::DynamicBlock(object))=app.tabs[i].scene.document.objects.get_mut(&handles[0]) else { panic!() };
    let codec::objects::DynamicBlockData::LinearGrip(grip)=&mut object.data else { panic!() };
    grip.grip.location.x+=1.0;
    let externally_changed=app.tabs[i].scene.document.clone();
    let _=app.update(Message::BlockCycleApply);
    assert_eq!(app.tabs[i].scene.document,externally_changed);
    assert!(app.block_cycle_order.as_ref().unwrap().error.as_ref().unwrap().contains("changed"));
    let _=app.update(Message::CloseModal);
}

#[test]
fn block_grip_cycling_ctrl_edges_use_saved_order_during_classic_insertion() {
    let (mut app,reference)=fixture();
    begin(&mut app,reference);
    let i=app.active_tab;
    let _=app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
    let _=app.run_command_line("BPARAMETER POINT 3,4 6,6");
    let definition=app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let grips=crate::scene::block_grips::grips(&app.tabs[i].scene.document,definition).unwrap();
    let rows=grips.iter().rev().map(|g| (g.handle,true)).collect::<Vec<_>>();
    crate::scene::block_grips::set_cycle_order(&mut app.tabs[i].scene.document,definition,&rows).unwrap();
    let points=crate::scene::block_grips::insertion_points(&app.tabs[i].scene.document,definition);
    app.tabs[i].dirty=true;
    let _=app.dispatch_command("BSAVE");
    let _=app.dispatch_command("BCLOSE");
    let _=app.dispatch_command("-INSERT");
    feed(&mut app,"EDIT_ME");
    let _=app.update(Message::SetModifiers { shift:false,ctrl:true,alt:false });
    let prompt=app.tabs[i].active_cmd.as_ref().unwrap().prompt();
    assert!(prompt.contains("(2/3)"),"{prompt}");
    let _=app.update(Message::SetModifiers { shift:false,ctrl:true,alt:false });
    assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().prompt(),prompt);
    let _=app.update(Message::SetModifiers { shift:false,ctrl:false,alt:false });
    assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().prompt(),prompt);
    let _=app.update(Message::SetModifiers { shift:false,ctrl:true,alt:false });
    assert!(app.tabs[i].active_cmd.as_ref().unwrap().prompt().contains("(3/3)"));
    feed(&mut app,"100,200");
    for _ in 0..3 { let _=app.feed_command(crate::command::StepInput::Enter); }
    let inserted=app.tabs[i].scene.document.entities().find_map(|entity| match entity {
        EntityType::Insert(insert) if insert.common.handle!=reference && insert.block_name=="EDIT_ME" => Some(insert),_ => None,
    }).unwrap();
    let landed=crate::scene::render_graph::insert_transform(&app.tabs[i].scene.document,inserted).apply(points[1]);
    assert!(landed.distance(&Vector3::new(100.0,200.0,0.0))<1e-8);
    let handle=inserted.common.handle;
    app.undo_steps(1);
    assert!(app.tabs[i].scene.document.get_entity(handle).is_none());
}

#[test]
fn block_action_bars_group_native_actions_and_preserve_drawing_history() {
    use crate::scene::{block_action_bars as bars, block_actions::{self as actions, Kind}};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    assert!(app.block_editor_preferences.action_bars);
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let parameter = actions::all_parameters(&app.tabs[i].scene.document, definition)[0].clone();
    let targets = members(&app);
    let mut handles = vec![];
    for kind in [Kind::Move, Kind::Scale] {
        handles.push(actions::create(&mut app.tabs[i].scene.document, definition, kind,
            parameter.handle, &targets, glam::DVec3::new(4.,4.,0.)).unwrap());
    }
    let groups = bars::groups(&app.tabs[i].scene.document, definition);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].parameter, parameter.handle);
    assert_eq!(groups[0].entries.len(), 2);
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let local_depth = main_depth(&app);
    let _ = app.update(Message::BlockActionBarSelect(handles[0]));
    assert_eq!(app.tabs[i].selected_authoring, Some(handles[0]));
    let _ = app.update(Message::BlockActionBarMove(parameter.handle, iced::Vector::new(80., 25.)));
    assert_eq!(app.tabs[i].action_bars.offsets[&parameter.handle], iced::Vector::new(80.,25.));
    let _ = app.dispatch_command("BACTIONBAR Hide");
    let _ = app.feed_command(crate::command::StepInput::Enter);
    assert!(app.tabs[i].action_bars.hidden.contains(&parameter.handle));
    app.tabs[i].selected_authoring = None;
    let _ = app.update(Message::BlockActionBarSelect(handles[0]));
    assert!(app.tabs[i].selected_authoring.is_none(), "hidden controls cannot select");
    let _ = app.dispatch_command("BACTIONBAR Show All");
    assert!(app.tabs[i].action_bars.hidden.is_empty());
    assert!(app.tabs[i].action_bars.offsets.contains_key(&parameter.handle));
    let _ = app.update(Message::BlockActionBarHide(parameter.handle));
    let _ = app.dispatch_command("BACTIONBAR Reset");
    assert!(app.tabs[i].action_bars.hidden.is_empty());
    assert!(app.tabs[i].action_bars.offsets.is_empty());
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    assert_eq!(main_depth(&app), local_depth);
    let _ = app.update(Message::BlockActionBarEdit(handles[0]));
    assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(), "BACTIONSET");
    let _ = app.update(Message::CommandEscape);
    let _ = app.dispatch_command("LINE");
    for command in ["BACTIONSET", "BASSOCIATE", "BLOOKUPTABLE"] {
        let _ = app.dispatch_command(command);
        assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(), "LINE");
    }
    let _ = app.update(Message::CommandEscape);
    let _ = app.dispatch_command("BACTIONBARMODE 0");
    let _ = app.dispatch_command("LINE");
    let _ = app.dispatch_command("BACTIONBAR Reset");
    assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(), "LINE");
    let _ = app.update(Message::CommandEscape);
    let _ = app.update(Message::BlockActionBarEdit(handles[0]));
    assert!(app.tabs[i].active_cmd.is_none());
    let _ = app.dispatch_command("BACTIONSET");
    assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(), "BACTIONSET");
}

#[test]
fn block_action_bars_open_lookup_and_orphan_association_and_ignore_stale_callbacks() {
    use crate::scene::{block_action_bars as bars, block_actions::{self as actions, Kind}};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LOOKUP 3,3");
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let lookup = crate::scene::block_lookup::parameters(&app.tabs[i].scene.document, definition)[0].0;
    let _ = app.update(Message::BlockActionBarEdit(lookup));
    assert_eq!(app.active_modal, Some(ModalKind::BlockLookup));
    assert_eq!(app.block_lookup.as_ref().unwrap().drafts[app.block_lookup.as_ref().unwrap().selected].parameter, lookup);
    let _ = app.update(Message::CloseModal);
    let parameter = actions::all_parameters(&app.tabs[i].scene.document, definition)[0].clone();
    let targets = members(&app);
    let action = actions::create(&mut app.tabs[i].scene.document, definition, Kind::Move,
        parameter.handle, &targets, glam::DVec3::new(4.,4.,0.)).unwrap();
    let Some(codec::objects::ObjectType::DynamicBlock(object)) = app.tabs[i].scene.document.objects.get_mut(&action) else { panic!() };
    let codec::objects::DynamicBlockData::MoveAction(native) = &mut object.data else { panic!() };
    native.connections.iter_mut().for_each(|connection| connection.code = -1);
    assert!(bars::groups(&app.tabs[i].scene.document, definition).iter().any(|group|
        group.entries.iter().any(|entry| entry.handle == action && matches!(entry.kind,bars::EntryKind::Orphan(_)))));
    let _ = app.update(Message::BlockActionBarEdit(action));
    assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(), "BASSOCIATE");
    let _ = app.update(Message::CommandEscape);
    let _ = app.dispatch_command("BEDIT_DISCARD");
    let before = app.tabs[i].scene.document.clone();
    let _ = app.update(Message::BlockActionBarMove(parameter.handle, iced::Vector::new(2.,3.)));
    let _ = app.update(Message::BlockActionBarHide(parameter.handle));
    let _ = app.update(Message::BlockActionBarEdit(action));
    assert!(app.tabs[i].active_cmd.is_none());
    assert!(app.tabs[i].action_bars.offsets.is_empty());
    assert_eq!(app.tabs[i].scene.document, before);
}

#[test]
fn block_action_overrides_properties_are_atomic_and_locally_undoable() {
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let parameter = actions::all_parameters(&app.tabs[i].scene.document,definition)[0].handle;
    let targets = members(&app);
    let action = actions::create(&mut app.tabs[i].scene.document,definition,Kind::Move,parameter,&targets,glam::DVec3::ZERO).unwrap();
    let _ = app.update(Message::BlockActionBarSelect(action));
    let fields=app.tabs[i].properties.sections.iter().flat_map(|s| &s.props).map(|p|p.field).collect::<Vec<_>>();
    assert!(fields.contains(&"authoring_action_multiplier"));
    assert!(fields.contains(&"authoring_action_angle"));
    let initial = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    for (field,value) in [("authoring_action_multiplier","1/2"),("authoring_action_angle","90")] {
        let _=app.update(Message::PropGeomInput{field,value:value.into()});
        let _=app.update(Message::PropGeomCommit(field));
    }
    let offsets=actions::options::offsets(&app.tabs[i].scene.document,action).unwrap();
    assert_eq!(offsets.distance_multiplier,0.5);
    assert_eq!(offsets.angle_offset,std::f64::consts::FRAC_PI_2);
    assert_eq!(app.tabs[i].history.undo_stack.len(),depth+2);
    let changed=app.tabs[i].scene.document.clone();
    for value in ["NaN","inf","unresolved_name"] {
        let _=app.update(Message::PropGeomInput{field:"authoring_action_multiplier",value:value.into()});
        let _=app.update(Message::PropGeomCommit("authoring_action_multiplier"));
        assert_eq!(app.tabs[i].scene.document,changed);
        assert_eq!(app.tabs[i].history.undo_stack.len(),depth+2);
    }
    app.undo_steps(2);
    assert_eq!(app.tabs[i].scene.document.objects[&action],initial.objects[&action]);
    app.redo_steps(2);
    assert_eq!(app.tabs[i].scene.document.objects[&action],changed.objects[&action]);
    let _=app.dispatch_command("BEDIT_DISCARD");
    let before=app.tabs[i].scene.document.clone();
    let _=app.update(Message::PropGeomCommit("authoring_action_multiplier"));
    assert_eq!(app.tabs[i].scene.document,before);
}

#[test]
fn block_action_overrides_command_options_create_one_atomic_action() {
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app,reference);
    let i=app.active_tab;
    let _=app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
    let definition=app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let parameter=actions::all_parameters(&app.tabs[i].scene.document,definition)[0].clone();
    let targets=members(&app);
    let before=app.tabs[i].scene.document.clone();
    let depth=app.tabs[i].history.undo_stack.len();
    let _=app.dispatch_command("BACTION MOVE");
    feed(&mut app,&parameter.name);
    let _=app.feed_command(crate::command::StepInput::SelectionComplete(targets));
    let _=app.feed_command(crate::command::StepInput::Enter);
    feed(&mut app,"Multiplier");
    feed(&mut app,"NaN");
    assert!(app.tabs[i].active_cmd.is_some());
    assert_eq!(app.tabs[i].scene.document,before);
    feed(&mut app,"0.5");
    feed(&mut app,"Offset");
    feed(&mut app,"90");
    feed(&mut app,"4,4");
    assert!(app.tabs[i].active_cmd.is_none());
    let native=actions::actions(&app.tabs[i].scene.document,definition);
    assert_eq!(native.len(),1);
    assert_eq!(native[0].2,Kind::Move);
    let action=native[0].0;
    let offsets=actions::options::offsets(&app.tabs[i].scene.document,action).unwrap();
    assert_eq!(offsets.distance_multiplier,0.5);
    assert_eq!(offsets.angle_offset,std::f64::consts::FRAC_PI_2);
    assert_eq!(app.tabs[i].history.undo_stack.len(),depth+1);
    app.undo_steps(1);
    assert!(actions::actions(&app.tabs[i].scene.document,definition).is_empty());
    app.redo_steps(1);
    assert!(actions::options::offsets(&app.tabs[i].scene.document,action).is_some());
}

#[test]
fn block_polygon_stretch_command_cancel_close_replace_and_local_history() {
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app,reference)=fixture();
    begin(&mut app,reference);
    let i=app.active_tab;
    let _=app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
    let definition=app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let parameter=actions::all_parameters(&app.tabs[i].scene.document,definition)[0].clone();
    let before=app.tabs[i].scene.document.clone();
    let depth=app.tabs[i].history.undo_stack.len();
    let _=app.dispatch_command("BACTION STRETCH");
    feed(&mut app,&parameter.name);
    feed(&mut app,"CPolygon");
    feed(&mut app,"0,0");
    feed(&mut app,"2,0");
    feed(&mut app,"Close");
    assert!(app.tabs[i].active_cmd.is_some());
    assert_eq!(app.tabs[i].scene.document,before);
    let _=app.update(Message::CommandEscape);
    assert_eq!(app.tabs[i].scene.document,before);
    assert_eq!(app.tabs[i].history.undo_stack.len(),depth);
    let _=app.dispatch_command("BACTION STRETCH");
    feed(&mut app,&parameter.name);
    feed(&mut app,"CPolygon");
    for value in ["-1,-1","2,-1","8,8","Undo","2,2","-1,2"] {feed(&mut app,value);}
    let _=app.feed_command(crate::command::StepInput::Enter);
    let targets=members(&app);
    let _=app.feed_command(crate::command::StepInput::SelectionComplete(targets));
    let _=app.feed_command(crate::command::StepInput::Enter);
    feed(&mut app,"4,4");
    assert!(app.tabs[i].active_cmd.is_none());
    let action=actions::actions(&app.tabs[i].scene.document,definition).iter().find(|(_,_,kind,_)|*kind==Kind::Stretch).unwrap().0;
    assert_eq!(actions::frame_outline(&app.tabs[i].scene.document,action).len(),4);
    let created=app.tabs[i].scene.document.objects[&action].clone();
    assert_eq!(app.tabs[i].history.undo_stack.len(),depth+1);
    let _=app.update(Message::BlockActionBarSelect(action));
    let _=app.update(Message::BlockActionBarEdit(action));
    feed(&mut app,"Frame");
    feed(&mut app,"CPolygon");
    for value in ["0,-1","2,-1","0,2"] {feed(&mut app,value);}
    feed(&mut app,"Close");
    assert!(app.tabs[i].active_cmd.is_none());
    assert_eq!(actions::frame_outline(&app.tabs[i].scene.document,action).len(),3);
    assert_eq!(app.tabs[i].history.undo_stack.len(),depth+2);
    app.undo_steps(1);
    assert_eq!(app.tabs[i].scene.document.objects[&action],created);
    app.undo_steps(1);
    assert!(actions::actions(&app.tabs[i].scene.document,definition).is_empty());
    app.redo_steps(2);
    assert_eq!(actions::frame_outline(&app.tabs[i].scene.document,action).len(),3);
}

#[test]
fn polar_stretch_authoring_collects_rotation_selection_and_edits_it_with_local_undo() {
    use crate::scene::block_actions::{self as actions,Kind};
    let (mut app,reference)=fixture();
    begin(&mut app,reference);
    let i=app.active_tab;
    circle(&mut app,3.);
    let _=app.run_command_line("BPARAMETER POLAR 0,0 10,0 5,2");
    let definition=app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let parameter=actions::all_parameters(&app.tabs[i].scene.document,definition)[0].clone();
    let targets=members(&app);
    let before=app.tabs[i].scene.document.clone();
    let depth=app.tabs[i].history.undo_stack.len();
    let _=app.dispatch_command("BACTION POLAR STRETCH");
    feed(&mut app,&parameter.name);
    feed(&mut app,"9,-1");feed(&mut app,"11,1");
    let _=app.feed_command(crate::command::StepInput::Enter);
    assert!(app.tabs[i].active_cmd.as_ref().unwrap().prompt().contains("rotate only"));
    let _=app.feed_command(crate::command::StepInput::SelectionComplete(vec![targets[1]]));
    let _=app.update(Message::CommandEscape);
    assert_eq!(app.tabs[i].scene.document,before);
    assert_eq!(app.tabs[i].history.undo_stack.len(),depth);
    let _=app.dispatch_command("BACTION POLAR STRETCH");
    feed(&mut app,&parameter.name);
    feed(&mut app,"9,-1");feed(&mut app,"11,1");
    let _=app.feed_command(crate::command::StepInput::SelectionComplete(vec![targets[0]]));
    let _=app.feed_command(crate::command::StepInput::Enter);
    assert!(app.tabs[i].active_cmd.as_ref().unwrap().prompt().contains("rotate only"));
    let _=app.feed_command(crate::command::StepInput::SelectionComplete(vec![targets[1]]));
    let _=app.feed_command(crate::command::StepInput::Enter);
    feed(&mut app,"Multiplier");feed(&mut app,"2");
    feed(&mut app,"Offset");feed(&mut app,"90");
    assert_eq!(app.tabs[i].scene.document,before,"all selections and overrides are staged");
    feed(&mut app,"4,4");
    assert!(app.tabs[i].active_cmd.is_none());
    let action=actions::actions(&app.tabs[i].scene.document,definition).iter().find(|(_,_,kind,_)|*kind==Kind::PolarStretch).unwrap().0;
    let native=app.tabs[i].scene.document.objects[&action].clone();
    let codec::objects::ObjectType::DynamicBlock(object)=&native else {panic!()};
    let codec::objects::DynamicBlockData::PolarStretchAction(polar)=&object.data else {panic!()};
    assert_eq!(polar.handles,vec![targets[1]]);
    assert_eq!(polar.action.dependencies,targets);
    assert_eq!(polar.distance_multiplier,2.);
    assert_eq!(polar.angle_offset,std::f64::consts::FRAC_PI_2);
    assert_eq!(app.tabs[i].history.undo_stack.len(),depth+1);
    let _=app.update(Message::BlockActionBarSelect(action));
    let _=app.update(Message::BlockActionBarEdit(action));
    feed(&mut app,"RotateOnly");
    let _=app.feed_command(crate::command::StepInput::Enter);
    assert!(app.tabs[i].active_cmd.is_none());
    let codec::objects::ObjectType::DynamicBlock(object)=&app.tabs[i].scene.document.objects[&action] else {panic!()};
    let codec::objects::DynamicBlockData::PolarStretchAction(polar)=&object.data else {panic!()};
    assert!(polar.handles.is_empty());
    assert_eq!(app.tabs[i].history.undo_stack.len(),depth+2);
    app.undo_steps(1);
    assert_eq!(app.tabs[i].scene.document.objects[&action],native);
    app.undo_steps(1);
    assert!(actions::actions(&app.tabs[i].scene.document,definition).is_empty());
    app.redo_steps(2);
    assert!(actions::options::offsets(&app.tabs[i].scene.document,action).is_some());
}

#[test]
fn block_action_base_properties_edit_native_pivot_with_local_undo() {
    use crate::scene::block_actions::{self as actions, Kind};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER LINEAR 2,3 12,3 7,5");
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let parameter = actions::all_parameters(&app.tabs[i].scene.document, definition)[0].handle;
    let targets = members(&app);
    let action = actions::create(&mut app.tabs[i].scene.document, definition, Kind::Scale, parameter, &targets, glam::DVec3::ZERO).unwrap();
    let _ = app.update(Message::BlockActionBarSelect(action));
    let initial = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let _ = app.update(Message::PropBoolToggle("authoring_action_base_dependent"));
    let base = actions::options::base(&app.tabs[i].scene.document, action).unwrap();
    assert!(!base.dependent);
    assert_eq!(base.base_point, codec::types::Vector3::new(2., 3., 0.));
    assert!(app.tabs[i].properties.sections.iter().flat_map(|s| &s.props).any(|p| p.field == "authoring_action_base_x"));
    let _ = app.update(Message::PropGeomInput { field: "authoring_action_base_x", value: "9/2".into() });
    let _ = app.update(Message::PropGeomCommit("authoring_action_base_x"));
    assert_eq!(actions::options::base(&app.tabs[i].scene.document, action).unwrap().base_point.x, 4.5);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 2);
    let changed = app.tabs[i].scene.document.clone();
    for input in ["NaN", "inf", "unknown_name"] {
        let _ = app.update(Message::PropGeomInput { field: "authoring_action_base_y", value: input.into() });
        let _ = app.update(Message::PropGeomCommit("authoring_action_base_y"));
        assert_eq!(app.tabs[i].scene.document, changed);
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 2);
    }
    app.undo_steps(2);
    assert_eq!(app.tabs[i].scene.document.objects[&action], initial.objects[&action]);
    app.redo_steps(2);
    assert_eq!(app.tabs[i].scene.document.objects[&action], changed.objects[&action]);
    let _ = app.dispatch_command("BEDIT_DISCARD");
    let before = app.tabs[i].scene.document.clone();
    let _ = app.update(Message::PropBoolToggle("authoring_action_base_dependent"));
    let _ = app.update(Message::PropGeomCommit("authoring_action_base_x"));
    assert_eq!(app.tabs[i].scene.document, before);
}

#[test]
fn block_action_base_offset_properties_refresh_references_and_keep_history() {
    use crate::scene::block_actions::{self as actions, Kind};
    use glam::DVec3;
    for kind in [Kind::Scale, Kind::Rotate] {
        let (mut app, reference) = fixture();
        begin(&mut app, reference);
        let i = app.active_tab;
        let _ = app.run_command_line(if kind == Kind::Scale { "BPARAMETER LINEAR 0,0 10,0 5,2" } else { "BPARAMETER ROTATION 0,0 10 0" });
        let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
        let parameter = actions::all_parameters(&app.tabs[i].scene.document, definition)[0].handle;
        let targets = members(&app);
        let action = actions::create(&mut app.tabs[i].scene.document, definition, kind, parameter, &targets, DVec3::new(8., 9., 0.)).unwrap();
        let _ = app.update(Message::BlockActionBarSelect(action));
        let initial = app.tabs[i].scene.document.clone();
        let depth = app.tabs[i].history.undo_stack.len();
        assert!(app.tabs[i].properties.sections.iter().flat_map(|s| &s.props).any(|p| p.field == "authoring_action_base_offset_x"));
        for (field, input) in [("authoring_action_base_offset_x", "6/2"), ("authoring_action_base_offset_y", "2")] {
            let _ = app.update(Message::PropGeomInput { field, value: input.into() });
            let _ = app.update(Message::PropGeomCommit(field));
        }
        let offset = Vector3::new(3., 2., 0.);
        assert_eq!(actions::options::base(&app.tabs[i].scene.document, action).unwrap().offset, offset);
        let changed = app.tabs[i].scene.document.clone();
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 2);
        for input in ["NaN", "inf", "unknown_name", "2"] {
            let _ = app.update(Message::PropGeomInput { field: "authoring_action_base_offset_y", value: input.into() });
            let _ = app.update(Message::PropGeomCommit("authoring_action_base_offset_y"));
            assert_eq!(app.tabs[i].scene.document, changed);
            assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 2);
        }
        app.undo_steps(2);
        assert_eq!(app.tabs[i].scene.document.objects[&action], initial.objects[&action]);
        app.redo_steps(2);
        assert_eq!(app.tabs[i].scene.document.objects[&action], changed.objects[&action]);
        let _ = app.update(Message::BlockActionBarSelect(action));
        let _ = app.update(Message::PropBoolToggle("authoring_action_base_dependent"));
        let base = actions::options::base(&app.tabs[i].scene.document, action).unwrap();
        assert_eq!(base.base_point, offset);
        assert_eq!(base.offset, offset);
        let independent = app.tabs[i].scene.document.clone();
        let _ = app.update(Message::PropGeomInput { field: "authoring_action_base_offset_x", value: "20".into() });
        let _ = app.update(Message::PropGeomCommit("authoring_action_base_offset_x"));
        assert_eq!(app.tabs[i].scene.document, independent);
        app.undo_steps(1);
        let _ = app.dispatch_command("BSAVE"); let _ = app.dispatch_command("BCLOSE");
        let desired = if kind == Kind::Scale { DVec3::new(20., 0., 0.) } else { DVec3::new(0., 10., 0.) };
        let staged = actions::instance::evaluate(&app.tabs[i].scene.document, reference, parameter, desired).unwrap().unwrap();
        app.apply_action_document(i, staged);
        begin(&mut app, reference);
        let _ = app.update(Message::BlockActionBarSelect(action));
        // Editing the offset also rebuilds the previously evaluated reference.
        let _ = app.update(Message::PropGeomInput { field: "authoring_action_base_offset_x", value: "4".into() });
        let _ = app.update(Message::PropGeomCommit("authoring_action_base_offset_x"));
        let doc = &app.tabs[i].scene.document;
        let EntityType::Insert(native) = doc.get_entity(reference).unwrap() else { panic!() };
        let line = doc.block_records.get(&native.block_name).unwrap().entity_handles.iter().find_map(|h| match doc.get_entity(*h) { Some(EntityType::Line(line)) => Some(line), _ => None }).unwrap();
        let expected = if kind == Kind::Scale { Vector3::new(-4., -2., 0.) } else { Vector3::new(6., -2., 0.) };
        assert!(line.start.distance(&expected) < 1e-8, "{kind:?}: {:?}", line.start);
        let _ = app.dispatch_command("BEDIT_DISCARD");
        let before = app.tabs[i].scene.document.clone();
        let _ = app.update(Message::PropGeomCommit("authoring_action_base_offset_x"));
        assert_eq!(app.tabs[i].scene.document, before);
    }
}

#[test]
fn block_polar_stretch_end_binding_command_scratch_and_reference_history() {
    use crate::scene::block_actions::{self as actions, Kind};
    use crate::command::{CmdResult, StepInput};
    use codec::objects::{DynamicBlockData, ObjectType};
    use glam::DVec3;
    for testing in [false, true] {
        let (mut app, reference) = fixture();
        begin(&mut app, reference);
        let source = app.active_tab;
        let _ = app.dispatch_command("BACTIONBARMODE 0");
        let _ = app.run_command_line("BPARAMETER POLAR 0,0 10,0 5,2");
        let definition = app.tabs[source].active_block_edit_session().unwrap().br_handle;
        let parameter = actions::all_parameters(&app.tabs[source].scene.document, definition).remove(0);
        let targets = members(&app);
        let initial = app.tabs[source].scene.document.clone();
        let depth = app.tabs[source].history.undo_stack.len();
        let _ = app.dispatch_command("BACTION POLARSTRETCH");
        feed(&mut app, &parameter.name); feed(&mut app, "End");
        assert!(app.tabs[source].active_cmd.as_ref().unwrap().prompt().contains("<End>"));
        let _ = app.update(Message::CommandEscape);
        assert_eq!(app.tabs[source].scene.document, initial);
        assert_eq!(app.tabs[source].history.undo_stack.len(), depth);
        let _ = app.dispatch_command("BACTION POLARSTRETCH");
        feed(&mut app, &parameter.name);
        assert!(app.tabs[source].active_cmd.as_ref().unwrap().options().iter().any(|option| option.keyword == "End"));
        feed(&mut app, "Base"); feed(&mut app, "End");
        feed(&mut app, "0.5,-0.5"); feed(&mut app, "1.5,0.5");
        let _ = app.feed_command(StepInput::SelectionComplete(targets));
        let _ = app.feed_command(StepInput::Enter);
        let _ = app.feed_command(StepInput::Enter);
        assert_eq!(app.tabs[source].scene.document, initial);
        feed(&mut app, "4,4");
        assert!(app.tabs[source].active_cmd.is_none());
        let action = actions::actions(&app.tabs[source].scene.document, definition).into_iter().find(|(_,_,kind,_)| *kind == Kind::PolarStretch).unwrap().0;
        let native = app.tabs[source].scene.document.objects[&action].clone();
        let ObjectType::DynamicBlock(object) = &native else { panic!() };
        let DynamicBlockData::PolarStretchAction(polar) = &object.data else { panic!() };
        assert_eq!(polar.connections[0].name, "EndXDelta");
        assert_eq!(polar.connections[1].name, "EndYDelta");
        assert_eq!(app.tabs[source].history.undo_stack.len(), depth + 1);
        app.undo_steps(1);
        assert!(actions::actions(&app.tabs[source].scene.document, definition).is_empty());
        app.redo_steps(1);
        assert_eq!(app.tabs[source].scene.document.objects[&action], native);
        let ObjectType::DynamicBlock(object) = app.tabs[source].scene.document.objects.get_mut(&action).unwrap() else { panic!() };
        let DynamicBlockData::PolarStretchAction(polar) = &mut object.data else { panic!() };
        for connection in &mut polar.connections { connection.code = -1; }
        let orphan = app.tabs[source].scene.document.objects[&action].clone();
        app.tabs[source].selected_authoring = Some(action);
        let _ = app.dispatch_command("BASSOCIATE");
        feed(&mut app, &parameter.name);
        assert!(app.tabs[source].active_cmd.as_ref().unwrap().prompt().contains("Base/End"));
        feed(&mut app, "End");
        assert!(app.tabs[source].active_cmd.is_none());
        assert_eq!(app.tabs[source].scene.document.objects[&action], native);
        app.undo_steps(1);
        assert_eq!(app.tabs[source].scene.document.objects[&action], orphan);
        app.redo_steps(1);
        assert_eq!(app.tabs[source].scene.document.objects[&action], native);
        if testing { let _ = app.open_block_test(source); }
        else { let _ = app.dispatch_command("BSAVE"); let _ = app.dispatch_command("BCLOSE"); }
        let source_before = app.tabs[source].scene.document.clone();
        let i = app.active_tab;
        let insert = app.tabs[i].block_test.as_ref().map_or(reference, |test| test.insert);
        app.tabs[i].scene.select_entity(insert, true); app.tabs[i].selected_handle = Some(insert);
        let before = app.tabs[i].scene.document.clone();
        let depth = app.tabs[i].history.undo_stack.len();
        let document = app.tabs[i].id;
        let _ = app.handle_block_action_result(CmdResult::EvaluateBlockParameter { grip: None,
            insert: (!testing).then_some(insert), document, definition, parameter: parameter.handle,
            value: actions::Value::Point(DVec3::new(0., 15., 0.)),
        });
        let geometry = |doc: &codec::Document| {
            let EntityType::Insert(reference) = doc.get_entity(insert).unwrap() else { panic!() };
            let line = doc.block_records.get(&reference.block_name).unwrap().entity_handles.iter().find_map(|h| match doc.get_entity(*h) { Some(EntityType::Line(line)) => Some(line), _ => None }).unwrap();
            (line.start, line.end)
        };
        let expected = (Vector3::ZERO, Vector3::new(0., 6., 0.));
        let changed = geometry(&app.tabs[i].scene.document);
        assert!(changed.0.distance(&expected.0) < 1e-8);
        assert!(changed.1.distance(&expected.1) < 1e-8);
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
        assert_eq!(app.tabs[i].scene.document.objects[&action], native);
        app.undo_steps(1);
        assert_eq!(geometry(&app.tabs[i].scene.document), geometry(&before));
        app.redo_steps(1);
        assert_eq!(geometry(&app.tabs[i].scene.document), changed);
        if testing { assert_eq!(app.tabs[source].scene.document, source_before); }
    }
}

#[test]
fn block_action_self_grip_selection_edits_keep_native_links_and_reference_history() {
    use crate::scene::{block_actions as actions, block_grips::config};
    use codec::objects::{DynamicBlockData, ObjectType};
    use glam::DVec3;
    for testing in [false, true] {
        let (mut app, reference) = fixture();
        begin(&mut app, reference);
        let source = app.active_tab;
        circle(&mut app, 3.);
        let _ = app.dispatch_command("BACTIONBARMODE 0");
        let _ = app.run_command_line("BPARAMETER POLAR 0,0 10,0 5,2");
        let definition = app.tabs[source].active_block_edit_session().unwrap().br_handle;
        let parameter = actions::all_parameters(&app.tabs[source].scene.document, definition).remove(0);
        let targets = members(&app);
        let action = actions::create_with_frame(&mut app.tabs[source].scene.document, definition, actions::Kind::PolarStretch,
            parameter.handle, &targets[..1], DVec3::new(4., 4., 0.), Some([DVec3::new(0.5, -1., 0.), DVec3::new(1.5, 1., 0.)])).unwrap();
        let grip = config::locations(&app.tabs[source].scene.document, definition, parameter.handle).unwrap().into_iter().find(|grip| grip.role == 1).unwrap().handle;
        let ObjectType::DynamicBlock(object) = app.tabs[source].scene.document.objects.get_mut(&action).unwrap() else { panic!() };
        let DynamicBlockData::PolarStretchAction(native) = &mut object.data else { panic!() };
        native.action.dependencies.insert(0, grip);
        let grip_before = app.tabs[source].scene.document.objects[&grip].clone();
        let original = app.tabs[source].scene.document.objects[&action].clone();
        edit_action_set(&mut app, action, "Add", targets[1..].to_vec());
        edit_action_set(&mut app, action, "Remove", targets[..1].to_vec());
        let deps = |app: &OpenCADStudio| actions::actions(&app.tabs[source].scene.document, definition).into_iter().find(|(h, ..)| *h == action).unwrap().1.dependencies.clone();
        assert_eq!(deps(&app), vec![grip, targets[1]]);
        app.undo_steps(2);
        assert_eq!(app.tabs[source].scene.document.objects[&action], original);
        app.redo_steps(2);
        app.tabs[source].selected_authoring = Some(action);
        let _ = app.dispatch_command("BACTIONSET"); feed(&mut app, "Frame"); feed(&mut app, "2,-1"); feed(&mut app, "4,1");
        assert!(app.tabs[source].active_cmd.is_none());
        edit_action_set(&mut app, action, "RotateOnly", targets[1..].to_vec());
        assert_eq!(deps(&app), vec![grip, targets[1]]);
        assert_eq!(app.tabs[source].scene.document.objects[&grip], grip_before);
        if testing { let _ = app.open_block_test(source); }
        else { let _ = app.dispatch_command("BSAVE"); let _ = app.dispatch_command("BCLOSE"); }
        let source_before = app.tabs[source].scene.document.clone();
        let i = app.active_tab;
        let insert = app.tabs[i].block_test.as_ref().map_or(reference, |test| test.insert);
        let document = app.tabs[i].id;
        let before = app.tabs[i].scene.document.clone();
        let _ = app.handle_block_action_result(crate::command::CmdResult::EvaluateBlockParameter { grip: None,
            insert: (!testing).then_some(insert), document, definition, parameter: parameter.handle, value: actions::Value::Point(DVec3::new(0., 15., 0.)),
        });
        let circle_center = |doc: &codec::Document| {
            let EntityType::Insert(reference) = doc.get_entity(insert).unwrap() else { panic!() };
            doc.block_records.get(&reference.block_name).unwrap().entity_handles.iter().find_map(|h| match doc.get_entity(*h) {
                Some(EntityType::Circle(circle)) => Some(circle.center), _ => None,
            }).unwrap()
        };
        assert!(circle_center(&app.tabs[i].scene.document).distance(&Vector3::new(0., 3., 0.)) < 1e-8);
        assert_eq!(app.tabs[i].scene.document.objects[&grip], grip_before);
        app.tabs[i].scene.select_entity(insert, true); app.tabs[i].selected_handle = Some(insert);
        app.tabs[i].selected_grips.clear(); app.tabs[i].selected_grip_handles.clear(); app.refresh_test_action_grips([0.; 3]);
        let id = crate::scene::block_grips::runtime::id(parameter.id, 1).unwrap();
        assert!(app.tabs[i].selected_grips.iter().find(|grip| grip.id == id).unwrap().world.distance(DVec3::new(0., 15., 0.)) < 1e-8);
        app.undo_steps(1);
        assert_eq!(circle_center(&app.tabs[i].scene.document), circle_center(&before));
        app.redo_steps(1);
        assert!(circle_center(&app.tabs[i].scene.document).distance(&Vector3::new(0., 3., 0.)) < 1e-8);
        if testing { assert_eq!(app.tabs[source].scene.document, source_before); }
    }
}

#[test]
fn block_grip_count_command_and_properties_are_atomic_and_undoable() {
    use crate::scene::block_grips::config;
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let _ = app.run_command_line("BPARAMETER XY 0,0 10,10 5,5");
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let parameter = crate::scene::block_actions::all_parameters(&app.tabs[i].scene.document, definition)[0].handle;
    app.tabs[i].selected_authoring = Some(parameter);
    app.refresh_properties();
    assert!(app.tabs[i].properties.sections.iter().flat_map(|s| &s.props).any(|p| p.field == "authoring_grip_count"));
    let initial = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let _ = app.run_command_line("BGRIPSET 4");
    assert_eq!(config::count(&app.tabs[i].scene.document, definition, parameter), Some(4));
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    let changed = app.tabs[i].scene.document.clone();
    let _ = app.update(Message::PropGeomInput { field: "authoring_grip_count", value: "3".into() });
    let _ = app.update(Message::PropGeomCommit("authoring_grip_count"));
    assert_eq!(app.tabs[i].scene.document, changed);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    let _ = app.update(Message::PropGeomInput { field: "authoring_grip_count", value: "0".into() });
    let _ = app.update(Message::PropGeomCommit("authoring_grip_count"));
    assert_eq!(config::count(&app.tabs[i].scene.document, definition, parameter), Some(0));
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 2);
    app.undo_steps(2);
    assert_eq!(app.tabs[i].scene.document.objects, initial.objects);
    app.redo_steps(2);
    assert_eq!(config::count(&app.tabs[i].scene.document, definition, parameter), Some(0));
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
    let parameter = crate::scene::block_actions::all_parameters(&app.tabs[i].scene.document, definition)[0].handle;
    let mut command = crate::modules::block_editor::grip_set::GripSetCommand::new(app.tabs[i].id, definition, &app.tabs[i].scene.document, Some(parameter)).unwrap();
    let result = command.on_text_input("2").unwrap();
    crate::scene::block_grips::config::set(&mut app.tabs[i].scene.document, definition, parameter, Some(0)).unwrap();
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
    let parameter = crate::scene::block_visibility::parameter(&app.tabs[i].scene.document, definition).unwrap().handle;
    for desired in [0, 1] {
        app.tabs[i].selected_authoring = Some(parameter);
        let _ = app.run_command_line(&format!("BGRIPSET {desired}"));
        assert_eq!(crate::scene::block_grips::config::count(&app.tabs[i].scene.document, definition, parameter), Some(desired));
        let _ = app.dispatch_command("BSAVE");
        let _ = app.dispatch_command("BCLOSE");
        assert!(app.tabs[i].active_block_edit_session().is_none());
        app.tabs[i].selected_handle = Some(reference);
        app.tabs[i].selected_grips.clear();
        app.tabs[i].selected_grip_handles.clear();
        app.refresh_visibility_grip([0.;3]);
        assert_eq!(app.tabs[i].visibility_grip.is_some(), desired > 0);
        if desired == 0 { begin(&mut app, reference); }
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
        let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
        let definition = app.tabs[source].active_block_edit_session().unwrap().br_handle;
        let parameter = actions::all_parameters(&app.tabs[source].scene.document, definition)[0].handle;
        let targets = members(&app);
        actions::create(&mut app.tabs[source].scene.document, definition, actions::Kind::Move, parameter, &targets, DVec3::ZERO).unwrap();
        app.tabs[source].selected_authoring = Some(parameter);
        app.refresh_properties();
        let parameter_before = app.tabs[source].scene.document.objects[&parameter].clone();
        let depth = app.tabs[source].history.undo_stack.len();
        for (field, value) in [("authoring_display_grip_end_x", "20+5"), ("authoring_display_grip_end_y", "7")] {
            let _ = app.update(Message::PropGeomInput { field, value: value.into() });
            let _ = app.update(Message::PropGeomCommit(field));
        }
        assert_eq!(app.tabs[source].scene.document.objects[&parameter], parameter_before);
        assert_eq!(config::locations(&app.tabs[source].scene.document, definition, parameter).unwrap()[0].point, Vector3::new(25., 7., 0.));
        assert_eq!(app.tabs[source].history.undo_stack.len(), depth + 2);
        let before = app.tabs[source].scene.document.clone();
        let _ = app.update(Message::PropGeomInput { field: "authoring_display_grip_end_y", value: "NaN".into() });
        let _ = app.update(Message::PropGeomCommit("authoring_display_grip_end_y"));
        assert_eq!(app.tabs[source].scene.document, before);
        app.undo_steps(2);
        assert_eq!(config::locations(&app.tabs[source].scene.document, definition, parameter).unwrap()[0].point, Vector3::new(10., 0., 0.));
        app.redo_steps(2);
        if testing { let _ = app.open_block_test(source); }
        else { let _ = app.dispatch_command("BSAVE"); let _ = app.dispatch_command("BCLOSE"); }
        let i = app.active_tab;
        let insert = app.tabs[i].block_test.as_ref().map(|test| test.insert).unwrap_or(reference);
        let EntityType::Insert(native) = app.tabs[i].scene.document.get_entity_mut(insert).unwrap() else { panic!() };
        native.insert_point = Vector3::new(100., 200., 0.);
        native.rotation = std::f64::consts::FRAC_PI_2;
        native.set_x_scale(2.); native.set_y_scale(2.); native.set_z_scale(2.);
        app.tabs[i].scene.select_entity(insert, true);
        app.tabs[i].selected_handle = Some(insert);
        app.tabs[i].selected_grips.clear(); app.tabs[i].selected_grip_handles.clear();
        app.refresh_test_action_grips([0.; 3]);
        let id = crate::scene::block_grips::runtime::id(actions::all_parameters(&app.tabs[i].scene.document, definition)[0].id, 1).unwrap();
        let displayed = app.tabs[i].selected_grips.iter().find(|grip| grip.id == id).unwrap().world;
        assert!(displayed.distance(DVec3::new(86., 250., 0.)) < 1e-8);
        let grip = config::locations(&app.tabs[i].scene.document, definition, parameter).unwrap()[0].handle;
        let grip_before = app.tabs[i].scene.document.objects[&grip].clone();
        assert!(app.start_test_action_grip(insert, id));
        let before = app.tabs[i].scene.document.clone();
        let _ = app.update(Message::CommandEscape);
        assert_eq!(app.tabs[i].scene.document, before);
        app.tabs[i].scene.select_entity(insert, true);
        app.tabs[i].selected_handle = Some(insert);
        assert!(app.start_test_action_grip(insert, id));
        let result = app.tabs[i].active_cmd.as_mut().unwrap().on_point(DVec3::new(86., 260., 0.));
        let _ = app.handle_block_action_result(result);
        let p = actions::all_parameters(&app.tabs[i].scene.document, definition).remove(0);
        let value = if testing { actions::value(&app.tabs[i].scene.document, &p) } else { actions::instance::value(&app.tabs[i].scene.document, insert, &p) }.unwrap();
        assert!(matches!(value, actions::Value::Point(point) if point.distance(DVec3::new(15.,0.,0.)) < 1e-8));
        assert_eq!(app.tabs[i].scene.document.objects[&grip], grip_before);
        let EntityType::Insert(native) = app.tabs[i].scene.document.get_entity(insert).unwrap() else { panic!() };
        let line = app.tabs[i].scene.document.block_records.get(&native.block_name).unwrap().entity_handles.iter().find_map(|handle| match app.tabs[i].scene.document.get_entity(*handle) { Some(EntityType::Line(line)) => Some(line), _ => None }).unwrap();
        assert!(line.start.distance(&Vector3::new(5., 0., 0.)) < 1e-8);
    }
}

#[test]
fn block_runtime_native_roles_drag_cancel_undo_redo_and_preserve_relocated_grips() {
    use crate::scene::{block_actions as actions, block_grips::{config, runtime}};
    use actions::{instance::ParameterFrame, XyKeyPoint};
    use glam::DVec3;
    for testing in [false, true] {
        for (family, role, desired, expected) in [
            ("POINT", 0, DVec3::new(3., 5., 0.), DVec3::new(3., 5., 0.)),
            ("LINEAR", 0, DVec3::new(-2., 4., 0.), DVec3::new(-2., 0., 0.)),
            ("LINEAR", 1, DVec3::new(13., 4., 0.), DVec3::new(3., 0., 0.)),
            ("POLAR", 0, DVec3::new(-2., -3., 0.), DVec3::new(-2., -3., 0.)),
            ("POLAR", 1, DVec3::new(13., 15., 0.), DVec3::new(3., 15., 0.)),
            ("XY", 0, DVec3::new(-2., -3., 0.), DVec3::new(-2., -3., 0.)),
            ("XY", 1, DVec3::new(13., 15., 0.), DVec3::new(3., 5., 0.)),
            ("XY", 2, DVec3::new(13., -3., 0.), DVec3::new(3., -3., 0.)),
            ("XY", 3, DVec3::new(-2., 15., 0.), DVec3::new(-2., 5., 0.)),
        ] {
            let (mut app, reference) = fixture();
            begin(&mut app, reference);
            let source = app.active_tab;
            let command = if family == "POINT" { "BPARAMETER POINT 0,0 5,2".to_owned() }
                else { format!("BPARAMETER {family} 0,0 {} 5,2", if family == "XY" { "10,10" } else { "10,0" }) };
            let _ = app.run_command_line(&command);
            let definition = app.tabs[source].active_block_edit_session().unwrap().br_handle;
            let p = actions::all_parameters(&app.tabs[source].scene.document, definition).remove(0);
            let key = [XyKeyPoint::Base, XyKeyPoint::End, XyKeyPoint::X, XyKeyPoint::Y][role];
            let targets = members(&app);
            actions::create_with_options(&mut app.tabs[source].scene.document, definition, actions::Kind::Move,
                p.handle, (family != "POINT").then_some(key), &targets, DVec3::ZERO, None, None).unwrap();
            let maximum = *config::choices(&app.tabs[source].scene.document, definition, p.handle).last().unwrap();
            config::set(&mut app.tabs[source].scene.document, definition, p.handle, Some(maximum)).unwrap();
            let stable_ids = runtime::grips(&app.tabs[source].scene.document, definition, &p, actions::Value::Point(p.end)).into_iter().map(|grip| grip.id).collect::<Vec<_>>();
            config::set(&mut app.tabs[source].scene.document, definition, p.handle, Some(0)).unwrap();
            assert!(runtime::grips(&app.tabs[source].scene.document, definition, &p, actions::Value::Point(p.end)).is_empty());
            config::set(&mut app.tabs[source].scene.document, definition, p.handle, Some(maximum)).unwrap();
            assert_eq!(runtime::grips(&app.tabs[source].scene.document, definition, &p, actions::Value::Point(p.end)).into_iter().map(|grip| grip.id).collect::<Vec<_>>(), stable_ids);
            let offset = Vector3::new(15., 7., 0.);
            for grip in config::locations(&app.tabs[source].scene.document, definition, p.handle).unwrap() {
                config::set_location(&mut app.tabs[source].scene.document, definition, p.handle, grip.role, grip.point + offset).unwrap();
            }
            if testing { let _ = app.open_block_test(source); }
            else { let _ = app.dispatch_command("BSAVE"); let _ = app.dispatch_command("BCLOSE"); }
            let i = app.active_tab;
            let insert = app.tabs[i].block_test.as_ref().map(|test| test.insert).unwrap_or(reference);
            let EntityType::Insert(native) = app.tabs[i].scene.document.get_entity_mut(insert).unwrap() else { panic!() };
            native.insert_point = Vector3::new(100., 200., 30.);
            native.normal = Vector3::new(0., std::f64::consts::FRAC_1_SQRT_2, std::f64::consts::FRAC_1_SQRT_2);
            native.rotation = std::f64::consts::FRAC_PI_2;
            native.set_x_scale(2.); native.set_y_scale(2.); native.set_z_scale(2.);
            let to_world = crate::scene::render_graph::insert_transform(&app.tabs[i].scene.document,
                match app.tabs[i].scene.document.get_entity(insert).unwrap() { EntityType::Insert(ins) => ins, _ => panic!() });
            app.tabs[i].scene.selection.borrow_mut().view.vp_size = (1000., 800.);
            app.tabs[i].scene.camera.borrow_mut().fit_to_bounds_f64(DVec3::new(40., 180., 0.), DVec3::new(120., 280., 0.), 1.25);
            let click = |app: &mut OpenCADStudio, point: DVec3| {
                let scene = &app.tabs[i].scene;
                let tile = scene.active_model_tile_bounds(1000., 800.);
                let at = scene.camera.borrow().project(point, iced::Rectangle::with_size(iced::Size::new(tile.width, tile.height))).unwrap();
                let point = iced::Point::new(at.x + tile.x, at.y + tile.y);
                let _ = app.update(Message::ViewportLeftPressAt(point));
                let _ = app.update(Message::ViewportLeftRelease);
            };
            let select = |app: &mut OpenCADStudio| {
                app.tabs[i].scene.select_entity(insert, true);
                app.tabs[i].selected_handle = Some(insert);
                app.tabs[i].selected_grips.clear(); app.tabs[i].selected_grip_handles.clear();
                app.refresh_test_action_grips([0.; 3]);
            };
            select(&mut app);
            let id = runtime::id(p.id, role).unwrap();
            assert_eq!(app.tabs[i].selected_grips.iter().filter(|grip| super::block_actions::is_test_grip(grip.id)).count(), if family == "XY" { 3 } else { 1 }, "grips sharing action input coordinates are eligible");
            assert!(app.tabs[i].selected_grips.iter().any(|grip| grip.id == id));
            let before = app.tabs[i].scene.document.clone();
            let displayed = app.tabs[i].selected_grips.iter().find(|grip| grip.id == id).unwrap().world;
            click(&mut app, displayed);
            assert_eq!(app.tabs[i].active_cmd.as_ref().and_then(|command| command.block_parameter_grip_id()), Some(id));
            let _ = app.update(Message::CommandEscape);
            assert_eq!(app.tabs[i].scene.document, before);
            select(&mut app);
            let native_grips = config::locations(&app.tabs[i].scene.document, definition, p.handle).unwrap().into_iter()
                .map(|grip| (grip.handle, app.tabs[i].scene.document.objects[&grip.handle].clone())).collect::<Vec<_>>();
            click(&mut app, displayed);
            assert_eq!(app.tabs[i].active_cmd.as_ref().and_then(|command| command.block_parameter_grip_id()), Some(id));
            let world = actions::v(to_world.apply(actions::native(desired) + offset));
            click(&mut app, world);
            let frame = actions::instance::frame(&app.tabs[i].scene.document, insert, &p).unwrap();
            let changed = app.tabs[i].scene.document.clone();
            let check = |app: &OpenCADStudio, delta: DVec3| {
                let EntityType::Insert(reference) = app.tabs[i].scene.document.get_entity(insert).unwrap() else { panic!() };
                let line = app.tabs[i].scene.document.block_records.get(&reference.block_name).unwrap().entity_handles.iter()
                    .find_map(|handle| match app.tabs[i].scene.document.get_entity(*handle) { Some(EntityType::Line(line)) => Some(line), _ => None }).unwrap();
                assert!(actions::v(line.start).distance(delta) < 1e-4, "{testing} {family} role{role}: {:?} != {delta:?}", line.start);
            };
            check(&app, expected);
            for (handle, native) in &native_grips { assert_eq!(&changed.objects[handle], native); }
            app.undo_steps(1);
            check(&app, DVec3::ZERO);
            assert_eq!(actions::instance::frame(&app.tabs[i].scene.document, insert, &p).unwrap(), ParameterFrame { base: p.base, end: p.end });
            assert_eq!(app.tabs[i].scene.document.objects.len(), before.objects.len());
            app.redo_steps(1);
            check(&app, expected);
            assert_eq!(actions::instance::frame(&app.tabs[i].scene.document, insert, &p).unwrap(), frame);
            select(&mut app);
            assert!(app.tabs[i].selected_grips.iter().any(|grip| grip.id == id));
        }
    }
}

#[test]
fn block_alignment_type_placement_properties_history_and_native_roundtrips() {
    use crate::scene::block_orientation_parameter::{self as orientation, Kind};
    use codec::objects::{DynamicBlockData, ObjectType};
    use std::io::Cursor;
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let _ = app.run_command_line("BPARAMETER ALIGNMENT 0,0 TYPE P 10,0");
    let parameter = orientation::parameters(&app.tabs[i].scene.document, definition).into_iter()
        .find(|p| p.kind == Kind::Alignment).unwrap().handle;
    let perpendicular = |doc: &codec::Document| match &doc.objects[&parameter] {
        ObjectType::DynamicBlock(object) => match &object.data {
            DynamicBlockData::AlignmentParameter(p) => p.align_perpendicular,
            _ => panic!(),
        }, _ => panic!(),
    };
    assert!(perpendicular(&app.tabs[i].scene.document));
    app.tabs[i].selected_authoring = Some(parameter);
    app.refresh_properties();
    assert!(app.tabs[i].properties.sections.iter().flat_map(|section| &section.props)
        .any(|property| property.field == "authoring_alignment_type"));
    let original = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let _ = app.update(Message::PropGeomChoiceChanged { field: "authoring_alignment_type", value: "Tangent".into() });
    assert!(!perpendicular(&app.tabs[i].scene.document));
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    for (handle, object) in &original.objects {
        if *handle != parameter { assert_eq!(app.tabs[i].scene.document.objects.get(handle), Some(object)); }
    }
    assert_eq!(app.tabs[i].scene.document.entities().cloned().collect::<Vec<_>>(), original.entities().cloned().collect::<Vec<_>>());
    app.undo_steps(1);
    assert!(perpendicular(&app.tabs[i].scene.document));
    app.redo_steps(1);
    assert!(!perpendicular(&app.tabs[i].scene.document));
    app.tabs[i].selected_authoring = Some(parameter);
    let unchanged = app.tabs[i].scene.document.clone();
    let _ = app.update(Message::PropGeomChoiceChanged { field: "authoring_alignment_type", value: "Tangent".into() });
    let _ = app.update(Message::PropGeomChoiceChanged { field: "authoring_alignment_type", value: "Diagonal".into() });
    assert_eq!(app.tabs[i].scene.document, unchanged);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    let _ = app.update(Message::PropGeomChoiceChanged { field: "authoring_alignment_type", value: "Perpendicular".into() });
    let doc = &app.tabs[i].scene.document;
    for saved in [
        codec::DwgReader::from_stream(Cursor::new(codec::DwgWriter::write_to_vec(doc).unwrap())).read().unwrap(),
        codec::DxfReader::from_reader(Cursor::new(codec::DxfWriter::new(doc).write_to_vec().unwrap())).unwrap().read().unwrap(),
    ] { assert!(perpendicular(&saved)); }
    let before = app.tabs[i].scene.document.clone();
    assert!(orientation::set_alignment_type(&mut app.tabs[i].scene.document, Handle::NULL, parameter, false).is_err());
    assert_eq!(app.tabs[i].scene.document, before);
    let _ = app.run_command_line("BPARAMETER ALIGNMENT 2,2 TYPE");
    let _ = app.update(Message::CommandEscape);
    assert_eq!(app.tabs[i].scene.document, before);
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");
    assert!(perpendicular(&app.tabs[i].scene.document));
    let before = app.tabs[i].scene.document.clone();
    let _ = app.update(Message::PropGeomChoiceChanged { field: "authoring_alignment_type", value: "Tangent".into() });
    assert_eq!(app.tabs[i].scene.document, before);
}

#[test]
fn block_alignment_runtime_grip_events_preview_history_and_native_roundtrips() {
    use crate::scene::block_grips::config;
    use glam::DVec3;
    use std::io::Cursor;
    for testing in [false, true] {
        for perpendicular in [false, true] {
            let (mut app, reference) = fixture();
            begin(&mut app, reference);
            let source = app.active_tab;
            let _ = app.run_command_line(if perpendicular { "BPARAMETER ALIGNMENT 0,0 TYPE P 10,0" } else { "BPARAMETER ALIGNMENT 0,0 10,0" });
            let definition = app.tabs[source].active_block_edit_session().unwrap().br_handle;
            let parameter = crate::scene::block_orientation_parameter::parameters(&app.tabs[source].scene.document, definition)[0].handle;
            config::set_location(&mut app.tabs[source].scene.document, definition, parameter, 0, Vector3::new(5., 7., 0.)).unwrap();
            if testing { let _ = app.open_block_test(source); }
            else { let _ = app.dispatch_command("BSAVE"); let _ = app.dispatch_command("BCLOSE"); }
            let source_before = app.tabs[source].scene.document.clone();
            let i = app.active_tab;
            let insert = app.tabs[i].block_test.as_ref().map(|test| test.insert).unwrap_or(reference);
            let other = app.tabs[i].scene.document.add_entity(EntityType::Insert(Insert::new("EDIT_ME", Vector3::new(-30., -30., 0.)))).unwrap();
            app.tabs[i].scene.document.add_entity(EntityType::Line(Line::from_points(Vector3::new(30., -20., 0.), Vector3::new(30., 20., 0.)))).unwrap();
            app.tabs[i].scene.bump_geometry();
            app.tabs[i].scene.selection.borrow_mut().view.vp_size = (1000., 800.);
            app.tabs[i].scene.camera.borrow_mut().fit_to_bounds_f64(DVec3::new(-20., -30., 0.), DVec3::new(50., 30., 0.), 1.25);
            // The destination is a point along the line, not one of its ends.
            app.snapper.enabled.insert(crate::snap::SnapType::Nearest);
            let select = |app: &mut OpenCADStudio| {
                app.tabs[i].scene.select_entity(insert, true);
                app.tabs[i].selected_handle = Some(insert);
                app.tabs[i].selected_grips.clear(); app.tabs[i].selected_grip_handles.clear();
                app.refresh_test_action_grips([0.; 3]);
            };
            let click = |app: &mut OpenCADStudio, point: DVec3| {
                let scene = &app.tabs[i].scene;
                let tile = scene.active_model_tile_bounds(1000., 800.);
                let at = scene.camera.borrow().project(point, iced::Rectangle::with_size(iced::Size::new(tile.width, tile.height))).unwrap();
                let _ = app.update(Message::ViewportLeftPressAt(iced::Point::new(at.x + tile.x, at.y + tile.y)));
                let _ = app.update(Message::ViewportLeftRelease);
            };
            select(&mut app);
            let alignment = app.tabs[i].scene.alignment_grips(insert).remove(0);
            assert!(app.tabs[i].selected_grips.iter().any(|grip| grip.id == alignment.id && grip.world.distance(alignment.anchor) < 1e-9));
            let before = app.tabs[i].scene.document.clone();
            let depth = app.tabs[i].history.undo_stack.len();
            click(&mut app, alignment.anchor);
            assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(), "BLOCKALIGNMENTGRIP");
            let destination = DVec3::new(30., 4., 0.);
            let preview = app.tabs[i].active_cmd.as_mut().unwrap().preview_entities(&before, destination);
            assert_eq!(preview.len(), 1);
            assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().preview_hidden_handles(), &[insert]);
            assert_eq!(app.tabs[i].scene.document, before);
            let _ = app.update(Message::CommandEscape);
            assert_eq!(app.tabs[i].scene.document, before);
            assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
            select(&mut app);
            click(&mut app, alignment.anchor);
            click(&mut app, destination);
            assert!(app.tabs[i].active_cmd.is_none());
            let aligned = app.tabs[i].scene.alignment_grips(insert).remove(0);
            assert!(aligned.anchor.distance(destination) < 1e-4, "{testing} {perpendicular}: {:?}", aligned.anchor);
            if perpendicular { assert!(aligned.direction.y.abs() < 1e-6); }
            else { assert!(aligned.direction.x.abs() < 1e-6); }
            assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
            assert_eq!(app.tabs[i].scene.document.get_entity(other), before.get_entity(other));
            assert_eq!(app.tabs[i].scene.document.objects, before.objects);
            let changed = app.tabs[i].scene.document.get_entity(insert).unwrap().clone();
            app.undo_steps(1);
            assert_eq!(app.tabs[i].scene.document.get_entity(insert), before.get_entity(insert));
            app.redo_steps(1);
            assert_eq!(app.tabs[i].scene.document.get_entity(insert), Some(&changed));
            let doc = &app.tabs[i].scene.document;
            for saved in [
                codec::DwgReader::from_stream(Cursor::new(codec::DwgWriter::write_to_vec(doc).unwrap())).read().unwrap(),
                codec::DxfReader::from_reader(Cursor::new(codec::DxfWriter::new(doc).write_to_vec().unwrap())).unwrap().read().unwrap(),
            ] {
                let restored = crate::scene::block_alignment::grips(&saved, definition, insert).remove(0);
                assert!(restored.anchor.distance(aligned.anchor) < 1e-8);
                assert!(restored.direction.distance(aligned.direction) < 1e-8);
                assert_eq!(restored.perpendicular, perpendicular);
            }
            if testing { assert_eq!(app.tabs[source].scene.document, source_before); }
        }
    }
}

#[test]
fn block_alignment_runtime_off_target_flip_preview_and_history() {
    use glam::DVec3;
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let _ = app.run_command_line("BPARAMETER ALIGNMENT 0,0 10,0");
    let definition = app.tabs[app.active_tab].active_block_edit_session().unwrap().br_handle;
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");
    let i = app.active_tab;
    app.tabs[i].scene.select_entity(reference, true);
    app.tabs[i].selected_handle = Some(reference);
    let alignment = app.tabs[i].scene.alignment_grips(reference).remove(0);
    let before = app.tabs[i].scene.document.clone();
    let depth = app.tabs[i].history.undo_stack.len();
    assert!(app.start_test_action_grip(reference, alignment.id));
    let cursor = DVec3::new(100., 80., 7.);
    let destination = DVec3::new(100., 80., 0.);
    let command = app.tabs[i].active_cmd.as_mut().unwrap();
    assert!(command.on_text_input("Flip").is_some());
    let preview = command.preview_entities(&before, cursor);
    assert_eq!(preview.len(), 1);
    let mut preview_doc = before.clone();
    *preview_doc.get_entity_mut(reference).unwrap() = preview[0].clone();
    let preview_grip = crate::scene::block_alignment::grips(&preview_doc, definition, reference).remove(0);
    assert!(preview_grip.anchor.distance(destination) < 1e-8);
    assert!(preview_grip.direction.distance(-alignment.direction) < 1e-8, "off-target Flip must reverse preview direction by PI");
    assert_eq!(app.tabs[i].scene.document, before);
    let result = app.tabs[i].active_cmd.as_mut().unwrap().on_point(cursor);
    let _ = app.handle_block_alignment(result);
    assert!(app.tabs[i].active_cmd.is_none());
    let committed = app.tabs[i].scene.alignment_grips(reference).remove(0);
    assert!(committed.anchor.distance(destination) < 1e-8);
    assert!(committed.direction.distance(preview_grip.direction) < 1e-8);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    for entity in before.entities() {
        if entity.common().handle != reference {
            assert_eq!(app.tabs[i].scene.document.get_entity(entity.common().handle), Some(entity));
        }
    }
    assert_eq!(app.tabs[i].scene.document.objects, before.objects);
    let changed = app.tabs[i].scene.document.get_entity(reference).unwrap().clone();
    app.undo_steps(1);
    assert_eq!(app.tabs[i].scene.document.get_entity(reference), before.get_entity(reference));
    app.redo_steps(1);
    assert_eq!(app.tabs[i].scene.document.get_entity(reference), Some(&changed));
}

#[test]
fn block_alignment_runtime_rejects_stale_sources_and_reevaluates_targets() {
    use glam::DVec3;
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let _ = app.run_command_line("BPARAMETER ALIGNMENT 0,0 10,0");
    let _ = app.dispatch_command("BSAVE"); let _ = app.dispatch_command("BCLOSE");
    let i = app.active_tab;
    app.tabs[i].scene.select_entity(reference, true); app.tabs[i].selected_handle = Some(reference);
    let target = app.tabs[i].scene.document.add_entity(EntityType::Line(Line::from_points(Vector3::new(30., -20., 0.), Vector3::new(30., 20., 0.)))).unwrap();
    let alignment = app.tabs[i].scene.alignment_grips(reference).remove(0);
    let document = app.tabs[i].id;
    let cursor = DVec3::new(30., 4., 0.);
    let depth = app.tabs[i].history.undo_stack.len();
    app.tabs[i].scene.document.layers.get_mut("0").unwrap().lock();
    assert!(app.start_test_action_grip(reference, alignment.id));
    assert!(app.tabs[i].active_cmd.is_none());
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    app.tabs[i].scene.document.layers.get_mut("0").unwrap().unlock();
    assert!(app.start_test_action_grip(reference, alignment.id));
    assert!(app.tabs[i].active_cmd.as_mut().unwrap().on_text_input("Flip").is_some());
    let result = app.tabs[i].active_cmd.as_mut().unwrap().on_point(cursor);
    let EntityType::Insert(native) = app.tabs[i].scene.document.get_entity_mut(reference).unwrap() else { panic!() };
    native.rotation = 0.2;
    let before = app.tabs[i].scene.document.clone();
    let _ = app.handle_block_alignment(result);
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    let current = app.tabs[i].scene.alignment_grips(reference).remove(0);
    assert!(app.start_test_action_grip(reference, current.id));
    let result = app.tabs[i].active_cmd.as_mut().unwrap().on_point(cursor);
    app.tabs[i].scene.document.layers.get_mut("0").unwrap().lock();
    let before = app.tabs[i].scene.document.clone();
    let _ = app.handle_block_alignment(result);
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    app.tabs[i].scene.document.layers.get_mut("0").unwrap().unlock();
    app.tabs[i].scene.select_entity(reference, true); app.tabs[i].selected_handle = Some(reference);
    assert!(app.start_test_action_grip(reference, current.id));
    let result = app.tabs[i].active_cmd.as_mut().unwrap().on_point(cursor);
    // A target hidden after preview must not impose its old tangent at commit.
    app.tabs[i].scene.select_entity(target, true); app.tabs[i].scene.hide_selected();
    let _ = app.handle_block_alignment(result);
    let after = app.tabs[i].scene.alignment_grips(reference).remove(0);
    assert!(after.anchor.distance(cursor) < 1e-8);
    assert!(after.direction.distance(current.direction) < 1e-8);
    app.undo_steps(1);
    app.tabs[i].selected_handle = Some(reference);
    let before = app.tabs[i].scene.document.clone();
    let _ = app.handle_block_alignment(crate::command::CmdResult::AlignBlockReference { document: document + 1, alignment: Box::new(current), cursor, context: None, flipped: false });
    assert_eq!(app.tabs[i].scene.document, before);
}

#[test]
fn block_parameter_metadata_properties_history_save_and_discard() {
    use crate::scene::block_authoring::metadata::{self, Field};
    use crate::scene::block_distance_parameter::{self as distance, Kind};
    use crate::scene::model::object::PropValue;
    for kind in [Kind::XY, Kind::Polar] {
        let (mut app, reference) = fixture();
        begin(&mut app, reference);
        let i = app.active_tab;
        let def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
        let handle = distance::create(
            &mut app.tabs[i].scene.document,
            def,
            kind,
            Vector3::ZERO,
            Vector3::new(2., 3., 0.),
            Vector3::UNIT_Y,
        )
        .unwrap();
        app.tabs[i].selected_authoring = Some(handle);
        app.refresh_properties();
        let original = app.tabs[i].scene.document.clone();
        let fields = app.tabs[i]
            .properties
            .sections
            .iter()
            .flat_map(|section| &section.props)
            .filter(|prop| metadata::Field::from_key(prop.field).is_some())
            .collect::<Vec<_>>();
        assert_eq!(fields.len(), 4);
        assert!(fields
            .iter()
            .all(|prop| matches!(prop.value, PropValue::PlainText(_))));
        let primary = metadata::read(&original, def, handle).unwrap().text[0]
            .value
            .to_owned();
        assert_eq!(fields[0].value, PropValue::PlainText(primary.clone()));
        let depth = app.tabs[i].history.undo_stack.len();
        for (field, value) in [
            ("authoring_name", primary.as_str()),
            ("authoring_name", " "),
            ("authoring_secondary_name", primary.as_str()),
        ] {
            let _ = app.update(Message::PropGeomInput {
                field,
                value: value.into(),
            });
            let _ = app.update(Message::PropGeomCommit(field));
            assert_eq!(app.tabs[i].scene.document, original);
            assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
        }
        for (field, value) in [
            ("authoring_name", "Travel"),
            ("authoring_secondary_name", "Secondary travel"),
            ("authoring_description", "  1 + 2 / literal text  "),
            ("authoring_secondary_description", "Second description"),
        ] {
            let _ = app.update(Message::PropGeomInput {
                field,
                value: value.into(),
            });
            let _ = app.update(Message::PropGeomCommit(field));
            assert_eq!(
                metadata::read(&app.tabs[i].scene.document, def, handle)
                    .unwrap()
                    .text
                    .into_iter()
                    .find(|text| text.field.key() == field)
                    .unwrap()
                    .value,
                value
            );
        }
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 4);
        let saved = app.tabs[i].scene.document.clone();
        app.undo_steps(4);
        assert_eq!(app.tabs[i].scene.document, original);
        app.redo_steps(4);
        assert_eq!(app.tabs[i].scene.document, saved);
        let _ = app.dispatch_command("BSAVE");
        app.tabs[i].selected_authoring = Some(handle);
        let _ = app.update(Message::PropGeomInput {
            field: "authoring_description",
            value: "".into(),
        });
        let _ = app.update(Message::PropGeomCommit("authoring_description"));
        assert_eq!(
            metadata::read(&app.tabs[i].scene.document, def, handle)
                .unwrap()
                .text
                .into_iter()
                .find(|text| text.field == Field::Description)
                .unwrap()
                .value,
            ""
        );
        let _ = app.dispatch_command("BEDIT_DISCARD");
        begin(&mut app, reference);
        assert_eq!(
            metadata::read(&app.tabs[i].scene.document, def, handle)
                .unwrap()
                .text
                .into_iter()
                .find(|text| text.field == Field::Description)
                .unwrap()
                .value,
            "  1 + 2 / literal text  "
        );
    }
}

#[test]
fn block_visibility_metadata_flag_hides_property_preserving_native_grips_and_states() {
    use crate::scene::{block_authoring::metadata, block_grips, block_visibility};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let handle =
        block_visibility::create(&mut app.tabs[i].scene.document, def, Vector3::ZERO).unwrap();
    metadata::set_text(
        &mut app.tabs[i].scene.document,
        def,
        handle,
        metadata::Field::Name,
        "Display choice",
    )
    .unwrap();
    app.tabs[i].selected_authoring = Some(handle);
    app.refresh_properties();
    assert!(app.tabs[i]
        .properties
        .sections
        .iter()
        .flat_map(|section| &section.props)
        .any(|prop| prop.field == "authoring_show_properties"));
    assert_eq!(
        OpenCADStudio::visibility_property(&app.tabs[i].scene.document, reference)
            .unwrap()
            .label,
        "Display choice"
    );
    let before = app.tabs[i].scene.document.clone();
    let grips = block_grips::config::locations(&before, def, handle)
        .unwrap()
        .into_iter()
        .map(|g| (g.role, g.handle, g.point, g.key_point, g.direction))
        .collect::<Vec<_>>();
    let states = block_visibility::parameter(&before, def)
        .unwrap()
        .states
        .clone();
    let depth = app.tabs[i].history.undo_stack.len();
    let _ = app.update(Message::PropBoolToggle("authoring_show_properties"));
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
    assert!(OpenCADStudio::visibility_property(&app.tabs[i].scene.document, reference).is_none());
    assert_eq!(
        block_grips::config::locations(&app.tabs[i].scene.document, def, handle)
            .unwrap()
            .into_iter()
            .map(|g| (g.role, g.handle, g.point, g.key_point, g.direction))
            .collect::<Vec<_>>(),
        grips
    );
    assert_eq!(
        block_visibility::parameter(&app.tabs[i].scene.document, def)
            .unwrap()
            .states,
        states
    );
    app.undo_steps(1);
    assert_eq!(app.tabs[i].scene.document, before);
    assert!(OpenCADStudio::visibility_property(&app.tabs[i].scene.document, reference).is_some());
    app.redo_steps(1);
    assert!(OpenCADStudio::visibility_property(&app.tabs[i].scene.document, reference).is_none());
}

#[test]
fn block_reference_numeric_properties_history_scratch_expression_and_stale_guards() {
    use crate::scene::model::object::{BlockParameterComponent as Component, PropValue};
    use crate::scene::{
        block_actions as actions, block_distance_parameter as distance, block_grips,
    };
    use crate::ui::properties::FieldKey;
    for testing in [false, true] {
        let (mut app, reference) = fixture();
        begin(&mut app, reference);
        let source = app.active_tab;
        let def = app.tabs[source]
            .active_block_edit_session()
            .unwrap()
            .br_handle;
        let parameter = distance::create(
            &mut app.tabs[source].scene.document,
            def,
            distance::Kind::Linear,
            Vector3::ZERO,
            Vector3::new(10., 0., 0.),
            Vector3::UNIT_Y,
        )
        .unwrap();
        let target = members(&app)[0];
        actions::create(
            &mut app.tabs[source].scene.document,
            def,
            actions::Kind::Move,
            parameter,
            &[target],
            glam::DVec3::ZERO,
        )
        .unwrap();
        block_grips::config::set(
            &mut app.tabs[source].scene.document,
            def,
            parameter,
            Some(0),
        )
        .unwrap();
        if testing {
            let _ = app.open_block_test(source);
        } else {
            let _ = app.dispatch_command("BSAVE");
            let _ = app.dispatch_command("BCLOSE");
        }
        let source_before = app.tabs[source].scene.document.clone();
        let i = app.active_tab;
        let insert = app.tabs[i]
            .block_test
            .as_ref()
            .map_or(reference, |test| test.insert);
        app.tabs[i].scene.select_entity(insert, true);
        app.tabs[i].selected_handle = Some(insert);
        app.refresh_properties();
        let field = app.tabs[i]
            .properties
            .sections
            .iter()
            .flat_map(|section| &section.props)
            .find_map(|property| match property.value {
                PropValue::BlockNumber { key, .. } if key.component == Component::Distance => {
                    Some(key)
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(field.document, app.tabs[i].id);
        assert!(app.tabs[i]
            .properties
            .prop_field_key_for_id(&FieldKey::BlockParameter(field).widget_id())
            .is_some());
        let before = app.tabs[i].scene.document.clone();
        let depth = app.tabs[i].history.undo_stack.len();
        for (key, raw) in [
            (field, "10"),
            (field, "invalid"),
            (field, "1/0"),
            (field, "-4"),
            (
                crate::scene::model::object::BlockParameterKey {
                    document: field.document + 1,
                    ..field
                },
                "20",
            ),
        ] {
            let _ = app.update(Message::PropBlockInput {
                key,
                value: raw.into(),
            });
            let _ = app.update(Message::PropBlockCommit(key));
            assert_eq!(app.tabs[i].scene.document, before);
            assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
        }
        let _ = app.update(Message::PropBlockInput {
            key: field,
            value: "10+5".into(),
        });
        let _ = app.update(Message::PropBlockCommit(field));
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
        assert!(!app.tabs[i]
            .properties
            .edit_buf
            .contains_key(&FieldKey::BlockParameter(field)));
        let changed = app.tabs[i].scene.document.clone();
        let p = actions::all_parameters(&before, def)
            .into_iter()
            .find(|p| p.handle == parameter)
            .unwrap();
        assert_eq!(
            actions::instance::frame(&changed, insert, &p).unwrap().end,
            glam::DVec3::new(15., 0., 0.)
        );
        let EntityType::Insert(reference) = changed.get_entity(insert).unwrap() else {
            panic!()
        };
        let line = changed
            .block_records
            .get(&reference.block_name)
            .unwrap()
            .entity_handles
            .iter()
            .find_map(|h| match changed.get_entity(*h) {
                Some(EntityType::Line(line)) => Some(line),
                _ => None,
            })
            .unwrap();
        assert_eq!(line.start, Vector3::new(5., 0., 0.));
        assert_eq!(line.end, Vector3::new(6., 0., 0.));
        app.undo_steps(1);
        let mut expected_undo = before.clone();
        // Entity undo preserves the monotonic handle allocator.
        while expected_undo.next_handle() < app.tabs[i].scene.document.next_handle() {
            expected_undo.allocate_handle();
        }
        assert_eq!(app.tabs[i].scene.document, expected_undo);
        app.redo_steps(1);
        assert_eq!(app.tabs[i].scene.document, changed);
        let _ = app.update(Message::PropBlockInput {
            key: field,
            value: "15".into(),
        });
        let _ = app.update(Message::PropBlockCommit(field));
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
        app.tabs[i]
            .scene
            .document
            .layers
            .get_mut("0")
            .unwrap()
            .lock();
        let locked = app.tabs[i].scene.document.clone();
        let _ = app.update(Message::PropBlockInput {
            key: field,
            value: "30".into(),
        });
        let _ = app.update(Message::PropBlockCommit(field));
        assert_eq!(app.tabs[i].scene.document, locked);
        if testing {
            assert_eq!(app.tabs[source].scene.document, source_before);
        }
    }
}

#[test]
fn block_reference_properties_keys_do_not_retarget_other_selections() {
    use crate::scene::model::object::PropValue;
    use crate::scene::{block_actions as actions, block_distance_parameter as distance};
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let i = app.active_tab;
    let def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
    let _ = distance::create(
        &mut app.tabs[i].scene.document,
        def,
        distance::Kind::XY,
        Vector3::ZERO,
        Vector3::new(10., 10., 0.),
        Vector3::UNIT_Y,
    )
    .unwrap();
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");
    let other = app.tabs[i]
        .scene
        .document
        .add_entity(EntityType::Insert(Insert::new(
            "EDIT_ME",
            Vector3::new(20., 0., 0.),
        )))
        .unwrap();
    app.tabs[i].scene.select_entity(reference, true);
    app.tabs[i].selected_handle = Some(reference);
    app.refresh_properties();
    let keys = app.tabs[i]
        .properties
        .sections
        .iter()
        .flat_map(|s| &s.props)
        .filter_map(|p| match p.value {
            PropValue::BlockNumber { key, .. } => Some(key),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(keys.len(), 2);
    assert_ne!(
        crate::ui::properties::FieldKey::BlockParameter(keys[0]).widget_id(),
        crate::ui::properties::FieldKey::BlockParameter(keys[1]).widget_id()
    );
    let _ = app.update(Message::PropBlockInput {
        key: keys[0],
        value: "20".into(),
    });
    app.tabs[i].scene.deselect_all();
    app.tabs[i].scene.select_entity(other, true);
    app.tabs[i].selected_handle = Some(other);
    app.refresh_properties();
    let before = app.tabs[i].scene.document.clone();
    let _ = app.update(Message::PropBlockCommit(keys[0]));
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(actions::all_parameters(&before, def).len(), 1);
}

#[test]
fn block_parameter_without_actions_hides_runtime_grips_and_rejects_stale_activation() {
    use crate::scene::block_grips::{config, runtime};
    use crate::scene::model::object::PropValue;
    use crate::scene::{
        block_actions as actions, block_distance_parameter as distance,
        block_orientation_parameter as orientation, block_point_parameter as point,
    };
    use codec::objects::{DynamicBlockData, ObjectType};
    for testing in [false, true] {
        for kind in 0..6 {
            let (mut app, reference) = fixture();
            begin(&mut app, reference);
            let source = app.active_tab;
            let def = app.tabs[source]
                .active_block_edit_session()
                .unwrap()
                .br_handle;
            let doc = &mut app.tabs[source].scene.document;
            let base = Vector3::new(2., 3., 0.);
            let end = Vector3::new(12., 13., 0.);
            let parameter = match kind {
                0 => point::create(doc, def, base, end),
                1..=3 => distance::create(
                    doc,
                    def,
                    match kind {
                        1 => distance::Kind::Linear,
                        2 => distance::Kind::Polar,
                        _ => distance::Kind::XY,
                    },
                    base,
                    end,
                    Vector3::new(4., 5., 0.),
                ),
                _ => orientation::create(
                    doc,
                    def,
                    if kind == 4 {
                        orientation::Kind::Rotation
                    } else {
                        orientation::Kind::Flip
                    },
                    base,
                    end,
                    end,
                ),
            }
            .unwrap();
            if testing {
                let _ = app.open_block_test(source);
            } else {
                let _ = app.dispatch_command("BSAVE");
                let _ = app.dispatch_command("BCLOSE");
            }
            let source_before = app.tabs[source].scene.document.clone();
            let i = app.active_tab;
            let insert = app.tabs[i]
                .block_test
                .as_ref()
                .map_or(reference, |test| test.insert);
            app.tabs[i].scene.select_entity(insert, true);
            app.tabs[i].selected_handle = Some(insert);
            app.refresh_properties();
            app.tabs[i].selected_grips.clear();
            app.tabs[i].selected_grip_handles.clear();
            app.refresh_test_action_grips([0.; 3]);
            assert!(!app.tabs[i]
                .selected_grips
                .iter()
                .any(|grip| runtime::decode(grip.id).is_some()));
            assert!(app.tabs[i]
                .properties
                .sections
                .iter()
                .flat_map(|section| &section.props)
                .any(|prop| matches!(
                    prop.value,
                    PropValue::BlockNumber { .. } | PropValue::BlockChoice { .. }
                )));
            let p = actions::all_parameters(&app.tabs[i].scene.document, def)
                .into_iter()
                .find(|p| p.handle == parameter)
                .unwrap();
            let before = app.tabs[i].scene.document.clone();
            let depth = app.tabs[i].history.undo_stack.len();
            let locations = config::locations(&before, def, parameter).unwrap();
            assert!(!locations.is_empty());
            for grip in locations {
                assert!(!app.start_test_action_grip(insert, runtime::id(p.id, grip.role).unwrap()));
                assert!(app.tabs[i].active_cmd.is_none());
            }
            let document = app.tabs[i].id;
            let _ =
                app.handle_block_action_result(crate::command::CmdResult::EvaluateBlockParameter { grip: None,
                    document,
                    definition: def,
                    parameter,
                    insert: (!testing).then_some(insert),
                    value: if kind == 5 {
                        actions::Value::Flip(true)
                    } else {
                        actions::Value::Point(glam::DVec3::new(20., 20., 0.))
                    },
                });
            assert_eq!(app.tabs[i].scene.document, before);
            assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
            if testing {
                assert_eq!(app.tabs[source].scene.document, source_before);
            }
            // Adding a valid native association restores the same configured grip.
            let member = app.tabs[i]
                .scene
                .document
                .block_records
                .get("EDIT_ME")
                .unwrap()
                .entity_handles[0];
            let action = actions::create(
                &mut app.tabs[i].scene.document,
                def,
                match kind {
                    4 => actions::Kind::Rotate,
                    5 => actions::Kind::Flip,
                    _ => actions::Kind::Move,
                },
                parameter,
                &[member],
                glam::DVec3::ZERO,
            )
            .unwrap();
            let baseline = app.tabs[i].scene.document.clone();
            if let Some(test) = &mut app.tabs[i].block_test {
                test.baseline = std::sync::Arc::new(baseline);
            }
            app.tabs[i].selected_grips.clear();
            app.tabs[i].selected_grip_handles.clear();
            app.refresh_test_action_grips([0.; 3]);
            let grip = app.tabs[i]
                .selected_grips
                .iter()
                .find(|grip| runtime::decode(grip.id).is_some_and(|(node, _)| node == p.id))
                .unwrap()
                .id;
            assert!(actions::has_action(
                &app.tabs[i].scene.document,
                def,
                parameter
            ));
            if kind != 5 {
                assert!(app.start_test_action_grip(insert, grip));
                assert!(app.tabs[i].active_cmd.is_some());
                let _ = app.update(Message::CommandEscape);
            }
            // A retained command result cannot mutate values after disassociation.
            let ObjectType::DynamicBlock(object) =
                app.tabs[i].scene.document.objects.get_mut(&action).unwrap()
            else {
                panic!()
            };
            let action = match &mut object.data {
                DynamicBlockData::MoveAction(a) => &mut a.connections[0],
                DynamicBlockData::RotateAction(a) => &mut a.connections[0],
                DynamicBlockData::FlipAction(a) => &mut a.connections[0],
                _ => unreachable!(),
            };
            action.code = -1;
            let baseline = app.tabs[i].scene.document.clone();
            if let Some(test) = &mut app.tabs[i].block_test {
                test.baseline = std::sync::Arc::new(baseline);
            }
            let before = app.tabs[i].scene.document.clone();
            let _ =
                app.handle_block_action_result(crate::command::CmdResult::EvaluateBlockParameter { grip: None,
                    document,
                    definition: def,
                    parameter,
                    insert: (!testing).then_some(insert),
                    value: if kind == 5 {
                        actions::Value::Flip(true)
                    } else {
                        actions::Value::Point(glam::DVec3::new(20., 20., 0.))
                    },
                });
            assert_eq!(app.tabs[i].scene.document, before);
        }
    }
}

#[test]
fn block_key_point_grip_eligibility_and_pending_result_follow_reassociation() {
    use crate::scene::{
        block_actions as actions, block_distance_parameter as distance,
        block_grips::{config, runtime},
    };
    use codec::objects::{DynamicBlockData, ObjectType};
    use glam::DVec3;
    for testing in [false, true] {
        let (mut app, reference) = fixture();
        begin(&mut app, reference);
        let source = app.active_tab;
        let def = app.tabs[source]
            .active_block_edit_session()
            .unwrap()
            .br_handle;
        let parameter = distance::create(
            &mut app.tabs[source].scene.document,
            def,
            distance::Kind::XY,
            Vector3::ZERO,
            Vector3::new(10., 10., 0.),
            Vector3::UNIT_Y,
        )
        .unwrap();
        let target = members(&app)[0];
        let action = actions::create(
            &mut app.tabs[source].scene.document,
            def,
            actions::Kind::Move,
            parameter,
            &[target],
            DVec3::ZERO,
        )
        .unwrap();
        config::set(
            &mut app.tabs[source].scene.document,
            def,
            parameter,
            Some(4),
        )
        .unwrap();
        crate::scene::block_authoring::metadata::set_visible(
            &mut app.tabs[source].scene.document,
            def,
            parameter,
            false,
        )
        .unwrap();
        if testing {
            let _ = app.open_block_test(source);
        } else {
            let _ = app.dispatch_command("BSAVE");
            let _ = app.dispatch_command("BCLOSE");
        }
        let source_before = app.tabs[source].scene.document.clone();
        let i = app.active_tab;
        let insert = app.tabs[i]
            .block_test
            .as_ref()
            .map_or(reference, |test| test.insert);
        app.tabs[i].scene.select_entity(insert, true);
        app.tabs[i].selected_handle = Some(insert);
        app.tabs[i].selected_grips.clear();
        app.tabs[i].selected_grip_handles.clear();
        app.refresh_test_action_grips([0.; 3]);
        let p = actions::all_parameters(&app.tabs[i].scene.document, def)
            .into_iter()
            .find(|p| p.handle == parameter)
            .unwrap();
        let end_id = runtime::id(p.id, 1).unwrap();
        assert_eq!(
            app.tabs[i]
                .selected_grips
                .iter()
                .filter_map(|grip| runtime::decode(grip.id))
                .collect::<Vec<_>>(),
            vec![(p.id, 1), (p.id, 2), (p.id, 3)]
        );
        assert_eq!(
            config::locations(&app.tabs[i].scene.document, def, parameter)
                .unwrap()
                .len(),
            4
        );
        assert!(!app.start_test_action_grip(insert, runtime::id(p.id, 0).unwrap()));
        assert!(app.start_test_action_grip(insert, end_id));
        assert_eq!(
            app.tabs[i]
                .active_cmd
                .as_ref()
                .unwrap()
                .block_parameter_grip_id(),
            Some(end_id)
        );
        let ObjectType::DynamicBlock(object) =
            app.tabs[i].scene.document.objects.get_mut(&action).unwrap()
        else {
            panic!()
        };
        let DynamicBlockData::MoveAction(movement) = &mut object.data else {
            panic!()
        };
        movement.connections[0].name = "BaseXDelta".into();
        movement.connections[1].name = "BaseYDelta".into();
        let baseline = app.tabs[i].scene.document.clone();
        if let Some(test) = &mut app.tabs[i].block_test {
            test.baseline = std::sync::Arc::new(baseline);
        }
        let before = app.tabs[i].scene.document.clone();
        let depth = app.tabs[i].history.undo_stack.len();
        let _ = app.feed_command(crate::command::StepInput::Point(DVec3::new(20., 20., 0.)));
        assert_eq!(app.tabs[i].scene.document, before);
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
        assert!(!app.start_test_action_grip(insert, end_id));
        app.tabs[i].selected_grips.clear();
        app.tabs[i].selected_grip_handles.clear();
        app.refresh_test_action_grips([0.; 3]);
        assert_eq!(
            app.tabs[i]
                .selected_grips
                .iter()
                .filter_map(|grip| runtime::decode(grip.id))
                .collect::<Vec<_>>(),
            vec![(p.id, 0), (p.id, 2), (p.id, 3)]
        );
        let base_id = runtime::id(p.id, 0).unwrap();
        assert!(app.start_test_action_grip(insert, base_id));
        let _ = app.update(Message::CommandEscape);
        assert!(runtime::action_grip_available(
            &app.tabs[i].scene.document,
            def,
            parameter,
            base_id
        ));
        assert!(!runtime::action_grip_available(
            &app.tabs[i].scene.document,
            def,
            parameter,
            end_id
        ));
        if testing {
            assert_eq!(app.tabs[source].scene.document, source_before);
        }
    }
}

mod scalar_grip_bindings;

mod alignment_insertion;

mod parameter_preview;

mod lookup_expressions;
