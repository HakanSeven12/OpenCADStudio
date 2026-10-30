use super::*;
use crate::scene::{block_distance_parameter as distance, block_point_parameter as point, Scene};
use codec::{DwgReader, DwgWriter, DxfReader, DxfWriter};
use std::io::Cursor;

#[test]
fn rotation_table_angles_match_across_pi_wrap() {
    let near_positive_pi = std::f64::consts::PI - 0.01;
    let near_negative_pi = -std::f64::consts::PI + 0.01;
    assert!((super::angle_distance(near_positive_pi, near_negative_pi) - 0.02).abs() < 1e-12);
    assert!(super::angle_distance(near_positive_pi, 0.0) > 3.0);
}

#[test]
fn angular_value_sets_wrap_and_choose_the_nearest_periodic_value() {
    use codec::objects::BlockParameterValueSet;
    let quarter = std::f64::consts::FRAC_PI_2;
    let mut set = BlockParameterValueSet {
        flags: 3,
        minimum: 0.0,
        maximum: std::f64::consts::TAU,
        ..Default::default()
    };
    assert!((constrain_angle(-quarter, &set) - 3.0 * quarter).abs() < 1e-12);
    set.flags = 8;
    set.values = vec![0.0, std::f64::consts::PI];
    assert_eq!(constrain_angle(-0.01, &set), 0.0);
    set.flags = 4;
    set.increment = quarter;
    assert!((constrain_angle(-quarter - 0.1, &set) - 3.0 * quarter).abs() < 1e-12);
}

#[test]
fn signed_value_set_increments_preserve_negative_axis_values() {
    let set = codec::objects::BlockParameterValueSet {
        flags: 4,
        increment: 2.0,
        ..Default::default()
    };
    assert_eq!(constrain_value_set(-5.0, &set), -6.0);
    assert_eq!(constrain_value_set(5.0, &set), 6.0);
}

pub(super) fn fixture() -> (Document, Handle, Vec<Handle>) {
    let mut scene = Scene::new();
    scene.define_block_raw(
        "ACTIONS",
        Vector3::ZERO,
        vec![
            EntityType::Line(codec::entities::Line::from_points(
                Vector3::UNIT_X,
                Vector3::new(2., 0., 0.),
            )),
            EntityType::Circle(codec::entities::Circle::from_center_radius(
                Vector3::new(4., 3., 0.),
                2.,
            )),
        ],
    );
    let block = scene.document.block_records.get("ACTIONS").unwrap();
    let (definition, handles) = (block.handle, block.entity_handles.clone());
    (scene.document, definition, handles)
}

fn orientation(
    doc: &mut Document,
    def: Handle,
    kind: super::super::block_orientation_parameter::Kind,
) -> Handle {
    super::super::block_orientation_parameter::create(
        doc,
        def,
        kind,
        Vector3::ZERO,
        Vector3::new(10., 0., 0.),
        if kind == super::super::block_orientation_parameter::Kind::Rotation {
            Vector3::new(10., 0., 0.)
        } else {
            Vector3::new(-2., 2., 0.)
        },
    )
    .unwrap()
}

#[test]
fn block_actions_rotate_flip_native_bindings_and_marker_edits_roundtrip() {
    use super::super::block_orientation_parameter::Kind as O;
    let (mut doc, def, targets) = fixture();
    let r = orientation(&mut doc, def, O::Rotation);
    let f = orientation(&mut doc, def, O::Flip);
    assert!(parameters(&doc, def, Kind::Move).is_empty());
    assert!(parameters(&doc, def, Kind::Scale).is_empty());
    assert_eq!(all_parameters(&doc, def).len(), 2);
    let rotate = create(
        &mut doc,
        def,
        Kind::Rotate,
        r,
        &targets,
        DVec3::new(3., 4., 0.),
    )
    .unwrap();
    let flip = create(
        &mut doc,
        def,
        Kind::Flip,
        f,
        &targets,
        DVec3::new(5., 6., 0.),
    )
    .unwrap();
    assert!(create(&mut doc, def, Kind::Rotate, f, &targets, DVec3::ZERO).is_err());
    for handle in [rotate, flip] {
        super::super::block_authoring::edit::move_part(
            &mut doc,
            def,
            handle,
            super::super::block_authoring::edit::Part::Point,
            Vector3::new(7., 8., 0.),
        )
        .unwrap();
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
        assert_eq!(actions(&saved, def).len(), 2);
        for handle in [rotate, flip] {
            assert_eq!(saved.objects[&handle], doc.objects[&handle]);
        }
        for (_, action, _, _) in actions(&saved, def) {
            assert_eq!(action.display_location, Vector3::new(7., 8., 0.));
            assert_eq!(action.dependencies, targets);
        }
    }
}

#[test]
fn block_actions_rotate_flip_saved_values_compose_copy_and_roundtrip() {
    use super::super::block_orientation_parameter::Kind as O;
    let (mut doc, def, targets) = fixture();
    let r = orientation(&mut doc, def, O::Rotation);
    let f = orientation(&mut doc, def, O::Flip);
    create(&mut doc, def, Kind::Rotate, r, &[targets[0]], DVec3::ZERO).unwrap();
    create(&mut doc, def, Kind::Flip, f, &[targets[0]], DVec3::ZERO).unwrap();
    let a = doc
        .add_entity(EntityType::Insert(codec::entities::Insert::new(
            "ACTIONS",
            Vector3::ZERO,
        )))
        .unwrap();
    let b = doc
        .add_entity(EntityType::Insert(codec::entities::Insert::new(
            "ACTIONS",
            Vector3::new(30., 0., 0.),
        )))
        .unwrap();
    let baseline = doc.clone();
    doc = instance::evaluate(&doc, a, r, DVec3::new(0., 50., 0.))
        .unwrap()
        .unwrap();
    assert!(instance_line(&doc, a).distance(&Vector3::UNIT_Y) < 1e-8);
    doc = instance::evaluate_value(&doc, a, f, Value::Flip(true))
        .unwrap()
        .unwrap();
    assert!(instance_line(&doc, a).distance(&-Vector3::UNIT_Y) < 1e-8);
    assert!(instance::evaluate_value(&doc, a, f, Value::Flip(true))
        .unwrap()
        .is_none());
    let props = doc.dynamic_block_properties(def, Some(a)).unwrap();
    assert!(props
        .iter()
        .any(|p| p.parameter_handle == f && p.value == BlockEvalValue::Short(1)));
    assert!(props.iter().any(|p|p.parameter_handle==r&&matches!(p.value,BlockEvalValue::Real(v) if (v-std::f64::consts::FRAC_PI_2).abs()<1e-8)));
    let variants = [
        doc.clone(),
        DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&doc).unwrap()))
            .read()
            .unwrap(),
        DxfReader::from_reader(Cursor::new(DxfWriter::new(&doc).write_to_vec().unwrap()))
            .unwrap()
            .read()
            .unwrap(),
    ];
    for saved in variants {
        let mut next = instance::evaluate(&saved, a, r, DVec3::new(0., -50., 0.))
            .unwrap()
            .unwrap();
        assert!(instance_line(&next, a).distance(&Vector3::UNIT_Y) < 1e-8);
        for _ in 0..6 {
            next = instance::evaluate_value(&next, a, f, Value::Flip(false))
                .unwrap()
                .unwrap();
            assert!(instance_line(&next, a).distance(&-Vector3::UNIT_Y) < 1e-8);
            next = instance::evaluate_value(&next, a, f, Value::Flip(true))
                .unwrap()
                .unwrap();
            assert!(instance_line(&next, a).distance(&Vector3::UNIT_Y) < 1e-8);
        }
        assert_eq!(instance_line(&next, b), Vector3::UNIT_X);
        assert_eq!(start(&next, targets[0]), start(&baseline, targets[0]));
        let mut scene = Scene::new();
        scene.document = next;
        let c = scene.copy_entities(
            &[a],
            &crate::command::EntityTransform::Translate(DVec3::new(40., 0., 0.)),
        )[0];
        let next = instance::evaluate_value(&scene.document, c, f, Value::Flip(false))
            .unwrap()
            .unwrap();
        assert!(instance_line(&next, c).distance(&-Vector3::UNIT_Y) < 1e-8);
        assert!(instance_line(&next, a).distance(&Vector3::UNIT_Y) < 1e-8);
        let resaved = DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&next).unwrap()))
            .read()
            .unwrap();
        assert!(instance_line(&resaved, c).distance(&-Vector3::UNIT_Y) < 1e-8);
        let fp = parameters(&resaved, def, Kind::Flip).remove(0);
        assert_eq!(
            instance::value(&resaved, c, &fp).unwrap(),
            Value::Flip(false)
        );
        assert_eq!(
            instance::value(&resaved, a, &fp).unwrap(),
            Value::Flip(true)
        );
    }
}

#[test]
fn block_actions_rotate_flip_test_baseline_reversible_and_invalid_atomic() {
    use super::super::block_orientation_parameter::Kind as O;
    let (mut doc, def, targets) = fixture();
    let r = orientation(&mut doc, def, O::Rotation);
    let f = orientation(&mut doc, def, O::Flip);
    let action = create(&mut doc, def, Kind::Rotate, r, &targets, DVec3::ZERO).unwrap();
    create(&mut doc, def, Kind::Flip, f, &targets, DVec3::ZERO).unwrap();
    let baseline = doc.clone();
    assert!(evaluate(&mut doc, &baseline, def, r, DVec3::ZERO).is_err());
    assert_eq!(doc, baseline);
    assert!(evaluate_value(&mut doc, &baseline, def, r, Value::Flip(true)).is_err());
    assert!(evaluate_value(&mut doc, &baseline, def, f, Value::Point(DVec3::ONE)).is_err());
    assert_eq!(doc, baseline);
    evaluate(&mut doc, &baseline, def, r, DVec3::new(0., 10., 0.)).unwrap();
    evaluate_value(&mut doc, &baseline, def, f, Value::Flip(true)).unwrap();
    assert!(start(&doc, targets[0]).distance(&-Vector3::UNIT_Y) < 1e-8);
    evaluate_value(&mut doc, &baseline, def, f, Value::Flip(false)).unwrap();
    evaluate(&mut doc, &baseline, def, r, DVec3::new(10., 0., 0.)).unwrap();
    assert!(start(&doc, targets[0]).distance(&Vector3::UNIT_X) < 1e-8);
    let ObjectType::DynamicBlock(o) = doc.objects.get_mut(&action).unwrap() else {
        panic!()
    };
    let DynamicBlockData::RotateAction(a) = &mut o.data else {
        panic!()
    };
    a.action.offset = Vector3::new(f64::INFINITY, 0., 0.);
    let unsupported = doc.clone();
    assert!(evaluate(&mut doc, &unsupported, def, r, DVec3::new(0., 10., 0.)).is_err());
    assert_eq!(doc, unsupported);
}
fn linear(doc: &mut Document, def: Handle) -> Handle {
    distance::create(
        doc,
        def,
        distance::Kind::Linear,
        Vector3::ZERO,
        Vector3::new(10., 0., 0.),
        Vector3::new(5., 2., 0.),
    )
    .unwrap()
}

#[test]
fn linear_value_set_snaps_interactive_edits_and_keeps_rebuild_values() {
    let (mut doc, def, targets) = fixture();
    let parameter = linear(&mut doc, def);
    create(&mut doc, def, Kind::Move, parameter, &targets, DVec3::ZERO).unwrap();
    let ObjectType::DynamicBlock(object) = doc.objects.get_mut(&parameter).unwrap() else {
        panic!()
    };
    let DynamicBlockData::LinearParameter(linear) = &mut object.data else {
        panic!()
    };
    linear.value_set.flags = 7;
    linear.value_set.minimum = 2.0;
    linear.value_set.maximum = 8.0;
    linear.value_set.increment = 1.0;
    let baseline = doc.clone();

    evaluate(&mut doc, &baseline, def, parameter, DVec3::new(4.7, 0.0, 0.0)).unwrap();
    assert!((current(&doc, &parameters(&doc, def, Kind::Move)[0]).x - 5.0).abs() < 1e-9);
    evaluate(&mut doc, &baseline, def, parameter, DVec3::new(1.0, 0.0, 0.0)).unwrap();
    assert!((current(&doc, &parameters(&doc, def, Kind::Move)[0]).x - 2.0).abs() < 1e-9);

    let p = parameters(&doc, def, Kind::Move).remove(0);
    set_value(&mut doc, &p, Value::Point(DVec3::new(1.0, 0.0, 0.0))).unwrap();
    rebuild(&mut doc, &baseline, def).unwrap();
    assert!((current(&doc, &p).x - 1.0).abs() < 1e-9);
}

