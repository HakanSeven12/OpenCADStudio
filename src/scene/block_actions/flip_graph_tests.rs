use super::*;
use crate::scene::{
    block_basepoint, block_distance_parameter as distance, block_lookup,
    block_orientation_parameter as orientation, block_properties_table as tables,
};

#[test]
fn authored_flip_links_require_complete_ordered_selections() {
    let (mut doc, def, geometry) = super::tests::fixture();
    let linear = distance::create(
        &mut doc,
        def,
        distance::Kind::Linear,
        Vector3::UNIT_Y,
        Vector3::new(4., 1., 0.),
        Vector3::UNIT_Y * 2.,
    )
    .unwrap();
    let flip = orientation::create(
        &mut doc,
        def,
        orientation::Kind::Flip,
        Vector3::ZERO,
        Vector3::UNIT_X * 10.,
        Vector3::UNIT_Y,
    )
    .unwrap();
    let early = create(&mut doc, def, Kind::Flip, flip, &geometry[..1], DVec3::ZERO).unwrap();
    let movement = create(
        &mut doc,
        def,
        Kind::Move,
        linear,
        &geometry[1..],
        DVec3::ZERO,
    )
    .unwrap();
    let before = doc.clone();
    let selection = [geometry[1], linear];
    assert!(retarget(&mut doc, def, early, &selection, None).is_err());
    assert!(create(
        &mut doc,
        def,
        Kind::Flip,
        flip,
        &[geometry[0], linear],
        DVec3::ZERO
    )
    .is_err());
    assert!(create(&mut doc, def, Kind::Flip, flip, &[flip], DVec3::ZERO).is_err());
    assert_eq!(doc, before);
    let linked = create(&mut doc, def, Kind::Flip, flip, &selection, DVec3::ZERO).unwrap();
    let dependencies = |doc: &Document| {
        actions(doc, def)
            .into_iter()
            .find(|(h, _, _, _)| *h == linked)
            .unwrap()
            .1
            .dependencies
            .clone()
    };
    assert!(dependencies(&doc).contains(&movement));
    assert!(!retarget(&mut doc, def, linked, &selection, None).unwrap());
    let before = doc.clone();
    assert!(retarget(&mut doc, def, movement, &geometry[..1], None).is_err());
    assert!(create(&mut doc, def, Kind::Move, linear, &geometry[1..], DVec3::ZERO).is_err());
    assert_eq!(doc, before);
    assert!(retarget(&mut doc, def, linked, &geometry[1..], None).unwrap());
    assert_eq!(dependencies(&doc), geometry[1..]);
}

