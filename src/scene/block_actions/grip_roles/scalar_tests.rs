use super::*;
use crate::scene::{
    block_distance_parameter as distance,
    block_grips::{config, runtime},
    block_orientation_parameter as orientation,
};

fn fixture(kind: Kind, family: distance::Kind) -> (Document, Handle, Parameter, Handle) {
    let (mut doc, definition, targets) = super::super::tests::fixture();
    let handle = if matches!(kind, Kind::Rotate | Kind::Flip) {
        orientation::create(
            &mut doc,
            definition,
            if kind == Kind::Rotate {
                orientation::Kind::Rotation
            } else {
                orientation::Kind::Flip
            },
            Vector3::ZERO,
            Vector3::new(10., 10., 0.),
            Vector3::new(10., 10., 0.),
        )
        .unwrap()
    } else {
        distance::create(
            &mut doc,
            definition,
            family,
            Vector3::ZERO,
            Vector3::new(10., 10., 0.),
            Vector3::UNIT_Y,
        )
        .unwrap()
    };
    let count = if matches!(kind, Kind::Rotate | Kind::Flip) {
        1
    } else if matches!(family, distance::Kind::XY) {
        4
    } else {
        2
    };
    config::set(&mut doc, definition, handle, Some(count)).unwrap();
    let action = create(
        &mut doc,
        definition,
        kind,
        handle,
        &targets[..1],
        DVec3::ZERO,
    )
    .unwrap();
    let p = all_parameters(&doc, definition)
        .into_iter()
        .find(|p| p.handle == handle)
        .unwrap();
    (doc, definition, p, action)
}

fn connections(data: &mut DynamicBlockData) -> Vec<&mut BlockConnection> {
    match data {
        DynamicBlockData::ScaleAction(a) | DynamicBlockData::RotateAction(a) => a
            .connections
            .iter_mut()
            .chain(a.action.connections.iter_mut())
            .collect(),
        DynamicBlockData::ArrayAction(a) => a.connections.iter_mut().collect(),
        DynamicBlockData::FlipAction(a) => a.connections.iter_mut().collect(),
        _ => unreachable!(),
    }
}

#[test]
fn block_scalar_grip_binding_contracts_preserve_configured_roles_and_roundtrip() {
    for (kind, family) in [
        (Kind::Scale, distance::Kind::Linear),
        (Kind::Scale, distance::Kind::Polar),
        (Kind::Array, distance::Kind::Linear),
        (Kind::Array, distance::Kind::Polar),
        (Kind::Array, distance::Kind::XY),
        (Kind::Rotate, distance::Kind::Linear),
        (Kind::Flip, distance::Kind::Linear),
    ] {
        let (mut doc, def, p, action) = fixture(kind, family);
        let locations = config::locations(&doc, def, p.handle).unwrap();
        let first = &locations[0];
        let relocated = first.point + Vector3::new(2., 3., 0.);
        config::set_location(&mut doc, def, p.handle, first.role, relocated).unwrap();
        let expected = locations.iter().map(|g| g.role).collect::<Vec<_>>();
        let value = if p.flip {
            Value::Flip(false)
        } else {
            Value::Point(p.end)
        };
        let grips = runtime::action_grips(&doc, def, &p, value);
        assert_eq!(grips.iter().map(|g| g.role).collect::<Vec<_>>(), expected);
        assert!(grips[0].point.distance(v(relocated)) < 1e-8);
        for g in &grips {
            assert!(runtime::action_grip_available(&doc, def, p.handle, g.id));
        }
        for extension in ["dwg", "dxf"] {
            let bytes = crate::io::save_to_bytes(&doc, extension, doc.version).unwrap();
            let loaded =
                crate::io::load_bytes(&format!("scalar-bindings.{extension}"), bytes).unwrap();
            assert_eq!(
                runtime::action_grips(&loaded, def, &p, value)
                    .iter()
                    .map(|g| g.role)
                    .collect::<Vec<_>>(),
                expected
            );
        }
        // Every retained connection matters, including secondary base bindings.
        let ObjectType::DynamicBlock(object) = doc.objects.get_mut(&action).unwrap() else {
            panic!()
        };
        let n = connections(&mut object.data).len();
        for index in 0..n {
            for wrong_id in [false, true] {
                let mut invalid = doc.clone();
                let ObjectType::DynamicBlock(object) = invalid.objects.get_mut(&action).unwrap()
                else {
                    panic!()
                };
                let mut fields = connections(&mut object.data);
                if wrong_id {
                    fields[index].code = -1;
                } else {
                    fields[index].name = "UnsupportedInput".into();
                }
                assert!(
                    runtime::action_grips(&invalid, def, &p, value).is_empty(),
                    "{kind:?} connection {index}"
                );
                for g in &grips {
                    assert!(!runtime::action_grip_available(
                        &invalid, def, p.handle, g.id
                    ));
                }
                assert_eq!(
                    config::locations(&invalid, def, p.handle).unwrap().len(),
                    locations.len()
                );
                let mut staged = invalid.clone();
                let input = if p.flip {
                    Value::Flip(true)
                } else {
                    Value::Point(DVec3::new(20., 30., 0.))
                };
                assert!(evaluate_value(&mut staged, &invalid, def, p.handle, input).is_err());
                assert_eq!(staged, invalid);
            }
        }
        config::set(&mut doc, def, p.handle, Some(0)).unwrap();
        assert!(runtime::action_grips(&doc, def, &p, value).is_empty());
        assert!(mask(&doc, def, &p) != 0);
    }
}

#[test]
fn block_scalar_grip_masks_union_valid_actions_without_accepting_malformed_siblings() {
    let (mut doc, def, p, action) = fixture(Kind::Scale, distance::Kind::Linear);
    let ObjectType::DynamicBlock(object) = doc.objects.get_mut(&action).unwrap() else {
        panic!()
    };
    let DynamicBlockData::ScaleAction(a) = &mut object.data else {
        panic!()
    };
    a.action.connections.clear();
    assert_eq!(mask(&doc, def, &p), 0);
    let target = doc.block_records.get("ACTIONS").unwrap().entity_handles[0];
    create(&mut doc, def, Kind::Array, p.handle, &[target], DVec3::ZERO).unwrap();
    assert_eq!(mask(&doc, def, &p), 0b0011);
}
