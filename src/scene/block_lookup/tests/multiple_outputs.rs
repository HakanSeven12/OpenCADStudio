use super::*;
use crate::scene::block_parameter_properties::{self as native, Input};
use crate::scene::model::object::{BlockParameterKey, PropValue};

fn setup() -> (Document, Handle, Handle, Handle, Handle) {
    let (mut doc, def) = fixture();
    add_distance(&mut doc, def);
    let p = crate::scene::block_actions::all_parameters(&doc, def).remove(0);
    let line = doc.block_records.get("LOOKUP").unwrap().entity_handles[0];
    crate::scene::block_actions::create(
        &mut doc,
        def,
        crate::scene::block_actions::Kind::Move,
        p.handle,
        &[line],
        glam::DVec3::ZERO,
    )
    .unwrap();
    let first = create(&mut doc, def, Vector3::ZERO).unwrap();
    let second = create(&mut doc, def, Vector3::UNIT_Y).unwrap();
    let mut edited = draft(&doc, def, first);
    edited.columns.push(
        empty_table(
            parameters(&doc, def)
                .into_iter()
                .find(|(h, _)| *h == second)
                .unwrap()
                .1,
        )
        .columns
        .remove(0),
    );
    edited.column_count = 3;
    edited.expressions = ["2", "Small", "Large", "4", "Large", "Small"]
        .map(str::to_owned)
        .to_vec();
    apply_table(&mut doc, def, first, edited).unwrap();
    let insert = doc
        .add_entity(EntityType::Insert(Insert::new("LOOKUP", Vector3::ZERO)))
        .unwrap();
    (doc, def, first, second, insert)
}

fn choices(
    doc: &Document,
    baseline: Option<&Document>,
    insert: Handle,
) -> Vec<(BlockParameterKey, usize, Vec<String>)> {
    native::properties(doc, baseline, 17, insert)
        .into_iter()
        .filter_map(|p| {
            if p.field != "dyn_lookup" {
                return None;
            }
            match p.value {
                PropValue::BlockChoice {
                    key,
                    selected,
                    options,
                    ..
                } => Some((key, selected, options)),
                _ => None,
            }
        })
        .collect()
}

#[test]
fn lookup_multiple_outputs_select_by_column_in_drawing_and_scratch() {
    for testing in [false, true] {
        let (mut doc, def, first, second, insert) = setup();
        let baseline = doc.clone();
        let source = testing.then_some(&baseline);
        let initial = choices(&doc, source, insert);
        assert_eq!(initial.len(), 2);
        assert_ne!(initial[0].0, initial[1].0);
        assert_eq!(initial[0].2, ["Small", "Large"]);
        assert_eq!(initial[1].2, ["Large", "Small"]);
        assert!(initial.iter().all(|p| p.1 == 0));
        assert!(
            native::evaluate(&doc, source, initial[1].0, Input::Choice(0))
                .unwrap()
                .is_none()
        );
        assert!(
            evaluate_reference_choice(&doc, insert, "Large").is_err(),
            "plain label has conflicting rows"
        );
        let by_name = evaluate_reference_value(&doc, insert, second, "Small")
            .unwrap()
            .unwrap();
        assert!(choices(&by_name, None, insert).iter().all(|p| p.1 == 1));
        for (column, row) in [(1, 1), (0, 0), (0, 1)] {
            let key = choices(&doc, source, insert)[column].0;
            doc = native::evaluate(&doc, source, key, Input::Choice(row))
                .unwrap()
                .unwrap();
            assert!(choices(&doc, source, insert).iter().all(|p| p.1 == row));
            let parameter = crate::scene::block_actions::all_parameters(&baseline, def).remove(0);
            let frame =
                crate::scene::block_actions::instance::frame(&doc, insert, &parameter).unwrap();
            assert_eq!(frame.end.x, if row == 0 { 2. } else { 4. });
            let EntityType::Insert(reference) = doc.get_entity(insert).unwrap() else {
                panic!()
            };
            let member = doc
                .block_records
                .get(&reference.block_name)
                .unwrap()
                .entity_handles[0];
            let EntityType::Line(line) = doc.get_entity(member).unwrap() else {
                panic!()
            };
            assert_eq!(line.start.x, if row == 0 { 0. } else { 2. });
            assert!(native::evaluate(&doc, source, key, Input::Choice(row))
                .unwrap()
                .is_none());
        }
        assert_eq!(
            table(&doc, def, first).unwrap(),
            table(&doc, def, second).unwrap()
        );
        for extension in ["dwg", "dxf"] {
            let bytes = crate::io::save_to_bytes(&doc, extension, doc.version).unwrap();
            let read = crate::io::load_bytes(&format!("multi.{extension}"), bytes).unwrap();
            assert!(choices(&read, None, insert).iter().all(|p| p.1 == 1));
            assert_links(&read, def);
        }
    }
}

