use super::*;

#[test]
fn add_entities_batch_assigns_handles_and_publishes_once() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);

    let pts: Vec<EntityType> = (0..5)
        .map(|i| {
            EntityType::Point(Point::at(document::types::Vector3::new(
                i as f64, i as f64, 0.0,
            )))
        })
        .collect();
    let handles = host.add_entities(pts);

    assert_eq!(handles.len(), 5);
    assert!(handles.iter().all(|h| !h.is_null()));
    // Each handle is unique.
    let mut set = std::collections::HashSet::new();
    for h in &handles {
        assert!(set.insert(h.value()));
    }
    // All entities are in the document.
    for h in &handles {
        assert!(host.document().get_entity(*h).is_some());
    }
}

#[test]
fn update_entity_replaces_in_place_preserving_handle() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let h = host.add_entity(EntityType::Point(Point::at(document::types::Vector3::new(
        1.0, 1.0, 0.0,
    ))));
    let epoch_before = host.app.tabs[0].scene.geometry_epoch;

    // Edit a snapshot copy (as a plugin would) and commit it.
    let mut edited = host.document().get_entity(h).unwrap().clone();
    edited.common_mut().layer = "PLUGIN_EDIT".to_string();
    assert!(host.update_entity(edited));

    // Same handle, edit applied, geometry re-tessellated.
    let got = host
        .document()
        .get_entity(h)
        .expect("entity kept its handle");
    assert_eq!(got.common().layer, "PLUGIN_EDIT");
    assert_ne!(
        host.app.tabs[0].scene.geometry_epoch, epoch_before,
        "update should bump geometry"
    );

    // Updating an unknown handle fails and changes nothing.
    let mut ghost = Point::new();
    ghost.common.handle = Handle::new(999_999);
    assert!(!host.update_entity(EntityType::Point(ghost)));
}

fn reference_test_line(end_x: f64, y: f64, handle: Handle) -> EntityType {
    let mut line = document::entities::Line::from_points(
        document::types::Vector3::new(0.0, y, 0.0),
        document::types::Vector3::new(end_x, y, 0.0),
    );
    line.common.handle = handle;
    EntityType::Line(line)
}

#[test]
fn reference_only_refusals_record_no_undo_step() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    // Marking an absent entity refuses.
    assert!(host.set_reference_only(Handle::new(999_999), true).is_err());
    let handle = host.add_entity(reference_test_line(10.0, 0.0, Handle::NULL));
    // A locked layer is as unwritable for the designation as for every
    // other plugin write.
    host.app.tabs[0]
        .scene
        .document
        .layers
        .get_mut("0")
        .map(|layer| layer.lock());
    assert!(host.set_reference_only(handle, true).is_err());
    host.app.tabs[0]
        .scene
        .document
        .layers
        .get_mut("0")
        .map(|layer| layer.unlock());
    assert!(
        !host.app.tabs[0].scene.is_reference_only(handle),
        "refusals must not partially apply the designation"
    );
    assert_eq!(
        host.app.tabs[0].history.undo_stack.len(),
        0,
        "refusals record no undo step"
    );
    // An idempotent re-mark succeeds without recording another step.
    host.set_reference_only(handle, true).unwrap();
    assert_eq!(host.app.tabs[0].history.undo_stack.len(), 1);
    host.set_reference_only(handle, true).unwrap();
    assert_eq!(
        host.app.tabs[0].history.undo_stack.len(),
        1,
        "an idempotent re-mark records no undo step"
    );
}