#[test]
fn reciprocal_native_flips_preserve_perpendicular_axes_and_compose() {
    let (mut doc, def, geometry) = super::tests::fixture();
    let anchor = Vector3::new(1., 2., 0.);
    let basepoint = block_basepoint::create(&mut doc, def, anchor).unwrap();
    let (_, base) = block_basepoint::parameter(&doc, def).unwrap();
    let base_id = base.parameter.parameter.element.eval.node_id;
    let first = orientation::create(
        &mut doc,
        def,
        orientation::Kind::Flip,
        Vector3::ZERO,
        Vector3::UNIT_X * 10.,
        Vector3::UNIT_Y,
    )
    .unwrap();
    let second = orientation::create(
        &mut doc,
        def,
        orientation::Kind::Flip,
        Vector3::ZERO,
        Vector3::UNIT_Y * 10.,
        Vector3::UNIT_X,
    )
    .unwrap();
    let linear = distance::create(
        &mut doc, def, distance::Kind::Linear,
        Vector3::new(2., 1., 0.), Vector3::new(4., 1., 0.), Vector3::UNIT_X,
    ).unwrap();
    let parameters = all_parameters(&doc, def);
    let a = create(&mut doc, def, Kind::Flip, first, &geometry, DVec3::ZERO).unwrap();
    let b = create(&mut doc, def, Kind::Flip, second, &geometry, DVec3::ZERO).unwrap();
    create(&mut doc, def, Kind::Scale, linear, &geometry, DVec3::ZERO).unwrap();
    for (action, target) in [(a, second), (b, first)] {
        let ObjectType::DynamicBlock(object) = doc.objects.get_mut(&action).unwrap() else {
            panic!()
        };
        let DynamicBlockData::FlipAction(action) = &mut object.data else {
            panic!()
        };
        action.action.dependencies.push(basepoint);
        action.action.action_ids.push(base_id);
        for parameter in [target, linear] {
            action.action.dependencies.push(parameter);
            action.action.action_ids.push(parameters.iter().find(|p| p.handle == parameter).unwrap().id);
        }
    }
    let insert = doc
        .add_entity(EntityType::Insert(document::entities::Insert::new(
            "ACTIONS",
            Vector3::ZERO,
        )))
        .unwrap();
    for order in [[first, second], [second, first]] {
        let mut evaluated = instance::evaluate_value(
            &doc, insert, linear, Value::Point(DVec3::new(6., 1., 0.)),
        ).unwrap().unwrap();
        for axis in order {
            evaluated = instance::evaluate_value(&evaluated, insert, axis, Value::Flip(true))
                .unwrap()
                .unwrap();
        }
        for p in parameters.iter().filter(|p| p.flip) {
            assert_eq!(
                instance::value(&evaluated, insert, p).unwrap(),
                Value::Flip(true)
            );
            let frame = instance::frame(&evaluated, insert, p).unwrap();
            assert!((frame.end + p.end).length() < 1e-8);
        }
        let EntityType::Insert(reference) = evaluated.get_entity(insert).unwrap() else {
            panic!()
        };
        assert_eq!(reference.insert_point, -anchor * 2.);
        let circle_handle = evaluated
            .block_records
            .get(&reference.block_name)
            .unwrap()
            .entity_handles[1];
        let EntityType::Circle(circle) = evaluated.get_entity(circle_handle).unwrap() else {
            panic!()
        };
        assert_eq!(circle.center_wcs(), Vector3::new(-6., -5., 0.));
        instance::clear_canonical_inputs(&mut evaluated, insert);
        let toggled = instance::evaluate_value(&evaluated, insert, first, Value::Flip(false))
            .unwrap()
            .unwrap();
        let EntityType::Circle(circle) = toggled.get_entity(circle_handle).unwrap() else {
            panic!()
        };
        assert_eq!(circle.center_wcs(), Vector3::new(-6., 5., 0.));
    }
}

