use super::*;

fn placed_visibility(app: &OpenCADStudio) -> &document::objects::BlockVisibilityParameter {
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
    use document::objects::{DynamicBlockData, DynamicBlockObject, ObjectType};
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
                document::objects::ObjectType::DynamicBlock(object)
                    if object.owner == parameter.owner =>
                {
                    match &object.data {
                        document::objects::DynamicBlockData::VisibilityGrip(grip) => {
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