#[test]
fn xy_move_box_set_creates_four_key_point_bindings() {
    let (mut doc, def, handles) = fixture();
    let parameter = distance::create(
        &mut doc,
        def,
        distance::Kind::XY,
        Vector3::ZERO,
        Vector3::new(10., 10., 0.),
        Vector3::new(5., 5., 0.),
    )
    .unwrap();
    let key_points = [
        (XyKeyPoint::Base, ["BaseXDelta", "BaseYDelta"]),
        (XyKeyPoint::X, ["EndXDelta", "BaseYDelta"]),
        (XyKeyPoint::Y, ["BaseXDelta", "EndYDelta"]),
        (XyKeyPoint::End, ["EndXDelta", "EndYDelta"]),
    ];
    for (index, (key_point, expected)) in key_points.iter().copied().enumerate() {
        let action_handle = create_with_options(
            &mut doc,
            def,
            Kind::Move,
            parameter,
            Some(key_point),
            &[handles[index % handles.len()]],
            DVec3::new(index as f64, 0., 0.),
            None,
            None,
        )
        .unwrap();
        let ObjectType::DynamicBlock(action_object) = &doc.objects[&action_handle] else {
            panic!()
        };
        let DynamicBlockData::MoveAction(action) = &action_object.data else {
            panic!()
        };
        assert_eq!(
            [
                action.connections[0].name.as_str(),
                action.connections[1].name.as_str(),
            ],
            expected
        );
    }
    assert_eq!(
        doc.objects
            .values()
            .filter(|object| matches!(object, ObjectType::DynamicBlock(object) if matches!(object.data, DynamicBlockData::MoveAction(_))))
            .count(),
        4
    );
    let roundtrips = [
        DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&doc).unwrap()))
            .read()
            .unwrap(),
        DxfReader::from_reader(Cursor::new(DxfWriter::new(&doc).write_to_vec().unwrap()))
            .unwrap()
            .read()
            .unwrap(),
    ];
    for roundtrip in roundtrips {
        for (_, expected) in key_points {
            assert!(roundtrip.objects.values().any(|object| {
                matches!(object, ObjectType::DynamicBlock(object) if matches!(&object.data, DynamicBlockData::MoveAction(action) if [action.connections[0].name.as_str(), action.connections[1].name.as_str()] == expected))
            }));
        }
    }
}

#[test]
fn xy_box_set_actions_apply_their_key_point_axis_deltas() {
    for kind in [Kind::Move, Kind::Stretch] {
        for (key_point, expected_delta) in [
            (XyKeyPoint::Base, DVec3::ZERO),
            (XyKeyPoint::X, DVec3::new(3., 0., 0.)),
            (XyKeyPoint::Y, DVec3::new(0., 5., 0.)),
            (XyKeyPoint::End, DVec3::new(3., 5., 0.)),
        ] {
            let (mut doc, def, targets) = fixture();
            let parameter = distance::create(
                &mut doc,
                def,
                distance::Kind::XY,
                Vector3::ZERO,
                Vector3::new(10., 10., 0.),
                Vector3::new(5., 5., 0.),
            )
            .unwrap();
            create_with_options(
                &mut doc,
                def,
                kind,
                parameter,
                Some(key_point),
                &[targets[0]],
                DVec3::ZERO,
                (kind == Kind::Stretch)
                    .then_some([DVec3::new(0.5, -1., 0.), DVec3::new(2.5, 1., 0.)]),
                None,
            )
            .unwrap();
            let baseline = doc.clone();
            evaluate(
                &mut doc,
                &baseline,
                def,
                parameter,
                DVec3::new(13., 15., 0.),
            )
            .unwrap();
            assert_eq!(
                start(&doc, targets[0]),
                Vector3::new(1. + expected_delta.x, expected_delta.y, 0.),
                "key point {key_point:?}"
            );
        }
    }
}

fn start(doc: &Document, h: Handle) -> Vector3 {
    let EntityType::Line(l) = doc.get_entity(h).unwrap() else {
        panic!()
    };
    l.start
}

fn instance_line(doc: &Document, insert: Handle) -> Vector3 {
    let EntityType::Insert(i) = doc.get_entity(insert).unwrap() else {
        panic!()
    };
    start(
        doc,
        doc.block_records.get(&i.block_name).unwrap().entity_handles[0],
    )
}

#[test]
fn block_actions_saved_xy_polar_cache_metadata_and_rejections() {
    for kind in [distance::Kind::XY, distance::Kind::Polar] {
        let (mut doc, def, handles) = fixture();
        let p = distance::create(
            &mut doc,
            def,
            kind,
            Vector3::new(2., 3., 0.),
            Vector3::new(12., 13., 0.),
            Vector3::new(5., 6., 0.),
        )
        .unwrap();
        create(&mut doc, def, Kind::Move, p, &[handles[0]], DVec3::ZERO).unwrap();
        let a = doc
            .add_entity(EntityType::Insert(codec::entities::Insert::new(
                "ACTIONS",
                Vector3::ZERO,
            )))
            .unwrap();
        let original = doc.clone();
        let mut doc = instance::evaluate(&doc, a, p, DVec3::new(20., 18., 0.))
            .unwrap()
            .unwrap();
        assert_eq!(instance_line(&doc, a), Vector3::new(9., 5., 0.));
        let parameter = parameters(&doc, def, Kind::Move).remove(0);
        assert_eq!(
            instance::current(&doc, a, &parameter).unwrap(),
            DVec3::new(20., 18., 0.)
        );
        let r = doc
            .objects
            .values_mut()
            .find_map(|o| match o {
                ObjectType::XRecord(r) => Some(r),
                _ => None,
            })
            .unwrap();
        r.entries.push(codec::objects::XRecordEntry::string(
            1,
            "retain opaque metadata",
        ));
        doc = instance::evaluate(&doc, a, p, DVec3::new(22., 21., 0.))
            .unwrap()
            .unwrap();
        assert!(doc.objects.values().any(|o|matches!(o,ObjectType::XRecord(r) if r.entries.iter().any(|e|e.value.as_string()==Some("retain opaque metadata")))));
        let reopened = DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&doc).unwrap()))
            .read()
            .unwrap();
        assert_eq!(
            instance::current(&reopened, a, &parameter).unwrap(),
            DVec3::new(22., 21., 0.)
        );
        assert_eq!(start(&reopened,handles[0]),start(&original,handles[0]));
        assert!(instance::evaluate(&doc, a, p, DVec3::splat(f64::NAN)).is_err());
        let mut invalid = doc.clone();
        let EntityType::Insert(i) = invalid.get_entity_mut(a).unwrap() else {
            panic!()
        };
        i.set_x_scale(2.);
        assert!(instance::definition(&invalid, a).is_none());
        assert!(instance::evaluate(&invalid, a, p, DVec3::ONE).is_err());
    }
}

#[test]
fn block_actions_saved_references_independent_absolute_and_roundtrip() {
    let (mut doc, def, handles) = fixture();
    let p = linear(&mut doc, def);
    let q = point::create(&mut doc, def, Vector3::ZERO, Vector3::UNIT_Y).unwrap();
    create(&mut doc, def, Kind::Scale, p, &[handles[1]], DVec3::ZERO).unwrap();
    create(&mut doc, def, Kind::Move, q, &[handles[0]], DVec3::ZERO).unwrap();
    let a = doc
        .add_entity(EntityType::Insert(codec::entities::Insert::new(
            "ACTIONS",
            Vector3::ZERO,
        )))
        .unwrap();
    let b = doc
        .add_entity(EntityType::Insert(codec::entities::Insert::new(
            "ACTIONS",
            Vector3::new(30., 0., 0.),
        )))
        .unwrap();
    let original = doc.clone();
    doc = instance::evaluate(&doc, a, q, DVec3::new(3., 4., 0.))
        .unwrap()
        .unwrap();
    doc = instance::evaluate(&doc, a, p, DVec3::new(20., 7., 0.))
        .unwrap()
        .unwrap();
    assert_eq!(instance_line(&doc, a), Vector3::new(4., 4., 0.));
    assert_eq!(instance_line(&doc, b), Vector3::UNIT_X);
    for h in &handles {
        assert_eq!(doc.get_entity(*h), original.get_entity(*h));
    }
    for (h, o) in &original.objects {
        assert_eq!(doc.objects.get(h), Some(o));
    }
    let mut saved = vec![doc.clone()];
    saved.push(
        DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&doc).unwrap()))
            .read()
            .unwrap(),
    );
    saved.push(
        DxfReader::from_reader(Cursor::new(DxfWriter::new(&doc).write_to_vec().unwrap()))
            .unwrap()
            .read()
            .unwrap(),
    );
    for doc in saved {
        let parameter = parameters(&doc, def, Kind::Move)
            .into_iter()
            .find(|x| x.handle == p)
            .unwrap();
        assert_eq!(
            instance::current(&doc, a, &parameter).unwrap(),
            DVec3::new(20., 0., 0.)
        );
        let props = doc.dynamic_block_properties(def, Some(a)).unwrap();
        assert!(props
            .iter()
            .any(|x| x.parameter_handle == p && x.value == BlockEvalValue::Real(20.)));
        assert!(instance::evaluate(&doc, a, p, DVec3::new(20., 0., 0.))
            .unwrap()
            .is_none());
        let next = instance::evaluate(&doc, a, q, DVec3::new(-2., 1., 0.))
            .unwrap()
            .unwrap();
        assert_eq!(instance_line(&next, a), Vector3::new(-1., 1., 0.));
        let EntityType::Insert(i) = next.get_entity(a).unwrap() else {
            panic!()
        };
        let EntityType::Circle(c) = next
            .get_entity(
                next.block_records
                    .get(&i.block_name)
                    .unwrap()
                    .entity_handles[1],
            )
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(c.radius, 4.);
        assert_eq!(instance_line(&next, b), Vector3::UNIT_X);
        let resaved = DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&next).unwrap()))
            .read()
            .unwrap();
        assert_eq!(instance_line(&resaved, a), Vector3::new(-1., 1., 0.));
        let point = parameters(&resaved, def, Kind::Move)
            .into_iter()
            .find(|x| x.handle == q)
            .unwrap();
        assert_eq!(
            instance::current(&resaved, a, &point).unwrap(),
            DVec3::new(-2., 1., 0.)
        );
        // A copied reference may share its evaluated block, but must isolate on edit.
        let mut scene = Scene::new();
        scene.document = next.clone();
        let c = scene.copy_entities(
            &[a],
            &crate::command::EntityTransform::Translate(DVec3::new(40., 0., 0.)),
        )[0];
        let copy = instance::evaluate(&scene.document, c, q, DVec3::new(7., 8., 0.))
            .unwrap()
            .unwrap();
        assert_eq!(instance_line(&copy, c), Vector3::new(8., 8., 0.));
        assert_eq!(instance_line(&copy, a), Vector3::new(-1., 1., 0.));
        assert_eq!(
            instance::current(&copy, c, &parameter).unwrap(),
            DVec3::new(20., 0., 0.)
        );
        let reset = instance::evaluate(&next, a, q, DVec3::ZERO)
            .unwrap()
            .unwrap();
        assert_eq!(instance_line(&reset, a), Vector3::UNIT_X);
    }
}
#[test]
fn block_actions_native_bindings_roundtrip_and_append_preserves_neighbors() {
    let (mut doc, def, handles) = fixture();
    let p = linear(&mut doc, def);
    for kind in [Kind::Move, Kind::Scale] {
        create(
            &mut doc,
            def,
            kind,
            p,
            &[handles[0]],
            DVec3::new(3., 4., 0.),
        )
        .unwrap();
    }
    let graph = authoring::graph(&doc, def).unwrap().unwrap();
    let ObjectType::DynamicBlock(o) = &doc.objects[&graph] else {
        panic!()
    };
    let DynamicBlockData::EvaluationGraph(g) = &o.data else {
        panic!()
    };
    assert!(g
        .nodes
        .iter()
        .all(|n| authoring::graph::valid_lists(g, n.id)));
    let dwg = DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&doc).unwrap()))
        .read()
        .unwrap();
    let dxf = DxfReader::from_reader(Cursor::new(DxfWriter::new(&doc).write_to_vec().unwrap()))
        .unwrap()
        .read()
        .unwrap();
    for saved in [dwg, dxf] {
        assert_eq!(actions(&saved, def).len(), 2);
        for (h, _, _, _) in actions(&doc, def) {
            assert_eq!(saved.objects[&h], doc.objects[&h]);
        }
        let mut test = saved.clone();
        assert!(evaluate(&mut test, &saved, def, p, DVec3::new(20., 5., 0.)).unwrap());
        assert_eq!(start(&test, handles[0]), Vector3::new(22., 0., 0.));
        assert_eq!(test.get_entity(handles[1]), saved.get_entity(handles[1]));
    }
}
#[test]
fn block_actions_point_move_is_absolute_reversible_and_leaves_unselected_objects() {
    let (mut doc, def, handles) = fixture();
    let p = point::create(
        &mut doc,
        def,
        Vector3::new(2., 3., 0.),
        Vector3::new(5., 6., 0.),
    )
    .unwrap();
    create(&mut doc, def, Kind::Move, p, &[handles[0]], DVec3::ZERO).unwrap();
    let baseline = doc.clone();
    for _ in 0..8 {
        assert!(evaluate(&mut doc, &baseline, def, p, DVec3::new(12., -7., 0.)).unwrap());
        assert_eq!(start(&doc, handles[0]), Vector3::new(11., -10., 0.));
        assert!(!evaluate(&mut doc, &baseline, def, p, DVec3::new(12., -7., 0.)).unwrap());
        assert!(evaluate(&mut doc, &baseline, def, p, DVec3::new(2., 3., 0.)).unwrap());
        assert_eq!(start(&doc, handles[0]), start(&baseline, handles[0]));
    }
    assert_eq!(doc.get_entity(handles[1]), baseline.get_entity(handles[1]));
}
#[test]
fn block_actions_scale_uses_distance_ratio_and_parameter_base() {
    let (mut doc, def, handles) = fixture();
    let p = distance::create(
        &mut doc,
        def,
        distance::Kind::Polar,
        Vector3::new(4., 3., 0.),
        Vector3::new(14., 3., 0.),
        Vector3::new(9., 5., 0.),
    )
    .unwrap();
    create(&mut doc, def, Kind::Scale, p, &[handles[1]], DVec3::ZERO).unwrap();
    let baseline = doc.clone();
    evaluate(&mut doc, &baseline, def, p, DVec3::new(4., 23., 0.)).unwrap();
    let EntityType::Circle(c) = doc.get_entity(handles[1]).unwrap() else {
        panic!()
    };
    assert_eq!(c.center, Vector3::new(4., 3., 0.));
    assert_eq!(c.radius, 4.);
    let before = doc.clone();
    assert!(evaluate(&mut doc, &baseline, def, p, DVec3::new(4., 3., 0.)).is_err());
    assert_eq!(doc, before);
}
#[test]
fn block_reset_restores_values_geometry_and_table_state_without_changing_other_references() {
    let (mut doc, def, targets) = fixture();
    let parameter = linear(&mut doc, def);
    create(
        &mut doc,
        def,
        Kind::Move,
        parameter,
        &targets[..1],
        DVec3::ZERO,
    )
    .unwrap();
    let columns = super::super::block_properties_table::columns(&doc, def);
    super::super::block_properties_table::apply(
        &mut doc,
        def,
        &columns,
        &[vec![10.], vec![20.]],
        false,
    )
    .unwrap();
    let a = doc
        .add_entity(EntityType::Insert(codec::entities::Insert::new(
            "ACTIONS",
            Vector3::ZERO,
        )))
        .unwrap();
    let b = doc
        .add_entity(EntityType::Insert(codec::entities::Insert::new(
            "ACTIONS",
            Vector3::new(30., 0., 0.),
        )))
        .unwrap();
    assert!(instance::reset(&doc, a).unwrap().is_none());
    let baseline = doc.clone();
    doc = instance::evaluate_table_row(&doc, a, &columns, 1)
        .unwrap()
        .unwrap();
    doc = instance::evaluate(&doc, b, parameter, DVec3::new(15., 0., 0.))
        .unwrap()
        .unwrap();
    assert_eq!(instance_line(&doc, a), Vector3::new(11., 0., 0.));
    let reset = instance::reset(&doc, a).unwrap().unwrap();
    assert_eq!(instance_line(&reset, a), Vector3::UNIT_X);
    assert_eq!(instance_line(&reset, b), Vector3::new(6., 0., 0.));
    assert_eq!(
        reset.get_entity(targets[0]),
        baseline.get_entity(targets[0])
    );
    let p = parameters(&reset, def, Kind::Move).remove(0);
    assert_eq!(instance::current(&reset, a, &p).unwrap(), p.end);
    assert!(!reset
        .objects
        .values()
        .any(|object| matches!(object, ObjectType::Dictionary(dictionary)
        if dictionary.entries.iter().any(|(name, _)| name == "ACAD_ENHANCEDBLOCKHISTORY"))));
    assert!(instance::reset(&reset, a).unwrap().is_none());
    for saved in [
        DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&reset).unwrap()))
            .read()
            .unwrap(),
        DxfReader::from_reader(Cursor::new(DxfWriter::new(&reset).write_to_vec().unwrap()))
            .unwrap()
            .read()
            .unwrap(),
    ] {
        assert_eq!(instance_line(&saved, a), Vector3::UNIT_X);
        assert_eq!(instance_line(&saved, b), Vector3::new(6., 0., 0.));
        assert_eq!(instance::current(&saved, a, &p).unwrap(), p.end);
    }
}

