use super::*;
use crate::scene::{
    block_distance_parameter as distance,
    block_grips::{config, runtime},
    block_point_parameter as point,
};

#[test]
fn block_displacement_grip_roles_follow_native_key_point_bindings() {
    for parameter_kind in 0..4 {
        for kind in [Kind::Move, Kind::Stretch, Kind::PolarStretch] {
            if kind == Kind::PolarStretch && parameter_kind != 2 {
                continue;
            }
            let (mut doc, definition, targets) = super::super::tests::fixture();
            let handle = if parameter_kind == 0 {
                point::create(&mut doc, definition, Vector3::ZERO, Vector3::UNIT_Y).unwrap()
            } else {
                distance::create(
                    &mut doc,
                    definition,
                    match parameter_kind {
                        1 => distance::Kind::Linear,
                        2 => distance::Kind::Polar,
                        _ => distance::Kind::XY,
                    },
                    Vector3::ZERO,
                    Vector3::new(10., 10., 0.),
                    Vector3::UNIT_Y,
                )
                .unwrap()
            };
            let count = if parameter_kind == 0 {
                1
            } else if parameter_kind == 3 {
                4
            } else {
                2
            };
            config::set(&mut doc, definition, handle, Some(count)).unwrap();
            let p = all_parameters(&doc, definition)
                .into_iter()
                .find(|p| p.handle == handle)
                .unwrap();
            assert_eq!(mask(&doc, definition, &p), 0);
            let keys = if parameter_kind == 0 {
                vec![(None, 1)]
            } else if parameter_kind == 3 {
                vec![
                    (Some(XyKeyPoint::Base), 0b1101),
                    (Some(XyKeyPoint::End), 0b1110),
                    (Some(XyKeyPoint::X), 0b0111),
                    (Some(XyKeyPoint::Y), 0b1011),
                ]
            } else {
                vec![(Some(XyKeyPoint::Base), 1), (Some(XyKeyPoint::End), 2)]
            };
            let baseline = doc.clone();
            let mut union_doc = doc.clone();
            let mut union_expected = 0;
            for (key, bit) in keys {
                let mut doc = baseline.clone();
                let frame = [DVec3::new(0.5, -0.5, 0.), DVec3::new(1.5, 0.5, 0.)];
                let action = create_with_options(
                    &mut doc,
                    definition,
                    kind,
                    handle,
                    key,
                    &targets[..1],
                    DVec3::ZERO,
                    needs_frame(kind).then_some(frame),
                    None,
                )
                .unwrap();
                let expected = bit;
                assert_eq!(mask(&doc, definition, &p), expected);
                create_with_options(
                    &mut union_doc,
                    definition,
                    kind,
                    handle,
                    key,
                    &targets[..1],
                    DVec3::ZERO,
                    needs_frame(kind).then_some(frame),
                    None,
                )
                .unwrap();
                union_expected |= bit;
                assert_eq!(mask(&union_doc, definition, &p), union_expected);
                let roles = runtime::action_grips(&doc, definition, &p, Value::Point(p.end))
                    .into_iter()
                    .map(|g| g.role)
                    .collect::<Vec<_>>();
                assert_eq!(
                    roles,
                    (0..4)
                        .filter(|role| expected & (1 << role) != 0)
                        .collect::<Vec<_>>()
                );
                assert_eq!(
                    config::locations(&doc, definition, handle).unwrap().len(),
                    count
                );
                for extension in ["dwg", "dxf"] {
                    let bytes = crate::io::save_to_bytes(&doc, extension, doc.version).unwrap();
                    let loaded =
                        crate::io::load_bytes(&format!("key-point.{extension}"), bytes).unwrap();
                    assert_eq!(mask(&loaded, definition, &p), expected);
                }
                let mut malformed = doc.clone();
                let ObjectType::DynamicBlock(object) = malformed.objects.get_mut(&action).unwrap()
                else {
                    panic!()
                };
                let connections = match &mut object.data {
                    DynamicBlockData::MoveAction(a) => &mut a.connections[..],
                    DynamicBlockData::StretchAction(a) => &mut a.connections[..],
                    DynamicBlockData::PolarStretchAction(a) => &mut a.connections[..],
                    _ => unreachable!(),
                };
                connections[1].code = -1;
                assert_eq!(mask(&malformed, definition, &p), 0);
                let unchanged = config::locations(&malformed, definition, handle).unwrap();
                assert_eq!(unchanged.len(), count);
            }
        }
    }
}

