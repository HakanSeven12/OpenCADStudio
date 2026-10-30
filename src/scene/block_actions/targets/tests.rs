use super::*;
use crate::scene::{
    block_distance_parameter as distance, block_orientation_parameter as orientation,
    block_point_parameter as point,
};
use codec::{entities::Insert, DwgReader, DwgWriter, DxfReader, DxfWriter};
use std::io::Cursor;

struct Fixture {
    doc: Document,
    definition: Handle,
    parameter: Parameter,
    action: Handle,
    targets: Vec<Handle>,
    grips: Vec<Handle>,
    first: Handle,
    second: Handle,
    input: Value,
}

fn fixture(kind: Kind) -> Fixture {
    let (mut doc, definition, targets) = super::super::tests::fixture();
    let parameter = match kind {
        Kind::Move => point::create(
            &mut doc,
            definition,
            Vector3::ZERO,
            Vector3::new(2., 2., 0.),
        )
        .unwrap(),
        Kind::Rotate | Kind::Flip => orientation::create(
            &mut doc,
            definition,
            if kind == Kind::Rotate {
                orientation::Kind::Rotation
            } else {
                orientation::Kind::Flip
            },
            Vector3::ZERO,
            if kind == Kind::Rotate {
                Vector3::new(10., 0., 0.)
            } else {
                Vector3::new(0., 10., 0.)
            },
            Vector3::new(10., 0., 0.),
        )
        .unwrap(),
        _ => distance::create(
            &mut doc,
            definition,
            match kind {
                Kind::Stretch => distance::Kind::XY,
                Kind::PolarStretch | Kind::Scale => distance::Kind::Polar,
                _ => distance::Kind::Linear,
            },
            Vector3::ZERO,
            Vector3::new(10., if kind == Kind::Stretch { 10. } else { 0. }, 0.),
            Vector3::new(5., 3., 0.),
        )
        .unwrap(),
    };
    let max = *crate::scene::block_grips::config::choices(&doc, definition, parameter)
        .last()
        .unwrap();
    crate::scene::block_grips::config::set(&mut doc, definition, parameter, Some(max)).unwrap();
    let action = create_with_options(
        &mut doc,
        definition,
        kind,
        parameter,
        None,
        &targets[..1],
        DVec3::new(7., 8., 0.),
        needs_frame(kind).then_some([DVec3::new(0.5, -0.5, 0.), DVec3::new(1.5, 0.5, 0.)]),
        (kind == Kind::Array).then_some((5., 0.)),
    )
    .unwrap();
    let grips = crate::scene::block_grips::config::locations(&doc, definition, parameter)
        .unwrap()
        .into_iter()
        .map(|grip| grip.handle)
        .collect::<Vec<_>>();
    assert!(!grips.is_empty());
    let ObjectType::DynamicBlock(object) = doc.objects.get_mut(&action).unwrap() else {
        panic!()
    };
    let native = action_mut(&mut object.data).unwrap();
    let original = native.dependencies.clone();
    native.dependencies = grips.iter().copied().chain(original).collect();
    let parameter = parameters(&doc, definition, kind)
        .into_iter()
        .find(|p| p.handle == parameter)
        .unwrap();
    let first = doc
        .add_entity(EntityType::Insert(Insert::new("ACTIONS", Vector3::ZERO)))
        .unwrap();
    let second = doc
        .add_entity(EntityType::Insert(Insert::new(
            "ACTIONS",
            Vector3::new(50., 50., 0.),
        )))
        .unwrap();
    let input = match kind {
        Kind::Move => Value::Point(DVec3::new(3., 4., 0.)),
        Kind::Stretch => Value::Point(DVec3::new(15., 20., 0.)),
        Kind::PolarStretch => Value::Point(DVec3::new(0., 15., 0.)),
        Kind::Rotate => Value::Point(DVec3::new(0., 10., 0.)),
        Kind::Scale => Value::Point(DVec3::new(20., 0., 0.)),
        Kind::Flip => Value::Flip(true),
        Kind::Array => Value::Point(DVec3::new(15., 0., 0.)),
    };
    Fixture {
        doc,
        definition,
        parameter,
        action,
        targets,
        grips,
        first,
        second,
        input,
    }
}