#[test]
fn reference_only_designation_composes_undo_like_entity_writes() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let handle = host.add_entity(reference_test_line(10.0, 0.0, Handle::NULL));
    // A scope-less designation is one undo step of its own.
    host.set_reference_only(handle, true).unwrap();
    host.set_reference_only(handle, false).unwrap();
    assert_eq!(host.app.tabs[0].history.undo_stack.len(), 2);

    // Undo reverts the unmark, redo re-applies it.
    drop(host);
    app.undo_steps(1);
    let host = HostSession::new(&mut app, 0);
    assert!(host.app.tabs[0].scene.is_reference_only(handle));
    drop(host);
    app.redo_steps(1);
    let mut host = HostSession::new(&mut app, 0);
    assert!(!host.app.tabs[0].scene.is_reference_only(handle));

    // Inside a plugin-opened push_undo scope the designation joins the
    // scope: an entity write plus two markings is ONE step under the
    // plugin's label.
    host.push_undo("PLUGIN: refresh");
    let preview = host.add_entity(EntityType::Point(Point::at(document::types::Vector3::new(
        3.0, 0.0, 0.0,
    ))));
    host.set_reference_only(preview, true).unwrap();
    host.set_reference_only(handle, true).unwrap();
    drop(host);
    app.finish_pending_history(0);
    assert_eq!(app.tabs[0].history.undo_stack.len(), 3);
    assert_eq!(app.tabs[0].history.undo_stack[2].label(), "PLUGIN: refresh");

    // Undoing the plugin operation reverts it whole.
    app.undo_steps(1);
    let host = HostSession::new(&mut app, 0);
    assert!(
        host.app.tabs[0]
            .scene
            .document
            .get_entity(preview)
            .is_none(),
        "undoing the scope removed the entity created inside it"
    );
    assert!(
        !host.app.tabs[0].scene.is_reference_only(handle),
        "undoing the scope restored the designation's prior state"
    );
}

#[test]
fn reference_only_designation_is_not_written_to_dwg_or_dxf() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let handle = host.add_entity(reference_test_line(10.0, 0.0, Handle::NULL));
    host.set_reference_only(handle, true).unwrap();
    // The designation lives in the scene's in-memory set only.
    assert!(host.app.tabs[0].scene.is_reference_only(handle));

    // The host writes nothing of its own into the document: no APPID
    // registration, no marker XDATA on the designated entity.
    assert!(host.document().app_ids.get("OCS").is_none());
    assert!(
        host.document()
            .get_entity(handle)
            .expect("designated entity")
            .common()
            .extended_data
            .get_record("OCS")
            .is_none()
    );

    let contains_marker =
        |bytes: &[u8]| bytes.windows(15).any(|window| window == b"REFERENCE_ONLY");

    // DWG: the marker string appears nowhere in the saved bytes, and a
    // reopened file carries neither the host APPID nor the marker record.
    let dwg = document::DwgWriter::write_to_vec(host.document()).unwrap();
    assert!(
        !contains_marker(&dwg),
        "DWG save must not contain the designation marker"
    );
    let reloaded = crate::io::load_bytes("reference-only.dwg", dwg).unwrap();
    assert!(reloaded.app_ids.get("OCS").is_none());
    assert!(
        reloaded
            .get_entity(handle)
            .expect("the line survives the round trip")
            .common()
            .extended_data
            .get_record("OCS")
            .is_none()
    );
    assert!(
        !contains_marker(&document::DwgWriter::write_to_vec(&reloaded).unwrap()),
        "a re-save of the reopened file stays marker-free"
    );

    // DXF: same contract for the text format.
    let dxf = document::DxfWriter::new(host.document())
        .write_to_vec()
        .unwrap();
    assert!(
        !contains_marker(&dxf),
        "DXF save must not contain the designation marker"
    );
    let reloaded = crate::io::load_bytes("reference-only.dxf", dxf).unwrap();
    assert!(reloaded.app_ids.get("OCS").is_none());
    assert!(
        reloaded
            .get_entity(handle)
            .expect("the line survives the round trip")
            .common()
            .extended_data
            .get_record("OCS")
            .is_none()
    );
}

#[test]
fn erasing_a_designated_entity_undoes_designation_with_it() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let handle = host.add_entity(reference_test_line(10.0, 0.0, Handle::NULL));
    host.set_reference_only(handle, true).unwrap();

    // The plugin erase path removes a designated entity inside a scope:
    // entity and set entry leave together.
    host.push_undo("PLUGIN: replace the underlying model");
    host.remove_entity(handle);
    drop(host);
    app.finish_pending_history(0);
    let host = HostSession::new(&mut app, 0);
    assert!(host.app.tabs[0].scene.document.get_entity(handle).is_none());
    assert!(!host.app.tabs[0].scene.is_reference_only(handle));
    drop(host);

    // Undoing the erase restores the entity AND its designation: the set
    // mutation rode the scope as an explicit designation channel.
    app.undo_steps(1);
    let host = HostSession::new(&mut app, 0);
    assert!(
        host.app.tabs[0].scene.document.get_entity(handle).is_some(),
        "undo restored the erased entity"
    );
    assert!(
        host.app.tabs[0].scene.is_reference_only(handle),
        "undo restored the designation together with its entity"
    );
}

