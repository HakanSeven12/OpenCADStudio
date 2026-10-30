use super::*;
mod multiple_outputs;
mod expressions;
mod rename;
use crate::scene::{block_basepoint, block_distance_parameter as distance, render_graph, Scene};
use codec::{
    entities::{Insert, Line},
    EntityType,
};

fn fixture() -> (Document, Handle) {
    let mut scene = Scene::new();
    scene.define_block_raw(
        "LOOKUP",
        Vector3::ZERO,
        vec![EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::UNIT_X,
        ))],
    );
    let definition = scene.document.block_records.get("LOOKUP").unwrap().handle;
    (scene.document, definition)
}
fn add_distance(document: &mut Document, definition: Handle) {
    distance::create(
        document,
        definition,
        distance::Kind::Linear,
        Vector3::ZERO,
        Vector3::new(2.0, 0.0, 0.0),
        Vector3::UNIT_Y,
    )
    .unwrap();
}
fn draft(document: &Document, definition: Handle, parameter: Handle) -> BlockLookupAction {
    let p = parameters(document, definition)
        .into_iter()
        .find(|(h, _)| *h == parameter)
        .unwrap()
        .1;
    let mut table = empty_table(p);
    table
        .columns
        .insert(0, properties(document, definition)[0].column.clone());
    table.column_count = 2;
    table.row_count = 2;
    table.expressions = vec!["2".into(), "Small".into(), "4".into(), "Large".into()];
    table
}
fn graph_data(document: &Document, definition: Handle) -> codec::objects::BlockEvaluationGraph {
    let ObjectType::DynamicBlock(o) =
        &document.objects[&graph_handle(document, definition).unwrap()]
    else {
        panic!()
    };
    let DynamicBlockData::EvaluationGraph(g) = &o.data else {
        panic!()
    };
    g.clone()
}
fn assert_links(document: &Document, definition: Handle) {
    let g = graph_data(document, definition);
    for n in &g.nodes {
        assert!(graph::valid_lists(&g, n.id), "invalid node {}", n.id);
    }
    for edge in g.edges.iter().filter(|e| e.next_id == 4) {
        let inverse = g
            .edges
            .iter()
            .find(|e| e.id == edge.outgoing_edges[4])
            .unwrap();
        assert_eq!(
            (
                inverse.source_node,
                inverse.destination_node,
                inverse.outgoing_edges[4]
            ),
            (edge.destination_node, edge.source_node, edge.id)
        );
    }
}
fn roundtrip(document: &Document, definition: Handle) {
    let graph = graph_handle(document, definition).unwrap();
    for extension in ["dwg", "dxf"] {
        let bytes = crate::io::save_to_bytes(document, extension, document.version).unwrap();
        let read = crate::io::load_bytes(&format!("lookup.{extension}"), bytes).unwrap();
        for (h, o) in &document.objects {
            if let ObjectType::DynamicBlock(o) = o {
                if o.owner == graph || o.handle == graph {
                    let ObjectType::DynamicBlock(actual) = &read.objects[h] else {
                        panic!()
                    };
                    assert_eq!(actual.data, o.data, "{extension}: {h}");
                    assert_eq!(actual.owner, o.owner);
                }
            }
        }
        assert_links(&read, definition);
        assert_eq!(
            block_basepoint::parameter(&read, definition),
            block_basepoint::parameter(document, definition)
        );
    }
}
#[test]
fn lookup_basepoint_native_roundtrip_rebinding_and_preservation() {
    let (mut document, definition) = fixture();
    add_distance(&mut document, definition);
    let original = document.clone();
    let base =
        block_basepoint::create(&mut document, definition, Vector3::new(3.0, 4.0, 0.0)).unwrap();
    let parameter = create(&mut document, definition, Vector3::UNIT_Y).unwrap();
    let edited = draft(&document, definition, parameter);
    let action = apply_table(&mut document, definition, parameter, edited).unwrap();
    assert_eq!(document.entities().count(), original.entities().count());
    assert_eq!(
        document.block_records.get("LOOKUP"),
        original.block_records.get("LOOKUP")
    );
    let gh = graph_handle(&document, definition).unwrap();
    for (h, o) in &original.objects {
        if *h != gh {
            assert_eq!(document.objects.get(h), Some(o), "preserved {h}");
        }
    }
    let first = graph_data(&document, definition);
    assert_eq!(first.nodes.len(), 10); // distance four, base one, lookup four, action one
    assert_eq!(first.edges.iter().filter(|e| e.next_id == 4).count(), 4);
    assert_links(&document, definition);
    let mut edited = table(&document, definition, parameter).unwrap().unwrap().1;
    edited.expressions[1] = "Compact".into();
    assert_eq!(
        apply_table(&mut document, definition, parameter, edited.clone()).unwrap(),
        action
    );
    assert_eq!(
        graph_data(&document, definition),
        first,
        "row edits preserve graph IDs and links"
    );
    add_distance(&mut document, definition);
    edited
        .columns
        .insert(1, properties(&document, definition)[1].column.clone());
    edited.column_count = 3;
    edited.expressions = vec![
        "2".into(),
        "2".into(),
        "Compact".into(),
        "4".into(),
        "2".into(),
        "Large".into(),
    ];
    apply_table(&mut document, definition, parameter, edited.clone()).unwrap();
    assert_links(&document, definition);
    assert_eq!(
        graph_data(&document, definition)
            .edges
            .iter()
            .filter(|e| e.next_id == 4)
            .count(),
        6
    );
    edited.columns.remove(0);
    edited.column_count = 2;
    edited.expressions = vec!["2".into(), "Compact".into(), "4".into(), "Large".into()];
    apply_table(&mut document, definition, parameter, edited).unwrap();
    assert_links(&document, definition);
    assert_eq!(
        graph_data(&document, definition)
            .edges
            .iter()
            .filter(|e| e.next_id == 4)
            .count(),
        4
    );
    assert_eq!(
        block_basepoint::create(&mut document, definition, Vector3::UNIT_X).unwrap(),
        base
    );
    roundtrip(&document, definition);
}
#[test]
fn lookup_audit_and_failed_binding_are_atomic() {
    let (mut document, definition) = fixture();
    add_distance(&mut document, definition);
    let parameter = create(&mut document, definition, Vector3::ZERO).unwrap();
    let good = draft(&document, definition, parameter);
    for (cell, value) in [(0, "NaN"), (0, "inf"), (0, ""), (2, "2.0"), (3, "small")] {
        let mut bad = good.clone();
        bad.expressions[cell] = value.into();
        let before = document.clone();
        assert!(apply_table(&mut document, definition, parameter, bad).is_err());
        assert_eq!(document, before);
    }
    let mut bad = good.clone();
    bad.columns[0].node_id = i32::MAX;
    let before = document.clone();
    assert!(apply_table(&mut document, definition, parameter, bad).is_err());
    assert_eq!(document, before);
    let mut forward_only = good;
    forward_only.columns[1].writable = false;
    forward_only.expressions[3] = "Small".into();
    assert!(validate(&forward_only, None).is_ok());
}