#[test]
fn block_actions_reject_invalid_selection_binding_and_nonfinite_without_mutation() {
    let (mut doc, def, handles) = fixture();
    let p = linear(&mut doc, def);
    let before = doc.clone();
    for (parameter, targets, location) in [
        (Handle::NULL, vec![handles[0]], DVec3::ZERO),
        (p, vec![], DVec3::ZERO),
        (p, vec![Handle::new(0xff_ff_ff)], DVec3::ZERO),
        (p, vec![handles[0]], DVec3::splat(f64::NAN)),
    ] {
        assert!(create(&mut doc, def, Kind::Move, parameter, &targets, location).is_err());
        assert_eq!(doc, before);
    }
    let action = create(&mut doc, def, Kind::Move, p, &[handles[0]], DVec3::ZERO).unwrap();
    let baseline = doc.clone();
    assert!(evaluate(&mut doc, &baseline, def, p, DVec3::splat(f64::NAN)).is_err());
    assert_eq!(doc, baseline);
    let mut restricted = doc.clone();
    let ObjectType::DynamicBlock(object) = restricted.objects.get_mut(&p).unwrap() else {
        panic!()
    };
    let DynamicBlockData::LinearParameter(parameter) = &mut object.data else {
        panic!()
    };
    parameter.value_set.maximum = 15.;
    parameter.value_set.flags |= 2;
    let restricted_baseline = restricted.clone();
    assert!(evaluate(
        &mut restricted,
        &restricted_baseline,
        def,
        p,
        DVec3::new(20., 0., 0.)
    )
    .unwrap());
    assert_eq!(start(&restricted, handles[0]), Vector3::new(6., 0., 0.));
    let constrained = parameters(&restricted_baseline, def, Kind::Move).remove(0);
    assert_eq!(current(&restricted, &constrained), DVec3::new(15., 0., 0.));
    let ObjectType::DynamicBlock(o) = doc.objects.get_mut(&action).unwrap() else {
        panic!()
    };
    let DynamicBlockData::MoveAction(m) = &mut o.data else {
        panic!()
    };
    m.offsets.distance_multiplier = f64::INFINITY;
    let baseline = doc.clone();
    assert!(evaluate(&mut doc, &baseline, def, p, DVec3::new(15., 0., 0.)).is_err());
    assert_eq!(doc, baseline);
}
#[test]
fn block_actions_reframe_moves_markers_and_retains_targets_and_connections() {
    let (mut doc, def, handles) = fixture();
    let p = linear(&mut doc, def);
    let h = create(
        &mut doc,
        def,
        Kind::Scale,
        p,
        &[handles[0]],
        DVec3::new(3., 4., 0.),
    )
    .unwrap();
    let before = doc.objects[&h].clone();
    let t = Transform::from_translation(Vector3::new(-2., -3., 0.));
    assert_eq!(reframe(&mut doc, def, &t), 1);
    let ObjectType::DynamicBlock(o) = &doc.objects[&h] else {
        panic!()
    };
    let DynamicBlockData::ScaleAction(s) = &o.data else {
        panic!()
    };
    assert_eq!(s.action.action.display_location, Vector3::new(1., 1., 0.));
    let ObjectType::DynamicBlock(o) = before else {
        panic!()
    };
    let DynamicBlockData::ScaleAction(b) = o.data else {
        panic!()
    };
    assert_eq!(s.action.action.dependencies, b.action.action.dependencies);
    assert_eq!(s.connections, b.connections);
}

fn stretch_fixture() -> (Document, Handle, Vec<Handle>, Handle, Handle) {
    let mut scene = Scene::new();
    let mut poly = codec::entities::LwPolyline::from_points(vec![
        codec::types::Vector2::new(0., 1.),
        codec::types::Vector2::new(10., 1.),
        codec::types::Vector2::new(10., 3.),
        codec::types::Vector2::new(0., 3.),
    ]);
    poly.is_closed = true;
    poly.vertices[0].bulge = 0.2;
    scene.define_block_raw(
        "STRETCH",
        Vector3::ZERO,
        vec![
            EntityType::Line(codec::entities::Line::from_points(
                Vector3::ZERO,
                Vector3::new(10., 0., 0.),
            )),
            EntityType::LwPolyline(poly),
            EntityType::Line(codec::entities::Line::from_points(
                Vector3::new(20., 0., 0.),
                Vector3::new(21., 0., 0.),
            )),
        ],
    );
    let br = scene.document.block_records.get("STRETCH").unwrap();
    let (def, handles) = (br.handle, br.entity_handles.clone());
    let mut doc = scene.document;
    let p = linear(&mut doc, def);
    let action = create_with_frame(
        &mut doc,
        def,
        Kind::Stretch,
        p,
        &handles,
        DVec3::new(5., 4., 0.),
        Some([DVec3::new(9., -1., 0.), DVec3::new(11., 4., 0.)]),
    )
    .unwrap();
    (doc, def, handles, p, action)
}

fn polar_stretch_fixture() -> (Document, Handle, Vec<Handle>, Handle, Handle) {
    let mut scene = Scene::new();
    scene.define_block_raw(
        "POLAR_STRETCH",
        Vector3::ZERO,
        vec![EntityType::Line(codec::entities::Line::from_points(
            Vector3::UNIT_X,
            Vector3::new(2., 0., 0.),
        ))],
    );
    let record = scene.document.block_records.get("POLAR_STRETCH").unwrap();
    let (definition, targets) = (record.handle, record.entity_handles.clone());
    let mut doc = scene.document;
    let parameter = distance::create(
        &mut doc,
        definition,
        distance::Kind::Polar,
        Vector3::ZERO,
        Vector3::new(10., 0., 0.),
        Vector3::new(5., 2., 0.),
    )
    .unwrap();
    let action = create_with_frame(
        &mut doc,
        definition,
        Kind::PolarStretch,
        parameter,
        &[targets[0]],
        DVec3::ZERO,
        Some([DVec3::new(0.5, -0.5, 0.), DVec3::new(1.5, 0.5, 0.)]),
    )
    .unwrap();
    (doc, definition, targets, parameter, action)
}

#[test]
fn polar_stretch_evaluates_test_block_and_saved_reference() {
    let (mut doc, definition, targets, parameter, action) = polar_stretch_fixture();
    let ObjectType::DynamicBlock(object) = &doc.objects[&action] else {
        panic!()
    };
    let DynamicBlockData::PolarStretchAction(action) = &object.data else {
        panic!()
    };
    assert_eq!(action.bindings.len(), 1);
    assert_eq!(action.bindings[0].indexes, vec![0]);

    let baseline = doc.clone();
    evaluate(
        &mut doc,
        &baseline,
        definition,
        parameter,
        DVec3::new(0., 10., 0.),
    )
    .unwrap();
    let EntityType::Line(line) = doc.get_entity(targets[0]).unwrap() else {
        panic!()
    };
    assert!((line.start-Vector3::new(0.,1.,0.)).length()<1e-10);
    assert!((line.end-Vector3::new(0.,2.,0.)).length()<1e-10);

    let mut source = baseline.clone();
    let insert = source
        .add_entity(EntityType::Insert(codec::entities::Insert::new(
            "POLAR_STRETCH",
            Vector3::ZERO,
        )))
        .unwrap();
    let changed = instance::evaluate(
        &source,
        insert,
        parameter,
        DVec3::new(0., 10., 0.),
    )
    .unwrap()
    .unwrap();
    assert_eq!(source.get_entity(targets[0]), baseline.get_entity(targets[0]));
    let EntityType::Insert(reference) = changed.get_entity(insert).unwrap() else {
        panic!()
    };
    let evaluated_member = changed
        .block_records
        .get(&reference.block_name)
        .unwrap()
        .entity_handles[0];
    let EntityType::Line(line) = changed.get_entity(evaluated_member).unwrap() else {
        panic!()
    };
    assert!((line.start-Vector3::new(0.,1.,0.)).length()<1e-10);
    assert!((line.end-Vector3::new(0.,2.,0.)).length()<1e-10);
}

#[test]
fn polar_stretch_retarget_rebinds_vertices_and_roundtrips() {
    let mut scene = Scene::new();
    scene.define_block_raw(
        "POLAR_RETARGET",
        Vector3::ZERO,
        vec![
            EntityType::Line(codec::entities::Line::from_points(
                Vector3::UNIT_X,
                Vector3::new(2., 0., 0.),
            )),
            EntityType::Line(codec::entities::Line::from_points(
                Vector3::new(4., 0., 0.),
                Vector3::new(5., 0., 0.),
            )),
        ],
    );
    let record = scene.document.block_records.get("POLAR_RETARGET").unwrap();
    let (definition, targets) = (record.handle, record.entity_handles.clone());
    let mut doc = scene.document;
    let parameter = distance::create(
        &mut doc,
        definition,
        distance::Kind::Polar,
        Vector3::ZERO,
        Vector3::new(10., 0., 0.),
        Vector3::new(5., 2., 0.),
    )
    .unwrap();
    let action = create_with_frame(
        &mut doc,
        definition,
        Kind::PolarStretch,
        parameter,
        &targets[..1],
        DVec3::ZERO,
        Some([DVec3::new(0.5, -0.5, 0.), DVec3::new(1.5, 0.5, 0.)]),
    )
    .unwrap();

    assert!(retarget(
        &mut doc,
        definition,
        action,
        &targets[1..],
        Some([DVec3::new(3.5, -0.5, 0.), DVec3::new(5.5, 0.5, 0.)]),
    )
    .unwrap());
    let ObjectType::DynamicBlock(object) = &doc.objects[&action] else {
        panic!()
    };
    let DynamicBlockData::PolarStretchAction(action_data) = &object.data else {
        panic!()
    };
    assert_eq!(action_data.action.dependencies, vec![targets[1]]);
    assert_eq!(action_data.bindings.len(), 1);
    assert_eq!(action_data.bindings[0].handle, targets[1]);
    assert_eq!(action_data.points.len(), 2);

    for saved in [
        DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&doc).unwrap()))
            .read()
            .unwrap(),
        DxfReader::from_reader(Cursor::new(DxfWriter::new(&doc).write_to_vec().unwrap()))
            .unwrap()
            .read()
            .unwrap(),
    ] {
        assert_eq!(saved.objects[&action], doc.objects[&action]);
    }
}

