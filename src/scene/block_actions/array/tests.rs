use super::*;
use crate::scene::block_distance_parameter as distance;
use codec::entities::Insert;

fn setup(
    kind: distance::Kind,
    targets_per_action: usize,
    action_count: usize,
) -> (Document, Handle, Parameter, BlockArrayAction) {
    let (mut doc, definition, targets) = super::super::tests::fixture();
    let handle = distance::create(
        &mut doc,
        definition,
        kind,
        Vector3::ZERO,
        Vector3::new(10., if kind == distance::Kind::XY { 10. } else { 0. }, 0.),
        Vector3::new(5., 2., 0.),
    )
    .unwrap();
    let mut action = None;
    for _ in 0..action_count {
        action = Some(
            create_array(
                &mut doc,
                definition,
                handle,
                &targets[..targets_per_action],
                DVec3::ZERO,
                1.,
                1.,
            )
            .unwrap(),
        );
    }
    let ObjectType::DynamicBlock(object) = &doc.objects[&action.unwrap()] else {
        panic!()
    };
    let DynamicBlockData::ArrayAction(action) = &object.data else {
        panic!()
    };
    let action = action.clone();
    let parameter = parameters(&doc, definition, Kind::Array)
        .into_iter()
        .find(|p| p.handle == handle)
        .unwrap();
    (doc, definition, parameter, action)
}

#[test]
fn block_array_plans_bound_extreme_spans_and_aggregate_generated_entities() {
    for kind in [
        distance::Kind::Linear,
        distance::Kind::Polar,
        distance::Kind::XY,
    ] {
        let (_, _, p, action) = setup(kind, 1, 1);
        let mut remaining = MAX_GENERATED_ENTITIES;
        assert_eq!(
            Plan::new(&p, p.base, DVec3::splat(1e100), &action, 1, &mut remaining).unwrap_err(),
            LIMIT_ERROR
        );
        assert_eq!(remaining, MAX_GENERATED_ENTITIES);
        let mut invalid = p.end;
        invalid.x = f64::INFINITY;
        assert!(Plan::new(&p, p.base, invalid, &action, 1, &mut remaining).is_err());
        assert_eq!(remaining, MAX_GENERATED_ENTITIES);
    }
    let (_, _, p, action) = setup(distance::Kind::Linear, 2, 1);
    let mut remaining = MAX_GENERATED_ENTITIES;
    let boundary = Plan::new(&p, p.base, DVec3::X * 50_000., &action, 2, &mut remaining).unwrap();
    assert_eq!(remaining, 0);
    assert_eq!(boundary.displacements().size_hint(), (50_000, Some(50_000)));
    assert_eq!(
        boundary.displacements().last(),
        Some((50_000, DVec3::X * 50_000.))
    );
    assert_eq!(
        Plan::new(&p, p.base, DVec3::X, &action, 1, &mut remaining).unwrap_err(),
        LIMIT_ERROR
    );
    assert_eq!(remaining, 0);
    let mut remaining = MAX_GENERATED_ENTITIES;
    assert_eq!(
        Plan::new(&p, p.base, DVec3::X * 50_001., &action, 2, &mut remaining).unwrap_err(),
        LIMIT_ERROR
    );
    assert_eq!(remaining, MAX_GENERATED_ENTITIES);
    let (_, _, p, action) = setup(distance::Kind::XY, 1, 1);
    assert_eq!(
        Plan::new(
            &p,
            p.base,
            DVec3::new(400., 400., 0.),
            &action,
            1,
            &mut remaining
        )
        .unwrap_err(),
        LIMIT_ERROR
    );
    assert_eq!(remaining, MAX_GENERATED_ENTITIES);
}