#[test]
fn lookup_malformed_dependencies_are_rejected_without_mutation() {
    let (mut document, definition) = fixture();
    add_distance(&mut document, definition);
    let parameter = create(&mut document, definition, Vector3::ZERO).unwrap();
    let edited = draft(&document, definition, parameter);
    apply_table(&mut document, definition, parameter, edited).unwrap();
    let mut edited = table(&document, definition, parameter).unwrap().unwrap().1;
    edited.expressions[1] = "Changed".into();
    let handle = graph_handle(&document, definition).unwrap();
    let ObjectType::DynamicBlock(object) = document.objects.get_mut(&handle).unwrap() else {
        panic!()
    };
    let DynamicBlockData::EvaluationGraph(g) = &mut object.data else {
        panic!()
    };
    g.edges
        .iter_mut()
        .find(|e| e.next_id == 4)
        .unwrap()
        .outgoing_edges[4] = -1;
    let before = document.clone();
    assert!(apply_table(&mut document, definition, parameter, edited).is_err());
    assert_eq!(document, before);
}

#[test]
fn lookup_reverse_selection_evaluates_saved_reference_geometry() {
    let (mut document, definition) = fixture();
    let line = document.block_records.get("LOOKUP").unwrap().entity_handles[0];
    let distance = distance::create(
        &mut document,
        definition,
        distance::Kind::Linear,
        Vector3::ZERO,
        Vector3::new(2., 0., 0.),
        Vector3::UNIT_Y,
    )
    .unwrap();
    crate::scene::block_actions::create(
        &mut document,
        definition,
        crate::scene::block_actions::Kind::Move,
        distance,
        &[line],
        glam::DVec3::ZERO,
    )
    .unwrap();
    let lookup = create(&mut document, definition, Vector3::ZERO).unwrap();
    let lookup_table = draft(&document, definition, lookup);
    apply_table(&mut document, definition, lookup, lookup_table).unwrap();
    let original_line = document.get_entity(line).unwrap().clone();
    let insert = document
        .add_entity(EntityType::Insert(Insert::new("LOOKUP", Vector3::ZERO)))
        .unwrap();

    let mut hidden = document.clone();
    let grips = crate::scene::block_grips::config::locations(&hidden, definition, lookup).unwrap().into_iter().map(|g| (g.role, g.handle, g.point, g.key_point, g.direction)).collect::<Vec<_>>();
    crate::scene::block_authoring::metadata::set_visible(&mut hidden, definition, lookup, false).unwrap();
    assert!(reference_property(&hidden, insert).is_none());
    assert_eq!(crate::scene::block_grips::config::locations(&hidden, definition, lookup).unwrap().into_iter().map(|g| (g.role, g.handle, g.point, g.key_point, g.direction)).collect::<Vec<_>>(), grips);
    let hidden_evaluated = evaluate_reference_choice(&hidden, insert, "Large").unwrap().unwrap();
    assert!(reference_property(&hidden_evaluated, insert).is_none());

    let initial = reference_property(&document, insert).unwrap();
    let crate::scene::model::object::PropValue::Choice { selected, options } = initial.value else {
        panic!("Lookup property should be a choice.")
    };
    assert_eq!(selected, "Small");
    assert_eq!(options, vec!["Small", "Large"]);

    let evaluated = evaluate_reference_choice(&document, insert, "Large")
        .unwrap()
        .unwrap();
    assert_eq!(document.get_entity(line), Some(&original_line));
    let EntityType::Insert(reference) = evaluated.get_entity(insert).unwrap() else {
        panic!()
    };
    let member = evaluated
        .block_records
        .get(&reference.block_name)
        .unwrap()
        .entity_handles[0];
    let EntityType::Line(line) = evaluated.get_entity(member).unwrap() else {
        panic!()
    };
    assert_eq!(line.start, Vector3::new(2., 0., 0.));
    assert_eq!(line.end, Vector3::new(3., 0., 0.));
    let selected = reference_property(&evaluated, insert).unwrap();
    let crate::scene::model::object::PropValue::Choice { selected, .. } = selected.value else {
        panic!()
    };
    assert_eq!(selected, "Large");
    assert!(
        evaluate_reference_value(&evaluated, insert, lookup, "Large")
            .unwrap()
            .is_none()
    );

    let mut unsupported = document.clone();
    let (_, mut malformed) = table(&unsupported, definition, lookup).unwrap().unwrap();
    malformed.columns[0].connection_name = "UpdatedAngle".into();
    let before = unsupported.clone();
    assert!(apply_table(&mut unsupported, definition, lookup, malformed.clone()).is_err());
    assert_eq!(unsupported, before);
    // Simulate a retained imported binding without authoring it through Apply.
    let (handle, _) = table(&unsupported, definition, lookup).unwrap().unwrap();
    let ObjectType::DynamicBlock(object) = unsupported.objects.get_mut(&handle).unwrap() else { panic!() };
    object.data = DynamicBlockData::LookupAction(malformed);
    let before = unsupported.clone();
    assert!(evaluate_reference_choice(&unsupported, insert, "Large").is_err());
    assert_eq!(unsupported, before);
}