#[test]
fn block_actions_stretch_frozen_vertices_polyline_and_whole_objects_roundtrip() {
    let (doc, def, handles, p, action) = stretch_fixture();
    let ObjectType::DynamicBlock(o) = &doc.objects[&action] else {
        panic!()
    };
    let DynamicBlockData::StretchAction(a) = &o.data else {
        panic!()
    };
    assert_eq!(a.handles[0].indexes, vec![1]);
    assert_eq!(a.handles[1].indexes, vec![1, 2]);
    assert_eq!(a.action.dependencies, handles);
    let mut evaluated = doc.clone();
    for x in [20., 30., 12.] {
        evaluate(&mut evaluated, &doc, def, p, DVec3::new(x, 0., 0.)).unwrap();
        let EntityType::Line(line) = evaluated.get_entity(handles[0]).unwrap() else {
            panic!()
        };
        assert_eq!(line.start, Vector3::ZERO);
        assert_eq!(line.end, Vector3::new(x, 0., 0.));
        // Entirely outside the frame but explicitly in the action selection.
        assert_eq!(start(&evaluated, handles[2]), Vector3::new(x + 10., 0., 0.));
        let EntityType::LwPolyline(poly) = evaluated.get_entity(handles[1]).unwrap() else {
            panic!()
        };
        assert_eq!(poly.vertices[0].location.x, 0.);
        assert_eq!(poly.vertices[1].location.x, x);
        assert_eq!(poly.vertices[2].location.x, x);
        assert_eq!(poly.vertices[3].location.x, 0.);
        // Preserve the curved segment's sagitta when its chord changes.
        assert!((poly.vertices[0].bulge * x / 2. - 1.).abs() < 1e-8);
    }
    for reopened in [
        DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&doc).unwrap()))
            .read()
            .unwrap(),
        DxfReader::from_reader(Cursor::new(DxfWriter::new(&doc).write_to_vec().unwrap()))
            .unwrap()
            .read()
            .unwrap(),
    ] {
        assert_eq!(reopened.objects[&action], doc.objects[&action]);
        let mut changed = reopened.clone();
        evaluate(&mut changed, &reopened, def, p, DVec3::new(14., 0., 0.)).unwrap();
        let EntityType::Line(line) = changed.get_entity(handles[0]).unwrap() else {
            panic!()
        };
        assert_eq!(line.end, Vector3::new(14., 0., 0.));
    }
}

#[test]
fn block_actions_stretch_saved_reference_independence_copy_and_reopen() {
    let (mut doc, def, handles, p, _) = stretch_fixture();
    let a = doc
        .add_entity(EntityType::Insert(codec::entities::Insert::new(
            "STRETCH",
            Vector3::ZERO,
        )))
        .unwrap();
    let b = doc
        .add_entity(EntityType::Insert(codec::entities::Insert::new(
            "STRETCH",
            Vector3::new(40., 0., 0.),
        )))
        .unwrap();
    let original = doc.clone();
    doc = instance::evaluate(&doc, a, p, DVec3::new(18., 0., 0.))
        .unwrap()
        .unwrap();
    assert_eq!(doc.get_entity(handles[0]), original.get_entity(handles[0]));
    assert_eq!(doc.get_entity(b), original.get_entity(b));
    let mut scene = Scene::new();
    scene.document = doc;
    let c = scene.copy_entities(
        &[a],
        &crate::command::EntityTransform::Translate(DVec3::new(60., 0., 0.)),
    )[0];
    let mut doc = scene.document;
    let EntityType::Insert(ins) = doc.get_entity(a).unwrap() else {
        panic!()
    };
    let a_member = doc
        .block_records
        .get(&ins.block_name)
        .unwrap()
        .entity_handles[0];
    doc = instance::evaluate(&doc, c, p, DVec3::new(22., 0., 0.))
        .unwrap()
        .unwrap();
    let EntityType::Line(line) = doc.get_entity(a_member).unwrap() else {
        panic!()
    };
    assert_eq!(line.end, Vector3::new(18., 0., 0.));
    let reopened = DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&doc).unwrap()))
        .read()
        .unwrap();
    let parameter = parameters(&reopened, def, Kind::Stretch)[0].clone();
    assert_eq!(
        instance::current(&reopened, c, &parameter).unwrap(),
        DVec3::new(22., 0., 0.)
    );
    let changed = instance::evaluate(&reopened, c, p, DVec3::new(13., 0., 0.))
        .unwrap()
        .unwrap();
    let EntityType::Insert(ins) = changed.get_entity(c).unwrap() else {
        panic!()
    };
    let EntityType::Line(line) = changed
        .get_entity(
            changed
                .block_records
                .get(&ins.block_name)
                .unwrap()
                .entity_handles[0],
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(line.end, Vector3::new(13., 0., 0.));
}

#[test]
fn stretch_pair_bindings_evaluate_and_roundtrip_for_linear_and_polar_parameters() {
    for parameter_kind in [distance::Kind::Linear, distance::Kind::Polar] {
        let (mut doc, def, targets) = fixture();
        let parameter = distance::create(
            &mut doc,
            def,
            parameter_kind,
            Vector3::ZERO,
            Vector3::new(10., 0., 0.),
            Vector3::new(5., 2., 0.),
        )
        .unwrap();
        for (key_point, frame) in [
            (
                XyKeyPoint::Base,
                [DVec3::new(0.5, -1., 0.), DVec3::new(1.5, 1., 0.)],
            ),
            (
                XyKeyPoint::End,
                [DVec3::new(1.5, -1., 0.), DVec3::new(2.5, 1., 0.)],
            ),
        ] {
            create_with_options(
                &mut doc,
                def,
                Kind::Stretch,
                parameter,
                Some(key_point),
                &targets[..1],
                DVec3::ZERO,
                Some(frame),
                None,
            )
            .unwrap();
        }
        let roundtrips = [
            doc.clone(),
            DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&doc).unwrap()))
                .read()
                .unwrap(),
            DxfReader::from_reader(Cursor::new(DxfWriter::new(&doc).write_to_vec().unwrap()))
                .unwrap()
                .read()
                .unwrap(),
        ];
        for baseline in roundtrips {
            let mut changed = baseline.clone();
            for end in [DVec3::new(13., 0., 0.), DVec3::new(14., 0., 0.)] {
                evaluate(&mut changed, &baseline, def, parameter, end).unwrap();
                let EntityType::Line(line) = changed.get_entity(targets[0]).unwrap() else {
                    panic!()
                };
                assert_eq!(line.start, Vector3::UNIT_X);
                assert_eq!(line.end, Vector3::new(2. + end.x - 10., 0., 0.));
            }
            if parameter_kind == distance::Kind::Polar {
                evaluate(
                    &mut changed,
                    &baseline,
                    def,
                    parameter,
                    DVec3::new(0., 12., 0.),
                )
                .unwrap();
                let EntityType::Line(line) = changed.get_entity(targets[0]).unwrap() else {
                    panic!()
                };
                assert_eq!(line.start, Vector3::UNIT_X);
                assert_eq!(line.end, Vector3::new(-8., 12., 0.));
            }
        }
    }
}

#[test]
fn block_actions_stretch_invalid_frame_binding_and_options_are_atomic() {
    let (mut doc, def, handles, p, action) = stretch_fixture();
    let before = doc.clone();
    for frame in [
        None,
        Some([DVec3::ZERO, DVec3::X]),
        Some([DVec3::ZERO, DVec3::splat(f64::NAN)]),
    ] {
        assert!(create_with_frame(
            &mut doc,
            def,
            Kind::Stretch,
            p,
            &handles,
            DVec3::ZERO,
            frame
        )
        .is_err());
        assert_eq!(doc, before);
    }
    for corrupt in [0, 1, 2] {
        let mut invalid = doc.clone();
        let ObjectType::DynamicBlock(o) = invalid.objects.get_mut(&action).unwrap() else {
            panic!()
        };
        let DynamicBlockData::StretchAction(a) = &mut o.data else {
            panic!()
        };
        match corrupt {
            0 => a.handles[0].indexes = vec![99],
            1 => a.offsets.distance_multiplier = f64::INFINITY,
            _ => a.connections[0].name = "BaseXDelta".into(),
        }
        let baseline = invalid.clone();
        assert!(evaluate(&mut invalid, &baseline, def, p, DVec3::new(12., 0., 0.)).is_err());
        assert_eq!(invalid, baseline);
    }
}

#[test]
fn block_actions_stretch_polar_xy_and_reframed_marker_keep_native_bindings() {
    for kind in [distance::Kind::Polar, distance::Kind::XY] {
        let (mut doc, def, targets) = fixture();
        let p = distance::create(
            &mut doc,
            def,
            kind,
            Vector3::ZERO,
            Vector3::new(10., 1., 0.),
            Vector3::new(5., 2., 0.),
        )
        .unwrap();
        let action = create_with_frame(
            &mut doc,
            def,
            Kind::Stretch,
            p,
            &targets[..1],
            DVec3::new(3., 4., 0.),
            Some([DVec3::new(1.5, -1., 0.), DVec3::new(2.5, 1., 0.)]),
        )
        .unwrap();
        let baseline = doc.clone();
        evaluate(&mut doc, &baseline, def, p, DVec3::new(12., 4., 0.)).unwrap();
        let EntityType::Line(line) = doc.get_entity(targets[0]).unwrap() else {
            panic!()
        };
        assert_eq!(line.start, Vector3::UNIT_X);
        assert!(line.end.distance(&Vector3::new(4., 3., 0.)) < 1e-8);
        super::super::block_authoring::edit::move_part(
            &mut doc,
            def,
            action,
            super::super::block_authoring::edit::Part::Point,
            Vector3::new(6., 7., 0.),
        )
        .unwrap();
        let frame = frame_outline(&doc, action);
        assert_eq!(frame[0], Vector3::new(1.5, -1., 0.));
        reframe(
            &mut doc,
            def,
            &Transform::from_translation(Vector3::new(-2., -3., 0.)),
        );
        assert_eq!(frame_outline(&doc, action)[0], Vector3::new(-0.5, -4., 0.));
        let ObjectType::DynamicBlock(o) = &doc.objects[&action] else {
            panic!()
        };
        let DynamicBlockData::StretchAction(a) = &o.data else {
            panic!()
        };
        assert_eq!(a.handles[0].indexes, vec![1]);
    }
}

#[test]
fn block_actions_retarget_preserves_identity_and_native_roundtrip() {
    for kind in [Kind::Move, Kind::Scale, Kind::Rotate, Kind::Flip] {
        let (mut doc, def, targets) = fixture();
        let p = match kind {
            Kind::Rotate => orientation(
                &mut doc,
                def,
                super::super::block_orientation_parameter::Kind::Rotation,
            ),
            Kind::Flip => orientation(
                &mut doc,
                def,
                super::super::block_orientation_parameter::Kind::Flip,
            ),
            _ => linear(&mut doc, def),
        };
        let a = create(
            &mut doc,
            def,
            kind,
            p,
            &targets[..1],
            DVec3::new(3., 4., 0.),
        )
        .unwrap();
        let graph = authoring::graph(&doc, def).unwrap().unwrap();
        let graph_before = doc.objects[&graph].clone();
        assert!(retarget(&mut doc, def, a, &targets[1..], None).unwrap());
        assert_eq!(doc.objects[&graph], graph_before);
        let (_, action, k, _) = actions(&doc, def)
            .into_iter()
            .find(|(h, _, _, _)| *h == a)
            .unwrap();
        assert_eq!(k, kind);
        assert_eq!(action.dependencies, targets[1..]);
        assert_eq!(action.display_location, Vector3::new(3., 4., 0.));
        assert!(!retarget(&mut doc, def, a, &targets[1..], None).unwrap());
        let before = doc.clone();
        assert!(retarget(&mut doc, def, a, &[], None).is_err());
        assert!(retarget(&mut doc, def, a, &[Handle::new(999999)], None).is_err());
        assert_eq!(doc, before);
        for saved in [
            DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&doc).unwrap()))
                .read()
                .unwrap(),
            DxfReader::from_reader(Cursor::new(DxfWriter::new(&doc).write_to_vec().unwrap()))
                .unwrap()
                .read()
                .unwrap(),
        ] {
            assert_eq!(saved.objects[&a], doc.objects[&a]);
        }
    }
}

#[test]
fn block_actions_retarget_stretch_retains_vertices_and_replaces_frame() {
    let (mut doc, def, targets, p, a) = stretch_fixture();
    // Removing members must retain the old vertex bindings of retained members.
    retarget(&mut doc, def, a, &targets[..1], None).unwrap();
    let EntityType::Line(line) = doc.get_entity_mut(targets[0]).unwrap() else {
        panic!()
    };
    line.end.x = 20.; // Now outside the old frame; retained binding must not change.
    retarget(&mut doc, def, a, &targets[..2], None).unwrap();
    let baseline = doc.clone();
    evaluate(&mut doc, &baseline, def, p, DVec3::new(12., 0., 0.)).unwrap();
    let EntityType::Line(line) = doc.get_entity(targets[0]).unwrap() else {
        panic!()
    };
    assert_eq!(line.start, Vector3::ZERO);
    assert_eq!(line.end.x, 22.);
    let mut doc = baseline;
    retarget(
        &mut doc,
        def,
        a,
        &targets[..2],
        Some([DVec3::new(-1., -1., 0.), DVec3::new(1., 4., 0.)]),
    )
    .unwrap();
    let baseline = doc.clone();
    evaluate(&mut doc, &baseline, def, p, DVec3::new(12., 0., 0.)).unwrap();
    let EntityType::Line(line) = doc.get_entity(targets[0]).unwrap() else {
        panic!()
    };
    assert_eq!(line.start.x, 2.);
    assert_eq!(line.end.x, 20.);
    let saved = DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&baseline).unwrap()))
        .read()
        .unwrap();
    assert_eq!(saved.objects[&a], baseline.objects[&a]);
    let mut polygon = baseline.clone();
    reframe(
        &mut polygon,
        def,
        &Transform::from_translation(Vector3::ZERO),
    );
    retarget(&mut polygon, def, a, &targets, None).unwrap();
    assert_eq!(frame_outline(&polygon, a).len(), 4);
}

