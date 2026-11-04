use super::*;
use crate::scene::{
    block_actions as actions, block_distance_parameter as distance,
    block_orientation_parameter as orientation,
};
use glam::DVec3;

#[test]
fn transported_flip_grip_uses_reference_frame_in_drawing_and_test_block() {
    for testing in [false, true] {
        let (mut app, reference) = fixture();
        let source = app.active_tab;
        begin(&mut app, reference);
        let definition = app.tabs[source]
            .active_block_edit_session()
            .unwrap()
            .br_handle;
        let member = members(&app)[0];
        let doc = &mut app.tabs[source].scene.document;
        let parent = distance::create(
            doc,
            definition,
            distance::Kind::Linear,
            Vector3::ZERO,
            Vector3::new(0., 10., 0.),
            Vector3::UNIT_X,
        )
        .unwrap();
        let flip = orientation::create(
            doc,
            definition,
            orientation::Kind::Flip,
            Vector3::new(2., 3., 0.),
            Vector3::new(12., 3., 0.),
            Vector3::new(4., 6., 0.),
        )
        .unwrap();
        let codec::objects::ObjectType::DynamicBlock(o) = doc.objects.get_mut(&flip).unwrap()
        else {
            panic!()
        };
        let codec::objects::DynamicBlockData::FlipParameter(p) = &mut o.data else {
            panic!()
        };
        p.parameter.parameter.chain_actions = true;
        actions::create(
            doc,
            definition,
            actions::Kind::Move,
            parent,
            &[flip],
            DVec3::ZERO,
        )
        .unwrap();
        actions::create(
            doc,
            definition,
            actions::Kind::Flip,
            flip,
            &[member],
            DVec3::ZERO,
        )
        .unwrap();
        let p = actions::all_parameters(doc, definition)
            .into_iter()
            .find(|p| p.handle == flip)
            .unwrap();
        if testing {
            let _ = app.open_block_test(source);
        } else {
            let _ = app.dispatch_command("BSAVE");
            let _ = app.dispatch_command("BCLOSE");
        }
        let i = app.active_tab;
        let insert = app.tabs[i]
            .block_test
            .as_ref()
            .map_or(reference, |test| test.insert);
        app.tabs[i].scene.selected.insert(insert);
        app.tabs[i].selected_handle = Some(insert);
        app.refresh_properties();
        let grip = |app: &OpenCADStudio| {
            app.tabs[i]
                .selected_grips
                .iter()
                .find(|g| {
                    crate::scene::block_grips::runtime::decode(g.id)
                        .is_some_and(|(node, _)| node == p.id)
                })
                .unwrap()
                .clone()
        };
        let before = grip(&app);
        let doc = &app.tabs[i].scene.document;
        let input = actions::Value::Point(DVec3::new(0., 14., 0.));
        let changed = if testing {
            actions::instance::evaluate_test_value(
                doc,
                &app.tabs[i].block_test.as_ref().unwrap().baseline,
                definition,
                insert,
                parent,
                input,
            )
        } else {
            actions::instance::evaluate_value(doc, insert, parent, input)
        }
        .unwrap()
        .unwrap();
        app.tabs[i].scene.document = changed;
        app.tabs[i].scene.bump_geometry();
        app.refresh_properties();
        let moved = grip(&app);
        let EntityType::Insert(reference) = app.tabs[i].scene.document.get_entity(insert).unwrap()
        else {
            panic!()
        };
        let transform =
            crate::scene::render_graph::insert_transform(&app.tabs[i].scene.document, reference);
        let delta = actions::v(transform.apply_rotation(actions::native(DVec3::Y * 4.)));
        assert!((moved.world - before.world - delta).length() < 1e-8);
        assert!(app.start_test_action_grip(insert, moved.id));
        assert_eq!(
            actions::instance::value(&app.tabs[i].scene.document, insert, &p).unwrap(),
            actions::Value::Flip(true)
        );
        let toggled = grip(&app);
        assert!((toggled.world - moved.world).length() < 1e-8);
        assert!((toggled.dir.unwrap() + moved.dir.unwrap()).length() < 1e-6);
    }
}
