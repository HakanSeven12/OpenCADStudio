use super::*;
use crate::scene::block_actions as actions;

pub(super) fn setup() -> (Document, Handle, Handle, Handle, Vec<Handle>) {
    let (mut doc, def) = fixture();
    for end in [2., 3.] {
        distance::create(
            &mut doc,
            def,
            distance::Kind::Linear,
            Vector3::ZERO,
            Vector3::new(end, 0., 0.),
            Vector3::UNIT_Y,
        )
        .unwrap();
    }
    let inputs = actions::all_parameters(&doc, def)
        .iter()
        .map(|p| p.handle)
        .collect::<Vec<_>>();
    let target = doc.block_records.get("LOOKUP").unwrap().entity_handles[0];
    actions::create(
        &mut doc,
        def,
        actions::Kind::Move,
        inputs[0],
        &[target],
        glam::DVec3::ZERO,
    )
    .unwrap();
    let lookup = create(&mut doc, def, Vector3::ZERO).unwrap();
    let insert = doc
        .add_entity(EntityType::Insert(Insert::new("LOOKUP", Vector3::ZERO)))
        .unwrap();
    (doc, def, lookup, insert, inputs)
}

pub(super) fn values(doc: &Document, def: Handle, insert: Handle) -> Vec<f64> {
    actions::all_parameters(doc, def)
        .iter()
        .map(|p| {
            let f = actions::instance::frame(doc, insert, p).unwrap();
            f.end.distance(f.base)
        })
        .collect()
}

#[test]
fn block_lookup_ranges_and_formulas_resolve_named_dependencies_and_preserve_native_text() {
    let (mut doc, def, lookup, insert, _) = setup();
    let mut edited = draft(&doc, def, lookup);
    edited
        .columns
        .insert(1, properties(&doc, def)[1].column.clone());
    edited.column_count = 3;
    edited.expressions = [
        " [2,6] ",
        " =Distance1 * 2 ",
        "Range",
        "sqrt(64)",
        "pow(Distance1,2)/4",
        "Formula",
    ]
    .map(str::to_owned)
    .to_vec();
    let literal = edited.expressions.clone();
    apply_table(&mut doc, def, lookup, edited).unwrap();
    let first = evaluate_reference_value(&doc, insert, lookup, "Range")
        .unwrap()
        .unwrap();
    assert_eq!(values(&first, def, insert), [2., 4.]);
    let second = evaluate_reference_value(&first, insert, lookup, "Formula")
        .unwrap()
        .unwrap();
    assert_eq!(values(&second, def, insert), [8., 16.]);
    let third = evaluate_reference_value(&second, insert, lookup, "Range")
        .unwrap()
        .unwrap();
    assert_eq!(values(&third, def, insert), [6., 12.]);
    assert!(evaluate_reference_value(&third, insert, lookup, "Range")
        .unwrap()
        .is_none());
    for extension in ["dwg", "dxf"] {
        let bytes = crate::io::save_to_bytes(&third, extension, third.version).unwrap();
        let loaded =
            crate::io::load_bytes(&format!("lookup-expressions.{extension}"), bytes).unwrap();
        let table = table(&loaded, def, lookup).unwrap().unwrap().1;
        assert_eq!(table.expressions, literal);
        assert_eq!(
            reference_row_context(&loaded, None, insert, def, &table),
            Some(0)
        );
        assert_eq!(values(&loaded, def, insert), [6., 12.]);
    }
}

#[test]
fn block_lookup_expression_cycles_domains_undefined_names_and_duplicates_fail_atomically() {
    let (mut doc, def, lookup, _, _) = setup();
    for expressions in [
        ["Distance2+1", "Distance1+1", "Cycle", "8", "4", "Other"],
        ["missing+1", "2", "Unknown", "8", "4", "Other"],
        ["1/0", "2", "Division", "8", "4", "Other"],
        ["sqrt(-1)", "2", "Domain", "8", "4", "Other"],
        ["(3,3)", "2", "Empty", "8", "4", "Other"],
        ["[6,2]", "2", "Reversed", "8", "4", "Other"],
        ["1+1", "2", "Duplicate", "2", "2", "Other"],
    ] {
        let mut edited = draft(&doc, def, lookup);
        edited
            .columns
            .insert(1, properties(&doc, def)[1].column.clone());
        edited.column_count = 3;
        edited.expressions = expressions.map(str::to_owned).to_vec();
        let before = doc.clone();
        assert!(
            apply_table(&mut doc, def, lookup, edited).is_err(),
            "{expressions:?}"
        );
        assert!(doc == before);
    }
    for source in [
        "(".repeat(70) + "1" + &")".repeat(70),
        "1+".repeat(300) + "1",
    ] {
        assert!(super::super::expressions::Cell::parse(&source).is_err());
    }
}

