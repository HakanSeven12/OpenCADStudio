use super::*;
use crate::scene::{
    block_actions as actions, block_distance_parameter as distance,
    block_grips::{config, runtime},
    block_orientation_parameter as orientation,
};
use codec::objects::{DynamicBlockData, ObjectType};
use glam::DVec3;

#[test]
fn block_scalar_grips_reject_malformed_secondary_bindings_in_drawing_and_test_block() {
    for testing in [false, true] {
        for kind in [
            actions::Kind::Scale,
            actions::Kind::Array,
            actions::Kind::Rotate,
            actions::Kind::Flip,
        ] {
            let (mut app, reference) = fixture();
            begin(&mut app, reference);
            let source = app.active_tab;
            let def = app.tabs[source]
                .active_block_edit_session()
                .unwrap()
                .br_handle;
            let member = members(&app)[0];
            let doc = &mut app.tabs[source].scene.document;
            let parameter = if matches!(kind, actions::Kind::Rotate | actions::Kind::Flip) {
                orientation::create(
                    doc,
                    def,
                    if kind == actions::Kind::Rotate {
                        orientation::Kind::Rotation
                    } else {
                        orientation::Kind::Flip
                    },
                    Vector3::ZERO,
                    Vector3::new(10., 0., 0.),
                    Vector3::new(10., 0., 0.),
                )
                .unwrap()
            } else {
                distance::create(
                    doc,
                    def,
                    distance::Kind::Linear,
                    Vector3::ZERO,
                    Vector3::new(10., 0., 0.),
                    Vector3::UNIT_Y,
                )
                .unwrap()
            };
            let action =
                actions::create(doc, def, kind, parameter, &[member], DVec3::ZERO).unwrap();
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
            let p = actions::all_parameters(&app.tabs[i].scene.document, def)
                .into_iter()
                .find(|p| p.handle == parameter)
                .unwrap();
            let ids = app.tabs[i]
                .selected_grips
                .iter()
                .filter(|g| runtime::decode(g.id).is_some_and(|(node, _)| node == p.id))
                .map(|g| g.id)
                .collect::<Vec<_>>();
            assert!(!ids.is_empty(), "{kind:?}");
            let native_count = config::locations(&app.tabs[i].scene.document, def, parameter)
                .unwrap()
                .len();
            if kind != actions::Kind::Flip {
                assert!(app.start_test_action_grip(insert, ids[0]));
                assert!(app.tabs[i].active_cmd.is_some());
            }
            // Keep the primary parameter ID intact: only a secondary binding is invalid.
            let ObjectType::DynamicBlock(object) =
                app.tabs[i].scene.document.objects.get_mut(&action).unwrap()
            else {
                panic!()
            };
            match &mut object.data {
                DynamicBlockData::ScaleAction(a) | DynamicBlockData::RotateAction(a) => {
                    a.action.connections[1].code = -1
                }
                DynamicBlockData::ArrayAction(a) => a.connections[3].code = -1,
                DynamicBlockData::FlipAction(a) => a.connections[3].code = -1,
                _ => unreachable!(),
            }
            let invalid = app.tabs[i].scene.document.clone();
            if let Some(test) = &mut app.tabs[i].block_test {
                test.baseline = std::sync::Arc::new(invalid.clone());
            }
            let depth = app.tabs[i].history.undo_stack.len();
            if kind != actions::Kind::Flip {
                let _ =
                    app.feed_command(crate::command::StepInput::Point(DVec3::new(20., 20., 0.)));
                assert_eq!(app.tabs[i].scene.document, invalid);
                assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
            }
            for id in &ids {
                assert!(!app.start_test_action_grip(insert, *id));
            }
            let _ =
                app.handle_block_action_result(crate::command::CmdResult::EvaluateBlockParameter {
                    grip: Some(ids[0]),
                    document: app.tabs[i].id,
                    definition: def,
                    parameter,
                    insert: (!testing).then_some(insert),
                    value: if kind == actions::Kind::Flip {
                        actions::Value::Flip(true)
                    } else {
                        actions::Value::Point(DVec3::new(20., 20., 0.))
                    },
                });
            assert_eq!(app.tabs[i].scene.document, invalid);
            assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
            app.tabs[i].selected_grips.clear();
            app.tabs[i].selected_grip_handles.clear();
            app.refresh_test_action_grips([0.; 3]);
            assert!(!app.tabs[i]
                .selected_grips
                .iter()
                .any(|g| runtime::decode(g.id).is_some_and(|(node, _)| node == p.id)));
            assert_eq!(
                config::locations(&app.tabs[i].scene.document, def, parameter)
                    .unwrap()
                    .len(),
                native_count
            );
            if testing {
                assert_eq!(app.tabs[source].scene.document, source_before);
            }
        }
    }
}
