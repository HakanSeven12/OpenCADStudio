use super::*;
use crate::scene::block_distance_parameter as distance;
use crate::scene::block_point_parameter as point;
use codec::{entities::Insert, DwgReader, DwgWriter, DxfReader, DxfWriter};
use std::io::Cursor;

fn set_targets(doc: &mut Document, action: Handle, targets: Vec<Handle>) {
    let ObjectType::DynamicBlock(object) = doc.objects.get_mut(&action).unwrap() else {
        panic!()
    };
    targets::action_mut(&mut object.data).unwrap().dependencies = targets;
}

fn fixture(
    chained: bool,
) -> (
    Document,
    Handle,
    Vec<Parameter>,
    Vec<Handle>,
    Handle,
    Handle,
) {
    let (mut doc, definition, targets) = super::super::tests::fixture();
    let mut handles = Vec::new();
    for y in [0., 10., 20.] {
        let h = point::create(
            &mut doc,
            definition,
            Vector3::new(0., y, 0.),
            Vector3::new(2., y, 0.),
        )
        .unwrap();
        let ObjectType::DynamicBlock(object) = doc.objects.get_mut(&h).unwrap() else {
            panic!()
        };
        let DynamicBlockData::PointParameter(p) = &mut object.data else {
            panic!()
        };
        p.parameter.parameter.chain_actions = chained;
        handles.push(h);
    }
    let mut actions = Vec::new();
    for h in &handles {
        actions.push(
            create(
                &mut doc,
                definition,
                Kind::Move,
                *h,
                &targets[..1],
                DVec3::ZERO,
            )
            .unwrap(),
        );
    }
    let params = all_parameters(&doc, definition);
    set_targets(&mut doc, actions[0], vec![params[1].handle, params[1].grip]);
    set_targets(&mut doc, actions[1], vec![params[2].handle]);
    let first = doc
        .add_entity(EntityType::Insert(Insert::new("ACTIONS", Vector3::ZERO)))
        .unwrap();
    let second = doc
        .add_entity(EntityType::Insert(Insert::new(
            "ACTIONS",
            Vector3::UNIT_X * 50.,
        )))
        .unwrap();
    (doc, definition, params, actions, first, second)
}

fn line(doc: &Document, insert: Handle) -> DVec3 {
    let EntityType::Insert(reference) = doc.get_entity(insert).unwrap() else {
        panic!()
    };
    doc.block_records
        .get(&reference.block_name)
        .unwrap()
        .entity_handles
        .iter()
        .find_map(|h| match doc.get_entity(*h) {
            Some(EntityType::Line(line)) => Some(v(line.start)),
            _ => None,
        })
        .unwrap()
}

#[test]
fn block_move_point_chains_recover_autonomous_values_without_repeated_edit_drift() {
    for chained in [false, true] {
        let (doc, definition, params, _, first, second) = fixture(chained);
        let mut changed = doc.clone();
        for offset in [
            DVec3::new(3., 4., 0.),
            DVec3::new(8., 2., 0.),
            DVec3::ZERO,
            DVec3::new(-2., -3., 0.),
        ] {
            changed = instance::evaluate_value(
                &changed,
                first,
                params[0].handle,
                Value::Point(params[0].end + offset),
            )
            .unwrap()
            .unwrap();
            assert_eq!(
                instance::frame(&changed, first, &params[1]).unwrap().end,
                params[1].end + offset
            );
            let propagated = if chained { offset } else { DVec3::ZERO };
            assert_eq!(
                instance::frame(&changed, first, &params[2]).unwrap().end,
                params[2].end + propagated
            );
            assert_eq!(line(&changed, first), DVec3::X + propagated);
            assert_eq!(line(&changed, second), DVec3::X);
            for p in &params {
                assert_eq!(changed.objects[&p.handle], doc.objects[&p.handle]);
            }
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
            let next = instance::evaluate_value(
                &saved,
                first,
                params[0].handle,
                Value::Point(params[0].end + DVec3::X * 5.),
            )
            .unwrap()
            .unwrap();
            assert_eq!(
                line(&next, first),
                DVec3::X + if chained { DVec3::X * 5. } else { DVec3::ZERO }
            );
            let reset = instance::reset(&next, first).unwrap().unwrap();
            assert_eq!(line(&reset, first), DVec3::X);
        }
        assert_eq!(authoring::members(&doc, definition).len(), 2);
    }
}

