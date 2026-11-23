use super::*;

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
            let _ = app.run_command_line(if perpendicular {
                "BPARAMETER ALIGNMENT 0,0 10,0"
            } else {
                "BPARAMETER ALIGNMENT 0,0 TYPE T 10,0"
            });
            let definition = app.tabs[source]
                .active_block_edit_session()
                .unwrap()
                .br_handle;
            let parameter = crate::scene::block_orientation_parameter::parameters(
                &app.tabs[source].scene.document,
                definition,
            )[0]
            .handle;
            config::set_location(
                &mut app.tabs[source].scene.document,
                definition,
                parameter,
                0,
                Vector3::new(5., 7., 0.),
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
                .map(|test| test.insert)
                .unwrap_or(reference);
            let other = app.tabs[i]
                .scene
                .document
                .add_entity(EntityType::Insert(Insert::new(
                    "EDIT_ME",
                    Vector3::new(-30., -30., 0.),
                )))
                .unwrap();
            app.tabs[i]
                .scene
                .document
                .add_entity(EntityType::Line(Line::from_points(
                    Vector3::new(30., -20., 0.),
                    Vector3::new(30., 20., 0.),
                )))
                .unwrap();
            app.tabs[i].scene.bump_geometry();
            app.tabs[i].scene.selection.borrow_mut().view.vp_size = (1000., 800.);
            app.tabs[i].scene.camera.borrow_mut().fit_to_bounds_f64(
                DVec3::new(-20., -30., 0.),
                DVec3::new(50., 30., 0.),
                1.25,
            );
            // The destination is a point along the line, not one of its ends.
            app.snapper.enabled.insert(crate::snap::SnapType::Nearest);
            let select = |app: &mut OpenCADStudio| {
                app.tabs[i].scene.select_entity(insert, true);
                app.tabs[i].selected_handle = Some(insert);
                app.tabs[i].selected_grips.clear();
                app.tabs[i].selected_grip_handles.clear();
                app.refresh_test_action_grips([0.; 3]);
            };
            let click = |app: &mut OpenCADStudio, point: DVec3| {
                let scene = &app.tabs[i].scene;
                let tile = scene.active_model_tile_bounds(1000., 800.);
                let at = scene
                    .camera
                    .borrow()
                    .project(
                        point,
                        iced::Rectangle::with_size(iced::Size::new(tile.width, tile.height)),
                    )
                    .unwrap();
                let _ = app.update(Message::ViewportLeftPressAt(iced::Point::new(
                    at.x + tile.x,
                    at.y + tile.y,
                )));
                let _ = app.update(Message::ViewportLeftRelease);
            };
            select(&mut app);
            let alignment = app.tabs[i].scene.alignment_grips(insert).remove(0);
            assert!(app.tabs[i].selected_grips.iter().any(
                |grip| grip.id == alignment.id && grip.world.distance(alignment.anchor) < 1e-9
            ));
            assert!(
                app.tabs[i].selected_grips.iter().any(|grip| {
                    grip.id == alignment.id
                        && grip.shape == crate::scene::model::object::GripShape::Alignment
                }),
                "the alignment grip paints as the tag, not a direction triangle"
            );
            let before = app.tabs[i].scene.document.clone();
            let depth = app.tabs[i].history.undo_stack.len();
            click(&mut app, alignment.anchor);
            assert_eq!(
                app.tabs[i].active_cmd.as_ref().unwrap().name(),
                "BLOCKALIGNMENTGRIP"
            );
            let destination = DVec3::new(30., 4., 0.);
            let preview = app.tabs[i]
                .active_cmd
                .as_mut()
                .unwrap()
                .preview_entities(&before, destination);
            assert_eq!(preview.len(), 1);
            assert_eq!(
                app.tabs[i]
                    .active_cmd
                    .as_ref()
                    .unwrap()
                    .preview_hidden_handles(),
                &[insert]
            );
            assert_eq!(app.tabs[i].scene.document, before);
            let _ = app.update(Message::CommandEscape);
            assert_eq!(app.tabs[i].scene.document, before);
            assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
            select(&mut app);
            click(&mut app, alignment.anchor);
            click(&mut app, destination);
            assert!(app.tabs[i].active_cmd.is_none());
            let aligned = app.tabs[i].scene.alignment_grips(insert).remove(0);
            assert!(
                aligned.anchor.distance(destination) < 1e-4,
                "{testing} {perpendicular}: {:?}",
                aligned.anchor
            );
            if perpendicular {
                assert!(aligned.direction.y.abs() < 1e-6);
            } else {
                assert!(aligned.direction.x.abs() < 1e-6);
            }
            assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
            assert_eq!(
                app.tabs[i].scene.document.get_entity(other),
                before.get_entity(other)
            );
            assert_eq!(app.tabs[i].scene.document.objects, before.objects);
            let changed = app.tabs[i]
                .scene
                .document
                .get_entity(insert)
                .unwrap()
                .clone();
            app.undo_steps(1);
            assert_eq!(
                app.tabs[i].scene.document.get_entity(insert),
                before.get_entity(insert)
            );
            app.redo_steps(1);
            assert_eq!(
                app.tabs[i].scene.document.get_entity(insert),
                Some(&changed)
            );
            let doc = &app.tabs[i].scene.document;
            for saved in [
                document::DwgReader::from_stream(Cursor::new(
                    document::DwgWriter::write_to_vec(doc).unwrap(),
                ))
                .read()
                .unwrap(),
                document::DxfReader::from_reader(Cursor::new(
                    document::DxfWriter::new(doc).write_to_vec().unwrap(),
                ))
                .unwrap()
                .read()
                .unwrap(),
            ] {
                let restored =
                    crate::scene::block_alignment::grips(&saved, definition, insert).remove(0);
                assert!(restored.anchor.distance(aligned.anchor) < 1e-8);
                assert!(restored.direction.distance(aligned.direction) < 1e-8);
                assert_eq!(restored.perpendicular, perpendicular);
            }
            if testing {
                assert_eq!(app.tabs[source].scene.document, source_before);
            }
        }
    }
}

