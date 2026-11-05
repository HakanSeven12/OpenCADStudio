use super::*;
use crate::scene::block_actions as actions;

#[test]
#[ignore = "requires an explicitly supplied dynamic-block fixture"]
fn imported_coupled_flip_reference_workflow() {
    let path = std::env::var("OCS_DYNAMIC_BLOCK_FIXTURE").expect("set OCS_DYNAMIC_BLOCK_FIXTURE");
    let doc = crate::io::load_bytes(&path, std::fs::read(&path).unwrap()).unwrap();
    let (insert, def) = doc
        .entities()
        .find_map(|e| {
            let EntityType::Insert(reference) = e else {
                return None;
            };
            let def = actions::instance::definition(&doc, reference.common.handle)?;
            (actions::all_parameters(&doc, def)
                .iter()
                .filter(|p| p.flip)
                .count()
                >= 2)
                .then_some((reference.common.handle, def))
        })
        .expect("fixture must contain a reference with at least two Flip parameters");
    let flips: Vec<_> = actions::all_parameters(&doc, def)
        .into_iter()
        .filter(|p| p.flip)
        .collect();
    for mask in 1..4 {
        for reverse in [false, true] {
            let mut app = OpenCADStudio::new_for_test();
            let i = app.active_tab;
            app.tabs[i].scene.document = doc.clone();
            let select = |app: &mut OpenCADStudio| {
                app.tabs[i].scene.selected.insert(insert);
                app.tabs[i].selected_handle = Some(insert);
                app.refresh_properties();
            };
            let mut order = flips.clone();
            if reverse {
                order.reverse();
            }
            for p in &order {
                let position = flips.iter().position(|f| f.handle == p.handle).unwrap();
                if mask & (1 << position) == 0 {
                    continue;
                }
                select(&mut app);
                let grip = app.tabs[i]
                    .selected_grips
                    .iter()
                    .find(|g| {
                        crate::scene::block_grips::runtime::decode(g.id)
                            .is_some_and(|(id, _)| id == p.id)
                    })
                    .expect("Flip grip must be displayed")
                    .clone();
                assert_eq!(grip.shape, crate::scene::model::object::GripShape::Arrow);
                assert!(app.start_test_action_grip(insert, grip.id));
                assert!(
                    app.command_line.last_error.is_none(),
                    "Flip {}: {:?}",
                    p.id,
                    app.command_line.last_error
                );
                assert_eq!(
                    actions::instance::value(&app.tabs[i].scene.document, insert, p).unwrap(),
                    actions::Value::Flip(true)
                );
                select(&mut app);
                let reversed = app.tabs[i]
                    .selected_grips
                    .iter()
                    .find(|marker| marker.id == grip.id)
                    .expect("Flip arrow must remain visible after toggling");
                assert!((reversed.dir.unwrap() + grip.dir.unwrap()).length() < 1e-8);
            }
            select(&mut app);
            let params = actions::all_parameters(&app.tabs[i].scene.document, def);
            let (grip, p) = app.tabs[i]
                .selected_grips
                .iter()
                .find_map(|g| {
                    let (id, _) = crate::scene::block_grips::runtime::decode(g.id)?;
                    let p = params.iter().find(|p| p.id == id && p.linear)?;
                    Some((g.clone(), p.clone()))
                })
                .expect("Linear grip must be displayed");
            let frame = actions::instance::frame(&app.tabs[i].scene.document, insert, &p).unwrap();
            let EntityType::Insert(reference) =
                app.tabs[i].scene.document.get_entity(insert).unwrap()
            else {
                panic!()
            };
            let transform = crate::scene::render_graph::insert_transform(
                &app.tabs[i].scene.document,
                reference,
            );
            let delta = actions::v(
                transform
                    .apply_rotation(actions::native((frame.end - frame.base).normalize() * 3.)),
            );
            assert!(app.start_test_action_grip(insert, grip.id));
            let _ = app.feed_command(crate::command::StepInput::Point(grip.world + delta));
            assert!(
                app.command_line.last_error.is_none(),
                "Linear {}: {:?}",
                p.id,
                app.command_line.last_error
            );
            let changed =
                actions::instance::frame(&app.tabs[i].scene.document, insert, &p).unwrap();
            assert!(changed.end.distance(frame.end) > 1e-6);
            assert!(
                (changed.end.distance(changed.base) - frame.end.distance(frame.base) - 3.).abs()
                    < 1e-6
            );
            let current = &app.tabs[i].scene.document;
            let refreshed = actions::instance::refresh(current, def).unwrap();
            for parameter in &params {
                assert_eq!(
                    actions::instance::frame(&refreshed, insert, parameter).unwrap(),
                    actions::instance::frame(current, insert, parameter).unwrap(),
                );
                assert_eq!(
                    actions::instance::value(&refreshed, insert, parameter).unwrap(),
                    actions::instance::value(current, insert, parameter).unwrap(),
                );
            }
            app.undo_steps(1);
            assert_eq!(
                actions::instance::frame(&app.tabs[i].scene.document, insert, &p).unwrap(),
                frame
            );
            app.redo_steps(1);
            assert_eq!(
                actions::instance::frame(&app.tabs[i].scene.document, insert, &p).unwrap(),
                changed
            );
            for extension in ["dwg", "dxf"] {
                let current = &app.tabs[i].scene.document;
                let bytes = crate::io::save_to_bytes(current, extension, current.version).unwrap();
                let saved = crate::io::load_bytes(&format!("coupled-reference.{extension}"), bytes)
                    .unwrap();
                assert_eq!(
                    actions::instance::frame(&saved, insert, &p).unwrap(),
                    changed
                );
                let mut reopened = OpenCADStudio::new_for_test();
                let j = reopened.active_tab;
                reopened.tabs[j].scene.document = saved.clone();
                for flip in order.iter().rev() {
                    let position = flips.iter().position(|f| f.handle == flip.handle).unwrap();
                    if mask & (1 << position) == 0 {
                        continue;
                    }
                    reopened.tabs[j].scene.selected.insert(insert);
                    reopened.tabs[j].selected_handle = Some(insert);
                    reopened.refresh_properties();
                    let grip = reopened.tabs[j]
                        .selected_grips
                        .iter()
                        .find(|g| {
                            crate::scene::block_grips::runtime::decode(g.id)
                                .is_some_and(|(id, _)| id == flip.id)
                        })
                        .unwrap()
                        .clone();
                    assert!(reopened.start_test_action_grip(insert, grip.id));
                    assert!(
                        reopened.command_line.last_error.is_none(),
                        "reopened Flip {}: {:?}",
                        flip.id,
                        reopened.command_line.last_error
                    );
                    assert_eq!(
                        actions::instance::value(&reopened.tabs[j].scene.document, insert, flip)
                            .unwrap(),
                        actions::Value::Flip(false)
                    );
                }
                let unflipped =
                    actions::instance::frame(&reopened.tabs[j].scene.document, insert, &p).unwrap();
                assert!(
                    (unflipped.end.distance(unflipped.base) - changed.end.distance(changed.base))
                        .abs()
                        < 1e-6
                );
                let reset = actions::instance::reset(&saved, insert).unwrap().unwrap();
                for original in &params {
                    assert_eq!(
                        actions::instance::frame(&reset, insert, original).unwrap(),
                        actions::instance::ParameterFrame {
                            base: original.base,
                            end: original.end
                        }
                    );
                }
            }
        }
    }
}