#[test]
fn block_move_point_chains_preserve_direct_child_edits_and_test_reference_caches() {
    let (doc, definition, params, _, first, _) = fixture(true);
    for testing in [false, true] {
        let mut changed = doc.clone();
        // Drawing and scratch evaluation both replay from the immutable baseline.
        for (p, offset) in [
            (0, DVec3::new(3., 4., 0.)),
            (1, DVec3::new(6., 8., 0.)),
            (0, DVec3::new(8., 2., 0.)),
        ] {
            changed = if testing {
                instance::evaluate_test_value(
                    &changed,
                    &doc,
                    definition,
                    first,
                    params[p].handle,
                    Value::Point(params[p].end + offset),
                )
            } else {
                instance::evaluate_value(
                    &changed,
                    first,
                    params[p].handle,
                    Value::Point(params[p].end + offset),
                )
            }
            .unwrap()
            .unwrap();
        }
        assert_eq!(
            instance::frame(&changed, first, &params[1]).unwrap().end,
            params[1].end + DVec3::new(11., 6., 0.)
        );
        assert_eq!(line(&changed, first), DVec3::new(12., 6., 0.));
    }
}

#[test]
fn block_move_point_chain_cycles_reject_before_mutating_the_document() {
    for self_cycle in [false, true] {
        let (mut doc, definition, params, actions, first, _) = fixture(true);
        set_targets(
            &mut doc,
            if self_cycle { actions[0] } else { actions[2] },
            vec![params[0].handle],
        );
        let baseline = doc.clone();
        assert_eq!(Graph::new(&doc, definition).err(), Some(CYCLE_ERROR));
        assert_eq!(
            evaluate_value(
                &mut doc,
                &baseline,
                definition,
                params[0].handle,
                Value::Point(DVec3::X)
            )
            .unwrap_err(),
            CYCLE_ERROR
        );
        assert_eq!(doc, baseline);
        assert_eq!(
            instance::evaluate_value(&doc, first, params[0].handle, Value::Point(DVec3::X))
                .unwrap_err(),
            CYCLE_ERROR
        );
        assert_eq!(doc, baseline);
    }
}

#[test]
fn block_move_point_chains_rebuild_bulk_inputs_from_the_previous_effective_frame() {
    let (doc, definition, params, _, first, _) = fixture(true);
    let changed = instance::evaluate_value(
        &doc,
        first,
        params[0].handle,
        Value::Point(DVec3::new(3., 4., 0.)),
    )
    .unwrap()
    .unwrap();
    for explicit_child in [false, true] {
        let mut evaluated = changed.clone();
        let frames = instance::load_frames(&changed, &mut evaluated, first, &params).unwrap();
        let old_values = params
            .iter()
            .map(|p| (p.handle, current(&evaluated, p)))
            .collect();
        set_value(
            &mut evaluated,
            &params[0],
            Value::Point(DVec3::new(8., 2., 0.)),
        )
        .unwrap();
        let mut requested = BTreeSet::from([params[0].handle]);
        if explicit_child {
            set_value(
                &mut evaluated,
                &params[1],
                Value::Point(params[1].end + DVec3::new(6., 8., 0.)),
            )
            .unwrap();
            requested.insert(params[1].handle);
        }
        rebuild_framed(
            &mut evaluated,
            &changed,
            definition,
            &frames,
            Some(RebuildInputs {
                document: &changed,
                frames: &frames,
                values: &old_values,
                requested: &requested,
            }),
        )
        .unwrap();
        let offset = if explicit_child {
            DVec3::new(6., 8., 0.)
        } else {
            DVec3::new(8., 2., 0.)
        };
        assert_eq!(current(&evaluated, &params[1]), params[1].end + offset);
        assert_eq!(current(&evaluated, &params[2]), params[2].end + offset);
    }
}