#[test]
fn block_actions_retarget_refresh_resets_old_targets_and_keeps_each_reference_value() {
    let (mut doc, def, targets) = fixture();
    let p = linear(&mut doc, def);
    let a = create(&mut doc, def, Kind::Move, p, &targets[..1], DVec3::ZERO).unwrap();
    let first = doc
        .add_entity(EntityType::Insert(codec::entities::Insert::new(
            "ACTIONS",
            Vector3::ZERO,
        )))
        .unwrap();
    let second = doc
        .add_entity(EntityType::Insert(codec::entities::Insert::new(
            "ACTIONS",
            Vector3::new(40., 0., 0.),
        )))
        .unwrap();
    doc = instance::evaluate(&doc, first, p, DVec3::new(18., 0., 0.))
        .unwrap()
        .unwrap();
    doc = instance::evaluate(&doc, second, p, DVec3::new(22., 0., 0.))
        .unwrap()
        .unwrap();
    let mut scene = Scene::new();
    scene.document = doc;
    let copied = scene.copy_entities(
        &[first],
        &crate::command::EntityTransform::Translate(DVec3::new(80., 0., 0.)),
    )[0];
    let mut doc = scene.document;
    let original_line = doc.get_entity(targets[0]).unwrap().clone();
    retarget(&mut doc, def, a, &targets[1..], None).unwrap();
    let doc = instance::refresh(&doc, def).unwrap();
    for (reference, delta) in [(first, 8.), (second, 12.), (copied, 8.)] {
        let EntityType::Insert(i) = doc.get_entity(reference).unwrap() else {
            panic!()
        };
        let members = &doc.block_records.get(&i.block_name).unwrap().entity_handles;
        assert_eq!(start(&doc, members[0]), Vector3::UNIT_X);
        let EntityType::Circle(circle) = doc.get_entity(members[1]).unwrap() else {
            panic!()
        };
        assert_eq!(circle.center, Vector3::new(4. + delta, 3., 0.));
        assert_eq!(
            instance::current(&doc, reference, &parameters(&doc, def, Kind::Move)[0]).unwrap(),
            DVec3::new(10. + delta, 0., 0.)
        );
    }
    assert_eq!(doc.get_entity(targets[0]).unwrap(), &original_line);
    // Re-evaluating a reference at a different value also resets removed targets.
    let changed = instance::evaluate(&doc, first, p, DVec3::new(14., 0., 0.))
        .unwrap()
        .unwrap();
    assert_eq!(instance_line(&changed, first), Vector3::UNIT_X);
}

#[test]
fn block_actions_retarget_stretch_can_remove_stale_binding_or_replace_it() {
    let (mut doc, def, targets, _, a) = stretch_fixture();
    let ObjectType::DynamicBlock(o) = doc.objects.get_mut(&a).unwrap() else {
        panic!()
    };
    let DynamicBlockData::StretchAction(action) = &mut o.data else {
        panic!()
    };
    action.handles[0].indexes = vec![99];
    let mut replace = doc.clone();
    retarget(&mut doc, def, a, &targets[1..], None).unwrap();
    retarget(
        &mut replace,
        def,
        a,
        &targets,
        Some([DVec3::new(9., -1., 0.), DVec3::new(11., 4., 0.)]),
    )
    .unwrap();
    let ObjectType::DynamicBlock(o) = &replace.objects[&a] else {
        panic!()
    };
    let DynamicBlockData::StretchAction(action) = &o.data else {
        panic!()
    };
    assert_eq!(action.handles[0].indexes, vec![1]);
}

#[test]
fn block_actions_stretch_duplicate_target_picks_create_one_binding() {
    let (mut doc, def, targets, p, _) = stretch_fixture();
    let a = create_with_frame(
        &mut doc,
        def,
        Kind::Stretch,
        p,
        &[targets[0], targets[0]],
        DVec3::ZERO,
        Some([DVec3::new(9., -1., 0.), DVec3::new(11., 4., 0.)]),
    )
    .unwrap();
    let ObjectType::DynamicBlock(o) = &doc.objects[&a] else {
        panic!()
    };
    let DynamicBlockData::StretchAction(action) = &o.data else {
        panic!()
    };
    assert_eq!(action.action.dependencies, vec![targets[0]]);
    assert_eq!(action.handles.len(), 1);
    let baseline = doc.clone();
    assert!(evaluate(&mut doc, &baseline, def, p, DVec3::new(12., 0., 0.)).unwrap());
}

#[test]
fn block_actions_topology_identity_reorder_add_delete_type_and_native_roundtrip() {
    let (mut doc, def, targets) = fixture();
    let p = linear(&mut doc, def);
    create(&mut doc, def, Kind::Move, p, &targets[..1], DVec3::ZERO).unwrap();
    let insert = doc
        .add_entity(EntityType::Insert(codec::entities::Insert::new(
            "ACTIONS",
            Vector3::ZERO,
        )))
        .unwrap();
    doc = instance::evaluate(&doc, insert, p, DVec3::new(12., 0., 0.))
        .unwrap()
        .unwrap();
    let before = doc.clone();
    let EntityType::Insert(i) = doc.get_entity(insert).unwrap() else {
        panic!()
    };
    let old = doc
        .block_records
        .get(&i.block_name)
        .unwrap()
        .entity_handles
        .clone();
    let old_id = authoring::element_id(doc.get_entity(targets[1]).unwrap()).unwrap();
    doc.remove_entity(targets[1]);
    doc.block_records
        .get_mut("ACTIONS")
        .unwrap()
        .entity_handles
        .retain(|h| *h != targets[1]);
    let mut line =
        codec::entities::Line::from_points(Vector3::new(20., 0., 0.), Vector3::new(21., 0., 0.));
    line.common.owner_handle = def;
    let new = doc.add_entity(EntityType::Line(line)).unwrap();
    authoring::tag_members(&mut doc, def);
    assert!(authoring::element_id(doc.get_entity(new).unwrap()).unwrap() > old_id);
    doc.block_records
        .get_mut("ACTIONS")
        .unwrap()
        .entity_handles
        .reverse();
    let doc = instance::refresh_after_edit(&doc, &before, def).unwrap();
    let EntityType::Insert(i) = doc.get_entity(insert).unwrap() else {
        panic!()
    };
    let members = &doc.block_records.get(&i.block_name).unwrap().entity_handles;
    assert_eq!(
        members[1], old[0],
        "surviving member keeps identity despite reordering"
    );
    assert_eq!(start(&doc, members[0]), Vector3::new(20., 0., 0.));
    assert_eq!(start(&doc, members[1]), Vector3::new(3., 0., 0.));
    assert!(doc.get_entity(old[1]).is_none());
    let mut changed = doc.clone();
    let mut circle = codec::entities::Circle::from_center_radius(Vector3::new(6., 6., 0.), 1.);
    circle.common = changed.get_entity(new).unwrap().common().clone();
    *changed.get_entity_mut(new).unwrap() = EntityType::Circle(circle);
    let changed = instance::refresh_after_edit(&changed, &doc, def).unwrap();
    for saved in [
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
        let EntityType::Insert(i) = saved.get_entity(insert).unwrap() else {
            panic!()
        };
        let members = &saved
            .block_records
            .get(&i.block_name)
            .unwrap()
            .entity_handles;
        assert!(matches!(
            saved.get_entity(members[0]),
            Some(EntityType::Circle(_))
        ));
        assert_eq!(start(&saved, members[1]), Vector3::new(3., 0., 0.));
    }
}

#[test]
fn block_actions_topology_copy_tags_and_extension_dictionary_do_not_share() {
    use codec::objects::XRecord;
    let (mut doc, def, targets) = fixture();
    let p = linear(&mut doc, def);
    create(&mut doc, def, Kind::Move, p, &targets[..1], DVec3::ZERO).unwrap();
    let insert = doc
        .add_entity(EntityType::Insert(codec::entities::Insert::new(
            "ACTIONS",
            Vector3::ZERO,
        )))
        .unwrap();
    doc = instance::evaluate(&doc, insert, p, DVec3::new(12., 0., 0.))
        .unwrap()
        .unwrap();
    let mut duplicate = doc.get_entity(targets[0]).unwrap().clone();
    duplicate.common_mut().handle = Handle::NULL;
    let new = doc.add_entity(duplicate).unwrap();
    let original_id = authoring::element_id(doc.get_entity(targets[0]).unwrap());
    doc.block_records
        .get_mut("ACTIONS")
        .unwrap()
        .entity_handles
        .rotate_right(1);
    authoring::tag_members(&mut doc, def);
    assert_eq!(
        authoring::element_id(doc.get_entity(targets[0]).unwrap()),
        original_id
    );
    assert_ne!(
        authoring::element_id(doc.get_entity(new).unwrap()),
        authoring::element_id(doc.get_entity(targets[0]).unwrap())
    );
    let root = doc.ensure_extension_dictionary(new);
    let handle = doc.allocate_handle();
    let mut record = XRecord::default();
    record.handle = handle;
    record.owner = root;
    doc.objects.insert(handle, ObjectType::XRecord(record));
    let ObjectType::Dictionary(d) = doc.objects.get_mut(&root).unwrap() else {
        panic!()
    };
    d.add_entry("DATA", handle);
    let before = doc.clone();
    let refreshed = instance::refresh_after_edit(&doc, &before, def).unwrap();
    let EntityType::Insert(i) = refreshed.get_entity(insert).unwrap() else {
        panic!()
    };
    let new_id = authoring::element_id(refreshed.get_entity(new).unwrap());
    let target = *refreshed
        .block_records
        .get(&i.block_name)
        .unwrap()
        .entity_handles
        .iter()
        .find(|h| authoring::element_id(refreshed.get_entity(**h).unwrap()) == new_id)
        .unwrap();
    let cloned = refreshed
        .get_entity(target)
        .unwrap()
        .common()
        .xdictionary_handle
        .unwrap();
    assert_ne!(cloned, root);
    let ObjectType::Dictionary(d) = &refreshed.objects[&cloned] else {
        panic!()
    };
    assert_ne!(d.get("DATA"), Some(handle));
}

#[test]
fn block_actions_topology_preserves_visibility_choice_after_discriminator_deleted() {
    use crate::scene::block_visibility as visibility;
    let (mut doc, def, targets) = fixture();
    let p = linear(&mut doc, def);
    create(&mut doc, def, Kind::Move, p, &targets[..1], DVec3::ZERO).unwrap();
    let visibility = visibility::create(&mut doc, def, Vector3::ZERO).unwrap();
    visibility::add_state(&mut doc, visibility, "Alternate", 0).unwrap();
    visibility::set_visible(&mut doc, visibility, 1, &targets[1..], false);
    visibility::finish_editing(&mut doc, def);
    let first = doc
        .add_entity(EntityType::Insert(codec::entities::Insert::new(
            "ACTIONS",
            Vector3::ZERO,
        )))
        .unwrap();
    let second = doc
        .add_entity(EntityType::Insert(codec::entities::Insert::new(
            "ACTIONS",
            Vector3::new(40., 0., 0.),
        )))
        .unwrap();
    doc = instance::evaluate(&doc, first, p, DVec3::new(18., 0., 0.))
        .unwrap()
        .unwrap();
    doc = instance::evaluate(&doc, second, p, DVec3::new(12., 0., 0.))
        .unwrap()
        .unwrap();
    representation::apply_visibility(&mut doc, def, second, 1).unwrap();
    let before = doc.clone();
    doc.remove_entity(targets[1]);
    doc.block_records
        .get_mut("ACTIONS")
        .unwrap()
        .entity_handles
        .retain(|h| *h != targets[1]);
    let mut entity =
        codec::entities::Line::from_points(Vector3::new(20., 0., 0.), Vector3::new(21., 0., 0.));
    entity.common.owner_handle = def;
    let new = doc.add_entity(EntityType::Line(entity)).unwrap();
    visibility::set_visible(&mut doc, visibility, 0, &[new], false);
    visibility::finish_editing(&mut doc, def);
    let refreshed = instance::refresh_after_edit(&doc, &before, def).unwrap();
    for (insert, hidden, start_x) in [(first, true, 9.), (second, false, 3.)] {
        let EntityType::Insert(i) = refreshed.get_entity(insert).unwrap() else {
            panic!()
        };
        let members = &refreshed
            .block_records
            .get(&i.block_name)
            .unwrap()
            .entity_handles;
        assert_eq!(members.len(), 2);
        assert_eq!(start(&refreshed, members[0]).x, start_x);
        assert_eq!(
            refreshed.get_entity(members[1]).unwrap().common().invisible,
            hidden
        );
    }
}

#[test]
fn block_actions_topology_nested_insert_attribute_handles_remain_independent() {
    let (mut doc, def, targets) = fixture();
    let p = linear(&mut doc, def);
    create(&mut doc, def, Kind::Move, p, &targets[..1], DVec3::ZERO).unwrap();
    let insert = doc
        .add_entity(EntityType::Insert(codec::entities::Insert::new(
            "ACTIONS",
            Vector3::ZERO,
        )))
        .unwrap();
    doc = instance::evaluate(&doc, insert, p, DVec3::new(12., 0., 0.))
        .unwrap()
        .unwrap();
    let mut scene = Scene::new();
    scene.document = doc;
    scene.define_block_raw(
        "CHILD",
        Vector3::ZERO,
        vec![EntityType::Line(codec::entities::Line::from_points(
            Vector3::ZERO,
            Vector3::UNIT_Y,
        ))],
    );
    let mut child = codec::entities::Insert::new("CHILD", Vector3::new(20., 0., 0.));
    child.common.owner_handle = def;
    let mut attribute = codec::entities::AttributeEntity::new("TAG".into(), "value".into());
    attribute.common.handle = scene.document.allocate_handle();
    child.attributes.push(attribute);
    child.seqend_handle = Some(scene.document.allocate_handle());
    let source = scene
        .document
        .add_entity(EntityType::Insert(child))
        .unwrap();
    let before = scene.document.clone();
    let doc = instance::refresh_after_edit(&before, &before, def).unwrap();
    let doc = instance::refresh_after_edit(&doc, &doc, def).unwrap();
    let reopened = DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&doc).unwrap()))
        .read()
        .unwrap();
    let EntityType::Insert(parent) = reopened.get_entity(insert).unwrap() else {
        panic!()
    };
    let member = *reopened
        .block_records
        .get(&parent.block_name)
        .unwrap()
        .entity_handles
        .last()
        .unwrap();
    let EntityType::Insert(copied) = reopened.get_entity(member).unwrap() else {
        panic!()
    };
    let EntityType::Insert(original) = reopened.get_entity(source).unwrap() else {
        panic!()
    };
    assert_eq!(copied.attributes[0].value, "value");
    assert_ne!(
        copied.attributes[0].common.handle,
        original.attributes[0].common.handle
    );
    assert_eq!(copied.attributes[0].common.owner_handle, member);
    assert_ne!(copied.seqend_handle, original.seqend_handle);
}