#[test]
fn lookup_and_property_table_rows_preserve_changed_bases_in_drawing_and_test_contexts() {
    use crate::scene::{block_actions as actions, block_properties_table as table};
    use glam::DVec3;
    for testing in [false, true] {
        for lookup in [false, true] {
            let (mut document, definition) = fixture();
            add_distance(&mut document, definition);
            let p = actions::all_parameters(&document, definition).remove(0);
            let line = document.block_records.get("LOOKUP").unwrap().entity_handles[0];
            actions::create(&mut document, definition, actions::Kind::Move, p.handle, &[line], DVec3::ZERO).unwrap();
            if lookup {
                let lookup = create(&mut document, definition, Vector3::ZERO).unwrap();
                let draft = draft(&document, definition, lookup);
                apply_table(&mut document, definition, lookup, draft).unwrap();
            } else {
                let columns = table::columns(&document, definition);
                table::apply(&mut document, definition, &columns, &[vec![2.], vec![4.]], false).unwrap();
            }
            let insert = document.add_entity(EntityType::Insert(Insert::new("LOOKUP", Vector3::ZERO))).unwrap();
            let baseline = document.clone();
            let input = actions::Value::Frame { base: DVec3::new(-2., 0., 0.), end: p.end };
            document = if testing {
                actions::instance::evaluate_test_value(&document, &baseline, definition, insert, p.handle, input)
            } else { actions::instance::evaluate_value(&document, insert, p.handle, input) }.unwrap().unwrap();
            for row in [0, 1, 0] {
                document = if lookup {
                    let label = if row == 0 { "Small" } else { "Large" };
                    if testing { evaluate_test_choice(&document, &baseline, insert, label) }
                    else { evaluate_reference_choice(&document, insert, label) }
                } else {
                    let columns = table::table(&document, definition).unwrap().unwrap().columns;
                    if testing { actions::instance::evaluate_test_table_row(&document, &baseline, insert, &columns, row) }
                    else { actions::instance::evaluate_table_row(&document, insert, &columns, row) }
                }.unwrap().unwrap();
                let frame = actions::instance::frame(&document, insert, &p).unwrap();
                assert_eq!(frame.base, DVec3::new(-2., 0., 0.));
                assert_eq!(frame.end, DVec3::new(if row == 0 { 0. } else { 2. }, 0., 0.));
                let EntityType::Insert(reference) = document.get_entity(insert).unwrap() else { panic!() };
                let member = document.block_records.get(&reference.block_name).unwrap().entity_handles[0];
                let EntityType::Line(line) = document.get_entity(member).unwrap() else { panic!() };
                assert_eq!(line.start, Vector3::new(if row == 0 { -2. } else { 0. }, 0., 0.));
            }
        }
    }
}

