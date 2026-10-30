use super::*;
use crate::scene::block_distance_parameter as distance;
use codec::{DwgReader, DwgWriter, DxfReader, DxfWriter};
use instance::ParameterFrame;
use std::io::Cursor;

fn insert(doc: &mut Document) -> Handle {
    doc.add_entity(EntityType::Insert(codec::entities::Insert::new(
        "ACTIONS",
        Vector3::ZERO,
    )))
    .unwrap()
}

fn line(doc: &Document, insert: Handle) -> &codec::entities::Line {
    let EntityType::Insert(reference) = doc.get_entity(insert).unwrap() else {
        panic!()
    };
    doc.block_records
        .get(&reference.block_name)
        .unwrap()
        .entity_handles
        .iter()
        .find_map(|h| match doc.get_entity(*h) {
            Some(EntityType::Line(line)) => Some(line),
            _ => None,
        })
        .unwrap()
}

fn parameter(doc: &Document, definition: Handle, handle: Handle) -> Parameter {
    all_parameters(doc, definition)
        .into_iter()
        .find(|p| p.handle == handle)
        .unwrap()
}

fn distance(doc: &mut Document, definition: Handle, kind: distance::Kind) -> Handle {
    distance::create(
        doc,
        definition,
        kind,
        Vector3::ZERO,
        Vector3::new(10., if kind == distance::Kind::XY { 10. } else { 0. }, 0.),
        Vector3::new(5., 3., 0.),
    )
    .unwrap()
}

fn roundtrips(doc: &Document) -> Vec<Document> {
    vec![
        doc.clone(),
        DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(doc).unwrap()))
            .read()
            .unwrap(),
        DxfReader::from_reader(Cursor::new(DxfWriter::new(doc).write_to_vec().unwrap()))
            .unwrap()
            .read()
            .unwrap(),
    ]
}