#[test]
fn block_actions_array_native_creation_and_dwg_dxf_roundtrip() {
    for kind in [distance::Kind::Linear, distance::Kind::Polar, distance::Kind::XY] {
        let (mut doc, def, targets) = fixture();
        let end_pt = if kind == distance::Kind::XY {
            Vector3::new(10., 10., 0.)
        } else {
            Vector3::new(10., 0., 0.)
        };
        let p = distance::create(
            &mut doc,
            def,
            kind,
            Vector3::ZERO,
            end_pt,
            Vector3::new(5., 2., 0.),
        )
        .unwrap();
        let column_offset = 2.5;
        let row_offset = if kind == distance::Kind::XY { 3.5 } else { 1.0 };
        let action = create_array(
            &mut doc,
            def,
            p,
            &[targets[0]],
            DVec3::new(5., 5., 0.),
            column_offset,
            row_offset,
        )
        .unwrap();

        let ObjectType::DynamicBlock(o) = doc.objects.get(&action).unwrap() else {
            panic!()
        };
        let DynamicBlockData::ArrayAction(arr) = &o.data else {
            panic!()
        };
        assert_eq!(arr.column_offset, column_offset);
        assert_eq!(arr.row_offset, row_offset);
        assert_eq!(arr.action.dependencies, vec![targets[0]]);
        assert_eq!(arr.connections.len(), 4);
        let param_id = parameters(&doc, def, Kind::Array)[0].id;
        assert_eq!(
            arr.connections,
            [
                BlockConnection {
                    code: param_id,
                    name: "Base".into(),
                },
                BlockConnection {
                    code: param_id,
                    name: "End".into(),
                },
                BlockConnection {
                    code: param_id,
                    name: "UpdatedBase".into(),
                },
                BlockConnection {
                    code: param_id,
                    name: "UpdatedEnd".into(),
                },
            ]
        );

        // DWG roundtrip
        let dwg_bytes = DwgWriter::write_to_vec(&doc).unwrap();
        let dwg_doc = DwgReader::from_stream(Cursor::new(dwg_bytes)).read().unwrap();
        let ObjectType::DynamicBlock(dwg_o) = dwg_doc.objects.get(&action).unwrap() else {
            panic!()
        };
        let DynamicBlockData::ArrayAction(dwg_arr) = &dwg_o.data else {
            panic!()
        };
        assert_eq!(dwg_arr.column_offset, column_offset);
        assert_eq!(dwg_arr.row_offset, row_offset);
        assert_eq!(dwg_arr.action.dependencies, vec![targets[0]]);
        assert_eq!(dwg_arr.connections, arr.connections);

        // DXF roundtrip
        let dxf_bytes = DxfWriter::new(&doc).write_to_vec().unwrap();
        let dxf_doc = DxfReader::from_reader(Cursor::new(dxf_bytes)).unwrap().read().unwrap();
        let ObjectType::DynamicBlock(dxf_o) = dxf_doc.objects.get(&action).unwrap() else {
            panic!()
        };
        let DynamicBlockData::ArrayAction(dxf_arr) = &dxf_o.data else {
            panic!()
        };
        assert_eq!(dxf_arr.column_offset, column_offset);
        assert_eq!(dxf_arr.row_offset, row_offset);
        assert_eq!(dxf_arr.action.dependencies, vec![targets[0]]);
        assert_eq!(dxf_arr.connections, arr.connections);
    }
}

