use super::*;
use crate::scene::{
    block_basepoint, block_distance_parameter as distance, block_lookup,
    block_orientation_parameter as orientation, block_properties_table as tables,
};

#[test]
fn coupled_flips_preserve_geometry_inputs_and_reload() {
    for testing in [false, true] {
        let (mut doc, def, geometry) = super::tests::fixture();
        let child = distance::create(
            &mut doc,
            def,
            distance::Kind::Linear,
            Vector3::new(0., 2., 0.),
            Vector3::new(10., 2., 0.),
            Vector3::UNIT_Y,
        )
        .unwrap();
        let linked = create(
            &mut doc,
            def,
            Kind::Move,
            child,
            &geometry[1..],
            DVec3::ZERO,
        )
        .unwrap();
        let lookup = block_lookup::create(&mut doc, def, Vector3::new(4., 5., 0.)).unwrap();
        let base = block_basepoint::create(&mut doc, def, Vector3::ZERO).unwrap();
        let mut flips = Vec::new();
        let mut flip_actions = Vec::new();
        for end in [Vector3::new(10., 0., 0.), Vector3::new(0., 10., 0.)] {
            let p = orientation::create(
                &mut doc,
                def,
                orientation::Kind::Flip,
                Vector3::ZERO,
                end,
                Vector3::UNIT_Y,
            )
            .unwrap();
            let action = create(&mut doc, def, Kind::Flip, p, &geometry[1..], DVec3::ZERO).unwrap();
            let ObjectType::DynamicBlock(o) = doc.objects.get_mut(&action).unwrap() else {
                panic!()
            };
            let DynamicBlockData::FlipAction(a) = &mut o.data else {
                panic!()
            };
            a.action.dependencies.extend([child, linked, lookup, base]);
            a.action.dependencies.extend(flip_actions.iter().copied());
            flips.push(p);
            flip_actions.push(action);
        }
        let first = doc
            .add_entity(EntityType::Insert(codec::entities::Insert::new(
                "ACTIONS",
                Vector3::ZERO,
            )))
            .unwrap();
        let second = doc
            .add_entity(EntityType::Insert(codec::entities::Insert::new(
                "ACTIONS",
                Vector3::UNIT_X * 30.,
            )))
            .unwrap();
        let p = all_parameters(&doc, def)
            .into_iter()
            .find(|p| p.handle == child)
            .unwrap();
        let (_, lookup_parameter) = block_lookup::parameters(&doc, def)
            .into_iter()
            .find(|(handle, _)| *handle == lookup)
            .unwrap();
        let mut lookup_table = block_lookup::empty_table(lookup_parameter);
        let lookup_input = block_lookup::properties(&doc, def)
            .into_iter()
            .find(|property| property.column.node_id == p.id)
            .unwrap();
        lookup_table.columns.insert(0, lookup_input.column);
        lookup_table.column_count = 2;
        lookup_table.row_count = 2;
        lookup_table.expressions = ["10", "Short", "17", "Long"].map(String::from).to_vec();
        block_lookup::apply_table(&mut doc, def, lookup, lookup_table).unwrap();
        let columns: Vec<_> = tables::columns(&doc, def)
            .into_iter()
            .filter(|column| column.handle == child)
            .collect();
        tables::apply(&mut doc, def, &columns, &[vec![10.], vec![19.]], false).unwrap();
        for grip in crate::scene::block_grips::config::locations(&doc, def, child).unwrap() {
            crate::scene::block_grips::config::set_location(
                &mut doc,
                def,
                child,
                grip.role,
                grip.point + Vector3::new(2., 3., 0.),
            )
            .unwrap();
        }
        let baseline = doc.clone();
        let eval = |d: &Document, param, input| {
            if testing {
                instance::evaluate_test_value(d, &baseline, def, first, param, input)
            } else {
                instance::evaluate_value(d, first, param, input)
            }
            .unwrap()
            .unwrap()
        };
        let circle = |d: &Document| {
            let EntityType::Insert(i) = d.get_entity(first).unwrap() else {
                panic!()
            };
            d.entities_in_block(&i.block_name)
                .find_map(|e| {
                    if let EntityType::Circle(c) = e {
                        Some(c.center)
                    } else {
                        None
                    }
                })
                .unwrap()
        };
        for reverse in [false, true] {
            let mut changed = baseline.clone();
            for h in if reverse {
                [flips[1], flips[0]]
            } else {
                [flips[0], flips[1]]
            } {
                changed = eval(&changed, h, Value::Flip(true));
            }
            changed = eval(&changed, child, Value::Point(DVec3::new(-15., -2., 0.)));
            assert_eq!(circle(&changed), Vector3::new(-9., -3., 0.));
            let from_lookup = if testing {
                let mut scratch = changed.clone();
                scratch.remove_entity(second);
                block_lookup::evaluate_test_choice(&scratch, &baseline, first, "Long")
            } else {
                block_lookup::evaluate_reference_value(&changed, first, lookup, "Long")
            }
            .unwrap()
            .unwrap();
            assert_eq!(circle(&from_lookup), Vector3::new(-11., -3., 0.));
            assert_eq!(
                instance::frame(&from_lookup, first, &p).unwrap().end,
                DVec3::new(-17., -2., 0.)
            );
            let from_table = if testing {
                instance::evaluate_test_table_row(&from_lookup, &baseline, first, &columns, 1)
            } else {
                instance::evaluate_table_row(&from_lookup, first, &columns, 1)
            }
            .unwrap()
            .unwrap();
            assert_eq!(circle(&from_table), Vector3::new(-13., -3., 0.));
            assert_eq!(
                instance::frame(&from_table, first, &p).unwrap().end,
                DVec3::new(-19., -2., 0.)
            );
            let unflipped = eval(&from_table, flips[0], Value::Flip(false));
            let unflipped = eval(&unflipped, flips[1], Value::Flip(false));
            assert_eq!(circle(&unflipped), Vector3::new(13., 3., 0.));
            if !testing {
                let mut edited = changed.clone();
                let EntityType::Circle(source) = edited.get_entity_mut(geometry[1]).unwrap() else {
                    panic!()
                };
                source.center.y += 4.;
                let refreshed = instance::refresh_after_edit(&edited, &changed, def).unwrap();
                assert_eq!(circle(&refreshed), Vector3::new(-9., -7., 0.));
                assert_eq!(
                    instance::frame(&refreshed, first, &p).unwrap(),
                    instance::frame(&changed, first, &p).unwrap()
                );
                let moved = eval(&refreshed, child, Value::Point(DVec3::new(-18., -2., 0.)));
                assert_eq!(circle(&moved), Vector3::new(-12., -7., 0.));
            }
            let value = instance::value(&changed, first, &p).unwrap();
            let grips = instance::action_grips(&changed, &baseline, def, first, &p, value, testing);
            assert!(!grips.is_empty());
            for grip in grips {
                assert_eq!(grip.offset, DVec3::new(-2., -3., 0.));
            }

            assert_eq!(
                instance::frame(&changed, second, &p).unwrap(),
                instance::ParameterFrame {
                    base: p.base,
                    end: p.end
                }
            );
            for extension in ["dwg", "dxf"] {
                let bytes =
                    crate::io::save_to_bytes(&from_table, extension, from_table.version).unwrap();
                let saved =
                    crate::io::load_bytes(&format!("coupled-flips.{extension}"), bytes).unwrap();
                assert_eq!(circle(&saved), Vector3::new(-13., -3., 0.));
                let next = eval(&saved, child, Value::Point(DVec3::new(-18., -2., 0.)));
                assert_eq!(circle(&next), Vector3::new(-12., -3., 0.));
                let next = eval(&next, flips[0], Value::Flip(false));
                assert_eq!(circle(&next), Vector3::new(-12., 3., 0.));
                let next = eval(&next, flips[1], Value::Flip(false));
                assert_eq!(circle(&next), Vector3::new(12., 3., 0.));
                assert_eq!(
                    instance::frame(&next, first, &p).unwrap().end,
                    DVec3::new(18., 2., 0.)
                );
                let reset = if testing {
                    instance::reset_test(&saved, &baseline, def, first)
                } else {
                    instance::reset(&saved, first)
                }
                .unwrap()
                .unwrap();
                assert_eq!(circle(&reset), Vector3::new(4., 3., 0.));
                assert!(reset
                    .get_entity(first)
                    .unwrap()
                    .common()
                    .extended_data
                    .get_record(super::flip_graph::INPUTS)
                    .is_none());
            }
        }
    }
}
