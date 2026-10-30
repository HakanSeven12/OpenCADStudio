//! Native grip metadata shared by the editor and insertion commands.
pub(crate) mod config;
pub(crate) mod runtime;
use codec::{
    objects::{BlockGrip, DynamicBlockData, ObjectType},
    types::Vector3,
    Document, Handle,
};

pub(crate) struct GripInfo {
    pub handle: Handle,
    pub name: String,
    pub parameter_type: &'static str,
    pub location: String,
    pub point: Vector3,
    pub enabled: bool,
    pub weight: i32,
}

fn grip(data: &DynamicBlockData) -> Option<&BlockGrip> {
    match data {
        DynamicBlockData::AlignmentGrip(g) | DynamicBlockData::LinearGrip(g) => Some(&g.grip),
        DynamicBlockData::FlipGrip(g) => Some(&g.grip),
        DynamicBlockData::LookupGrip(g)
        | DynamicBlockData::PolarGrip(g)
        | DynamicBlockData::RotationGrip(g)
        | DynamicBlockData::VisibilityGrip(g)
        | DynamicBlockData::XYGrip(g)
        | DynamicBlockData::PropertiesTableGrip(g) => Some(g),
        _ => None,
    }
}

fn grip_mut(data: &mut DynamicBlockData) -> Option<&mut BlockGrip> {
    match data {
        DynamicBlockData::AlignmentGrip(g) | DynamicBlockData::LinearGrip(g) => Some(&mut g.grip),
        DynamicBlockData::FlipGrip(g) => Some(&mut g.grip),
        DynamicBlockData::LookupGrip(g)
        | DynamicBlockData::PolarGrip(g)
        | DynamicBlockData::RotationGrip(g)
        | DynamicBlockData::VisibilityGrip(g)
        | DynamicBlockData::XYGrip(g)
        | DynamicBlockData::PropertiesTableGrip(g) => Some(g),
        _ => None,
    }
}

pub(crate) fn grips(doc: &Document, definition: Handle) -> Result<Vec<GripInfo>, &'static str> {
    let Some(graph) = super::block_authoring::graph(doc, definition)? else {
        return Ok(vec![]);
    };
    let Some(ObjectType::DynamicBlock(object)) = doc.objects.get(&graph) else {
        return Err("The authoring graph is unavailable.");
    };
    let DynamicBlockData::EvaluationGraph(g) = &object.data else {
        return Err("The authoring graph is unavailable.");
    };
    let parameters = super::block_actions::all_parameters(doc, definition);
    let mut result = g
        .nodes
        .iter()
        .filter_map(|node| {
            let ObjectType::DynamicBlock(object) = doc.objects.get(&node.expression)? else {
                return None;
            };
            if object.owner != graph {
                return None;
            }
            let grip = grip(&object.data)?;
            let parameter = parameters
                .iter()
                .find(|parameter| parameter.grip == object.handle);
            let name = parameter
                .map(|p| p.name.clone())
                .unwrap_or_else(|| grip.element.name.clone());
            let parameter_type = match &object.data {
                DynamicBlockData::AlignmentGrip(_) => "Alignment",
                DynamicBlockData::LinearGrip(_) => "Linear",
                DynamicBlockData::FlipGrip(_) => "Flip",
                DynamicBlockData::LookupGrip(_) => "Lookup",
                DynamicBlockData::PolarGrip(_) => "Polar",
                DynamicBlockData::RotationGrip(_) => "Rotation",
                DynamicBlockData::VisibilityGrip(_) => "Visibility",
                DynamicBlockData::PropertiesTableGrip(_) => "Block Properties Table",
                _ if parameter.is_some_and(|p| p.point) => "Point",
                _ => "XY",
            };
            Some(GripInfo {
                handle: object.handle,
                name,
                parameter_type,
                location: grip.element.name.clone(),
                point: grip.location,
                enabled: grip.insert_cycling,
                weight: grip.insert_cycling_weight,
            })
        })
        .collect::<Vec<_>>();
    result.sort_by_key(|g| {
        (
            !g.enabled,
            if g.enabled { g.weight } else { i32::MAX },
            g.handle.value(),
        )
    });
    Ok(result)
}

pub(crate) fn set_cycle_order(
    doc: &mut Document,
    definition: Handle,
    rows: &[(Handle, bool)],
) -> Result<bool, &'static str> {
    let native = grips(doc, definition)?;
    let mut actual = native.iter().map(|g| g.handle).collect::<Vec<_>>();
    actual.sort();
    let mut supplied = rows.iter().map(|(handle, _)| *handle).collect::<Vec<_>>();
    supplied.sort();
    if actual != supplied || supplied.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err("The grip list changed; reopen BCYCLEORDER.");
    }
    let original = native
        .iter()
        .filter(|g| g.enabled)
        .map(|g| g.handle)
        .collect::<Vec<_>>();
    let ordered = rows
        .iter()
        .filter(|(_, enabled)| *enabled)
        .map(|(handle, _)| *handle)
        .collect::<Vec<_>>();
    if original == ordered {
        return Ok(false);
    }
    let mut staged = doc.clone();
    let mut weight = 0;
    for (handle, enabled) in rows {
        let Some(ObjectType::DynamicBlock(object)) = staged.objects.get_mut(handle) else {
            return Err("A native grip is unavailable.");
        };
        let native = grip_mut(&mut object.data).ok_or("A native grip is unavailable.")?;
        native.insert_cycling = *enabled;
        native.insert_cycling_weight = if *enabled {
            let current = weight;
            weight += 1;
            current
        } else {
            -1
        };
    }
    *doc = staged;
    Ok(true)
}