#[test]
fn block_action_base_offsets_follow_frames_and_survive_native_rebuilds() {
    for kind in [Kind::Rotate, Kind::Scale] {
        for offset in [DVec3::new(3., 2., 0.), DVec3::new(-4., -3., 0.)] {
            let (mut doc, def, targets) = super::tests::fixture();
            let p = if kind == Kind::Scale {
                distance(&mut doc, def, distance::Kind::Linear)
            } else {
                crate::scene::block_orientation_parameter::create(
                    &mut doc,
                    def,
                    crate::scene::block_orientation_parameter::Kind::Rotation,
                    Vector3::ZERO,
                    Vector3::new(10., 0., 0.),
                    Vector3::new(10., 0., 0.),
                )
                .unwrap()
            };
            let action = create(
                &mut doc,
                def,
                kind,
                p,
                &targets[..1],
                DVec3::new(7., 8., 0.),
            )
            .unwrap();
            assert!(options::set_base_offset(&mut doc, def, action, offset).unwrap());
            assert!(!options::set_base_offset(&mut doc, def, action, offset).unwrap());
            let native = options::base(&doc, action).unwrap().clone();
            let before = doc.clone();
            for invalid in [DVec3::NAN, DVec3::INFINITY, DVec3::new(1., 2., 1.)] {
                assert!(options::set_base_offset(&mut doc, def, action, invalid).is_err());
                assert_eq!(doc, before);
            }
            assert!(options::set_base_offset(&mut doc, Handle::NULL, action, offset).is_err());
            let a = insert(&mut doc);
            let b = insert(&mut doc);
            let p = parameter(&doc, def, p);
            let base = DVec3::new(4., 5., 0.);
            let frame = ParameterFrame {
                base,
                end: base
                    + if kind == Kind::Scale {
                        DVec3::new(20., 0., 0.)
                    } else {
                        DVec3::new(0., 10., 0.)
                    },
            };
            let expected = if kind == Kind::Scale {
                DVec3::X * 2. - offset + base
            } else {
                offset + DVec3::new(offset.y, 1. - offset.x, 0.) + base
            };
            let scratch = instance::evaluate_test_value(
                &doc,
                &doc,
                def,
                a,
                p.handle,
                Value::Frame {
                    base: frame.base,
                    end: frame.end,
                },
            )
            .unwrap()
            .unwrap();
            assert!(v(line(&scratch, a).start).distance(expected) < 1e-8);
            let reset_scratch = instance::evaluate_test_value(
                &scratch,
                &doc,
                def,
                a,
                p.handle,
                Value::Frame {
                    base: p.base,
                    end: p.end,
                },
            )
            .unwrap()
            .unwrap();
            assert_eq!(line(&reset_scratch, a).start, Vector3::UNIT_X);
            let changed = instance::evaluate_frame(&doc, a, p.handle, frame)
                .unwrap()
                .unwrap();
            assert_eq!(changed.get_entity(targets[0]), doc.get_entity(targets[0]));
            for saved in roundtrips(&changed) {
                assert!(v(line(&saved, a).start).distance(expected) < 1e-8);
                assert_eq!(line(&saved, b).start, Vector3::UNIT_X);
                let EntityType::Line(source) = saved.get_entity(targets[0]).unwrap() else {
                    panic!()
                };
                assert_eq!(source.start, Vector3::UNIT_X);
                assert_eq!(source.end, Vector3::new(2., 0., 0.));
                assert_eq!(options::base(&saved, action), Some(&native));
                let refreshed = instance::refresh(&saved, def).unwrap();
                assert!(v(line(&refreshed, a).start).distance(expected) < 1e-8);
                assert!(instance::evaluate_frame(&saved, a, p.handle, frame)
                    .unwrap()
                    .is_none());
                let reset = instance::evaluate_frame(
                    &saved,
                    a,
                    p.handle,
                    ParameterFrame {
                        base: p.base,
                        end: p.end,
                    },
                )
                .unwrap()
                .unwrap();
                assert_eq!(line(&reset, a).start, Vector3::UNIT_X);
                let again = instance::evaluate_frame(&reset, a, p.handle, frame)
                    .unwrap()
                    .unwrap();
                assert!(v(line(&again, a).start).distance(expected) < 1e-8);
            }
            // Switching to an independent base preserves the current definition pivot.
            assert!(options::set_base(&mut doc, def, action, false, None).unwrap());
            let independent = options::base(&doc, action).unwrap();
            assert_eq!(v(independent.base_point), offset);
            assert_eq!(v(independent.offset), offset);
            assert!(options::set_base_offset(&mut doc, def, action, DVec3::ZERO).is_err());
            let independent = instance::evaluate_frame(&doc, a, p.handle, frame)
                .unwrap()
                .unwrap();
            assert!(v(line(&independent, a).start).distance(expected - base) < 1e-8);
        }
    }
}