#[test]
fn block_move_point_chains_replay_definition_edits_using_the_previous_graph() {
    let (doc, definition, params, actions, first, second) = fixture(true);
    let changed = instance::evaluate_value(
        &doc,
        first,
        params[0].handle,
        Value::Point(DVec3::new(3., 4., 0.)),
    )
    .unwrap()
    .unwrap();
    for edit in 0..3 {
        let mut edited = changed.clone();
        if edit == 0 {
            let ObjectType::DynamicBlock(object) = edited.objects.get_mut(&actions[0]).unwrap()
            else {
                panic!()
            };
            let DynamicBlockData::MoveAction(action) = &mut object.data else {
                panic!()
            };
            action.offsets.distance_multiplier = 2.;
        } else if edit == 1 {
            let ObjectType::DynamicBlock(object) =
                edited.objects.get_mut(&params[1].handle).unwrap()
            else {
                panic!()
            };
            let DynamicBlockData::PointParameter(parameter) = &mut object.data else {
                panic!()
            };
            parameter.parameter.parameter.chain_actions = false;
        } else {
            set_targets(&mut edited, actions[0], vec![]);
        }
        let refreshed = instance::refresh_after_edit(&edited, &changed, definition).unwrap();
        let offset = if edit == 0 {
            DVec3::new(6., 8., 0.)
        } else {
            DVec3::ZERO
        };
        assert_eq!(line(&refreshed, first), DVec3::X + offset);
        assert_eq!(line(&refreshed, second), DVec3::X);
        let expected_point = params[1].end
            + if edit == 0 {
                offset
            } else if edit == 1 {
                DVec3::new(3., 4., 0.)
            } else {
                DVec3::ZERO
            };
        assert_eq!(
            instance::frame(&refreshed, first, &params[1]).unwrap().end,
            expected_point
        );
        assert_eq!(
            instance::frame(&refreshed, first, &params[2]).unwrap().end,
            params[2].end + offset
        );
    }
}

#[test]
fn block_move_point_chains_recover_previous_base_driven_reference_frames() {
    use crate::scene::block_distance_parameter as distance;
    for base_driven in [false, true] {
        let (mut doc, definition, targets) = super::super::tests::fixture();
        let root = distance::create(
            &mut doc,
            definition,
            distance::Kind::Linear,
            Vector3::ZERO,
            Vector3::UNIT_X * 10.,
            Vector3::UNIT_Y,
        )
        .unwrap();
        let child = point::create(
            &mut doc,
            definition,
            Vector3::UNIT_Y * 10.,
            Vector3::UNIT_Y * 12.,
        )
        .unwrap();
        let ObjectType::DynamicBlock(object) = doc.objects.get_mut(&child).unwrap() else {
            panic!()
        };
        let DynamicBlockData::PointParameter(p) = &mut object.data else {
            panic!()
        };
        p.parameter.parameter.chain_actions = true;
        let action = create(
            &mut doc,
            definition,
            Kind::Move,
            root,
            &targets[..1],
            DVec3::ZERO,
        )
        .unwrap();
        set_targets(&mut doc, action, vec![child]);
        if base_driven {
            let ObjectType::DynamicBlock(object) = doc.objects.get_mut(&action).unwrap() else {
                panic!()
            };
            let DynamicBlockData::MoveAction(action) = &mut object.data else {
                panic!()
            };
            action.connections[0].name = "BaseXDelta".into();
            action.connections[1].name = "BaseYDelta".into();
        }
        create(
            &mut doc,
            definition,
            Kind::Move,
            child,
            &targets[..1],
            DVec3::ZERO,
        )
        .unwrap();
        let insert = doc
            .add_entity(EntityType::Insert(Insert::new("ACTIONS", Vector3::ZERO)))
            .unwrap();
        let child_parameter = all_parameters(&doc, definition)
            .into_iter()
            .find(|p| p.handle == child)
            .unwrap();
        for offset in [5., 8., -2., 0.] {
            doc = instance::evaluate_value(
                &doc,
                insert,
                root,
                Value::Frame {
                    base: DVec3::X * offset,
                    end: DVec3::X * (10. + offset),
                },
            )
            .unwrap()
            .unwrap();
            assert_eq!(
                instance::frame(&doc, insert, &child_parameter).unwrap().end,
                child_parameter.end + DVec3::X * offset
            );
            assert_eq!(line(&doc, insert), DVec3::X * (1. + offset));
        }

        if !base_driven {
            use crate::scene::block_lookup as lookup;
            let lookup_parameter = lookup::create(&mut doc, definition, Vector3::ZERO).unwrap();
            let native = lookup::parameters(&doc, definition)
                .into_iter()
                .find(|(h, _)| *h == lookup_parameter)
                .unwrap()
                .1;
            let mut table = lookup::empty_table(native);
            table
                .columns
                .insert(0, lookup::properties(&doc, definition)[0].column.clone());
            table.column_count = 2;
            table.row_count = 2;
            table.expressions = ["10", "Small", "20", "Large"].map(str::to_owned).to_vec();
            lookup::apply_table(&mut doc, definition, lookup_parameter, table).unwrap();
            let baseline = doc.clone();
            for testing in [false, true] {
                let mut selected = doc.clone();
                for (label, delta) in [("Large", 10.), ("Small", 0.), ("Large", 10.)] {
                    selected = if testing {
                        lookup::evaluate_test_choice(&selected, &baseline, insert, label)
                    } else {
                        lookup::evaluate_reference_value(&selected, insert, lookup_parameter, label)
                    }
                    .unwrap()
                    .unwrap();
                    assert_eq!(
                        instance::frame(&selected, insert, &child_parameter)
                            .unwrap()
                            .end,
                        child_parameter.end + DVec3::X * delta
                    );
                    assert_eq!(line(&selected, insert), DVec3::X * (1. + delta));
                }
            }
        }
    }
}