pub(crate) fn insertion_points(doc: &Document, definition: Handle) -> Vec<Vector3> {
    grips(doc, definition)
        .unwrap_or_default()
        .into_iter()
        .filter(|g| g.enabled)
        .map(|g| g.point)
        .filter(|point| point.x.is_finite() && point.y.is_finite() && point.z.is_finite())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{block_distance_parameter as distance, Scene};
    use codec::{DwgReader, DwgWriter, DxfReader, DxfWriter};
    use std::io::Cursor;

    #[test]
    fn draw_order_table_does_not_drop_the_dynamic_graph() {
        let mut scene = Scene::new();
        scene.define_block_raw("SORTED", Vector3::ZERO, vec![]);
        scene.define_block_raw("COPY", Vector3::ZERO, vec![]);
        let doc = &mut scene.document;
        let definition = doc.block_records.get("SORTED").unwrap().handle;
        let (start, end) = (Vector3::ZERO, Vector3::new(10.0, 0.0, 0.0));
        let label = Vector3::new(5.0, 2.0, 0.0);
        distance::create(doc, definition, distance::Kind::Linear, start, end, label).unwrap();
        let xdict = doc.extension_dictionary_handle(definition).unwrap();
        let mut table = codec::objects::SortEntitiesTable::for_block(definition);
        table.handle = doc.allocate_handle();
        table.owner_handle = xdict;
        let sort = table.handle;
        doc.objects.insert(sort, ObjectType::SortEntitiesTable(table));
        if let Some(ObjectType::Dictionary(d)) = doc.objects.get_mut(&xdict) {
            d.add_entry("ACAD_SORTENTS", sort);
        }
        assert!(doc.export_block_graph(definition).is_none());
        let graph = crate::scene::block_authoring::export_graph(doc, definition).unwrap();
        let copy = doc.block_records.get("COPY").unwrap().handle;
        doc.import_block_graph(&graph, copy, &Default::default());
        assert_eq!(grips(doc, copy).unwrap().len(), 1);
    }

    #[test]
    fn block_grip_cycling_preserves_native_order_through_dwg_and_dxf() {
        let mut scene = Scene::new();
        scene.define_block_raw("CYCLE", Vector3::ZERO, vec![]);
        let definition = scene.document.block_records.get("CYCLE").unwrap().handle;
        for x in [10.0, 20.0, 30.0] {
            distance::create(
                &mut scene.document,
                definition,
                distance::Kind::Linear,
                Vector3::ZERO,
                Vector3::new(x, 0.0, 0.0),
                Vector3::new(x / 2.0, 2.0, 0.0),
            )
            .unwrap();
        }
        let handles = grips(&scene.document, definition)
            .unwrap()
            .into_iter()
            .map(|g| g.handle)
            .collect::<Vec<_>>();
        let rows = vec![(handles[2], true), (handles[0], false), (handles[1], true)];
        let before = scene.document.clone();
        assert!(set_cycle_order(&mut scene.document, definition, &rows[..2]).is_err());
        assert_eq!(scene.document, before);
        assert!(set_cycle_order(&mut scene.document, definition, &rows).unwrap());
        let expected = vec![Vector3::new(30.0, 0.0, 0.0), Vector3::new(20.0, 0.0, 0.0)];
        assert_eq!(insertion_points(&scene.document, definition), expected);
        assert!(!set_cycle_order(&mut scene.document, definition, &rows).unwrap());
        for doc in [
            DwgReader::from_stream(Cursor::new(
                DwgWriter::write_to_vec(&scene.document).unwrap(),
            ))
            .read()
            .unwrap(),
            DxfReader::from_reader(Cursor::new(
                DxfWriter::new(&scene.document).write_to_vec().unwrap(),
            ))
            .unwrap()
            .read()
            .unwrap(),
        ] {
            assert_eq!(insertion_points(&doc, definition), expected);
            for handle in &handles {
                assert_eq!(doc.objects[handle], scene.document.objects[handle]);
            }
        }
        let rows = handles
            .iter()
            .map(|handle| (*handle, false))
            .collect::<Vec<_>>();
        assert!(set_cycle_order(&mut scene.document, definition, &rows).unwrap());
        assert!(insertion_points(&scene.document, definition).is_empty());
    }
}