#[test]
fn authored_flip_link_reflects_an_unbound_flip_axis() {
    let (mut doc, def, _) = super::tests::fixture();
    let source = orientation::create(
        &mut doc,
        def,
        orientation::Kind::Flip,
        Vector3::ZERO,
        Vector3::UNIT_X * 10.,
        Vector3::UNIT_Y,
    )
    .unwrap();
    let target = orientation::create(
        &mut doc,
        def,
        orientation::Kind::Flip,
        Vector3::UNIT_Y * 2.,
        Vector3::new(5., 2., 0.),
        Vector3::UNIT_Y * 3.,
    )
    .unwrap();
    create(&mut doc, def, Kind::Flip, source, &[target], DVec3::ZERO).unwrap();
    let insert = doc
        .add_entity(EntityType::Insert(document::entities::Insert::new(
            "ACTIONS",
            Vector3::ZERO,
        )))
        .unwrap();
    let parameter = all_parameters(&doc, def)
        .into_iter()
        .find(|p| p.handle == target)
        .unwrap();
    let reflected = instance::evaluate_value(&doc, insert, source, Value::Flip(true))
        .unwrap()
        .unwrap();
    assert_eq!(
        instance::frame(&reflected, insert, &parameter).unwrap(),
        instance::ParameterFrame {
            base: -DVec3::Y * 2.,
            end: DVec3::new(5., -2., 0.),
        }
    );
}

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
            .add_entity(EntityType::Insert(document::entities::Insert::new(
                "ACTIONS",
                Vector3::ZERO,
            )))
            .unwrap();
        let second = doc
            .add_entity(EntityType::Insert(document::entities::Insert::new(
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
            let at_default = changed.clone();
            changed = eval(&changed, child, Value::Point(DVec3::new(-15., -2., 0.)));
            assert_eq!(circle(&changed), Vector3::new(-9., -3., 0.));
            let mut native_only = changed.clone();
            instance::clear_canonical_inputs(&mut native_only, first);
            for extension in ["dwg", "dxf"] {
                let bytes =
                    crate::io::save_to_bytes(&native_only, extension, native_only.version).unwrap();
                let reopened =
                    crate::io::load_bytes(&format!("native-only-flips.{extension}"), bytes)
                        .unwrap();
                assert!(!has_inputs(&reopened, first));
                let recovered = eval(&reopened, child, Value::Point(DVec3::new(-18., -2., 0.)));
                assert_eq!(circle(&recovered), Vector3::new(-12., -3., 0.));
                assert!(has_inputs(&recovered, first));
            }
            instance::remove_cache(&mut native_only, first, p.id).unwrap();
            let missing = if testing {
                instance::evaluate_test_value(
                    &native_only,
                    &baseline,
                    def,
                    first,
                    flips[0],
                    Value::Flip(false),
                )
            } else {
                instance::evaluate_value(&native_only, first, flips[0], Value::Flip(false))
            };
            assert!(matches!(missing, Err(super::flip_graph::FRAME_ERROR)));
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
                crate::scene::block_authoring::edit::remove_parameter(&mut edited, def, flips[1])
                    .unwrap();
                let single = instance::refresh_after_edit(&edited, &changed, def).unwrap();
                assert_eq!(circle(&single), Vector3::new(9., -3., 0.));
                assert_eq!(
                    instance::frame(&single, first, &p).unwrap().end,
                    DVec3::new(15., -2., 0.)
                );
                assert!(has_inputs(&single, first));

                let mut ordinary = single.clone();
                let ObjectType::DynamicBlock(o) =
                    ordinary.objects.get_mut(&flip_actions[0]).unwrap()
                else {
                    panic!()
                };
                targets::action_mut(&mut o.data).unwrap().dependencies = vec![geometry[1]];
                let ordinary = instance::refresh_after_edit(&ordinary, &single, def).unwrap();
                assert!(!has_inputs(&ordinary, first));
                assert_eq!(circle(&ordinary), Vector3::new(9., -3., 0.));
                assert_eq!(
                    instance::frame(&ordinary, first, &p).unwrap().end,
                    DVec3::new(15., 2., 0.)
                );
                let resaved = instance::refresh_after_edit(&ordinary, &single, def).unwrap();
                assert_eq!(circle(&resaved), Vector3::new(9., -3., 0.));
                assert_eq!(
                    circle(&eval(&resaved, flips[0], Value::Flip(false))),
                    Vector3::new(9., 3., 0.)
                );

                let mut legacy = single.clone();
                let ObjectType::DynamicBlock(o) = legacy.objects.get_mut(&flip_actions[0]).unwrap()
                else {
                    panic!()
                };
                targets::action_mut(&mut o.data).unwrap().dependencies =
                    vec![geometry[1], child, linked];
                for grip in
                    crate::scene::block_grips::config::locations(&legacy, def, child).unwrap()
                {
                    crate::scene::block_grips::config::set_location(
                        &mut legacy,
                        def,
                        child,
                        grip.role,
                        grip.key_point,
                    )
                    .unwrap();
                }
                assert!(super::flip_frames::Plan::new(&legacy, def).is_some());
                let legacy = instance::refresh_after_edit(&legacy, &single, def).unwrap();
                assert!(!has_inputs(&legacy, first));
                assert_eq!(circle(&legacy), Vector3::new(9., -3., 0.));
                assert_eq!(
                    instance::frame(&legacy, first, &p).unwrap().end,
                    DVec3::new(15., -2., 0.)
                );
                let resaved = instance::refresh_after_edit(&legacy, &single, def).unwrap();
                assert_eq!(circle(&resaved), Vector3::new(9., -3., 0.));
                let resaved = instance::refresh(&resaved, def).unwrap();
                let selected =
                    block_lookup::evaluate_reference_value(&resaved, first, lookup, "Long")
                        .unwrap()
                        .unwrap();
                assert!(!has_inputs(&selected, first));
                assert_eq!(circle(&selected), Vector3::new(11., -3., 0.));
                let selected = instance::evaluate_table_row(&selected, first, &columns, 1)
                    .unwrap()
                    .unwrap();
                assert_eq!(circle(&selected), Vector3::new(13., -3., 0.));
                for extension in ["dwg", "dxf"] {
                    let bytes =
                        crate::io::save_to_bytes(&selected, extension, selected.version).unwrap();
                    let saved =
                        crate::io::load_bytes(&format!("native-flips.{extension}"), bytes).unwrap();
                    assert!(!has_inputs(&saved, first));
                    let unflipped = eval(&saved, flips[0], Value::Flip(false));
                    assert_eq!(circle(&unflipped), Vector3::new(13., 3., 0.));
                    assert_eq!(
                        instance::frame(&unflipped, first, &p).unwrap().end,
                        DVec3::new(19., 2., 0.)
                    );
                }

                let mut no_flip = single.clone();
                crate::scene::block_authoring::edit::remove_parameter(&mut no_flip, def, flips[0])
                    .unwrap();
                let no_flip = instance::refresh_after_edit(&no_flip, &single, def).unwrap();
                assert!(!has_inputs(&no_flip, first));
                assert_eq!(circle(&no_flip), Vector3::new(9., 3., 0.));

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

                let mut with_parent = changed.clone();
                let parent = crate::scene::block_point_parameter::create(
                    &mut with_parent,
                    def,
                    Vector3::ZERO,
                    Vector3::UNIT_X,
                )
                .unwrap();
                create(
                    &mut with_parent,
                    def,
                    Kind::Move,
                    parent,
                    &[child, flips[0]],
                    DVec3::ZERO,
                )
                .unwrap();
                let with_parent =
                    instance::refresh_after_edit(&with_parent, &changed, def).unwrap();
                let added = all_parameters(&with_parent, def)
                    .into_iter()
                    .find(|p| p.handle == parent)
                    .unwrap();
                assert_eq!(
                    instance::frame(&with_parent, first, &added).unwrap().end,
                    DVec3::ZERO
                );
                assert_eq!(circle(&with_parent), Vector3::new(-9., -3., 0.));
                let transported = eval(&with_parent, parent, Value::Point(DVec3::Y * 3.));
                let mut native_only = transported.clone();
                instance::clear_canonical_inputs(&mut native_only, first);
                let recovered = eval(&native_only, child, Value::Point(DVec3::new(-18., -5., 0.)));
                assert_eq!(circle(&recovered), Vector3::new(-12., -3., 0.));
                assert_eq!(
                    instance::frame(&transported, first, &p).unwrap().end,
                    DVec3::new(-15., -5., 0.)
                );
                assert_eq!(
                    instance::frame(&transported, first, &added).unwrap().end,
                    DVec3::Y * 3.
                );
                let resaved = instance::refresh_after_edit(&transported, &changed, def).unwrap();
                assert_eq!(
                    instance::frame(&resaved, first, &p).unwrap().end,
                    DVec3::new(-15., -5., 0.)
                );

                let mut deleted = transported.clone();
                crate::scene::block_authoring::edit::remove_parameter(&mut deleted, def, parent)
                    .unwrap();
                let deleted = instance::refresh_after_edit(&deleted, &transported, def).unwrap();
                assert_eq!(
                    instance::frame(&deleted, first, &p).unwrap().end,
                    DVec3::new(-15., -2., 0.)
                );
                assert_eq!(
                    instance::frame(&deleted, first, &added).unwrap().end,
                    DVec3::ZERO
                );
                assert_eq!(circle(&deleted), Vector3::new(-9., -3., 0.));
                let next = eval(&deleted, child, Value::Point(DVec3::new(-18., -2., 0.)));
                assert_eq!(circle(&next), Vector3::new(-12., -3., 0.));

                use crate::scene::block_authoring::edit::{move_part, Part};
                let mut edited = at_default.clone();
                move_part(
                    &mut edited,
                    def,
                    child,
                    Part::End,
                    Vector3::new(0., 22., 0.),
                )
                .unwrap();
                let new_default = instance::refresh_after_edit(&edited, &at_default, def).unwrap();
                let linear = all_parameters(&new_default, def)
                    .into_iter()
                    .find(|q| q.handle == child)
                    .unwrap();
                assert_eq!(
                    instance::frame(&new_default, first, &linear).unwrap().end,
                    DVec3::new(0., -22., 0.)
                );
                assert_eq!(circle(&new_default), Vector3::new(-4., -3., 0.));

                let mut edited = transported.clone();
                move_part(&mut edited, def, parent, Part::Point, Vector3::UNIT_X * 5.).unwrap();
                let moved_parent =
                    instance::refresh_after_edit(&edited, &transported, def).unwrap();
                let point = all_parameters(&moved_parent, def)
                    .into_iter()
                    .find(|q| q.handle == parent)
                    .unwrap();
                assert_eq!(
                    instance::frame(&moved_parent, first, &point).unwrap().end,
                    DVec3::new(5., 3., 0.)
                );
                assert_eq!(
                    instance::frame(&moved_parent, first, &p).unwrap().end,
                    DVec3::new(-15., -5., 0.)
                );

                let mut edited = transported.clone();
                move_part(
                    &mut edited,
                    def,
                    child,
                    Part::End,
                    Vector3::new(0., 12., 0.),
                )
                .unwrap();
                let reframed = instance::refresh_after_edit(&edited, &transported, def).unwrap();
                let linear = all_parameters(&reframed, def)
                    .into_iter()
                    .find(|q| q.handle == child)
                    .unwrap();
                assert_eq!(
                    instance::frame(&reframed, first, &linear).unwrap(),
                    instance::ParameterFrame {
                        base: DVec3::new(0., -5., 0.),
                        end: DVec3::new(0., -20., 0.),
                    }
                );
                assert_eq!(circle(&reframed), Vector3::new(-4., -8., 0.));
                let mut edited = reframed.clone();
                move_part(&mut edited, def, flips[0], Part::Base, Vector3::UNIT_Y).unwrap();
                move_part(
                    &mut edited,
                    def,
                    flips[0],
                    Part::End,
                    Vector3::new(10., 1., 0.),
                )
                .unwrap();
                let reframed = instance::refresh_after_edit(&edited, &reframed, def).unwrap();
                assert_eq!(circle(&reframed), Vector3::new(-4., -6., 0.));
                for extension in ["dwg", "dxf"] {
                    let bytes =
                        crate::io::save_to_bytes(&reframed, extension, reframed.version).unwrap();
                    let reopened =
                        crate::io::load_bytes(&format!("reframed-flips.{extension}"), bytes)
                            .unwrap();
                    assert_eq!(circle(&reopened), Vector3::new(-4., -6., 0.));
                    let toggled = eval(&reopened, flips[0], Value::Flip(false));
                    assert_eq!(circle(&toggled), Vector3::new(-4., 8., 0.));
                }
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

fn has_inputs(doc: &Document, insert: Handle) -> bool {
    doc.get_entity(insert)
        .unwrap()
        .common()
        .extended_data
        .get_record(super::flip_graph::INPUTS)
        .is_some()
}

#[test]
fn coupled_refresh_migrates_polar_and_rotation_values() {
    use crate::scene::block_authoring::edit::{move_part, Part};
    let (mut doc, def, geometry) = super::tests::fixture();
    let linear = distance::create(
        &mut doc,
        def,
        distance::Kind::Linear,
        Vector3::ZERO,
        Vector3::UNIT_X * 10.,
        Vector3::UNIT_Y,
    )
    .unwrap();
    let polar = distance::create(
        &mut doc,
        def,
        distance::Kind::Polar,
        Vector3::ZERO,
        Vector3::UNIT_X * 4.,
        Vector3::UNIT_Y,
    )
    .unwrap();
    let rotation = orientation::create(
        &mut doc,
        def,
        orientation::Kind::Rotation,
        Vector3::ZERO,
        Vector3::UNIT_Y * 4.,
        Vector3::UNIT_X * 4.,
    )
    .unwrap();
    create(
        &mut doc,
        def,
        Kind::Move,
        polar,
        &geometry[1..],
        DVec3::ZERO,
    )
    .unwrap();
    create(
        &mut doc,
        def,
        Kind::Rotate,
        rotation,
        &geometry[..1],
        DVec3::ZERO,
    )
    .unwrap();
    create(
        &mut doc,
        def,
        Kind::Move,
        linear,
        &geometry[1..],
        DVec3::ZERO,
    )
    .unwrap();
    let mut flips = Vec::new();
    for axis in [Vector3::UNIT_X, Vector3::UNIT_Y] {
        let flip = orientation::create(
            &mut doc,
            def,
            orientation::Kind::Flip,
            Vector3::ZERO,
            axis * 10.,
            Vector3::UNIT_X,
        )
        .unwrap();
        let selection = [geometry[1], linear];
        if flips.is_empty() {
            create(&mut doc, def, Kind::Flip, flip, &selection, DVec3::ZERO).unwrap();
        } else {
            let action = create(&mut doc, def, Kind::Flip, flip, &geometry[1..], DVec3::ZERO).unwrap();
            assert!(retarget(&mut doc, def, action, &selection, None).unwrap());
        }
        flips.push(flip);
    }
    let mut inserts = Vec::new();
    for origin in [Vector3::ZERO, Vector3::UNIT_Y * 30.] {
        let insert = doc
            .add_entity(EntityType::Insert(document::entities::Insert::new(
                "ACTIONS", origin,
            )))
            .unwrap();
        for flip in &flips {
            doc = instance::evaluate_value(&doc, insert, *flip, Value::Flip(true))
                .unwrap()
                .unwrap();
        }
        inserts.push(insert);
    }
    let edited_insert = inserts[0];
    let default_insert = inserts[1];
    for (p, end) in [(polar, DVec3::Y * 6.), (rotation, -DVec3::X * 4.)] {
        doc = instance::evaluate_value(&doc, edited_insert, p, Value::Point(end))
            .unwrap()
            .unwrap();
    }
    let mut definition_edit = doc.clone();
    move_part(
        &mut definition_edit,
        def,
        polar,
        Part::End,
        -Vector3::UNIT_X * 8.,
    )
    .unwrap();
    move_part(
        &mut definition_edit,
        def,
        rotation,
        Part::Reference,
        -Vector3::UNIT_Y * 7.,
    )
    .unwrap();
    move_part(
        &mut definition_edit,
        def,
        rotation,
        Part::End,
        Vector3::UNIT_X * 7.,
    )
    .unwrap();
    let before_reference = instance::refresh_after_edit(&definition_edit, &doc, def).unwrap();
    let mut reference_edit = before_reference.clone();
    move_part(
        &mut reference_edit,
        def,
        rotation,
        Part::Reference,
        Vector3::UNIT_Y * 7.,
    )
    .unwrap();
    let refreshed = instance::refresh_after_edit(&reference_edit, &before_reference, def).unwrap();
    let refreshed = instance::refresh_after_edit(&refreshed, &before_reference, def).unwrap();
    let close = |actual: DVec3, expected: DVec3| assert!(actual.distance(expected) < 1e-8);
    for extension in ["dwg", "dxf"] {
        let bytes = crate::io::save_to_bytes(&refreshed, extension, refreshed.version).unwrap();
        let reopened =
            crate::io::load_bytes(&format!("angular-reframe.{extension}"), bytes).unwrap();
        let parameters = all_parameters(&reopened, def);
        for (handle, edited, default) in [
            (polar, DVec3::Y * 6., -DVec3::X * 8.),
            (rotation, -DVec3::X * 7., DVec3::X * 7.),
        ] {
            let parameter = parameters.iter().find(|p| p.handle == handle).unwrap();
            close(
                instance::frame(&reopened, edited_insert, parameter)
                    .unwrap()
                    .end,
                edited,
            );
            close(
                instance::frame(&reopened, default_insert, parameter)
                    .unwrap()
                    .end,
                default,
            );
        }
        let EntityType::Insert(reference) = reopened.get_entity(edited_insert).unwrap() else {
            panic!()
        };
        let mut members = reopened.entities_in_block(&reference.block_name);
        let line = members
            .by_ref()
            .find_map(|entity| match entity {
                EntityType::Line(line) => Some(line),
                _ => None,
            })
            .unwrap();
        close(v(line.start), -DVec3::X);
        let circle = reopened
            .entities_in_block(&reference.block_name)
            .find_map(|entity| match entity {
                EntityType::Circle(circle) => Some(circle),
                _ => None,
            })
            .unwrap();
        close(v(circle.center), DVec3::new(-12., -9., 0.));
    }
}