#[test]
fn block_move_point_chains_sum_independent_parents_before_replaying_children() {
    let (mut doc, definition, params, _, first, _) = fixture(true);
    let second_root = point::create(
        &mut doc,
        definition,
        Vector3::UNIT_Y * 30.,
        Vector3::UNIT_Y * 32.,
    )
    .unwrap();
    let target = authoring::members(&doc, definition)[0];
    let action = create(
        &mut doc,
        definition,
        Kind::Move,
        second_root,
        &[target],
        DVec3::ZERO,
    )
    .unwrap();
    set_targets(&mut doc, action, vec![params[1].handle]);
    let root = all_parameters(&doc, definition)
        .into_iter()
        .find(|p| p.handle == second_root)
        .unwrap();
    for (handle, end, offset) in [
        (
            params[0].handle,
            params[0].end + DVec3::new(3., 4., 0.),
            DVec3::new(3., 4., 0.),
        ),
        (
            root.handle,
            root.end + DVec3::new(1., 2., 0.),
            DVec3::new(4., 6., 0.),
        ),
        (params[0].handle, params[0].end, DVec3::new(1., 2., 0.)),
    ] {
        doc = instance::evaluate_value(&doc, first, handle, Value::Point(end))
            .unwrap()
            .unwrap();
        assert_eq!(
            instance::frame(&doc, first, &params[1]).unwrap().end,
            params[1].end + offset
        );
        assert_eq!(
            instance::frame(&doc, first, &params[2]).unwrap().end,
            params[2].end + offset
        );
        assert_eq!(line(&doc, first), DVec3::X + offset);
    }
}

#[test]
fn block_move_point_chains_preserve_independent_fractional_inputs_across_repeated_edits() {
    let (mut doc, _, params, _, first, _) = fixture(true);
    let residual = DVec3::new(0.35, 0.85, 0.);
    doc = instance::evaluate_value(
        &doc,
        first,
        params[1].handle,
        Value::Point(params[1].end + residual),
    )
    .unwrap()
    .unwrap();
    for i in 0..32 {
        let angle = i as f64 * 0.3;
        let delta = DVec3::new(angle.sin() * 1000.1, angle.cos() * 300.7, 0.);
        doc = instance::evaluate_value(
            &doc,
            first,
            params[0].handle,
            Value::Point(params[0].end + delta),
        )
        .unwrap()
        .unwrap();
        let child = instance::frame(&doc, first, &params[1]).unwrap();
        assert!(child.end.distance(params[1].end + residual + delta) < 1e-8);
        assert!(line(&doc, first).distance(DVec3::X + residual + delta) < 1e-8);
    }
    doc = instance::evaluate_value(&doc, first, params[0].handle, Value::Point(params[0].end))
        .unwrap()
        .unwrap();
    assert!(line(&doc, first).distance(DVec3::X + residual) < 1e-8);
}