#[test]
fn block_xy_shared_coordinate_grips_drive_endpoint_actions() {
    for (role, cursor, translation) in [
        (2, DVec3::new(13., -4., 0.), DVec3::new(3., 0., 0.)),
        (3, DVec3::new(-4., 15., 0.), DVec3::new(0., 5., 0.)),
    ] {
        let (mut doc, definition, targets) = super::super::tests::fixture();
        let handle = distance::create(
            &mut doc,
            definition,
            distance::Kind::XY,
            Vector3::ZERO,
            Vector3::new(10., 10., 0.),
            Vector3::UNIT_Y,
        )
        .unwrap();
        config::set(&mut doc, definition, handle, Some(4)).unwrap();
        create_with_options(
            &mut doc,
            definition,
            Kind::Move,
            handle,
            Some(XyKeyPoint::End),
            &targets[..1],
            DVec3::ZERO,
            None,
            None,
        )
        .unwrap();
        let insert = doc
            .add_entity(EntityType::Insert(document::entities::Insert::new(
                "ACTIONS",
                Vector3::ZERO,
            )))
            .unwrap();
        let p = all_parameters(&doc, definition).remove(0);
        let grip = runtime::action_grips(&doc, definition, &p, Value::Point(p.end))
            .into_iter()
            .find(|g| g.role == role)
            .expect("shared coordinate grip");
        let input = runtime::Input {
            id: grip.id,
            role,
            frame: instance::ParameterFrame {
                base: p.base,
                end: p.end,
            },
            offset: grip.offset,
            point: false,
            linear_axis: None,
            limits: runtime::Limits::read(&doc, definition, &p),
            to_world: document::types::Transform::identity(),
            to_local: document::types::Transform::identity(),
        };
        let Value::Frame { base, end } = input.value(cursor + grip.offset) else {
            panic!()
        };
        let changed =
            instance::evaluate_frame(&doc, insert, handle, instance::ParameterFrame { base, end })
                .unwrap()
                .unwrap();
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
        assert!(v(line.start).distance(DVec3::X + translation) < 1e-8);
        assert_eq!(changed.get_entity(targets[0]), doc.get_entity(targets[0]));
    }
}

#[test]
fn block_key_point_synthetic_suppresses_unassociated_base_grip() {
    let (mut doc, definition, targets) = super::super::tests::fixture();
    let handle = distance::create(
        &mut doc,
        definition,
        distance::Kind::Linear,
        Vector3::ZERO,
        Vector3::new(10., 0., 0.),
        Vector3::UNIT_Y,
    )
    .unwrap();
    config::set(&mut doc, definition, handle, Some(2)).unwrap();
    let p = all_parameters(&doc, definition)
        .into_iter()
        .find(|p| p.handle == handle)
        .unwrap();
    assert_eq!(
        config::locations(&doc, definition, handle).unwrap().len(),
        2
    );

    // Associate an action ONLY with End key point (role 1)
    create_with_options(
        &mut doc,
        definition,
        Kind::Move,
        handle,
        Some(XyKeyPoint::End),
        &targets[..1],
        DVec3::ZERO,
        None,
        None,
    )
    .unwrap();

    assert_eq!(mask(&doc, definition, &p), 2);
    assert_eq!(
        runtime::action_grips(&doc, definition, &p, Value::Point(p.end))
            .into_iter()
            .map(|g| g.role)
            .collect::<Vec<_>>(),
        vec![1]
    );
    for extension in ["dwg", "dxf"] {
        let data = crate::io::save_to_bytes(&doc, extension, doc.version).unwrap();
        let loaded = crate::io::load_bytes(&format!("roles.{extension}"), data).unwrap();
        assert_eq!(mask(&loaded, definition, &p), 2);
        assert_eq!(
            config::locations(&loaded, definition, handle)
                .unwrap()
                .len(),
            2
        );
    }
}