#[test]
fn block_actions_array_linear_and_polar_dynamic_evaluation() {
    let (mut doc, def, targets) = fixture();
    let p = distance::create(
        &mut doc,
        def,
        distance::Kind::Linear,
        Vector3::ZERO,
        Vector3::new(10., 0., 0.),
        Vector3::new(5., 2., 0.),
    )
    .unwrap();
    create_array(
        &mut doc,
        def,
        p,
        &[targets[0]],
        DVec3::ZERO,
        2.0,
        1.0,
    )
    .unwrap();

    let baseline = doc.clone();

    // At distance 1.0 (< column_offset 2.0), only base item exists
    evaluate(&mut doc, &baseline, def, p, DVec3::new(1.0, 0.0, 0.0)).unwrap();
    let members = authoring::members(&doc, def);
    assert_eq!(members.len(), 2); // original line + circle

    // At distance 2.0 (>= 2.0), 1 clone is added -> 3 members total
    evaluate(&mut doc, &baseline, def, p, DVec3::new(2.0, 0.0, 0.0)).unwrap();
    let members = authoring::members(&doc, def);
    assert_eq!(members.len(), 3);
    let lines: Vec<_> = members
        .iter()
        .filter_map(|&h| match doc.get_entity(h) {
            Some(EntityType::Line(l)) => Some((h, l.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(lines.len(), 2);
    // Original line was from (1,0,0) to (2,0,0). Clone should be translated by (2,0,0): from (3,0,0) to (4,0,0)
    assert!(lines.iter().any(|(_, l)| (l.start - Vector3::new(3., 0., 0.)).length() < 1e-8));

    // At distance 6.5, items count = floor(6.5/2.0)+1 = 4 -> 3 clones + 1 base = 4 lines + 1 circle = 5 members
    evaluate(&mut doc, &baseline, def, p, DVec3::new(6.5, 0.0, 0.0)).unwrap();
    let members = authoring::members(&doc, def);
    assert_eq!(members.len(), 5);

    // Shrink back to 0.5: clones are removed, reverting to original 2 members
    evaluate(&mut doc, &baseline, def, p, DVec3::new(0.5, 0.0, 0.0)).unwrap();
    let members = authoring::members(&doc, def);
    assert_eq!(members.len(), 2);

    // Polar parameter at angle 45 deg
    let (mut doc, def, targets) = fixture();
    let p_polar = distance::create(
        &mut doc,
        def,
        distance::Kind::Polar,
        Vector3::ZERO,
        Vector3::new(10., 0., 0.),
        Vector3::new(5., 2., 0.),
    )
    .unwrap();
    create_array(
        &mut doc,
        def,
        p_polar,
        &[targets[0]],
        DVec3::ZERO,
        2.0,
        1.0,
    )
    .unwrap();
    let baseline = doc.clone();
    // Grip moved to (3.0, 4.0, 0) -> distance 5.0, axis (0.6, 0.8, 0)
    // n = floor(5.0/2.0) + 1 = 3 items (1 base + 2 clones)
    evaluate(&mut doc, &baseline, def, p_polar, DVec3::new(3.0, 4.0, 0.0)).unwrap();
    let members = authoring::members(&doc, def);
    assert_eq!(members.len(), 4); // 3 lines + 1 circle
    let lines: Vec<_> = members
        .iter()
        .filter_map(|&h| match doc.get_entity(h) {
            Some(EntityType::Line(l)) => Some((h, l.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(lines.len(), 3);
    assert!(lines.iter().any(|(_, l)| (l.start - Vector3::new(1.0 + 1.2, 1.6, 0.)).length() < 1e-8));
    assert!(lines.iter().any(|(_, l)| (l.start - Vector3::new(1.0 + 2.4, 3.2, 0.)).length() < 1e-8));
}

#[test]
fn block_actions_array_xy_grid_evaluation() {
    let (mut doc, def, targets) = fixture();
    let p_xy = distance::create(
        &mut doc,
        def,
        distance::Kind::XY,
        Vector3::ZERO,
        Vector3::new(10., 10., 0.),
        Vector3::new(5., 5., 0.),
    )
    .unwrap();
    create_array(
        &mut doc,
        def,
        p_xy,
        &[targets[0]],
        DVec3::ZERO,
        3.0,
        4.0,
    )
    .unwrap();
    let baseline = doc.clone();

    // Move XY grip to (6.0, 8.0, 0.0)
    // nx = floor(6/3)+1 = 3 (cols 0, 3, 6)
    // ny = floor(8/4)+1 = 3 (rows 0, 4, 8)
    // total items: 3 * 3 = 9 (1 base + 8 clones)
    evaluate(&mut doc, &baseline, def, p_xy, DVec3::new(6.0, 8.0, 0.0)).unwrap();
    let members = authoring::members(&doc, def);
    assert_eq!(members.len(), 10); // 9 lines + 1 circle

    let lines: Vec<_> = members
        .iter()
        .filter_map(|&h| match doc.get_entity(h) {
            Some(EntityType::Line(l)) => Some(l.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(lines.len(), 9);

    // Verify all 9 grid positions exist: (col*3, row*4) + base (1, 0, 0)
    for col in 0..3 {
        for row in 0..3 {
            let expected_start = Vector3::new(1.0 + (col as f64) * 3.0, (row as f64) * 4.0, 0.0);
            assert!(
                lines.iter().any(|l| (l.start - expected_start).length() < 1e-8),
                "Missing line at col {col}, row {row}"
            );
        }
    }
}

#[test]
fn block_actions_array_saved_references_independent_copy_and_roundtrip() {
    let (mut doc, def, targets) = fixture();
    let p = distance::create(
        &mut doc,
        def,
        distance::Kind::Linear,
        Vector3::ZERO,
        Vector3::new(10., 0., 0.),
        Vector3::new(5., 2., 0.),
    )
    .unwrap();
    create_array(
        &mut doc,
        def,
        p,
        &[targets[0]],
        DVec3::ZERO,
        2.0,
        1.0,
    )
    .unwrap();

    let a = doc
        .add_entity(EntityType::Insert(codec::entities::Insert::new(
            "ACTIONS",
            Vector3::ZERO,
        )))
        .unwrap();
    let b = doc
        .add_entity(EntityType::Insert(codec::entities::Insert::new(
            "ACTIONS",
            Vector3::new(100., 0., 0.),
        )))
        .unwrap();

    // Evaluate A at distance 6.0 (3 clones + 1 base = 4 lines)
    let doc = instance::evaluate(&doc, a, p, DVec3::new(6.0, 0.0, 0.0))
        .unwrap()
        .unwrap();
    // Evaluate B at distance 2.0 (1 clone + 1 base = 2 lines)
    let mut doc = instance::evaluate(&doc, b, p, DVec3::new(2.0, 0.0, 0.0))
        .unwrap()
        .unwrap();

    let EntityType::Insert(ref_a) = doc.get_entity(a).unwrap() else {
        panic!()
    };
    let EntityType::Insert(ref_b) = doc.get_entity(b).unwrap() else {
        panic!()
    };
    assert_ne!(ref_a.block_name, ref_b.block_name);
    let block_a = doc.block_records.get(&ref_a.block_name).unwrap();
    let block_b = doc.block_records.get(&ref_b.block_name).unwrap();
    let a_lines = block_a
        .entity_handles
        .iter()
        .filter(|&&h| matches!(doc.get_entity(h), Some(EntityType::Line(_))))
        .count();
    let b_lines = block_b
        .entity_handles
        .iter()
        .filter(|&&h| matches!(doc.get_entity(h), Some(EntityType::Line(_))))
        .count();
    assert_eq!(a_lines, 4);
    assert_eq!(b_lines, 2);

    // Copy reference A -> C
    let mut insert_c = ref_a.clone();
    let c = doc.allocate_handle();
    insert_c.common.handle = c;
    insert_c.insert_point = Vector3::new(200., 0., 0.);
    doc.add_entity(EntityType::Insert(insert_c)).unwrap();

    // Roundtrip to DWG
    let reopened = DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&doc).unwrap()))
        .read()
        .unwrap();
    let EntityType::Insert(reopened_a) = reopened.get_entity(a).unwrap() else {
        panic!()
    };
    let EntityType::Insert(reopened_b) = reopened.get_entity(b).unwrap() else {
        panic!()
    };
    let EntityType::Insert(reopened_c) = reopened.get_entity(c).unwrap() else {
        panic!()
    };
    let rec_a = reopened.block_records.get(&reopened_a.block_name).unwrap();
    let rec_b = reopened.block_records.get(&reopened_b.block_name).unwrap();
    let rec_c = reopened.block_records.get(&reopened_c.block_name).unwrap();
    assert_eq!(
        rec_a
            .entity_handles
            .iter()
            .filter(|&&h| matches!(reopened.get_entity(h), Some(EntityType::Line(_))))
            .count(),
        4
    );
    assert_eq!(
        rec_b
            .entity_handles
            .iter()
            .filter(|&&h| matches!(reopened.get_entity(h), Some(EntityType::Line(_))))
            .count(),
        2
    );
    assert_eq!(
        rec_c
            .entity_handles
            .iter()
            .filter(|&&h| matches!(reopened.get_entity(h), Some(EntityType::Line(_))))
            .count(),
        4
    );
}

#[test]
fn block_actions_array_rejections() {
    let (mut doc, def, targets) = fixture();
    let p_point = point::create(&mut doc, def, Vector3::ZERO, Vector3::new(5., 5., 0.)).unwrap();
    assert!(create_array(&mut doc, def, p_point, &targets, DVec3::ZERO, 2.0, 1.0).is_err());

    let p_rot = orientation(
        &mut doc,
        def,
        super::super::block_orientation_parameter::Kind::Rotation,
    );
    assert!(create_array(&mut doc, def, p_rot, &targets, DVec3::ZERO, 2.0, 1.0).is_err());

    let p_lin = distance::create(
        &mut doc,
        def,
        distance::Kind::Linear,
        Vector3::ZERO,
        Vector3::new(10., 0., 0.),
        Vector3::new(5., 2., 0.),
    )
    .unwrap();
    assert!(create_array(&mut doc, def, p_lin, &targets, DVec3::ZERO, 0.0, 1.0).is_err());
    assert!(create_array(&mut doc, def, p_lin, &targets, DVec3::ZERO, -2.0, 1.0).is_err());
}


#[test]
fn block_action_overrides_evaluate_and_roundtrip_without_accumulation() {
    for stretch in [false, true] {
        let (mut doc, def, targets, parameter, action) = if stretch {
            stretch_fixture()
        } else {
            let (mut doc, def, targets) = fixture();
            let parameter = linear(&mut doc, def);
            let action = create(&mut doc, def, Kind::Move, parameter, &targets, DVec3::ZERO).unwrap();
            (doc, def, targets, parameter, action)
        };
        assert!(options::set(&mut doc, def, action, 2., std::f64::consts::FRAC_PI_2).unwrap());
        assert!(!options::set(&mut doc, def, action, 2., std::f64::consts::FRAC_PI_2).unwrap());
        let before = doc.clone();
        for (multiplier, angle) in [(f64::NAN,0.),(1.,f64::INFINITY)] {
            assert!(options::set(&mut doc, def, action, multiplier, angle).is_err());
            assert_eq!(doc, before);
        }
        let record = doc.objects[&action].clone();
        let baseline = doc.clone();
        for distance in [15.,12.,10.] {
            evaluate(&mut doc, &baseline, def, parameter, DVec3::new(distance,0.,0.)).unwrap();
            let EntityType::Line(line) = doc.get_entity(targets[0]).unwrap() else { panic!() };
            let offset = (distance-10.)*2.;
            let (expected_start, expected_end) = if stretch {
                (Vector3::ZERO,Vector3::new(10.,offset,0.))
            } else {
                (Vector3::new(1.,offset,0.),Vector3::new(2.,offset,0.))
            };
            assert!((line.start-expected_start).length()<1e-10);
            assert!((line.end-expected_end).length()<1e-10);
            assert_eq!(doc.objects[&action], record);
        }
        for reopened in [
            DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&baseline).unwrap())).read().unwrap(),
            DxfReader::from_reader(Cursor::new(DxfWriter::new(&baseline).write_to_vec().unwrap())).unwrap().read().unwrap(),
        ] {
            assert_eq!(reopened.objects[&action], record);
            let name = reopened.block_records.iter().find(|r| r.handle==def).unwrap().name.clone();
            let mut source = reopened.clone();
            let reference = source.add_entity(EntityType::Insert(codec::entities::Insert::new(name,Vector3::ZERO))).unwrap();
            let changed = instance::evaluate(&source,reference,parameter,DVec3::new(15.,0.,0.)).unwrap().unwrap();
            let EntityType::Insert(insert) = changed.get_entity(reference).unwrap() else { panic!() };
            let evaluated = changed.block_records.get(&insert.block_name).unwrap().entity_handles[0];
            let EntityType::Line(line) = changed.get_entity(evaluated).unwrap() else { panic!() };
            let expected = if stretch { Vector3::new(10.,10.,0.) } else { Vector3::new(2.,10.,0.) };
            assert!((line.end-expected).length()<1e-10);
            assert_eq!(source.get_entity(targets[0]),reopened.get_entity(targets[0]));
        }
    }
}

#[test]
fn block_stretch_supports_curves_points_and_text_with_native_vertex_membership() {
    use codec::entities::{Arc,Circle,Ellipse,Point,Text,MText};
    let mut text=Text::new();
    text.insertion_point=Vector3::new(-2.,0.,0.);
    text.normal=Vector3::new(0.,0.,-1.);
    let mut mtext=MText::new();
    mtext.insertion_point=Vector3::new(2.,0.,0.);
    let mut scene=Scene::new();
    scene.define_block_raw("CURVE_STRETCH",Vector3::ZERO,vec![
        EntityType::Arc(Arc::from_center_radius_angles(Vector3::ZERO,2.,0.,std::f64::consts::PI)),
        EntityType::Circle(Circle::from_center_radius(Vector3::new(2.,0.,0.),1.)),
        EntityType::Ellipse(Ellipse::from_center_axes(Vector3::new(2.,0.,0.),Vector3::UNIT_X,0.5)),
        EntityType::Point(Point::from_coords(2.,0.,0.)),
        EntityType::Text(text),EntityType::MText(Box::new(mtext)),
    ]);
    let record=scene.document.block_records.get("CURVE_STRETCH").unwrap();
    let (definition,targets)=(record.handle,record.entity_handles.clone());
    let mut doc=scene.document;
    let parameter=linear(&mut doc,definition);
    let action=create_with_frame(&mut doc,definition,Kind::Stretch,parameter,&targets,DVec3::ZERO,
        Some([DVec3::new(1.,-1.,0.),DVec3::new(3.,1.,0.)])).unwrap();
    let baseline=doc.clone();
    for reopened in [
        baseline.clone(),
        DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&baseline).unwrap())).read().unwrap(),
        DxfReader::from_reader(Cursor::new(DxfWriter::new(&baseline).write_to_vec().unwrap())).unwrap().read().unwrap(),
    ] {
        assert_eq!(reopened.objects[&action],baseline.objects[&action]);
        let mut changed=reopened.clone();
        evaluate(&mut changed,&reopened,definition,parameter,DVec3::new(12.,0.,0.)).unwrap();
        let EntityType::Arc(arc)=changed.get_entity(targets[0]).unwrap() else {panic!()};
        assert!((arc.start_point_wcs()-Vector3::new(4.,0.,0.)).length()<1e-10);
        assert!((arc.end_point_wcs()-Vector3::new(-2.,0.,0.)).length()<1e-10);
        let height=arc.radius*(1.-(arc.sweep_angle()/2.).cos());
        assert!((height-2.).abs()<1e-10,"arc height is retained");
        for target in &targets[1..] {
            let points=stretch::control_points(changed.get_entity(*target).unwrap()).unwrap();
            assert!((points[0]-DVec3::new(4.,0.,0.)).length()<1e-10);
        }
        evaluate(&mut changed,&reopened,definition,parameter,DVec3::new(10.,0.,0.)).unwrap();
        assert_eq!(changed.get_entity(targets[0]),reopened.get_entity(targets[0]));
    }
    let mut collapsed=baseline.clone();
    assert!(evaluate(&mut collapsed,&baseline,definition,parameter,DVec3::new(6.,0.,0.)).is_err());
    assert_eq!(collapsed,baseline,"a collapsed arc chord leaves the document unchanged");
}

#[test]
fn block_stretch_accepts_point_parameters_and_freezes_endpoint_membership() {
    let (mut doc,definition,targets)=fixture();
    let parameter=point::create(&mut doc,definition,Vector3::ZERO,Vector3::UNIT_Y).unwrap();
    let action=create_with_frame(&mut doc,definition,Kind::Stretch,parameter,&targets[..1],DVec3::ZERO,
        Some([DVec3::new(1.5,-1.,0.),DVec3::new(2.5,1.,0.)])).unwrap();
    let baseline=doc.clone();
    for value in [DVec3::new(2.,3.,0.),DVec3::new(4.,5.,0.)] {
        evaluate(&mut doc,&baseline,definition,parameter,value).unwrap();
        let EntityType::Line(line)=doc.get_entity(targets[0]).unwrap() else {panic!()};
        assert_eq!(line.start,Vector3::UNIT_X);
        assert_eq!(line.end,Vector3::new(2.+value.x,value.y,0.));
        assert_eq!(doc.objects[&action],baseline.objects[&action]);
    }
}

#[test]
fn block_polygon_stretch_frames_bind_concave_regions_and_replace_atomically() {
    let mut scene=Scene::new();
    scene.define_block_raw("POLYGON_FRAME",Vector3::ZERO,vec![EntityType::Line(codec::entities::Line::from_points(
        Vector3::new(0.5,3.,0.),Vector3::new(3.,3.,0.)))]);
    let record=scene.document.block_records.get("POLYGON_FRAME").unwrap();
    let (definition,targets)=(record.handle,record.entity_handles.clone());
    let mut doc=scene.document;
    let parameter=linear(&mut doc,definition);
    let frame=vec![DVec3::ZERO,DVec3::new(4.,0.,0.),DVec3::new(4.,1.,0.),
        DVec3::new(1.,1.,0.),DVec3::new(1.,4.,0.),DVec3::new(0.,4.,0.)];
    let action=create_with_frame_points(&mut doc,definition,Kind::Stretch,parameter,None,&targets,DVec3::ZERO,Some(&frame),None).unwrap();
    let ObjectType::DynamicBlock(object)=&doc.objects[&action] else {panic!()};
    let DynamicBlockData::StretchAction(native)=&object.data else {panic!()};
    assert_eq!(native.points.len(),6);
    assert_eq!(native.handles[0].indexes,vec![0]);
    let baseline=doc.clone();
    for reopened in [
        baseline.clone(),
        DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&baseline).unwrap())).read().unwrap(),
        DxfReader::from_reader(Cursor::new(DxfWriter::new(&baseline).write_to_vec().unwrap())).unwrap().read().unwrap(),
    ] {
        assert_eq!(reopened.objects[&action],baseline.objects[&action]);
        let mut changed=reopened.clone();
        for distance in [12.,14.] {
            evaluate(&mut changed,&reopened,definition,parameter,DVec3::new(distance,0.,0.)).unwrap();
            let EntityType::Line(line)=changed.get_entity(targets[0]).unwrap() else {panic!()};
            assert_eq!(line.start,Vector3::new(distance-9.5,3.,0.));
            assert_eq!(line.end,Vector3::new(3.,3.,0.));
        }
    }
    for bad in [
        vec![DVec3::ZERO,DVec3::X,DVec3::new(2.,0.,0.)],
        vec![DVec3::ZERO,DVec3::new(2.,2.,0.),DVec3::new(0.,2.,0.),DVec3::new(2.,0.,0.)],
        vec![DVec3::ZERO,DVec3::X,DVec3::new(0.,1.,1.)],
        vec![DVec3::ZERO,DVec3::X,DVec3::Y,DVec3::X],
    ] {
        assert!(retarget_frame_points(&mut doc,definition,action,&targets,Some(&bad)).is_err());
        assert_eq!(doc,baseline);
    }
    let triangle=vec![DVec3::new(2.,2.,0.),DVec3::new(4.,2.,0.),DVec3::new(3.,4.,0.)];
    assert!(retarget_frame_points(&mut doc,definition,action,&targets,Some(&triangle)).unwrap());
    let ObjectType::DynamicBlock(object)=&doc.objects[&action] else {panic!()};
    let DynamicBlockData::StretchAction(native)=&object.data else {panic!()};
    assert_eq!(native.points.len(),3);
    assert_eq!(native.handles[0].indexes,vec![1]);
    assert_eq!(native.action.dependencies,targets);
}