#[test]
fn block_move_point_chains_reject_ambiguous_grip_aliases_before_publication() {
    let (mut doc, definition, params, _, first, _) = fixture(true);
    let ObjectType::DynamicBlock(object) = &doc.objects[&params[1].handle] else {
        panic!()
    };
    let DynamicBlockData::PointParameter(source) = &object.data else {
        panic!()
    };
    let grip_node = source.parameter.property_count;
    let ObjectType::DynamicBlock(object) = doc.objects.get_mut(&params[2].handle).unwrap() else {
        panic!()
    };
    let DynamicBlockData::PointParameter(target) = &mut object.data else {
        panic!()
    };
    target.parameter.property_count = grip_node;
    assert!(Graph::new(&doc, definition).is_err());
    let baseline = doc.clone();
    assert!(
        instance::evaluate_value(&doc, first, params[0].handle, Value::Point(DVec3::X)).is_err()
    );
    assert_eq!(doc, baseline);
    assert!(evaluate_value(
        &mut doc,
        &baseline,
        definition,
        params[0].handle,
        Value::Point(DVec3::X)
    )
    .is_err());
    assert_eq!(doc, baseline);
}

#[test]
fn block_named_order_composes_with_point_transport_on_shared_geometry() {
    for scale_first in [false, true] {
        let (mut doc, definition, members) = super::super::tests::fixture();
        let parent = distance::create(
            &mut doc,
            definition,
            distance::Kind::Linear,
            Vector3::ZERO,
            Vector3::new(10., 0., 0.),
            Vector3::new(5., 2., 0.),
        )
        .unwrap();
        let child = point::create(&mut doc, definition, Vector3::ZERO, Vector3::ZERO).unwrap();
        let ObjectType::DynamicBlock(object) = doc.objects.get_mut(&child).unwrap() else {
            panic!()
        };
        let DynamicBlockData::PointParameter(p) = &mut object.data else {
            panic!()
        };
        p.parameter.parameter.chain_actions = true;
        let ObjectType::DynamicBlock(object) = doc.objects.get_mut(&parent).unwrap() else {
            panic!()
        };
        let DynamicBlockData::LinearParameter(p) = &mut object.data else {
            panic!()
        };
        p.parameter.parameter.chain_actions = true;
        let move_action = create(
            &mut doc,
            definition,
            Kind::Move,
            parent,
            &[members[1]],
            DVec3::ZERO,
        )
        .unwrap();
        let scale_action = create(
            &mut doc,
            definition,
            Kind::Scale,
            parent,
            &[members[1]],
            DVec3::ZERO,
        )
        .unwrap();
        create(
            &mut doc,
            definition,
            Kind::Move,
            child,
            &[members[1]],
            DVec3::ZERO,
        )
        .unwrap();
        set_targets(&mut doc, move_action, vec![members[1], child]);
        let second_parent =
            point::create(&mut doc, definition, Vector3::ZERO, Vector3::UNIT_Y).unwrap();
        let second_move = create(
            &mut doc,
            definition,
            Kind::Move,
            second_parent,
            &[members[1]],
            DVec3::ZERO,
        )
        .unwrap();
        set_targets(&mut doc, second_move, vec![child]);
        let (head, tail) = if scale_first {
            (scale_action, move_action)
        } else {
            (move_action, scale_action)
        };
        assert!(super::super::chain::set(&mut doc, definition, head, Some(tail)).unwrap());
        let first = doc
            .add_entity(EntityType::Insert(Insert::new("ACTIONS", Vector3::ZERO)))
            .unwrap();
        let second = doc
            .add_entity(EntityType::Insert(Insert::new(
                "ACTIONS",
                Vector3::UNIT_X * 50.,
            )))
            .unwrap();
        let parameters = all_parameters(&doc, definition);
        let child_parameter = parameters
            .iter()
            .find(|p| p.handle == child)
            .unwrap()
            .clone();
        let changed =
            instance::evaluate_value(&doc, first, parent, Value::Point(DVec3::new(20., 0., 0.)))
                .unwrap()
                .unwrap();
        assert_eq!(
            instance::frame(&changed, first, &child_parameter)
                .unwrap()
                .end,
            DVec3::X * 10.
        );
        assert_eq!(
            instance::frame(&changed, second, &child_parameter)
                .unwrap()
                .end,
            DVec3::ZERO
        );
        let circle = |drawing: &Document, reference: Handle| {
            let EntityType::Insert(insert) = drawing.get_entity(reference).unwrap() else {
                panic!()
            };
            drawing
                .block_records
                .get(&insert.block_name)
                .unwrap()
                .entity_handles
                .iter()
                .find_map(|h| match drawing.get_entity(*h) {
                    Some(EntityType::Circle(circle)) => Some((v(circle.center), circle.radius)),
                    _ => None,
                })
                .unwrap()
        };
        let expected = (DVec3::new(if scale_first { 28. } else { 38. }, 6., 0.), 4.);
        assert_eq!(circle(&changed, first), expected);
        assert_eq!(circle(&changed, second), (DVec3::new(4., 3., 0.), 2.));
        // The second parent contributes independently; a direct child edit adds
        // its own input, then changing the first parent must preserve both.
        let combined =
            instance::evaluate_value(&changed, first, second_parent, Value::Point(DVec3::X * 3.))
                .unwrap()
                .unwrap();
        assert_eq!(
            instance::frame(&combined, first, &child_parameter)
                .unwrap()
                .end,
            DVec3::X * 13.
        );
        assert_eq!(
            circle(&combined, first),
            (expected.0 + DVec3::X * 3., expected.1)
        );
        let combined =
            instance::evaluate_value(&combined, first, child, Value::Point(DVec3::X * 15.))
                .unwrap()
                .unwrap();
        assert_eq!(
            circle(&combined, first),
            (expected.0 + DVec3::X * 5., expected.1)
        );
        let combined =
            instance::evaluate_value(&combined, first, parent, Value::Point(DVec3::X * 10.))
                .unwrap()
                .unwrap();
        assert_eq!(
            instance::frame(&combined, first, &child_parameter)
                .unwrap()
                .end,
            DVec3::X * 5.
        );
        assert_eq!(circle(&combined, first), (DVec3::new(9., 3., 0.), 2.));
        assert_eq!(circle(&combined, second), (DVec3::new(4., 3., 0.), 2.));
        for saved in [
            DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&combined).unwrap()))
                .read()
                .unwrap(),
            DxfReader::from_reader(Cursor::new(
                DxfWriter::new(&combined).write_to_vec().unwrap(),
            ))
            .unwrap()
            .read()
            .unwrap(),
        ] {
            assert_eq!(circle(&saved, first), (DVec3::new(9., 3., 0.), 2.));
            assert_eq!(
                instance::frame(&saved, first, &child_parameter)
                    .unwrap()
                    .end,
                DVec3::X * 5.
            );
            let resumed =
                instance::evaluate_value(&saved, first, second_parent, Value::Point(DVec3::X * 4.))
                    .unwrap()
                    .unwrap();
            assert_eq!(
                instance::frame(&resumed, first, &child_parameter)
                    .unwrap()
                    .end,
                DVec3::X * 6.
            );
            assert_eq!(circle(&resumed, first), (DVec3::new(10., 3., 0.), 2.));
        }
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
            assert_eq!(circle(&saved, first), expected);
            assert_eq!(
                instance::frame(&saved, first, &child_parameter)
                    .unwrap()
                    .end,
                DVec3::X * 10.
            );
            let reset = instance::reset(&saved, first).unwrap().unwrap();
            assert_eq!(circle(&reset, first), (DVec3::new(4., 3., 0.), 2.));
        }
    }
}

