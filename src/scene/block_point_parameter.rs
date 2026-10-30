//! Native Point parameter authoring. Action evaluation is a separate service.
use super::block_authoring::{self as authoring, *};
use codec::objects::{
    BlockConnection, BlockElement, BlockEvalValue, BlockGrip, BlockGripExpression,
    BlockOnePointParameter, BlockParameter, BlockParameterProperty, BlockPointParameter,
    DynamicBlockData, ObjectType,
};
use codec::types::{Transform, Vector3};
use codec::{Document, Handle};

fn graph_handle(document: &Document, definition: Handle) -> Option<Handle> {
    match document
        .objects
        .get(&super::block_authoring::definition_dictionary(document, definition)?)?
    {
        ObjectType::Dictionary(dictionary) => dictionary.get("ACAD_ENHANCEDBLOCK"),
        _ => None,
    }
}

pub(crate) fn parameters(
    document: &Document,
    definition: Handle,
) -> Vec<(Handle, &BlockPointParameter)> {
    let Some(graph) = graph_handle(document, definition) else {
        return Vec::new();
    };
    let Some(ObjectType::DynamicBlock(object)) = document.objects.get(&graph) else {
        return Vec::new();
    };
    let DynamicBlockData::EvaluationGraph(graph_data) = &object.data else {
        return Vec::new();
    };
    graph_data
        .nodes
        .iter()
        .filter_map(|node| match document.objects.get(&node.expression) {
            Some(ObjectType::DynamicBlock(object)) if object.owner == graph => match &object.data {
                DynamicBlockData::PointParameter(parameter) => Some((object.handle, parameter)),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

pub(crate) fn placement_error(document: &Document, definition: Handle) -> Option<&'static str> {
    authoring::graph(document, definition).err()
}

pub(crate) fn create(
    document: &mut Document,
    definition: Handle,
    location: Vector3,
    label: Vector3,
) -> Result<Handle, &'static str> {
    if ![
        location.x, location.y, location.z, label.x, label.y, label.z,
    ]
    .iter()
    .all(|value| value.is_finite())
    {
        return Err("Specify finite parameter and label locations.");
    }
    let (graph, slots) = authoring::prepare(document, definition)?;
    register_classes(
        document,
        &[
            ("BLOCKPOINTPARAMETER", "AcDbBlockPointParameter", 55),
            ("BLOCKXYGRIP", "AcDbBlockXYGrip", 55),
            ("BLOCKGRIPLOCATIONCOMPONENT", "AcDbBlockGripExpr", 20),
        ],
    );
    let existing = parameters(document, definition);
    let name = (1..)
        .map(|number| format!("Position{number}"))
        .find(|name| {
            existing
                .iter()
                .all(|(_, parameter)| !parameter.position_name.eq_ignore_ascii_case(name))
        })
        .unwrap();
    let [parameter, grip, x, y] = [(); 4].map(|_| document.allocate_handle());
    let element = |offset, name: &str| BlockElement {
        eval: expression(slots.eval + offset, NO_VALUE, BlockEvalValue::None),
        name: name.to_string(),
        major: MAJOR,
        minor: MINOR,
        eed_1071: 0,
    };
    let native = BlockPointParameter {
        parameter: BlockOnePointParameter {
            parameter: BlockParameter {
                element: element(0, "Point"),
                show_properties: true,
                chain_actions: false,
            },
            definition_point: location,
            properties: ["DisplacementX", "DisplacementY"].map(|name| BlockParameterProperty {
                connections: vec![BlockConnection {
                    code: slots.eval + 1,
                    name: name.into(),
                }],
            }),
            // Native 93 links the one-point parameter to its grip expression ID.
            property_count: slots.eval + 1,
        },
        position_name: name,
        position_description: String::new(),
        definition_label_point: label,
    };
    document.objects.insert(
        parameter,
        ObjectType::DynamicBlock(dynamic(
            parameter,
            graph,
            "BLOCKPOINTPARAMETER",
            "AcDbBlockPointParameter",
            DynamicBlockData::PointParameter(native),
        )),
    );
    document.objects.insert(
        grip,
        ObjectType::DynamicBlock(dynamic(
            grip,
            graph,
            "BLOCKXYGRIP",
            "AcDbBlockXYGrip",
            DynamicBlockData::XYGrip(BlockGrip {
                element: element(1, "Grip"),
                flags_91: slots.eval + 2,
                flags_92: slots.eval + 3,
                location,
                insert_cycling: true,
                insert_cycling_weight: -1,
            }),
        )),
    );
    for (handle, offset, axis) in [(x, 2, "UpdatedX"), (y, 3, "UpdatedY")] {
        document.objects.insert(
            handle,
            ObjectType::DynamicBlock(dynamic(
                handle,
                graph,
                "BLOCKGRIPLOCATIONCOMPONENT",
                "AcDbBlockGripExpr",
                DynamicBlockData::GripLocationComponent(BlockGripExpression {
                    eval: expression(slots.eval + offset, 40, BlockEvalValue::Real(0.0)),
                    grip_type: slots.eval,
                    expression: axis.into(),
                }),
            )),
        );
    }
    authoring::append(document, graph, slots, [parameter, grip, x, y]);
    Ok(parameter)
}

pub(crate) fn reframe(
    document: &mut Document,
    definition: Handle,
    transform: &Transform,
) -> usize {
    let Some(graph) = graph_handle(document, definition) else {
        return 0;
    };
    let owned = parameters(document, definition)
        .iter()
        .map(|(handle, parameter)| (*handle, parameter.parameter.property_count))
        .collect::<Vec<_>>();
    let mut changed = 0;
    for (handle, grip_id) in owned {
        if let Some(ObjectType::DynamicBlock(object)) = document.objects.get_mut(&handle) {
            if let DynamicBlockData::PointParameter(parameter) = &mut object.data {
                super::block_actions::reframe_value(&mut parameter.parameter.parameter.element.eval, parameter.parameter.definition_point.z, transform);
                parameter.parameter.definition_point =
                    transform.apply(parameter.parameter.definition_point);
                parameter.definition_label_point =
                    transform.apply(parameter.definition_label_point);
                changed += 1;
            }
        }
        for object in document.objects.values_mut() {
            if let ObjectType::DynamicBlock(object) = object {
                if object.owner == graph {
                    if let DynamicBlockData::XYGrip(grip) = &mut object.data {
                        if grip.element.eval.node_id == grip_id {
                            grip.location = transform.apply(grip.location);
                            changed += 1;
                        }
                    }
                }
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{block_visibility, Scene};
    use codec::{entities::Line, EntityType};

    #[test]
    #[ignore = "requires OCS_DYNAMIC_BLOCK_FIXTURES with local DWG fixtures"]
    fn point_parameter_fixture_graph_append_roundtrips() {
        use sha2::{Digest, Sha256};
        let directory = std::env::var_os("OCS_DYNAMIC_BLOCK_FIXTURES").expect("fixture directory");
        let mut paths = std::fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("dwg"))
            })
            .collect::<Vec<_>>();
        paths.sort();
        assert!(!paths.is_empty());
        for path in paths {
            let bytes = std::fs::read(&path).unwrap();
            let digest = Sha256::digest(&bytes);
            let filename = path.file_name().unwrap().to_string_lossy();
            let mut scene = Scene::new();
            scene.document = crate::io::load_bytes(&filename, bytes).unwrap();
            let definitions = scene
                .document
                .block_records
                .iter()
                .filter(|record| graph_handle(&scene.document, record.handle).is_some())
                .map(|record| record.handle)
                .collect::<Vec<_>>();
            let existing = !definitions.is_empty();
            let definition = if existing {
                definitions
                    .iter()
                    .copied()
                    .find(|definition| authoring::graph(&scene.document, *definition).is_ok())
                    .unwrap_or_else(|| {
                        panic!(
                            "{filename}: no editable decoded graph: {:?}",
                            definitions
                                .iter()
                                .map(|definition| authoring::graph(&scene.document, *definition))
                                .collect::<Vec<_>>()
                        )
                    })
            } else {
                scene.define_block_raw(
                    "POINT_PARAMETER_TEST",
                    Vector3::ZERO,
                    vec![EntityType::Line(Line::from_points(
                        Vector3::ZERO,
                        Vector3::UNIT_X,
                    ))],
                );
                scene
                    .document
                    .block_records
                    .get("POINT_PARAMETER_TEST")
                    .unwrap()
                    .handle
            };
            let before = scene.document.clone();
            let graph = graph_handle(&before, definition);
            let old_nodes = existing.then(|| graph_data(&before, definition).nodes.clone());
            let location = Vector3::new(3.25, -4.5, 2.0);
            let label = Vector3::new(6.0, 7.0, 2.0);
            let handle = create(&mut scene.document, definition, location, label).unwrap();
            let name = parameters(&scene.document, definition)
                .iter()
                .find(|(found, _)| *found == handle)
                .unwrap()
                .1
                .position_name
                .clone();
            for (handle, object) in &before.objects {
                if Some(*handle) != graph {
                    assert_eq!(
                        scene.document.objects.get(handle),
                        Some(object),
                        "{filename}: preserved {handle}"
                    );
                }
            }
            if let Some(nodes) = old_nodes {
                assert_eq!(
                    &graph_data(&scene.document, definition).nodes[..nodes.len()],
                    &nodes
                );
            }
            for extension in ["dwg", "dxf"] {
                let bytes =
                    crate::io::save_to_bytes(&scene.document, extension, scene.document.version)
                        .unwrap();
                let read = crate::io::load_bytes(&format!("point.{extension}"), bytes).unwrap();
                let points = parameters(&read, definition);
                let point = points
                    .iter()
                    .find(|(_, point)| point.position_name == name)
                    .unwrap_or_else(|| panic!("{filename}: missing {name} in {extension}"))
                    .1;
                assert_eq!(
                    point.parameter.definition_point, location,
                    "{filename}: {extension}"
                );
                assert_eq!(
                    point.definition_label_point, label,
                    "{filename}: {extension}"
                );
                assert!(
                    authoring::graph(&read, definition).is_ok(),
                    "{filename}: graph in {extension}"
                );
            }
            assert_eq!(Sha256::digest(std::fs::read(&path).unwrap()), digest);
            eprintln!("{filename}: Point DWG/DXF roundtrips; appended to existing graph={existing}; original objects/checksum preserved");
        }
    }

    fn fixture() -> (Scene, Handle) {
        let mut scene = Scene::new();
        scene.define_block_raw(
            "PARAMETERS",
            Vector3::ZERO,
            vec![EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::UNIT_X,
            ))],
        );
        let definition = scene
            .document
            .block_records
            .get("PARAMETERS")
            .unwrap()
            .handle;
        (scene, definition)
    }

    fn graph_data(
        document: &Document,
        definition: Handle,
    ) -> &codec::objects::BlockEvaluationGraph {
        let ObjectType::DynamicBlock(object) =
            &document.objects[&graph_handle(document, definition).unwrap()]
        else {
            panic!("native graph");
        };
        let DynamicBlockData::EvaluationGraph(graph) = &object.data else {
            panic!("graph data");
        };
        graph
    }

    #[test]
    fn point_parameter_appends_to_visibility_and_keeps_existing_objects() {
        for visibility_first in [false, true] {
            let (mut scene, definition) = fixture();
            if visibility_first {
                block_visibility::create(&mut scene.document, definition, Vector3::ZERO).unwrap();
            } else {
                create(
                    &mut scene.document,
                    definition,
                    Vector3::ZERO,
                    Vector3::UNIT_X,
                )
                .unwrap();
            }
            let before = scene.document.clone();
            let graph = graph_handle(&before, definition).unwrap();
            let nodes = graph_data(&before, definition).nodes.clone();
            let edges = graph_data(&before, definition).edges.clone();
            create(
                &mut scene.document,
                definition,
                Vector3::new(2.0, 3.0, 4.0),
                Vector3::new(5.0, 6.0, 4.0),
            )
            .unwrap();
            if !visibility_first {
                block_visibility::create(&mut scene.document, definition, Vector3::UNIT_Y).unwrap();
            }
            for (handle, object) in &before.objects {
                if *handle != graph {
                    assert_eq!(
                        scene.document.objects.get(handle),
                        Some(object),
                        "preserved {handle}"
                    );
                }
            }
            let after = graph_data(&scene.document, definition);
            assert_eq!(&after.nodes[..nodes.len()], &nodes);
            assert_eq!(&after.edges[..edges.len()], &edges);
            assert!(block_visibility::parameter(&scene.document, definition).is_some());
            assert_eq!(
                parameters(&scene.document, definition).len(),
                if visibility_first { 1 } else { 2 }
            );
            assert!(authoring::graph(&scene.document, definition).is_ok());
        }
    }

    #[test]
    fn point_parameter_native_fields_and_shared_graph_roundtrip() {
        let (mut scene, definition) = fixture();
        let location = Vector3::new(3.25, -4.5, 6.0);
        let label = Vector3::new(7.0, 8.0, 6.0);
        create(&mut scene.document, definition, location, label).unwrap();
        create(
            &mut scene.document,
            definition,
            Vector3::ZERO,
            Vector3::UNIT_X,
        )
        .unwrap();
        block_visibility::create(&mut scene.document, definition, Vector3::ZERO).unwrap();
        for extension in ["dwg", "dxf"] {
            let bytes =
                crate::io::save_to_bytes(&scene.document, extension, scene.document.version)
                    .unwrap();
            let document =
                crate::io::load_bytes(&format!("parameters.{extension}"), bytes).unwrap();
            let found = parameters(&document, definition);
            assert_eq!(found.len(), 2, "{extension}");
            assert_eq!(found[0].1.position_name, "Position1");
            assert_eq!(found[1].1.position_name, "Position2");
            assert_eq!(found[0].1.parameter.definition_point, location);
            assert_eq!(found[0].1.definition_label_point, label);
            assert_eq!(
                found[0].1.parameter.properties[0].connections[0].name,
                "DisplacementX"
            );
            assert!(block_visibility::parameter(&document, definition).is_some());
            assert!(authoring::graph(&document, definition).is_ok());
            let point = found[0].1;
            let grip = document
                .objects
                .values()
                .find_map(|object| match object {
                    ObjectType::DynamicBlock(object) => match &object.data {
                        DynamicBlockData::XYGrip(grip)
                            if grip.element.eval.node_id == point.parameter.property_count =>
                        {
                            Some(grip)
                        }
                        _ => None,
                    },
                    _ => None,
                })
                .expect("native point grip");
            assert_eq!(grip.location, location);
        }
    }

    #[test]
    fn point_parameter_reframe_moves_position_label_and_own_grip() {
        let (mut scene, definition) = fixture();
        create(
            &mut scene.document,
            definition,
            Vector3::new(3.0, 4.0, 5.0),
            Vector3::new(6.0, 7.0, 5.0),
        )
        .unwrap();
        let transform = Transform::from_translation(Vector3::new(-1.0, -2.0, -3.0));
        assert_eq!(reframe(&mut scene.document, definition, &transform), 2);
        let point = parameters(&scene.document, definition)[0].1;
        assert_eq!(
            point.parameter.definition_point,
            Vector3::new(2.0, 2.0, 2.0)
        );
        assert_eq!(point.definition_label_point, Vector3::new(5.0, 5.0, 2.0));
        let grip = scene
            .document
            .objects
            .values()
            .find_map(|object| match object {
                ObjectType::DynamicBlock(object) => match &object.data {
                    DynamicBlockData::XYGrip(grip) => Some(grip),
                    _ => None,
                },
                _ => None,
            })
            .unwrap();
        assert_eq!(grip.location, point.parameter.definition_point);
    }

    #[test]
    fn point_parameter_positions_contribute_to_editor_extents_only() {
        let (mut scene, definition) = fixture();
        create(
            &mut scene.document,
            definition,
            Vector3::new(30.0, 40.0, 0.0),
            Vector3::new(60.0, 70.0, 0.0),
        )
        .unwrap();
        scene.block_edit_block = Some(definition);
        scene.bump_geometry();
        let (_, max) = scene.visible_block_bounds(definition).unwrap();
        assert!(max.x >= 60.0 && max.y >= 70.0);
        scene.block_edit_block = None;
        scene.bump_geometry();
        let (_, max) = scene.visible_block_bounds(definition).unwrap();
        assert!(
            max.x <= 1.0 && max.y < 1.0,
            "authoring labels are not block reference geometry"
        );
    }

    #[test]
    fn point_parameter_rejects_invalid_graph_and_locations_without_mutation() {
        let (mut scene, definition) = fixture();
        let before = scene.document.clone();
        assert!(create(
            &mut scene.document,
            definition,
            Vector3::new(f64::NAN, 0.0, 0.0),
            Vector3::ZERO
        )
        .is_err());
        assert_eq!(scene.document, before);
        create(
            &mut scene.document,
            definition,
            Vector3::ZERO,
            Vector3::UNIT_X,
        )
        .unwrap();
        let handle = graph_handle(&scene.document, definition).unwrap();
        if let Some(ObjectType::DynamicBlock(object)) = scene.document.objects.get_mut(&handle) {
            if let DynamicBlockData::EvaluationGraph(graph) = &mut object.data {
                graph.first_node_id = i32::MAX;
            }
        }
        let before = scene.document.clone();
        assert!(create(
            &mut scene.document,
            definition,
            Vector3::ZERO,
            Vector3::ZERO
        )
        .is_err());
        assert_eq!(scene.document, before);
        assert!(block_visibility::create(&mut scene.document, definition, Vector3::ZERO).is_none());
        assert_eq!(scene.document, before);
    }
}