#[test]
fn polar_stretch_rotates_and_extends_separate_selections_with_native_overrides() {
    // A rotation-only override must restore the saved frame membership when cleared.
    let (mut partial, def, partial_targets, _, partial_action) = polar_stretch_fixture();
    let original = partial.objects[&partial_action].clone();
    options::set_rotate_only(&mut partial, def, partial_action, &partial_targets).unwrap();
    let ObjectType::DynamicBlock(object) = &partial.objects[&partial_action] else { panic!() };
    let DynamicBlockData::PolarStretchAction(native) = &object.data else { panic!() };
    assert!(native.bindings.is_empty());
    options::set_rotate_only(&mut partial, def, partial_action, &[]).unwrap();
    assert_eq!(partial.objects[&partial_action], original);
    let mut scene=Scene::new();
    scene.define_block_raw("POLAR_GROUPS",Vector3::ZERO,vec![
        EntityType::Line(codec::entities::Line::from_points(Vector3::ZERO,Vector3::new(10.,0.,0.))),
        EntityType::Line(codec::entities::Line::from_points(Vector3::new(1.,1.,0.),Vector3::new(2.,1.,0.))),
        EntityType::Circle(codec::entities::Circle::from_center_radius(Vector3::new(10.,2.,0.),1.)),
        EntityType::Point(codec::entities::Point::from_coords(4.,4.,0.)),
    ]);
    let record=scene.document.block_records.get("POLAR_GROUPS").unwrap();
    let (definition,targets)=(record.handle,record.entity_handles.clone());
    let mut doc=scene.document;
    let parameter=distance::create(&mut doc,definition,distance::Kind::Polar,Vector3::ZERO,
        Vector3::new(10.,0.,0.),Vector3::new(5.,2.,0.)).unwrap();
    let action=create_with_frame(&mut doc,definition,Kind::PolarStretch,parameter,&targets[..3],DVec3::ZERO,
        Some([DVec3::new(9.,-1.,0.),DVec3::new(11.,1.,0.)])).unwrap();
    assert!(options::set_rotate_only(&mut doc,definition,action,&targets[1..2]).unwrap());
    assert!(!options::set_rotate_only(&mut doc,definition,action,&targets[1..2]).unwrap());
    let before=doc.clone();
    assert!(options::set_rotate_only(&mut doc,definition,action,&targets[3..]).is_err());
    assert_eq!(doc,before);
    options::set(&mut doc,definition,action,2.,0.).unwrap();
    let baseline=doc.clone();
    for reopened in [baseline.clone(),
        DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&baseline).unwrap())).read().unwrap(),
        DxfReader::from_reader(Cursor::new(DxfWriter::new(&baseline).write_to_vec().unwrap())).unwrap().read().unwrap(),
    ] {
        assert_eq!(reopened.objects[&action],baseline.objects[&action]);
        let mut changed=reopened.clone();
        evaluate(&mut changed,&reopened,definition,parameter,DVec3::new(0.,15.,0.)).unwrap();
        let EntityType::Line(stretched)=changed.get_entity(targets[0]).unwrap() else {panic!()};
        assert!((stretched.start-Vector3::ZERO).length()<1e-10);
        assert!((stretched.end-Vector3::new(0.,20.,0.)).length()<1e-10);
        let EntityType::Line(rotated)=changed.get_entity(targets[1]).unwrap() else {panic!()};
        assert!((rotated.start-Vector3::new(-1.,1.,0.)).length()<1e-10);
        assert!((rotated.end-Vector3::new(-1.,2.,0.)).length()<1e-10);
        let EntityType::Circle(moved)=changed.get_entity(targets[2]).unwrap() else {panic!()};
        assert!((moved.center-Vector3::new(-2.,20.,0.)).length()<1e-10);
        assert_eq!(changed.get_entity(targets[3]),reopened.get_entity(targets[3]));
        evaluate(&mut changed,&reopened,definition,parameter,DVec3::new(10.,0.,0.)).unwrap();
        for target in &targets {assert_eq!(changed.get_entity(*target),reopened.get_entity(*target));}
        let mut source=reopened.clone();
        let insert=source.add_entity(EntityType::Insert(codec::entities::Insert::new("POLAR_GROUPS",Vector3::ZERO))).unwrap();
        let changed=instance::evaluate(&source,insert,parameter,DVec3::new(0.,15.,0.)).unwrap().unwrap();
        let EntityType::Insert(reference)=changed.get_entity(insert).unwrap() else {panic!()};
        let members=&changed.block_records.get(&reference.block_name).unwrap().entity_handles;
        let EntityType::Line(rotated)=changed.get_entity(members[1]).unwrap() else {panic!()};
        assert!((rotated.start-Vector3::new(-1.,1.,0.)).length()<1e-10);
    }
    options::set(&mut doc,definition,action,2.,std::f64::consts::FRAC_PI_2).unwrap();
    let baseline=doc.clone();
    evaluate(&mut doc,&baseline,definition,parameter,DVec3::new(0.,15.,0.)).unwrap();
    let EntityType::Line(stretched)=doc.get_entity(targets[0]).unwrap() else {panic!()};
    assert!((stretched.end-Vector3::new(-10.,10.,0.)).length()<1e-10);
    let EntityType::Line(rotated)=doc.get_entity(targets[1]).unwrap() else {panic!()};
    assert!((rotated.start-Vector3::new(-1.,1.,0.)).length()<1e-10);
    let mut reset=baseline.clone();
    assert!(options::set_rotate_only(&mut reset,definition,action,&[]).unwrap());
    let ObjectType::DynamicBlock(object)=&reset.objects[&action] else {panic!()};
    let DynamicBlockData::PolarStretchAction(native)=&object.data else {panic!()};
    assert!(native.handles.is_empty());
    assert_eq!(native.action.dependencies,targets[..3]);
}

#[test]
fn block_actions_independent_bases_evaluate_save_reset_and_isolate_references() {
    use crate::scene::block_orientation_parameter::Kind as O;
    use crate::scene::block_authoring::edit::{self, Part};
    for kind in [Kind::Rotate, Kind::Scale] {
        let (mut doc, def, targets) = fixture();
        let parameter = if kind == Kind::Rotate { orientation(&mut doc, def, O::Rotation) } else { linear(&mut doc, def) };
        let action = create(&mut doc, def, kind, parameter, &targets[..1], DVec3::new(7., 8., 0.)).unwrap();
        assert!(options::set_base(&mut doc, def, action, false, Some(DVec3::new(3., 2., 0.))).unwrap());
        let item = edit::items(&doc, def).into_iter().find(|item| item.handle == action).unwrap();
        assert!(item.grips.contains(&(Part::Base, Vector3::new(3., 2., 0.))));
        // The pivot grip and the action display marker edit distinct native fields.
        edit::move_part(&mut doc, def, action, Part::Base, Vector3::new(4., 2., 0.)).unwrap();
        assert_eq!(options::base(&doc, action).unwrap().action.display_location, Vector3::new(7., 8., 0.));
        edit::move_part(&mut doc, def, action, Part::Base, Vector3::new(3., 2., 0.)).unwrap();
        let before = doc.clone();
        for point in [DVec3::new(f64::INFINITY, 0., 0.), DVec3::new(3., 2., 1.)] {
            assert!(options::set_base(&mut doc, def, action, false, Some(point)).is_err());
            assert_eq!(doc, before);
        }
        assert!(options::set_base(&mut doc, Handle::NULL, action, false, None).is_err());
        let a = doc.add_entity(EntityType::Insert(codec::entities::Insert::new("ACTIONS", Vector3::ZERO))).unwrap();
        let b = doc.add_entity(EntityType::Insert(codec::entities::Insert::new("ACTIONS", Vector3::new(30., 0., 0.)))).unwrap();
        let end = if kind == Kind::Rotate { DVec3::new(0., 10., 0.) } else { DVec3::new(20., 0., 0.) };
        let expected = if kind == Kind::Rotate { Vector3::new(5., 0., 0.) } else { Vector3::new(-1., -2., 0.) };
        doc = instance::evaluate(&doc, a, parameter, end).unwrap().unwrap();
        assert!(instance_line(&doc, a).distance(&expected) < 1e-8);
        for saved in [
            doc.clone(),
            DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&doc).unwrap())).read().unwrap(),
            DxfReader::from_reader(Cursor::new(DxfWriter::new(&doc).write_to_vec().unwrap())).unwrap().read().unwrap(),
        ] {
            assert_eq!(options::base(&saved, action), options::base(&doc, action));
            assert!(instance_line(&saved, a).distance(&expected) < 1e-8);
            assert_eq!(instance_line(&saved, b), Vector3::UNIT_X);
            assert_eq!(start(&saved, targets[0]), Vector3::UNIT_X);
            let reset = instance::evaluate(&saved, a, parameter, DVec3::new(10., 0., 0.)).unwrap().unwrap();
            assert!(instance_line(&reset, a).distance(&Vector3::UNIT_X) < 1e-8);
            let again = instance::evaluate(&reset, a, parameter, end).unwrap().unwrap();
            assert!(instance_line(&again, a).distance(&expected) < 1e-8);
            let mut dependent = saved.clone();
            options::set_base(&mut dependent, def, action, true, None).unwrap();
            let refreshed = instance::refresh(&dependent, def).unwrap();
            let expected_dependent = if kind == Kind::Rotate { Vector3::UNIT_Y } else { Vector3::new(2., 0., 0.) };
            assert!(instance_line(&refreshed, a).distance(&expected_dependent) < 1e-8);
        }
    }
}

#[test]
fn block_parameter_values_are_independent_of_display_grip_locations() {
    use crate::scene::block_orientation_parameter::Kind as O;
    for family in 0..5 {
        let (mut doc, def, targets) = fixture();
        let (parameter, kind, input) = match family {
            0 => (point::create(&mut doc, def, Vector3::new(4., 3., 0.), Vector3::new(5., 4., 0.)).unwrap(), Kind::Move, DVec3::new(8., 5., 0.)),
            1 => (linear(&mut doc, def), Kind::Move, DVec3::new(15., 0., 0.)),
            2 => (distance::create(&mut doc, def, distance::Kind::Polar, Vector3::ZERO, Vector3::new(10., 0., 0.), Vector3::new(5., 2., 0.)).unwrap(), Kind::Move, DVec3::new(0., 15., 0.)),
            3 => (distance::create(&mut doc, def, distance::Kind::XY, Vector3::ZERO, Vector3::new(10., 10., 0.), Vector3::new(5., 5., 0.)).unwrap(), Kind::Move, DVec3::new(15., 20., 0.)),
            _ => (orientation(&mut doc, def, O::Rotation), Kind::Rotate, DVec3::new(0., 10., 0.)),
        };
        create(&mut doc, def, kind, parameter, &targets[..1], DVec3::ZERO).unwrap();
        let p = parameters(&doc, def, kind).remove(0);
        let ObjectType::DynamicBlock(object) = doc.objects.get_mut(&p.grip).unwrap() else { panic!() };
        match &mut object.data {
            DynamicBlockData::LinearGrip(g) => g.grip.location = Vector3::new(35., 42., 0.),
            DynamicBlockData::XYGrip(g) | DynamicBlockData::PolarGrip(g) | DynamicBlockData::RotationGrip(g) => g.location = Vector3::new(35., 42., 0.),
            _ => panic!(),
        }
        assert_eq!(current(&doc, &p), p.end);
        let baseline = doc.clone();
        evaluate(&mut doc, &baseline, def, parameter, input).unwrap();
        assert_eq!(doc.objects[&p.grip], baseline.objects[&p.grip]);
        assert!(current(&doc, &p).distance(input) < 1e-8);
        for saved in [
            DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&doc).unwrap())).read().unwrap(),
            DxfReader::from_reader(Cursor::new(DxfWriter::new(&doc).write_to_vec().unwrap())).unwrap().read().unwrap(),
        ] {
            assert!(current(&saved, &p).distance(input) < 1e-8);
            assert_eq!(saved.objects[&p.grip], baseline.objects[&p.grip]);
        }
        let translation = Vector3::new(13., -4., 7.);
        let transform = Transform::from_translation(translation);
        let mut reframed = doc.clone();
        point::reframe(&mut reframed, def, &transform);
        distance::reframe(&mut reframed, def, &transform);
        crate::scene::block_orientation_parameter::reframe(&mut reframed, def, &transform);
        let moved = parameters(&reframed, def, kind).remove(0);
        assert!(current(&reframed, &moved).distance(input + v(translation)) < 1e-8);
        evaluate(&mut doc, &baseline, def, parameter, p.end).unwrap();
        assert!(start(&doc, targets[0]).distance(&Vector3::UNIT_X) < 1e-8);
    }
}

#[test]
fn block_parameters_without_grips_remain_evaluable_and_roundtrip() {
    use crate::scene::block_orientation_parameter::Kind as O;
    for flip in [false, true] {
        let (mut doc, def, targets) = fixture();
        let (parameter, kind) = if flip { (orientation(&mut doc, def, O::Flip), Kind::Flip) } else { (linear(&mut doc, def), Kind::Move) };
        let p = parameters(&doc, def, kind).remove(0);
        let graph_handle = authoring::graph(&doc, def).unwrap().unwrap();
        doc.objects.remove(&p.grip);
        let ObjectType::DynamicBlock(native) = doc.objects.get_mut(&parameter).unwrap() else { panic!() };
        match &mut native.data {
            DynamicBlockData::LinearParameter(p) => {
                p.parameter.property_states = [0; 4];
                p.parameter.properties.iter_mut().for_each(|property| property.connections.clear());
            }
            DynamicBlockData::FlipParameter(p) => {
                p.flags_96 = 0;
                p.parameter.property_states = [0; 4];
            }
            _ => panic!(),
        }
        if flip {
            let EntityType::Line(line) = doc.get_entity_mut(targets[0]).unwrap() else { panic!() };
            line.start = Vector3::UNIT_Y;
            line.end = Vector3::new(1., 1., 0.);
        }
        let ObjectType::DynamicBlock(object) = doc.objects.get_mut(&graph_handle).unwrap() else { panic!() };
        let DynamicBlockData::EvaluationGraph(graph) = &mut object.data else { panic!() };
        let node = graph.nodes.iter().find(|node| node.expression == p.grip).unwrap().id;
        let removed = graph.edges.iter().filter(|edge| edge.source_node == node || edge.destination_node == node).map(|edge| edge.id).collect::<Vec<_>>();
        graph.nodes.retain(|node| node.expression != p.grip);
        graph.edges.retain(|edge| !removed.contains(&edge.id));
        for node in &mut graph.nodes { for edge in &mut node.node_data { if removed.contains(edge) { *edge = -1; } } }
        for edge in &mut graph.edges { for link in &mut edge.outgoing_edges { if removed.contains(link) { *link = -1; } } }
        create(&mut doc, def, kind, parameter, &targets[..1], DVec3::ZERO).unwrap();
        assert_eq!(all_parameters(&doc, def).len(), 1);
        assert_eq!(all_parameters(&doc, def)[0].grip, Handle::NULL);
        let insert = doc.add_entity(EntityType::Insert(codec::entities::Insert::new("ACTIONS", Vector3::ZERO))).unwrap();
        let input = if flip { Value::Flip(true) } else { Value::Point(DVec3::new(15., 0., 0.)) };
        let before = doc.objects[&parameter].clone();
        let changed = instance::evaluate_value(&doc, insert, parameter, input).unwrap().unwrap();
        assert_eq!(changed.objects[&parameter], before);
        let expected = if flip { -Vector3::UNIT_Y } else { Vector3::new(6., 0., 0.) };
        for saved in [
            changed.clone(),
            DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&changed).unwrap())).read().unwrap(),
            DxfReader::from_reader(Cursor::new(DxfWriter::new(&changed).write_to_vec().unwrap())).unwrap().read().unwrap(),
        ] {
            assert!(instance_line(&saved, insert).distance(&expected) < 1e-8);
            assert_eq!(instance::value(&saved, insert, &all_parameters(&saved, def)[0]).unwrap(), input);
            assert_eq!(saved.objects[&parameter], before);
        }
    }
}