fn lines(doc: &Document, insert: Handle) -> Vec<(DVec3, DVec3)> {
    let EntityType::Insert(reference) = doc.get_entity(insert).unwrap() else {
        panic!()
    };
    doc.block_records
        .get(&reference.block_name)
        .unwrap()
        .entity_handles
        .iter()
        .filter_map(|h| match doc.get_entity(*h) {
            Some(EntityType::Line(line)) => Some((v(line.start), v(line.end))),
            _ => None,
        })
        .collect()
}

#[test]
fn block_action_self_grip_dependencies_preserve_frames_and_native_records() {
    for kind in [
        Kind::Move,
        Kind::Stretch,
        Kind::PolarStretch,
        Kind::Rotate,
        Kind::Scale,
        Kind::Flip,
        Kind::Array,
    ] {
        let f = fixture(kind);
        let before = f
            .grips
            .iter()
            .map(|h| (*h, f.doc.objects[h].clone()))
            .collect::<Vec<_>>();
        let changed = instance::evaluate_value(&f.doc, f.first, f.parameter.handle, f.input)
            .unwrap()
            .unwrap();
        let expected = match kind {
            Kind::Move => (DVec3::new(4., 4., 0.), DVec3::new(5., 4., 0.)),
            Kind::Stretch => (DVec3::new(6., 10., 0.), DVec3::new(2., 0., 0.)),
            Kind::PolarStretch => (DVec3::new(0., 6., 0.), DVec3::new(0., 2., 0.)),
            Kind::Rotate => (DVec3::Y, DVec3::Y * 2.),
            Kind::Scale => (DVec3::X * 2., DVec3::X * 4.),
            Kind::Flip => (-DVec3::X, -DVec3::X * 2.),
            Kind::Array => (DVec3::X, DVec3::X * 2.),
        };
        for saved in [
            changed.clone(),
            DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&changed).unwrap()))
                .read()
                .unwrap(),
            DxfReader::from_reader(Cursor::new(
                DxfWriter::new(&changed).write_to_vec().unwrap(),
            ))
            .unwrap()
            .read()
            .unwrap(),
        ] {
            let check = |doc: &Document| {
                let actual = lines(doc, f.first);
                assert!(
                    actual[0].0.distance(expected.0) < 1e-8,
                    "{kind:?}: {:?}",
                    actual[0]
                );
                assert!(actual[0].1.distance(expected.1) < 1e-8);
                if kind == Kind::Array {
                    assert_eq!(actual.len(), 4);
                }
                assert_eq!(lines(doc, f.second), lines(&f.doc, f.second));
                for (handle, object) in &before {
                    assert_eq!(
                        doc.objects.get(handle),
                        Some(object),
                        "{kind:?} grip {handle}"
                    );
                }
                assert_eq!(doc.objects[&f.action], f.doc.objects[&f.action]);
            };
            check(&saved);
            check(&instance::refresh(&saved, f.definition).unwrap());
            assert!(
                instance::evaluate_value(&saved, f.first, f.parameter.handle, f.input)
                    .unwrap()
                    .is_none()
            );
            let reset = instance::evaluate_value(
                &saved,
                f.first,
                f.parameter.handle,
                if f.parameter.flip {
                    Value::Flip(false)
                } else {
                    Value::Point(f.parameter.end)
                },
            )
            .unwrap()
            .unwrap();
            for (handle, object) in &before {
                assert_eq!(reset.objects.get(handle), Some(object));
            }
            assert_eq!(
                lines(&reset, f.first).len(),
                if kind == Kind::Array { 3 } else { 1 }
            );
            check(
                &instance::evaluate_value(&reset, f.first, f.parameter.handle, f.input)
                    .unwrap()
                    .unwrap(),
            );
        }
        for handle in &f.targets {
            assert_eq!(changed.get_entity(*handle), f.doc.get_entity(*handle));
        }
    }
}