#[test]
fn block_move_selection_retargets_point_aliases_and_refreshes_previous_graph() {
    let (doc, definition, params, actions, first, second) = fixture(true);
    let mut unchanged = doc.clone();
    assert!(!super::super::retarget(
        &mut unchanged,
        definition,
        actions[0],
        &[params[1].handle],
        None
    )
    .unwrap());
    assert_eq!(
        unchanged, doc,
        "the parameter and grip aliases retain their native links"
    );
    let changed = instance::evaluate_value(
        &doc,
        first,
        params[0].handle,
        Value::Point(params[0].end + DVec3::new(3., 4., 0.)),
    )
    .unwrap()
    .unwrap();
    let geometry = authoring::members(&changed, definition)[0];
    let mut detached = changed.clone();
    assert!(
        super::super::retarget(&mut detached, definition, actions[0], &[geometry], None).unwrap()
    );
    let detached = instance::refresh_after_edit(&detached, &changed, definition).unwrap();
    assert_eq!(
        instance::frame(&detached, first, &params[1]).unwrap().end,
        params[1].end
    );
    assert_eq!(line(&detached, first), DVec3::X + DVec3::new(3., 4., 0.));
    assert_eq!(line(&detached, second), DVec3::X);
    let mut attached = detached.clone();
    assert!(super::super::retarget(
        &mut attached,
        definition,
        actions[0],
        &[geometry, params[1].grip],
        None
    )
    .unwrap());
    let attached = instance::refresh_after_edit(&attached, &detached, definition).unwrap();
    assert_eq!(
        instance::frame(&attached, first, &params[1]).unwrap().end,
        params[1].end + DVec3::new(3., 4., 0.)
    );
    assert_eq!(line(&attached, first), DVec3::X + DVec3::new(6., 8., 0.));
    assert_eq!(line(&attached, second), DVec3::X);
    let mut cycle = attached.clone();
    assert!(super::super::retarget(
        &mut cycle,
        definition,
        actions[1],
        &[params[0].handle],
        None
    )
    .is_err());
    assert_eq!(
        cycle, attached,
        "cycle rejection must not publish a partial edit"
    );
}