#[test]
fn user_paths_refuse_and_plugin_paths_write_reference_only_entities() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let handle = host.add_entity(reference_test_line(10.0, 0.0, Handle::NULL));
    host.set_reference_only(handle, true).unwrap();
    let plain = host.add_entity(reference_test_line(10.0, 5.0, Handle::NULL));

    // Grips: the reference-only entity contributes none; the editable one
    // keeps its own.
    host.set_selection(&[handle]).unwrap();
    host.app.refresh_selected_grips();
    assert!(
        host.app.tabs[0].selected_grips.is_empty(),
        "reference-only entities contribute no grips"
    );
    host.set_selection(&[plain]).unwrap();
    host.app.refresh_selected_grips();
    assert!(!host.app.tabs[0].selected_grips.is_empty());

    // The user in-place update refuses; the user batch skips only the
    // reference-only member; the plugin-owned variants write through.
    assert!(
        !host.app.tabs[0]
            .scene
            .update_entity(reference_test_line(20.0, 0.0, handle))
    );
    let applied = host.app.tabs[0].scene.update_entities(vec![
        reference_test_line(20.0, 0.0, handle),
        reference_test_line(20.0, 5.0, plain),
    ]);
    assert_eq!(
        applied, 1,
        "the user batch applies only the editable entity"
    );
    let end_x = |scene: &crate::scene::Scene, h: Handle| match scene.document.get_entity(h) {
        Some(EntityType::Line(line)) => line.end.x,
        _ => panic!("entity {h:?} must be a line"),
    };
    let scene = &host.app.tabs[0].scene;
    assert_eq!(
        end_x(scene, handle),
        10.0,
        "the user paths refused the write"
    );
    assert_eq!(end_x(scene, plain), 20.0);
    // The plugin-owned variant writes through. A real plugin refresh
    // clones the live entity (designation included), edits it, and
    // replaces it — so model that flow instead of a bare synthetic.
    let mut moved = match host.app.tabs[0].scene.document.get_entity(handle) {
        Some(entity) => entity.clone(),
        _ => unreachable!("the entity exists"),
    };
    if let EntityType::Line(line) = &mut moved {
        line.end.x = 30.0;
    }
    assert!(host.app.tabs[0].scene.update_entity_plugin(moved));
    let scene = &host.app.tabs[0].scene;
    assert_eq!(
        end_x(scene, handle),
        30.0,
        "the plugin-owned update writes through"
    );
    assert!(
        scene.is_reference_only(handle),
        "a refresh-style plugin write preserves the designation"
    );

    // User erase skips reference-only entities; the plugin erase path
    // removes them.
    host.app.tabs[0].scene.erase_entities(&[handle]);
    assert!(
        host.app.tabs[0].scene.document.get_entity(handle).is_some(),
        "user erase skips reference-only entities"
    );
    host.app.tabs[0].scene.erase_entities_plugin(&[handle]);
    assert!(
        host.app.tabs[0].scene.document.get_entity(handle).is_none(),
        "the plugin erase path removes reference-only entities"
    );
    assert!(host.app.tabs[0].scene.document.get_entity(plain).is_some());
}

#[test]
fn select_all_visible_skips_reference_only_entities() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let handle = host.add_entity(reference_test_line(10.0, 0.0, Handle::NULL));
    host.set_reference_only(handle, true).unwrap();
    let plain = host.add_entity(reference_test_line(10.0, 5.0, Handle::NULL));

    assert_eq!(host.app.tabs[0].scene.select_all_visible(), 1);
    let selected = host.app.tabs[0].scene.selected_handles_in_order();
    assert_eq!(selected, vec![plain]);
}