#[test]
fn block_action_selection_editing_and_erase_preserve_owned_grip_links() {
    for kind in [
        Kind::Move,
        Kind::Stretch,
        Kind::PolarStretch,
        Kind::Rotate,
        Kind::Scale,
        Kind::Flip,
        Kind::Array,
    ] {
        let mut f = fixture(kind);
        let before = f.doc.clone();
        assert!(!retarget(&mut f.doc, f.definition, f.action, &f.targets[..1], None).unwrap());
        assert_eq!(f.doc, before);
        assert!(retarget(&mut f.doc, f.definition, f.action, &f.targets[1..], None).unwrap());
        let action = actions(&f.doc, f.definition)
            .into_iter()
            .find(|(h, ..)| *h == f.action)
            .unwrap()
            .1;
        assert_eq!(
            action.dependencies,
            f.grips
                .iter()
                .copied()
                .chain(f.targets[1..].iter().copied())
                .collect::<Vec<_>>()
        );
        let changed = instance::evaluate_value(&f.doc, f.first, f.parameter.handle, f.input)
            .unwrap()
            .unwrap();
        assert_eq!(lines(&changed, f.first), lines(&f.doc, f.first));
        f.doc.remove_entity(f.targets[1]);
        prune_erased_targets(&mut f.doc, f.definition);
        assert_eq!(
            actions(&f.doc, f.definition)
                .into_iter()
                .find(|(h, ..)| *h == f.action)
                .unwrap()
                .1
                .dependencies,
            f.grips
        );
        // Metadata-only self links still validate action options before saving a frame.
        let _ = instance::evaluate_value(&f.doc, f.first, f.parameter.handle, f.input)
            .unwrap()
            .unwrap();
    }
}

#[test]
fn block_action_foreign_or_malformed_native_dependencies_fail_atomically() {
    for invalid in 0..4 {
        let mut f = fixture(Kind::Move);
        let foreign = point::create(
            &mut f.doc,
            f.definition,
            Vector3::new(10., 10., 0.),
            Vector3::new(12., 12., 0.),
        )
        .unwrap();
        let foreign_grip =
            crate::scene::block_grips::config::locations(&f.doc, f.definition, foreign).unwrap()[0]
                .handle;
        let target = match invalid {
            0 => foreign,
            1 => foreign_grip,
            2 => Handle::new(0xffff_ffff),
            _ => {
                let ObjectType::DynamicBlock(grip) = f.doc.objects.get_mut(&f.grips[0]).unwrap()
                else {
                    panic!()
                };
                grip.owner = Handle::NULL;
                f.grips[0]
            }
        };
        let ObjectType::DynamicBlock(object) = f.doc.objects.get_mut(&f.action).unwrap() else {
            panic!()
        };
        action_mut(&mut object.data)
            .unwrap()
            .dependencies
            .push(target);
        let baseline = f.doc.clone();
        assert!(evaluate_value(
            &mut f.doc,
            &baseline,
            f.definition,
            f.parameter.handle,
            f.input
        )
        .is_err());
        assert_eq!(f.doc, baseline);
        assert!(retarget(&mut f.doc, f.definition, f.action, &f.targets[..1], None).is_err());
        assert_eq!(f.doc, baseline);
        if invalid < 2 {
            prune_erased_targets(&mut f.doc, f.definition);
            assert!(actions(&f.doc, f.definition)
                .into_iter()
                .find(|(h, ..)| *h == f.action)
                .unwrap()
                .1
                .dependencies
                .contains(&target));
        }
    }
    let mut f = fixture(Kind::Move);
    let ObjectType::DynamicBlock(object) = f.doc.objects.get_mut(&f.action).unwrap() else {
        panic!()
    };
    let DynamicBlockData::MoveAction(action) = &mut object.data else {
        panic!()
    };
    action.action.dependencies = f.grips.clone();
    action.offsets.flags = 1;
    assert!(instance::evaluate_value(&f.doc, f.first, f.parameter.handle, f.input).is_err());
    let ObjectType::DynamicBlock(object) = f.doc.objects.get_mut(&f.action).unwrap() else {
        panic!()
    };
    let DynamicBlockData::MoveAction(action) = &mut object.data else {
        panic!()
    };
    action.offsets.flags = 0;
    action_mut(match f.doc.objects.get_mut(&f.action).unwrap() {
        ObjectType::DynamicBlock(object) => &mut object.data,
        _ => panic!(),
    })
    .unwrap()
    .action_ids
    .push(999);
    assert!(instance::evaluate_value(&f.doc, f.first, f.parameter.handle, f.input).is_err());
}
