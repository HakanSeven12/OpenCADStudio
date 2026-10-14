use super::{Message, ModalKind, OpenCADStudio};
use codec::entities::{Insert, Line};
use codec::types::Vector3;
use codec::{EntityType, Handle};

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

#[test]
fn local_undo_preserves_imported_native_networks_when_constraints_are_unchanged() {
    use codec::objects::{AssocNetwork, AssociativeData, AssociativeObject, Dictionary, ObjectType};
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
    assert!(!app.save_block_edit_as(i, "COPY"));
    assert_eq!(app.tabs[i].scene.document.objects, original_objects);
    app.undo_steps(1);
    assert_eq!(members(&app).len(), 1);
    assert_eq!(app.tabs[i].scene.document.objects, original_objects);
    app.redo_steps(1);
    assert_eq!(members(&app).len(), 2);
    assert_eq!(app.tabs[i].scene.document.objects, original_objects);
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