#[test]
fn block_lookup_open_intervals_clamp_and_overlapping_rows_are_ambiguous() {
    let (mut doc, def, lookup, insert, inputs) = setup();
    let mut edited = draft(&doc, def, lookup);
    edited.expressions = ["(2,6)", "Open", "8", "Outside"]
        .map(str::to_owned)
        .to_vec();
    apply_table(&mut doc, def, lookup, edited.clone()).unwrap();
    let clamped = evaluate_reference_value(&doc, insert, lookup, "Open")
        .unwrap()
        .unwrap();
    let value = values(&clamped, def, insert)[0];
    assert!(value > 2. && value < 6.);
    edited.expressions = ["[1,5]", "First", "[3,7]", "Second"]
        .map(str::to_owned)
        .to_vec();
    apply_table(&mut doc, def, lookup, edited).unwrap();
    let overlapping = actions::instance::evaluate_value(
        &doc,
        insert,
        inputs[0],
        actions::Value::Point(glam::DVec3::new(4., 0., 0.)),
    )
    .unwrap()
    .unwrap();
    let view = table(&overlapping, def, lookup).unwrap().unwrap().1;
    assert_eq!(
        reference_row_context(&overlapping, None, insert, def, &view),
        None
    );
    let before = overlapping.clone();
    assert!(evaluate_reference_value(&overlapping, insert, lookup, "First").is_err());
    assert!(overlapping == before);
}

#[test]
fn block_lookup_range_selection_intersects_native_lists_and_increments() {
    for list in [false, true] {
        let (mut doc, def, lookup, insert, inputs) = setup();
        let mut edited = draft(&doc, def, lookup);
        edited.expressions = ["(2,6)", "Open", "8", "Outside"]
            .map(str::to_owned)
            .to_vec();
        apply_table(&mut doc, def, lookup, edited).unwrap();
        let ObjectType::DynamicBlock(object) = doc.objects.get_mut(&inputs[0]).unwrap() else {
            panic!()
        };
        let DynamicBlockData::LinearParameter(p) = &mut object.data else {
            panic!()
        };
        p.value_set = codec::objects::BlockParameterValueSet {
            flags: if list { 8 } else { 4 },
            values: vec![2., 4., 6., 8.],
            increment: 2.,
            ..Default::default()
        };
        let selected = evaluate_reference_value(&doc, insert, lookup, "Open")
            .unwrap()
            .unwrap();
        assert_eq!(values(&selected, def, insert)[0], 4.);
        let ObjectType::DynamicBlock(object) = doc.objects.get_mut(&inputs[0]).unwrap() else {
            panic!()
        };
        let DynamicBlockData::LinearParameter(p) = &mut object.data else {
            panic!()
        };
        p.value_set = codec::objects::BlockParameterValueSet {
            flags: 3,
            minimum: 7.,
            maximum: 9.,
            ..Default::default()
        };
        let before = doc.clone();
        assert!(evaluate_reference_value(&doc, insert, lookup, "Open").is_err());
        assert!(doc == before);
    }
}

#[test]
fn block_lookup_matching_range_still_checks_changed_native_value_sets() {
    let (mut doc, def, lookup, insert, inputs) = setup();
    let mut edited = draft(&doc, def, lookup);
    edited.expressions = ["[2,6]", "Current", "8", "Other"]
        .map(str::to_owned)
        .to_vec();
    apply_table(&mut doc, def, lookup, edited).unwrap();
    let table = table(&doc, def, lookup).unwrap().unwrap().1;
    assert_eq!(
        reference_row_context(&doc, None, insert, def, &table),
        Some(0)
    );
    let ObjectType::DynamicBlock(object) = doc.objects.get_mut(&inputs[0]).unwrap() else {
        panic!()
    };
    let DynamicBlockData::LinearParameter(p) = &mut object.data else {
        panic!()
    };
    p.value_set = codec::objects::BlockParameterValueSet {
        flags: 3,
        minimum: 7.,
        maximum: 9.,
        ..Default::default()
    };
    let before = doc.clone();
    assert!(evaluate_reference_value(&doc, insert, lookup, "Current").is_err());
    assert!(doc == before);
}