#[test]
fn block_frame_move_and_stretch_evaluate_base_end_and_xy_corner_bindings() {
    for family in [
        distance::Kind::Linear,
        distance::Kind::Polar,
        distance::Kind::XY,
    ] {
        for kind in [Kind::Move, Kind::Stretch] {
            for key in [
                XyKeyPoint::Base,
                XyKeyPoint::End,
                XyKeyPoint::X,
                XyKeyPoint::Y,
            ] {
                if family != distance::Kind::XY && matches!(key, XyKeyPoint::X | XyKeyPoint::Y) {
                    continue;
                }
                let (mut doc, def, targets) = super::tests::fixture();
                let p = distance(&mut doc, def, family);
                create_with_options(
                    &mut doc,
                    def,
                    kind,
                    p,
                    Some(key),
                    &targets[..1],
                    DVec3::ZERO,
                    (kind == Kind::Stretch)
                        .then_some([DVec3::new(0., -2., 0.), DVec3::new(3., 2., 0.)]),
                    None,
                )
                .unwrap();
                let a = insert(&mut doc);
                let b = insert(&mut doc);
                let baseline = doc.clone();
                let y = if family == distance::Kind::Linear {
                    0.
                } else {
                    -3.
                };
                let frame = ParameterFrame {
                    base: DVec3::new(-2., y, 0.),
                    end: DVec3::new(
                        14.,
                        if family == distance::Kind::Linear {
                            0.
                        } else {
                            15.
                        },
                        0.,
                    ),
                };
                let p = parameter(&doc, def, p);
                let base_delta = frame.base - p.base;
                let end_delta = frame.end - p.end;
                let delta = match key {
                    XyKeyPoint::Base => base_delta,
                    XyKeyPoint::End => end_delta,
                    XyKeyPoint::X => DVec3::new(end_delta.x, base_delta.y, 0.),
                    XyKeyPoint::Y => DVec3::new(base_delta.x, end_delta.y, 0.),
                };
                let changed = instance::evaluate_frame(&doc, a, p.handle, frame)
                    .unwrap()
                    .unwrap();
                assert!(instance::evaluate_frame(&changed, a, p.handle, frame)
                    .unwrap()
                    .is_none());
                for saved in roundtrips(&changed) {
                    assert!(
                        line(&saved, a).start.distance(&native(DVec3::X + delta)) < 1e-8,
                        "{family:?} {kind:?} {key:?}"
                    );
                    let current = instance::frame(&saved, a, &p).unwrap();
                    assert!(current.base.distance(frame.base) < 1e-8);
                    assert!(current.end.distance(frame.end) < 1e-8);
                    assert_eq!(line(&saved, b).start, Vector3::UNIT_X);
                    let refreshed = instance::refresh(&saved, def).unwrap();
                    assert!(line(&refreshed, a).start.distance(&line(&saved, a).start) < 1e-8);
                    // A later endpoint-only input retains the changed base and all other frames.
                    let end = frame.end + DVec3::X;
                    let next = instance::evaluate(&saved, a, p.handle, end)
                        .unwrap()
                        .unwrap();
                    assert_eq!(instance::frame(&next, a, &p).unwrap().base, frame.base);
                    let reset = instance::reset(&saved, a).unwrap().unwrap();
                    assert_eq!(line(&reset, a).start, Vector3::UNIT_X);
                    assert_eq!(
                        instance::frame(&reset, a, &p).unwrap(),
                        ParameterFrame {
                            base: p.base,
                            end: p.end
                        }
                    );
                }
                for h in &targets {
                    assert_eq!(changed.get_entity(*h), baseline.get_entity(*h));
                }
                for (h, object) in &baseline.objects {
                    assert_eq!(changed.objects.get(h), Some(object));
                }
            }
        }
    }
}

#[test]
fn block_frame_scale_and_rotation_map_dependent_pivots_and_keep_independent_pivots() {
    for kind in [Kind::Scale, Kind::Rotate] {
        for independent in [false, true] {
            let (mut doc, def, targets) = super::tests::fixture();
            let p = if kind == Kind::Scale {
                distance(&mut doc, def, distance::Kind::Linear)
            } else {
                crate::scene::block_orientation_parameter::create(
                    &mut doc,
                    def,
                    crate::scene::block_orientation_parameter::Kind::Rotation,
                    Vector3::ZERO,
                    Vector3::new(10., 0., 0.),
                    Vector3::new(10., 0., 0.),
                )
                .unwrap()
            };
            let action = create(&mut doc, def, kind, p, &targets[..1], DVec3::ZERO).unwrap();
            if independent {
                options::set_base(&mut doc, def, action, false, Some(DVec3::new(2., 0., 0.)))
                    .unwrap();
            }
            let a = insert(&mut doc);
            let p = parameter(&doc, def, p);
            let frame = if kind == Kind::Scale {
                ParameterFrame {
                    base: DVec3::new(-5., 0., 0.),
                    end: DVec3::new(10., 0., 0.),
                }
            } else {
                ParameterFrame {
                    base: DVec3::new(3., 4., 0.),
                    end: DVec3::new(3., 14., 0.),
                }
            };
            let expected = match (kind, independent) {
                (Kind::Scale, false) => Vector3::new(-3.5, 0., 0.),
                (Kind::Scale, true) => Vector3::new(0.5, 0., 0.),
                (Kind::Rotate, false) => Vector3::new(3., 5., 0.),
                _ => Vector3::new(2., -1., 0.),
            };
            let changed = instance::evaluate_frame(&doc, a, p.handle, frame)
                .unwrap()
                .unwrap();
            for saved in roundtrips(&changed) {
                if kind == Kind::Rotate {
                    let properties = saved.dynamic_block_properties(def, Some(a)).unwrap();
                    assert!(properties.iter().any(|property| property.parameter_handle == p.handle
                        && matches!(property.value, BlockEvalValue::Real(angle) if (angle - std::f64::consts::FRAC_PI_2).abs() < 1e-8)));
                }
                assert!(line(&saved, a).start.distance(&expected) < 1e-8);
                assert!(
                    line(&instance::refresh(&saved, def).unwrap(), a)
                        .start
                        .distance(&expected)
                        < 1e-8
                );
            }
        }
    }
}

