use super::*;
use crate::scene::block_orientation_parameter as orientation;
use glam::DVec3;

#[test]
fn block_alignment_insertion_command_paths_refresh_geometry_preserve_history_and_roundtrip() {
    for palette in [false, true] {
        for stale in [false, true] {
            let (mut app, reference) = fixture();
            begin(&mut app, reference);
            let i = app.active_tab;
            let def = app.tabs[i].active_block_edit_session().unwrap().br_handle;
            orientation::create(
                &mut app.tabs[i].scene.document,
                def,
                orientation::Kind::Alignment,
                Vector3::ZERO,
                Vector3::UNIT_X,
                Vector3::ZERO,
            )
            .unwrap();
            let _ = app.dispatch_command("BSAVE");
            let _ = app.dispatch_command("BCLOSE");
            let target = app.tabs[i]
                .scene
                .document
                .add_entity(EntityType::Line(codec::entities::Line::from_points(
                    Vector3::new(200., 0., 0.),
                    Vector3::new(200., 100., 0.),
                )))
                .unwrap();
            app.tabs[i].scene.bump_geometry();
            app.tabs[i].scene.selection.borrow_mut().view.vp_size = (1000., 800.);
            app.tabs[i].scene.camera.borrow_mut().fit_to_bounds_f64(
                DVec3::new(-10., -10., 0.),
                DVec3::new(220., 110., 0.),
                1.25,
            );
            if palette {
                let _ = app.palette_insert(
                    crate::ui::window::block_palette::Item::Current("EDIT_ME".into()),
                    None,
                );
            } else {
                let _ = app.dispatch_command("-INSERT");
                feed(&mut app, "EDIT_ME");
            }
            app.refresh_command_point_pick_context(i);
            let point = DVec3::new(200., 20., 0.);
            let preview = app.tabs[i]
                .active_cmd
                .as_mut()
                .unwrap()
                .on_preview_wires(point);
            assert!(!preview.is_empty());
            let depth = app.tabs[i].history.undo_stack.len();
            if stale {
                app.tabs[i].scene.document.remove_entity(target);
                app.tabs[i].scene.bump_geometry();
            }
            let before = app.tabs[i].scene.document.clone();
            let _ = app.feed_command(crate::command::StepInput::Point(point));
            if !palette {
                for _ in 0..3 {
                    let _ = app.feed_command(crate::command::StepInput::Enter);
                }
            }
            let insert = app.tabs[i]
                .scene
                .document
                .entities()
                .find_map(|entity| match entity {
                    EntityType::Insert(ins)
                        if ins.common.handle != reference && ins.block_name == "EDIT_ME" =>
                    {
                        Some(ins.clone())
                    }
                    _ => None,
                })
                .unwrap();
            let expected = if stale {
                0.
            } else {
                std::f64::consts::FRAC_PI_2
            };
            assert!((insert.rotation.abs() - expected).abs() < 1e-8);
            assert!(
                DVec3::new(
                    insert.insert_point.x,
                    insert.insert_point.y,
                    insert.insert_point.z
                )
                .distance(point)
                    < 1e-8,
                "palette={palette} stale={stale}: {:?}",
                insert.insert_point
            );
            assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
            let authored = app.tabs[i].scene.document.clone();
            app.undo_steps(1);
            let undone = &app.tabs[i].scene.document;
            assert!(undone.get_entity(insert.common.handle).is_none());
            for entity in before.entities() {
                assert_eq!(undone.get_entity(entity.common().handle), Some(entity));
            }
            // Undo preserves the monotonic handle allocator, while restoring content.
            assert!(undone.objects == before.objects);
            assert_eq!(
                undone.block_records.get("EDIT_ME"),
                before.block_records.get("EDIT_ME")
            );
            app.redo_steps(1);
            assert_eq!(
                app.tabs[i].scene.document.get_entity(insert.common.handle),
                authored.get_entity(insert.common.handle)
            );
            assert!(app.tabs[i].scene.document.objects == authored.objects);
            for extension in ["dwg", "dxf"] {
                let bytes =
                    crate::io::save_to_bytes(&authored, extension, authored.version).unwrap();
                let loaded =
                    crate::io::load_bytes(&format!("aligned-insert.{extension}"), bytes).unwrap();
                let EntityType::Insert(actual) = loaded.get_entity(insert.common.handle).unwrap()
                else {
                    panic!()
                };
                assert!((actual.rotation - insert.rotation).abs() < 1e-8);
                assert!(actual.insert_point.distance(&insert.insert_point) < 1e-8);
            }
            if palette {
                let _ = app.palette_insert(
                    crate::ui::window::block_palette::Item::Current("EDIT_ME".into()),
                    None,
                );
            } else {
                let _ = app.dispatch_command("-INSERT");
                feed(&mut app, "EDIT_ME");
            }
            let before = app.tabs[i].scene.document.clone();
            let depth = app.tabs[i].history.undo_stack.len();
            let _ = app.update(Message::CommandEscape);
            assert_eq!(app.tabs[i].scene.document, before);
            assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
        }
    }
}