#[test]
fn block_array_plans_preserve_signed_row_order_and_omit_self_grip_copies() {
    let (_, _, p, action) = setup(distance::Kind::XY, 1, 1);
    let mut remaining = MAX_GENERATED_ENTITIES;
    let base = DVec3::new(20., 30., 0.);
    let plan = Plan::new(
        &p,
        base,
        base + DVec3::new(-2., -1., 0.),
        &action,
        2,
        &mut remaining,
    )
    .unwrap();
    assert_eq!(
        plan.displacements().collect::<Vec<_>>(),
        vec![
            (1, DVec3::new(-1., 0., 0.)),
            (2, DVec3::new(-2., 0., 0.)),
            (3, DVec3::new(0., -1., 0.)),
            (4, DVec3::new(-1., -1., 0.)),
            (5, DVec3::new(-2., -1., 0.)),
        ]
    );
    assert_eq!(remaining, MAX_GENERATED_ENTITIES - 10);
    let empty = Plan::new(&p, base, DVec3::splat(1e100), &action, 0, &mut remaining).unwrap();
    assert_eq!(empty.displacements().count(), 0);
    assert_eq!(remaining, MAX_GENERATED_ENTITIES - 10);
    let (_, _, p, action) = setup(distance::Kind::Polar, 1, 1);
    let polar = Plan::new(&p, base, base - DVec3::Y * 2., &action, 1, &mut remaining).unwrap();
    assert_eq!(
        polar.displacements().collect::<Vec<_>>(),
        vec![(1, -DVec3::Y), (2, -DVec3::Y * 2.)]
    );
}

#[test]
fn block_array_over_budget_evaluation_is_atomic_across_actions_and_reference_contexts() {
    for kind in [
        distance::Kind::Linear,
        distance::Kind::Polar,
        distance::Kind::XY,
    ] {
        for actions in [1, 2] {
            let (mut doc, definition, p, _) = setup(kind, 2, actions);
            let first = doc
                .add_entity(EntityType::Insert(Insert::new("ACTIONS", Vector3::ZERO)))
                .unwrap();
            let second = doc
                .add_entity(EntityType::Insert(Insert::new(
                    "ACTIONS",
                    Vector3::new(50., 0., 0.),
                )))
                .unwrap();
            let baseline = doc.clone();
            // Each of two actions fits separately (60000 copies), but not together.
            let span = if actions == 1 { 50_001. } else { 30_000. };
            let end = DVec3::new(span, 0., 0.);
            let error = evaluate(&mut doc, &baseline, definition, p.handle, end).unwrap_err();
            assert_eq!(error, LIMIT_ERROR, "{kind:?}, {actions}");
            assert_eq!(doc, baseline);
            assert_eq!(
                instance::evaluate_value(&doc, first, p.handle, Value::Point(end)).unwrap_err(),
                LIMIT_ERROR
            );
            assert_eq!(doc, baseline);
            assert_eq!(
                instance::evaluate_test_value(
                    &doc,
                    &baseline,
                    definition,
                    first,
                    p.handle,
                    Value::Point(end)
                )
                .unwrap_err(),
                LIMIT_ERROR
            );
            assert_eq!(doc, baseline);
            assert_eq!(instance::frame(&doc, second, &p).unwrap().end, p.end);
            // A rejected request does not poison later valid generation/reset.
            let valid =
                instance::evaluate_value(&doc, first, p.handle, Value::Point(DVec3::X * 2.))
                    .unwrap()
                    .unwrap();
            assert_eq!(doc, baseline);
            assert_eq!(instance::frame(&valid, second, &p).unwrap().end, p.end);
        }
    }
}

#[test]
fn block_array_clone_tags_do_not_overflow_for_large_valid_arrays() {
    let (mut doc, definition, p, _) = setup(distance::Kind::Linear, 1, 1);
    let baseline = doc.clone();
    evaluate(
        &mut doc,
        &baseline,
        definition,
        p.handle,
        DVec3::X * 21_475.,
    )
    .unwrap();
    let members = authoring::members(&doc, definition);
    assert_eq!(members.len(), 21_477);
    let tags = members
        .into_iter()
        .filter_map(|h| doc.get_entity(h).and_then(authoring::element_id))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(tags.len(), 21_477);
}