#[test]
fn add_and_update_auto_register_novel_layers_with_real_handles() {
    // A plugin adds/edits an entity naming a layer no LAYER command ever
    // created. The layer must gain a real table entry (non-null handle) so
    // it survives a DWG save instead of collapsing to layer 0 (#252, #67).
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);

    assert!(!host.document().layers.contains("PLUGIN-LAYER"));
    let mut pt = Point::at(document::types::Vector3::new(3.0, 3.0, 0.0));
    pt.common.layer = "PLUGIN-LAYER".to_string();
    host.add_entity(EntityType::Point(pt));

    let layer = host
        .document()
        .layers
        .get("PLUGIN-LAYER")
        .expect("novel layer auto-registered on add_entity");
    assert!(
        !layer.handle.is_null(),
        "auto-registered layer must carry a real handle (#67)"
    );

    // Adding again on the same (now-existing) layer must not duplicate or
    // error — the always-present default "0" is likewise never re-created.
    let count_before = host.document().layers.len();
    host.add_entity(EntityType::Point(Point::new())); // default layer "0"
    let mut pt2 = Point::new();
    pt2.common.layer = "PLUGIN-LAYER".to_string();
    host.add_entity(EntityType::Point(pt2));
    assert_eq!(host.document().layers.len(), count_before);

    // Retargeting an entity to a novel layer via update_entity registers it.
    let h = host.add_entity(EntityType::Point(Point::new()));
    let mut edited = host.document().get_entity(h).unwrap().clone();
    edited.common_mut().layer = "EDIT-LAYER".to_string();
    assert!(host.update_entity(edited));
    assert!(host.document().layers.contains("EDIT-LAYER"));
}

#[test]
fn remove_entity_deletes_and_clears_caches() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let h = host.add_entity(EntityType::Point(Point::at(document::types::Vector3::new(
        2.0, 2.0, 0.0,
    ))));
    assert!(host.document().get_entity(h).is_some());

    assert!(host.remove_entity(h));
    assert!(host.document().get_entity(h).is_none());
    assert!(!host.app.tabs[0].scene.hatches.contains_key(&h));
    assert!(!host.app.tabs[0].scene.meshes.contains_key(&h));

    // Removing an already-gone handle reports false.
    assert!(!host.remove_entity(h));
}

#[test]
fn update_entity_refreshes_selected_grips() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let handle = {
        let mut host = HostSession::new(&mut app, 0);
        let mut line = document::entities::Line::new();
        line.start = document::types::Vector3::new(1.0, 1.0, 0.0);
        line.end = document::types::Vector3::new(3.0, 1.0, 0.0);
        let handle = host.add_entity(EntityType::Line(line));
        host.set_selection(&[handle]).unwrap();
        handle
    };
    // A real selection event rebuilt the grip cache the way the app's
    // own paths do.
    app.refresh_selected_grips();
    assert_eq!(app.tabs[0].selected_grips.len(), 3);
    assert_eq!(
        app.tabs[0]
            .selected_grips
            .iter()
            .map(|grip| grip.world)
            .collect::<Vec<_>>(),
        vec![
            glam::DVec3::new(1.0, 1.0, 0.0),
            glam::DVec3::new(3.0, 1.0, 0.0),
            glam::DVec3::new(2.0, 1.0, 0.0),
        ]
    );
    {
        let mut host = HostSession::new(&mut app, 0);
        let mut edited = host.document().get_entity(handle).cloned().unwrap();
        if let EntityType::Line(line) = &mut edited {
            line.start = document::types::Vector3::new(101.0, 1.0, 0.0);
            line.end = document::types::Vector3::new(103.0, 1.0, 0.0);
        }
        assert!(host.update_entity(edited));
    }
    // The cached grip markers must sit on the refreshed geometry, not at
    // their pre-edit positions.
    assert_eq!(
        app.tabs[0]
            .selected_grips
            .iter()
            .map(|grip| grip.world)
            .collect::<Vec<_>>(),
        vec![
            glam::DVec3::new(101.0, 1.0, 0.0),
            glam::DVec3::new(103.0, 1.0, 0.0),
            glam::DVec3::new(102.0, 1.0, 0.0),
        ]
    );
}

#[test]
fn update_entities_transaction_refreshes_selected_grips() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let handle = {
        let mut host = HostSession::new(&mut app, 0);
        let mut line = document::entities::Line::new();
        line.start = document::types::Vector3::new(0.0, 0.0, 0.0);
        line.end = document::types::Vector3::new(2.0, 0.0, 0.0);
        let handle = host.add_entity(EntityType::Line(line));
        host.set_selection(&[handle]).unwrap();
        handle
    };
    app.refresh_selected_grips();
    assert_eq!(app.tabs[0].selected_grips.len(), 3);
    {
        let mut host = HostSession::new(&mut app, 0);
        let mut edited = host.document().get_entity(handle).cloned().unwrap();
        if let EntityType::Line(line) = &mut edited {
            line.start = document::types::Vector3::new(50.0, 0.0, 0.0);
            line.end = document::types::Vector3::new(52.0, 0.0, 0.0);
        }
        host.update_entities_transaction("Move line", vec![edited])
            .unwrap();
    }
    app.finish_pending_history(0);
    assert_eq!(
        app.tabs[0]
            .selected_grips
            .iter()
            .map(|grip| grip.world)
            .collect::<Vec<_>>(),
        vec![
            glam::DVec3::new(50.0, 0.0, 0.0),
            glam::DVec3::new(52.0, 0.0, 0.0),
            glam::DVec3::new(51.0, 0.0, 0.0),
        ]
    );
}