#[test]
fn lookup_multiple_outputs_link_unlink_and_reject_conflicting_ownership_atomically() {
    let (mut doc, def, first, second, _) = setup();
    let shared = table(&doc, def, first).unwrap().unwrap();
    assert_eq!(Some(shared.clone()), table(&doc, def, second).unwrap());
    assert_links(&doc, def);
    roundtrip(&doc, def);
    let before = doc.clone();
    let mut duplicate = shared.1.clone();
    duplicate.columns[2] = duplicate.columns[1].clone();
    assert!(apply_table(&mut doc, def, first, duplicate).is_err());
    assert_eq!(doc, before);
    let third = create(&mut doc, def, Vector3::UNIT_X).unwrap();
    let mut stealing = draft(&doc, def, third);
    stealing.columns.push(shared.1.columns[2].clone());
    stealing.column_count = 3;
    stealing.expressions = shared.1.expressions.clone();
    let before = doc.clone();
    assert!(apply_table(&mut doc, def, third, stealing).is_err());
    assert_eq!(doc, before);
    let mut reduced = shared.1;
    reduced.columns.remove(2);
    reduced.column_count = 2;
    reduced.expressions = ["2", "Small", "4", "Large"].map(str::to_owned).to_vec();
    apply_table(&mut doc, def, first, reduced).unwrap();
    assert!(table(&doc, def, second).unwrap().is_none());
    assert_links(&doc, def);
    roundtrip(&doc, def);
}

#[test]
fn lookup_multiple_outputs_visibility_readonly_and_stale_keys() {
    let (mut doc, def, first, second, insert) = setup();
    let keys = choices(&doc, None, insert);
    crate::scene::block_authoring::metadata::set_visible(&mut doc, def, first, false).unwrap();
    assert_eq!(choices(&doc, None, insert).len(), 1);
    assert!(native::evaluate(&doc, None, keys[0].0, Input::Choice(1)).is_err());
    assert!(native::evaluate(&doc, None, keys[1].0, Input::Choice(1))
        .unwrap()
        .is_some());
    let (_, mut edited) = table(&doc, def, first).unwrap().unwrap();
    edited.columns[2].writable = false;
    apply_table(&mut doc, def, first, edited).unwrap();
    assert!(choices(&doc, None, insert).is_empty());
    assert!(native::evaluate(&doc, None, keys[1].0, Input::Choice(1)).is_err());
    let rows = native::properties(&doc, None, 17, insert);
    assert!(rows.iter().any(|p| p.field == "dyn_lookup"
        && matches!(&p.value,
        PropValue::ReadOnlyWithTooltip { value, .. } if value == "Large")));
    crate::scene::block_authoring::metadata::set_visible(&mut doc, def, second, false).unwrap();
    assert!(!native::properties(&doc, None, 17, insert)
        .iter()
        .any(|p| p.field == "dyn_lookup"));
}

#[test]
fn lookup_multiple_outputs_reject_incompatible_rows_and_shared_scratch_references() {
    let (mut doc, def, first, _, insert) = setup();
    let key = choices(&doc, None, insert)[1].0;
    let baseline = doc.clone();
    doc.add_entity(EntityType::Insert(Insert::new("LOOKUP", Vector3::UNIT_Y)))
        .unwrap();
    let before = doc.clone();
    assert!(native::evaluate(&doc, Some(&baseline), key, Input::Choice(1)).is_err());
    assert_eq!(doc, before);
    // A changed table cannot accept an outstanding callback.
    let (_, mut edited) = table(&doc, def, first).unwrap().unwrap();
    edited.expressions[3] = " 6 ".into();
    edited.expressions[5] = "Secondary only".into();
    apply_table(&mut doc, def, first, edited).unwrap();
    assert!(native::evaluate(&doc, None, key, Input::Choice(1)).is_err());
    let key = choices(&doc, None, insert)[1].0;
    let evaluated = native::evaluate(&doc, None, key, Input::Choice(1))
        .unwrap()
        .unwrap();
    assert_eq!(
        evaluate_reference_choice(&doc, insert, "Secondary only")
            .unwrap()
            .unwrap(),
        evaluated
    );
    assert!(
        choices(&evaluated, None, insert).iter().all(|p| p.1 == 1),
        "numeric whitespace is accepted consistently"
    );
    let p = crate::scene::block_actions::all_parameters(&doc, def).remove(0);
    let ObjectType::DynamicBlock(object) = doc.objects.get_mut(&p.handle).unwrap() else {
        panic!()
    };
    let DynamicBlockData::LinearParameter(parameter) = &mut object.data else {
        panic!()
    };
    parameter.value_set.flags = 8;
    parameter.value_set.values = vec![2., 8.];
    let before = doc.clone();
    assert!(native::evaluate(&doc, None, key, Input::Choice(1)).is_err());
    assert_eq!(doc, before, "clamped values cannot partially publish a row");
}