#[test]
fn block_frame_array_uses_updated_extent_and_polar_stretch_uses_updated_frame() {
    for kind in [Kind::Array, Kind::PolarStretch] {
        let (mut doc, def, targets) = super::tests::fixture();
        let p = distance(
            &mut doc,
            def,
            if kind == Kind::Array {
                distance::Kind::Linear
            } else {
                distance::Kind::Polar
            },
        );
        create_with_options(
            &mut doc,
            def,
            kind,
            p,
            None,
            &targets[..1],
            DVec3::ZERO,
            (kind == Kind::PolarStretch)
                .then_some([DVec3::new(0., -2., 0.), DVec3::new(3., 2., 0.)]),
            (kind == Kind::Array).then_some((5., 5.)),
        )
        .unwrap();
        let a = insert(&mut doc);
        let p = parameter(&doc, def, p);
        let frame = if kind == Kind::Array {
            ParameterFrame {
                base: DVec3::new(-5., 0., 0.),
                end: DVec3::new(10., 0., 0.),
            }
        } else {
            ParameterFrame {
                base: DVec3::new(3., 4., 0.),
                end: DVec3::new(3., 24., 0.),
            }
        };
        let changed = instance::evaluate_frame(&doc, a, p.handle, frame)
            .unwrap()
            .unwrap();
        if kind == Kind::Array {
            let EntityType::Insert(reference) = changed.get_entity(a).unwrap() else {
                panic!()
            };
            let lines = changed
                .block_records
                .get(&reference.block_name)
                .unwrap()
                .entity_handles
                .iter()
                .filter_map(|h| match changed.get_entity(*h) {
                    Some(EntityType::Line(l)) => Some(l.start.x),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(lines, vec![1., 6., 11., 16.]);
        } else {
            assert!(line(&changed, a).start.distance(&Vector3::new(3., 15., 0.)) < 1e-8);
            assert!(line(&changed, a).end.distance(&Vector3::new(3., 16., 0.)) < 1e-8);
        }
    }
}

#[test]
fn block_frames_preserve_other_parameters_flip_states_and_reference_isolation() {
    let (mut doc, def, targets) = super::tests::fixture();
    let xy = distance(&mut doc, def, distance::Kind::XY);
    let scale = distance(&mut doc, def, distance::Kind::Linear);
    let point =
        crate::scene::block_point_parameter::create(&mut doc, def, Vector3::ZERO, Vector3::UNIT_Y)
            .unwrap();
    let flip = crate::scene::block_orientation_parameter::create(
        &mut doc,
        def,
        crate::scene::block_orientation_parameter::Kind::Flip,
        Vector3::ZERO,
        Vector3::new(10., 0., 0.),
        Vector3::new(-2., 2., 0.),
    )
    .unwrap();
    create_with_options(
        &mut doc,
        def,
        Kind::Move,
        xy,
        Some(XyKeyPoint::Base),
        &targets[..1],
        DVec3::ZERO,
        None,
        None,
    )
    .unwrap();
    create(
        &mut doc,
        def,
        Kind::Scale,
        scale,
        &targets[1..],
        DVec3::ZERO,
    )
    .unwrap();
    create(&mut doc, def, Kind::Move, point, &targets[1..], DVec3::ZERO).unwrap();
    create(&mut doc, def, Kind::Flip, flip, &targets[..1], DVec3::ZERO).unwrap();
    let a = insert(&mut doc);
    let b = insert(&mut doc);
    doc = instance::evaluate(&doc, b, point, DVec3::new(7., 8., 0.))
        .unwrap()
        .unwrap();
    let frames = [
        (
            xy,
            ParameterFrame {
                base: DVec3::new(-2., -3., 0.),
                end: DVec3::new(14., 15., 0.),
            },
        ),
        (
            scale,
            ParameterFrame {
                base: DVec3::new(-5., 0., 0.),
                end: DVec3::new(10., 0., 0.),
            },
        ),
    ];
    for reverse in [false, true] {
        let mut changed = doc.clone();
        for index in if reverse { [1, 0] } else { [0, 1] } {
            changed = instance::evaluate_frame(&changed, a, frames[index].0, frames[index].1)
                .unwrap()
                .unwrap();
        }
        changed = instance::evaluate(&changed, a, point, DVec3::new(1., 2., 0.))
            .unwrap()
            .unwrap();
        changed = instance::evaluate_value(&changed, a, flip, Value::Flip(true))
            .unwrap()
            .unwrap();
        for saved in roundtrips(&changed) {
            let refreshed = instance::refresh(&saved, def).unwrap();
            assert_eq!(line(&refreshed, a).start, Vector3::new(-1., 3., 0.));
            assert_eq!(line(&refreshed, b).start, Vector3::UNIT_X);
            for (handle, expected) in frames {
                assert_eq!(
                    instance::frame(&refreshed, a, &parameter(&refreshed, def, handle)).unwrap(),
                    expected
                );
            }
            for (reference, expected) in [
                (a, Vector3::new(2., 6.5, 0.)),
                (b, Vector3::new(11., 11., 0.)),
            ] {
                let EntityType::Insert(reference) = refreshed.get_entity(reference).unwrap() else {
                    panic!()
                };
                let center = refreshed
                    .block_records
                    .get(&reference.block_name)
                    .unwrap()
                    .entity_handles
                    .iter()
                    .find_map(|h| match refreshed.get_entity(*h) {
                        Some(EntityType::Circle(c)) => Some(c.center),
                        _ => None,
                    })
                    .unwrap();
                assert_eq!(center, expected);
            }
            let reset = instance::reset(&refreshed, a).unwrap().unwrap();
            assert_eq!(line(&reset, a).start, Vector3::UNIT_X);
            assert_eq!(
                instance::frame(&reset, b, &parameter(&reset, def, point))
                    .unwrap()
                    .end,
                DVec3::new(7., 8., 0.)
            );
        }
    }
}

#[test]
fn block_frame_constraints_snap_relative_to_the_updated_base_and_fail_atomically() {
    let (mut doc, def, targets) = super::tests::fixture();
    let p = distance(&mut doc, def, distance::Kind::Linear);
    create(&mut doc, def, Kind::Scale, p, &targets[..1], DVec3::ZERO).unwrap();
    let a = insert(&mut doc);
    let p = parameter(&doc, def, p);
    for invalid in [
        ParameterFrame {
            base: DVec3::ZERO,
            end: DVec3::ZERO,
        },
        ParameterFrame {
            base: DVec3::Z,
            end: p.end,
        },
        ParameterFrame {
            base: DVec3::ZERO,
            end: DVec3::splat(f64::NAN),
        },
    ] {
        let before = doc.clone();
        assert!(instance::evaluate_frame(&doc, a, p.handle, invalid).is_err());
        assert_eq!(doc, before);
    }
    let Some(ObjectType::DynamicBlock(object)) = doc.objects.get_mut(&p.handle) else {
        panic!()
    };
    let DynamicBlockData::LinearParameter(native) = &mut object.data else {
        panic!()
    };
    native.value_set.flags = 8;
    native.value_set.values = vec![5., 15.];
    let input = ParameterFrame {
        base: DVec3::new(-5., 0., 0.),
        end: DVec3::new(11., 0., 0.),
    };
    let changed = instance::evaluate_frame(&doc, a, p.handle, input)
        .unwrap()
        .unwrap();
    assert_eq!(
        instance::frame(&changed, a, &p).unwrap(),
        ParameterFrame {
            end: DVec3::new(10., 0., 0.),
            ..input
        }
    );
    assert_eq!(line(&changed, a).start, Vector3::new(-3.5, 0., 0.));
    assert!(instance::evaluate_frame(&changed, a, p.handle, input)
        .unwrap()
        .is_none());
}
