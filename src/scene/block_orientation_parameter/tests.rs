use super::*;
use crate::scene::{block_distance_parameter, block_point_parameter, block_visibility, Scene};
use codec::{entities::Line, EntityType};

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
    let handle = scene
        .document
        .block_records
        .get("PARAMETERS")
        .unwrap()
        .handle;
    (scene, handle)
}
fn append_batch(document: &mut Document, definition: Handle) {
    for kind in [Kind::Rotation, Kind::Alignment, Kind::Flip] {
        create(
            document,
            definition,
            kind,
            Vector3::ZERO,
            Vector3::new(0.0, 2.0, 0.0),
            if kind == Kind::Rotation {
                Vector3::new(2.0, 0.0, 0.0)
            } else {
                Vector3::new(3.0, 3.0, 0.0)
            },
        )
        .unwrap();
    }
}
fn graph_data(document: &Document, definition: Handle) -> &codec::objects::BlockEvaluationGraph {
    let ObjectType::DynamicBlock(o) =
        &document.objects[&graph_handle(document, definition).unwrap()]
    else {
        panic!()
    };
    let DynamicBlockData::EvaluationGraph(g) = &o.data else {
        panic!()
    };
    g
}
fn assert_roundtrips(document: &Document, definition: Handle, before: &Document) {
    let graph = graph_handle(document, definition).unwrap();
    if let Some(old_graph) = graph_handle(before, definition) {
        for (h, o) in &before.objects {
            if *h != old_graph {
                assert_eq!(document.objects.get(h), Some(o), "preserved {h}");
            }
        }
        let old = graph_data(before, definition);
        let new = graph_data(document, definition);
        assert_eq!(&new.nodes[..old.nodes.len()], &old.nodes);
        assert_eq!(&new.edges[..old.edges.len()], &old.edges);
    }
    let objects = document
        .objects
        .iter()
        .filter_map(|(h, o)| match o {
            ObjectType::DynamicBlock(o) if o.owner == graph && !before.objects.contains_key(h) => {
                Some((*h, o.data.clone()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(objects.len(), 13);
    let g = graph_data(document, definition);
    for node in g
        .nodes
        .iter()
        .filter(|n| objects.iter().any(|(h, _)| *h == n.expression))
    {
        let incoming = g
            .edges
            .iter()
            .filter(|e| e.destination_node == node.id)
            .map(|e| e.id)
            .collect::<Vec<_>>();
        let outgoing = g
            .edges
            .iter()
            .filter(|e| e.source_node == node.id)
            .map(|e| e.id)
            .collect::<Vec<_>>();
        assert_eq!(
            node.node_data,
            [
                incoming.first().copied().unwrap_or(-1),
                incoming.last().copied().unwrap_or(-1),
                outgoing.first().copied().unwrap_or(-1),
                outgoing.last().copied().unwrap_or(-1)
            ]
        );
    }
    for extension in ["dwg", "dxf"] {
        let bytes = crate::io::save_to_bytes(document, extension, document.version).unwrap();
        let read = crate::io::load_bytes(&format!("orientation.{extension}"), bytes).unwrap();
        for (h, data) in &objects {
            let ObjectType::DynamicBlock(object) = &read.objects[h] else {
                panic!("{extension}: native {h}")
            };
            assert_eq!(&object.data, data, "{extension}: {h}");
            assert_eq!(object.owner, graph);
        }
        assert!(authoring::graph(&read, definition).is_ok());
    }
}
#[test]
fn orientation_parameter_native_batch_coexists_and_roundtrips_complete_objects() {
    let (mut scene, definition) = fixture();
    block_point_parameter::create(
        &mut scene.document,
        definition,
        Vector3::ZERO,
        Vector3::UNIT_X,
    )
    .unwrap();
    block_visibility::create(&mut scene.document, definition, Vector3::ZERO).unwrap();
    block_distance_parameter::create(
        &mut scene.document,
        definition,
        block_distance_parameter::Kind::Polar,
        Vector3::ZERO,
        Vector3::UNIT_Y,
        Vector3::UNIT_X,
    )
    .unwrap();
    let before = scene.document.clone();
    append_batch(&mut scene.document, definition);
    assert_roundtrips(&scene.document, definition, &before);
    let parameters = parameters(&scene.document, definition);
    assert_eq!(parameters[0].label, "Angle2", "share labels with Polar");
    for p in &parameters {
        let id = p.native.parameter.element.eval.node_id;
        assert_eq!(
            p.native.property_states,
            if p.kind == Kind::Rotation {
                [0, id + 1, 0, 0]
            } else {
                [id + 1, 0, 0, 0]
            }
        );
    }
    let flip = parameters.iter().find(|p| p.kind == Kind::Flip).unwrap();
    let id = flip.native.parameter.element.eval.node_id;
    assert!(scene.document.objects.values().any(|o|matches!(o,ObjectType::DynamicBlock(o) if matches!(&o.data,DynamicBlockData::FlipGrip(g) if g.combined_state==id+2&&g.grip.flags_91==id+3&&g.grip.flags_92==id+4))));
    assert!(scene.document.objects.values().any(|o|matches!(o,ObjectType::DynamicBlock(o) if matches!(&o.data,DynamicBlockData::GripLocationComponent(c) if c.expression=="UpdatedFlip"&&c.grip_type==id&&c.eval.value_code==70&&c.eval.value==BlockEvalValue::Short(0)))));
}
#[test]
fn orientation_parameter_rotation_and_polar_share_unique_labels_in_both_orders() {
    for rotation_first in [false, true] {
        let (mut scene, definition) = fixture();
        for rotation in [rotation_first, !rotation_first] {
            if rotation {
                create(
                    &mut scene.document,
                    definition,
                    Kind::Rotation,
                    Vector3::ZERO,
                    Vector3::UNIT_Y,
                    Vector3::UNIT_X,
                )
                .unwrap();
            } else {
                block_distance_parameter::create(
                    &mut scene.document,
                    definition,
                    block_distance_parameter::Kind::Polar,
                    Vector3::ZERO,
                    Vector3::UNIT_Y,
                    Vector3::UNIT_X,
                )
                .unwrap();
            }
        }
        let rotation = parameters(&scene.document, definition)[0].label.clone();
        let polar =
            block_distance_parameter::parameters(&scene.document, definition)[0].labels[1].clone();
        assert_ne!(rotation, polar);
    }
}

#[test]
fn orientation_parameter_flip_capacity_and_invalid_points_leave_document_intact() {
    let (mut scene, definition) = fixture();
    for kind in [Kind::Rotation, Kind::Alignment, Kind::Flip] {
        for end in [
            Vector3::ZERO,
            Vector3::new(f64::NAN, 1.0, 0.0),
            Vector3::new(1.0, 1.0, 1.0),
        ] {
            let before = scene.document.clone();
            assert!(create(
                &mut scene.document,
                definition,
                kind,
                Vector3::ZERO,
                end,
                Vector3::UNIT_X
            )
            .is_err());
            assert_eq!(scene.document, before);
        }
    }
    append_batch(&mut scene.document, definition);
    let graph = graph_handle(&scene.document, definition).unwrap();
    if let ObjectType::DynamicBlock(o) = scene.document.objects.get_mut(&graph).unwrap() {
        if let DynamicBlockData::EvaluationGraph(g) = &mut o.data {
            g.first_node_id = i32::MAX - 4;
        }
    }
    let before = scene.document.clone();
    assert!(placement_error(&scene.document, definition, Kind::Flip).is_some());
    assert!(create(
        &mut scene.document,
        definition,
        Kind::Flip,
        Vector3::ZERO,
        Vector3::UNIT_X,
        Vector3::ZERO
    )
    .is_err());
    assert_eq!(scene.document, before);
    assert!(placement_error(&scene.document, definition, Kind::Rotation).is_none());
    create(
        &mut scene.document,
        definition,
        Kind::Rotation,
        Vector3::ZERO,
        Vector3::UNIT_Y,
        Vector3::UNIT_X,
    )
    .unwrap();
    assert_eq!(
        graph_data(&scene.document, definition).first_node_id,
        i32::MAX
    );
}
#[test]
fn orientation_parameter_reframe_and_editor_extents_preserve_native_points() {
    let (mut scene, definition) = fixture();
    append_batch(&mut scene.document, definition);
    let original = parameters(&scene.document, definition)
        .iter()
        .map(|p| {
            (
                p.native.definition_base_point,
                p.native.definition_end_point,
                p.reference,
            )
        })
        .collect::<Vec<_>>();
    let transform = Transform::from_translation(Vector3::new(10.0, 20.0, 0.0));
    assert_eq!(reframe(&mut scene.document, definition, &transform), 6);
    for (p, (base, end, reference)) in parameters(&scene.document, definition).iter().zip(original)
    {
        assert_eq!(p.native.definition_base_point, transform.apply(base));
        assert_eq!(p.native.definition_end_point, transform.apply(end));
        assert_eq!(p.reference, transform.apply(reference));
        assert!(graphic(p)
            .lines
            .into_iter()
            .flatten()
            .all(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite()));
    }
    scene.block_edit_block = Some(definition);
    let (_, max) = scene.visible_block_bounds(definition).unwrap();
    assert!(max.x >= 13.0 && max.y >= 23.0);
    scene.block_edit_block = None;
    let (_, max) = scene.visible_block_bounds(definition).unwrap();
    assert!(max.x <= 1.0 && max.y < 1.0);
}
#[test]
fn orientation_parameter_rotation_270_graphic_retains_counterclockwise_arc() {
    let (mut scene, definition) = fixture();
    create(
        &mut scene.document,
        definition,
        Kind::Rotation,
        Vector3::ZERO,
        -Vector3::UNIT_Y,
        Vector3::UNIT_X,
    )
    .unwrap();
    let g = graphic(&parameters(&scene.document, definition)[0]);
    assert!(g.lines.iter().flatten().any(|p| p.x < -0.99));
}
#[test]
#[ignore = "requires OCS_DYNAMIC_BLOCK_FIXTURES with local DWG fixtures"]
fn orientation_parameter_fixture_batch_roundtrips() {
    use sha2::{Digest, Sha256};
    let directory = std::env::var_os("OCS_DYNAMIC_BLOCK_FIXTURES").expect("fixture directory");
    let mut paths = std::fs::read_dir(directory)
        .unwrap()
        .map(|p| p.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("dwg")))
        .collect::<Vec<_>>();
    paths.sort();
    assert_eq!(paths.len(), 17);
    for path in paths {
        let bytes = std::fs::read(&path).unwrap();
        let digest = Sha256::digest(&bytes);
        let filename = path.file_name().unwrap().to_string_lossy();
        let mut document = crate::io::load_bytes(&filename, bytes).unwrap();
        let definition = document
            .block_records
            .iter()
            .map(|b| b.handle)
            .find(|h| {
                authoring::graph(&document, *h).is_ok_and(|g| g.is_some())
                    && placement_error(&document, *h, Kind::Flip).is_none()
            })
            .unwrap();
        let before = document.clone();
        append_batch(&mut document, definition);
        assert_roundtrips(&document, definition, &before);
        assert_eq!(Sha256::digest(std::fs::read(&path).unwrap()), digest);
        eprintln!("{filename}: Rotation/Alignment/Flip native DWG/DXF objects and graph append passed; original graph/objects/checksum preserved");
    }
}