#[test]
fn lookup_updated_angle_rotates_polar_parameter_and_materializes_action_geometry() {
    let (mut document, definition) = fixture();
    let line = document.block_records.get("LOOKUP").unwrap().entity_handles[0];
    let parameter = distance::create(
        &mut document,
        definition,
        distance::Kind::Polar,
        Vector3::ZERO,
        Vector3::new(2.0, 0.0, 0.0),
        Vector3::UNIT_Y,
    )
    .unwrap();
    crate::scene::block_actions::create(
        &mut document,
        definition,
        crate::scene::block_actions::Kind::Move,
        parameter,
        &[line],
        glam::DVec3::ZERO,
    )
    .unwrap();
    let lookup = create(&mut document, definition, Vector3::ZERO).unwrap();
    let lookup_parameter = parameters(&document, definition)
        .into_iter()
        .find(|(handle, _)| *handle == lookup)
        .unwrap()
        .1;
    let polar_properties = properties(&document, definition);
    assert_eq!(polar_properties.len(), 2);
    assert_eq!(polar_properties[1].column.connection_name, "UpdatedAngle");
    let mut table = empty_table(lookup_parameter);
    table.columns.insert(0, polar_properties[0].column.clone());
    table.columns.insert(1, polar_properties[1].column.clone());
    table.column_count = 3;
    table.row_count = 2;
    table.expressions = vec![
        "2".into(),
        "0".into(),
        "Small".into(),
        "4".into(),
        std::f64::consts::FRAC_PI_2.to_string(),
        "Large".into(),
    ];
    apply_table(&mut document, definition, lookup, table).unwrap();
    let insert = document
        .add_entity(EntityType::Insert(Insert::new("LOOKUP", Vector3::ZERO)))
        .unwrap();

    let evaluated = evaluate_reference_choice(&document, insert, "Large")
        .unwrap()
        .unwrap();
    let EntityType::Insert(reference) = evaluated.get_entity(insert).unwrap() else {
        panic!()
    };
    let member = evaluated
        .block_records
        .get(&reference.block_name)
        .unwrap()
        .entity_handles[0];
    let EntityType::Line(line) = evaluated.get_entity(member).unwrap() else {
        panic!()
    };
    assert!((line.start - Vector3::new(-2.0, 4.0, 0.0)).length() < 1e-9);
    assert!((line.end - Vector3::new(-1.0, 4.0, 0.0)).length() < 1e-9);
    assert!(evaluate_reference_choice(&evaluated, insert, "Large")
        .unwrap()
        .is_none());
}

