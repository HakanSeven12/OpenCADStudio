use super::expressions::{setup, values};
use super::*;
use crate::scene::block_authoring::metadata::{self, Field};

#[test]
fn block_lookup_input_rename_preserves_formula_tokens_and_reference_behavior() {
    let (mut doc, def, lookup, insert, inputs) = setup();
    let mut edited = draft(&doc, def, lookup);
    edited
        .columns
        .insert(1, properties(&doc, def)[1].column.clone());
    edited.column_count = 3;
    edited.expressions = [
        " 2 ",
        " =max(Distance1, 1e3) + (Distance1) ",
        "Distance1 literal",
        "4",
        "Distance1*2",
        "Other",
    ]
    .map(str::to_owned)
    .to_vec();
    apply_table(&mut doc, def, lookup, edited).unwrap();
    let graph_before = graph_data(&doc, def);
    for name in [
        "Panel width",
        "max",
        "Façade — مسافة — 距離",
        "A`B,(x)",
        "Width",
    ] {
        metadata::set_text(&mut doc, def, inputs[0], Field::Name, name).unwrap();
        let t = table(&doc, def, lookup).unwrap().unwrap().1;
        assert_eq!(t.expressions[2], "Distance1 literal");
        assert!(t.expressions[1].starts_with(" =max("));
        assert!(t.expressions[1].contains("1e3"));
        let selected = evaluate_reference_value(&doc, insert, lookup, "Other")
            .unwrap()
            .unwrap();
        assert_eq!(values(&selected, def, insert), [4., 8.]);
        assert_eq!(graph_data(&doc, def), graph_before);
    }
    let t = table(&doc, def, lookup).unwrap().unwrap().1;
    assert_eq!(t.expressions[1], " =max(`Width`, 1e3) + (`Width`) ");
    for extension in ["dwg", "dxf"] {
        let bytes = crate::io::save_to_bytes(&doc, extension, doc.version).unwrap();
        let loaded = crate::io::load_bytes(&format!("lookup-rename.{extension}"), bytes).unwrap();
        assert_eq!(
            table(&loaded, def, lookup).unwrap().unwrap().1.expressions,
            t.expressions
        );
        let selected = evaluate_reference_value(&loaded, insert, lookup, "Other")
            .unwrap()
            .unwrap();
        assert_eq!(values(&selected, def, insert), [4., 8.]);
    }
    let before = doc.clone();
    assert!(metadata::set_text(&mut doc, def, inputs[0], Field::Name, "Distance2").is_err());
    assert!(doc == before);
    assert!(metadata::set_text(&mut doc, def, inputs[0], Field::Name, &"W".repeat(5000)).is_err());
    assert!(doc == before);
}

#[test]
fn block_lookup_rename_preserves_opaque_rows_and_other_definition_tables() {
    let (mut doc, def, lookup, _, inputs) = setup();
    let mut edited = draft(&doc, def, lookup);
    edited
        .columns
        .insert(1, properties(&doc, def)[1].column.clone());
    edited.column_count = 3;
    edited.expressions = ["2", "Distance1+1", "First", "4", "Distance1*2", "Second"]
        .map(str::to_owned)
        .to_vec();
    let handle = apply_table(&mut doc, def, lookup, edited).unwrap();
    let ObjectType::DynamicBlock(object) = doc.objects.get_mut(&handle).unwrap() else {
        panic!()
    };
    let DynamicBlockData::LookupAction(t) = &mut object.data else {
        panic!()
    };
    t.expressions[4] = "foreign(Distance1)".into();
    let opaque = t.expressions[4].clone();
    let mut scene = Scene::new();
    scene.document = doc;
    scene.define_block_raw(
        "OTHER",
        Vector3::ZERO,
        vec![EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::UNIT_X,
        ))],
    );
    doc = scene.document;
    let other_def = doc.block_records.get("OTHER").unwrap().handle;
    add_distance(&mut doc, other_def);
    distance::create(
        &mut doc,
        other_def,
        distance::Kind::Linear,
        Vector3::ZERO,
        Vector3::new(3., 0., 0.),
        Vector3::UNIT_Y,
    )
    .unwrap();
    let other_lookup = create(&mut doc, other_def, Vector3::ZERO).unwrap();
    let mut other_table = draft(&doc, other_def, other_lookup);
    other_table
        .columns
        .insert(1, properties(&doc, other_def)[1].column.clone());
    other_table.column_count = 3;
    other_table.expressions = ["2", "Distance1+10", "First", "4", "Distance1+10", "Second"]
        .map(str::to_owned)
        .to_vec();
    apply_table(&mut doc, other_def, other_lookup, other_table.clone()).unwrap();
    metadata::set_text(&mut doc, def, inputs[0], Field::Name, "New width").unwrap();
    let t = table(&doc, def, lookup).unwrap().unwrap().1;
    assert_eq!(t.expressions[1], "`New width`+1");
    assert_eq!(t.expressions[4], opaque);
    assert_eq!(
        table(&doc, other_def, other_lookup)
            .unwrap()
            .unwrap()
            .1
            .expressions,
        other_table.expressions
    );
}

#[test]
fn block_lookup_secondary_angle_rename_rewrites_only_the_owned_input_symbol() {
    let (mut doc, def) = fixture();
    let polar = distance::create(
        &mut doc,
        def,
        distance::Kind::Polar,
        Vector3::ZERO,
        Vector3::new(2., 0., 0.),
        Vector3::UNIT_Y,
    )
    .unwrap();
    let lookup = create(&mut doc, def, Vector3::ZERO).unwrap();
    let mut edited = draft(&doc, def, lookup);
    edited
        .columns
        .insert(1, properties(&doc, def)[1].column.clone());
    edited.column_count = 3;
    edited.expressions = ["Angle1 + 2", "0", "First", "Angle1 + 4", "1", "Second"]
        .map(str::to_owned)
        .to_vec();
    apply_table(&mut doc, def, lookup, edited).unwrap();
    metadata::set_text(&mut doc, def, polar, Field::SecondaryName, "角度 a").unwrap();
    let t = table(&doc, def, lookup).unwrap().unwrap().1;
    assert_eq!(t.expressions[0], "`角度 a` + 2");
    assert_eq!(t.expressions[3], "`角度 a` + 4");
    let insert = doc
        .add_entity(EntityType::Insert(Insert::new("LOOKUP", Vector3::ZERO)))
        .unwrap();
    let selected = evaluate_reference_value(&doc, insert, lookup, "Second")
        .unwrap()
        .unwrap();
    assert!((values(&selected, def, insert)[0] - 5.).abs() < 1e-8);
}