#[test]
fn block_move_creation_validates_point_ownership_and_cycles_atomically() {
    let (mut doc, definition, params, _, _, foreign_entity) = fixture(true);
    let before = doc.clone();
    assert!(create(
        &mut doc,
        definition,
        Kind::Move,
        params[2].handle,
        &[params[0].handle],
        DVec3::ZERO
    )
    .is_err());
    assert_eq!(doc, before, "cycle must not publish an action");
    assert!(create(
        &mut doc,
        definition,
        Kind::Move,
        params[0].handle,
        &[foreign_entity],
        DVec3::ZERO
    )
    .is_err());
    assert_eq!(doc, before, "foreign entity must not publish an action");
    let mut scene = crate::scene::Scene::new();
    scene.document = doc;
    scene.define_block_raw(
        "FOREIGN",
        Vector3::ZERO,
        vec![EntityType::Line(codec::entities::Line::from_points(
            Vector3::ZERO,
            Vector3::UNIT_X,
        ))],
    );
    let foreign = scene.document.block_records.get("FOREIGN").unwrap().handle;
    let point =
        point::create(&mut scene.document, foreign, Vector3::ZERO, Vector3::UNIT_Y).unwrap();
    let before = scene.document.clone();
    assert!(create(
        &mut scene.document,
        definition,
        Kind::Move,
        params[0].handle,
        &[point],
        DVec3::ZERO
    )
    .is_err());
    assert_eq!(
        scene.document, before,
        "foreign parameter must not publish an action"
    );
}

#[test]
fn block_chain_frames_preserve_current_source_base_and_point_anchor() {
    let (mut doc, definition, _) = super::super::tests::fixture();
    let source = distance::create(
        &mut doc,
        definition,
        distance::Kind::Linear,
        Vector3::ZERO,
        Vector3::new(10., 0., 0.),
        Vector3::UNIT_Y,
    )
    .unwrap();
    let child = point::create(
        &mut doc,
        definition,
        Vector3::new(0., 5., 0.),
        Vector3::new(0., 7., 0.),
    )
    .unwrap();
    create_with_options(
        &mut doc,
        definition,
        Kind::Move,
        source,
        Some(XyKeyPoint::Base),
        &[child],
        DVec3::ZERO,
        None,
        None,
    )
    .unwrap();
    let graph = Graph::new(&doc, definition).unwrap();
    let old_base = DVec3::new(1., 2., 0.);
    let new_base = DVec3::new(3., 4., 0.);
    let child_parameter = graph.parameter(child);
    let values = BTreeMap::from([
        (source, DVec3::new(12., 0., 0.)),
        (child, child_parameter.end + old_base),
    ]);
    let autonomous = graph
        .autonomous(
            &doc,
            Some(&values),
            Some(&BTreeMap::from([(source, old_base)])),
        )
        .unwrap();
    assert_eq!(autonomous[&source].base, old_base);
    assert_eq!(autonomous[&source].end, values[&source]);
    assert_eq!(
        autonomous[&child],
        ParameterFrame {
            base: child_parameter.base,
            end: child_parameter.end
        }
    );
    let mut staged = doc.clone();
    let result = graph
        .resolve(
            &doc,
            &mut staged,
            &BTreeSet::new(),
            &autonomous,
            Some(&values),
            Some(&BTreeMap::from([(source, new_base)])),
        )
        .unwrap();
    assert_eq!(
        result[&child],
        ParameterFrame {
            base: child_parameter.base,
            end: child_parameter.end + new_base
        }
    );
    assert_eq!(current(&staged, child_parameter), child_parameter.end);
}
