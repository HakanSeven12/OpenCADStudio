use super::*;
use crate::scene::{block_distance_parameter as distance, Scene};
use codec::{
    entities::{Insert, Line},
    DwgReader, DwgWriter, DxfReader, DxfWriter,
};
use instance::ParameterFrame;
use std::io::Cursor;

fn polar(doc: &Document, action: Handle) -> &BlockPolarStretchAction {
    let ObjectType::DynamicBlock(object) = &doc.objects[&action] else {
        panic!()
    };
    let DynamicBlockData::PolarStretchAction(action) = &object.data else {
        panic!()
    };
    action
}

fn geometry(doc: &Document, insert: Handle) -> Vec<(DVec3, DVec3)> {
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

fn fixture(
    key: XyKeyPoint,
) -> (
    Document,
    Handle,
    Handle,
    Handle,
    Vec<Handle>,
    Handle,
    Handle,
) {
    let end_pivot = key == XyKeyPoint::Base;
    let mut scene = Scene::new();
    let lines = [
        (Vector3::new(2., 3., 0.), Vector3::new(12., 3., 0.)),
        if end_pivot {
            (Vector3::new(2., 3., 0.), Vector3::new(3., 4., 0.))
        } else {
            (Vector3::new(11., 3., 0.), Vector3::new(12., 4., 0.))
        },
        (Vector3::new(8., 5., 0.), Vector3::new(9., 5., 0.)),
        (Vector3::new(30., 30., 0.), Vector3::new(31., 30., 0.)),
    ];
    scene.define_block_raw(
        "POLAR_FRAME",
        Vector3::ZERO,
        lines
            .into_iter()
            .map(|(a, b)| EntityType::Line(Line::from_points(a, b)))
            .collect(),
    );
    let record = scene.document.block_records.get("POLAR_FRAME").unwrap();
    let (definition, targets) = (record.handle, record.entity_handles.clone());
    let mut doc = scene.document;
    let parameter = distance::create(
        &mut doc,
        definition,
        distance::Kind::Polar,
        Vector3::new(2., 3., 0.),
        Vector3::new(12., 3., 0.),
        Vector3::new(7., 5., 0.),
    )
    .unwrap();
    let frame = if end_pivot {
        [DVec3::new(1., 2., 0.), DVec3::new(4., 5., 0.)]
    } else {
        [DVec3::new(10., 2., 0.), DVec3::new(13., 5., 0.)]
    };
    let action = create_with_options(
        &mut doc,
        definition,
        Kind::PolarStretch,
        parameter,
        Some(key),
        &targets[..3],
        DVec3::new(20., 0., 0.),
        Some(frame),
        None,
    )
    .unwrap();
    options::set_rotate_only(&mut doc, definition, action, &targets[2..3]).unwrap();
    let a = doc
        .add_entity(EntityType::Insert(Insert::new(
            "POLAR_FRAME",
            Vector3::ZERO,
        )))
        .unwrap();
    let b = doc
        .add_entity(EntityType::Insert(Insert::new(
            "POLAR_FRAME",
            Vector3::new(100., 100., 0.),
        )))
        .unwrap();
    (doc, definition, parameter, action, targets, a, b)
}

#[test]
fn block_polar_stretch_base_and_end_bindings_evaluate_complete_reference_frames() {
    for key in [XyKeyPoint::Base, XyKeyPoint::End] {
        let (doc, definition, parameter, action, targets, a, b) = fixture(key);
        assert_eq!(
            polar(&doc, action)
                .bindings
                .iter()
                .find(|binding| binding.handle == targets[0])
                .unwrap()
                .indexes,
            vec![if key == XyKeyPoint::End { 1 } else { 0 }]
        );
        let requested = ParameterFrame {
            base: DVec3::new(4., 5., 0.),
            end: DVec3::new(4., 20., 0.),
        };
        let expected_rotation = if key == XyKeyPoint::End {
            DVec3::new(2., 11., 0.)
        } else {
            DVec3::new(2., 16., 0.)
        };
        let expected_full = if key == XyKeyPoint::End {
            (DVec3::new(4., 19., 0.), DVec3::new(3., 20., 0.))
        } else {
            (DVec3::new(4., 5., 0.), DVec3::new(3., 6., 0.))
        };
        let check = |doc: &Document| {
            let result = geometry(doc, a);
            assert!(
                result[0].0.distance(requested.base) < 1e-8,
                "{key:?}: {:?}",
                result[0]
            );
            assert!(result[0].1.distance(requested.end) < 1e-8);
            assert!(result[1].0.distance(expected_full.0) < 1e-8);
            assert!(result[1].1.distance(expected_full.1) < 1e-8);
            assert!(result[2].0.distance(expected_rotation) < 1e-8);
            assert!((result[2].0.distance(result[2].1) - 1.).abs() < 1e-8);
            assert_eq!(
                result[3],
                (DVec3::new(30., 30., 0.), DVec3::new(31., 30., 0.))
            );
        };
        let scratch = instance::evaluate_test_value(
            &doc,
            &doc,
            definition,
            a,
            parameter,
            Value::Frame {
                base: requested.base,
                end: requested.end,
            },
        )
        .unwrap()
        .unwrap();
        check(&scratch);
        let scratch_reset = instance::evaluate_test_value(
            &scratch,
            &doc,
            definition,
            a,
            parameter,
            Value::Frame {
                base: DVec3::new(2., 3., 0.),
                end: DVec3::new(12., 3., 0.),
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(geometry(&scratch_reset, a), geometry(&doc, a));
        let changed = instance::evaluate_frame(&doc, a, parameter, requested)
            .unwrap()
            .unwrap();
        for target in &targets {
            assert_eq!(changed.get_entity(*target), doc.get_entity(*target));
        }
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
            check(&saved);
            assert_eq!(polar(&saved, action), polar(&doc, action));
            assert_eq!(geometry(&saved, b), geometry(&doc, b));
            check(&instance::refresh(&saved, definition).unwrap());
            assert!(instance::evaluate_frame(&saved, a, parameter, requested)
                .unwrap()
                .is_none());
            let reset = instance::evaluate_frame(
                &saved,
                a,
                parameter,
                ParameterFrame {
                    base: DVec3::new(2., 3., 0.),
                    end: DVec3::new(12., 3., 0.),
                },
            )
            .unwrap()
            .unwrap();
            assert_eq!(geometry(&reset, a), geometry(&doc, a));
            check(
                &instance::evaluate_frame(&reset, a, parameter, requested)
                    .unwrap()
                    .unwrap(),
            );
        }
    }
}

#[test]
fn block_polar_stretch_reassociation_preserves_end_key_and_rejects_mixed_bindings() {
    let (mut doc, definition, parameter, action, _, a, _) = fixture(XyKeyPoint::End);
    let expected = polar(&doc, action).clone();
    let ObjectType::DynamicBlock(object) = doc.objects.get_mut(&action).unwrap() else {
        panic!()
    };
    let DynamicBlockData::PolarStretchAction(native) = &mut object.data else {
        panic!()
    };
    for connection in &mut native.connections {
        connection.code = -1;
    }
    assert!(association::associate(&mut doc, definition, action, parameter, None).unwrap());
    assert_eq!(polar(&doc, action), &expected);
    for invalid in 0..4 {
        let mut malformed = doc.clone();
        let ObjectType::DynamicBlock(object) = malformed.objects.get_mut(&action).unwrap() else {
            panic!()
        };
        let DynamicBlockData::PolarStretchAction(native) = &mut object.data else {
            panic!()
        };
        match invalid {
            0 => native.connections[0].name = "BaseXDelta".into(),
            1 => native.connections[1].code += 1,
            2 => native.connections[4].name = "Base".into(),
            _ => native.connections[5].code += 1,
        }
        let before = malformed.clone();
        assert!(instance::evaluate(&malformed, a, parameter, DVec3::new(2., 18., 0.)).is_err());
        assert_eq!(malformed, before);
    }
    let before = doc.clone();
    assert!(create_with_options(
        &mut doc,
        definition,
        Kind::PolarStretch,
        parameter,
        Some(XyKeyPoint::X),
        &expected.action.dependencies,
        DVec3::ZERO,
        Some([DVec3::ZERO, DVec3::ONE]),
        None
    )
    .is_err());
    assert_eq!(doc, before);
}

#[test]
fn block_polar_stretch_key_point_overrides_do_not_displace_rotate_only_targets() {
    for key in [XyKeyPoint::Base, XyKeyPoint::End] {
        let (mut doc, _, parameter, action, _, a, _) = fixture(key);
        let definition = instance::definition(&doc, a).unwrap();
        options::set(
            &mut doc,
            definition,
            action,
            2.,
            std::f64::consts::FRAC_PI_2,
        )
        .unwrap();
        let changed = instance::evaluate_frame(
            &doc,
            a,
            parameter,
            ParameterFrame {
                base: DVec3::new(4., 5., 0.),
                end: DVec3::new(4., 20., 0.),
            },
        )
        .unwrap()
        .unwrap();
        let result = geometry(&changed, a);
        if key == XyKeyPoint::End {
            assert!(result[0].0.distance(DVec3::new(4., 5., 0.)) < 1e-8);
            assert!(result[0].1.distance(DVec3::new(-6., 15., 0.)) < 1e-8);
            assert!(result[2].0.distance(DVec3::new(2., 11., 0.)) < 1e-8);
        } else {
            assert!(result[0].0.distance(DVec3::new(14., 10., 0.)) < 1e-8);
            assert!(result[0].1.distance(DVec3::new(4., 20., 0.)) < 1e-8);
            assert!(result[2].0.distance(DVec3::new(2., 16., 0.)) < 1e-8);
        }
    }
}

#[test]
#[ignore = "requires OCS_DYNAMIC_BLOCK_FIXTURES with local DXF fixtures"]
fn block_polar_stretch_imported_end_connections_roundtrip() {
    let root = std::path::PathBuf::from(
        std::env::var_os("OCS_DYNAMIC_BLOCK_FIXTURES").expect("fixture directory"),
    );
    let mut count = 0;
    for name in ["straight_stair.dxf", "u_shaped_stair.dxf"] {
        let bytes = std::fs::read(root.join("DXF-2018").join(name)).unwrap();
        let doc = DxfReader::from_reader(Cursor::new(bytes))
            .unwrap()
            .read()
            .unwrap();
        let actions = doc
            .objects
            .iter()
            .filter_map(|(handle, object)| match object {
                ObjectType::DynamicBlock(object) => match &object.data {
                    DynamicBlockData::PolarStretchAction(action) => Some((*handle, action.clone())),
                    _ => None,
                },
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(!actions.is_empty(), "{name}");
        for (_, action) in &actions {
            assert_eq!(
                stretch::polar_key_point(action).unwrap(),
                XyKeyPoint::End,
                "{name}"
            );
            count += 1;
        }
        // Exercise the imported two-vertex target through the production frame
        // transform. Its first vertex is the driven end and its second is fixed.
        for (handle, action) in &actions {
            let ObjectType::DynamicBlock(owner) = &doc.objects[handle] else {
                panic!()
            };
            let definition = doc
                .block_records
                .iter()
                .find(|record| {
                    authoring::graph(&doc, record.handle).ok().flatten() == Some(owner.owner)
                })
                .unwrap()
                .handle;
            let geometric = super::targets::geometry(
                &doc,
                definition,
                Kind::PolarStretch,
                action.connections[0].code,
                &action.action.dependencies,
            )
            .unwrap();
            assert_eq!(geometric.len(), 2);
            assert_eq!(action.action.dependencies.len(), 3);
            assert!(geometric
                .iter()
                .all(|handle| doc.get_entity(*handle).is_some()));
            let parameter = doc
                .objects
                .values()
                .find_map(|object| match object {
                    ObjectType::DynamicBlock(object) if object.owner == owner.owner => {
                        match &object.data {
                            DynamicBlockData::PolarParameter(p)
                                if p.parameter.parameter.element.eval.node_id
                                    == action.connections[0].code =>
                            {
                                Some(&p.parameter)
                            }
                            _ => None,
                        }
                    }
                    _ => None,
                })
                .unwrap();
            let base = v(parameter.definition_base_point);
            let end = v(parameter.definition_end_point);
            let mut checked = false;
            for binding in &action.bindings {
                let mut entity = doc.get_entity(binding.handle).unwrap().clone();
                let points = stretch::control_points(&entity).unwrap();
                if points.len() != 2
                    || points[0].distance(end) > 1e-8
                    || points[1].distance(base) > 1e-8
                {
                    continue;
                }
                stretch::apply_polar_frame(&mut entity, action, base, end, base, end + DVec3::Y)
                    .unwrap();
                let points = stretch::control_points(&entity).unwrap();
                assert!(points[0].distance(end + DVec3::Y) < 1e-8);
                assert!(points[1].distance(base) < 1e-8);
                checked = true;
            }
            assert!(checked, "{name}: imported endpoint target");
        }
        for saved in [
            DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&doc).unwrap()))
                .read()
                .unwrap(),
            DxfReader::from_reader(Cursor::new(DxfWriter::new(&doc).write_to_vec().unwrap()))
                .unwrap()
                .read()
                .unwrap(),
        ] {
            for (handle, expected) in &actions {
                let actual = polar(&saved, *handle);
                assert_eq!(actual.connections, expected.connections);
                assert_eq!(actual.bindings, expected.bindings);
                assert_eq!(actual.handles, expected.handles);
                assert_eq!(actual.points, expected.points);
                assert_eq!(actual.distance_multiplier, expected.distance_multiplier);
                assert_eq!(actual.angle_offset, expected.angle_offset);
            }
        }
    }
    assert_eq!(count, 2);
}