#[test]
fn remove_entity_clears_selected_grips_of_removed_entity() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let handle = {
        let mut host = HostSession::new(&mut app, 0);
        let mut line = document::entities::Line::new();
        line.start = document::types::Vector3::new(1.0, 1.0, 0.0);
        line.end = document::types::Vector3::new(3.0, 1.0, 0.0);
        let handle = host.add_entity(EntityType::Line(line));
        host.set_selection(&[handle]).unwrap();
        handle
    };
    app.refresh_selected_grips();
    assert_eq!(app.tabs[0].selected_grips.len(), 3);
    {
        let mut host = HostSession::new(&mut app, 0);
        assert!(host.remove_entity(handle));
    }
    // Erase deselects the handle; the grip cache must drop the markers
    // of the removed entity instead of keeping them until the next
    // selection event.
    assert!(app.tabs[0].selected_grips.is_empty());
}

#[test]
fn update_entity_without_selection_keeps_grip_cache_empty() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let handle = {
        let mut host = HostSession::new(&mut app, 0);
        let mut line = document::entities::Line::new();
        line.start = document::types::Vector3::new(1.0, 1.0, 0.0);
        line.end = document::types::Vector3::new(3.0, 1.0, 0.0);
        host.add_entity(EntityType::Line(line))
    };
    app.refresh_selected_grips();
    assert!(app.tabs[0].selected_grips.is_empty());
    {
        let mut host = HostSession::new(&mut app, 0);
        let mut edited = host.document().get_entity(handle).cloned().unwrap();
        if let EntityType::Line(line) = &mut edited {
            line.start = document::types::Vector3::new(10.0, 1.0, 0.0);
        }
        // Still succeeds; nothing was selected, so no panic, no phantom
        // grips.
        assert!(host.update_entity(edited));
    }
    assert!(app.tabs[0].selected_grips.is_empty());
}

#[test]
fn background_tab_update_leaves_active_grips_untouched() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let _handle = {
        let mut host = HostSession::new(&mut app, 0);
        let mut line = document::entities::Line::new();
        line.start = document::types::Vector3::new(1.0, 1.0, 0.0);
        line.end = document::types::Vector3::new(3.0, 1.0, 0.0);
        let handle = host.add_entity(EntityType::Line(line));
        host.set_selection(&[handle]).unwrap();
        handle
    };
    app.refresh_selected_grips();
    assert_eq!(app.tabs[0].selected_grips.len(), 3);
    // Mark one cached grip so an accidental refresh (which would rebuild
    // it from the active tab's unchanged scene) is observable.
    app.tabs[0].selected_grips[0].world = glam::DVec3::new(999.0, 999.0, 0.0);
    // A second document tab is background while tab 0 stays active.
    app.push_test_document();
    assert_eq!(app.active_tab, 0);
    {
        let mut host = HostSession::new(&mut app, 1);
        let mut line = document::entities::Line::new();
        line.start = document::types::Vector3::new(100.0, 100.0, 0.0);
        line.end = document::types::Vector3::new(102.0, 100.0, 0.0);
        let background = host.add_entity(EntityType::Line(line));
        host.set_selection(&[background]).unwrap();
        let mut edited = host.document().get_entity(background).cloned().unwrap();
        if let EntityType::Line(line) = &mut edited {
            line.start = document::types::Vector3::new(200.0, 100.0, 0.0);
        }
        assert!(host.update_entity(edited));
    }
    // The background-tab session neither refreshed nor disturbed the
    // active tab's cached grips.
    assert_eq!(
        app.tabs[0].selected_grips[0].world,
        glam::DVec3::new(999.0, 999.0, 0.0)
    );
    assert_eq!(app.tabs[0].selected_grips.len(), 3);
}
