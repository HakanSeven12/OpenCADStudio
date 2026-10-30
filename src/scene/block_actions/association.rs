//! Reconnect orphaned native actions without replacing their identities.
use super::*;

pub(crate) fn orphaned(doc: &Document, definition: Handle) -> Vec<Handle> {
    actions(doc, definition)
        .into_iter()
        .filter(|(_, _, kind, id)| {
            !parameters(doc, definition, *kind)
                .iter()
                .any(|p| p.id == *id && compatible(p, *kind))
        })
        .map(|(handle, _, _, _)| handle)
        .collect()
}

pub(crate) fn associate(
    doc: &mut Document,
    definition: Handle,
    action: Handle,
    parameter: Handle,
    key_point: Option<XyKeyPoint>,
) -> Result<bool, &'static str> {
    if !orphaned(doc, definition).contains(&action) {
        return Err("Select an action that has no compatible parameter association.");
    }
    let (_, native, kind, _) = actions(doc, definition)
        .into_iter()
        .find(|(h, _, _, _)| *h == action)
        .ok_or("The action is no longer available.")?;
    let action_id = native.element.eval.node_id;
    let p = parameters(doc, definition, kind)
        .into_iter()
        .find(|p| p.handle == parameter && compatible(p, kind))
        .ok_or("Select a compatible parameter for this action.")?;
    if key_point.is_some() && !matches!(kind, Kind::Move | Kind::Stretch | Kind::PolarStretch) {
        return Err("This action does not use a parameter key point.");
    }
    if !p.xy && matches!(key_point, Some(XyKeyPoint::X | XyKeyPoint::Y)) {
        return Err("X and Y key points require an XY parameter.");
    }
    let graph = authoring::graph(doc, definition)?.ok_or("The authoring graph is unavailable.")?;
    let mut staged = doc.clone();
    let Some(ObjectType::DynamicBlock(object)) = staged.objects.get_mut(&action) else {
        return Err("The action is no longer available.");
    };
    let connection = |name: &str| BlockConnection {
        code: p.id,
        name: name.into(),
    };
    match &mut object.data {
        DynamicBlockData::MoveAction(native) => {
            native.connections = move_connections(&p, key_point)
        }
        DynamicBlockData::StretchAction(native) => {
            native.connections = move_connections(&p, key_point);
            native.handles =
                stretch::bind_existing(doc, &native.action.dependencies, &native.points, p.end.z)?;
        }
        DynamicBlockData::PolarStretchAction(native) => {
            let key_point = key_point.or_else(|| stretch::polar_key_point(native).ok());
            native.connections = polar_connections(&p, key_point);
            let stretching = native.action.dependencies.iter().copied().filter(|handle| !native.handles.contains(handle)).collect::<Vec<_>>();
            native.bindings =
                stretch::bind_existing(doc, &stretching, &native.points, p.end.z)?;
        }
        DynamicBlockData::ArrayAction(native) => {
            native.connections = ["Base", "End", "UpdatedBase", "UpdatedEnd"].map(connection)
        }
        DynamicBlockData::FlipAction(native) => {
            native.connections =
                ["Flip", "UpdatedFlip", "UpdatedBase", "UpdatedEnd"].map(connection)
        }
        DynamicBlockData::RotateAction(native) => {
            native.connections = vec![connection("AngleDelta")];
            native.action.connections = ["UpdatedBaseX", "UpdatedBaseY"].map(connection).to_vec();
        }
        DynamicBlockData::ScaleAction(native) => {
            native.connections = ["Scale", "XScale", "YScale"].map(connection).to_vec();
            native.action.connections = ["UpdatedBaseX", "UpdatedBaseY"].map(connection).to_vec();
        }
        _ => return Err("Select a supported native action."),
    }
    authoring::graph::bind_ids(&mut staged, graph, action_id, &[p.id])?;
    if staged == *doc {
        return Ok(false);
    }
    *doc = staged;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{
        block_distance_parameter as distance, block_orientation_parameter as orientation, Scene,
    };
    use codec::{DwgReader, DwgWriter, DxfReader, DxfWriter};
    use std::io::Cursor;

    fn parameter(doc: &mut Document, definition: Handle, kind: Kind) -> Handle {
        if matches!(kind, Kind::Rotate | Kind::Flip) {
            orientation::create(
                doc,
                definition,
                if kind == Kind::Rotate {
                    orientation::Kind::Rotation
                } else {
                    orientation::Kind::Flip
                },
                Vector3::ZERO,
                Vector3::new(10.0, 0.0, 0.0),
                Vector3::new(10.0, 0.0, 0.0),
            )
            .unwrap()
        } else {
            distance::create(
                doc,
                definition,
                if kind == Kind::PolarStretch {
                    distance::Kind::Polar
                } else {
                    distance::Kind::Linear
                },
                Vector3::ZERO,
                Vector3::new(10.0, 0.0, 0.0),
                Vector3::new(5.0, 2.0, 0.0),
            )
            .unwrap()
        }
    }

    #[test]
    fn block_action_association_reconnects_all_action_kinds_and_roundtrips_native_graphs() {
        for kind in [
            Kind::Move,
            Kind::Stretch,
            Kind::PolarStretch,
            Kind::Rotate,
            Kind::Scale,
            Kind::Flip,
            Kind::Array,
        ] {
            let mut scene = Scene::new();
            scene.define_block_raw(
                "ASSOCIATE",
                Vector3::ZERO,
                vec![EntityType::Line(codec::entities::Line::from_points(
                    Vector3::new(1.0, 1.0, 0.0),
                    Vector3::new(2.0, 2.0, 0.0),
                ))],
            );
            let definition = scene
                .document
                .block_records
                .get("ASSOCIATE")
                .unwrap()
                .handle;
            let target = scene
                .document
                .block_records
                .get("ASSOCIATE")
                .unwrap()
                .entity_handles[0];
            let mut doc = scene.document;
            let original = parameter(&mut doc, definition, kind);
            let replacement = parameter(&mut doc, definition, kind);
            let action = create_with_options(
                &mut doc,
                definition,
                kind,
                original,
                None,
                &[target],
                DVec3::new(4.0, 4.0, 0.0),
                needs_frame(kind).then_some([DVec3::ZERO, DVec3::new(5.0, 5.0, 0.0)]),
                (kind == Kind::Array).then_some((2.0, 0.0)),
            )
            .unwrap();
            let Some(ObjectType::DynamicBlock(object)) = doc.objects.get_mut(&action) else {
                panic!()
            };
            match &mut object.data {
                DynamicBlockData::MoveAction(a) => {
                    a.connections.iter_mut().for_each(|c| c.code = -1)
                }
                DynamicBlockData::StretchAction(a) => {
                    a.connections.iter_mut().for_each(|c| c.code = -1)
                }
                DynamicBlockData::PolarStretchAction(a) => {
                    a.connections.iter_mut().for_each(|c| c.code = -1)
                }
                DynamicBlockData::ArrayAction(a) => {
                    a.connections.iter_mut().for_each(|c| c.code = -1)
                }
                DynamicBlockData::FlipAction(a) => {
                    a.connections.iter_mut().for_each(|c| c.code = -1)
                }
                DynamicBlockData::RotateAction(a) | DynamicBlockData::ScaleAction(a) => {
                    a.connections.iter_mut().for_each(|c| c.code = -1)
                }
                _ => panic!(),
            }
            assert_eq!(orphaned(&doc, definition), vec![action]);
            let before = doc.clone();
            assert!(associate(&mut doc, definition, action, Handle::NULL, None).is_err());
            assert_eq!(doc, before);
            assert!(associate(&mut doc, definition, action, replacement, None).unwrap());
            assert!(orphaned(&doc, definition).is_empty());
            let (_, native, _, id) = actions(&doc, definition)
                .into_iter()
                .find(|(h, _, _, _)| *h == action)
                .unwrap();
            let new_id = all_parameters(&doc, definition)
                .iter()
                .find(|p| p.handle == replacement)
                .unwrap()
                .id;
            assert_eq!(id, new_id);
            assert_eq!(native.dependencies, vec![target]);
            assert_eq!(native.display_location, Vector3::new(4.0, 4.0, 0.0));
            let graph = authoring::graph(&doc, definition).unwrap().unwrap();
            let ObjectType::DynamicBlock(object) = &doc.objects[&graph] else {
                panic!()
            };
            let DynamicBlockData::EvaluationGraph(g) = &object.data else {
                panic!()
            };
            assert!(g
                .nodes
                .iter()
                .all(|node| authoring::graph::valid_lists(g, node.id)));
            for saved in [
                DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&doc).unwrap()))
                    .read()
                    .unwrap(),
                DxfReader::from_reader(Cursor::new(DxfWriter::new(&doc).write_to_vec().unwrap()))
                    .unwrap()
                    .read()
                    .unwrap(),
            ] {
                assert_eq!(saved.objects[&action], doc.objects[&action]);
                assert_eq!(saved.objects[&graph], doc.objects[&graph]);
            }
            let baseline = doc.clone();
            let value = match kind {
                Kind::Flip => Value::Flip(true),
                Kind::Rotate => Value::Point(DVec3::new(0.0, 10.0, 0.0)),
                _ => Value::Point(DVec3::new(20.0, 0.0, 0.0)),
            };
            evaluate_value(&mut doc, &baseline, definition, replacement, value).unwrap();
            let old = all_parameters(&doc, definition)
                .into_iter()
                .find(|p| p.handle == original)
                .unwrap();
            assert_eq!(
                super::super::value(&doc, &old).unwrap(),
                if kind == Kind::Flip {
                    Value::Flip(false)
                } else {
                    Value::Point(old.end)
                }
            );
            let before = doc.clone();
            assert!(associate(&mut doc, definition, action, original, None).is_err());
            assert_eq!(doc, before);
        }
    }
}