#[test]
fn block_alignment_runtime_off_target_flip_preview_and_history() {
    use glam::DVec3;
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let _ = app.run_command_line("BPARAMETER ALIGNMENT 0,0 10,0");
    let definition = app.tabs[app.active_tab]
        .active_block_edit_session()
        .unwrap()
        .br_handle;
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
    let preview_grip =
        crate::scene::block_alignment::grips(&preview_doc, definition, reference).remove(0);
    assert!(preview_grip.anchor.distance(destination) < 1e-8);
    assert!(
        preview_grip.direction.distance(-alignment.direction) < 1e-8,
        "off-target Flip must reverse preview direction by PI"
    );
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
            assert_eq!(
                app.tabs[i]
                    .scene
                    .document
                    .get_entity(entity.common().handle),
                Some(entity)
            );
        }
    }
    assert_eq!(app.tabs[i].scene.document.objects, before.objects);
    let changed = app.tabs[i]
        .scene
        .document
        .get_entity(reference)
        .unwrap()
        .clone();
    app.undo_steps(1);
    assert_eq!(
        app.tabs[i].scene.document.get_entity(reference),
        before.get_entity(reference)
    );
    app.redo_steps(1);
    assert_eq!(
        app.tabs[i].scene.document.get_entity(reference),
        Some(&changed)
    );
}

#[test]
fn block_alignment_runtime_rejects_stale_sources_and_reevaluates_targets() {
    use glam::DVec3;
    let (mut app, reference) = fixture();
    begin(&mut app, reference);
    let _ = app.run_command_line("BPARAMETER ALIGNMENT 0,0 10,0");
    let _ = app.dispatch_command("BSAVE");
    let _ = app.dispatch_command("BCLOSE");
    let i = app.active_tab;
    app.tabs[i].scene.select_entity(reference, true);
    app.tabs[i].selected_handle = Some(reference);
    let target = app.tabs[i]
        .scene
        .document
        .add_entity(EntityType::Line(Line::from_points(
            Vector3::new(30., -20., 0.),
            Vector3::new(30., 20., 0.),
        )))
        .unwrap();
    let alignment = app.tabs[i].scene.alignment_grips(reference).remove(0);
    let document = app.tabs[i].id;
    let cursor = DVec3::new(30., 4., 0.);
    let depth = app.tabs[i].history.undo_stack.len();
    app.tabs[i]
        .scene
        .document
        .layers
        .get_mut("0")
        .unwrap()
        .lock();
    assert!(app.start_test_action_grip(reference, alignment.id));
    assert!(app.tabs[i].active_cmd.is_none());
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    app.tabs[i]
        .scene
        .document
        .layers
        .get_mut("0")
        .unwrap()
        .unlock();
    assert!(app.start_test_action_grip(reference, alignment.id));
    assert!(app.tabs[i]
        .active_cmd
        .as_mut()
        .unwrap()
        .on_text_input("Flip")
        .is_some());
    let result = app.tabs[i].active_cmd.as_mut().unwrap().on_point(cursor);
    let EntityType::Insert(native) = app.tabs[i]
        .scene
        .document
        .get_entity_mut(reference)
        .unwrap()
    else {
        panic!()
    };
    native.rotation = 0.2;
    let before = app.tabs[i].scene.document.clone();
    let _ = app.handle_block_alignment(result);
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    let current = app.tabs[i].scene.alignment_grips(reference).remove(0);
    assert!(app.start_test_action_grip(reference, current.id));
    let result = app.tabs[i].active_cmd.as_mut().unwrap().on_point(cursor);
    app.tabs[i]
        .scene
        .document
        .layers
        .get_mut("0")
        .unwrap()
        .lock();
    let before = app.tabs[i].scene.document.clone();
    let _ = app.handle_block_alignment(result);
    assert_eq!(app.tabs[i].scene.document, before);
    assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
    app.tabs[i]
        .scene
        .document
        .layers
        .get_mut("0")
        .unwrap()
        .unlock();
    app.tabs[i].scene.select_entity(reference, true);
    app.tabs[i].selected_handle = Some(reference);
    assert!(app.start_test_action_grip(reference, current.id));
    let result = app.tabs[i].active_cmd.as_mut().unwrap().on_point(cursor);
    // A target hidden after preview must not impose its old direction at commit.
    app.tabs[i].scene.select_entity(target, true);
    app.tabs[i].scene.hide_selected();
    let _ = app.handle_block_alignment(result);
    let after = app.tabs[i].scene.alignment_grips(reference).remove(0);
    assert!(after.anchor.distance(cursor) < 1e-8);
    assert!(after.direction.distance(current.direction) < 1e-8);
    app.undo_steps(1);
    app.tabs[i].selected_handle = Some(reference);
    let before = app.tabs[i].scene.document.clone();
    let _ = app.handle_block_alignment(crate::command::CmdResult::AlignBlockReference {
        document: document + 1,
        alignment: Box::new(current),
        cursor,
        context: None,
        flipped: false,
    });
    assert_eq!(app.tabs[i].scene.document, before);
}
