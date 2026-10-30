//! The native authoring anchor, separate from the BLOCK definition origin.
use super::block_authoring::{self as authoring, *};
use codec::objects::{
    BlockBasePointParameter, BlockElement, BlockEvalValue, BlockOnePointParameter, BlockParameter,
    DynamicBlockData, ObjectType,
};
use codec::{
    types::{Transform, Vector3},
    Document, Handle,
};

pub(crate) fn parameter(
    document: &Document,
    definition: Handle,
) -> Option<(Handle, &BlockBasePointParameter)> {
    let ObjectType::Dictionary(dictionary) = document
        .objects
        .get(&authoring::definition_dictionary(document, definition)?)?
    else {
        return None;
    };
    let graph = dictionary.get("ACAD_ENHANCEDBLOCK")?;
    let ObjectType::DynamicBlock(object) = document.objects.get(&graph)? else {
        return None;
    };
    let DynamicBlockData::EvaluationGraph(data) = &object.data else {
        return None;
    };
    data.nodes
        .iter()
        .find_map(|node| match document.objects.get(&node.expression) {
            Some(ObjectType::DynamicBlock(object)) if object.owner == graph => match &object.data {
                DynamicBlockData::BasePointParameter(p) => Some((object.handle, p)),
                _ => None,
            },
            _ => None,
        })
}
pub(crate) fn placement_error(document: &Document, definition: Handle) -> Option<&'static str> {
    authoring::graph(document, definition).err()
}
pub(crate) fn create(
    document: &mut Document,
    definition: Handle,
    location: Vector3,
) -> Result<Handle, &'static str> {
    if ![location.x, location.y, location.z]
        .iter()
        .all(|v| v.is_finite())
    {
        return Err("Specify a finite parameter location.");
    }
    authoring::graph(document, definition)?;
    let name = document
        .block_records
        .iter()
        .find(|b| b.handle == definition)
        .unwrap()
        .name
        .clone();
    let old_base = super::render_graph::block_base_point(document, &name);
    if let Some((handle, _)) = parameter(document, definition) {
        let ObjectType::DynamicBlock(object) = document.objects.get_mut(&handle).unwrap() else {
            unreachable!()
        };
        let DynamicBlockData::BasePointParameter(p) = &mut object.data else {
            unreachable!()
        };
        p.parameter.definition_point = location;
        p.point = location;
        p.base_point = location;
        move_attributes(document, &name, old_base, location);
        return Ok(handle);
    }
    let (graph, slots) = authoring::prepare(document, definition)?;
    register_classes(
        document,
        &[("BLOCKBASEPOINTPARAMETER", "AcDbBlockBasepointParameter", 55)],
    );
    let handle = document.allocate_handle();
    let native = BlockBasePointParameter {
        parameter: BlockOnePointParameter {
            parameter: BlockParameter {
                element: BlockElement {
                    eval: expression(slots.eval, NO_VALUE, BlockEvalValue::None),
                    name: "Base Point".into(),
                    major: MAJOR,
                    minor: MINOR,
                    eed_1071: 0,
                },
                show_properties: true,
                chain_actions: false,
            },
            definition_point: location,
            ..Default::default()
        },
        point: location,
        base_point: location,
    };
    document.objects.insert(
        handle,
        ObjectType::DynamicBlock(dynamic(
            handle,
            graph,
            "BLOCKBASEPOINTPARAMETER",
            "AcDbBlockBasepointParameter",
            DynamicBlockData::BasePointParameter(native),
        )),
    );
    authoring::append_single(document, graph, slots, handle);
    move_attributes(document, &name, old_base, location);
    Ok(handle)
}

fn move_attributes(document: &mut Document, name: &str, old: Vector3, new: Vector3) {
    let handles = document
        .entities()
        .filter_map(|entity| match entity {
            codec::EntityType::Insert(i)
                if i.block_name.eq_ignore_ascii_case(name) && !i.attributes.is_empty() =>
            {
                Some(i.common.handle)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    for handle in handles {
        if let Some(codec::EntityType::Insert(insert)) = document.get_entity_mut(handle) {
            let shift = insert.get_transform().apply_rotation(old - new);
            let transform = Transform::from_translation(shift);
            for attribute in &mut insert.attributes {
                codec::Entity::apply_transform(attribute, &transform);
            }
        }
    }
}
pub(crate) fn reframe(
    document: &mut Document,
    definition: Handle,
    transform: &Transform,
) -> usize {
    let Some((handle, _)) = parameter(document, definition) else {
        return 0;
    };
    if let Some(ObjectType::DynamicBlock(object)) = document.objects.get_mut(&handle) {
        if let DynamicBlockData::BasePointParameter(p) = &mut object.data {
            p.parameter.definition_point = transform.apply(p.parameter.definition_point);
            p.point = transform.apply(p.point);
            p.base_point = transform.apply(p.base_point);
            return 1;
        }
    }
    0
}