#[test]
fn lookup_existing_expressions_survive_row_reordering_and_output_edits() {
    let (mut document, definition) = fixture();
    add_distance(&mut document, definition);
    let parameter = create(&mut document, definition, Vector3::ZERO).unwrap();
    let mut original = draft(&document, definition, parameter);
    original.expressions[0] = "[1,3]".into();
    original.expressions[2] = "[4,6]".into();
    let mut edited = original.clone();
    edited.expressions.rotate_left(2);
    edited.expressions[1] = "Renamed".into();
    assert!(validate(&edited, Some(&original)).is_ok());
    edited.expressions[0] = "[7,9]".into();
    assert!(validate(&edited, Some(&original)).is_ok());
    edited.expressions[0] = "{opaque}".into();
    assert!(validate(&edited, Some(&original)).is_err());
}
#[test]
fn lookup_basepoint_anchor_transform_and_reframe() {
    let (mut document, definition) = fixture();
    let original = document.clone();
    let point = Vector3::new(3.0, 4.0, 2.0);
    let base = block_basepoint::create(&mut document, definition, point).unwrap();
    let objects = document.objects.len();
    assert_eq!(
        block_basepoint::create(&mut document, definition, point).unwrap(),
        base
    );
    assert_eq!(document.objects.len(), objects);
    let mut insert = Insert::new("LOOKUP", Vector3::new(11.0, 13.0, 17.0));
    insert.rotation = 1.2;
    insert.set_x_scale(2.0);
    insert.set_y_scale(3.0);
    insert.set_z_scale(4.0);
    assert!(
        (render_graph::insert_transform(&document, &insert).apply(point) - insert.insert_point)
            .length()
            < 1e-9
    );
    assert_eq!(document.entities().count(), original.entities().count());
    for original in original.entities() {
        let EntityType::Line(before) = original else {
            panic!()
        };
        let EntityType::Line(after) = document.get_entity(before.common.handle).unwrap() else {
            panic!()
        };
        assert_eq!(
            (after.start, after.end, after.normal, after.thickness),
            (before.start, before.end, before.normal, before.thickness),
            "native definition tags may be added, geometry stays intact"
        );
    }
    add_distance(&mut document, definition);
    let parameter = create(&mut document, definition, Vector3::UNIT_Y).unwrap();
    let edited = draft(&document, definition, parameter);
    apply_table(&mut document, definition, parameter, edited).unwrap();
    let old = table(&document, definition, parameter).unwrap().unwrap().1;
    let transform = Transform::from_translation(Vector3::new(10.0, 20.0, 0.0));
    assert_eq!(
        block_basepoint::reframe(&mut document, definition, &transform),
        1
    );
    assert_eq!(reframe(&mut document, definition, &transform), 3);
    let new = table(&document, definition, parameter).unwrap().unwrap().1;
    assert_eq!(old.columns, new.columns);
    assert_eq!(old.expressions, new.expressions);
    assert_eq!(
        new.action.display_location,
        transform.apply(old.action.display_location)
    );
    assert_eq!(
        block_basepoint::parameter(&document, definition)
            .unwrap()
            .1
            .point,
        transform.apply(point)
    );
    let before = document.clone();
    assert!(
        block_basepoint::create(&mut document, definition, Vector3::new(f64::NAN, 0.0, 0.0))
            .is_err()
    );
    assert!(create(
        &mut document,
        definition,
        Vector3::new(0.0, f64::INFINITY, 0.0)
    )
    .is_err());
    assert_eq!(document, before);
}

#[test]
fn lookup_basepoint_existing_attributes_follow_anchor_and_editor_frame() {
    use codec::entities::{AttributeDefinition, AttributeEntity};
    let (document, definition) = fixture();
    let mut scene = Scene::new();
    scene.document = document;
    let local = Vector3::new(7.0, 9.0, 0.0);
    let mut attdef = AttributeDefinition::new("TAG".into(), "Prompt".into(), "Value".into());
    attdef.insertion_point = local;
    attdef.common.owner_handle = definition;
    scene
        .document
        .add_entity(EntityType::AttributeDefinition(attdef))
        .unwrap();
    let mut insert = Insert::new("LOOKUP", Vector3::new(10.0, 20.0, 0.0)).with_scale(2.0, 3.0, 1.0);
    insert.rotation = 0.5;
    let mut attribute = AttributeEntity::simple("TAG", "Value");
    attribute.insertion_point =
        render_graph::insert_transform(&scene.document, &insert).apply(local);
    insert.attributes.push(attribute);
    let handle = scene
        .document
        .add_entity(EntityType::Insert(insert))
        .unwrap();
    block_basepoint::create(&mut scene.document, definition, Vector3::new(3.0, 4.0, 0.0)).unwrap();
    let Some(EntityType::Insert(i)) = scene.document.get_entity(handle) else {
        panic!()
    };
    assert!(
        (i.attributes[0].insertion_point
            - render_graph::insert_transform(&scene.document, i).apply(local))
        .length()
            < 1e-9
    );
    let transform = Transform::from_translation(Vector3::new(-1.0, -2.0, 0.0));
    scene.reframe_block_definition(definition, &transform);
    let Some(EntityType::Insert(i)) = scene.document.get_entity(handle) else {
        panic!()
    };
    assert!(
        (i.attributes[0].insertion_point
            - render_graph::insert_transform(&scene.document, i).apply(transform.apply(local)))
        .length()
            < 1e-9
    );
}
